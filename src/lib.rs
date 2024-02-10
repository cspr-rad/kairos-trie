#![cfg_attr(not(feature = "std"), no_std)]
#[cfg(not(feature = "std"))]
extern crate core as std;

extern crate alloc;

pub mod modified;
pub mod stored;

use alloc::{string::String, vec::Vec};
pub use modified::*;
pub use stored::{Ref, Store};

use bitvec::prelude::*;
use sha2::{Digest, Sha256};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct KeyHash(pub [u8; 32]);

/// TODO: Switch to usize for more efficient bit indexing.
type HashedKeyBits = BitSlice<u8, Lsb0>;

fn hash_key(key: &[u8]) -> KeyHash {
    let mut hasher = Sha256::new();
    hasher.update(key);
    KeyHash(hasher.finalize().into())
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Branch<NodeRef> {
    pub bit_idx: u8,
    pub left: NodeRef,
    pub right: NodeRef,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Leaf {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum TrieRoot<SBR: Ref, SLR: Ref> {
    #[default]
    Empty,
    Node(NodeRef<SBR, SLR>),
}

pub struct Transaction<S: Store> {
    pub data_store: S,
    pub witness_branches: Vec<(S::BranchRef, Branch<stored::NodeRef<S>>)>,
    pub witness_leaves: Vec<(KeyHash, Leaf)>,

    pub current_root: TrieRoot<S::BranchRef, S::LeafRef>,
    pub modified_branches: Branches<S::BranchRef, S::LeafRef>,
    pub modified_leaves: Leaves,
}

impl<S: Store> Transaction<S> {
    pub fn new(root: TrieRoot<S::BranchRef, S::LeafRef>, data_store: S) -> Self {
        Transaction {
            data_store,
            witness_branches: Vec::new(),
            witness_leaves: Vec::new(),
            current_root: root,
            modified_branches: Branches::default(),
            modified_leaves: Leaves::default(),
        }
    }

    pub fn get(&mut self, key: &[u8]) -> Result<Option<&Leaf>, String> {
        self.get_hashed(key, &hash_key(key))
    }

    pub fn get_hashed(&mut self, key: &[u8], key_hash: &KeyHash) -> Result<Option<&Leaf>, String> {
        match self.current_root {
            TrieRoot::Empty => Ok(None),
            TrieRoot::Node(NodeRef::ModLeaf(leaf_idx)) => {
                let (leaf_key_hash, leaf) = &self.modified_leaves[leaf_idx];
                if leaf.key == key {
                    Ok(Some(leaf))
                } else if *key_hash != *leaf_key_hash {
                    Err("Provided key an Hash do not match hash".into())
                } else {
                    Ok(None)
                }
            }
            TrieRoot::Node(NodeRef::ModBranch(branch_idx)) => {
                self.get_modified(branch_idx, key, key_hash)
            }
            TrieRoot::Node(NodeRef::StoredBranch(br)) => {
                todo!();
            }
            TrieRoot::Node(NodeRef::StoredLeaf(lr)) => {
                todo!();
            }
        }
    }

    fn get_modified(
        &self,
        branch_idx: BranchIdx,
        key: &[u8],
        key_hash: &KeyHash,
    ) -> Result<Option<&Leaf>, String> {
        let mut branch = &self.modified_branches[branch_idx];
        loop {
            let bits = HashedKeyBits::from_slice(key_hash.0.as_ref());
            let next = if bits[branch.bit_idx as usize] {
                branch.right
            } else {
                branch.left
            };

            match next {
                NodeRef::ModBranch(branch_idx) => {
                    branch = &self.modified_branches[branch_idx];
                }

                NodeRef::ModLeaf(leaf_idx) => {
                    let (leaf_hash, leaf) = &self.modified_leaves[leaf_idx];
                    if leaf.key == key {
                        return Ok(Some(leaf));
                    } else if *key_hash != *leaf_hash {
                        return Err("Provided key an Hash do not match hash".into());
                    } else {
                        return Ok(None);
                    }
                }
                NodeRef::StoredBranch(node_hash) => {
                    // return self.get_stored_hashed(node_hash, key, key_hash)
                    todo!();
                }
                NodeRef::StoredLeaf(node_hash) => {
                    // return self.get_stored_hashed(node_hash, key, key_hash)
                    todo!();
                }
            };
        }
    }

    pub fn get_stored_branch(
        &self,
        mut branch_ref: S::BranchRef,
        key: &[u8],
        hash: &KeyHash,
    ) -> Result<Option<&Leaf>, String> {
        loop {
            let Branch {
                bit_idx,
                left,
                right,
            } = self.data_store.get_branch(branch_ref)?;

            let bit_slice = HashedKeyBits::from_slice(hash.0.as_ref());
            let node_ref = if bit_slice[bit_idx as usize] {
                right
            } else {
                left
            };


            match node_ref {
                stored::Node::Branch(br) => {
                    branch_ref = br;
                }
                stored::Node::Leaf(lr) => {
                    return self.get_stored_leaf(lr, key, hash);
                }
            }

        }
    }

    pub fn get_stored_leaf(
        &self,
        leaf: S::LeafRef,
        key: &[u8],
        hash: &KeyHash,
    ) -> Result<Option<&Leaf>, String> {
        let leaf = self.data_store.get_leaf(leaf)?;
        if leaf.key == key {
            Ok(Some(leaf))
        } else if *hash != hash_key(&leaf.key) {
            Err("Provided key an Hash do not match hash".into())
        } else {
            Ok(None)
        }
    }

    /// Insert a new key value pair into the trie.
    /// Returns the previous value if it existed.
    // pub fn insert(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<Option<Leaf>, String> {
    //     let new_key_hash = hash_key(key.as_slice());

    //     match self.current_root {
    //         TrieRoot::ModifiedNode(node_idx) => {
    //             // we init the prior node to a leaf with a dummy index
    //             let mut node_idx = BranchIdx(node_idx.0);
    //             let mut prior_bit_idx = 0;
    //             loop {
    //                 let node = self.modified_branches[node_idx.0 as usize];
    //                 match node {
    //                     ModifiedNode::BranchBoth {
    //                         bit_idx,
    //                         left,
    //                         right,
    //                     } => {
    //                         let bit_slice = HashedKeyBits::from_slice(new_key_hash.0.as_ref());
    //                         let next_node_idx = if bit_slice[bit_idx as usize] {
    //                             right
    //                         } else {
    //                             left
    //                         };

    //                         node_idx = next_node_idx;
    //                         prior_bit_idx = bit_idx;
    //                     }
    //                     ModifiedNode::BranchLeft {
    //                         bit_idx,
    //                         left,
    //                         right,
    //                     } => {
    //                         let bit_slice = HashedKeyBits::from_slice(new_key_hash.0.as_ref());
    //                         if bit_slice[bit_idx as usize] {
    //                             todo!();
    //                         } else {
    //                             node_idx = left;
    //                             prior_bit_idx = bit_idx;
    //                         }
    //                     }
    //                     ModifiedNode::BranchRight {
    //                         bit_idx,
    //                         left,
    //                         right,
    //                     } => {
    //                         let bit_slice = HashedKeyBits::from_slice(new_key_hash.0.as_ref());
    //                         if bit_slice[bit_idx as usize] {
    //                             node_idx = right;
    //                             prior_bit_idx = bit_idx;
    //                         } else {
    //                             todo!();
    //                         }
    //                     }
    //                     ModifiedNode::Leaf(old_leaf_idx) => {
    //                         let (old_key_hash, old_leaf) =
    //                             &mut self.modified_leaves[old_leaf_idx.0 as usize];

    //                         if key.as_slice() == old_leaf.key.as_slice() {
    //                             let value = std::mem::replace(&mut old_leaf.value, value);
    //                             return Ok(Some(Leaf { key, value }));
    //                         } else {
    //                             let prior_bit_idx = prior_bit_idx as usize;
    //                             let old_bits =
    //                                 &HashedKeyBits::from_slice(&old_key_hash.0)[prior_bit_idx..];
    //                             let new_bits =
    //                                 &HashedKeyBits::from_slice(&new_key_hash.0)[prior_bit_idx..];

    //                             // This can be optimized by applying word wise comparison first
    //                             let Some((bit_idx, new_bit)) = iter::zip(old_bits, new_bits)
    //                                 .enumerate()
    //                                 .find(|(_, (a, b))| a != b)
    //                                 .map(|(idx, (_, b))| (idx, *b))
    //                             else {
    //                                 return Err(
    //                                     "The hashes are equal, but the keys are not".to_string()
    //                                 );
    //                             };

    //                             let bit_idx = (prior_bit_idx + bit_idx) as u8;

    //                             let moved_old_leaf_node_idx: BranchIdx =
    //                                 self.modified_branches.0.len().into();
    //                             self.modified_branches
    //                                 .0
    //                                 .push(ModifiedNode::Leaf(old_leaf_idx));

    //                             let new_leaf_idx: LeafIdx = self.modified_leaves.len().into();
    //                             self.modified_leaves
    //                                 .push((new_key_hash, Leaf { key, value }));
    //                             let new_leaf_node_idx: BranchIdx =
    //                                 self.modified_branches.0.len().into();
    //                             self.modified_branches
    //                                 .push(ModifiedNode::Leaf(new_leaf_idx));

    //                             let (left, right) = if new_bit {
    //                                 (moved_old_leaf_node_idx, new_leaf_node_idx)
    //                             } else {
    //                                 (new_leaf_node_idx, moved_old_leaf_node_idx)
    //                             };

    //                             self.modified_branches[node_idx.0 as usize] =
    //                                 ModifiedNode::BranchBoth {
    //                                     bit_idx,
    //                                     left,
    //                                     right,
    //                                 };
    //                         }
    //                         return Ok(None);
    //                     }
    //                 }
    //             }
    //         }
    //         TrieRoot::StoredNode(ref node_hash) => {
    //             // self.insert_stored(node_hash, hash,);
    //             todo!();
    //         }
    //         TrieRoot::Empty => {
    //             todo!("Insert into empty trie");
    //             // self.modified_leaves
    //             //     .push((new_key_hash, Leaf { key, value }));
    //             // let leaf_idx = LeafIdx((self.modified_leaves.len() - 1) as u32);
    //             // self.modified_branches.push(ModifiedNode::Leaf(leaf_idx));
    //             // self.current_root = TrieRoot::ModifiedNode(BranchIdx(0));

    //             Ok(None)
    //         }
    //     }
    // }

    fn insert_stored(
        &mut self,
        node_hash: stored::NodeRef<S>,
        key_hash: &KeyHash,
        key: Vec<u8>,
        value: Vec<u8>,
    ) -> Result<Option<Leaf>, String> {
        let init_modified_node_count = self.modified_branches.0.len();
        let mut node_hash = node_hash;

        // loop {
        //     let node = self.data_store.get(node_hash).ok_or("Node not found")?;
        //     self.stored_nodes.push((node_hash.clone(), node.clone()));

        //     match node {
        //         StoredNode::Branch {
        //             bit_idx,
        //             left,
        //             right,
        //         } => {
        //             let bit_slice = HashedKeyBits::from_slice(&key_hash.0);
        //             node_hash = if bit_slice[*bit_idx as usize] {
        //                 self.modified_branches.push(ModifiedNode::BranchRight {
        //                     bit_idx: *bit_idx,
        //                     left: *left,
        //                     right: BranchIdx(self.modified_branches.len() as u32),
        //                 });
        //                 right
        //             } else {
        //                 left
        //             };
        //         }

        //         StoredNode::Leaf(leaf) => {
        //             todo!();
        //         }
        //     }
        // }

        todo!();
    }
}

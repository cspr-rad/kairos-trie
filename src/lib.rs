#![cfg_attr(not(feature = "std"), no_std)]
#[cfg(not(feature = "std"))]
extern crate core as std;

extern crate alloc;

pub mod modified;
pub mod stored;

use std::iter;

use alloc::{boxed::Box, string::String, vec::Vec};
pub use modified::*;
pub use stored::{Ref, Store};

use bitvec::prelude::*;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct KeyHash(pub [u8; 32]);

/// TODO: Switch to usize for more efficient bit indexing.
type HashedKeyBits = BitSlice<u8, Lsb0>;

fn hash_key(key: &[u8]) -> KeyHash {
    let mut hasher = Sha256::new();
    hasher.update(key);
    KeyHash(hasher.finalize().into())
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Branch<NR> {
    pub bit_idx: u8,
    pub left: NR,
    pub right: NR,
}

impl<SBR, SER, SLR> Branch<NodeRef<SBR, SER, SLR>> {
    pub fn from_hashed_key_bits(
        prior_bit_idx: u8,
        a_leaf_idx: NodeRef<SBR, SER, SLR>,
        a_hash: &KeyHash,
        b_hash: &KeyHash,
        b_leaf_idx: NodeRef<SBR, SER, SLR>,
    ) -> Self {
        let a_bits = &HashedKeyBits::from_slice(&a_hash.0)[prior_bit_idx as usize..];
        let b_bits = &HashedKeyBits::from_slice(&b_hash.0)[prior_bit_idx as usize..];

        iter::zip(a_bits, b_bits)
            .enumerate()
            .find(|(_, (a, b))| a != b)
            .map(|(bit_idx, (a_bit, _))| {
                let (left, right) = if *a_bit {
                    (b_leaf_idx, a_leaf_idx)
                } else {
                    (a_leaf_idx, b_leaf_idx)
                };

                Branch {
                    bit_idx: prior_bit_idx + bit_idx as u8,
                    left,
                    right,
                }
            })
            .unwrap_or_else(|| {
                // The hashes are equal, but the keys are not
                // TODO: handle this case
                panic!("The hashes are equal, but the keys are not");
            })
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Extension<SBR, SER, SLR> {
    next: NodeRef<SBR, SER, SLR>,
    bits: [u8],
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Leaf {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum TrieRoot<SBR, SER, SLR> {
    #[default]
    Empty,
    Node(NodeRef<SBR, SER, SLR>),
}

pub struct Transaction<S: Store> {
    pub data_store: S,

    // TODO: break witness into a separate struct
    pub witness_branches: Vec<(S::BranchRef, Branch<stored::NodeRef<S>>)>,
    pub witness_leaves: Vec<(KeyHash, Leaf)>,

    pub current_root: TrieRoot<S::BranchRef, S::ExtensionRef, S::LeafRef>,
    pub modified_branches: Branches<S::BranchRef, S::ExtensionRef, S::LeafRef>,
    pub modified_extensions: Vec<Box<Extension<S::BranchRef, S::ExtensionRef, S::LeafRef>>>,
    pub modified_leaves: Leaves,
}

type NodeRefTxn<S> =
    NodeRef<<S as Store>::BranchRef, <S as Store>::ExtensionRef, <S as Store>::LeafRef>;

impl<S: Store> Transaction<S> {
    pub fn new(root: TrieRoot<S::BranchRef, S::ExtensionRef, S::LeafRef>, data_store: S) -> Self {
        Transaction {
            data_store,
            witness_branches: Vec::new(),
            witness_leaves: Vec::new(),
            current_root: root,
            modified_branches: Branches::default(),
            modified_extensions: Vec::new(),
            modified_leaves: Leaves::default(),
        }
    }

    pub fn get(&mut self, key: &[u8]) -> Result<Option<&Leaf>, String> {
        self.get_hashed(key, &hash_key(key))
    }

    fn get_hashed(&mut self, key: &[u8], key_hash: &KeyHash) -> Result<Option<&Leaf>, String> {
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
                return self.get_stored_branch(br, key, key_hash);
            }
            TrieRoot::Node(NodeRef::StoredLeaf(lr)) => {
                return self.get_stored_leaf(lr, key, key_hash);
            }
            TrieRoot::Node(NodeRef::StoredExtension(_)) => todo!(),
            TrieRoot::Node(NodeRef::ModExtension(_)) => todo!(),
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
                NodeRef::ModExtension(_) => todo!(),

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
                NodeRef::StoredBranch(br) => {
                    return self.get_stored_branch(br, key, key_hash);
                }
                NodeRef::StoredLeaf(lr) => {
                    return self.get_stored_leaf(lr, key, key_hash);
                }
                NodeRef::StoredExtension(_) => todo!(),
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
                stored::Node::Extension(_) => todo!(),
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
    pub fn insert(&mut self, key: &[u8], value: &[u8]) -> Result<(), String> {
        let key_hash = hash_key(key);
        self.insert_hashed(key, value, key_hash)
    }

    fn insert_hashed(&mut self, key: &[u8], value: &[u8], key_hash: KeyHash) -> Result<(), String> {
        match self.current_root {
            TrieRoot::Empty => {
                let node = self.modified_leaves.push(
                    key_hash,
                    Leaf {
                        key: key.to_vec(),
                        value: value.to_vec(),
                    },
                );
                self.current_root = TrieRoot::Node(NodeRef::ModLeaf(node));
                Ok(())
            }
            TrieRoot::Node(NodeRef::ModBranch(branch_idx)) => {
                self.insert_modified_branch(branch_idx, key, value, key_hash)
            }
            TrieRoot::Node(NodeRef::ModLeaf(old_leaf_idx)) => {
                self.insert_modified_leaf(old_leaf_idx, key, value, key_hash)
            }
            TrieRoot::Node(NodeRef::StoredBranch(branch_ref)) => {
                self.current_root =
                    TrieRoot::Node(self.insert_stored_branch(branch_ref, key, value, &key_hash)?);
                Ok(())
            }
            TrieRoot::Node(NodeRef::StoredLeaf(leaf_ref)) => {
                self.current_root =
                    TrieRoot::Node(self.insert_stored_leaf(leaf_ref, key, value, &key_hash)?);
                Ok(())
            }
            TrieRoot::Node(NodeRef::StoredExtension(_)) => todo!(),
            TrieRoot::Node(NodeRef::ModExtension(_)) => todo!(),
        }
    }

    fn insert_modified_branch(
        &mut self,
        mut branch_idx: BranchIdx,
        key: &[u8],
        value: &[u8],
        key_hash: KeyHash,
    ) -> Result<(), String> {
        // loop {
        //     let branch = &mut self.modified_branches[branch_idx];

        //     let bit_slice = HashedKeyBits::from_slice(&key_hash.0);

        //     let next_mod_ref = |next_node| -> Result<NodeRefTxn<S>, String> {
        //         match next_node {
        //             NodeRef::ModBranch(br) => {
        //                 branch_idx = br;
        //                 Ok(NodeRef::ModBranch(br))
        //             }
        //             NodeRef::ModLeaf(lr)

        // if bit_slice[branch.bit_idx as usize] {
        //     branch_idx = branch.right;
        // } else {
        //     branch_idx = branch.left;
        // }

        // }
        todo!();
    }

    fn insert_modified_leaf(
        &mut self,
        leaf_idx: LeafIdx,
        key: &[u8],
        value: &[u8],
        key_hash: KeyHash,
    ) -> Result<(), String> {
        todo!();
    }

    fn insert_stored_branch(
        &mut self,
        mut branch_ref: S::BranchRef,
        key: &[u8],
        value: &[u8],
        key_hash: &KeyHash,
    ) -> Result<NodeRefTxn<S>, String> {
        loop {
            let branch = self.data_store.get_branch(branch_ref)?;

            let bit_slice = HashedKeyBits::from_slice(&key_hash.0);

            let mut next_mod_ref = |next_node| -> Result<NodeRefTxn<S>, String> {
                match next_node {
                    stored::Node::Branch(br) => {
                        branch_ref = br;
                        Ok(NodeRef::ModBranch(BranchIdx(
                            // refers to the branch that will be inserted in the next iteration
                            self.modified_branches.0.len() as u32 + 1,
                        )))
                    }
                    stored::Node::Extension(_) => todo!(),
                    stored::Node::Leaf(lr) => self.insert_stored_leaf(lr, key, value, key_hash),
                }
            };

            let (left, right) = if bit_slice[branch.bit_idx as usize] {
                (branch.left.into(), next_mod_ref(branch.right)?)
            } else {
                (next_mod_ref(branch.left)?, branch.right.into())
            };

            // the current branch contains a reference to the next branch or leaf
            self.modified_branches.push(Branch {
                bit_idx: branch.bit_idx,
                left,
                right,
            });
        }
    }

    fn insert_stored_leaf(
        &mut self,
        leaf_ref: S::LeafRef,
        key: &[u8],
        value: &[u8],
        key_hash: &KeyHash,
    ) -> Result<NodeRefTxn<S>, String> {
        let leaf = self.data_store.get_leaf(leaf_ref)?;

        let leaf_idx = self.modified_leaves.push(
            *key_hash,
            Leaf {
                key: key.to_vec(),
                value: value.to_vec(),
            },
        );

        if leaf.key == key {
            Ok(NodeRef::ModLeaf(leaf_idx))
        } else {
            let new_branch_idx = self.modified_branches.push(Branch::from_hashed_key_bits(
                0,
                NodeRef::ModLeaf(leaf_idx),
                key_hash,
                &hash_key(&leaf.key),
                NodeRef::StoredLeaf(leaf_ref),
            ));

            Ok(NodeRef::ModBranch(new_branch_idx))
        }
    }
}

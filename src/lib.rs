use std::{collections::HashMap, iter};

use bitvec::prelude::*;
use sha2::{Digest, Sha256};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct HashedKey(pub [u8; 32]);
type HashedKeyBits = BitSlice<u8, Lsb0>;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct HashedNode(pub [u8; 32]);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct StoredIdx(u32);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct ModifiedIdx(u32);

impl From<usize> for ModifiedIdx {
    fn from(idx: usize) -> Self {
        ModifiedIdx(idx as u32)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct LeafIdx(u32);

impl From<usize> for LeafIdx {
    fn from(idx: usize) -> Self {
        LeafIdx(idx as u32)
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum StoredNode {
    Branch {
        bit_idx: u8,
        left: HashedNode,
        right: HashedNode,
    },
    Leaf(Leaf),
}

impl StoredNode {
    pub fn get_hashed(
        &self,
        key: &[u8],
        hash: &HashedKey,
        data_store: &DataStore,
    ) -> Result<Option<Leaf>, String> {
        let mut node = self;
        loop {
            match node {
                StoredNode::Branch {
                    bit_idx,
                    left,
                    right,
                } => {
                    let bit_slice = HashedKeyBits::from_slice(hash.0.as_ref());
                    let next_node_hash = if bit_slice[*bit_idx as usize] {
                        right
                    } else {
                        left
                    };

                    node = data_store.get(next_node_hash).ok_or("Node not found")?;
                }
                StoredNode::Leaf(leaf) => {
                    if leaf.key == key {
                        return Ok(Some(leaf.clone()));
                    } else if *hash != hash_key(leaf.key.as_slice()) {
                        return Err("Provided key an Hash do not match hash".to_string());
                    } else {
                        return Ok(None);
                    }
                }
            }
        }
    }
}

fn hash_key(key: &[u8]) -> HashedKey {
    let mut hasher = Sha256::new();
    hasher.update(key);
    HashedKey(hasher.finalize().into())
}

/// A node that has been modified in a transaction.
///
/// Note: This merged representation is more efficient than nesting an enum `StoredIdx` | `ModifiedIdx`
/// We could replace the Leaf variant indirection by adding more variants.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ModifiedNode {
    BranchBoth {
        bit_idx: u8,
        left: ModifiedIdx,
        right: ModifiedIdx,
    },
    BranchLeft {
        bit_idx: u8,
        left: ModifiedIdx,
        right: StoredIdx,
    },
    BranchRight {
        bit_idx: u8,
        left: StoredIdx,
        right: ModifiedIdx,
    },
    Leaf(LeafIdx),
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Leaf {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

pub type DataStore = HashMap<HashedNode, StoredNode>;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum TrieRoot {
    #[default]
    Empty,
    StoredNode(HashedNode),
    ModifiedNode(ModifiedIdx),
}

pub struct Transaction {
    pub data_store: DataStore,
    pub original_tree: Vec<(HashedNode, StoredNode)>,

    pub current_root: TrieRoot,
    pub modified_nodes: Vec<ModifiedNode>,
    pub modified_leafs: Vec<(HashedKey, Leaf)>,
}

impl Transaction {
    pub fn new(root: TrieRoot, data_store: DataStore) -> Self {
        Transaction {
            data_store,
            original_tree: Vec::new(),
            current_root: root,
            modified_nodes: Vec::new(),
            modified_leafs: Vec::new(),
        }
    }

    pub fn get(&self, key: &[u8]) -> Result<Option<Leaf>, String> {
        self.get_hashed(key, &hash_key(key))
    }

    pub fn get_hashed(&self, key: &[u8], hash: &HashedKey) -> Result<Option<Leaf>, String> {
        match &self.current_root {
            TrieRoot::Empty => Ok(None),
            TrieRoot::ModifiedNode(node_idx) => self.get_modified(*node_idx, key, hash),
            TrieRoot::StoredNode(node) => {
                let original_node = self.data_store.get(node).ok_or("Node not found")?;
                original_node.get_hashed(key, hash, &self.data_store)
            }
        }
    }

    fn get_modified(
        &self,
        node: ModifiedIdx,
        key: &[u8],
        hash: &HashedKey,
    ) -> Result<Option<Leaf>, String> {
        let mut node = &self.modified_nodes[node.0 as usize];
        loop {
            match node {
                ModifiedNode::BranchBoth {
                    bit_idx,
                    left,
                    right,
                } => {
                    let bit_slice = HashedKeyBits::from_slice(hash.0.as_ref());
                    let next_node_idx = if bit_slice[*bit_idx as usize] {
                        right
                    } else {
                        left
                    };

                    node = &self.modified_nodes[next_node_idx.0 as usize];
                }
                ModifiedNode::BranchLeft {
                    bit_idx,
                    left,
                    right,
                } => {
                    let bit_slice = HashedKeyBits::from_slice(hash.0.as_ref());
                    if bit_slice[*bit_idx as usize] {
                        return self.original_tree[right.0 as usize].1.get_hashed(
                            key,
                            hash,
                            &self.data_store,
                        );
                    } else {
                        node = &self.modified_nodes[left.0 as usize];
                    }
                }
                ModifiedNode::BranchRight {
                    bit_idx,
                    left,
                    right,
                } => {
                    let bit_slice = HashedKeyBits::from_slice(hash.0.as_ref());
                    if bit_slice[*bit_idx as usize] {
                        node = &self.modified_nodes[right.0 as usize];
                    } else {
                        return self.original_tree[left.0 as usize].1.get_hashed(
                            key,
                            hash,
                            &self.data_store,
                        );
                    }
                }
                ModifiedNode::Leaf(leaf_idx) => {
                    let (_, leaf) = &self.modified_leafs[leaf_idx.0 as usize];
                    if leaf.key == key {
                        return Ok(Some(leaf.clone()));
                    } else if *hash != hash_key(leaf.key.as_slice()) {
                        return Err("Provided key an Hash do not match hash".to_string());
                    } else {
                        return Ok(None);
                    }
                }
            }
        }
    }

    pub fn insert(&mut self, key: Vec<u8>, value: Vec<u8>) {
        let new_key_hash = hash_key(key.as_slice());

        match self.current_root {
            TrieRoot::ModifiedNode(node_idx) => {
                // we init the prior node to a leaf with a dummy index
                let mut node_idx = ModifiedIdx(node_idx.0);
                let mut prior_bit_idx = 0;
                loop {
                    let node = self.modified_nodes[node_idx.0 as usize];
                    match node {
                        ModifiedNode::BranchBoth {
                            bit_idx,
                            left,
                            right,
                        } => {
                            let bit_slice = HashedKeyBits::from_slice(new_key_hash.0.as_ref());
                            let next_node_idx = if bit_slice[bit_idx as usize] {
                                right
                            } else {
                                left
                            };

                            node_idx = next_node_idx;
                            prior_bit_idx = bit_idx;
                        }
                        ModifiedNode::BranchLeft {
                            bit_idx,
                            left,
                            right,
                        } => {
                            let bit_slice = HashedKeyBits::from_slice(new_key_hash.0.as_ref());
                            if bit_slice[bit_idx as usize] {
                                todo!();
                            } else {
                                node_idx = left;
                                prior_bit_idx = bit_idx;
                            }
                        }
                        ModifiedNode::BranchRight {
                            bit_idx,
                            left,
                            right,
                        } => {
                            let bit_slice = HashedKeyBits::from_slice(new_key_hash.0.as_ref());
                            if bit_slice[bit_idx as usize] {
                                node_idx = right;
                                prior_bit_idx = bit_idx;
                            } else {
                                todo!();
                            }
                        }
                        ModifiedNode::Leaf(old_leaf_idx) => {
                            let (old_key_hash, old_leaf) =
                                &mut self.modified_leafs[old_leaf_idx.0 as usize];

                            if key.as_slice() == old_leaf.key.as_slice() {
                                old_leaf.value = value;
                                return;
                            } else {
                                let prior_bit_idx = prior_bit_idx as usize;
                                let old_bits =
                                    &HashedKeyBits::from_slice(&old_key_hash.0)[prior_bit_idx..];
                                let new_bits =
                                    &HashedKeyBits::from_slice(&new_key_hash.0)[prior_bit_idx..];

                                // This can be optimized by applying word wise comparison first
                                let Some((bit_idx, new_bit)) = iter::zip(old_bits, new_bits)
                                    .enumerate()
                                    .find(|(_, (a, b))| a != b)
                                    .map(|(idx, (_, b))| (idx, *b))
                                else {
                                    // TODO: Error the hashes are equal, but the keys are not
                                    return;
                                };

                                let bit_idx = (prior_bit_idx + bit_idx) as u8;

                                let moved_old_leaf_node_idx: ModifiedIdx =
                                    self.modified_nodes.len().into();
                                self.modified_nodes.push(ModifiedNode::Leaf(old_leaf_idx));

                                let new_leaf_idx: LeafIdx = self.modified_leafs.len().into();
                                self.modified_leafs
                                    .push((new_key_hash, Leaf { key, value }));
                                let new_leaf_node_idx: ModifiedIdx =
                                    self.modified_nodes.len().into();
                                self.modified_nodes.push(ModifiedNode::Leaf(new_leaf_idx));

                                let (left, right) = if new_bit {
                                    (moved_old_leaf_node_idx, new_leaf_node_idx)
                                } else {
                                    (new_leaf_node_idx, moved_old_leaf_node_idx)
                                };

                                self.modified_nodes[node_idx.0 as usize] =
                                    ModifiedNode::BranchBoth {
                                        bit_idx,
                                        left,
                                        right,
                                    };
                            }
                            return;
                        }
                    }
                }
            }
            TrieRoot::StoredNode(ref node_hash) => {
                // self.insert_stored(node_hash, hash,);
                todo!();
            }
            TrieRoot::Empty => {
                self.modified_leafs
                    .push((new_key_hash, Leaf { key, value }));
                let leaf_idx = LeafIdx((self.modified_leafs.len() - 1) as u32);
                self.modified_nodes.push(ModifiedNode::Leaf(leaf_idx));
                self.current_root = TrieRoot::ModifiedNode(ModifiedIdx(0));
            }
        }
    }
}

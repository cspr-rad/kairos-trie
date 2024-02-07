pub mod modified;
pub mod stored;

pub use modified::*;
pub use stored::*;

use std::{collections::HashMap, iter};

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

/// A node that has been modified in a transaction.
///
/// Note: This merged representation is more efficient than nesting an enum `StoredIdx` | `ModifiedIdx`
/// We could replace the Leaf variant indirection by adding more variants.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ModifiedNode {
    BranchBoth {
        bit_idx: u8,
        left: NodeIdx,
        right: NodeIdx,
    },
    BranchLeft {
        bit_idx: u8,
        left: NodeIdx,
        right: NodeHash,
    },
    BranchRight {
        bit_idx: u8,
        left: NodeHash,
        right: NodeIdx,
    },
    Leaf(LeafIdx),
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Leaf {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

pub type DataStore = HashMap<NodeHash, StoredNode>;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum TrieRoot {
    #[default]
    Empty,
    StoredNode(NodeHash),
    ModifiedNode(NodeIdx),
}

pub struct Transaction {
    pub data_store: DataStore,
    pub stored_nodes: Vec<(NodeHash, StoredNode)>,

    pub current_root: TrieRoot,
    pub modified_nodes: Vec<ModifiedNode>,
    pub modified_leafs: Vec<(KeyHash, Leaf)>,
}

impl Transaction {
    pub fn new(root: TrieRoot, data_store: DataStore) -> Self {
        Transaction {
            data_store,
            stored_nodes: Vec::new(),
            current_root: root,
            modified_nodes: Vec::new(),
            modified_leafs: Vec::new(),
        }
    }

    pub fn get(&self, key: &[u8]) -> Result<Option<Leaf>, String> {
        self.get_hashed(key, &hash_key(key))
    }

    pub fn get_hashed(&self, key: &[u8], hash: &KeyHash) -> Result<Option<Leaf>, String> {
        match &self.current_root {
            TrieRoot::Empty => Ok(None),
            TrieRoot::ModifiedNode(node_idx) => self.get_modified(*node_idx, key, hash),
            TrieRoot::StoredNode(node_hash) => self.get_stored_hashed(*node_hash, key, hash),
        }
    }

    fn get_modified(
        &self,
        node: NodeIdx,
        key: &[u8],
        hash: &KeyHash,
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
                        return self.get_stored_hashed(*right, key, hash);
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
                        return self.get_stored_hashed(*left, key, hash);
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

    pub fn get_stored_hashed(
        &self,
        mut node_hash: NodeHash,
        key: &[u8],
        hash: &KeyHash,
    ) -> Result<Option<Leaf>, String> {
        loop {
            let node = self.data_store.get(&node_hash).ok_or("Node not found")?;

            match node {
                StoredNode::Branch {
                    bit_idx,
                    left,
                    right,
                } => {
                    let bit_slice = HashedKeyBits::from_slice(hash.0.as_ref());
                    node_hash = if bit_slice[*bit_idx as usize] {
                        *right
                    } else {
                        *left
                    };
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

    /// Insert a new key value pair into the trie.
    /// Returns the previous value if it existed.
    pub fn insert(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<Option<Leaf>, String> {
        let new_key_hash = hash_key(key.as_slice());

        match self.current_root {
            TrieRoot::ModifiedNode(node_idx) => {
                // we init the prior node to a leaf with a dummy index
                let mut node_idx = NodeIdx(node_idx.0);
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
                                let value = std::mem::replace(&mut old_leaf.value, value);
                                return Ok(Some(Leaf { key, value }));
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
                                    return Err(
                                        "The hashes are equal, but the keys are not".to_string()
                                    );
                                };

                                let bit_idx = (prior_bit_idx + bit_idx) as u8;

                                let moved_old_leaf_node_idx: NodeIdx =
                                    self.modified_nodes.len().into();
                                self.modified_nodes.push(ModifiedNode::Leaf(old_leaf_idx));

                                let new_leaf_idx: LeafIdx = self.modified_leafs.len().into();
                                self.modified_leafs
                                    .push((new_key_hash, Leaf { key, value }));
                                let new_leaf_node_idx: NodeIdx = self.modified_nodes.len().into();
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
                            return Ok(None);
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
                self.current_root = TrieRoot::ModifiedNode(NodeIdx(0));

                Ok(None)
            }
        }
    }

    fn insert_stored(
        &mut self,
        node_hash: &NodeHash,
        key_hash: &KeyHash,
        key: Vec<u8>,
        value: Vec<u8>,
    ) -> Result<Option<Leaf>, String> {
        let init_modified_node_count = self.modified_nodes.len();
        let mut node_hash = node_hash;

        loop {
            let node = self.data_store.get(node_hash).ok_or("Node not found")?;
            self.stored_nodes.push((node_hash.clone(), node.clone()));

            match node {
                StoredNode::Branch {
                    bit_idx,
                    left,
                    right,
                } => {
                    let bit_slice = HashedKeyBits::from_slice(&key_hash.0);
                    node_hash = if bit_slice[*bit_idx as usize] {
                        self.modified_nodes.push(ModifiedNode::BranchRight {
                            bit_idx: *bit_idx,
                            left: *left,
                            right: NodeIdx(self.modified_nodes.len() as u32),
                        });
                        right
                    } else {
                        left
                    };
                }

                StoredNode::Leaf(leaf) => {
                    todo!();
                }
            }
        }

        todo!();
    }
}

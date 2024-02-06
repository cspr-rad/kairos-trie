use std::collections::HashMap;

use bitvec::prelude::*;
use sha2::{Digest, Sha256};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct HashedKey(pub [u8; 32]);

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct HashedNode(pub [u8; 32]);

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct StoredIdx(u32);

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct ModifiedIdx(u32);

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct LeafIdx(u32);

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
                    let bit_slice = BitSlice::<u8, Lsb0>::from_slice(hash.0.as_ref());
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
/// Note: This merged representation is 16 bytes, a naive representation would be 20 bytes.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
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
    ModifiedNode(ModifiedNode),
}

pub struct Transaction {
    pub data_store: DataStore,
    pub original_tree: Vec<(HashedNode, StoredNode)>,

    pub current_root: TrieRoot,
    pub modified_nodes: Vec<ModifiedNode>,
    pub modified_leafs: Vec<Leaf>,
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
            TrieRoot::ModifiedNode(node) => self.get_modified(node, key, hash),
            TrieRoot::StoredNode(node) => {
                let original_node = self.data_store.get(node).ok_or("Node not found")?;
                original_node.get_hashed(key, hash, &self.data_store)
            }
        }
    }

    fn get_modified(
        &self,
        node: &ModifiedNode,
        key: &[u8],
        hash: &HashedKey,
    ) -> Result<Option<Leaf>, String> {
        let mut node = node;
        loop {
            match node {
                ModifiedNode::BranchBoth {
                    bit_idx,
                    left,
                    right,
                } => {
                    let bit_slice = BitSlice::<u8, Lsb0>::from_slice(hash.0.as_ref());
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
                    let bit_slice = BitSlice::<u8, Lsb0>::from_slice(hash.0.as_ref());
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
                    let bit_slice = BitSlice::<u8, Lsb0>::from_slice(hash.0.as_ref());
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
                    let leaf = &self.modified_leafs[leaf_idx.0 as usize];
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

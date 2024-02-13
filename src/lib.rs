#![allow(clippy::type_complexity)]
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

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct KeyHash(pub [u8; 32]);

/// TODO: Switch to usize for more efficient bit indexing.
type HashedKeyBits = BitSlice<u8, Lsb0>;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Branch<NR> {
    // TODO we can make bit_idx a debug only field
    bit_idx: u8,
    left_bits: u8,
    right_bits: u8,
    pub left: NR,
    pub right: NR,
}

impl<SBR, SER, SLR> Branch<NodeRef<SBR, SER, SLR>> {
    /// Create a new branch.
    /// Returns the byte index of the branch for creating an extension node.
    pub fn from_hashed_key_bits(
        _prior_bit_idx: u8,
        a_leaf_idx: NodeRef<SBR, SER, SLR>,
        a_hash: &KeyHash,
        b_hash: &KeyHash,
        b_leaf_idx: NodeRef<SBR, SER, SLR>,
    ) -> (usize, Self) {
        iter::zip(a_hash.0, b_hash.0)
            .enumerate()
            .find(|(_, (a, b))| a != b)
            .map(|(idx, (a, b))| {
                let matched = a ^ b;
                let rel_bit_idx = matched.leading_zeros();

                let (left, right, left_bits, right_bits) = if (a >> rel_bit_idx) & 1 == 1 {
                    (b_leaf_idx, a_leaf_idx, b, a)
                } else {
                    (a_leaf_idx, b_leaf_idx, a, b)
                };

                (
                    idx,
                    Branch {
                        bit_idx: idx as u8 * 8 + rel_bit_idx as u8,
                        left_bits,
                        right_bits,
                        left,
                        right,
                    },
                )
            })
            .unwrap_or_else(|| {
                // TODO handle the case where the two hashes are equal
                panic!("The two hashes are equal");
            })
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Extension<SBR, SER, SLR> {
    next: NodeRef<SBR, SER, SLR>,
    bits: Box<[u8]>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Leaf {
    pub key_hash: KeyHash,
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

    fn get_hashed(&mut self, key_hash: &KeyHash) -> Result<Option<&Leaf>, String> {
        match self.current_root {
            TrieRoot::Empty => Ok(None),
            TrieRoot::Node(NodeRef::ModLeaf(leaf_idx)) => {
                let leaf = &self.modified_leaves[leaf_idx];
                if leaf.key_hash == *key_hash {
                    Ok(Some(leaf))
                } else {
                    Ok(None)
                }
            }
            TrieRoot::Node(NodeRef::ModBranch(branch_idx)) => {
                self.get_modified(branch_idx, key_hash)
            }
            TrieRoot::Node(NodeRef::StoredBranch(br)) => {
                return self.get_stored_branch(br, key_hash);
            }
            TrieRoot::Node(NodeRef::StoredLeaf(lr)) => {
                return self.get_stored_leaf(lr, key_hash);
            }
            TrieRoot::Node(NodeRef::StoredExtension(_)) => todo!(),
            TrieRoot::Node(NodeRef::ModExtension(_)) => todo!(),
        }
    }

    fn get_modified(
        &self,
        branch_idx: BranchIdx,
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
                    let leaf = &self.modified_leaves[leaf_idx];
                    if leaf.key_hash == *key_hash {
                        return Ok(Some(leaf));
                    } else {
                        return Ok(None);
                    }
                }
                NodeRef::StoredBranch(br) => {
                    return self.get_stored_branch(br, key_hash);
                }
                NodeRef::StoredLeaf(lr) => {
                    return self.get_stored_leaf(lr, key_hash);
                }
                NodeRef::StoredExtension(_) => todo!(),
            };
        }
    }

    pub fn get_stored_branch(
        &self,
        mut branch_ref: S::BranchRef,
        key_hash: &KeyHash,
    ) -> Result<Option<&Leaf>, String> {
        loop {
            let Branch {
                bit_idx,
                left_bits,
                right_bits,
                left,
                right,
            } = self.data_store.get_branch(branch_ref)?;

            let bit_slice = HashedKeyBits::from_slice(key_hash.0.as_ref());
            let node_ref = if bit_slice[*bit_idx as usize] {
                right
            } else {
                left
            };

            match node_ref {
                stored::Node::Branch(br) => {
                    branch_ref = *br;
                }
                stored::Node::Leaf(lr) => {
                    return self.get_stored_leaf(*lr, key_hash);
                }
                stored::Node::Extension(_) => todo!(),
            }
        }
    }

    pub fn get_stored_leaf(
        &self,
        leaf: S::LeafRef,

        key_hash: &KeyHash,
    ) -> Result<Option<&Leaf>, String> {
        let leaf = self.data_store.get_leaf(leaf)?;
        if leaf.key_hash == *key_hash {
            Ok(Some(leaf))
        } else {
            Ok(None)
        }
    }

    fn insert_hashed(&mut self, value: &[u8], key_hash: &KeyHash) -> Result<(), String> {
        match self.current_root {
            TrieRoot::Empty => {
                let node = self.modified_leaves.push(Leaf {
                    key_hash: *key_hash,
                    value: value.to_vec(),
                });
                self.current_root = TrieRoot::Node(NodeRef::ModLeaf(node));
                Ok(())
            }
            TrieRoot::Node(NodeRef::ModBranch(branch_idx)) => {
                self.insert_modified_branch(branch_idx, key_hash, value)
            }
            TrieRoot::Node(NodeRef::ModLeaf(old_leaf_idx)) => {
                self.insert_modified_leaf(old_leaf_idx, key_hash, value)
            }
            TrieRoot::Node(NodeRef::StoredBranch(branch_ref)) => {
                self.current_root =
                    TrieRoot::Node(self.insert_stored_branch(branch_ref, key_hash, value)?);
                Ok(())
            }
            TrieRoot::Node(NodeRef::StoredLeaf(leaf_ref)) => {
                self.current_root =
                    TrieRoot::Node(self.insert_stored_leaf(leaf_ref, key_hash, value)?);
                Ok(())
            }
            TrieRoot::Node(NodeRef::StoredExtension(_)) => todo!(),
            TrieRoot::Node(NodeRef::ModExtension(_)) => todo!(),
        }
    }

    fn insert_modified_branch(
        &mut self,
        mut branch_idx: BranchIdx,
        key_hash: &KeyHash,
        value: &[u8],
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
        key_hash: &KeyHash,
        value: &[u8],
    ) -> Result<(), String> {
        todo!();
    }

    fn insert_stored_branch(
        &mut self,
        mut branch_ref: S::BranchRef,
        key_hash: &KeyHash,
        value: &[u8],
    ) -> Result<NodeRefTxn<S>, String> {
        loop {
            let Branch {
                bit_idx,
                left_bits,
                right_bits,
                left,
                right,
            } = *self.data_store.get_branch(branch_ref)?;

            let bit_slice = HashedKeyBits::from_slice(&key_hash.0);

            let mut next_mod_ref = |next_node| {
                match next_node {
                    stored::Node::Branch(br) => {
                        branch_ref = br;
                        Ok(NodeRef::ModBranch(BranchIdx(
                            // refers to the branch that will be inserted in the next iteration
                            self.modified_branches.0.len() as u32 + 1,
                        )))
                    }
                    stored::Node::Extension(_) => todo!(),
                    stored::Node::Leaf(lr) => self.insert_stored_leaf(lr, key_hash, value),
                }
            };

            let (left, right) = if bit_slice[bit_idx as usize] {
                (left.into(), next_mod_ref(right)?)
            } else {
                (next_mod_ref(left)?, right.into())
            };

            // the current branch contains a reference to the next branch or leaf
            self.modified_branches.push(Branch {
                bit_idx,
                left_bits,
                right_bits,
                left,
                right,
            });
        }
    }

    fn insert_stored_leaf(
        &mut self,
        leaf_ref: S::LeafRef,
        key_hash: &KeyHash,
        value: &[u8],
    ) -> Result<NodeRefTxn<S>, String> {
        let leaf = self.data_store.get_leaf(leaf_ref)?;

        let leaf_idx = self.modified_leaves.push(Leaf {
            key_hash: *key_hash,
            value: value.to_vec(),
        });

        if leaf.key_hash == *key_hash {
            Ok(NodeRef::ModLeaf(leaf_idx))
        } else {
            // TODO Create extension
            let (idx, branch) = Branch::from_hashed_key_bits(
                0,
                NodeRef::StoredLeaf(leaf_ref),
                key_hash,
                &leaf.key_hash,
                NodeRef::ModLeaf(leaf_idx),
            );
            let new_branch_idx = self.modified_branches.push(branch);

            Ok(NodeRef::ModBranch(new_branch_idx))
        }
    }
}

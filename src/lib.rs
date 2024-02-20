#![allow(clippy::type_complexity)]
#![cfg_attr(not(feature = "std"), no_std)]
#[cfg(not(feature = "std"))]
extern crate core as std;

extern crate alloc;

pub mod modified;
pub mod stored;

use std::iter;

use alloc::{boxed::Box, string::String};
pub use modified::*;
pub use stored::Store;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct KeyHash(pub [u32; 8]);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Branch<NR> {
    bit_idx: u32,
    left_bits: u32,
    right_bits: u32,
    pub left: NR,
    pub right: NR,
}

impl<V> Branch<NodeRef<V>> {
    /// Create a new branch.
    /// Returns the byte index of the branch for creating an extension node.
    pub fn from_hashed_key_bits(
        _prior_bit_idx: u8,
        a_leaf_idx: NodeRef<V>,
        a_hash: &KeyHash,
        b_hash: &KeyHash,
        b_leaf_idx: NodeRef<V>,
    ) -> (usize, Self) {
        iter::zip(a_hash.0, b_hash.0)
            .enumerate()
            .find(|(_, (a, b))| a != b)
            .map(|(idx, (a, b))| {
                let matched = a ^ b;
                let rel_bit_idx = matched.leading_zeros();

                let (left, right, left_bits, right_bits) = if bit_at(a, rel_bit_idx) {
                    (b_leaf_idx, a_leaf_idx, b, a)
                } else {
                    (a_leaf_idx, b_leaf_idx, a, b)
                };

                (
                    idx,
                    Branch {
                        bit_idx: rel_bit_idx + (idx * 32) as u32,
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

impl<NR> Branch<NR> {
    pub fn desend<T>(
        &self,
        bit_idx: u32,
        hash_segment: u32,
        left: impl FnOnce() -> T,
        right: impl FnOnce() -> T,
    ) -> Option<T> {
        let rel_bit_idx = self.bit_idx - bit_idx;
        debug_assert!(rel_bit_idx < 32, "Bit index must be less than 32");
        debug_assert_eq!(
            (self.left_bits ^ self.right_bits).leading_zeros(),
            rel_bit_idx
        );

        // The mask of the prefix of the branch and the discriminant
        let prefix_mask: u32 = (1 << (rel_bit_idx + 1)) - 1;
        debug_assert_eq!(prefix_mask.count_ones(), (rel_bit_idx + 1));

        if (hash_segment ^ self.left_bits) & prefix_mask == 0 {
            Some(left())
        } else if (hash_segment ^ self.right_bits) & prefix_mask == 0 {
            Some(right())
        } else {
            None
        }
    }
}

#[inline(always)]
fn bit_at(hash_segment: u32, rel_bit_idx: u32) -> bool {
    ((hash_segment >> rel_bit_idx) & 1) == 1
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Extension<NR> {
    next: NR,
    bits: Box<[u8]>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Leaf<V> {
    pub key_hash: KeyHash,
    pub value: V,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum TrieRoot<V> {
    #[default]
    Empty,
    Node(NodeRef<V>),
}

pub struct Transaction<S: Store<V>, V> {
    data_store: S,
    pub current_root: TrieRoot<V>,
}

impl<S: Store<V>, V> Transaction<S, V> {
    pub fn new(root: TrieRoot<V>, data_store: S) -> Self {
        Transaction {
            current_root: root,
            data_store,
        }
    }

    pub fn get(&mut self, key_hash: &KeyHash) -> Result<Option<&V>, String> {
        match &self.current_root {
            TrieRoot::Empty => Ok(None),
            TrieRoot::Node(node_ref) => Self::get_node(&mut self.data_store, node_ref, key_hash),
        }
    }

    pub fn get_node<'root>(
        data_store: &mut S,
        mut node_ref: &'root NodeRef<V>,
        key_hash: &KeyHash,
    ) -> Result<Option<&'root V>, String> {
        let mut word_idx = 0;
        let mut hash_segment = key_hash.0[word_idx as usize];

        // Will be overwritten by the first or extension node
        let mut prior_hash_word = 0;

        loop {
            let bit_idx = word_idx * 32;

            match node_ref {
                NodeRef::ModBranch(branch) => {
                    debug_assert!(word_idx < 256, "Bit index must be less than 256");

                    if branch.bit_idx - bit_idx > 31 {
                        if prior_hash_word == hash_segment {
                            // TODO: assert tree did not start as a branch
                            // this case is possible once we allow transaction spliting
                            word_idx += 1;
                            hash_segment = key_hash.0[word_idx as usize];
                        } else {
                            return Ok(None);
                        }
                    }

                    let Some((next, bits)) = branch.desend(
                        bit_idx,
                        hash_segment,
                        || (&branch.left, branch.left_bits),
                        || (&branch.right, branch.right_bits),
                    ) else {
                        return Ok(None);
                    };

                    node_ref = next;
                }
                NodeRef::ModExtension(_) => todo!(),
                NodeRef::ModLeaf(leaf) => {
                    if leaf.key_hash == *key_hash {
                        return Ok(Some(&leaf.value));
                    } else {
                        return Ok(None);
                    }
                }
                NodeRef::Stored(stored_idx) => {
                    let _todo = data_store.get_node(*stored_idx).map_err(|e| e.into())?;
                }
            }
        }
    }

    // fn get_modified(
    //     &mut self,
    //     mut branch: &Branch<NodeRef<V>>,
    //     key_hash: &KeyHash,
    //     hash_word_idx: u32,
    // ) -> Result<Option<&V>, String> {
    //     loop {
    //         match next {
    //             NodeRef::ModBranch(next_branch) => branch = next_branch,
    //             NodeRef::ModExtension(_) => {
    //                 // let iter::zip(bits.iter(), key_hash.0.iter().skip(hash_seg_idx as usize)) .find(|(a, b)| a != b);
    //                 todo!();
    //             }

    //             NodeRef::ModLeaf(leaf) => {
    //                 if leaf.key_hash == *key_hash {
    //                     return Ok(Some(&leaf.value));
    //                 } else {
    //                     return Ok(None);
    //                 }
    //             }
    //             NodeRef::StoredExtension(_) => todo!(),
    //         };
    //     }
    // }

    // pub fn get_stored_branch(
    //     &self,
    //     mut branch_ref: B,
    //     key_hash: &KeyHash,
    // ) -> Result<Option<&Leaf<V>>, String> {
    //     loop {
    //         let branch = self.data_store.get_branch(branch_ref)?;

    //         let node_ref = branch.desend(
    //             key_hash.0[branch.rel_bit_idx as usize],
    //             branch.left,
    //             branch.right,
    //         );

    //         match node_ref {
    //             stored::Node::Branch(br) => {
    //                 branch_ref = br;
    //             }
    //             stored::Node::Leaf(lr) => {
    //                 return self.get_stored_leaf(lr, key_hash);
    //             }
    //             stored::Node::Extension(_) => todo!(),
    //         }
    //     }
    // }

    // pub fn get_stored_leaf(&self, leaf: L, key_hash: &KeyHash) -> Result<Option<&Leaf<V>>, String> {
    //     let leaf = self.data_store.get_leaf(leaf)?;
    //     if leaf.key_hash == *key_hash {
    //         Ok(Some(leaf))
    //     } else {
    //         Ok(None)
    //     }
    // }

    // fn insert_hashed(&mut self, value: &[u8], key_hash: &KeyHash) -> Result<(), String> {
    //     match &mut self.current_root {
    //         TrieRoot::Empty => {
    //             self.current_root = TrieRoot::Node(NodeRef::ModLeaf(Box::new(Leaf {
    //                 key_hash: *key_hash,
    //                 value: value.to_vec(),
    //             })));
    //             Ok(())
    //         }
    //         TrieRoot::Node(NodeRef::ModBranch(branch_idx)) => {
    //             self.insert_modified_branch(branch_idx, key_hash, value)
    //         }
    //         TrieRoot::Node(NodeRef::ModLeaf(old_leaf_idx)) => {
    //             self.insert_modified_leaf(old_leaf_idx, key_hash, value)
    //         }
    //         TrieRoot::Node(NodeRef::StoredBranch(branch_ref)) => {
    //             self.current_root =
    //                 TrieRoot::Node(self.insert_stored_branch(branch_ref, key_hash, value)?);
    //             Ok(())
    //         }
    //         TrieRoot::Node(NodeRef::StoredLeaf(leaf_ref)) => {
    //             self.current_root =
    //                 TrieRoot::Node(self.insert_stored_leaf(leaf_ref, key_hash, value)?);
    //             Ok(())
    //         }
    //         TrieRoot::Node(NodeRef::StoredExtension(_)) => todo!(),
    //         TrieRoot::Node(NodeRef::ModExtension(_)) => todo!(),
    //     }
    // }

    // fn insert_modified_branch(
    //     &mut self,
    //     mut branch_idx: &mut Branch<NodeRef<HR, V>>,
    //     key_hash: &KeyHash,
    //     value: &[u8],
    // ) -> Result<(), String> {
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
    //     todo!();
    // }

    // fn insert_modified_leaf(
    //     &mut self,
    //     leaf_idx: LeafIdx,
    //     key_hash: &KeyHash,
    //     value: &[u8],
    // ) -> Result<(), String> {
    //     todo!();
    // }

    // fn insert_stored_branch(
    //     &mut self,
    //     mut branch_ref: S::BranchRef,
    //     key_hash: &KeyHash,
    //     value: &[u8],
    // ) -> Result<NodeRefTxn<S>, String> {
    //     loop {
    //         let Branch {
    //             rel_bit_idx,
    //             left_bits,
    //             right_bits,
    //             left,
    //             right,
    //         } = *self.data_store.get_branch(branch_ref)?;

    //         let mut next_mod_ref = |next_node| {
    //             match next_node {
    //                 stored::Node::Branch(br) => {
    //                     branch_ref = br;
    //                     Ok(NodeRef::ModBranch(BranchIdx(
    //                         // refers to the branch that will be inserted in the next iteration
    //                         self.modified_branches.0.len() as u32 + 1,
    //                     )))
    //                 }
    //                 stored::Node::Extension(_) => todo!(),
    //                 stored::Node::Leaf(lr) => self.insert_stored_leaf(lr, key_hash, value),
    //             }
    //         };

    //         let (left, right) = if bit_at(key_hash.0[rel_bit_idx as usize], rel_bit_idx) {
    //             (left.into(), next_mod_ref(right)?)
    //         } else {
    //             (next_mod_ref(left)?, right.into())
    //         };

    //         // the current branch contains a reference to the next branch or leaf
    //         self.modified_branches.push(Branch {
    //             rel_bit_idx,
    //             left_bits,
    //             right_bits,
    //             left,
    //             right,
    //         });
    //     }
    // }

    // fn insert_stored_leaf(
    //     &mut self,
    //     leaf_ref: S::LeafRef,
    //     key_hash: &KeyHash,
    //     value: &[u8],
    // ) -> Result<NodeRefTxn<S>, String> {
    //     let leaf = self.data_store.get_leaf(leaf_ref)?;

    //     let leaf_idx = self.modified_leaves.push(Leaf {
    //         key_hash: *key_hash,
    //         value: value.to_vec(),
    //     });

    //     if leaf.key_hash == *key_hash {
    //         Ok(NodeRef::ModLeaf(leaf_idx))
    //     } else {
    //         // TODO Create extension
    //         let (idx, branch) = Branch::from_hashed_key_bits(
    //             0,
    //             NodeRef::StoredLeaf(leaf_ref),
    //             key_hash,
    //             &leaf.key_hash,
    //             NodeRef::ModLeaf(leaf_idx),
    //         );
    //         let new_branch_idx = self.modified_branches.push(branch);

    //         Ok(NodeRef::ModBranch(new_branch_idx))
    //     }
    // }
}

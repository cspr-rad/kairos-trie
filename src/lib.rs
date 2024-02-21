#![allow(clippy::type_complexity)]
#![cfg_attr(not(feature = "std"), no_std)]
#[cfg(not(feature = "std"))]
extern crate core as std;

extern crate alloc;

pub mod modified;
pub mod stored;

use std::iter;

use alloc::{string::String, vec::Vec};
pub use modified::*;
pub use stored::Store;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct KeyHash(pub [u32; 8]);

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Branch<NR> {
    bit_idx: u32,
    /// Contains prefix bits, 1, suffix bits, trailing 0s.
    /// Zero occurs at the bit index of the branch.
    prefix_discriminant_suffix: u32,
    /// Contains 1 in the discriminant bit.
    /// Contains 1 in all divergent suffix bits.
    /// These are the trailing zeros in `prefix_suffix`.
    ///
    /// If the mask is 1 it represents the divergent bit not a single divirgent suffix bit.
    ///
    /// bit_idx_mask = prefix_discriminant_suffix & discriminant_trailing_bits_mask;
    /// bit_idx = bit_idx_mask.leading_zeros() == discriminant_trailing_bits_mask.leading_zeros()
    discriminant_trailing_bits_mask: u32,
    pub left: NR,
    pub right: NR,

    pub extension: Vec<u32>,
}

impl<NR> Branch<NR> {
    fn discriminant_bit_mask(&self) -> u32 {
        self.prefix_discriminant_suffix & self.discriminant_trailing_bits_mask
    }

    fn discriminant_bit_idx(&self) -> u32 {
        self.discriminant_trailing_bits_mask.leading_zeros()
    }

    fn prefix_mask(&self) -> u32 {
        self.discriminant_bit_mask() - 1
    }

    fn is_right_descendant(&self, hash_segment: u32) -> bool {
        self.prefix_discriminant_suffix ^ hash_segment ^ self.discriminant_trailing_bits_mask == 0
    }

    fn is_left_descendant(&self, hash_segment: u32) -> bool {
        let zero_discriminant =
            self.prefix_discriminant_suffix ^ self.discriminant_trailing_bits_mask;
        zero_discriminant ^ hash_segment ^ self.discriminant_trailing_bits_mask == 0
    }

    fn trailing_bits_mask(&self) -> u32 {
        self.discriminant_bit_mask() ^ self.discriminant_trailing_bits_mask
    }

    fn no_trailing_bits(&self) -> bool {
        let r = self.trailing_bits_mask() == 0;
        debug_assert!(
            !r && self.extension.is_empty(),
            "A branch with trailing bits cannot have an extension"
        );

        r
    }
}

// impl<V> Branch<NodeRef<V>> {

//     /// Create a new branch.
//     /// Returns the byte index of the branch for creating an extension node.
//     pub fn from_hashed_key_bits(
//         _prior_bit_idx: u8,
//         a_leaf_idx: NodeRef<V>,
//         a_hash: &KeyHash,
//         b_hash: &KeyHash,
//         b_leaf_idx: NodeRef<V>,
//     ) -> (usize, Self) {
//         iter::zip(a_hash.0, b_hash.0)
//             .enumerate()
//             .find(|(_, (a, b))| a != b)
//             .map(|(idx, (a, b))| {
//                 let matched = a ^ b;

//                 let rel_bit_idx = matched.leading_zeros();
//                 let rel_bit_mask = 1 << rel_bit_idx;

//                 let prefix_mask = rel_bit_mask - 1;

//                 let suffix_mask = !(prefix_mask | rel_bit_mask);

//                 let common_suffix =

//                 let prefix_suffix = todo!();
//                 let suffix_mask = todo!();
//                 let prefix_suffix =
//                     (left_bits & !((rel_bit_mask) - 1)) | (right_bits & ((1 << rel_bit_idx) - 1));
//                 let suffix_mask = !(matched - 1);
//                 let (left, right, left_bits, right_bits) = if bit_at(a, rel_bit_idx) {
//                     (b_leaf_idx, a_leaf_idx, b, a)
//                 } else {
//                     (a_leaf_idx, b_leaf_idx, a, b)
//                 };

//                 (
//                     idx,
//                     Branch {
//                         bit_idx: rel_bit_idx + (idx * 32) as u32,
//                         prefix_suffix,
//                         suffix_mask,
//                         left,
//                         right,
//                     },
//                 )
//             })
//             .unwrap_or_else(|| {
//                 // TODO handle the case where the two hashes are equal
//                 panic!("The two hashes are equal");
//             })
//     }
// }

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

    pub fn get_node<'root, 's: 'root>(
        data_store: &'s mut S,
        mut node_ref: &'root NodeRef<V>,
        key_hash: &KeyHash,
    ) -> Result<Option<&'root V>, String> {
        let mut word_idx = 0;
        loop {
            debug_assert!(word_idx < 8);
            let hash_segment = key_hash.0[word_idx];

            match node_ref {
                NodeRef::ModBranch(branch) => {
                    let next = if branch.is_right_descendant(hash_segment) {
                        &branch.right
                    } else if branch.is_left_descendant(hash_segment) {
                        &branch.left
                    } else {
                        return Ok(None);
                    };

                    let check_extension =
                        iter::zip(branch.extension.iter(), key_hash.0.iter().skip(word_idx))
                            .all(|(a, b)| a == b);

                    // advance to the next word if this one is matched
                    if branch.no_trailing_bits() && check_extension {
                        word_idx += 1 + branch.extension.len();
                    } else {
                        return Ok(None);
                    }

                    node_ref = next;
                }
                NodeRef::ModLeaf(leaf) => {
                    if leaf.key_hash == *key_hash {
                        return Ok(Some(&leaf.value));
                    } else {
                        return Ok(None);
                    }
                }
                NodeRef::Stored(stored_idx) => {
                    return Self::get_stored_node(data_store, *stored_idx, key_hash, word_idx);
                }
            }
        }
    }

    pub fn get_stored_node<'s>(
        data_store: &'s mut S,
        mut stored_idx: stored::Idx,
        key_hash: &KeyHash,
        mut word_idx: usize,
    ) -> Result<Option<&'s V>, String> {
        debug_assert!(word_idx < 8);

        loop {
            let node = data_store.get_node(stored_idx).map_err(|e| e.into())?;

            match node {
                stored::Node::Branch(branch) => {
                    let hash_segment = key_hash.0[word_idx];

                    stored_idx = if branch.is_right_descendant(hash_segment) {
                        branch.right
                    } else if branch.is_left_descendant(hash_segment) {
                        branch.left
                    } else {
                        return Ok(None);
                    };

                    let check_extension =
                        iter::zip(branch.extension.iter(), key_hash.0.iter().skip(word_idx))
                            .all(|(a, b)| a == b);

                    // advance to the next word if this one is matched
                    if branch.no_trailing_bits() && check_extension {
                        word_idx += 1 + branch.extension.len();
                    } else {
                        return Ok(None);
                    }
                }
                stored::Node::Leaf(_) => {
                    break;
                }
            }
        }

        // This makes the borrow checker happy
        if let stored::Node::Leaf(leaf) = data_store.get_node(stored_idx).map_err(|e| e.into())? {
            if leaf.key_hash == *key_hash {
                Ok(Some(&leaf.value))
            } else {
                Ok(None)
            }
        } else {
            unreachable!("The prior loop only breaks if the node is a leaf");
        }
    }

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

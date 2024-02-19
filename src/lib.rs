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

#[inline(always)]
fn bit_at(hash_segment: u32, rel_bit_idx: u32) -> bool {
    (hash_segment >> rel_bit_idx) & 1 == 1
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Branch<NR> {
    // TODO we can make bit_idx a debug only field by counting leading zeros in l ^ r
    // We may want to keep it because risc0 does not have a leading zeros instruction
    rel_bit_idx: u32,
    left_bits: u32,
    right_bits: u32,
    pub left: NR,
    pub right: NR,
}

impl<B, E, L, V> Branch<NodeRef<B, E, L, V>> {
    /// Create a new branch.
    /// Returns the byte index of the branch for creating an extension node.
    pub fn from_hashed_key_bits(
        _prior_bit_idx: u8,
        a_leaf_idx: NodeRef<B, E, L, V>,
        a_hash: &KeyHash,
        b_hash: &KeyHash,
        b_leaf_idx: NodeRef<B, E, L, V>,
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
                        rel_bit_idx,
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
    pub fn desend<T>(&self, hash_segment: u32, left: T, right: T) -> T {
        if bit_at(hash_segment, self.rel_bit_idx) {
            left
        } else {
            right
        }
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Extension<B, E, L, V> {
    next: NodeRef<B, E, L, V>,
    bits: Box<[u8]>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Leaf<V> {
    pub key_hash: KeyHash,
    pub value: V,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum TrieRoot<B, E, L, V> {
    #[default]
    Empty,
    Node(NodeRef<B, E, L, V>),
}

pub struct Transaction<'s, S: Store<V>, V> {
    data_store: &'s S,
    pub current_root: TrieRoot<S::HashRef, S::HashRef, S::HashRef, V>,
}

impl<'s, S: Store<V>, V> Transaction<'s, S, V> {
    pub fn new(
        root: TrieRoot<S::HashRef, S::HashRef, S::HashRef, V>,
        data_store: &'s S,
    ) -> Self {
        Transaction {
            current_root: root,
            data_store,
        }
    }

    pub fn get<'a>(
        key_hash: &KeyHash,
        node: &'a NodeRef<S::HashRef, S::HashRef, S::HashRef, V>,
        data_store: &'s S,
    ) -> Result<Option<&'a V>, String>
    where
        's: 'a,
    {
        todo!();
    }

    // pub fn get_node(&self, key_hash: &KeyHash) -> Result<Option<NodeRef<B, E, L, V>>, String> {}

    // fn get_modified_branch(
    //     &self,
    //     mut branch: &Branch<NodeRef<B, E, L, V>>,
    //     key_hash: &KeyHash,
    //     hash_idx: u32,
    // ) -> Result<Option<&Leaf<V>>, String> {
    //     let mut hash_segment = key_hash.0[hash_idx as usize];
    //     loop {
    //         let next = if bit_at(hash_segment, branch.rel_bit_idx) {
    //             &branch.right
    //         } else {
    //             &branch.left
    //         };

    //         match next {
    //             NodeRef::ModBranch(next_branch) => branch = next_branch,
    //             NodeRef::ModExtension(_) => {
    //                 // let iter::zip(bits.iter(), key_hash.0.iter().skip(hash_seg_idx as usize)) .find(|(a, b)| a != b);
    //                 todo!();
    //             }

    //             NodeRef::ModLeaf(leaf) => {
    //                 if leaf.key_hash == *key_hash {
    //                     return Ok(Some(leaf.as_ref()));
    //                 } else {
    //                     return Ok(None);
    //                 }
    //             }
    //             NodeRef::StoredBranch(br) => {
    //                 return self.get_stored_branch(*br, key_hash);
    //             }
    //             NodeRef::StoredLeaf(lr) => {
    //                 return self.get_stored_leaf(*lr, key_hash);
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
    //     mut branch_idx: &mut Branch<NodeRef<B, E, L, V>>,
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

#![allow(clippy::type_complexity)]
#![cfg_attr(not(feature = "std"), no_std)]
#[cfg(not(feature = "std"))]
extern crate core as std;

extern crate alloc;

pub mod modified;
pub mod stored;

use core::fmt::Debug;
use std::{iter, mem};

use alloc::{boxed::Box, string::String, vec::Vec};
pub use modified::*;
use sha2::{Digest, Sha256};
pub use stored::Store;
use stored::{
    merkle::{Snapshot, SnapshotBuilder},
    DatabaseSet, Node, NodeHash,
};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct KeyHash(pub [u32; 8]);

impl KeyHash {
    pub fn from_bytes(hash_key: &[u8; 32]) -> Self {
        let mut r = [0; 8];

        hash_key
            .chunks_exact(4)
            .enumerate()
            .for_each(|(i, chunk)| r[i] = u32::from_le_bytes(chunk.try_into().unwrap()));

        Self(r)
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        let mut r = [0; 32];

        self.0.iter().enumerate().for_each(|(i, &word)| {
            let [a, b, c, d] = word.to_le_bytes();
            let offset = i * 4;
            r[offset] = a;
            r[offset + 1] = b;
            r[offset + 2] = c;
            r[offset + 3] = d;
        });

        r
    }
}

impl From<&[u8; 32]> for KeyHash {
    fn from(hash_key: &[u8; 32]) -> Self {
        Self::from_bytes(hash_key)
    }
}

impl From<&KeyHash> for [u8; 32] {
    fn from(hash: &KeyHash) -> [u8; 32] {
        hash.to_bytes()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
struct BranchMask {
    /// The index of the discriminant bit in the 256 bit hash key.
    bit_idx: u32,

    /// Common prefix of word at `bit_idx / 32`, a 0 discriminant bit, and trailing 0s
    left_prefix: u32,
}

impl BranchMask {
    const fn new(word_idx: u32, a: u32, b: u32) -> Self {
        let diff = a ^ b;
        let relative_bit_idx = diff.trailing_zeros();

        let bit_idx = word_idx * 32 + relative_bit_idx;

        let prefix_mask = (1 << relative_bit_idx) - 1;

        // The left branch always has a 0 discriminant bit.
        let left_prefix = a & prefix_mask;

        BranchMask {
            bit_idx,
            left_prefix,
        }
    }

    #[inline(always)]
    fn right_prefix(&self) -> u32 {
        self.left_prefix | self.discriminant_bit_mask()
    }

    #[inline(always)]
    fn is_left_descendant(&self, hash_segment: u32) -> bool {
        (hash_segment & self.prefix_discriminant_mask()) == self.left_prefix
    }

    #[inline(always)]
    fn is_right_descendant(&self, hash_segment: u32) -> bool {
        (hash_segment & self.prefix_discriminant_mask()) == self.right_prefix()
    }

    #[inline(always)]
    fn word_idx(&self) -> usize {
        (self.bit_idx / 32) as usize
    }

    /// The index of the discriminant bit in the `left_prefix`.
    #[inline(always)]
    fn relative_bit_idx(&self) -> u32 {
        let r = self.bit_idx % 32;
        debug_assert!(r < 32);
        r
    }

    #[inline(always)]
    fn discriminant_bit_mask(&self) -> u32 {
        1 << self.relative_bit_idx()
    }

    /// A mask containing 1s in the prefix and discriminant bit.
    #[inline(always)]
    fn prefix_discriminant_mask(&self) -> u32 {
        let relative_bit_idx = self.relative_bit_idx();
        if relative_bit_idx == 31 {
            u32::MAX
        } else {
            let r = (1 << (relative_bit_idx + 1)) - 1;
            debug_assert_ne!(r, 0);
            r
        }
    }

    #[allow(dead_code)]
    #[inline(always)]
    fn trailing_bits_mask(&self) -> u32 {
        u32::MAX << (self.relative_bit_idx() + 1)
    }
}

#[cfg(all(feature = "std", test))]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1_000_000))]
        #[test]
        fn test_branch_mask(word_idx in 0u32..8, a: u32, b: u32) {
            let mask = BranchMask::new(word_idx, a, b);

            match (mask.is_left_descendant(a),
                   mask.is_right_descendant(a),
                   mask.is_left_descendant(b),
                   mask.is_right_descendant(b)) {
                (true, false, false, true) | (false, true, true, false) => (),
                other => panic!("\n\
                                mast.relative_bit_idx: {}\n\
                                mask.left_prefix: {:032b}\n\
                                a:                {:032b}\n\
                                b:                {:032b}\n\
                                (a.left, a.right, b.left, b.right): {:?}",
                                mask.relative_bit_idx(),
                                mask.left_prefix,
                                a, b, other),

            }
        }

    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Branch<NR> {
    pub left: NR,
    pub right: NR,
    mask: BranchMask,
    /// The word at the `(bit_idx / 32) - 1`.
    /// Common to both children.
    /// Will be 0 if this node is the root.
    prior_word: u32,
    /// The the segment of the hash key from the parent branch to `prior_word`.
    /// Will be empty if the parent_branch.mask.bit_idx / 32 ==  self.mask.bit_idx / 32.
    pub prefix: Vec<u32>,
}

/// I'm counting on the compiler to optimize this out when matched immediately.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum KeyPosition {
    Right,
    Left,
    PriorWord,
    PrefixWord,
    PrefixVec {
        word_idx: usize,
        branch_word: u32,
        key_word: u32,
    },
}

impl<NR> Branch<NR> {
    #[inline(always)]
    fn descend(&self, key_hash: &KeyHash) -> KeyPosition {
        let word_idx = self.mask.bit_idx as usize / 32;
        debug_assert!(word_idx < 8);

        let prefix_offset = word_idx - self.prefix.len();

        let prefix_diff = iter::zip(self.prefix.iter(), key_hash.0.iter().skip(prefix_offset))
            .enumerate()
            .find(|(_, (branch_word, key_word))| branch_word != key_word);

        if let Some((idx, (branch_word, key_word))) = prefix_diff {
            return KeyPosition::PrefixVec {
                word_idx: idx + prefix_offset,
                branch_word: *branch_word,
                key_word: *key_word,
            };
        }

        let prior_word_idx = word_idx.wrapping_sub(1);
        let prior_word = key_hash.0.get(prior_word_idx).unwrap_or(&0);

        if self.prior_word != *prior_word {
            return KeyPosition::PriorWord;
        }

        let hash_segment = key_hash.0[word_idx];

        if self.mask.is_left_descendant(hash_segment) {
            KeyPosition::Left
        } else if self.mask.is_right_descendant(hash_segment) {
            KeyPosition::Right
        } else {
            KeyPosition::PrefixWord
        }
    }

    pub fn hash_branch(&self, left: &NodeHash, right: &NodeHash) -> NodeHash {
        let mut hasher = Sha256::new();

        hasher.update(left);
        hasher.update(right);
        hasher.update(self.mask.bit_idx.to_le_bytes());
        hasher.update(self.mask.left_prefix.to_le_bytes());
        hasher.update(self.prior_word.to_le_bytes());

        self.prefix
            .iter()
            .for_each(|word| hasher.update(word.to_le_bytes()));

        hasher.finalize().into()
    }
}

impl<V> Branch<NodeRef<V>> {
    fn new_at_branch(
        word_idx: usize,
        branch_word_or_prefix: u32,
        branch: &mut Box<Self>,
        leaf: Box<Leaf<V>>,
    ) {
        debug_assert!(word_idx < 8);

        let leaf_word = leaf.key_hash.0[word_idx];

        let prior_word = if word_idx == 0 {
            0
        } else {
            leaf.key_hash.0[word_idx - 1]
        };

        let diff = branch_word_or_prefix ^ leaf_word;
        let discriminant_bit_idx = diff.trailing_zeros();

        let mask = BranchMask {
            bit_idx: word_idx as u32 * 32 + discriminant_bit_idx,
            left_prefix: leaf_word & ((1 << discriminant_bit_idx) - 1),
        };

        debug_assert!(branch.mask.word_idx() >= word_idx);

        let prefix = if word_idx == branch.mask.word_idx() {
            debug_assert_eq!(prior_word, branch.prior_word);
            mem::take(&mut branch.prefix)
        } else if word_idx == branch.mask.word_idx() - 1 {
            mem::take(&mut branch.prefix)
        } else {
            // prefix:      [(1, _), (2, 0xF), (3, _), (4, _)] prior_word: (5, _) left_prefix: (6, _)
            // key: [(0, _), (1, _), (2, 0xA), (3, _), (4, _), (5, _), (6, _), (7, _)]
            // word_idx: 2
            // branch.word_idx: 6

            let prefix_start_idx = branch.mask.word_idx() - (branch.prefix.len() + 1);
            let check_prefix = iter::zip(
                branch.prefix.iter(),
                leaf.key_hash.0.iter().skip(prefix_start_idx),
            )
            .enumerate()
            .find(|(_, (branch_word, key_word))| branch_word != key_word)
            .map(|(idx, _)| idx + prefix_start_idx);
            debug_assert!(check_prefix.is_none());

            let mut prefix = mem::take(&mut branch.prefix);
            branch.prefix = prefix.split_off(word_idx - prefix_start_idx);
            prefix
        };

        let new_parent = Box::new(Branch {
            left: NodeRef::Stored(0),
            right: NodeRef::Stored(0),
            mask,
            prior_word,
            prefix,
        });

        let old_branch = mem::replace(branch, new_parent);

        if mask.is_left_descendant(leaf_word) {
            debug_assert!(!mask.is_right_descendant(leaf_word));

            branch.left = NodeRef::ModLeaf(leaf);
            branch.right = NodeRef::ModBranch(old_branch);
        } else {
            debug_assert!(mask.is_right_descendant(leaf_word));
            debug_assert!(!mask.is_left_descendant(leaf_word));

            branch.left = NodeRef::ModBranch(old_branch);
            branch.right = NodeRef::ModLeaf(leaf);
        };
    }

    /// Create a new branch above two leafs.
    ///
    /// # Panics
    /// Panics if the keys are the same.
    fn new_from_leafs(
        prefix_start_idx: usize,
        old_leaf: impl AsRef<Leaf<V>> + Into<NodeRef<V>>,
        new_leaf: Box<Leaf<V>>,
    ) -> Box<Self> {
        let Some((word_idx, (a, b))) = iter::zip(new_leaf.key_hash.0, old_leaf.as_ref().key_hash.0)
            .enumerate()
            .skip(prefix_start_idx)
            .find(|(_, (a, b))| a != b)
        else {
            panic!("The keys are the same")
        };

        debug_assert!(new_leaf.key_hash.0[..word_idx] == old_leaf.as_ref().key_hash.0[..word_idx]);

        let prior_word_idx = word_idx.saturating_sub(1);
        let prefix = new_leaf.key_hash.0[prefix_start_idx..prior_word_idx].to_vec();
        let prior_word = if word_idx == 0 {
            0
        } else {
            debug_assert_eq!(
                new_leaf.key_hash.0[prior_word_idx],
                old_leaf.as_ref().key_hash.0[prior_word_idx]
            );

            new_leaf.key_hash.0[prior_word_idx]
        };

        let mask = BranchMask::new(word_idx as u32, a, b);

        let (left, right) = if mask.is_left_descendant(a) {
            debug_assert!(!mask.is_right_descendant(a));

            debug_assert!(mask.is_right_descendant(b));
            debug_assert!(!mask.is_left_descendant(b));

            (new_leaf.into(), old_leaf.into())
        } else {
            debug_assert!(mask.is_right_descendant(a));
            debug_assert!(!mask.is_left_descendant(a));

            debug_assert!(mask.is_left_descendant(b));
            debug_assert!(!mask.is_right_descendant(b));

            (old_leaf.into(), new_leaf.into())
        };

        Box::new(Branch {
            left,
            right,
            mask,
            prior_word,
            prefix,
        })
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Leaf<V> {
    pub key_hash: KeyHash,
    pub value: V,
}

impl<V: AsRef<[u8]>> Leaf<V> {
    pub fn hash_leaf(&self) -> NodeHash {
        let mut hasher = Sha256::new();
        hasher.update(self.key_hash.to_bytes());
        hasher.update(self.value.as_ref());
        let hash: NodeHash = hasher.finalize().into();
        hash
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub enum TrieRoot<V> {
    #[default]
    Empty,
    Node(NodeRef<V>),
}

pub struct Transaction<S, V> {
    data_store: S,
    pub current_root: TrieRoot<V>,
}

impl<'a, Db: DatabaseSet<V>, V: Clone + AsRef<[u8]>> Transaction<SnapshotBuilder<'a, Db, V>, V> {
    /// Write modified nodes to the database and return the root hash.
    /// Calling this method will write all modified nodes to the database.
    /// Calling this method again will rewrite the nodes to the database.
    ///
    /// Caching writes is the responsibility of the `DatabaseSet` implementation.
    pub fn commit(&self) -> Result<NodeHash, String> {
        let store_modified_branch =
            &mut |hash: &NodeHash, branch: &Branch<NodeRef<V>>, left: NodeHash, right: NodeHash| {
                let branch = Branch {
                    left,
                    right,
                    mask: branch.mask,
                    prior_word: branch.prior_word,
                    prefix: branch.prefix.clone(),
                };

                self.data_store
                    .db
                    .set(*hash, Node::Branch(branch))
                    .map_err(|e| e.into())
            };

        let store_modified_leaf = &mut |hash: &NodeHash, leaf: &Leaf<V>| {
            self.data_store
                .db
                .set(*hash, Node::Leaf(leaf.clone()))
                .map_err(|e| e.into())
        };

        let root_hash = self.calc_root_hash_inner(store_modified_branch, store_modified_leaf)?;
        Ok(root_hash)
    }
}

impl<S: Store<V>, V: AsRef<[u8]>> Transaction<S, V> {
    /// TODO a version of this that writes to the database.
    pub fn calc_root_hash_inner(
        &self,
        on_modified_branch: &mut impl FnMut(
            &NodeHash,
            &Branch<NodeRef<V>>,
            NodeHash,
            NodeHash,
        ) -> Result<(), String>,
        on_modified_leaf: &mut impl FnMut(&NodeHash, &Leaf<V>) -> Result<(), String>,
    ) -> Result<NodeHash, String> {
        let root_hash = match &self.current_root {
            TrieRoot::Empty => return Ok([0; 32]),
            TrieRoot::Node(node_ref) => Self::calc_root_hash_node(
                &self.data_store,
                node_ref,
                on_modified_leaf,
                on_modified_branch,
            )?,
        };

        Ok(root_hash)
    }

    pub fn calc_root_hash(&self) -> Result<NodeHash, String> {
        self.calc_root_hash_inner(&mut |_, _, _, _| Ok(()), &mut |_, _| Ok(()))
    }

    /// TODO use this to store nodes in the data base
    fn calc_root_hash_node(
        data_store: &S,
        node_ref: &NodeRef<V>,
        on_modified_leaf: &mut impl FnMut(&NodeHash, &Leaf<V>) -> Result<(), String>,
        on_modified_branch: &mut impl FnMut(
            &NodeHash,
            &Branch<NodeRef<V>>,
            NodeHash,
            NodeHash,
        ) -> Result<(), String>,
    ) -> Result<NodeHash, String> {
        // TODO use a stack instead of recursion
        match node_ref {
            NodeRef::ModBranch(branch) => {
                let left = Self::calc_root_hash_node(
                    data_store,
                    &branch.left,
                    on_modified_leaf,
                    on_modified_branch,
                )?;
                let right = Self::calc_root_hash_node(
                    data_store,
                    &branch.right,
                    on_modified_leaf,
                    on_modified_branch,
                )?;

                let hash = branch.hash_branch(&left, &right);
                on_modified_branch(&hash, branch, left, right)?;
                Ok(hash)
            }
            NodeRef::ModLeaf(leaf) => {
                let hash = leaf.hash_leaf();

                on_modified_leaf(&hash, leaf)?;
                Ok(hash)
            }
            NodeRef::Stored(stored_idx) => {
                let hash = data_store
                    .get_unvisted_hash(*stored_idx)
                    .copied()
                    .map_err(|e| e.into())?;
                Ok(hash)
            }
        }
    }

    pub fn get(&self, key_hash: &KeyHash) -> Result<Option<&V>, String> {
        match &self.current_root {
            TrieRoot::Empty => Ok(None),
            TrieRoot::Node(node_ref) => Self::get_node(&self.data_store, node_ref, key_hash),
        }
    }

    pub fn get_node<'root, 's: 'root>(
        data_store: &'s S,
        mut node_ref: &'root NodeRef<V>,
        key_hash: &KeyHash,
    ) -> Result<Option<&'root V>, String> {
        loop {
            match node_ref {
                // TODO check that the KeyPosition is optimized out.
                NodeRef::ModBranch(branch) => match branch.descend(key_hash) {
                    KeyPosition::Left => node_ref = &branch.left,
                    KeyPosition::Right => node_ref = &branch.right,
                    KeyPosition::PriorWord
                    | KeyPosition::PrefixWord
                    | KeyPosition::PrefixVec { .. } => return Ok(None),
                },
                NodeRef::ModLeaf(leaf) => {
                    if leaf.key_hash == *key_hash {
                        return Ok(Some(&leaf.value));
                    } else {
                        return Ok(None);
                    }
                }
                NodeRef::Stored(stored_idx) => {
                    return Self::get_stored_node(data_store, *stored_idx, key_hash);
                }
            }
        }
    }

    pub fn get_stored_node<'s>(
        data_store: &'s S,
        mut stored_idx: stored::Idx,
        key_hash: &KeyHash,
    ) -> Result<Option<&'s V>, String> {
        loop {
            let node = data_store.get_node(stored_idx).map_err(|e| e.into())?;
            match node {
                // TODO check that the KeyPosition is optimized out.
                Node::Branch(branch) => match branch.descend(key_hash) {
                    KeyPosition::Left => stored_idx = branch.left,
                    KeyPosition::Right => stored_idx = branch.right,
                    KeyPosition::PriorWord
                    | KeyPosition::PrefixWord
                    | KeyPosition::PrefixVec { .. } => return Ok(None),
                },
                Node::Leaf(leaf) => {
                    if leaf.key_hash == *key_hash {
                        break;
                    } else {
                        return Ok(None);
                    }
                }
            }
        }

        match data_store.get_node(stored_idx).map_err(|e| e.into())? {
            Node::Leaf(leaf) => Ok(Some(&leaf.value)),
            _ => unreachable!("Prior loop only breaks on a leaf"),
        }
    }

    pub fn insert(&mut self, key_hash: &KeyHash, value: V) -> Result<(), String> {
        match &mut self.current_root {
            TrieRoot::Empty => {
                self.current_root = TrieRoot::Node(NodeRef::ModLeaf(Box::new(Leaf {
                    key_hash: *key_hash,
                    value,
                })));
                Ok(())
            }
            TrieRoot::Node(node_ref) => {
                Self::insert_node(&mut self.data_store, node_ref, key_hash, value)
            }
        }
    }

    fn insert_node(
        data_store: &mut S,
        root: &mut NodeRef<V>,
        key_hash: &KeyHash,
        value: V,
    ) -> Result<(), String> {
        match root {
            NodeRef::ModBranch(branch) => {
                Self::insert_below_branch(data_store, branch, key_hash, value)
            }
            NodeRef::ModLeaf(leaf) => {
                if leaf.key_hash == *key_hash {
                    leaf.value = value;
                    Ok(())
                } else {
                    let old_leaf = mem::replace(root, NodeRef::Stored(0));
                    let NodeRef::ModLeaf(old_leaf) = old_leaf else {
                        unreachable!("We just matched a ModLeaf");
                    };
                    *root = NodeRef::ModBranch(Branch::new_from_leafs(
                        0,
                        old_leaf,
                        Box::new(Leaf {
                            key_hash: *key_hash,
                            value,
                        }),
                    ));
                    Ok(())
                }
            }
            NodeRef::Stored(stored_idx) => {
                let new_node = data_store.get_node(*stored_idx).map_err(|e| e.into())?;
                match new_node {
                    stored::Node::Branch(new_branch) => {
                        *root = NodeRef::ModBranch(Box::new(Branch {
                            left: NodeRef::Stored(new_branch.left),
                            right: NodeRef::Stored(new_branch.right),
                            mask: new_branch.mask,
                            prior_word: new_branch.prior_word,
                            prefix: new_branch.prefix.clone(),
                        }));

                        let NodeRef::ModBranch(branch) = root else {
                            unreachable!("We just set root to a ModBranch");
                        };

                        Self::insert_below_branch(data_store, branch, key_hash, value)
                    }
                    stored::Node::Leaf(leaf) => {
                        *root = NodeRef::ModBranch(Branch::new_from_leafs(
                            0,
                            StoredLeafRef::new(leaf, *stored_idx),
                            Box::new(Leaf {
                                key_hash: *key_hash,
                                value,
                            }),
                        ));
                        Ok(())
                    }
                }
            }
        }
    }

    fn insert_below_branch(
        data_store: &mut S,
        mut branch: &mut Box<Branch<NodeRef<V>>>,
        key_hash: &KeyHash,
        value: V,
    ) -> Result<(), String> {
        loop {
            let next = match branch.descend(key_hash) {
                KeyPosition::Left => &mut branch.left,
                KeyPosition::Right => &mut branch.right,
                KeyPosition::PrefixWord => {
                    Branch::new_at_branch(
                        branch.mask.word_idx(),
                        branch.mask.left_prefix,
                        branch,
                        Box::new(Leaf {
                            key_hash: *key_hash,
                            value,
                        }),
                    );

                    return Ok(());
                }
                KeyPosition::PriorWord => {
                    Branch::new_at_branch(
                        branch.mask.word_idx() - 1,
                        branch.prior_word,
                        branch,
                        Box::new(Leaf {
                            key_hash: *key_hash,
                            value,
                        }),
                    );

                    return Ok(());
                }
                KeyPosition::PrefixVec {
                    word_idx,
                    branch_word,
                    key_word: _,
                } => {
                    Branch::new_at_branch(
                        word_idx,
                        branch_word,
                        branch,
                        Box::new(Leaf {
                            key_hash: *key_hash,
                            value,
                        }),
                    );

                    return Ok(());
                }
            };

            match next {
                NodeRef::ModBranch(next_branch) => {
                    branch = next_branch;
                }
                NodeRef::ModLeaf(_) => {
                    let old_next = mem::replace(next, NodeRef::Stored(0));
                    let NodeRef::ModLeaf(leaf) = old_next else {
                        unreachable!("We just matched a ModLeaf");
                    };

                    *next = NodeRef::ModBranch(Branch::new_from_leafs(
                        branch.mask.word_idx().saturating_sub(1),
                        leaf,
                        Box::new(Leaf {
                            key_hash: *key_hash,
                            value,
                        }),
                    ));

                    return Ok(());
                }
                NodeRef::Stored(stored_idx) => {
                    // TODO this is an artificial load of leaf.value.
                    let new_node = data_store.get_node(*stored_idx).map_err(|e| e.into())?;
                    match new_node {
                        stored::Node::Branch(new_branch) => {
                            *next = NodeRef::ModBranch(Box::new(Branch {
                                left: NodeRef::Stored(new_branch.left),
                                right: NodeRef::Stored(new_branch.right),
                                mask: new_branch.mask,
                                // TODO remove the clone
                                // Maybe use a AsRef<[u32]> instead of Vec<u32>
                                prior_word: new_branch.prior_word,
                                prefix: new_branch.prefix.clone(),
                            }));

                            let NodeRef::ModBranch(next_branch) = next else {
                                unreachable!("We just set next to a ModBranch");
                            };

                            branch = next_branch;
                        }
                        stored::Node::Leaf(leaf) => {
                            *next = NodeRef::ModBranch(Branch::new_from_leafs(
                                branch.mask.word_idx() - 1,
                                StoredLeafRef::new(leaf, *stored_idx),
                                Box::new(Leaf {
                                    key_hash: *key_hash,
                                    value,
                                }),
                            ));
                            return Ok(());
                        }
                    }
                }
            }
        }
    }
}

impl<'a, Db, V> Transaction<SnapshotBuilder<'a, Db, V>, V> {
    /// An alias for `SnapshotBuilder::new_with_db`.
    ///
    /// Builds a snapshot of the trie before the transaction.
    /// The `Snapshot` is not a complete representation of the trie.
    /// The `Snapshot` only contains information about the parts of the trie touched by the transaction.
    /// Because of this, two `Snapshot`s of the same trie may not be equal if the transactions differ.
    ///
    /// Note: All operations including get affect the contents of the snapshot.
    pub fn build_initial_snapshot(&self) -> Snapshot<V>
    where
        V: Clone,
    {
        self.data_store.build_initial_snapshot()
    }

    pub fn from_snapshot_builder(builder: SnapshotBuilder<'a, Db, V>) -> Self {
        Transaction {
            current_root: builder.trie_root(),
            data_store: builder,
        }
    }
}

impl<'s, V: AsRef<[u8]>> Transaction<&'s Snapshot<V>, V> {
    pub fn from_snapshot(snapshot: &'s Snapshot<V>) -> Result<Self, String> {
        Ok(Transaction {
            current_root: snapshot.trie_root()?,
            data_store: snapshot,
        })
    }
}

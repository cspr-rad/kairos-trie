use alloc::{boxed::Box, vec::Vec};
use core::{iter, mem};

use crate::{hash::PortableHasher, stored, KeyHash, NodeHash, PortableHash, PortableUpdate};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub enum TrieRoot<T> {
    #[default]
    Empty,
    Node(T),
}

impl From<NodeHash> for TrieRoot<NodeHash> {
    #[inline]
    fn from(hash: NodeHash) -> Self {
        Self::Node(hash)
    }
}

impl From<Option<NodeHash>> for TrieRoot<NodeHash> {
    #[inline]
    fn from(hash: Option<NodeHash>) -> Self {
        match hash {
            Some(hash) => Self::Node(hash),
            None => Self::Empty,
        }
    }
}

impl From<TrieRoot<NodeHash>> for Option<NodeHash> {
    #[inline]
    fn from(value: TrieRoot<NodeHash>) -> Self {
        match value {
            TrieRoot::Empty => None,
            TrieRoot::Node(hash) => Some(hash),
        }
    }
}

/// A unmodified Node
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Node<B, L> {
    Branch(B),
    Leaf(L),
}

/// A Node representation which may be partially modified.
/// `ModBranch` and `ModLeaf` are used to represent a node which has been modified in the current transaction.
/// `Stored` is used to represent an unmodified node stored in the database.
/// `Stored`s `Idx` represents a reference to a `Node` in the database.
/// When executing in zkVM where a `Snapshot` is the DB, this is an in memory `Node`.
/// When executing against a `SnapshotBuilder`, it's a reference to a `NodeHash`,
/// which can in turn be used to retrieve the `Node`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum NodeRef<V> {
    ModBranch(Box<Branch<Self>>),
    ModLeaf(Box<Leaf<V>>),
    Stored(stored::Idx),
}

impl<V> From<Box<Branch<NodeRef<V>>>> for NodeRef<V> {
    #[inline]
    fn from(branch: Box<Branch<NodeRef<V>>>) -> Self {
        NodeRef::ModBranch(branch)
    }
}

impl<V> From<Box<Leaf<V>>> for NodeRef<V> {
    #[inline]
    fn from(leaf: Box<Leaf<V>>) -> Self {
        NodeRef::ModLeaf(leaf)
    }
}

pub struct StoredLeafRef<'s, V> {
    leaf: &'s Leaf<V>,
    stored: stored::Idx,
}

impl<'s, V> From<StoredLeafRef<'s, V>> for NodeRef<V> {
    #[inline]
    fn from(leaf: StoredLeafRef<'s, V>) -> Self {
        NodeRef::Stored(leaf.stored)
    }
}

impl<'s, V> AsRef<Leaf<V>> for StoredLeafRef<'s, V> {
    #[inline]
    fn as_ref(&self) -> &Leaf<V> {
        self.leaf
    }
}

impl<'s, V> StoredLeafRef<'s, V> {
    #[inline]
    pub fn new(leaf: &'s Leaf<V>, stored: stored::Idx) -> Self {
        Self { leaf, stored }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct BranchMask {
    /// The index of the discriminant bit in the 256 bit hash key.
    pub bit_idx: u32,

    /// Common prefix of word at `bit_idx / 32`, a 0 discriminant bit, and trailing 0s
    pub left_prefix: u32,
}

impl BranchMask {
    #[inline]
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
    pub fn right_prefix(&self) -> u32 {
        self.left_prefix | self.discriminant_bit_mask()
    }

    #[inline(always)]
    pub fn is_left_descendant(&self, hash_segment: u32) -> bool {
        (hash_segment & self.prefix_discriminant_mask()) == self.left_prefix
    }

    #[inline(always)]
    pub fn is_right_descendant(&self, hash_segment: u32) -> bool {
        (hash_segment & self.prefix_discriminant_mask()) == self.right_prefix()
    }

    #[inline(always)]
    pub fn word_idx(&self) -> usize {
        (self.bit_idx / 32) as usize
    }

    /// The index of the discriminant bit in the `left_prefix`.
    #[inline(always)]
    pub fn relative_bit_idx(&self) -> u32 {
        let r = self.bit_idx % 32;
        debug_assert!(r < 32);
        r
    }

    #[inline(always)]
    pub fn discriminant_bit_mask(&self) -> u32 {
        1 << self.relative_bit_idx()
    }

    /// A mask containing 1s in the prefix and discriminant bit.
    #[inline(always)]
    pub fn prefix_discriminant_mask(&self) -> u32 {
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
    pub fn trailing_bits_mask(&self) -> u32 {
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
    pub mask: BranchMask,
    /// The word at the `(bit_idx / 32) - 1`.
    /// Common to both children.
    /// Will be 0 if this node is the root.
    pub prior_word: u32,
    /// The the segment of the hash key from the parent branch to `prior_word`.
    /// Will be empty if the parent_branch.mask.bit_idx / 32 ==  self.mask.bit_idx / 32.
    pub prefix: Vec<u32>,
}

/// I'm counting on the compiler to optimize this out when matched immediately.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum KeyPosition {
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
    pub fn descend(&self, key_hash: &KeyHash) -> KeyPosition {
        let word_idx = self.mask.bit_idx as usize / 32;
        debug_assert!(word_idx < 8);

        debug_assert!(self.prefix.len() <= word_idx);
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

        // If sub wraps around to the last word, the prior word is 0.
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

    #[inline]
    pub fn hash_branch<H: PortableHasher<32>>(
        &self,
        hasher: &mut H,
        left: &NodeHash,
        right: &NodeHash,
    ) -> NodeHash {
        hasher.portable_update(left);
        hasher.portable_update(right);
        hasher.portable_update(self.mask.bit_idx.to_le_bytes());
        hasher.portable_update(self.mask.left_prefix.to_le_bytes());
        hasher.portable_update(self.prior_word.to_le_bytes());

        self.prefix
            .iter()
            .for_each(|word| hasher.portable_update(word.to_le_bytes()));

        NodeHash::new(hasher.finalize_reset())
    }
}

impl<V> Branch<NodeRef<V>> {
    pub(crate) fn from_stored(branch: &Branch<stored::Idx>) -> Branch<NodeRef<V>> {
        Branch {
            left: NodeRef::Stored(branch.left),
            right: NodeRef::Stored(branch.right),
            mask: branch.mask,
            prior_word: branch.prior_word,
            // TODO remove the clone
            // Maybe use a AsRef<[u32]> instead of Vec<u32>
            prefix: branch.prefix.clone(),
        }
    }

    /// A wrapper around `new_at_branch_ret` which returns nothing.
    /// This exists to aid compiler inlining.
    #[inline]
    pub(crate) fn new_at_branch(
        word_idx: usize,
        branch_word_or_prefix: u32,
        branch: &mut Box<Self>,
        leaf: Box<Leaf<V>>,
    ) {
        Self::new_at_branch_ret(word_idx, branch_word_or_prefix, branch, leaf);
    }

    /// Store a new leaf adjacent to an existing branch.
    /// New branch will be stored in the old branch's Box.
    /// The old branch will be moved to a new Box, under the new branch.
    // inline(always) is used to increace the odds of the compiler removing the return when unused.
    #[inline(always)]
    pub(crate) fn new_at_branch_ret(
        word_idx: usize,
        branch_word_or_prefix: u32,
        branch: &mut Box<Self>,
        leaf: Box<Leaf<V>>,
    ) -> &mut Leaf<V> {
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

        let r = if mask.is_left_descendant(leaf_word) {
            debug_assert!(!mask.is_right_descendant(leaf_word));

            branch.left = NodeRef::ModLeaf(leaf);
            branch.right = NodeRef::ModBranch(old_branch);

            &mut branch.left
        } else {
            debug_assert!(mask.is_right_descendant(leaf_word));
            debug_assert!(!mask.is_left_descendant(leaf_word));

            branch.left = NodeRef::ModBranch(old_branch);
            branch.right = NodeRef::ModLeaf(leaf);

            &mut branch.right
        };

        match r {
            NodeRef::ModLeaf(leaf) => leaf,
            _ => unreachable!(),
        }
    }

    /// Create a new branch above two leafs.
    /// Returns the new branch and a bool indicating if the new leaf is the right child.
    ///
    /// # Panics
    /// Panics if the keys are the same.
    #[inline]
    pub(crate) fn new_from_leafs(
        prefix_start_idx: usize,
        old_leaf: impl AsRef<Leaf<V>> + Into<NodeRef<V>>,
        new_leaf: Box<Leaf<V>>,
    ) -> (Box<Self>, bool) {
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

        let (left, right, is_right) = if mask.is_left_descendant(a) {
            debug_assert!(!mask.is_right_descendant(a));

            debug_assert!(mask.is_right_descendant(b));
            debug_assert!(!mask.is_left_descendant(b));

            (new_leaf.into(), old_leaf.into(), false)
        } else {
            debug_assert!(mask.is_right_descendant(a));
            debug_assert!(!mask.is_left_descendant(a));

            debug_assert!(mask.is_left_descendant(b));
            debug_assert!(!mask.is_right_descendant(b));

            (old_leaf.into(), new_leaf.into(), true)
        };

        (
            Box::new(Branch {
                left,
                right,
                mask,
                prior_word,
                prefix,
            }),
            // TODO use an enum
            is_right,
        )
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Leaf<V> {
    pub key_hash: KeyHash,
    pub value: V,
}

impl<V: PortableHash> PortableHash for Leaf<V> {
    #[inline]
    fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
        hasher.portable_update(self.key_hash.to_bytes());
        self.value.portable_hash(hasher);
    }
}

impl<V: PortableHash> Leaf<V> {
    #[inline]
    pub fn hash_leaf<H: PortableHasher<32>>(&self, hasher: &mut H) -> NodeHash {
        hasher.portable_update(self.key_hash.to_bytes());
        self.value.portable_hash(hasher);
        NodeHash::new(hasher.finalize_reset())
    }
}

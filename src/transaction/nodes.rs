use alloc::boxed::Box;
use core::{fmt, iter, mem};

use crate::{hash::PortableHasher, stored, KeyHash, NodeHash, PortableHash, PortableUpdate};

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

impl From<TrieRoot<NodeHash>> for Option<[u8; 32]> {
    #[inline]
    fn from(value: TrieRoot<NodeHash>) -> Self {
        match value {
            TrieRoot::Empty => None,
            TrieRoot::Node(hash) => Some(hash.bytes),
        }
    }
}

impl From<Option<[u8; 32]>> for TrieRoot<NodeHash> {
    #[inline]
    fn from(hash: Option<[u8; 32]>) -> Self {
        match hash {
            Some(hash) => Self::Node(NodeHash::new(hash)),
            None => Self::Empty,
        }
    }
}

/// A unmodified Node
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum NodeRef<V> {
    ModBranch(Box<Branch<Self>>),
    ModLeaf(Box<Leaf<V>>),
    Stored(stored::Idx),
}

impl<V> NodeRef<V> {
    #[inline(always)]
    pub fn temp_null_stored() -> Self {
        NodeRef::Stored(u32::MAX)
    }
}

impl<V> fmt::Debug for NodeRef<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ModBranch(b) => f.debug_tuple("ModBranch").field(b).finish(),
            Self::ModLeaf(l) => f.debug_tuple("ModLeaf").field(l).finish(),
            Self::Stored(idx) => f.debug_tuple("Stored").field(idx).finish(),
        }
    }
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

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct BranchMask {
    /// The index of the discriminant bit in the 256 bit hash key.
    bit_idx: u32,

    /// Common prefix of word at `bit_idx / 32`, a 0 discriminant bit, and trailing 0s
    left_prefix: u32,
}

impl BranchMask {
    pub const fn new(word_idx: u32, a: u32, b: u32) -> Self {
        Self::new_inner(word_idx, a, a ^ b)
    }

    #[inline(always)]
    pub const fn new_with_mask(word_idx: u32, a: u32, b: u32, prefix_mask: u32) -> Self {
        Self::new_inner(word_idx, a, (a ^ b) & prefix_mask)
    }

    #[inline(always)]
    const fn new_inner(word_idx: u32, a: u32, diff: u32) -> Self {
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
    pub const fn right_prefix(&self) -> u32 {
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
    pub const fn word_idx(&self) -> usize {
        (self.bit_idx / 32) as usize
    }

    /// The index of the discriminant bit in the `left_prefix`.
    #[inline(always)]
    pub const fn relative_bit_idx(&self) -> u32 {
        let r = self.bit_idx % 32;
        debug_assert!(r < 32);
        r
    }

    #[inline(always)]
    pub const fn discriminant_bit_mask(&self) -> u32 {
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

    #[inline(always)]
    pub fn prefix_mask(&self) -> u32 {
        self.prefix_discriminant_mask() ^ self.discriminant_bit_mask()
    }

    #[inline(always)]
    pub const fn trailing_bits_mask(&self) -> u32 {
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

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
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
    pub prefix: Box<[u32]>,
}

impl<NR> fmt::Debug for Branch<NR> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Branch")
            .field("mask", &self.mask)
            .field("prior_word", &self.prior_word)
            .field("prefix", &self.prefix)
            .finish()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum KeyPosition {
    Adjacent(KeyPositionAdjacent),
    Right,
    Left,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum KeyPositionAdjacent {
    /// The delta bit occurs before the existing branch's discriminant bit in the same word.
    /// `branch.mask.word_idx() == PrefixOfWord(word_idx)`.
    PrefixOfWord(usize),
    /// The delta bit occurs in the word prior to the existing branch's discriminant bit.
    /// `PrefixOfWord(word_idx) == branch.mask.word_idx() - 1`
    /// `PrefixOfWord(word_idx) != 0`.
    PriorWord(usize),
    /// The delta bit occurs in the prefix vector.
    /// `branch.mask.word_idx() - PrefixVec(word_idx) >= 2`.
    PrefixVec(usize),
}

impl<NR> Branch<NR> {
    /// Returns the position of the key relative to the branch.
    #[inline(always)]
    pub fn key_position(&self, key_hash: &KeyHash) -> KeyPosition {
        let word_idx = self.mask.bit_idx as usize / 32;
        debug_assert!(word_idx < 8);

        debug_assert!(self.prefix.len() <= word_idx);
        let prefix_offset = word_idx.saturating_sub(self.prefix.len() + 1);

        let prefix_diff = iter::zip(
            self.prefix.iter(),
            key_hash.0.iter().enumerate().skip(prefix_offset),
        )
        .find(|(branch_word, (_, key_word))| branch_word != key_word);

        if let Some((_, (idx, _))) = prefix_diff {
            return KeyPosition::Adjacent(KeyPositionAdjacent::PrefixVec(idx));
        }

        // If sub wraps around to the last word, the prior word is 0.
        let prior_word_idx = word_idx.wrapping_sub(1);
        let prior_word = key_hash.0.get(prior_word_idx).unwrap_or(&0);

        if self.prior_word != *prior_word {
            return KeyPosition::Adjacent(KeyPositionAdjacent::PriorWord(prior_word_idx));
        }

        let hash_segment = key_hash.0[word_idx];

        if self.mask.is_left_descendant(hash_segment) {
            KeyPosition::Left
        } else if self.mask.is_right_descendant(hash_segment) {
            KeyPosition::Right
        } else {
            KeyPosition::Adjacent(KeyPositionAdjacent::PrefixOfWord(word_idx))
        }
    }

    /// Hash a branch node with known child hashes.
    ///
    /// Caller must ensure that the hasher is reset before calling this function.
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
            // Maybe use a AsRef<[u32]> instead of Box<[u32]>
            prefix: branch.prefix.clone(),
        }
    }

    /// A wrapper around `new_at_branch_ret` which returns nothing.
    /// This exists to aid compiler inlining.
    ///
    /// `key_position` must come from `branch.key_position(leaf.key_hash)`.
    #[inline]
    pub(crate) fn new_adjacent_leaf(
        self: &mut Box<Self>,
        key_position: KeyPositionAdjacent,
        leaf: Box<Leaf<V>>,
    ) {
        self.new_adjacent_leaf_ret(key_position, leaf);
    }

    /// Store a new leaf adjacent to an existing branch.
    /// New branch will be stored in the old branch's Box.
    /// The old branch will be moved to a new Box, under the new branch.
    // inline(always) is used to increase the odds of the compiler removing the return when unused.
    #[inline(always)]
    pub(crate) fn new_adjacent_leaf_ret<'a>(
        self: &'a mut Box<Self>,
        key_position: KeyPositionAdjacent,
        leaf: Box<Leaf<V>>,
    ) -> &'a mut Leaf<V> {
        let (mask, prior_word, prefix, leaf_word) = match key_position {
            KeyPositionAdjacent::PrefixOfWord(word_idx) => {
                debug_assert_eq!(self.mask.word_idx(), word_idx);

                let branch_word = self.mask.left_prefix;
                let leaf_word = leaf.key_hash.0[word_idx];

                let mask = BranchMask::new_with_mask(
                    word_idx as u32,
                    branch_word,
                    leaf_word,
                    self.mask.prefix_mask(),
                );

                debug_assert_eq!(
                    self.prior_word,
                    word_idx
                        .checked_sub(1)
                        .map(|i| leaf.key_hash.0[i])
                        .unwrap_or(0)
                );

                (
                    mask,
                    self.prior_word,
                    mem::take(&mut self.prefix),
                    leaf_word,
                )
            }
            KeyPositionAdjacent::PriorWord(word_idx) => {
                debug_assert_eq!(word_idx, self.mask.word_idx() - 1);

                let branch_word = self.prior_word;
                let leaf_word = leaf.key_hash.0[word_idx];

                let mask = BranchMask::new(word_idx as u32, branch_word, leaf_word);

                // If sub wraps around to the last word, the prior word is 0.
                // This is a little optimization since we are already paying for a bounds check.
                let prior_word_idx = word_idx.wrapping_sub(1);
                let prior_word = leaf.key_hash.0.get(prior_word_idx).unwrap_or(&0);

                (mask, *prior_word, mem::take(&mut self.prefix), leaf_word)
            }
            KeyPositionAdjacent::PrefixVec(word_idx) => {
                debug_assert!(self.mask.word_idx() - word_idx >= 2);
                debug_assert!(!self.prefix.is_empty());

                // we don't include word or prior_word in the prefix
                let key_prefix = &leaf.key_hash.0[..word_idx.saturating_sub(1)];
                let delta_in_prefix = key_prefix
                    .iter()
                    .rev()
                    .zip(self.prefix.iter().rev())
                    .enumerate()
                    .find(|(_, (key_word, branch_word))| key_word != branch_word);

                debug_assert_eq!(delta_in_prefix, None);

                let prefix_offset = word_idx.saturating_sub(self.prefix.len() + 1);

                let new_prefix = leaf.key_hash.0[prefix_offset..word_idx.saturating_sub(1)].into();
                let old_prefix = self.prefix[word_idx + 1 - prefix_offset..].into();

                let branch_word = self.prefix[word_idx - prefix_offset];
                let leaf_word = leaf.key_hash.0[word_idx];
                let mask = BranchMask::new(word_idx as u32, branch_word, leaf_word);

                let prior_word_idx = word_idx.wrapping_sub(1);
                let prior_word = leaf.key_hash.0.get(prior_word_idx).unwrap_or(&0);

                self.prefix = old_prefix;

                (mask, *prior_word, new_prefix, leaf_word)
            }
        };

        let new_parent = Box::new(Branch {
            left: NodeRef::temp_null_stored(),
            right: NodeRef::temp_null_stored(),
            mask,
            prior_word,
            prefix,
        });

        let old_branch = mem::replace(self, new_parent);

        let r = if mask.is_left_descendant(leaf_word) {
            debug_assert!(!mask.is_right_descendant(leaf_word));

            self.left = NodeRef::ModLeaf(leaf);
            self.right = NodeRef::ModBranch(old_branch);

            &mut self.left
        } else {
            debug_assert!(mask.is_right_descendant(leaf_word));
            debug_assert!(!mask.is_left_descendant(leaf_word));

            self.left = NodeRef::ModBranch(old_branch);
            self.right = NodeRef::ModLeaf(leaf);

            &mut self.right
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
        let prefix = new_leaf.key_hash.0[prefix_start_idx..prior_word_idx].into();
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

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Leaf<V> {
    pub key_hash: KeyHash,
    pub value: V,
}

impl<V> fmt::Debug for Leaf<V> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Leaf")
            .field("key_hash", &self.key_hash)
            .finish()
    }
}

impl<V: PortableHash> PortableHash for Leaf<V> {
    #[inline]
    fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
        hasher.portable_update(self.key_hash.to_bytes());
        self.value.portable_hash(hasher);
    }
}

impl<V: PortableHash> Leaf<V> {
    /// Hash a leaf node.
    ///
    /// Caller must ensure that the hasher is reset before calling this function.
    #[inline]
    pub fn hash_leaf<H: PortableHasher<32>>(&self, hasher: &mut H) -> NodeHash {
        hasher.portable_update(self.key_hash.to_bytes());
        self.value.portable_hash(hasher);
        NodeHash::new(hasher.finalize_reset())
    }
}

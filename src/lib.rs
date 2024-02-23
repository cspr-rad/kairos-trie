#![allow(clippy::type_complexity)]
#![cfg_attr(not(feature = "std"), no_std)]
#[cfg(not(feature = "std"))]
extern crate core as std;

extern crate alloc;

pub mod modified;
pub mod stored;

use std::{iter, mem};

use alloc::{boxed::Box, string::String, vec::Vec};
pub use modified::*;
use stored::Node;
pub use stored::Store;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct KeyHash(pub [u32; 8]);

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
        let relative_bit_idx = diff.leading_zeros();

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
        self.bit_idx % 32
    }

    #[inline(always)]
    fn discriminant_bit_mask(&self) -> u32 {
        1 << self.relative_bit_idx()
    }

    /// A mask containing 1s in the prefix and discriminant bit.
    #[inline(always)]
    fn prefix_discriminant_mask(&self) -> u32 {
        (1 << (self.relative_bit_idx() + 1)) - 1
    }

    #[allow(dead_code)]
    #[inline(always)]
    fn trailing_bits_mask(&self) -> u32 {
        u32::MAX << (self.relative_bit_idx() + 1)
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
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
    fn desend(&self, key_hash: &KeyHash) -> KeyPosition {
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
}

impl<V> Branch<NodeRef<V>> {
    #[allow(dead_code)]
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
        let discriminant_bit_idx = diff.leading_zeros();

        let mask = BranchMask {
            bit_idx: word_idx as u32 * 32 + discriminant_bit_idx,
            left_prefix: leaf_word & ((1 << discriminant_bit_idx) - 1),
        };

        debug_assert!(branch.mask.word_idx() >= word_idx);

        debug_assert_eq!(
            branch.prior_word,
            leaf.key_hash.0[branch.mask.word_idx() - 1]
        );

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

    #[allow(dead_code)]
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
            .skip(prefix_start_idx)
            .enumerate()
            .find(|(_, (a, b))| a != b)
        else {
            panic!("The keys are the same")
        };

        debug_assert!(new_leaf.key_hash.0[..word_idx] == old_leaf.as_ref().key_hash.0[..word_idx]);

        let prefix = new_leaf.key_hash.0[prefix_start_idx..word_idx - 1].to_vec();
        let prior_word = if word_idx == 0 {
            0
        } else {
            debug_assert_eq!(
                new_leaf.key_hash.0[word_idx - 1],
                old_leaf.as_ref().key_hash.0[word_idx - 1]
            );

            new_leaf.key_hash.0[word_idx - 1]
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
        loop {
            match node_ref {
                // TODO check that the KeyPosition is optimized out.
                NodeRef::ModBranch(branch) => match branch.desend(key_hash) {
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
        data_store: &'s mut S,
        mut stored_idx: stored::Idx,
        key_hash: &KeyHash,
    ) -> Result<Option<&'s V>, String> {
        loop {
            let node = data_store.get_node(stored_idx).map_err(|e| e.into())?;
            match node {
                // TODO check that the KeyPosition is optimized out.
                Node::Branch(branch) => match branch.desend(key_hash) {
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
            let next = match branch.desend(key_hash) {
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
                        branch.mask.word_idx() - 1,
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

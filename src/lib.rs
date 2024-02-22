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
    /// If the mask is 1 it represents the divergent bit not a single divergent suffix bit.
    ///
    /// bit_idx_mask = prefix_discriminant_suffix & discriminant_trailing_bits_mask;
    /// bit_idx = bit_idx_mask.leading_zeros() == discriminant_trailing_bits_mask.leading_zeros()
    discriminant_trailing_bits_mask: u32,
    pub left: NR,
    pub right: NR,

    /// The the segment of the hash key from the last branch to this branch.
    /// Will be empty if the last branch had no trailing bits.
    pub prefix: Vec<u32>,
}

impl<NR> Branch<NR> {
    fn discriminant_bit_mask(&self) -> u32 {
        self.prefix_discriminant_suffix & self.discriminant_trailing_bits_mask
    }

    fn discriminant_bit_idx(&self) -> u32 {
        self.discriminant_trailing_bits_mask.leading_zeros()
    }

    #[allow(dead_code)]
    fn prefix_mask(&self) -> u32 {
        self.discriminant_bit_mask() - 1
    }

    fn is_right_descendant(&self, hash_segment: u32) -> bool {
        let trailing_bits_mask = self.trailing_bits_mask();

        (hash_segment | trailing_bits_mask)
            == (self.prefix_discriminant_suffix | trailing_bits_mask)
    }

    fn is_left_descendant(&self, hash_segment: u32) -> bool {
        let inverted_discriminant = hash_segment ^ self.prefix_discriminant_suffix;

        (inverted_discriminant | self.trailing_bits_mask()) == self.discriminant_trailing_bits_mask
    }

    fn trailing_bits_mask(&self) -> u32 {
        self.discriminant_bit_mask() ^ self.discriminant_trailing_bits_mask
    }

    fn no_trailing_bits(&self) -> bool {
        let r = self.trailing_bits_mask() == 0;
        debug_assert!(
            !r && self.prefix.is_empty(),
            "A branch with trailing bits cannot have an extension"
        );

        r
    }
}

impl<V> Branch<NodeRef<V>> {
    fn new_at_branch(branch: Box<Self>, leaf: Box<Leaf<V>>) {
        let diff = branch.prefix_discriminant_suffix ^ leaf.key_hash.0[0];
        debug_assert!(diff != 0);

        let discriminant_bit_idx = diff.leading_zeros();
        debug_assert!(
            discriminant_bit_idx < branch.discriminant_bit_idx(),
            "This leaf is not a descendant of the branch"
        );
    }

    /// Create a new branch.
    ///
    /// prefix_start_idx is the index of the prior branch if the prior branch has trailing bits.
    ///
    /// # Panics
    /// Panics if the keys are the same.
    pub fn from_hashed_key_bits(
        prefix_start_idx: usize,
        a_leaf: Box<Leaf<V>>,
        b_leaf: Box<Leaf<V>>,
    ) -> Self {
        let Some((word_idx, (a, b))) = iter::zip(a_leaf.key_hash.0, b_leaf.key_hash.0)
            .skip(prefix_start_idx)
            .enumerate()
            .find(|(_, (a, b))| a != b)
        else {
            panic!("The keys are the same")
        };
        let prefix = a_leaf.key_hash.0[prefix_start_idx..word_idx].to_vec();
        debug_assert_eq!(prefix, b_leaf.key_hash.0[prefix_start_idx..word_idx]);

        let diff = a ^ b;
        let discriminant_bit_idx = diff.leading_zeros();
        let discriminant_bit_mask = 1 << discriminant_bit_idx;

        let trailing_bits_idx = (diff ^ discriminant_bit_mask).leading_zeros();
        let not_trailing_bits_mask = (1 << trailing_bits_idx) - 1;

        let prefix_discriminant_suffix = (a & not_trailing_bits_mask) | discriminant_bit_mask;

        let trailing_bits_mask = !not_trailing_bits_mask;

        let discriminant_trailing_bits_mask = trailing_bits_mask | discriminant_bit_mask;

        let branch = Branch {
            bit_idx: 0,
            prefix_discriminant_suffix,
            discriminant_trailing_bits_mask,
            left: (),
            right: (),
            prefix: Vec::new(),
        };

        let (left, right) = if branch.is_left_descendant(a) {
            debug_assert!(!branch.is_right_descendant(a));

            debug_assert!(branch.is_right_descendant(b));
            debug_assert!(!branch.is_left_descendant(b));

            (a_leaf, b_leaf)
        } else {
            debug_assert!(branch.is_right_descendant(a));
            debug_assert!(!branch.is_left_descendant(a));

            debug_assert!(branch.is_left_descendant(b));
            debug_assert!(!branch.is_right_descendant(b));

            (b_leaf, a_leaf)
        };

        Branch {
            bit_idx: 0,
            prefix_discriminant_suffix,
            discriminant_trailing_bits_mask,
            left: NodeRef::ModLeaf(left),
            right: NodeRef::ModLeaf(right),
            prefix,
        }
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
        let mut word_idx = 0;
        loop {
            debug_assert!(word_idx < 8);

            match node_ref {
                NodeRef::ModBranch(branch) => {
                    let check_prefix =
                        iter::zip(branch.prefix.iter(), key_hash.0.iter().skip(word_idx))
                            .all(|(a, b)| a == b);

                    if check_prefix {
                        word_idx += branch.prefix.len();
                    } else {
                        return Ok(None);
                    }

                    let hash_segment = key_hash.0[word_idx];

                    let next = if branch.is_right_descendant(hash_segment) {
                        &branch.right
                    } else if branch.is_left_descendant(hash_segment) {
                        &branch.left
                    } else {
                        return Ok(None);
                    };

                    // advance to the next word if this one is fully matched
                    if branch.no_trailing_bits() {
                        word_idx += 1;
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
                    let check_prefix =
                        iter::zip(branch.prefix.iter(), key_hash.0.iter().skip(word_idx))
                            .all(|(a, b)| a == b);

                    if check_prefix {
                        word_idx += branch.prefix.len();
                    } else {
                        return Ok(None);
                    }

                    let hash_segment = key_hash.0[word_idx];

                    stored_idx = if branch.is_right_descendant(hash_segment) {
                        branch.right
                    } else if branch.is_left_descendant(hash_segment) {
                        branch.left
                    } else {
                        return Ok(None);
                    };

                    // advance to the next word if this one is matched
                    if branch.no_trailing_bits() {
                        word_idx += 1
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
        let mut node_ref = root;
        let mut word_idx = 0;

        todo!()
    }

    fn insert_below_branch(
        data_store: &mut S,
        mut branch: &mut Box<Branch<NodeRef<V>>>,
        key_hash: &KeyHash,
        value: V,
    ) -> Result<(), String> {
        let mut word_idx = 0;
        loop {
            debug_assert!(word_idx < 8);
            let check_prefix = iter::zip(branch.prefix.iter(), key_hash.0.iter().skip(word_idx))
                .all(|(a, b)| a == b);

            if check_prefix {
                word_idx += branch.prefix.len();
            } else {
                todo!()
            }

            let hash_segment = key_hash.0[word_idx];

            // advance to the next word if this one is matched
            if branch.no_trailing_bits() {
                word_idx += 1
            } else {
                return Ok(());
            }

            let next = if branch.is_right_descendant(hash_segment) {
                &mut branch.right
            } else if branch.is_left_descendant(hash_segment) {
                &mut branch.left
            } else {
                todo!()
            };

            match next {
                NodeRef::ModBranch(next_branch) => {
                    branch = next_branch;
                }
                NodeRef::ModLeaf(leaf) => {
                    todo!()
                }
                NodeRef::Stored(stored_idx) => {
                    // TODO this is an artificial load of leaf.value.
                    let new_node = data_store.get_node(*stored_idx).map_err(|e| e.into())?;
                    match new_node {
                        stored::Node::Branch(new_branch) => {
                            *next = NodeRef::ModBranch(Box::new(Branch {
                                left: NodeRef::Stored(new_branch.left),
                                right: NodeRef::Stored(new_branch.right),
                                bit_idx: new_branch.bit_idx,
                                prefix_discriminant_suffix: new_branch.prefix_discriminant_suffix,
                                discriminant_trailing_bits_mask: new_branch
                                    .discriminant_trailing_bits_mask,
                                // TODO remove the clone
                                // Maybe use a AsRef<[u32]> instead of Vec<u32>
                                prefix: new_branch.prefix.clone(),
                            }));

                            let NodeRef::ModBranch(next_branch) = next else {
                                unreachable!("We just set next to a ModBranch");
                            };

                            branch = next_branch;
                        }
                        stored::Node::Leaf(leaf) => {
                            todo!()
                        }
                    }
                }
            }
        }
    }
}

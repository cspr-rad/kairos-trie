use alloc::boxed::Box;

use crate::{stored, Branch, Leaf};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum NodeRef<V> {
    ModBranch(Box<Branch<Self>>),
    ModLeaf(Box<Leaf<V>>),

    Stored(stored::Idx),
}

impl<V> From<Box<Branch<NodeRef<V>>>> for NodeRef<V> {
    fn from(branch: Box<Branch<NodeRef<V>>>) -> Self {
        NodeRef::ModBranch(branch)
    }
}

impl<V> From<Box<Leaf<V>>> for NodeRef<V> {
    fn from(leaf: Box<Leaf<V>>) -> Self {
        NodeRef::ModLeaf(leaf)
    }
}

pub struct StoredLeafRef<'s, V> {
    leaf: &'s Leaf<V>,
    stored: stored::Idx,
}

impl<'s, V> From<StoredLeafRef<'s, V>> for NodeRef<V> {
    fn from(leaf: StoredLeafRef<'s, V>) -> Self {
        NodeRef::Stored(leaf.stored)
    }
}

impl<'s, V> AsRef<Leaf<V>> for StoredLeafRef<'s, V> {
    fn as_ref(&self) -> &Leaf<V> {
        self.leaf
    }
}

impl<'s, V> StoredLeafRef<'s, V> {
    pub fn new(leaf: &'s Leaf<V>, stored: stored::Idx) -> Self {
        Self { leaf, stored }
    }
}

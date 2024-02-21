use alloc::boxed::Box;

use crate::{stored, Branch, Leaf};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum NodeRef<V> {
    ModBranch(Box<Branch<Self>>),
    ModLeaf(Box<Leaf<V>>),

    Stored(stored::Idx),
}

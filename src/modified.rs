use alloc::boxed::Box;

use crate::{stored, Branch, Extension, Leaf};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum NodeRef<V> {
    ModBranch(Box<Branch<Self>>),
    ModExtension(Box<Extension<V>>),
    ModLeaf(Box<Leaf<V>>),

    StoredBranch(stored::Idx),
}

use alloc::boxed::Box;

use crate::{stored, Branch, Extension, Leaf};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum NodeRef<B, E, L, V> {
    ModBranch(Box<Branch<Self>>),
    ModExtension(Box<Extension<B, E, L, V>>),
    ModLeaf(Box<Leaf<V>>),

    StoredBranch(B),
    StoredExtension(E),
    StoredLeaf(L),
}

impl<B, E, L, V> From<stored::Node<B, E, L>> for NodeRef<B, E, L, V> {
    fn from(node_ref: stored::Node<B, E, L>) -> Self {
        match node_ref {
            stored::Node::Branch(branch) => NodeRef::StoredBranch(branch),
            stored::Node::Extension(extension) => NodeRef::StoredExtension(extension),
            stored::Node::Leaf(leaf) => NodeRef::StoredLeaf(leaf),
        }
    }
}

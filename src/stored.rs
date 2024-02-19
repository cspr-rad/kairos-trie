pub mod merkle;

use std::hash::Hash;

use alloc::{collections::BTreeMap, fmt::Debug, string::String};

use crate::{Branch, Extension, Leaf};

pub trait PartialStore<V>: Store<V> {
    fn get_unvisted_hash(&self, hash_ref: &Self::HashRef) -> Result<&NodeHash, Self::Error>;
}

pub trait Store<V> {
    /// The hash of a node or leaf.
    /// Alternatively, this could be a reference or an index that uniquely identifies a node or leaf
    ///
    /// TODO consider using a single u32 as the Idx type.
    type HashRef: Copy + Clone + Eq + Ord + Hash + Debug;
    type Error: Into<String>;

    fn get_branch(
        &self,
        hash_ref: &Self::HashRef,
    ) -> Result<&Branch<Node<Self::HashRef, Self::HashRef, Self::HashRef>>, Self::Error>;
    fn get_extension(
        &self,
        hash_ref: &Self::HashRef,
    ) -> Result<&Extension<Self::HashRef, Self::HashRef, Self::HashRef, V>, Self::Error>;

    fn get_leaf(&self, hash_ref: &Self::HashRef) -> Result<&Leaf<V>, Self::Error>;
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Node<B, E, L> {
    Branch(B),
    Extension(E),
    Leaf(L),
}

pub enum Error {
    NodeNotFound,
}

impl From<Error> for String {
    fn from(err: Error) -> String {
        match err {
            Error::NodeNotFound => "Node not found".into(),
        }
    }
}

pub type NodeHash = [u8; 32];

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct MemoryStore<V> {
    // TODO: use a indexmap
    branches: BTreeMap<NodeHash, Branch<Node<NodeHash, NodeHash, NodeHash>>>,
    extensions: BTreeMap<NodeHash, Extension<NodeHash, NodeHash, NodeHash, V>>,
    leaves: BTreeMap<NodeHash, Leaf<V>>,
}

impl<V> Store<V> for MemoryStore<V> {
    type HashRef = NodeHash;
    type Error = Error;

    fn get_branch(
        &self,
        hash_ref: &Self::HashRef,
    ) -> Result<&Branch<Node<Self::HashRef, Self::HashRef, Self::HashRef>>, Self::Error> {
        self.branches.get(hash_ref).ok_or(Error::NodeNotFound)
    }

    fn get_extension(
        &self,
        idx: &Self::HashRef,
    ) -> Result<&Extension<Self::HashRef, Self::HashRef, Self::HashRef, V>, Self::Error> {
        self.extensions.get(idx).ok_or(Error::NodeNotFound)
    }

    fn get_leaf(&self, idx: &Self::HashRef) -> Result<&Leaf<V>, Self::Error> {
        self.leaves.get(idx).ok_or(Error::NodeNotFound)
    }
}

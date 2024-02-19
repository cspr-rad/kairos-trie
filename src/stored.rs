pub mod merkle;

use std::hash::Hash;

use alloc::{collections::BTreeMap, fmt::Debug, string::String};

use crate::{Branch, Extension, Leaf};

pub type Idx = u32;

pub trait Store<V> {
    type Error: Into<String>;

    /// Must return a hash of a node that has not been visited.
    /// May return a hash of a node that has already been visited.
    fn get_unvisted_hash(&self, hash: Idx) -> Result<&NodeHash, Self::Error>;

    fn get_node(
        &mut self,
        hash: Idx,
    ) -> Result<Node<&Branch<Idx>, &Extension<V>, &Leaf<V>>, Self::Error>;
}

pub trait Db<V> {
    type Error: Into<String>;

    fn get(
        &self,
        hash: &NodeHash,
    ) -> Result<Node<Branch<NodeHash>, Extension<V>, NodeHash>, Self::Error>;
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
pub struct MemoryDb<V> {
    leaves: BTreeMap<NodeHash, Node<Branch<NodeHash>, Extension<V>, NodeHash>>,
}

impl<V: Clone> Db<V> for MemoryDb<V> {
    type Error = Error;

    fn get(
        &self,
        hash: &NodeHash,
    ) -> Result<Node<Branch<NodeHash>, Extension<V>, NodeHash>, Self::Error> {
        self.leaves.get(hash).cloned().ok_or(Error::NodeNotFound)
    }
}

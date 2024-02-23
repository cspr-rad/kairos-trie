pub mod merkle;

use std::hash::Hash;

use alloc::{collections::BTreeMap, fmt::Debug, string::String};

use crate::{Branch, Leaf};

pub type Idx = u32;

pub trait Store<V> {
    type Error: Into<String>;

    /// Must return a hash of a node that has not been visited.
    /// May return a hash of a node that has already been visited.
    fn get_unvisted_hash(&self, hash_idx: Idx) -> Result<&NodeHash, Self::Error>;

    fn get_node(&mut self, hash_idx: Idx) -> Result<Node<&Branch<Idx>, &Leaf<V>>, Self::Error>;
}

pub trait Database<V> {
    type Error: Into<String>;

    fn get(&self, hash: &NodeHash) -> Result<Node<Branch<NodeHash>, Leaf<V>>, Self::Error>;
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Node<B, L> {
    Branch(B),
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

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct MemoryDb<V> {
    leaves: BTreeMap<NodeHash, Node<Branch<NodeHash>, Leaf<V>>>,
}

impl<V: Clone> Database<V> for MemoryDb<V> {
    type Error = Error;

    fn get(&self, hash_idx: &NodeHash) -> Result<Node<Branch<NodeHash>, Leaf<V>>, Self::Error> {
        self.leaves
            .get(hash_idx)
            .cloned()
            .ok_or(Error::NodeNotFound)
    }
}

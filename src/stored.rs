pub mod merkle;

use std::cell::RefCell;
use std::hash::Hash;

use alloc::{collections::BTreeMap, fmt::Debug, string::String};

use crate::{Branch, Leaf};

pub type Idx = u32;

pub trait Store<V> {
    type Error: Into<String> + Debug;

    /// Must return a hash of a node that has not been visited.
    /// May return a hash of a node that has already been visited.
    fn get_unvisted_hash(&self, hash_idx: Idx) -> Result<&NodeHash, Self::Error>;

    fn get_node(&self, hash_idx: Idx) -> Result<Node<&Branch<Idx>, &Leaf<V>>, Self::Error>;
}

impl<V, S: Store<V>> Store<V> for &S {
    type Error = S::Error;

    fn get_unvisted_hash(&self, hash_idx: Idx) -> Result<&NodeHash, Self::Error> {
        (**self).get_unvisted_hash(hash_idx)
    }

    fn get_node(&self, hash_idx: Idx) -> Result<Node<&Branch<Idx>, &Leaf<V>>, Self::Error> {
        (**self).get_node(hash_idx)
    }
}

pub trait DatabaseGet<V> {
    type GetError: Into<String> + Debug;

    fn get(&self, hash: &NodeHash) -> Result<Node<Branch<NodeHash>, Leaf<V>>, Self::GetError>;
}

impl<V, D: DatabaseGet<V>> DatabaseGet<V> for &D {
    type GetError = D::GetError;

    fn get(&self, hash: &NodeHash) -> Result<Node<Branch<NodeHash>, Leaf<V>>, Self::GetError> {
        (**self).get(hash)
    }
}

pub trait DatabaseSet<V>: DatabaseGet<V> {
    type SetError: Into<String> + Debug;

    fn set(
        &self,
        hash: NodeHash,
        node: Node<Branch<NodeHash>, Leaf<V>>,
    ) -> Result<(), Self::GetError>;
}

impl<V, D: DatabaseSet<V>> DatabaseSet<V> for &D {
    type SetError = D::SetError;

    fn set(
        &self,
        hash: NodeHash,
        node: Node<Branch<NodeHash>, Leaf<V>>,
    ) -> Result<(), Self::GetError> {
        (**self).set(hash, node)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Node<B, L> {
    Branch(B),
    Leaf(L),
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
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
    leaves: RefCell<BTreeMap<NodeHash, Node<Branch<NodeHash>, Leaf<V>>>>,
}

impl<V> MemoryDb<V> {
    pub fn empty() -> Self {
        Self {
            leaves: RefCell::default(),
        }
    }
}

impl<V: Clone> DatabaseGet<V> for MemoryDb<V> {
    type GetError = Error;

    fn get(&self, hash_idx: &NodeHash) -> Result<Node<Branch<NodeHash>, Leaf<V>>, Self::GetError> {
        self.leaves
            .borrow()
            .get(hash_idx)
            .cloned()
            .ok_or(Error::NodeNotFound)
    }
}

impl<V: Clone> DatabaseSet<V> for MemoryDb<V> {
    type SetError = Error;

    fn set(
        &self,
        hash_idx: NodeHash,
        node: Node<Branch<NodeHash>, Leaf<V>>,
    ) -> Result<(), Self::SetError> {
        self.leaves.borrow_mut().insert(hash_idx, node);
        Ok(())
    }
}

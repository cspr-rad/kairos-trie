pub mod memory_db;
pub mod merkle;

use alloc::fmt::Debug;
use core::{fmt::Display, hash::Hash};

use crate::{Branch, Leaf};

pub type Idx = u32;

pub trait Store<V> {
    type Error: Display;

    /// Must return a hash of a node that has not been visited.
    /// May return a hash of a node that has already been visited.
    fn get_unvisited_hash(&self, hash_idx: Idx) -> Result<&NodeHash, Self::Error>;

    fn get_node(&self, hash_idx: Idx) -> Result<Node<&Branch<Idx>, &Leaf<V>>, Self::Error>;
}

impl<V, S: Store<V>> Store<V> for &S {
    type Error = S::Error;

    fn get_unvisited_hash(&self, hash_idx: Idx) -> Result<&NodeHash, Self::Error> {
        (**self).get_unvisited_hash(hash_idx)
    }

    fn get_node(&self, hash_idx: Idx) -> Result<Node<&Branch<Idx>, &Leaf<V>>, Self::Error> {
        (**self).get_node(hash_idx)
    }
}

pub trait DatabaseGet<V> {
    type GetError: Display;

    fn get(&self, hash: &NodeHash) -> Result<Node<Branch<NodeHash>, Leaf<V>>, Self::GetError>;
}

impl<V, D: DatabaseGet<V>> DatabaseGet<V> for &D {
    type GetError = D::GetError;

    fn get(&self, hash: &NodeHash) -> Result<Node<Branch<NodeHash>, Leaf<V>>, Self::GetError> {
        (**self).get(hash)
    }
}

pub trait DatabaseSet<V>: DatabaseGet<V> {
    type SetError: Display;

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

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct NodeHash {
    pub bytes: [u8; 32],
}

impl NodeHash {
    #[inline]
    pub fn new(bytes: [u8; 32]) -> Self {
        Self { bytes }
    }
}

impl AsRef<[u8]> for NodeHash {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

impl Display for NodeHash {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // TODO hex
        write!(f, "NodeHash({:?})", &self.bytes)
    }
}

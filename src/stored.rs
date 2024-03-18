pub mod memory_db;
pub mod merkle;

use core::fmt::Display;

use crate::{
    transaction::nodes::{Branch, Leaf, Node},
    NodeHash,
};

pub type Idx = u32;

pub trait Store<V> {
    type Error: Display;

    fn calc_subtree_hash(&self, hash_idx: Idx) -> Result<NodeHash, Self::Error>;

    fn get_node(&self, hash_idx: Idx) -> Result<Node<&Branch<Idx>, &Leaf<V>>, Self::Error>;
}

impl<V, S: Store<V>> Store<V> for &S {
    type Error = S::Error;

    #[inline(always)]
    fn calc_subtree_hash(&self, hash_idx: Idx) -> Result<NodeHash, Self::Error> {
        (**self).calc_subtree_hash(hash_idx)
    }

    #[inline(always)]
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

    #[inline]
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

    #[inline]
    fn set(
        &self,
        hash: NodeHash,
        node: Node<Branch<NodeHash>, Leaf<V>>,
    ) -> Result<(), Self::GetError> {
        (**self).set(hash, node)
    }
}

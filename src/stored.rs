pub mod memory_db;
pub mod merkle;

use core::fmt::Display;

use alloc::{rc::Rc, sync::Arc};

use crate::{
    transaction::nodes::{Branch, Leaf, Node},
    NodeHash, PortableHash, PortableHasher,
};

pub type Idx = u32;

pub trait Store {
    type Error: Display;
    type Value: Clone + PortableHash;

    fn calc_subtree_hash(
        &self,
        hasher: &mut impl PortableHasher<32>,
        hash_idx: Idx,
    ) -> Result<NodeHash, Self::Error>;

    fn get_node(
        &self,
        hash_idx: Idx,
    ) -> Result<Node<&Branch<Idx>, &Leaf<Self::Value>>, Self::Error>;

    fn get_node_hash(
        &self,
        hash_idx: Idx,
    ) -> Result<NodeHash, Self::Error>;
}

impl<S: Store> Store for &S {
    type Error = S::Error;
    type Value = S::Value;

    #[inline(always)]
    fn calc_subtree_hash(
        &self,

        hasher: &mut impl PortableHasher<32>,
        hash_idx: Idx,
    ) -> Result<NodeHash, Self::Error> {
        (**self).calc_subtree_hash(hasher, hash_idx)
    }

    #[inline(always)]
    fn get_node(
        &self,
        hash_idx: Idx,
    ) -> Result<Node<&Branch<Idx>, &Leaf<Self::Value>>, Self::Error> {
        (**self).get_node(hash_idx)
    }

    #[inline(always)]
    fn get_node_hash(
        &self,
        hash_idx: Idx,
    ) -> Result<NodeHash, Self::Error> {
        (**self).get_node_hash(hash_idx)
    }
}

impl<S: Store> Store for Rc<S> {
    type Error = S::Error;
    type Value = S::Value;

    #[inline(always)]
    fn calc_subtree_hash(
        &self,
        hasher: &mut impl PortableHasher<32>,
        hash_idx: Idx,
    ) -> Result<NodeHash, Self::Error> {
        (**self).calc_subtree_hash(hasher, hash_idx)
    }

    #[inline(always)]
    fn get_node(
        &self,
        hash_idx: Idx,
    ) -> Result<Node<&Branch<Idx>, &Leaf<Self::Value>>, Self::Error> {
        (**self).get_node(hash_idx)
    }

    #[inline(always)]
    fn get_node_hash(
        &self,
        hash_idx: Idx,
    ) -> Result<NodeHash, Self::Error> {
        (**self).get_node_hash(hash_idx)
    }
}

impl<S: Store> Store for Arc<S> {
    type Error = S::Error;
    type Value = S::Value;

    #[inline(always)]
    fn calc_subtree_hash(
        &self,
        hasher: &mut impl PortableHasher<32>,
        hash_idx: Idx,
    ) -> Result<NodeHash, Self::Error> {
        (**self).calc_subtree_hash(hasher, hash_idx)
    }

    #[inline(always)]
    fn get_node(
        &self,
        hash_idx: Idx,
    ) -> Result<Node<&Branch<Idx>, &Leaf<Self::Value>>, Self::Error> {
        (**self).get_node(hash_idx)
    }

    #[inline(always)]
    fn get_node_hash(
        &self,
        hash_idx: Idx,
    ) -> Result<NodeHash, Self::Error> {
        (**self).get_node_hash(hash_idx)
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

impl<V, D: DatabaseGet<V>> DatabaseGet<V> for Rc<D> {
    type GetError = D::GetError;

    #[inline]
    fn get(&self, hash: &NodeHash) -> Result<Node<Branch<NodeHash>, Leaf<V>>, Self::GetError> {
        (**self).get(hash)
    }
}

impl<V, D: DatabaseSet<V>> DatabaseSet<V> for Rc<D> {
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

impl<V, D: DatabaseGet<V>> DatabaseGet<V> for Arc<D> {
    type GetError = D::GetError;

    #[inline]
    fn get(&self, hash: &NodeHash) -> Result<Node<Branch<NodeHash>, Leaf<V>>, Self::GetError> {
        (**self).get(hash)
    }
}

impl<V, D: DatabaseSet<V>> DatabaseSet<V> for Arc<D> {
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

/// Trait for databases that support removing nodes
pub trait DatabaseRemove {
    /// Error type returned when a remove operation fails
    type RemoveError: Display;

    /// Remove a node from the database
    fn remove(&self, key: &NodeHash) -> Result<(), Self::RemoveError>;
}

impl<D: DatabaseRemove> DatabaseRemove for &D {
    type RemoveError = D::RemoveError;

    #[inline]
    fn remove(&self, key: &NodeHash) -> Result<(), Self::RemoveError> {
        (**self).remove(key)
    }
}

impl<D: DatabaseRemove> DatabaseRemove for Rc<D> {
    type RemoveError = D::RemoveError;

    #[inline]
    fn remove(&self, key: &NodeHash) -> Result<(), Self::RemoveError> {
        (**self).remove(key)
    }
}

impl<D: DatabaseRemove> DatabaseRemove for Arc<D> {
    type RemoveError = D::RemoveError;

    #[inline]
    fn remove(&self, key: &NodeHash) -> Result<(), Self::RemoveError> {
        (**self).remove(key)
    }
}

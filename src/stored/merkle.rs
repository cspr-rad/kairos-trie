use alloc::{boxed::Box, vec::Vec};

use crate::{Branch, Extension, Leaf, Store};

use super::{Error, Node, NodeHash, PartialStore};

type Idx = u32;

pub struct Snapshot<V> {
    branches: Box<[Branch<Node<Idx, Idx, Idx>>]>,
    extension: Box<[Extension<Idx, Idx, Idx, V>]>,
    leaves: Box<[Leaf<V>]>,

    // Unvisited we only store the hash of.
    unvisted_nodes: Vec<NodeHash>,
}

impl<V> Snapshot<V> {
    fn get_branch(&self, idx: Idx) -> Option<&Branch<Node<Idx, Idx, Idx>>> {
        let idx = idx as usize;

        self.branches.get(idx)
    }

    fn get_extension(&self, idx: Idx) -> Option<&Extension<Idx, Idx, Idx, V>> {
        // Wrapping here is safe because we will never have anything near 2^32 nodes in a Snapshot.
        // So get will error if we wrap around.
        //
        // TODO ensure this, and keys don't overlap verifying a snapshot.
        let idx = idx as usize - self.branches.len();

        self.extension.get(idx)
    }

    fn get_leaf(&self, idx: Idx) -> Option<&Leaf<V>> {
        let idx = idx as usize - self.branches.len() - self.extension.len();

        self.leaves.get(idx)
    }

    fn get_unvisted_hash(&self, idx: Idx) -> Option<&NodeHash> {
        let idx = idx as usize - self.branches.len() - self.extension.len() - self.leaves.len();

        self.unvisted_nodes.get(idx)
    }
}

impl<V> Store<V> for Snapshot<V> {
    type HashRef = Idx;
    type Error = Error;

    fn get_branch(&self, idx: &Self::HashRef) -> Result<&Branch<Node<Idx, Idx, Idx>>, Self::Error> {
        self.get_branch(*idx).ok_or(Error::NodeNotFound)
    }

    fn get_extension(
        &self,
        idx: &Self::HashRef,
    ) -> Result<&Extension<Idx, Idx, Idx, V>, Self::Error> {
        self.get_extension(*idx).ok_or(Error::NodeNotFound)
    }

    fn get_leaf(&self, idx: &Self::HashRef) -> Result<&Leaf<V>, Self::Error> {
        self.get_leaf(*idx).ok_or(Error::NodeNotFound)
    }
}

impl<V> PartialStore<V> for Snapshot<V> {
    fn get_unvisted_hash(&self, hash_ref: &Self::HashRef) -> Result<&NodeHash, Self::Error> {
        self.get_unvisted_hash(*hash_ref).ok_or(Error::NodeNotFound)
    }
}

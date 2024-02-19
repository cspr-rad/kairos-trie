use alloc::{boxed::Box, vec::Vec};

use crate::{Branch, Extension, Leaf};

use super::{Error, Idx, Node, NodeHash, Store};

pub struct Snapshot<V> {
    branches: Box<[Branch<Idx>]>,
    extension: Box<[Extension<V>]>,
    leaves: Box<[Leaf<V>]>,

    // Unvisited we only store the hash of.
    unvisted_nodes: Box<[NodeHash]>,
}

impl<V> Snapshot<V> {
    fn get_unvisted_hash(&self, idx: Idx) -> Option<&NodeHash> {
        let idx = idx as usize - self.branches.len() - self.extension.len() - self.leaves.len();

        self.unvisted_nodes.get(idx)
    }
}

impl<V> Store<V> for Snapshot<V> {
    type Error = Error;

    fn get_unvisted_hash(&self, idx: Idx) -> Result<&NodeHash, Self::Error> {
        self.get_unvisted_hash(idx).ok_or(Error::NodeNotFound)
    }

    fn get_node(
        &mut self,
        idx: Idx,
    ) -> Result<Node<&Branch<Idx>, &Extension<V>, &Leaf<V>>, Self::Error> {
        let idx = idx as usize;
        let extension_offset = self.branches.len();
        let leaf_offset = extension_offset + self.extension.len();
        let unvisited_offset = leaf_offset + self.leaves.len();

        if idx < extension_offset {
            Ok(Node::Branch(&self.branches[idx]))
        } else if idx < leaf_offset {
            Ok(Node::Extension(&self.extension[idx - extension_offset]))
        } else if idx < unvisited_offset {
            Ok(Node::Leaf(&self.leaves[idx - leaf_offset]))
        } else {
            Err(Error::NodeNotFound)
        }
    }
}

pub struct SnapshotBuilder<V> {
    branches: Vec<Branch<Idx>>,
    extension: Vec<Extension<V>>,
    leaves: Vec<Leaf<V>>,

    // Unvisited we only store the hash of.
    unvisted_nodes: Vec<NodeHash>,
}

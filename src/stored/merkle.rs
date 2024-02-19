use alloc::{boxed::Box, vec::Vec};
use bumpalo::Bump;
use ouroboros::self_referencing;

use crate::{Branch, Extension, Leaf};

use super::{Database, Error, Idx, Node, NodeHash, Store};

pub struct Snapshot<V> {
    branches: Box<[Branch<Idx>]>,
    extension: Box<[Extension<Idx>]>,
    leaves: Box<[Leaf<V>]>,

    // Unvisited we only store the hash of.
    nodes: Box<[NodeHash]>,
}

impl<V> Snapshot<V> {
    fn get_unvisted_hash(&self, idx: Idx) -> Option<&NodeHash> {
        let idx = idx as usize - self.branches.len() - self.extension.len() - self.leaves.len();

        self.nodes.get(idx)
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
    ) -> Result<Node<&Branch<Idx>, &Extension<Idx>, &Leaf<V>>, Self::Error> {
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

#[self_referencing]
pub struct SnapshotBuilder<Db: 'static, V: 'static> {
    db: Db,
    bump: Bump,

    #[borrows(bump)]
    #[covariant]
    nodes: Vec<&'this NodeHashMaybeNode<'this, V>>,
}

type NodeHashMaybeNode<'a, V> = (
    NodeHash,
    Option<Node<&'a Branch<Idx>, &'a Extension<Idx>, &'a Leaf<V>>>,
);

impl<Db: 'static + Database<V>, V: 'static> Store<V> for SnapshotBuilder<Db, V> {
    type Error = Error;

    fn get_unvisted_hash(&self, hash_idx: Idx) -> Result<&NodeHash, Self::Error> {
        let hash_idx = hash_idx as usize;

        self.with_nodes(|nodes| {
            nodes
                .get(hash_idx)
                .map(|(hash, _)| hash)
                .ok_or(Error::NodeNotFound)
        })
    }

    fn get_node(
        &mut self,
        hash_idx: Idx,
    ) -> Result<Node<&Branch<Idx>, &Extension<Idx>, &Leaf<V>>, Self::Error> {
        let hash_idx = hash_idx as usize;

        self.with_mut(|this| {
            let Some((hash, o_node)) = this
                .nodes
                .get(hash_idx)
                .map(|(hash, o_node)| (hash, *o_node))
            else {
                return Err(Error::NodeNotFound);
            };

            if let Some(node) = o_node {
                return Ok(node);
            }

            let next_idx = this.nodes.len() as Idx;
            let (node, left, right) = Self::get_from_db(this.bump, this.db, hash, next_idx)?;

            let mut add_unvisited = |hash: Option<NodeHash>| {
                if let Some(hash) = hash {
                    this.nodes.push(this.bump.alloc((hash, None)))
                }
            };

            add_unvisited(left);
            add_unvisited(right);

            Ok(node)
        })
    }
}

impl<Db: 'static + Database<V>, V: 'static> SnapshotBuilder<Db, V> {
    #[inline(always)]
    fn get_from_db<'a>(
        bump: &'a Bump,
        db: &Db,
        hash: &NodeHash,
        next_idx: Idx,
    ) -> Result<
        (
            Node<&'a Branch<Idx>, &'a Extension<Idx>, &'a Leaf<V>>,
            Option<NodeHash>,
            Option<NodeHash>,
        ),
        Error,
    > {
        let Ok(node) = db.get(hash) else {
            return Err(Error::NodeNotFound);
        };

        Ok(match node {
            Node::Branch(Branch {
                rel_bit_idx,
                left_bits,
                right_bits,
                left,
                right,
            }) => (
                Node::Branch(&*bump.alloc(Branch {
                    rel_bit_idx,
                    left_bits,
                    right_bits,
                    left: next_idx,
                    right: next_idx + 1,
                })),
                Some(left),
                Some(right),
            ),
            Node::Extension(Extension { next, bits }) => (
                Node::Extension(&*bump.alloc(Extension {
                    next: next_idx,
                    bits,
                })),
                Some(next),
                None,
            ),
            Node::Leaf(leaf) => (Node::Leaf(&*bump.alloc(leaf)), None, None),
        })
    }
}

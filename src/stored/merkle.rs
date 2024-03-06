use alloc::{boxed::Box, string::String, vec::Vec};
use bumpalo::Bump;

use crate::{Branch, Leaf};

use super::{Database, Error, Idx, Node, NodeHash, Store};

pub struct Snapshot<V> {
    branches: Box<[Branch<Idx>]>,
    leaves: Box<[Leaf<V>]>,

    // we only store the hashes of the nodes that have not been visited.
    nodes: Box<[NodeHash]>,
}

impl<V: AsRef<[u8]>> Snapshot<V> {
    fn get_unvisted_hash(&self, idx: Idx) -> Option<&NodeHash> {
        let idx = idx as usize - self.branches.len() - self.leaves.len();

        self.nodes.get(idx)
    }

    /// Always check that the snapshot is of the merkle tree you expect.
    pub fn calc_root_hash(&self) -> Result<NodeHash, String> {
        if self.branches.is_empty() {
            if self.leaves.is_empty() {
                Ok([0; 32])
            } else {
                if self.leaves.len() != 1 {
                    return Err("Invalid snapshot".into());
                }
                Ok(self.leaves[0].hash_node())
            }
        } else {
            if self.leaves.is_empty() {
                return Err("Invalid snapshot".into());
            }

            let _root = &self.branches[0];
            todo!("calc the root hash starting from the root branch at index 0");
        }
    }
}

impl<V: AsRef<[u8]>> Store<V> for Snapshot<V> {
    type Error = Error;

    fn get_unvisted_hash(&self, idx: Idx) -> Result<&NodeHash, Self::Error> {
        self.get_unvisted_hash(idx).ok_or(Error::NodeNotFound)
    }

    fn get_node(&mut self, idx: Idx) -> Result<Node<&Branch<Idx>, &Leaf<V>>, Self::Error> {
        let idx = idx as usize;
        let leaf_offset = self.branches.len();
        let unvisited_offset = leaf_offset + self.leaves.len();

        if idx < leaf_offset {
            Ok(Node::Branch(&self.branches[idx]))
        } else if idx < unvisited_offset {
            Ok(Node::Leaf(&self.leaves[idx - leaf_offset]))
        } else {
            Err(Error::NodeNotFound)
        }
    }
}

pub struct SnapshotBuilder<'a, Db, V> {
    db: Db,
    bump: &'a Bump,

    nodes: Vec<&'a NodeHashMaybeNode<'a, V>>,
}

type NodeHashMaybeNode<'a, V> = (NodeHash, Option<Node<&'a Branch<Idx>, &'a Leaf<V>>>);

impl<'a, Db: Database<V>, V> Store<V> for SnapshotBuilder<'a, Db, V> {
    type Error = Error;

    fn get_unvisted_hash(&self, hash_idx: Idx) -> Result<&NodeHash, Self::Error> {
        let hash_idx = hash_idx as usize;

        self.nodes
            .get(hash_idx)
            .map(|(hash, _)| hash)
            .ok_or(Error::NodeNotFound)
    }

    fn get_node(&mut self, hash_idx: Idx) -> Result<Node<&Branch<Idx>, &Leaf<V>>, Self::Error> {
        let hash_idx = hash_idx as usize;

        let Some((hash, o_node)) = self
            .nodes
            .get(hash_idx)
            .map(|(hash, o_node)| (hash, *o_node))
        else {
            return Err(Error::NodeNotFound);
        };

        if let Some(node) = o_node {
            return Ok(node);
        }

        let next_idx = self.nodes.len() as Idx;
        let (node, left, right) = Self::get_from_db(self.bump, &self.db, hash, next_idx)?;

        let mut add_unvisited = |hash: Option<NodeHash>| {
            if let Some(hash) = hash {
                self.nodes.push(self.bump.alloc((hash, None)))
            }
        };

        add_unvisited(left);
        add_unvisited(right);

        Ok(node)
    }
}

impl<'a, Db: Database<V>, V> SnapshotBuilder<'a, Db, V> {
    pub fn new_with_db(db: Db, bump: &'a Bump) -> Self {
        Self {
            db,
            bump,
            nodes: Vec::new(),
        }
    }

    #[inline(always)]
    fn get_from_db(
        bump: &'a Bump,
        db: &Db,
        hash: &NodeHash,
        next_idx: Idx,
    ) -> Result<
        (
            Node<&'a Branch<Idx>, &'a Leaf<V>>,
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
                mask,
                left,
                right,
                prior_word,
                prefix,
            }) => (
                Node::Branch(&*bump.alloc(Branch {
                    mask,
                    left: next_idx,
                    right: next_idx + 1,
                    prior_word,
                    prefix,
                })),
                Some(left),
                Some(right),
            ),

            Node::Leaf(leaf) => (Node::Leaf(&*bump.alloc(leaf)), None, None),
        })
    }
}

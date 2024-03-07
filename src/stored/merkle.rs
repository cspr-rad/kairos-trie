use std::cell::RefCell;

use alloc::{boxed::Box, string::String, vec::Vec};
use bumpalo::Bump;

use crate::{Branch, Leaf};

use super::{Database, Error, Idx, Node, NodeHash, Store};

/// A snapshot of the merkle trie
///
/// Contains visited nodes and unvisited nodes
pub struct Snapshot<V> {
    /// Branches[0] is the root of the tree
    branches: Box<[Branch<Idx>]>,
    leaves: Box<[Leaf<V>]>,

    // we only store the hashes of the nodes that have not been visited.
    unvisited_nodes: Box<[NodeHash]>,
}

impl<V: AsRef<[u8]>> Snapshot<V> {
    fn get_unvisted_hash(&self, idx: Idx) -> Option<&NodeHash> {
        let idx = idx as usize - self.branches.len() - self.leaves.len();

        self.unvisited_nodes.get(idx)
    }

    /// Always check that the snapshot is of the merkle tree you expect.
    pub fn calc_root_hash(&self) -> Result<NodeHash, String> {
        match (
            self.branches.len(),
            self.leaves.len(),
            self.unvisited_nodes.len(),
        ) {
            // A empty tree
            (0, 0, 0) => Ok([0; 32]),
            // A tree with only one unvisted node
            (0, 0, 1) => Ok(self.unvisited_nodes[0]),
            (0, 0, unvisited) => Err(format!(
                "Invalid snapshot: unvisited nodes cannot contain more than one node if there are no branches or leaves. Found {} unvisited nodes",
                unvisited
            )),

            // A tree with only one leaf
            (0, 1, 0) => Ok(self.leaves[0].hash_node()),
            (0, leaves, 0) => Err(format!(
                "Invalid snapshot: a tree with no branches can only have one leaf. Found {} leaves",
                leaves
            )),

            (branches, 0, 0) => Err(format!(
                "Invalid snapshot: a branch must have descendants. Found {} branches",
                branches
            )),

            _ => todo!("calc the root hash starting from the root branch at index 0"),
        }
    }

    fn calc_root_hash_inner(&self, idx: Idx) -> Result<NodeHash, Error> {
        debug_assert!(!self.branches.is_empty());

        let root = &self.branches[0];

        // depth first hash
        // We could put this on the stack using a `ArrayVec<_, 256>`
        let mut stack = vec![(root.left, root.right)];

        let mut branch = &self.branches[0];

        while let Node::Branch(Branch { left, right, .. }) = self.get_node(branch.left)? {
            stack.push((*left, *right));
        }

        todo!()
    }
}

impl<V: AsRef<[u8]>> Store<V> for Snapshot<V> {
    type Error = Error;

    fn get_unvisted_hash(&self, idx: Idx) -> Result<&NodeHash, Self::Error> {
        self.get_unvisted_hash(idx).ok_or(Error::NodeNotFound)
    }

    fn get_node(&self, idx: Idx) -> Result<Node<&Branch<Idx>, &Leaf<V>>, Self::Error> {
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

impl<'a, Db, V> From<SnapshotBuilder<'a, Db, V>> for Snapshot<V> {
    fn from(builder: SnapshotBuilder<'a, Db, V>) -> Self {
        todo!()
    }
}

pub struct SnapshotBuilder<'a, Db, V> {
    db: Db,
    bump: &'a Bump,

    nodes: RefCell<Vec<&'a NodeHashMaybeNode<'a, V>>>,
}

type NodeHashMaybeNode<'a, V> = (NodeHash, Option<Node<&'a Branch<Idx>, &'a Leaf<V>>>);

impl<'a, Db: Database<V>, V> Store<V> for SnapshotBuilder<'a, Db, V> {
    type Error = Error;

    fn get_unvisted_hash(&self, hash_idx: Idx) -> Result<&NodeHash, Self::Error> {
        let hash_idx = hash_idx as usize;

        self.nodes
            .borrow()
            .get(hash_idx)
            .map(|(hash, _)| hash)
            .ok_or(Error::NodeNotFound)
    }

    fn get_node(&self, hash_idx: Idx) -> Result<Node<&Branch<Idx>, &Leaf<V>>, Self::Error> {
        let hash_idx = hash_idx as usize;

        let Some((hash, o_node)) = self
            .nodes
            .borrow()
            .get(hash_idx)
            .map(|(hash, o_node)| (hash, *o_node))
        else {
            return Err(Error::NodeNotFound);
        };

        if let Some(node) = o_node {
            return Ok(node);
        }

        let next_idx = self.nodes.borrow().len() as Idx;
        let (node, left, right) = Self::get_from_db(self.bump, &self.db, hash, next_idx)?;

        let add_unvisited = |hash: Option<NodeHash>| {
            if let Some(hash) = hash {
                self.nodes.borrow_mut().push(self.bump.alloc((hash, None)))
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
            nodes: RefCell::new(Vec::new()),
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

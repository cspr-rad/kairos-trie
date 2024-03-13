use core::ops::Deref;
use std::cell::RefCell;

use alloc::{boxed::Box, format, string::String, vec::Vec};
use bumpalo::Bump;

use crate::{Branch, Leaf, NodeRef, TrieRoot};

use super::{DatabaseGet, Error, Idx, Node, NodeHash, Store};

/// A snapshot of the merkle trie
///
/// Contains visited nodes and unvisited nodes
pub struct Snapshot<V> {
    /// The last branch is the root of the trie if it exists.
    branches: Box<[Branch<Idx>]>,
    /// A Snapshot containing only
    leaves: Box<[Leaf<V>]>,

    // we only store the hashes of the nodes that have not been visited.
    unvisited_nodes: Box<[NodeHash]>,
}

impl<V: AsRef<[u8]>> Snapshot<V> {
    pub fn root_node_idx(&self) -> Result<Option<Idx>, String> {
        // Revist this once https://github.com/rust-lang/rust/issues/37854 is stable
        match (
            self.branches.deref(),
            self.leaves.deref(),
            self.unvisited_nodes.deref(),
        ) {
            // A empty tree
            ([], [], []) => Ok(None),
            // A tree with only one node
            ([_], [], []) | ([], [_], []) | ([], [], [_]) => Ok(Some(0)),
            (branches, _, _) if !branches.is_empty() => Ok(Some(branches.len() as Idx - 1)),
            _ => Err(format!(
                "Invalid snapshot: \n\
                a tree with no branches can only have one leaf.\n\
                a tree with no branches or leaves can only have one unvisited node.\n\
                Found {} branches, {} leaves, and {} unvisited nodes",
                self.branches.len(),
                self.leaves.len(),
                self.unvisited_nodes.len()
            )),
        }
    }

    pub fn trie_root(&self) -> Result<TrieRoot<V>, String> {
        match self.root_node_idx()? {
            Some(idx) => Ok(TrieRoot::Node(NodeRef::Stored(idx))),
            None => Ok(TrieRoot::Empty),
        }
    }

    /// Always check that the snapshot is of the merkle tree you expect.
    pub fn calc_root_hash(&self) -> Result<NodeHash, String> {
        self.root_node_idx()?
            .map(|idx| self.calc_root_hash_inner(idx))
            .unwrap_or(Ok(NodeHash::default()))
    }

    // TODO fix possible stack overflow
    // I dislike using an explicit mutable stack.
    // I have an idea for abusing async for high performance segmented stacks
    fn calc_root_hash_inner(&self, node: Idx) -> Result<NodeHash, String> {
        match self.get_node(node) {
            Ok(Node::Branch(branch)) => {
                let left = self.calc_root_hash_inner(branch.left)?;
                let right = self.calc_root_hash_inner(branch.right)?;

                Ok(branch.hash_branch(&left, &right))
            }
            Ok(Node::Leaf(leaf)) => Ok(leaf.hash_leaf()),
            Err(_) => self
                .get_unvisted_hash(node)
                .copied()
                .map_err(|_| format!("Invalid snapshot: node {} not found", node)),
        }
    }
}

impl<V: AsRef<[u8]>> Store<V> for Snapshot<V> {
    type Error = Error;

    fn get_unvisted_hash(&self, idx: Idx) -> Result<&NodeHash, Self::Error> {
        let idx = idx as usize - self.branches.len() - self.leaves.len();

        self.unvisited_nodes.get(idx).ok_or(Error::NodeNotFound)
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

#[derive(Clone, Debug)]
pub struct SnapshotBuilder<'a, Db, V> {
    pub db: Db,
    bump: &'a Bump,

    /// The root of the trie is always at index 0
    nodes: RefCell<Vec<&'a NodeHashMaybeNode<'a, V>>>,
}

type NodeHashMaybeNode<'a, V> = (NodeHash, Option<Node<&'a Branch<Idx>, &'a Leaf<V>>>);

impl<'a, Db: DatabaseGet<V>, V: Clone> Store<V> for SnapshotBuilder<'a, Db, V> {
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

impl<'a, Db, V> SnapshotBuilder<'a, Db, V> {
    pub fn empty(db: Db, bump: &'a Bump) -> Self {
        Self {
            db,
            bump,
            nodes: RefCell::new(Vec::new()),
        }
    }

    pub fn with_root_hash(self, root_hash: NodeHash) -> Self {
        // I don't love empty being a zeroed hash
        if root_hash == NodeHash::default() {
            return self;
        };
        self.nodes
            .borrow_mut()
            .push(self.bump.alloc((root_hash, None)));
        self
    }

    pub fn trie_root(&self) -> TrieRoot<V> {
        match self.nodes.borrow().first() {
            Some(_) => TrieRoot::Node(NodeRef::Stored(0)),
            None => TrieRoot::Empty,
        }
    }

    pub fn build_initial_snapshot(&self) -> Snapshot<V>
    where
        V: Clone,
    {
        let nodes = self.nodes.borrow();

        if nodes.is_empty() {
            Snapshot {
                branches: Box::new([]),
                leaves: Box::new([]),
                unvisited_nodes: Box::new([]),
            }
        } else {
            let mut state = SnapshotBuilderFold::new(&nodes);
            let root_idx = state.fold(0);

            debug_assert!(state.branches.is_empty() || root_idx == state.branches.len() as Idx - 1);
            debug_assert_eq!(state.branch_count, state.branches.len() as u32);
            debug_assert_eq!(state.leaf_count, state.leaves.len() as u32);
            debug_assert_eq!(state.unvisited_count, state.unvisited_nodes.len() as u32);

            state.build()
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
    >
    where
        Db: DatabaseGet<V>,
    {
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

struct SnapshotBuilderFold<'v, 'a, V> {
    nodes: &'v [&'a NodeHashMaybeNode<'a, V>],
    /// The count of branches that will be in the snapshot
    branch_count: u32,
    /// The count of leaves that will be in the snapshot
    leaf_count: u32,
    /// The count of unvisited nodes that will be in the snapshot
    unvisited_count: u32,
    branches: Vec<Branch<Idx>>,
    leaves: Vec<Leaf<V>>,
    unvisited_nodes: Vec<NodeHash>,
}

impl<'v, 'a, V> SnapshotBuilderFold<'v, 'a, V> {
    fn new(nodes: &'v [&'a NodeHashMaybeNode<'_, V>]) -> Self {
        let mut branch_count = 0;
        let mut leaf_count = 0;
        let mut unvisited_count = 0;

        for (_, node) in nodes.iter() {
            match node {
                Some(Node::Branch(_)) => branch_count += 1,
                Some(Node::Leaf(_)) => leaf_count += 1,
                None => unvisited_count += 1,
            }
        }

        SnapshotBuilderFold {
            nodes,
            branch_count,
            leaf_count,
            unvisited_count,
            branches: Vec::with_capacity(branch_count as usize),
            leaves: Vec::with_capacity(leaf_count as usize),
            unvisited_nodes: Vec::with_capacity(unvisited_count as usize),
        }
    }

    fn push_branch(&mut self, branch: Branch<Idx>) -> Idx {
        let idx = self.branches.len() as Idx;
        self.branches.push(branch);
        idx
    }

    fn push_leaf(&mut self, leaf: Leaf<V>) -> Idx {
        let idx = self.leaves.len() as Idx;
        self.leaves.push(leaf);
        self.branch_count + idx
    }

    fn push_unvisited(&mut self, hash: NodeHash) -> Idx {
        let idx = self.unvisited_nodes.len() as Idx;
        self.unvisited_nodes.push(hash);
        self.branch_count + self.leaf_count + idx
    }

    fn fold(&mut self, node_idx: Idx) -> Idx
    where
        V: Clone,
    {
        match self.nodes[node_idx as usize] {
            (_, Some(Node::Branch(branch))) => {
                let left = self.fold(branch.left);
                let right = self.fold(branch.right);

                self.push_branch(Branch {
                    left,
                    right,
                    mask: branch.mask,
                    prior_word: branch.prior_word,
                    prefix: branch.prefix.clone(),
                })
            }
            // We could remove the clone by taking ownership of the SnapshotBuilder.
            // However, given this only runs on the server we can afford the clone.
            (_, Some(Node::Leaf(leaf))) => self.push_leaf((*leaf).clone()),
            (hash, None) => self.push_unvisited(*hash),
        }
    }

    fn build(self) -> Snapshot<V> {
        Snapshot {
            branches: self.branches.into_boxed_slice(),
            leaves: self.leaves.into_boxed_slice(),
            unvisited_nodes: self.unvisited_nodes.into_boxed_slice(),
        }
    }
}

use core::{cell::RefCell, ops::Deref};

use alloc::{boxed::Box, format, vec::Vec};
use bumpalo::Bump;
use ouroboros::self_referencing;

use crate::{
    transaction::nodes::{NodeRef, TrieRoot},
    Branch, Leaf, TrieError,
};

use super::{DatabaseGet, Idx, Node, NodeHash, Store};

type Result<T, E = TrieError> = core::result::Result<T, E>;

/// A snapshot of the merkle trie
///
/// Contains visited nodes and unvisited nodes
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot<V> {
    /// The last branch is the root of the trie if it exists.
    branches: Box<[Branch<Idx>]>,
    /// A Snapshot containing only
    leaves: Box<[Leaf<V>]>,

    // we only store the hashes of the nodes that have not been visited.
    unvisited_nodes: Box<[NodeHash]>,
}

impl<V: AsRef<[u8]>> Snapshot<V> {
    #[inline]
    pub fn root_node_idx(&self) -> Result<TrieRoot<Idx>> {
        // Revist this once https://github.com/rust-lang/rust/issues/37854 is stable
        match (
            self.branches.deref(),
            self.leaves.deref(),
            self.unvisited_nodes.deref(),
        ) {
            // A empty tree
            ([], [], []) => Ok(TrieRoot::Empty),
            // A tree with only one node
            ([_], [], []) | ([], [_], []) | ([], [], [_]) => Ok(TrieRoot::Node(0)),
            (branches, _, _) if !branches.is_empty() => {
                Ok(TrieRoot::Node(branches.len() as Idx - 1))
            }
            _ => Err(format!(
                "Invalid snapshot: \n\
                a tree with no branches can only have one leaf.\n\
                a tree with no branches or leaves can only have one unvisited node.\n\
                Found {} branches, {} leaves, and {} unvisited nodes",
                self.branches.len(),
                self.leaves.len(),
                self.unvisited_nodes.len()
            )
            .into()),
        }
    }

    #[inline]
    pub fn trie_root(&self) -> Result<TrieRoot<NodeRef<V>>> {
        match self.root_node_idx()? {
            TrieRoot::Node(idx) => Ok(TrieRoot::Node(NodeRef::Stored(idx))),
            TrieRoot::Empty => Ok(TrieRoot::Empty),
        }
    }

    /// Always check that the snapshot is of the merkle tree you expect.
    #[inline]
    pub fn calc_root_hash(&self) -> Result<TrieRoot<NodeHash>> {
        match self.root_node_idx()? {
            TrieRoot::Node(idx) => Ok(TrieRoot::Node(self.calc_subtree_hash(idx)?)),
            TrieRoot::Empty => Ok(TrieRoot::Empty),
        }
    }
}

impl<V: AsRef<[u8]>> Store<V> for Snapshot<V> {
    type Error = TrieError;

    // TODO fix possible stack overflow
    // I dislike using an explicit mutable stack.
    // I have an idea for abusing async for high performance segmented stacks
    #[inline]
    fn calc_subtree_hash(&self, node: Idx) -> Result<NodeHash> {
        let idx = node as usize;
        let leaf_offset = self.branches.len();
        let unvisited_offset = leaf_offset + self.leaves.len();

        if let Some(branch) = self.branches.get(idx) {
            let left = self.calc_subtree_hash(branch.left)?;
            let right = self.calc_subtree_hash(branch.right)?;

            Ok(branch.hash_branch(&left, &right))
        } else if let Some(leaf) = self.leaves.get(idx - leaf_offset) {
            Ok(leaf.hash_leaf())
        } else if let Some(hash) = self.unvisited_nodes.get(idx - unvisited_offset) {
            Ok(*hash)
        } else {
            Err(format!(
                "Invalid snapshot: node {} not found\n\
                Snapshot has {} branches, {} leaves, and {} unvisited nodes",
                idx,
                self.branches.len(),
                self.leaves.len(),
                self.unvisited_nodes.len(),
            )
            .into())
        }
    }

    #[inline]
    fn get_node(&self, idx: Idx) -> Result<Node<&Branch<Idx>, &Leaf<V>>> {
        let idx = idx as usize;
        let leaf_offset = self.branches.len();
        let unvisited_offset = leaf_offset + self.leaves.len();

        if idx < leaf_offset {
            Ok(Node::Branch(&self.branches[idx]))
        } else if idx < unvisited_offset {
            Ok(Node::Leaf(&self.leaves[idx - leaf_offset]))
        } else {
            Err(format!(
                "Invalid snapshot: no visited node at index {}\n\
                Snapshot has {} branches, {} leaves, and {} unvisited nodes",
                idx,
                self.branches.len(),
                self.leaves.len(),
                self.unvisited_nodes.len(),
            )
            .into())
        }
    }
}

type NodeHashMaybeNode<'a, V> = (&'a NodeHash, Option<Node<&'a Branch<Idx>, &'a Leaf<V>>>);

#[self_referencing]
pub struct SnapshotBuilder<Db: 'static, V: 'static> {
    db: Db,
    bump: Bump,

    /// The root of the trie is always at index 0
    #[borrows(bump)]
    #[not_covariant]
    nodes: RefCell<Vec<NodeHashMaybeNode<'this, V>>>,
}

impl<Db: DatabaseGet<V>, V: Clone> Store<V> for SnapshotBuilder<Db, V> {
    type Error = TrieError;

    #[inline]
    fn calc_subtree_hash(&self, hash_idx: Idx) -> Result<NodeHash, Self::Error> {
        let hash_idx = hash_idx as usize;

        self.with_nodes(|nodes| {
            let nodes = nodes.borrow();
            nodes.get(hash_idx).map(|(hash, _)| **hash).ok_or_else(|| {
                format!(
                    "Invalid snapshot: no unvisited node at index {}\n\
                        SnapshotBuilder has {} nodes",
                    hash_idx,
                    nodes.len()
                )
                .into()
            })
        })
    }

    #[inline]
    fn get_node(&self, hash_idx: Idx) -> Result<Node<&Branch<Idx>, &Leaf<V>>, Self::Error> {
        let hash_idx = hash_idx as usize;
        self.with(|this| {
            let mut nodes = this.nodes.borrow_mut();

            let Some((hash, o_node)) = nodes.get(hash_idx).map(|(hash, o_node)| (hash, *o_node))
            else {
                return Err(format!(
                    "Invalid snapshot: no node at index {}\n\
                SnapshotBuilder has {} nodes",
                    hash_idx,
                    nodes.len()
                )
                .into());
            };

            if let Some(node) = o_node {
                return Ok(node);
            }

            let node = this
                .db
                .get(hash)
                .map_err(|e| format!("Error getting {hash} from database: `{e}`"))?;

            let node = match node {
                Node::Branch(Branch {
                    mask,
                    left,
                    right,
                    prior_word,
                    prefix,
                }) => {
                    let idx = nodes.len() as Idx;

                    let left = this.bump.alloc(left);
                    let right = this.bump.alloc(right);

                    nodes.push((&*left, None));
                    nodes.push((&*right, None));

                    Node::Branch(&*this.bump.alloc(Branch {
                        mask,
                        left: idx,
                        right: idx + 1,
                        prior_word,
                        prefix,
                    }))
                }
                Node::Leaf(leaf) => Node::Leaf(&*this.bump.alloc(leaf)),
            };

            nodes[hash_idx].1 = Some(node);
            Ok(node)
        })
    }
}

impl<Db, V> SnapshotBuilder<Db, V> {
    #[inline]
    pub fn empty(db: Db) -> Self {
        SnapshotBuilderBuilder {
            db,
            bump: Bump::new(),
            nodes_builder: |_| RefCell::new(Vec::new()),
        }
        .build()
    }

    #[inline]
    pub fn db(&self) -> &Db {
        self.borrow_db()
    }

    #[inline]
    pub fn with_trie_root_hash(self, root_hash: TrieRoot<NodeHash>) -> Self {
        match root_hash {
            TrieRoot::Node(hash) => self.with_root_hash(hash),
            TrieRoot::Empty => self,
        }
    }

    #[inline]
    pub fn with_root_hash(self, root_hash: NodeHash) -> Self {
        self.with(|this| {
            let root_hash = this.bump.alloc(root_hash);
            this.nodes.borrow_mut().push((&*root_hash, None));
        });
        self
    }

    #[inline]
    pub fn trie_root(&self) -> TrieRoot<NodeRef<V>> {
        self.with_nodes(|nodes| match nodes.borrow().first() {
            Some(_) => TrieRoot::Node(NodeRef::Stored(0)),
            None => TrieRoot::Empty,
        })
    }

    #[inline]
    pub fn build_initial_snapshot(&self) -> Snapshot<V>
    where
        V: Clone,
    {
        self.with_nodes(|nodes| {
            let nodes = nodes.borrow();
            if nodes.is_empty() {
                Snapshot {
                    branches: Box::new([]),
                    leaves: Box::new([]),
                    unvisited_nodes: Box::new([]),
                }
            } else {
                let mut state = SnapshotBuilderFold::new(&nodes);
                let root_idx = state.fold(0);

                debug_assert!(
                    state.branches.is_empty() || root_idx == state.branches.len() as Idx - 1
                );
                debug_assert_eq!(state.branch_count, state.branches.len() as u32);
                debug_assert_eq!(state.leaf_count, state.leaves.len() as u32);
                debug_assert_eq!(state.unvisited_count, state.unvisited_nodes.len() as u32);

                state.build()
            }
        })
    }
}

struct SnapshotBuilderFold<'v, 'a, V> {
    nodes: &'v [NodeHashMaybeNode<'a, V>],
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
    #[inline]
    fn new(nodes: &'v [NodeHashMaybeNode<'a, V>]) -> Self {
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

    #[inline]
    fn push_branch(&mut self, branch: Branch<Idx>) -> Idx {
        let idx = self.branches.len() as Idx;
        self.branches.push(branch);
        idx
    }

    #[inline]
    fn push_leaf(&mut self, leaf: Leaf<V>) -> Idx {
        let idx = self.leaves.len() as Idx;
        self.leaves.push(leaf);
        self.branch_count + idx
    }

    #[inline]
    fn push_unvisited(&mut self, hash: NodeHash) -> Idx {
        let idx = self.unvisited_nodes.len() as Idx;
        self.unvisited_nodes.push(hash);
        self.branch_count + self.leaf_count + idx
    }

    #[inline]
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

    #[inline]
    fn build(self) -> Snapshot<V> {
        Snapshot {
            branches: self.branches.into_boxed_slice(),
            leaves: self.leaves.into_boxed_slice(),
            unvisited_nodes: self.unvisited_nodes.into_boxed_slice(),
        }
    }
}

use core::{cell::RefCell, ops::Deref};

use alloc::{boxed::Box, format, vec::Vec};
use bumpalo::Bump;
use ouroboros::self_referencing;

use crate::{
    transaction::nodes::{NodeRef, TrieRoot},
    Branch, Leaf, PortableHash, PortableHasher, TrieError,
};

use super::{DatabaseGet, Idx, Node, NodeHash, Store};

type Result<T, E = TrieError> = core::result::Result<T, E>;

/// A snapshot of the merkle trie verified
///
/// Contains visited nodes and unvisited nodes
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedSnapshot<S: Store> {
    snapshot: S,

    /// The root hash of the snapshot is the last hash in the slice.
    /// The indexes of each hash match the indexes of nodes in the snapshot.
    branch_hashes: Box<[NodeHash]>,
    leaf_hashes: Box<[NodeHash]>,
}

impl<S: Store + AsRef<Snapshot<S::Value>>> VerifiedSnapshot<S> {
    /// Verify the snapshot by checking that it is well formed and calculating the merkle hashes of all nodes.
    /// The merkle hashes are cached such that `calc_subtree_hash` is an O(1) operation for all nodes in the snapshot.
    /// In practice this means if a transaction gets, but does not modify a node the hash is never recalculated.
    ///
    /// Storing the hashes of all nodes does increase memory usage,
    /// so for some use cases it may be better to use `Snapshot` directly.
    /// If you choose to do that, be sure to run `calc_root_hash` before using the snapshot.
    #[inline]
    pub fn verify_snapshot(snapshot: S, hasher: &mut impl PortableHasher<32>) -> Result<Self> {
        let snapshot_ref = snapshot.as_ref();

        // Check that the snapshot is well formed.
        let _ = snapshot_ref.root_node_idx()?;

        let mut leaf_hashes = Vec::with_capacity(snapshot_ref.leaves.len());
        let mut branch_hashes = Vec::with_capacity(snapshot_ref.branches.len());

        for leaf in snapshot_ref.leaves.iter() {
            leaf_hashes.push(leaf.hash_leaf(hasher));
        }

        let leaf_offset = snapshot_ref.branches.len();
        let unvisited_offset = leaf_offset + snapshot_ref.leaves.len();

        for (idx, branch) in snapshot_ref.branches.iter().enumerate() {
            let hash_of_child = |child| {
                if child < leaf_offset {
                    branch_hashes.get(child).ok_or_else(|| {
                        format!(
                            "Invalid snapshot: branch {} has child {},\
                            child branch index must be less than parent, branches are not in post-order traversal",
                            idx, child
                        )
                    })
                } else if child < unvisited_offset {
                    leaf_hashes.get(child - leaf_offset).ok_or_else(|| {
                        format!(
                            "Invalid snapshot: branch {} has child {},\
                            child leaf does not exist",
                            idx, child
                        )
                    })
                } else {
                    snapshot_ref
                        .unvisited_nodes
                        .get(child - unvisited_offset)
                        .ok_or_else(|| {
                            format!(
                                "Invalid snapshot: branch {} has child {},\
                                child unvisited node does not exist",
                                idx, child
                            )
                        })
                }
            };

            let left_hash = hash_of_child(branch.left as usize)?;
            let right_hash = hash_of_child(branch.right as usize)?;

            branch_hashes.push(branch.hash_branch(hasher, left_hash, right_hash));
        }

        Ok(VerifiedSnapshot {
            snapshot,
            branch_hashes: branch_hashes.into_boxed_slice(),
            leaf_hashes: leaf_hashes.into_boxed_slice(),
        })
    }

    #[inline]
    pub fn trie_root(&self) -> TrieRoot<NodeRef<S::Value>> {
        let snapshot = self.snapshot.as_ref();

        if !snapshot.branches.is_empty() {
            TrieRoot::Node(NodeRef::Stored(snapshot.branches.len() as Idx - 1))
        } else if snapshot.leaves.is_empty() && snapshot.unvisited_nodes.is_empty() {
            TrieRoot::Empty
        } else {
            // We know that the snapshot is valid, because we verified it in `verify_snapshot`.
            // If a non-empty snapshot contains no branches, it must have a single leaf or unvisited node.
            // Any other combination is invalid.
            debug_assert_eq!(snapshot.leaves.len() + snapshot.unvisited_nodes.len(), 1);
            TrieRoot::Node(NodeRef::Stored(0))
        }
    }

    /// Returns the merkle root hash of the trie in the snapshot.
    /// The hash of all nodes has already been calculated in `VerifiedSnapshot::verify_snapshot`.
    /// `trie_root_hash` and `calc_root_hash` are both O(1) operations on a `VerifiedSnapshot`,
    /// unlike `Snapshot` and `SnapshotBuilder`.
    #[inline]
    pub fn trie_root_hash(&self) -> TrieRoot<NodeHash> {
        self.branch_hashes
            .last()
            // Given a valid snapshot: if no branches exist, there can only be one leaf or one unvisited node.
            .or_else(|| self.leaf_hashes.first())
            .or_else(|| self.snapshot.as_ref().unvisited_nodes.first())
            .map_or(TrieRoot::Empty, |hash| TrieRoot::Node(*hash))
    }
}

impl<S: Store + AsRef<Snapshot<S::Value>>> Store for VerifiedSnapshot<S> {
    type Error = TrieError;
    type Value = S::Value;

    #[inline]
    fn calc_subtree_hash(
        &self,
        _: &mut impl PortableHasher<32>,
        node: Idx,
    ) -> Result<NodeHash, Self::Error> {
        let snapshot = self.snapshot.as_ref();

        let idx = node as usize;
        let leaf_offset = snapshot.branches.len();
        let unvisited_offset = leaf_offset + snapshot.leaves.len();

        if let Some(branch) = self.branch_hashes.get(idx) {
            Ok(*branch)
        } else if let Some(leaf) = self.leaf_hashes.get(idx - leaf_offset) {
            Ok(*leaf)
        } else if let Some(hash) = snapshot.unvisited_nodes.get(idx - unvisited_offset) {
            Ok(*hash)
        } else {
            Err(format!(
                "Invalid arg: node {} does not exist\n\
                Snapshot has {} nodes",
                idx,
                snapshot.branches.len() + snapshot.leaves.len() + snapshot.unvisited_nodes.len(),
            )
            .into())
        }
    }

    #[inline]
    fn get_node(&self, idx: Idx) -> Result<Node<&Branch<Idx>, &Leaf<S::Value>>> {
        let snapshot = self.snapshot.as_ref();

        let idx = idx as usize;
        let leaf_offset = snapshot.branches.len();
        let unvisited_offset = leaf_offset + snapshot.leaves.len();

        if let Some(branch) = snapshot.branches.get(idx) {
            Ok(Node::Branch(branch))
        } else if let Some(leaf) = snapshot.leaves.get(idx - leaf_offset) {
            Ok(Node::Leaf(leaf))
        } else if snapshot
            .unvisited_nodes
            .get(idx - unvisited_offset)
            .is_some()
        {
            Err(format!(
                "Invalid arg: node {idx} is unvisited\n\
                get_node can only return visited nodes"
            )
            .into())
        } else {
            Err(format!(
                "Invalid arg: node {} does not exist\n\
                Snapshot has {} nodes",
                idx,
                snapshot.branches.len() + snapshot.leaves.len() + snapshot.unvisited_nodes.len(),
            )
            .into())
        }
    }

    #[inline]
    fn get_node_hash(
        &self,
        _hash_idx: Idx,
    ) -> Result<NodeHash, Self::Error> {
        Ok(NodeHash::new([0; 32])) // TODO: implement
    }
}

/// A snapshot of the merkle trie
///
/// Contains visited nodes and unvisited nodes
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot<V> {
    /// The last branch is the root of the trie if it exists.
    branches: Box<[Branch<Idx>]>,
    /// A Snapshot containing only
    leaves: Box<[Leaf<V>]>,

    // we only store the hashes of the nodes that have not been visited.
    unvisited_nodes: Box<[NodeHash]>,
}

impl<V> AsRef<Snapshot<V>> for Snapshot<V> {
    #[inline]
    fn as_ref(&self) -> &Snapshot<V> {
        self
    }
}

impl<V: Clone + PortableHash> Snapshot<V> {
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
            // A tree with only one node, it must be a leaf or unvisited node.
            // It can't be a branch because branches have children.
            ([], [_], []) | ([], [], [_]) => Ok(TrieRoot::Node(0)),
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

    /// Calculate the merkle root hash of the snapshot.
    /// This computation can be thought of as verifying a Snapshot has a particular Merkle root hash.
    /// However, in reality, it is calculating the root hash of the snapshot
    /// by visiting all nodes touched by the transaction.
    ///
    /// Always check that the snapshot is of the merkle tree you expect.
    #[inline]
    pub fn calc_root_hash(
        &self,
        hasher: &mut impl PortableHasher<32>,
    ) -> Result<TrieRoot<NodeHash>> {
        match self.root_node_idx()? {
            TrieRoot::Node(idx) => Ok(TrieRoot::Node(self.calc_subtree_hash(hasher, idx)?)),
            TrieRoot::Empty => Ok(TrieRoot::Empty),
        }
    }

    /// Verify the snapshot by checking that it is well formed and calculating the merkle hashes of all nodes.
    /// This is an alias for `VerifiedSnapshot::verify_snapshot`.
    #[inline]
    pub fn verify_ref(
        &self,
        hasher: &mut impl PortableHasher<32>,
    ) -> Result<VerifiedSnapshot<&Self>> {
        VerifiedSnapshot::verify_snapshot(self, hasher)
    }

    /// Verify the snapshot by checking that it is well formed and calculating the merkle hashes of all nodes.
    /// This is an alias for `VerifiedSnapshot::verify_snapshot`.
    #[inline]
    pub fn verify(self, hasher: &mut impl PortableHasher<32>) -> Result<VerifiedSnapshot<Self>> {
        VerifiedSnapshot::verify_snapshot(self, hasher)
    }
}

impl<V: Clone + PortableHash> Store for Snapshot<V> {
    type Error = TrieError;
    type Value = V;

    // TODO fix possible stack overflow
    // I dislike using an explicit mutable stack.
    // I have an idea for abusing async for high performance segmented stacks
    /// Calculate the hash of the subtree.
    /// If you know the hashes of both children, you should use `Branch::hash_branch` instead.
    ///
    /// Caller must ensure that the hasher is reset before calling this function.
    #[inline]
    fn calc_subtree_hash(
        &self,
        hasher: &mut impl PortableHasher<32>,
        node: Idx,
    ) -> Result<NodeHash> {
        let idx = node as usize;
        let leaf_offset = self.branches.len();
        let unvisited_offset = leaf_offset + self.leaves.len();

        if let Some(branch) = self.branches.get(idx) {
            let left = self.calc_subtree_hash(hasher, branch.left)?;
            let right = self.calc_subtree_hash(hasher, branch.right)?;

            Ok(branch.hash_branch(hasher, &left, &right))
        } else if let Some(leaf) = self.leaves.get(idx - leaf_offset) {
            Ok(leaf.hash_leaf(hasher))
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
    #[inline]
    fn get_node_hash(
        &self,
        _hash_idx: Idx,
    ) -> Result<NodeHash, Self::Error> {
        Ok(NodeHash::new([0; 32])) // TODO: implement
    }
}

type NodeHashMaybeNode<'a, V> = (&'a NodeHash, Option<Node<&'a Branch<Idx>, &'a Leaf<V>>>);

pub struct SnapshotBuilder<Db: 'static, V: 'static> {
    inner: SnapshotBuilderInner<Db, V>,
}

#[self_referencing]
struct SnapshotBuilderInner<Db: 'static, V: 'static> {
    db: Db,
    bump: Bump,

    /// The root of the trie is always at index 0
    #[borrows(bump)]
    #[not_covariant]
    nodes: RefCell<Vec<NodeHashMaybeNode<'this, V>>>,
}

impl<Db: DatabaseGet<V>, V: Clone + PortableHash> Store for SnapshotBuilder<Db, V> {
    type Error = TrieError;
    type Value = V;

    #[inline]
    fn calc_subtree_hash(
        &self,
        _: &mut impl PortableHasher<32>,
        hash_idx: Idx,
    ) -> Result<NodeHash, Self::Error> {
        let hash_idx = hash_idx as usize;

        self.inner.with_nodes(|nodes| {
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
        self.inner.with(|this| {
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

    #[inline]
    fn get_node_hash(
        &self,
        hash_idx: Idx,
    ) -> Result<NodeHash, Self::Error> {
        self.inner.with_nodes(|nodes| {
            let nodes = nodes.borrow();
            nodes
                .get(hash_idx as usize)
                .map(|(hash, _)| **hash)
                .ok_or_else(|| {
                    format!(
                        "Invalid snapshot: no node at index {}\n\
                        SnapshotBuilder has {} nodes",
                        hash_idx,
                        nodes.len()
                    )
                    .into()
                })
        })
    }
}

impl<Db, V> SnapshotBuilderInner<Db, V> {
    fn new_with_db(db: Db) -> Self {
        SnapshotBuilderInnerBuilder {
            db,
            bump: Bump::new(),
            nodes_builder: |_| RefCell::new(Vec::new()),
        }
        .build()
    }
}

impl<Db, V> SnapshotBuilder<Db, V> {
    /// Create a new `SnapshotBuilder` with the given database from a trie root hash.
    ///
    /// This is an alias for `SnapshotBuilderBuilder::empty(db).with_trie_root_hash(root_hash)`.
    #[inline]
    pub fn new(db: Db, root_hash: TrieRoot<NodeHash>) -> Self {
        SnapshotBuilder::empty(db).with_trie_root_hash(root_hash)
    }

    #[inline]
    pub fn empty(db: Db) -> Self {
        SnapshotBuilder {
            inner: SnapshotBuilderInner::new_with_db(db),
        }
    }

    #[inline]
    pub fn db(&self) -> &Db {
        self.inner.borrow_db()
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
        self.inner.with(|this| {
            let root_hash = this.bump.alloc(root_hash);
            this.nodes.borrow_mut().push((&*root_hash, None));
        });
        self
    }

    #[inline]
    pub fn trie_root(&self) -> TrieRoot<NodeRef<V>> {
        self.inner.with_nodes(|nodes| match nodes.borrow().first() {
            Some(_) => TrieRoot::Node(NodeRef::Stored(0)),
            None => TrieRoot::Empty,
        })
    }

    #[inline]
    pub fn get_node_hash(&self, idx: Idx) -> Result<NodeHash, TrieError> {
        self.inner.with_nodes(|nodes| {
            let nodes = nodes.borrow();
            nodes
                .get(idx as usize)
                .map(|(hash, _)| **hash)
                .ok_or_else(|| {
                    TrieError::from(format!(
                        "Invalid snapshot: no node at index {}\n\
                    SnapshotBuilder has {} nodes",
                        idx,
                        nodes.len()
                    ))
                })
        })
    }

    #[inline]
    pub fn build_initial_snapshot(&self) -> Snapshot<V>
    where
        V: Clone,
    {
        self.inner.with_nodes(|nodes| {
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

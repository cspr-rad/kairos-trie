pub(crate) mod nodes;

use alloc::{boxed::Box, format};
use core::{mem, usize};

use crate::{stored, KeyHash, NodeHash, PortableHash, PortableHasher};
use crate::{
    stored::{
        merkle::{Snapshot, SnapshotBuilder},
        DatabaseSet, Store,
    },
    TrieError,
};

use self::nodes::{
    Branch, KeyPosition, KeyPositionAdjacent, Leaf, Node, NodeRef, StoredLeafRef, TrieRoot,
};

pub struct Transaction<S, V> {
    pub data_store: S,
    current_root: TrieRoot<NodeRef<V>>,
}

impl<Db: DatabaseSet<V>, V: Clone + PortableHash> Transaction<SnapshotBuilder<Db, V>, V> {
    /// Write modified nodes to the database and return the root hash.
    /// Calling this method will write all modified nodes to the database.
    /// Calling this method again will rewrite the nodes to the database.
    ///
    /// Caching writes is the responsibility of the `DatabaseSet` implementation.
    ///
    /// Caller must ensure that the hasher is reset before calling this method.
    #[inline]
    pub fn commit(
        &self,
        hasher: &mut impl PortableHasher<32>,
    ) -> Result<TrieRoot<NodeHash>, TrieError> {
        let store_modified_branch =
            &mut |hash: &NodeHash, branch: &Branch<NodeRef<V>>, left: NodeHash, right: NodeHash| {
                let branch = Branch {
                    left,
                    right,
                    mask: branch.mask,
                    prior_word: branch.prior_word,
                    prefix: branch.prefix.clone(),
                };

                self.data_store
                    .db()
                    .set(*hash, Node::Branch(branch))
                    .map_err(|e| format!("Error writing branch {hash} to database: {e}").into())
            };

        let store_modified_leaf = &mut |hash: &NodeHash, leaf: &Leaf<V>| {
            self.data_store
                .db()
                .set(*hash, Node::Leaf(leaf.clone()))
                .map_err(|e| format!("Error writing leaf {hash} to database: {e}").into())
        };

        let root_hash =
            self.calc_root_hash_inner(hasher, store_modified_branch, store_modified_leaf)?;
        Ok(root_hash)
    }
}

impl<S: Store<V>, V: PortableHash> Transaction<S, V> {
    /// Caller must ensure that the hasher is reset before calling this method.
    #[inline]
    pub fn calc_root_hash_inner(
        &self,
        hasher: &mut impl PortableHasher<32>,
        on_modified_branch: &mut impl FnMut(
            &NodeHash,
            &Branch<NodeRef<V>>,
            NodeHash,
            NodeHash,
        ) -> Result<(), TrieError>,
        on_modified_leaf: &mut impl FnMut(&NodeHash, &Leaf<V>) -> Result<(), TrieError>,
    ) -> Result<TrieRoot<NodeHash>, TrieError> {
        let root_hash = match &self.current_root {
            TrieRoot::Empty => return Ok(TrieRoot::Empty),
            TrieRoot::Node(node_ref) => Self::calc_root_hash_node(
                hasher,
                &self.data_store,
                node_ref,
                on_modified_leaf,
                on_modified_branch,
            )?,
        };

        Ok(TrieRoot::Node(root_hash))
    }

    /// Calculate the root hash of the trie.
    ///
    /// Caller must ensure that the hasher is reset before calling this method.
    #[inline]
    pub fn calc_root_hash(
        &self,
        hasher: &mut impl PortableHasher<32>,
    ) -> Result<TrieRoot<NodeHash>, TrieError> {
        self.calc_root_hash_inner(hasher, &mut |_, _, _, _| Ok(()), &mut |_, _| Ok(()))
    }

    #[inline]
    fn calc_root_hash_node(
        hasher: &mut impl PortableHasher<32>,
        data_store: &S,
        node_ref: &NodeRef<V>,
        on_modified_leaf: &mut impl FnMut(&NodeHash, &Leaf<V>) -> Result<(), TrieError>,
        on_modified_branch: &mut impl FnMut(
            &NodeHash,
            &Branch<NodeRef<V>>,
            NodeHash,
            NodeHash,
        ) -> Result<(), TrieError>,
    ) -> Result<NodeHash, TrieError> {
        // TODO use a stack instead of recursion
        match node_ref {
            NodeRef::ModBranch(branch) => {
                let left = Self::calc_root_hash_node(
                    hasher,
                    data_store,
                    &branch.left,
                    on_modified_leaf,
                    on_modified_branch,
                )?;
                let right = Self::calc_root_hash_node(
                    hasher,
                    data_store,
                    &branch.right,
                    on_modified_leaf,
                    on_modified_branch,
                )?;

                let hash = branch.hash_branch(hasher, &left, &right);
                on_modified_branch(&hash, branch, left, right)?;
                Ok(hash)
            }
            NodeRef::ModLeaf(leaf) => {
                let hash = leaf.hash_leaf(hasher);

                on_modified_leaf(&hash, leaf)?;
                Ok(hash)
            }
            NodeRef::Stored(stored_idx) => data_store
                .calc_subtree_hash(hasher, *stored_idx)
                .map_err(|e| {
                    format!(
                        "Error in `calc_root_hash_node`: {e} at {file}:{line}:{column}",
                        file = file!(),
                        line = line!(),
                        column = column!()
                    )
                    .into()
                }),
        }
    }
}

impl<S: Store<V>, V> Transaction<S, V> {
    #[inline]
    pub fn get(&self, key_hash: &KeyHash) -> Result<Option<&V>, TrieError> {
        match &self.current_root {
            TrieRoot::Empty => Ok(None),
            TrieRoot::Node(node_ref) => Self::get_node(&self.data_store, node_ref, key_hash),
        }
    }

    #[inline]
    pub fn get_node<'root, 's: 'root>(
        data_store: &'s S,
        mut node_ref: &'root NodeRef<V>,
        key_hash: &KeyHash,
    ) -> Result<Option<&'root V>, TrieError> {
        loop {
            match node_ref {
                NodeRef::ModBranch(branch) => match branch.key_position(key_hash) {
                    KeyPosition::Left => node_ref = &branch.left,
                    KeyPosition::Right => node_ref = &branch.right,
                    KeyPosition::Adjacent(_) => return Ok(None),
                },
                NodeRef::ModLeaf(leaf) => {
                    if leaf.key_hash == *key_hash {
                        return Ok(Some(&leaf.value));
                    } else {
                        return Ok(None);
                    }
                }
                NodeRef::Stored(stored_idx) => {
                    return Self::get_stored_node(data_store, *stored_idx, key_hash);
                }
            }
        }
    }

    #[inline]
    pub fn get_stored_node<'s>(
        data_store: &'s S,
        mut stored_idx: stored::Idx,
        key_hash: &KeyHash,
    ) -> Result<Option<&'s V>, TrieError> {
        loop {
            let node = data_store
                .get_node(stored_idx)
                .map_err(|e| format!("Error in `get_stored_node`: {e}"))?;

            match node {
                Node::Branch(branch) => match branch.key_position(key_hash) {
                    KeyPosition::Left => stored_idx = branch.left,
                    KeyPosition::Right => stored_idx = branch.right,
                    KeyPosition::Adjacent(_) => return Ok(None),
                },
                Node::Leaf(leaf) => {
                    if leaf.key_hash == *key_hash {
                        break;
                    } else {
                        return Ok(None);
                    }
                }
            }
        }

        match data_store
            .get_node(stored_idx)
            .map_err(|e| format!("Error in `get_stored_node`: {e}"))?
        {
            Node::Leaf(leaf) => Ok(Some(&leaf.value)),
            _ => unreachable!("Prior loop only breaks on a leaf"),
        }
    }

    #[inline]
    pub fn insert(&mut self, key_hash: &KeyHash, value: V) -> Result<(), TrieError> {
        match &mut self.current_root {
            TrieRoot::Empty => {
                self.current_root = TrieRoot::Node(NodeRef::ModLeaf(Box::new(Leaf {
                    key_hash: *key_hash,
                    value,
                })));
                Ok(())
            }
            TrieRoot::Node(node_ref) => {
                Self::insert_node(&mut self.data_store, node_ref, key_hash, value)
            }
        }
    }

    #[inline(always)]
    fn insert_node<'root, 's: 'root>(
        data_store: &'s mut S,
        mut node_ref: &'root mut NodeRef<V>,
        key_hash: &KeyHash,
        value: V,
    ) -> Result<(), TrieError> {
        loop {
            match node_ref {
                NodeRef::ModBranch(branch) => match branch.key_position(key_hash) {
                    KeyPosition::Left => {
                        node_ref = &mut branch.left;
                        continue;
                    }
                    KeyPosition::Right => {
                        node_ref = &mut branch.right;
                        continue;
                    }
                    KeyPosition::Adjacent(pos) => {
                        branch.new_adjacent_leaf(
                            pos,
                            Box::new(Leaf {
                                key_hash: *key_hash,
                                value,
                            }),
                        );

                        return Ok(());
                    }
                },
                NodeRef::ModLeaf(leaf) => {
                    if leaf.key_hash == *key_hash {
                        leaf.value = value;

                        return Ok(());
                    } else {
                        let old_leaf = mem::replace(node_ref, NodeRef::temp_null_stored());
                        let NodeRef::ModLeaf(old_leaf) = old_leaf else {
                            unreachable!("We just matched a ModLeaf");
                        };
                        let new_leaf = Box::new(Leaf {
                            key_hash: *key_hash,
                            value,
                        });

                        let (new_branch, _) = Branch::new_from_leafs(0, old_leaf, new_leaf);

                        *node_ref = NodeRef::ModBranch(new_branch);
                        return Ok(());
                    }
                }
                NodeRef::Stored(stored_idx) => {
                    let new_node = data_store.get_node(*stored_idx).map_err(|e| {
                        format!("Error at `{}:{}:{}`: `{e}`", file!(), line!(), column!())
                    })?;
                    match new_node {
                        Node::Branch(new_branch) => {
                            *node_ref = NodeRef::ModBranch(Box::new(Branch {
                                left: NodeRef::Stored(new_branch.left),
                                right: NodeRef::Stored(new_branch.right),
                                mask: new_branch.mask,
                                prior_word: new_branch.prior_word,
                                prefix: new_branch.prefix.clone(),
                            }));

                            continue;
                        }
                        Node::Leaf(leaf) => {
                            if leaf.key_hash == *key_hash {
                                *node_ref = NodeRef::ModLeaf(Box::new(Leaf {
                                    key_hash: *key_hash,
                                    value,
                                }));

                                return Ok(());
                            } else {
                                let (new_branch, _) = Branch::new_from_leafs(
                                    // TODO we can use the most recent branch.word_idx - 1
                                    // not sure if it's worth it, 0 is always correct.
                                    0,
                                    StoredLeafRef::new(leaf, *stored_idx),
                                    Box::new(Leaf {
                                        key_hash: *key_hash,
                                        value,
                                    }),
                                );

                                *node_ref = NodeRef::ModBranch(new_branch);
                                return Ok(());
                            }
                        }
                    }
                }
            }
        }
    }
}

impl<S: Store<V>, V: PortableHash + Clone> Transaction<S, V> {
    /// This method allows for getting, inserting, and updating a entry in the trie with a single lookup.
    /// We match the standard library's `Entry` API for the most part.
    ///
    /// Note: Use of `entry` renders the trie path even if the entry is not modified.
    /// This incurs allocations, now and unnecessary rehashing later when calculating the root hash.
    /// For this reason you should prefer `get` if you have a high probability of not modifying the entry.
    #[inline]
    pub fn entry<'txn>(&'txn mut self, key_hash: &KeyHash) -> Result<Entry<'txn, V>, TrieError> {
        let mut key_position = KeyPositionAdjacent::PrefixOfWord(usize::MAX);

        match self.current_root {
            TrieRoot::Empty => Ok(Entry::VacantEmptyTrie(VacantEntryEmptyTrie {
                root: &mut self.current_root,
                key_hash: *key_hash,
            })),
            TrieRoot::Node(ref mut root) => {
                let mut node_ref = root;
                loop {
                    let go_right = match &*node_ref {
                        NodeRef::ModBranch(branch) => match branch.key_position(key_hash) {
                            KeyPosition::Left => false,
                            KeyPosition::Right => true,
                            KeyPosition::Adjacent(pos) => {
                                key_position = pos;
                                break;
                            }
                        },
                        NodeRef::ModLeaf(_) => break,
                        NodeRef::Stored(idx) => {
                            let loaded_node = self.data_store.get_node(*idx).map_err(|e| {
                                format!(
                                    "Error in `entry` at {file}:{line}:{column}: could not get stored node: {e}",
                                    file = file!(),
                                    line = line!(),
                                    column = column!(),
                                )
                            })?;

                            match loaded_node {
                                Node::Branch(branch) => {
                                    // Connect the new branch to the trie.
                                    *node_ref =
                                        NodeRef::ModBranch(Box::new(Branch::from_stored(branch)));
                                }
                                Node::Leaf(leaf) => {
                                    *node_ref = NodeRef::ModLeaf(Box::new(leaf.clone()));
                                }
                            }
                            continue;
                        }
                    };

                    match (go_right, node_ref) {
                        (true, NodeRef::ModBranch(ref mut branch)) => {
                            node_ref = &mut branch.right;
                        }
                        (false, NodeRef::ModBranch(ref mut branch)) => {
                            node_ref = &mut branch.left;
                        }
                        _ => unreachable!("We just matched a ModBranch"),
                    }
                }

                // This convoluted return makes the borrow checker happy.
                if let NodeRef::ModLeaf(leaf) = &*node_ref {
                    if leaf.key_hash != *key_hash {
                        // This is a logical null
                        // TODO we should break VacantEntry into two types VacantEntryBranch and VacantEntryLeaf
                        debug_assert_eq!(
                            key_position,
                            KeyPositionAdjacent::PrefixOfWord(usize::MAX)
                        );

                        return Ok(Entry::Vacant(VacantEntry {
                            parent: node_ref,
                            key_hash: *key_hash,
                            key_position,
                        }));
                    }
                };

                if let NodeRef::ModBranch(_) = &*node_ref {
                    Ok(Entry::Vacant(VacantEntry {
                        parent: node_ref,
                        key_hash: *key_hash,
                        key_position,
                    }))
                } else if let NodeRef::ModLeaf(leaf) = &mut *node_ref {
                    Ok(Entry::Occupied(OccupiedEntry { leaf }))
                } else {
                    unreachable!("prior loop only breaks on a leaf or branch");
                }
            }
        }
    }
}

impl<Db, V: PortableHash + Clone> Transaction<SnapshotBuilder<Db, V>, V> {
    /// An alias for `SnapshotBuilder::new_with_db`.
    ///
    /// Builds a snapshot of the trie before the transaction.
    /// The `Snapshot` is not a complete representation of the trie.
    /// The `Snapshot` only contains information about the parts of the trie touched by the transaction.
    /// Because of this, two `Snapshot`s of the same trie may not be equal if the transactions differ.
    ///
    /// Note: All operations including get affect the contents of the snapshot.
    #[inline]
    pub fn build_initial_snapshot(&self) -> Snapshot<V> {
        self.data_store.build_initial_snapshot()
    }

    #[inline]
    pub fn from_snapshot_builder(builder: SnapshotBuilder<Db, V>) -> Self {
        Transaction {
            current_root: builder.trie_root(),
            data_store: builder,
        }
    }
}

impl<'s, V: PortableHash + Clone> Transaction<&'s Snapshot<V>, V> {
    #[inline]
    pub fn from_snapshot(snapshot: &'s Snapshot<V>) -> Result<Self, TrieError> {
        Ok(Transaction {
            current_root: snapshot.trie_root()?,
            data_store: snapshot,
        })
    }
}

pub enum Entry<'a, V> {
    /// A Leaf
    Occupied(OccupiedEntry<'a, V>),
    /// The first Branch that proves the key is not in the trie.
    Vacant(VacantEntry<'a, V>),
    VacantEmptyTrie(VacantEntryEmptyTrie<'a, V>),
}

impl<'a, V> Entry<'a, V> {
    #[inline]
    pub fn get(&self) -> Option<&V> {
        match self {
            Entry::Occupied(OccupiedEntry { leaf }) => Some(&leaf.value),
            _ => None,
        }
    }

    #[inline]
    pub fn get_mut(&mut self) -> Option<&mut V> {
        match self {
            Entry::Occupied(OccupiedEntry { leaf }) => Some(&mut leaf.value),
            _ => None,
        }
    }

    #[inline]
    pub fn into_mut(self) -> Option<&'a mut V> {
        match self {
            Entry::Occupied(OccupiedEntry { leaf }) => Some(&mut leaf.value),
            _ => None,
        }
    }

    /// Prefer `Transaction::insert` over `Entry::insert` if you are not using any other `Entry` methods.
    #[inline]
    pub fn insert(self, value: V) -> &'a mut V {
        match self {
            Entry::Occupied(mut o) => {
                o.insert(value);
                o.into_mut()
            }
            Entry::VacantEmptyTrie(entry) => entry.insert(value),
            Entry::Vacant(entry) => entry.insert(value),
        }
    }

    #[inline]
    pub fn or_insert(self, value: V) -> &'a mut V {
        self.or_insert_with(|| value)
    }

    #[inline]
    pub fn or_insert_with<F>(self, default: F) -> &'a mut V
    where
        F: FnOnce() -> V,
    {
        self.or_insert_with_key(|_| default())
    }

    #[inline]
    pub fn or_insert_with_key<F>(self, default: F) -> &'a mut V
    where
        F: FnOnce(&KeyHash) -> V,
    {
        match self {
            Entry::Occupied(o) => &mut o.leaf.value,
            Entry::VacantEmptyTrie(entry) => {
                let value = default(entry.key());
                entry.insert(value)
            }
            Entry::Vacant(entry) => {
                let value = default(entry.key());
                entry.insert(value)
            }
        }
    }

    #[inline]
    pub fn key(&self) -> &KeyHash {
        match self {
            Entry::Occupied(OccupiedEntry { leaf }) => &leaf.key_hash,
            Entry::Vacant(VacantEntry { key_hash, .. })
            | Entry::VacantEmptyTrie(VacantEntryEmptyTrie { key_hash, .. }) => key_hash,
        }
    }
    #[inline]
    pub fn and_modify<F>(mut self, f: F) -> Self
    where
        F: FnOnce(&mut V),
    {
        match self {
            Entry::Occupied(OccupiedEntry { ref mut leaf }) => {
                f(&mut leaf.value);
                self
            }
            _ => self,
        }
    }

    #[inline]
    pub fn or_default(self) -> &'a mut V
    where
        V: Default,
    {
        #[allow(clippy::unwrap_or_default)]
        self.or_insert_with(Default::default)
    }
}

pub struct OccupiedEntry<'a, V> {
    /// This always points to a Leaf.
    /// It may be a ModLeaf or a stored Leaf.
    leaf: &'a mut Leaf<V>,
}

impl<'a, V> OccupiedEntry<'a, V> {
    #[inline]
    pub fn key(&self) -> &KeyHash {
        &self.leaf.key_hash
    }

    #[inline]
    pub fn get(&self) -> &V {
        &self.leaf.value
    }

    #[inline]
    pub fn get_mut(&mut self) -> &mut V {
        &mut self.leaf.value
    }

    #[inline]
    pub fn into_mut(self) -> &'a mut V {
        &mut self.leaf.value
    }

    #[inline]
    pub fn insert(&mut self, value: V) -> V {
        mem::replace(&mut self.leaf.value, value)
    }
}

pub struct VacantEntry<'a, V> {
    parent: &'a mut NodeRef<V>,
    key_hash: KeyHash,
    key_position: KeyPositionAdjacent,
}

impl<'a, V> VacantEntry<'a, V> {
    #[inline]
    pub fn key(&self) -> &KeyHash {
        &self.key_hash
    }

    #[inline]
    pub fn into_key(self) -> KeyHash {
        self.key_hash
    }

    #[inline]
    pub fn insert(self, value: V) -> &'a mut V {
        let VacantEntry {
            parent,
            key_hash,
            key_position,
        } = self;
        if let NodeRef::ModBranch(branch) = parent {
            let leaf =
                branch.new_adjacent_leaf_ret(key_position, Box::new(Leaf { key_hash, value }));
            return &mut leaf.value;
        };

        let owned_parent = mem::replace(parent, NodeRef::temp_null_stored());
        match owned_parent {
            NodeRef::ModLeaf(old_leaf) => {
                let (new_branch, new_leaf_is_right) =
                    Branch::new_from_leafs(0, old_leaf, Box::new(Leaf { key_hash, value }));

                *parent = NodeRef::ModBranch(new_branch);

                match parent {
                    NodeRef::ModBranch(branch) => {
                        let leaf = if new_leaf_is_right {
                            &mut branch.right
                        } else {
                            &mut branch.left
                        };

                        match leaf {
                            NodeRef::ModLeaf(ref mut leaf) => &mut leaf.value,
                            _ => {
                                unreachable!("new_from_leafs returns the location of the new leaf")
                            }
                        }
                    }
                    _ => unreachable!("new_from_leafs returns a ModBranch"),
                }
            }
            _ => {
                unreachable!("`entry` ensures VacantEntry should never point to a Stored node")
            }
        }
    }
}

pub struct VacantEntryEmptyTrie<'a, V> {
    root: &'a mut TrieRoot<NodeRef<V>>,
    key_hash: KeyHash,
}

impl<'a, V> VacantEntryEmptyTrie<'a, V> {
    #[inline]
    pub fn key(&self) -> &KeyHash {
        &self.key_hash
    }

    #[inline]
    pub fn into_key(self) -> KeyHash {
        self.key_hash
    }

    #[inline]
    pub fn insert(self, value: V) -> &'a mut V {
        let VacantEntryEmptyTrie { root, key_hash } = self;
        *root = TrieRoot::Node(NodeRef::ModLeaf(Box::new(Leaf { key_hash, value })));

        match root {
            TrieRoot::Node(NodeRef::ModLeaf(leaf)) => &mut leaf.value,
            _ => unreachable!("We just set root to a ModLeaf"),
        }
    }
}

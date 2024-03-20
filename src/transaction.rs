pub(crate) mod nodes;

use alloc::{boxed::Box, format, string::String};
use core::{mem, ops::Deref};

use crate::stored::{
    merkle::{Snapshot, SnapshotBuilder},
    DatabaseSet, Store,
};
use crate::{stored, KeyHash, NodeHash};

use self::nodes::{Branch, KeyPosition, Leaf, Node, NodeRef, StoredLeafRef, TrieRoot};

pub struct Transaction<S, V> {
    data_store: S,
    pub current_root: TrieRoot<NodeRef<V>>,
}

impl<'a, Db: DatabaseSet<V>, V: Clone + AsRef<[u8]>> Transaction<SnapshotBuilder<'a, Db, V>, V> {
    /// Write modified nodes to the database and return the root hash.
    /// Calling this method will write all modified nodes to the database.
    /// Calling this method again will rewrite the nodes to the database.
    ///
    /// Caching writes is the responsibility of the `DatabaseSet` implementation.
    #[inline]
    pub fn commit(&self) -> Result<TrieRoot<NodeHash>, String> {
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
                    .db
                    .set(*hash, Node::Branch(branch))
                    .map_err(|e| format!("Error writing branch {hash} to database: {e}"))
            };

        let store_modified_leaf = &mut |hash: &NodeHash, leaf: &Leaf<V>| {
            self.data_store
                .db
                .set(*hash, Node::Leaf(leaf.clone()))
                .map_err(|e| format!("Error writing leaf {hash} to database: {e}"))
        };

        let root_hash = self.calc_root_hash_inner(store_modified_branch, store_modified_leaf)?;
        Ok(root_hash)
    }
}

impl<S: Store<V>, V: AsRef<[u8]>> Transaction<S, V> {
    #[inline]
    pub fn calc_root_hash_inner(
        &self,
        on_modified_branch: &mut impl FnMut(
            &NodeHash,
            &Branch<NodeRef<V>>,
            NodeHash,
            NodeHash,
        ) -> Result<(), String>,
        on_modified_leaf: &mut impl FnMut(&NodeHash, &Leaf<V>) -> Result<(), String>,
    ) -> Result<TrieRoot<NodeHash>, String> {
        let root_hash = match &self.current_root {
            TrieRoot::Empty => return Ok(TrieRoot::Empty),
            TrieRoot::Node(node_ref) => Self::calc_root_hash_node(
                &self.data_store,
                node_ref,
                on_modified_leaf,
                on_modified_branch,
            )?,
        };

        Ok(TrieRoot::Node(root_hash))
    }

    #[inline]
    pub fn calc_root_hash(&self) -> Result<TrieRoot<NodeHash>, String> {
        self.calc_root_hash_inner(&mut |_, _, _, _| Ok(()), &mut |_, _| Ok(()))
    }

    #[inline]
    fn calc_root_hash_node(
        data_store: &S,
        node_ref: &NodeRef<V>,
        on_modified_leaf: &mut impl FnMut(&NodeHash, &Leaf<V>) -> Result<(), String>,
        on_modified_branch: &mut impl FnMut(
            &NodeHash,
            &Branch<NodeRef<V>>,
            NodeHash,
            NodeHash,
        ) -> Result<(), String>,
    ) -> Result<NodeHash, String> {
        // TODO use a stack instead of recursion
        match node_ref {
            NodeRef::ModBranch(branch) => {
                let left = Self::calc_root_hash_node(
                    data_store,
                    &branch.left,
                    on_modified_leaf,
                    on_modified_branch,
                )?;
                let right = Self::calc_root_hash_node(
                    data_store,
                    &branch.right,
                    on_modified_leaf,
                    on_modified_branch,
                )?;

                let hash = branch.hash_branch(&left, &right);
                on_modified_branch(&hash, branch, left, right)?;
                Ok(hash)
            }
            NodeRef::ModLeaf(leaf) => {
                let hash = leaf.hash_leaf();

                on_modified_leaf(&hash, leaf)?;
                Ok(hash)
            }
            NodeRef::Stored(stored_idx) => data_store.calc_subtree_hash(*stored_idx).map_err(|e| {
                format!(
                    "Error in `calc_root_hash_node`: {e} at {file}:{line}:{column}",
                    file = file!(),
                    line = line!(),
                    column = column!()
                )
            }),
        }
    }

    #[inline]
    pub fn get(&self, key_hash: &KeyHash) -> Result<Option<&V>, String> {
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
    ) -> Result<Option<&'root V>, String> {
        loop {
            match node_ref {
                // TODO check that the KeyPosition is optimized out.
                NodeRef::ModBranch(branch) => match branch.descend(key_hash) {
                    KeyPosition::Left => node_ref = &branch.left,
                    KeyPosition::Right => node_ref = &branch.right,
                    KeyPosition::PriorWord
                    | KeyPosition::PrefixWord
                    | KeyPosition::PrefixVec { .. } => return Ok(None),
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
    ) -> Result<Option<&'s V>, String> {
        loop {
            let node = data_store
                .get_node(stored_idx)
                .map_err(|e| format!("Error in `get_stored_node`: {e}"))?;
            match node {
                // TODO check that the KeyPosition is optimized out.
                Node::Branch(branch) => match branch.descend(key_hash) {
                    KeyPosition::Left => stored_idx = branch.left,
                    KeyPosition::Right => stored_idx = branch.right,
                    KeyPosition::PriorWord
                    | KeyPosition::PrefixWord
                    | KeyPosition::PrefixVec { .. } => return Ok(None),
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
    pub fn insert(&mut self, key_hash: &KeyHash, value: V) -> Result<(), String> {
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

    #[inline]
    fn insert_node(
        data_store: &mut S,
        root: &mut NodeRef<V>,
        key_hash: &KeyHash,
        value: V,
    ) -> Result<(), String> {
        match root {
            NodeRef::ModBranch(branch) => {
                Self::insert_below_branch(data_store, branch, key_hash, value)
            }
            NodeRef::ModLeaf(leaf) => {
                if leaf.key_hash == *key_hash {
                    leaf.value = value;
                    Ok(())
                } else {
                    let old_leaf = mem::replace(root, NodeRef::Stored(0));
                    let NodeRef::ModLeaf(old_leaf) = old_leaf else {
                        unreachable!("We just matched a ModLeaf");
                    };
                    *root = NodeRef::ModBranch(Branch::new_from_leafs(
                        0,
                        old_leaf,
                        Box::new(Leaf {
                            key_hash: *key_hash,
                            value,
                        }),
                    ));
                    Ok(())
                }
            }
            NodeRef::Stored(stored_idx) => {
                let new_node = data_store.get_node(*stored_idx).map_err(|e| {
                    format!("Error at `{}:{}:{}`: `{e}`", file!(), line!(), column!())
                })?;
                match new_node {
                    Node::Branch(new_branch) => {
                        *root = NodeRef::ModBranch(Box::new(Branch {
                            left: NodeRef::Stored(new_branch.left),
                            right: NodeRef::Stored(new_branch.right),
                            mask: new_branch.mask,
                            prior_word: new_branch.prior_word,
                            prefix: new_branch.prefix.clone(),
                        }));

                        let NodeRef::ModBranch(branch) = root else {
                            unreachable!("We just set root to a ModBranch");
                        };

                        Self::insert_below_branch(data_store, branch, key_hash, value)
                    }
                    Node::Leaf(leaf) => {
                        if leaf.key_hash == *key_hash {
                            *root = NodeRef::ModLeaf(Box::new(Leaf {
                                key_hash: *key_hash,
                                value,
                            }));
                            Ok(())
                        } else {
                            *root = NodeRef::ModBranch(Branch::new_from_leafs(
                                0,
                                StoredLeafRef::new(leaf, *stored_idx),
                                Box::new(Leaf {
                                    key_hash: *key_hash,
                                    value,
                                }),
                            ));
                            Ok(())
                        }
                    }
                }
            }
        }
    }

    #[inline]
    fn insert_below_branch(
        data_store: &mut S,
        mut branch: &mut Box<Branch<NodeRef<V>>>,
        key_hash: &KeyHash,
        value: V,
    ) -> Result<(), String> {
        loop {
            let next = match branch.descend(key_hash) {
                KeyPosition::Left => &mut branch.left,
                KeyPosition::Right => &mut branch.right,
                KeyPosition::PrefixWord => {
                    Branch::new_at_branch(
                        branch.mask.word_idx(),
                        branch.mask.left_prefix,
                        branch,
                        Box::new(Leaf {
                            key_hash: *key_hash,
                            value,
                        }),
                    );

                    return Ok(());
                }
                KeyPosition::PriorWord => {
                    Branch::new_at_branch(
                        branch.mask.word_idx() - 1,
                        branch.prior_word,
                        branch,
                        Box::new(Leaf {
                            key_hash: *key_hash,
                            value,
                        }),
                    );

                    return Ok(());
                }
                KeyPosition::PrefixVec {
                    word_idx,
                    branch_word,
                    key_word: _,
                } => {
                    Branch::new_at_branch(
                        word_idx,
                        branch_word,
                        branch,
                        Box::new(Leaf {
                            key_hash: *key_hash,
                            value,
                        }),
                    );

                    return Ok(());
                }
            };

            match next {
                NodeRef::ModBranch(next_branch) => {
                    branch = next_branch;
                }
                NodeRef::ModLeaf(leaf) if &leaf.key_hash == key_hash => {
                    leaf.value = value;
                    return Ok(());
                }
                NodeRef::ModLeaf(_leaf) => {
                    let old_next = mem::replace(next, NodeRef::Stored(0));
                    let NodeRef::ModLeaf(leaf) = old_next else {
                        unreachable!("We just matched a ModLeaf");
                    };

                    debug_assert_ne!(leaf.key_hash, *key_hash);
                    *next = NodeRef::ModBranch(Branch::new_from_leafs(
                        branch.mask.word_idx().saturating_sub(1),
                        leaf,
                        Box::new(Leaf {
                            key_hash: *key_hash,
                            value,
                        }),
                    ));

                    return Ok(());
                }
                NodeRef::Stored(stored_idx) => {
                    // TODO this is an artificial load of leaf.value.
                    let new_node = data_store
                        .get_node(*stored_idx)
                        .map_err(|e| format!("Error in `insert_below_branch`: {e}"))?;
                    match new_node {
                        Node::Branch(new_branch) => {
                            *next = NodeRef::ModBranch(Box::new(Branch::from_stored(new_branch)));

                            let NodeRef::ModBranch(next_branch) = next else {
                                unreachable!("We just set next to a ModBranch");
                            };

                            branch = next_branch;
                        }

                        Node::Leaf(leaf) if leaf.key_hash == *key_hash => {
                            *next = NodeRef::ModLeaf(Box::new(Leaf {
                                key_hash: *key_hash,
                                value,
                            }));
                            return Ok(());
                        }
                        Node::Leaf(leaf) => {
                            debug_assert_ne!(leaf.key_hash, *key_hash);
                            *next = NodeRef::ModBranch(Branch::new_from_leafs(
                                branch.mask.word_idx().saturating_sub(1),
                                StoredLeafRef::new(leaf, *stored_idx),
                                Box::new(Leaf {
                                    key_hash: *key_hash,
                                    value,
                                }),
                            ));
                            return Ok(());
                        }
                    }
                }
            }
        }
    }
}

impl<S: Store<V>, V: AsRef<[u8]> + Clone> Transaction<S, V> {
    /// This method allows for getting, inserting, and updating a entry in the trie with a single lookup.
    /// We match the standard library's `Entry` API for the most part.
    ///
    /// Note: Use of `entry` renders the trie path even if the entry is not modified.
    /// This will incures allocations, now and unessisary rehashing later when calculating the root hash.
    /// For this reason you should prefer `get` if you have a high probability of not modifying the entry.
    #[inline]
    pub fn entry_(&mut self, key_hash: &KeyHash) -> Result<Entry<'_, V>, String> {
        match self.current_root {
            TrieRoot::Empty => Ok(Entry::VacantEmptyTrie(VacantEntryEmptyTrie {
                root: &mut self.current_root,
                key_hash: *key_hash,
            })),
            TrieRoot::Node(ref mut node_ref) => {
                // Self::entry_node(&mut self.data_store, node_ref, key_hash)
                todo!()
            }
        }
    }

    #[inline]
    pub fn entry(&mut self, key_hash: &KeyHash) -> Result<Entry<'_, V>, String> {
        let node_ref = &mut self.current_root;

        match self.current_root {
            TrieRoot::Empty => Ok(Entry::VacantEmptyTrie(VacantEntryEmptyTrie {
                root: &mut self.current_root,
                key_hash: *key_hash,
            })),
            TrieRoot::Node(ref mut node_ref) => {
                todo!()
            }
        }
    }

    #[inline]
    fn entry_node<'root, 's: 'root>(
        data_store: &'s mut S,
        root_ref: &'root mut TrieRoot<NodeRef<V>>,
        key_hash: &KeyHash,
    ) -> Result<Entry<'root, V>, String> {
        let root = mem::take(root_ref);
        loop {
            match root {
                // TODO check the duplicated call to descend is optimized out.
                // If it's not figured out another way to satisfy the borrow checker.
                TrieRoot::Node(NodeRef::ModBranch(branch))
                    if matches!(branch.descend(key_hash), KeyPosition::Left) =>
                {
                    root = TrieRoot::Node(branch.left)
                }
                TrieRoot::Node(NodeRef::ModBranch(branch))
                    if matches!(branch.descend(key_hash), KeyPosition::Right) =>
                {
                    root = TrieRoot::Node(branch.right)
                }

                TrieRoot::Node(NodeRef::ModBranch(_)) => {
                    break;
                }

                TrieRoot::Node(NodeRef::ModLeaf(leaf)) if &leaf.key_hash == key_hash => {
                    break;
                }
                TrieRoot::Node(NodeRef::ModLeaf(_)) => {
                    break;
                }
                TrieRoot::Node(node_ref @ NodeRef::Stored(_)) => {
                    let loaded_node = data_store.get_node(1).map_err(|e| {
                        format!(
                            "Error in `entry_node`: {e} at {file}:{line}:{column}",
                            file = file!(),
                            line = line!(),
                            column = column!()
                        )
                    })?;

                    match loaded_node {
                        Node::Branch(loaded_branch) => {
                            // Connect the new branch to the trie.
                            *node_ref =
                                NodeRef::ModBranch(Box::new(Branch::from_stored(loaded_branch)));
                            break;
                        }
                        Node::Leaf(loaded_leaf) if loaded_leaf.key_hash == *key_hash => {
                            // Connect the new leaf to the trie.
                            *node_ref = NodeRef::ModLeaf(Box::new(loaded_leaf.clone()));

                            break;
                        }

                        Node::Leaf(_leaf) => {
                            debug_assert_ne!(_leaf.key_hash, *key_hash);

                            break;
                        }
                    }
                }
                TrieRoot::Empty => {
                    break;
                }
            };
        }

        *root_ref = root;

        todo!()
    }
}

impl<'a, Db, V> Transaction<SnapshotBuilder<'a, Db, V>, V> {
    /// An alias for `SnapshotBuilder::new_with_db`.
    ///
    /// Builds a snapshot of the trie before the transaction.
    /// The `Snapshot` is not a complete representation of the trie.
    /// The `Snapshot` only contains information about the parts of the trie touched by the transaction.
    /// Because of this, two `Snapshot`s of the same trie may not be equal if the transactions differ.
    ///
    /// Note: All operations including get affect the contents of the snapshot.
    #[inline]
    pub fn build_initial_snapshot(&self) -> Snapshot<V>
    where
        V: Clone,
    {
        self.data_store.build_initial_snapshot()
    }

    #[inline]
    pub fn from_snapshot_builder(builder: SnapshotBuilder<'a, Db, V>) -> Self {
        Transaction {
            current_root: builder.trie_root(),
            data_store: builder,
        }
    }
}

impl<'s, V: AsRef<[u8]>> Transaction<&'s Snapshot<V>, V> {
    #[inline]
    pub fn from_snapshot(snapshot: &'s Snapshot<V>) -> Result<Self, String> {
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

pub struct OccupiedEntry<'a, V> {
    leaf: &'a mut Leaf<V>,
}

pub struct VacantEntry<'a, V> {
    parent: &'a mut NodeRef<V>,
    key_hash: KeyHash,
}

pub struct VacantEntryEmptyTrie<'a, V> {
    root: &'a mut TrieRoot<NodeRef<V>>,
    key_hash: KeyHash,
}

impl<'a, V> Entry<'a, V> {
    #[inline]
    pub fn or_insert(self, value: V) -> &'a mut V {
        match self {
            Entry::Occupied(occupied) => &mut occupied.leaf.value,
            Entry::VacantEmptyTrie(VacantEntryEmptyTrie { root, key_hash }) => {
                *root = TrieRoot::Node(NodeRef::ModLeaf(Box::new(Leaf { key_hash, value })));

                match root {
                    TrieRoot::Node(NodeRef::ModLeaf(leaf)) => &mut leaf.value,
                    _ => unreachable!("We just set root to a ModLeaf"),
                }
            }
            Entry::Vacant(VacantEntry { parent, key_hash }) => todo!(),
        }
    }

    #[inline]
    pub fn or_insert_with<F>(self, value: F) -> &'a mut V
    where
        F: FnOnce() -> V,
    {
        todo!()
    }

    #[inline]
    pub fn and_modify<F>(self, f: F) -> Self
    where
        F: FnOnce(&mut V),
    {
        todo!()
    }

    #[inline]
    pub fn key(&self) -> &KeyHash {
        match self {
            Entry::Occupied(OccupiedEntry {
                leaf: Leaf { key_hash, .. },
            })
            | Entry::Vacant(VacantEntry { key_hash, .. })
            | Entry::VacantEmptyTrie(VacantEntryEmptyTrie { key_hash, .. }) => key_hash,
        }
    }
}

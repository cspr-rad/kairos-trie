pub(crate) mod nodes;

use alloc::{boxed::Box, format, string::String};
use core::mem;

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
    /// TODO a version of this that writes to the database.
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

    pub fn calc_root_hash(&self) -> Result<TrieRoot<NodeHash>, String> {
        self.calc_root_hash_inner(&mut |_, _, _, _| Ok(()), &mut |_, _| Ok(()))
    }

    /// TODO use this to store nodes in the data base
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

    pub fn get(&self, key_hash: &KeyHash) -> Result<Option<&V>, String> {
        match &self.current_root {
            TrieRoot::Empty => Ok(None),
            TrieRoot::Node(node_ref) => Self::get_node(&self.data_store, node_ref, key_hash),
        }
    }

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
                NodeRef::ModLeaf(_) => {
                    let old_next = mem::replace(next, NodeRef::Stored(0));
                    let NodeRef::ModLeaf(leaf) = old_next else {
                        unreachable!("We just matched a ModLeaf");
                    };

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
                            *next = NodeRef::ModBranch(Box::new(Branch {
                                left: NodeRef::Stored(new_branch.left),
                                right: NodeRef::Stored(new_branch.right),
                                mask: new_branch.mask,
                                // TODO remove the clone
                                // Maybe use a AsRef<[u32]> instead of Vec<u32>
                                prior_word: new_branch.prior_word,
                                prefix: new_branch.prefix.clone(),
                            }));

                            let NodeRef::ModBranch(next_branch) = next else {
                                unreachable!("We just set next to a ModBranch");
                            };

                            branch = next_branch;
                        }
                        Node::Leaf(leaf) => {
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

impl<'a, Db, V> Transaction<SnapshotBuilder<'a, Db, V>, V> {
    /// An alias for `SnapshotBuilder::new_with_db`.
    ///
    /// Builds a snapshot of the trie before the transaction.
    /// The `Snapshot` is not a complete representation of the trie.
    /// The `Snapshot` only contains information about the parts of the trie touched by the transaction.
    /// Because of this, two `Snapshot`s of the same trie may not be equal if the transactions differ.
    ///
    /// Note: All operations including get affect the contents of the snapshot.
    pub fn build_initial_snapshot(&self) -> Snapshot<V>
    where
        V: Clone,
    {
        self.data_store.build_initial_snapshot()
    }

    pub fn from_snapshot_builder(builder: SnapshotBuilder<'a, Db, V>) -> Self {
        Transaction {
            current_root: builder.trie_root(),
            data_store: builder,
        }
    }
}

impl<'s, V: AsRef<[u8]>> Transaction<&'s Snapshot<V>, V> {
    pub fn from_snapshot(snapshot: &'s Snapshot<V>) -> Result<Self, String> {
        Ok(Transaction {
            current_root: snapshot.trie_root()?,
            data_store: snapshot,
        })
    }
}

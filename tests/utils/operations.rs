#![allow(unused)]

use std::{
    collections::{hash_map, HashMap},
    rc::Rc,
};

use proptest::{prelude::*, sample::SizeRange};

use kairos_trie::{
    stored::{
        memory_db::MemoryDb,
        merkle::{Snapshot, SnapshotBuilder},
        Store,
    },
    DigestHasher, KeyHash, NodeHash, Transaction, TrieRoot,
};
use sha2::Sha256;

use super::arb_key_hash;

pub type Value = [u8; 8];

#[derive(Debug, Clone, Copy)]
pub enum Operation {
    Get(KeyHash),
    Insert(KeyHash, Value),
    EntryGet(KeyHash),
    EntryInsert(KeyHash, Value),
    EntryAndModifyOrInsert(KeyHash, Value),
    EntryOrInsert(KeyHash, Value),
}

prop_compose! {
    pub fn arb_value()(data in any::<[u8; 8]>()) -> Value {
        data
    }
}

prop_compose! {
    pub fn arb_operations(key_count: impl Into<SizeRange>, op_count: impl Into<SizeRange>)
                         (keys in prop::collection::vec(arb_key_hash(), key_count),
                          ops in prop::collection::vec(
                              (0..5u8,
                               any::<prop::sample::Index>(),
                               arb_value()
                              ),
                              op_count
                            )
                         ) -> Vec<Operation> {
    ops.into_iter().map(|(op, idx, value)| {
        let key = keys[idx.index(keys.len())];
        match op {
            0 => Operation::Get(key),
            1 => Operation::Insert(key, value),
            2 => Operation::EntryGet(key),
            3 => Operation::EntryInsert(key, value),
            4 => Operation::EntryAndModifyOrInsert(key, value),
            5 => Operation::EntryOrInsert(key, value),
            _ => unreachable!(),
        }}).collect()
    }
}

prop_compose! {
    pub fn arb_batches(key_count: impl Into<SizeRange>, op_count: impl Into<SizeRange>, max_batch_count: usize, max_batch_size: usize)
                      (
                          ops in arb_operations(key_count, op_count),
                          windows in prop::collection::vec(0..max_batch_size, max_batch_count - 1)
                      ) -> Vec<Vec<Operation>> {
                          arb_batches_inner(ops, windows)
    }
}

fn arb_batches_inner(ops: Vec<Operation>, windows: Vec<usize>) -> Vec<Vec<Operation>> {
    let mut batches = Vec::new();
    let mut start = 0;

    // Partition the operations into batches
    for window_size in windows {
        if start + window_size > ops.len() {
            break;
        }

        batches.push(ops[start..start + window_size].to_vec());

        start += window_size;
    }

    if start < ops.len() {
        batches.push(ops[start..].to_vec());
    }

    batches
}

// Code like this runs in the server.
pub fn run_against_snapshot_builder(
    batch: &[Operation],
    old_root_hash: TrieRoot<NodeHash>,
    db: Rc<MemoryDb<Value>>,
    hash_map: &mut HashMap<KeyHash, Value>,
) -> (TrieRoot<NodeHash>, Snapshot<Value>) {
    let bump = bumpalo::Bump::new();
    let builder = SnapshotBuilder::empty(db).with_trie_root_hash(old_root_hash);
    let mut txn = Transaction::from_snapshot_builder(builder);

    for op in batch {
        let (old, new) = trie_op(op, &mut txn);
        let (old_hm, new_hm) = hashmap_op(op, hash_map);
        assert_eq!(old, old_hm);
        assert_eq!(new, new_hm);
    }

    let new_root_hash = txn.commit(&mut DigestHasher::<Sha256>::default()).unwrap();
    let snapshot = txn.build_initial_snapshot();
    (new_root_hash, snapshot)
}

/// Code like this would run in a zkVM
pub fn run_against_snapshot(
    batch: &[Operation],
    snapshot: Snapshot<[u8; 8]>,
    new_root_hash: TrieRoot<NodeHash>,
    old_root_hash: TrieRoot<NodeHash>,
) {
    // Does the contract's expected old root hash match the submitted snapshot?
    assert_eq!(
        old_root_hash,
        snapshot
            .calc_root_hash(&mut DigestHasher::<Sha256>::default())
            .unwrap()
    );

    // Create a transaction against the snapshot at the old root hash
    let mut txn = Transaction::from_snapshot(&snapshot).unwrap();

    // Apply the operations to the transaction
    for op in batch {
        trie_op(op, &mut txn);
    }

    // Calculate the new root hash
    let root_hash = txn
        .calc_root_hash(&mut DigestHasher::<Sha256>::default())
        .unwrap();

    // Check that the new root hash matches the submitted new root hash
    // This last bit is actually unnecessary, but it's a good sanity check
    assert_eq!(root_hash, new_root_hash);
}

fn trie_op<S: Store<Value>>(
    op: &Operation,
    txn: &mut Transaction<S, Value>,
) -> (Option<Value>, Option<Value>) {
    match op {
        Operation::Insert(key, value) => {
            txn.insert(key, *value).unwrap();
            (None, Some(*value))
        }
        Operation::EntryInsert(key, value) => match txn.entry(key).unwrap() {
            kairos_trie::Entry::Occupied(mut o) => {
                let old = *o.get();
                o.insert(*value);
                (Some(old), Some(*value))
            }
            kairos_trie::Entry::Vacant(v) => {
                let new = v.insert(*value);
                (None, Some(*new))
            }
            kairos_trie::Entry::VacantEmptyTrie(v) => {
                let new = v.insert(*value);
                (None, Some(*new))
            }
        },
        Operation::EntryAndModifyOrInsert(key, value) => {
            let entry = txn.entry(key).unwrap();
            let mut old = None;
            let new = entry
                .and_modify(|v| {
                    old = Some(*v);
                    *v = *value;
                })
                .or_insert(*value);

            assert_eq!(new, value);

            (old, Some(*new))
        }
        Operation::EntryOrInsert(key, value) => {
            let mut old = None;
            let new = txn
                .entry(key)
                .unwrap()
                .and_modify(|v| old = Some(*v))
                .or_insert(*value);

            (old, Some(*new))
        }
        Operation::Get(key) => {
            let old = txn.get(key).unwrap().copied();
            (old, old)
        }
        Operation::EntryGet(key) => {
            let old = txn.entry(key).unwrap().get().copied();
            (old, old)
        }
    }
}

fn hashmap_op(op: &Operation, map: &mut HashMap<KeyHash, Value>) -> (Option<Value>, Option<Value>) {
    match op {
        Operation::Insert(key, value) => {
            map.insert(*key, *value);
            (None, Some(*value))
        }
        Operation::EntryInsert(key, value) => match map.entry(*key) {
            hash_map::Entry::Occupied(mut o) => {
                let old = *o.get();
                o.insert(*value);
                (Some(old), Some(*value))
            }
            hash_map::Entry::Vacant(v) => {
                let new = v.insert(*value);
                (None, Some(*new))
            }
        },
        Operation::EntryAndModifyOrInsert(key, value) => {
            let entry = map.entry(*key);
            let mut old = None;
            let new = entry
                .and_modify(|v| {
                    old = Some(*v);
                    *v = *value;
                })
                .or_insert(*value);

            assert_eq!(new, value);
            (old, Some(*new))
        }
        Operation::EntryOrInsert(key, value) => {
            let entry = map.entry(*key);
            let mut old = None;
            let new = entry.and_modify(|v| old = Some(*v)).or_insert(*value);

            (old, Some(*new))
        }
        Operation::Get(key) => {
            let old = map.get(key).copied();
            (old, old)
        }
        Operation::EntryGet(key) => {
            let old = map.get(key).copied();
            (old, old)
        }
    }
}

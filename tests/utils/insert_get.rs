use std::collections::HashMap;

use kairos_trie::{
    stored::{
        merkle::{Snapshot, SnapshotBuilder},
        DatabaseSet,
    },
    KeyHash, NodeHash, Transaction, TrieRoot,
};

pub fn run_against_snapshot_builder<Db: DatabaseSet<[u8; 8]>>(
    new: &HashMap<KeyHash, u64>,
    old_root_hash: TrieRoot<NodeHash>,
    db: Db,
) -> (TrieRoot<NodeHash>, Snapshot<[u8; 8]>) {
    let bump = bumpalo::Bump::new();
    let builder = SnapshotBuilder::empty(db, &bump).with_trie_root_hash(old_root_hash);

    let mut txn = Transaction::from_snapshot_builder(builder);

    for (key, value) in new.iter() {
        txn.insert(key, value.to_le_bytes()).unwrap();
    }

    let new_root_hash = txn.commit().unwrap();
    let snapshot = txn.build_initial_snapshot();

    (new_root_hash, snapshot)
}

/// Code like this would run in a zkVM
pub fn run_against_snapshot(
    new: &HashMap<KeyHash, u64>,
    snapshot: Snapshot<[u8; 8]>,
    new_root_hash: TrieRoot<NodeHash>,
    old_root_hash: TrieRoot<NodeHash>,
) {
    assert_eq!(old_root_hash, snapshot.calc_root_hash().unwrap());

    let mut txn = Transaction::from_snapshot(&snapshot).unwrap();

    for (key, value) in new.iter() {
        txn.insert(key, value.to_le_bytes()).unwrap();
    }

    for (key, value) in new.iter() {
        let ret_val = txn.get(key).unwrap().unwrap();
        assert_eq!(ret_val, &value.to_le_bytes());
    }

    let root_hash = txn.calc_root_hash().unwrap();
    assert_eq!(root_hash, new_root_hash);
}

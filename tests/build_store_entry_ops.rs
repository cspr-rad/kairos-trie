mod utils;
use std::collections::HashMap;

use kairos_trie::{
    stored::{memory_db::MemoryDb, merkle::SnapshotBuilder},
    Transaction, TrieRoot,
};
use utils::operations::*;

pub fn end_to_end_entry_ops(batches: &[&[Operation]]) {
    let db = &MemoryDb::<[u8; 8]>::empty();

    let mut prior_root_hash = TrieRoot::default();
    // used as a reference for trie behavior
    let mut hash_map = HashMap::new();

    for batch in batches.iter() {
        let (new_root_hash, snapshot) =
            run_against_snapshot_builder(batch, prior_root_hash, db, &mut hash_map);

        run_against_snapshot(batch, snapshot, new_root_hash, prior_root_hash);
        prior_root_hash = new_root_hash;
    }

    // After all batches are applied, the trie and the hashmap should be in sync
    let bump = bumpalo::Bump::new();
    let txn = Transaction::from_snapshot_builder(
        SnapshotBuilder::<_, [u8; 8]>::empty(db, &bump).with_trie_root_hash(prior_root_hash),
    );

    // Check that the trie and the hashmap are in sync
    for (k, v) in hash_map.iter() {
        let ret_v = txn.get(k).unwrap().unwrap();
        assert_eq!(v, ret_v);
    }
}

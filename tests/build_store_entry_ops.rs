mod utils;
use std::collections::HashMap;

use proptest::prelude::*;

use kairos_trie::{
    stored::{memory_db::MemoryDb, merkle::SnapshotBuilder},
    Transaction, TrieRoot,
};
use utils::operations::*;

fn end_to_end_entry_ops(batches: Vec<Vec<Operation>>) {
    // The persistent backing, likely rocksdb
    let db = &MemoryDb::<[u8; 8]>::empty();

    // An empty trie root
    let mut prior_root_hash = TrieRoot::default();

    // used as a reference for trie behavior
    let mut hash_map = HashMap::new();

    for batch in batches.iter() {
        eprintln!("Batch size: {}", batch.len());
        // We build a snapshot on the server.
        let (new_root_hash, snapshot) =
            run_against_snapshot_builder(batch, prior_root_hash, db, &mut hash_map);

        // We verify the snapshot in a zkVM
        run_against_snapshot(batch, snapshot, new_root_hash, prior_root_hash);

        // After a batch is verified in an on chain zkVM the contract would update's its root hash
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

proptest! {
    #[test]
    fn prop_end_to_end_entry_ops(
        batches in arb_batches(1..5000usize, 1..100_000usize, 1000, 10_000)) {
        end_to_end_entry_ops(batches);
    }
}

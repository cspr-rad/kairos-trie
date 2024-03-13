mod utils;

use proptest::prelude::*;
use std::collections::HashMap;

use sha2::{Digest, Sha256};

use kairos_trie::{
    stored::{merkle::SnapshotBuilder, MemoryDb, NodeHash},
    KeyHash, Transaction, TrieRoot,
};

prop_compose! {
    fn arb_key_hash()(data in any::<[u8; 32]>()) -> KeyHash {
        KeyHash::from(&data)
    }
}

prop_compose! {
    fn arb_hashmap()(
        map in prop::collection::hash_map(arb_key_hash(), 0u64.., 0..2)
    ) -> HashMap<KeyHash, u64> {
        map
    }
}

proptest! {
    #[test]
    fn prop_end_to_end_example(
        maps in prop::collection::vec(arb_hashmap(), 1..3)
    ) {
        end_to_end_example(maps)
    }
}

#[test]
fn end_to_end_example_1() {
    end_to_end_example(vec![HashMap::new()]);
}

#[test]
fn end_to_end_example_2() {
    end_to_end_example(vec![HashMap::new(), HashMap::new()]);
}

#[test]
fn end_to_end_example_dup_maps_0() {
    let map = HashMap::from_iter([(KeyHash([0; 8]), 0)]);
    end_to_end_example(vec![map.clone(), map]);
}

fn end_to_end_example(maps: Vec<HashMap<KeyHash, u64>>) {
    let db = &MemoryDb::<[u8; 8]>::empty();

    let mut prior_root_hash = TrieRoot::default();

    for map in maps.iter() {
        let (new_root_hash, snapshot) =
            utils::run_against_snapshot_builder(map, prior_root_hash, db);
        utils::run_against_snapshot(map, snapshot, new_root_hash, prior_root_hash);
        prior_root_hash = new_root_hash;
    }

    let merged_map: HashMap<KeyHash, u64> = maps.into_iter().flat_map(|m| m.into_iter()).collect();

    let bump = bumpalo::Bump::new();
    let txn = Transaction::from_snapshot_builder(
        SnapshotBuilder::<_, [u8; 8]>::empty(db, &bump).with_trie_root_hash(prior_root_hash),
    );

    //     for (k, v) in merged_map.iter() {
    //         let v = v.to_be_bytes();
    //         let ret_v = txn.get(k).unwrap().unwrap();
    //         assert_eq!(v, *ret_v);
    //     }
}

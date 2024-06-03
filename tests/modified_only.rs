use proptest::prelude::*;
use std::collections::HashMap;

use sha2::{Digest, Sha256};

use kairos_trie::{
    stored::{memory_db::MemoryDb, merkle::SnapshotBuilder},
    KeyHash, Transaction,
};

fn sha256_hash(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

#[test]
fn insert_get_u64_round_trip() {
    let hashmap: HashMap<KeyHash, u64> = (0u64..10_000)
        .map(|i| (KeyHash::from(&sha256_hash(&i.to_le_bytes())), i))
        .collect();

    let builder = SnapshotBuilder::empty(MemoryDb::<u64>::empty());

    let mut txn = Transaction::from_snapshot_builder(builder);

    for (key, value) in hashmap.iter() {
        txn.insert(key, *value).unwrap();
        let ret_val = txn.get(key).unwrap().unwrap();
        assert_eq!(ret_val, value);
    }

    for (key, value) in hashmap.iter() {
        let ret_val = txn.get(key).unwrap().unwrap();
        assert_eq!(ret_val, value);
    }
}

prop_compose! {
    fn arb_key_hash()(data in any::<[u8; 32]>()) -> KeyHash {
        KeyHash::from(&data)
    }
}

proptest! {
    #[test]
    fn prop_insert_get_rand(
        keys in prop::collection::hash_map(arb_key_hash(), 0u64.., 0..10_000)
    ) {
        let builder = SnapshotBuilder::empty(MemoryDb::<[u8; 8]>::empty());

        let mut txn = Transaction::from_snapshot_builder(builder);

        for (key, value) in keys.iter() {
            txn.insert(key, value.to_le_bytes()).unwrap();
            let ret_val = txn.get(key).unwrap().unwrap();
            assert_eq!(ret_val, &value.to_le_bytes());
        }

        for (key, value) in keys.iter() {
            let ret_val = txn.get(key).unwrap().unwrap();
            assert_eq!(ret_val, &value.to_le_bytes());
        }
    }
}

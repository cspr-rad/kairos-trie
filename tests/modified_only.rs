use proptest::prelude::*;
use std::collections::HashMap;

use sha2::{Digest, Sha256};

use kairos_trie::{
    stored::{merkle::SnapshotBuilder, MemoryDb},
    KeyHash, Transaction, TrieRoot,
};

fn sha256_hash(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

#[test]
fn insert_get_u64_round_trip() {
    let hashmap: HashMap<KeyHash, Vec<u8>> = (0u64..10000)
        .map(|i| {
            (
                KeyHash::from(&sha256_hash(&i.to_le_bytes())),
                i.to_le_bytes().to_vec(),
            )
        })
        .collect();

    let bump = bumpalo::Bump::new();
    let builder = SnapshotBuilder::empty(MemoryDb::<Vec<u8>>::empty(), &bump);

    let mut txn = Transaction::from_snapshot_builder(builder);

    for (key, value) in hashmap.iter() {
        txn.insert(key, value.clone()).unwrap();
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
        keys in prop::collection::hash_map(arb_key_hash(), 0u64.., 0..100_000)
    ) {
        let bump = bumpalo::Bump::new();
        let builder = SnapshotBuilder::empty(MemoryDb::<[u8; 8]>::empty(), &bump);

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

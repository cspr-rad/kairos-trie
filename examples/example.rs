use std::rc::Rc;

use kairos_trie::{
    stored::{memory_db::MemoryDb, merkle::SnapshotBuilder},
    DigestHasher, KeyHash, PortableHash, PortableHasher, Transaction, TrieRoot,
};
use sha2::Sha256;

fn main() {
    // Bring your own key-value database, rocksdb, sled, etc.
    let db = Rc::new(MemoryDb::<u64>::empty());

    // On server
    let _snapshot = {
        let mut txn =
            Transaction::from_snapshot_builder(SnapshotBuilder::new(db.clone(), TrieRoot::Empty));

        let hasher = &mut DigestHasher::<Sha256>::default();

        let key_values = vec![("foo", 1), ("bar", 2), ("baz", 3)];

        for (key, value) in key_values {
            key.portable_hash(hasher);
            let key_hash = KeyHash::from_bytes(&hasher.finalize_reset());

            txn.insert(&key_hash, value).unwrap();
        }

        let _root = txn.commit(hasher).unwrap();
        txn.build_initial_snapshot()
    };
}

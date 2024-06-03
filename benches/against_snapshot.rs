use std::rc::Rc;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use kairos_trie::{
    stored::{
        memory_db::MemoryDb,
        merkle::{Snapshot, SnapshotBuilder},
    },
    DigestHasher, KeyHash, PortableHash, PortableHasher, Transaction, TrieRoot,
};
use sha2::Sha256;

fn txn_over_fraction_of_trie(size: u64, denominator: u64) -> Snapshot<u64> {
    let db = Rc::new(MemoryDb::<u64>::empty());

    let mut txn =
        Transaction::from_snapshot_builder(SnapshotBuilder::new(db.clone(), TrieRoot::Empty));

    let hasher = &mut DigestHasher::<Sha256>::default();

    for k in 0..size {
        k.portable_hash(hasher);
        let key_hash = KeyHash::from_bytes(&hasher.finalize_reset());

        txn.insert(&key_hash, k).unwrap();
    }

    let root = txn.commit(hasher).unwrap();

    let mut txn = Transaction::from_snapshot_builder(SnapshotBuilder::new(db, root));

    for k in 0..size / denominator {
        k.portable_hash(hasher);
        let key_hash = KeyHash::from_bytes(&hasher.finalize_reset());

        *txn.entry(&key_hash).unwrap().get_mut().unwrap() += size;
    }

    txn.build_initial_snapshot()
}

fn against_snapshot(c: &mut Criterion) {
    // A snapshot for a txn that increments 10% of the values in the trie
    let snapshot = txn_over_fraction_of_trie(100_000, 10);

    let hasher = &mut DigestHasher::<Sha256>::default();

    let root = snapshot
        .calc_root_hash(&mut DigestHasher::<Sha256>::default())
        .unwrap();

    c.bench_function("verify snapshot", |b| {
        b.iter(|| {
            assert_eq!(
                root,
                black_box(
                    black_box(&snapshot)
                        .calc_root_hash(black_box(hasher))
                        .unwrap()
                )
            )
        })
    });
}

criterion_group!(benches, against_snapshot);
criterion_main!(benches);

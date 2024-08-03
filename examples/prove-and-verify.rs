use std::rc::Rc;

use kairos_trie::{
    stored::{
        memory_db::MemoryDb,
        merkle::{Snapshot, SnapshotBuilder},
        Store,
    },
    DigestHasher, KeyHash, NodeHash, PortableHash, PortableHasher, Transaction, TrieRoot,
};
use sha2::Sha256;

enum Ops {
    Add(String, u64),
    Sub(String, u64),
}

fn hash(key: &str) -> KeyHash {
    let hasher = &mut DigestHasher::<Sha256>::default();
    key.portable_hash(hasher);
    KeyHash::from_bytes(&hasher.finalize_reset())
}

fn apply_operations(txn: &mut Transaction<impl Store<u64>, u64>, operations: &[Ops]) {
    for op in operations {
        match op {
            Ops::Add(key, value) => {
                let old_amount = txn.entry(&hash(key)).unwrap().or_default();
                *old_amount += value;
            }
            Ops::Sub(key, value) => {
                let old_amount = txn.entry(&hash(key)).unwrap().or_default();
                *old_amount -= value;
            }
        }
    }
}

fn prover(
    // Provide any key-value database, rocksdb, sled, etc..
    db: Rc<MemoryDb<u64>>,
    // merkle root before the transaction
    pre_txn_merkle_root: TrieRoot<NodeHash>,
    // the operations to apply in a transaction
    operations: &[Ops],
) -> (Snapshot<u64>, TrieRoot<NodeHash>) {
    let mut txn =
        Transaction::from_snapshot_builder(SnapshotBuilder::new(db.clone(), pre_txn_merkle_root));

    apply_operations(&mut txn, operations);

    let hasher = &mut DigestHasher::<Sha256>::default();

    // Commit the new merkle root to the database.
    // The old merkle trie can still be accessed through the old merkle root.
    let merkle_root = txn.commit(hasher).unwrap();

    // Build a Snapshot containing the minimal portion of the old merkle tree needed to replay the transaction.
    let snapshot = txn.build_initial_snapshot();

    (snapshot, merkle_root)
}

/// In zkVM or other verifiable environment
fn verifier(
    // State stored by the verifier
    pre_txn_merkle_root: TrieRoot<NodeHash>,

    // Data provided by the prover
    snapshot: &Snapshot<u64>,
    operations: &[Ops],
) -> TrieRoot<NodeHash> {
    let hasher = &mut DigestHasher::<Sha256>::default();

    let mut txn = Transaction::from_snapshot(snapshot).unwrap();

    let pre_batch_trie_root = txn.calc_root_hash(hasher).unwrap();
    // Assert that the trie started the transaction with the correct root hash.
    assert_eq!(pre_batch_trie_root, pre_txn_merkle_root);

    // Replay the exact same operations inside the zkVM.
    // The business logic is entirely identical.
    apply_operations(&mut txn, operations);

    txn.calc_root_hash(hasher).unwrap()
}

fn main() {
    // Bring your own key-value database, rocksdb, sled, etc.
    let server_db = Rc::new(MemoryDb::empty());

    let operations_1 = vec![
        Ops::Add("Alice".to_string(), 100),
        Ops::Add("Bob".to_string(), 200),
        Ops::Sub("Alice".to_string(), 50),
    ];

    // Prove a set of operations on the server.
    let (snapshot_0, _merkle_root_1) = prover(server_db.clone(), TrieRoot::Empty, &operations_1);

    // Rerun the computation in a verifiable environment (zkVM, L1, etc) against the minimal snapshot.
    let merkle_root_1 = verifier(TrieRoot::Empty, &snapshot_0, &operations_1);

    let operations_2 = vec![
        Ops::Add("Alice".to_string(), 50),
        Ops::Sub("Bob".to_string(), 100),
    ];

    // Prove the second batch of operations on the server.
    let (snapshot_1, _merkle_root_2) = prover(server_db.clone(), merkle_root_1, &operations_2);

    // Rerun batch 2 in zkVM.
    let _merkle_root_2 = verifier(merkle_root_1, &snapshot_1, &operations_2);
}

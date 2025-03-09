mod utils;

use std::rc::Rc;


use kairos_trie::{
    stored::{memory_db::MemoryDb, merkle::SnapshotBuilder},
    DigestHasher, KeyHash, Transaction, TrieRoot,
};
use sha2::Sha256;
use kairos_trie::Leaf;
use kairos_trie::stored::DatabaseGet;

#[test]
fn test_remove_empty_trie() {
    let db = Rc::new(MemoryDb::<[u8; 8]>::empty());
    let builder = SnapshotBuilder::empty(db).with_trie_root_hash(TrieRoot::default());
    let mut txn = Transaction::from_snapshot_builder(builder);
    
    // Try to remove a key from an empty trie
    let key_hash = KeyHash([0; 8]);
    let result = txn.remove(&key_hash);
    
    // Should succeed without error
    assert!(result.is_ok());
    
    // Commit the transaction
    let root_hash = txn.commit(&mut DigestHasher::<Sha256>::default()).unwrap();
    
    // Root should still be empty
    assert_eq!(root_hash, TrieRoot::default());
}

#[test]
fn test_remove_single_key() {
    let db = Rc::new(MemoryDb::<[u8; 8]>::empty());
    let builder = SnapshotBuilder::empty(db.clone()).with_trie_root_hash(TrieRoot::default());
    let mut txn = Transaction::from_snapshot_builder(builder);
    
    // Insert a single key
    let key_hash = KeyHash([1; 8]);
    let value = 42u64.to_le_bytes();
    txn.insert(&key_hash, value).unwrap();
    
    // Commit the transaction to write the node to the db
    let mut hasher = DigestHasher::<Sha256>::default();
    let root_hash = txn.commit(&mut hasher).unwrap();
    
    // Compute the expected node hash for the inserted leaf
    let mut hasher = DigestHasher::<Sha256>::default();
    let expected_hash = Leaf { key_hash, value }.hash_leaf(&mut hasher);
    
    // Check that the node is available in the db directly
    let db_node = db.clone().get(&expected_hash);  
    assert!(db_node.is_ok(), "Node should be available in DB after initial commit");
    
    // Create a new transaction with the updated root, then remove the key
    let builder = SnapshotBuilder::empty(db.clone()).with_trie_root_hash(root_hash);
    let mut txn = Transaction::from_snapshot_builder(builder);
    assert_eq!(txn.get(&key_hash).unwrap(), Some(&value));
    txn.remove(&key_hash).unwrap();
    
    // Before committing the removal, the node should still be in the db
    let db_node = db.clone().get(&expected_hash);
    assert!(db_node.is_ok(), "Node should still be available in DB after removal before commit");
    
    // Now commit the removal
    let mut hasher = DigestHasher::<Sha256>::default();
    let root_hash = txn.commit(&mut hasher).unwrap();
    
    // After commit, the removed node should be gone from the db (get errors)
    let db_node = db.clone().get(&expected_hash);
    assert!(db_node.is_err(), "Node should be removed from DB after commit");
    
    // Finally, verify that txn.get returns None
    let builder = SnapshotBuilder::empty(db.clone()).with_trie_root_hash(root_hash);
    let txn = Transaction::from_snapshot_builder(builder);
    let result = txn.get(&key_hash);
    assert_eq!(result, Ok(None));
}

#[test]
fn test_remove_multiple_keys() {
    let db = Rc::new(MemoryDb::<[u8; 8]>::empty());
    let builder = SnapshotBuilder::empty(db.clone()).with_trie_root_hash(TrieRoot::default());
    let mut txn = Transaction::from_snapshot_builder(builder);
    
    // Insert multiple keys
    let keys = [
        KeyHash([1, 0, 0, 0, 0, 0, 0, 0]),
        KeyHash([2, 0, 0, 0, 0, 0, 0, 0]),
        KeyHash([3, 0, 0, 0, 0, 0, 0, 0]),
        KeyHash([4, 0, 0, 0, 0, 0, 0, 0]),
        KeyHash([5, 0, 0, 0, 0, 0, 0, 0]),
    ];
    
    for (i, key) in keys.iter().enumerate() {
        let value = (i as u64).to_le_bytes();
        txn.insert(key, value).unwrap();
    }
    
    // Commit the transaction
    let mut hasher = DigestHasher::<Sha256>::default();
    let root_hash = txn.commit(&mut hasher).unwrap();
    
    // Create a new transaction with the updated root
    let builder = SnapshotBuilder::empty(db.clone()).with_trie_root_hash(root_hash);
    let mut txn = Transaction::from_snapshot_builder(builder);
    
    // Verify all keys exist
    for (i, key) in keys.iter().enumerate() {
        let value = (i as u64).to_le_bytes();
        let result = txn.get(key).unwrap();
        assert_eq!(result, Some(&value));

        let expected_hash = Leaf { key_hash: *key, value }.hash_leaf(&mut hasher);
        let db_node = db.clone().get(&expected_hash);
        assert!(db_node.is_ok(), "Node should be available in DB after initial commit");
    }
    
    // Remove keys 1, 3, and 5
    let keys_to_remove = [0, 2, 4];
    for &idx in &keys_to_remove {
        txn.remove(&keys[idx]).unwrap();
    }
    
    // Verify removed keys no longer exist
    for &idx in &keys_to_remove {
        let result = txn.get(&keys[idx]).unwrap();
        assert_eq!(result, None);

        let expected_hash = Leaf { key_hash: keys[idx], value: (idx as u64).to_le_bytes() }.hash_leaf(&mut hasher);
        let db_node = db.clone().get(&expected_hash);
        assert!(db_node.is_ok(), "Node should be still available in DB before commit");
    }
    
    // Verify remaining keys still exist
    for idx in 0..keys.len() {
        if !keys_to_remove.contains(&idx) {
            let value = (idx as u64).to_le_bytes();
            let result = txn.get(&keys[idx]).unwrap();
            assert_eq!(result, Some(&value));
        }
    }
    
    // Commit the transaction
    let mut hasher = DigestHasher::<Sha256>::default();
    let root_hash = txn.commit(&mut hasher).unwrap();
    
    // Create a new transaction with the updated root
    let builder = SnapshotBuilder::empty(db.clone()).with_trie_root_hash(root_hash);
    let txn = Transaction::from_snapshot_builder(builder);
    
    // Verify removed keys no longer exist in the database
    for &idx in &keys_to_remove {
        let result = txn.get(&keys[idx]);
        assert_eq!(result, Ok(None));

        let expected_hash = Leaf { key_hash: keys[idx], value: (idx as u64).to_le_bytes() }.hash_leaf(&mut hasher);
        let db_node = db.clone().get(&expected_hash);
        assert!(db_node.is_err(), "Node should be removed from DB after commit");
    }
    
    // Verify remaining keys still exist in the database
    for idx in 0..keys.len() {
        if !keys_to_remove.contains(&idx) {
            let value = (idx as u64).to_le_bytes();
            let result = txn.get(&keys[idx]).unwrap();
            assert_eq!(result, Some(&value));

            let expected_hash = Leaf { key_hash: keys[idx], value }.hash_leaf(&mut hasher);
            let db_node = db.clone().get(&expected_hash);
            assert!(db_node.is_ok(), "Node should be available in DB after commit");
        }
    }
}

#[test]
fn test_remove_nonexistent_key() {
    let db = Rc::new(MemoryDb::<[u8; 8]>::empty());
    let builder = SnapshotBuilder::empty(db.clone()).with_trie_root_hash(TrieRoot::default());
    let mut txn = Transaction::from_snapshot_builder(builder);
    
    // Insert a key
    let key_hash = KeyHash([1; 8]);
    let value = 42u64.to_le_bytes();
    txn.insert(&key_hash, value).unwrap();
    
    // Commit the transaction
    let mut hasher = DigestHasher::<Sha256>::default();
    let root_hash = txn.commit(&mut hasher).unwrap();
    
    // Create a new transaction with the updated root
    let builder = SnapshotBuilder::empty(db).with_trie_root_hash(root_hash);
    let mut txn = Transaction::from_snapshot_builder(builder);
    
    // Try to remove a nonexistent key
    let nonexistent_key = KeyHash([2; 8]);
    let result = txn.remove(&nonexistent_key);
    
    // Should succeed without error
    assert!(result.is_ok());
    
    // Verify the original key still exists
    let result = txn.get(&key_hash).unwrap();
    assert_eq!(result, Some(&value));
}
#[test]
fn test_database_node_removal() {
    let db = Rc::new(MemoryDb::<[u8; 8]>::empty());
    let builder = SnapshotBuilder::empty(db.clone()).with_trie_root_hash(TrieRoot::default());
    let mut txn = Transaction::from_snapshot_builder(builder);
    
    // Insert keys that will create a branch node
    let key1 = KeyHash([0, 1, 0, 0, 0, 0, 0, 0]);
    let key2 = KeyHash([0, 2, 0, 0, 0, 0, 0, 0]);
    let value1 = 1u64.to_le_bytes();
    let value2 = 2u64.to_le_bytes();
    
    txn.insert(&key1, value1).unwrap();
    txn.insert(&key2, value2).unwrap();
    
    // Commit to store nodes in the database
    let mut hasher = DigestHasher::<Sha256>::default();
    let root_hash = txn.commit(&mut hasher).unwrap();
    
    // Compute the expected node hashes for the inserted leaves
    let mut hasher = DigestHasher::<Sha256>::default();
    let expected_hash1 = Leaf { key_hash: key1, value: value1 }.hash_leaf(&mut hasher);
    let expected_hash2 = Leaf { key_hash: key2, value: value2 }.hash_leaf(&mut hasher);
    
    // Check that the nodes are available in the db directly
    let db_node1 = db.clone().get(&expected_hash1);
    let db_node2 = db.clone().get(&expected_hash2);
    assert!(db_node1.is_ok(), "Node 1 should be available in DB after initial commit");
    assert!(db_node2.is_ok(), "Node 2 should be available in DB after initial commit");
    
    // Create a new transaction with the updated root
    let builder = SnapshotBuilder::empty(db.clone()).with_trie_root_hash(root_hash);
    let mut txn = Transaction::from_snapshot_builder(builder);
    
    // Verify both keys exist
    assert_eq!(txn.get(&key1).unwrap(), Some(&value1));
    assert_eq!(txn.get(&key2).unwrap(), Some(&value2));
    
    // Remove one key
    txn.remove(&key1).unwrap();
    
    // Before committing the removal, the node should still be in the db
    let db_node1 = db.clone().get(&expected_hash1);
    assert!(db_node1.is_ok(), "Node 1 should still be available in DB after removal before commit");
    
    // Commit the transaction
    let mut hasher = DigestHasher::<Sha256>::default();
    let new_root_hash = txn.commit(&mut hasher).unwrap();
    
    // After commit, the removed node should be gone from the db
    let db_node1 = db.clone().get(&expected_hash1);
    let db_node2 = db.clone().get(&expected_hash2);
    assert!(db_node1.is_err(), "Node 1 should be removed from DB after commit");
    assert!(db_node2.is_ok(), "Node 2 should still be available in DB after commit");
    
    // Create a new transaction with the updated root
    let builder = SnapshotBuilder::empty(db.clone()).with_trie_root_hash(new_root_hash);
    let txn = Transaction::from_snapshot_builder(builder);
    
    // Verify key1 is gone and key2 still exists
    let result1 = txn.get(&key1);
    let result2 = txn.get(&key2);
    
    assert_eq!(result1, Ok(None));
    assert_eq!(result2, Ok(Some(&value2)));
}

#[test]
fn test_insert_remove_without_commit() {
    let db = Rc::new(MemoryDb::<[u8; 8]>::empty());
    let builder = SnapshotBuilder::empty(db).with_trie_root_hash(TrieRoot::default());
    let mut txn = Transaction::from_snapshot_builder(builder);
    
    // Insert a key
    let key_hash = KeyHash([1; 8]);
    let value = 42u64.to_le_bytes();
    txn.insert(&key_hash, value).unwrap();
    
    // Verify the key exists
    let result = txn.get(&key_hash).unwrap();
    assert_eq!(result, Some(&value));
    
    // Remove the key without committing
    txn.remove(&key_hash).unwrap();
    
    // Verify the key no longer exists in the transaction
    let result = txn.get(&key_hash).unwrap();
    assert_eq!(result, None);
    
    // Commit the transaction
    let root_hash = txn.commit(&mut DigestHasher::<Sha256>::default()).unwrap();
    
    // Root should be empty
    assert_eq!(root_hash, TrieRoot::default());
}

#[test]
fn test_multiple_insert_remove_without_commit() {
    let db = Rc::new(MemoryDb::<[u8; 8]>::empty());
    let builder = SnapshotBuilder::empty(db).with_trie_root_hash(TrieRoot::default());
    let mut txn = Transaction::from_snapshot_builder(builder);
    
    // Insert multiple keys
    let keys = [
        KeyHash([1, 0, 0, 0, 0, 0, 0, 0]),
        KeyHash([2, 0, 0, 0, 0, 0, 0, 0]),
        KeyHash([3, 0, 0, 0, 0, 0, 0, 0]),
    ];
    
    for (i, key) in keys.iter().enumerate() {
        let value = (i as u64).to_le_bytes();
        txn.insert(key, value).unwrap();
    }
    
    // Verify all keys exist
    for (i, key) in keys.iter().enumerate() {
        let value = (i as u64).to_le_bytes();
        let result = txn.get(key).unwrap();
        assert_eq!(result, Some(&value));
    }
    
    // Remove the keys without committing
    for key in &keys {
        txn.remove(key).unwrap();
    }
    
    // Verify the keys no longer exist in the transaction
    for key in &keys {
        let result = txn.get(key).unwrap();
        assert_eq!(result, None);
    }
    
    // Commit the transaction
    let root_hash = txn.commit(&mut DigestHasher::<Sha256>::default()).unwrap();
    
    // Root should be empty
    assert_eq!(root_hash, TrieRoot::default());
}

#[test]
fn test_insert_remove_same_key_multiple_times() {
    let db = Rc::new(MemoryDb::<[u8; 8]>::empty());
    let builder = SnapshotBuilder::empty(db).with_trie_root_hash(TrieRoot::default());
    let mut txn = Transaction::from_snapshot_builder(builder);
    
    let key_hash = KeyHash([1; 8]);
    let value1 = 42u64.to_le_bytes();
    let value2 = 84u64.to_le_bytes();
    
    // Insert a key
    txn.insert(&key_hash, value1).unwrap();
    
    // Verify the key exists
    let result = txn.get(&key_hash).unwrap();
    assert_eq!(result, Some(&value1));
    
    // Remove the key
    txn.remove(&key_hash).unwrap();
    
    // Verify the key no longer exists
    let result = txn.get(&key_hash).unwrap();
    assert_eq!(result, None);
    
    // Insert the same key with a different value
    txn.insert(&key_hash, value2).unwrap();
    
    // Verify the key exists with the new value
    let result = txn.get(&key_hash).unwrap();
    assert_eq!(result, Some(&value2));
    
    // Remove the key again
    txn.remove(&key_hash).unwrap();
    
    // Verify the key no longer exists
    let result = txn.get(&key_hash).unwrap();
    assert_eq!(result, None);
    
    // Commit the transaction
    let root_hash = txn.commit(&mut DigestHasher::<Sha256>::default()).unwrap();
    
    // Root should be empty
    assert_eq!(root_hash, TrieRoot::default());
}

#[test]
fn test_transaction_size_after_operations() {
    let db = Rc::new(MemoryDb::<[u8; 8]>::empty());
    let builder = SnapshotBuilder::empty(db).with_trie_root_hash(TrieRoot::default());
    let mut txn = Transaction::from_snapshot_builder(builder);
    
    // We can't directly access the transaction's internal state,
    // but we can check if the root is empty before and after operations
    
    // Initially the root should be empty
    let is_empty_before = match txn.calc_root_hash(&mut DigestHasher::<Sha256>::default()).unwrap() {
        TrieRoot::Empty => true,
        _ => false,
    };
    assert!(is_empty_before, "Trie should be empty initially");
    
    // Insert a key
    let key_hash = KeyHash([1; 8]);
    let value = 42u64.to_le_bytes();
    txn.insert(&key_hash, value).unwrap();
    
    // Root should not be empty after insertion
    let is_empty_after_insert = match txn.calc_root_hash(&mut DigestHasher::<Sha256>::default()).unwrap() {
        TrieRoot::Empty => true,
        _ => false,
    };
    assert!(!is_empty_after_insert, "Trie should not be empty after insertion");
    
    // Remove the key
    txn.remove(&key_hash).unwrap();
    
    // Root should be empty again after removal
    let is_empty_after_remove = match txn.calc_root_hash(&mut DigestHasher::<Sha256>::default()).unwrap() {
        TrieRoot::Empty => true,
        _ => false,
    };
    assert!(is_empty_after_remove, "Trie should be empty after removing the only key");
}
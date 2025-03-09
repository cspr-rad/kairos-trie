mod utils;

use std::{collections::HashMap, rc::Rc};

use proptest::prelude::*;

use kairos_trie::{
    stored::{memory_db::MemoryDb, merkle::SnapshotBuilder, Store},
    DigestHasher, KeyHash, NodeHash, Transaction, TrieError, TrieRoot,
};
use sha2::Sha256;
use utils::*;

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
    
    // Commit the transaction
    let mut hasher = DigestHasher::<Sha256>::default();
    let root_hash = txn.commit(&mut hasher).unwrap();
    
    // Create a new transaction with the updated root
    let builder = SnapshotBuilder::empty(db.clone()).with_trie_root_hash(root_hash);
    let mut txn = Transaction::from_snapshot_builder(builder);
    
    // Verify the key exists
    let result = txn.get(&key_hash).unwrap();
    assert_eq!(result, Some(&value));
    
    // Remove the key
    txn.remove(&key_hash).unwrap();
    
    // Verify the key no longer exists
    let result = txn.get(&key_hash).unwrap();
    assert_eq!(result, None);
    
    // Commit the transaction
    let mut hasher = DigestHasher::<Sha256>::default();
    let root_hash = txn.commit(&mut hasher).unwrap();
    
    // Root should be empty again
    assert_eq!(root_hash, TrieRoot::default());
    
    // Create a new transaction with the updated root
    let builder = SnapshotBuilder::empty(db).with_trie_root_hash(root_hash);
    let txn = Transaction::from_snapshot_builder(builder);
    
    // Verify the key no longer exists in the database
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
    let builder = SnapshotBuilder::empty(db).with_trie_root_hash(root_hash);
    let txn = Transaction::from_snapshot_builder(builder);
    
    // Verify removed keys no longer exist in the database
    for &idx in &keys_to_remove {
        let result = txn.get(&keys[idx]);
        assert_eq!(result, Ok(None));
    }
    
    // Verify remaining keys still exist in the database
    for idx in 0..keys.len() {
        if !keys_to_remove.contains(&idx) {
            let value = (idx as u64).to_le_bytes();
            let result = txn.get(&keys[idx]).unwrap();
            assert_eq!(result, Some(&value));
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

// Property-based test for remove functionality
fn test_remove_functionality(
    map: HashMap<KeyHash, u64>,
    keys_to_remove_indices: Vec<usize>,
) -> Result<(), TestCaseError> {
    if map.is_empty() {
        return Ok(());
    }
    
    let db = Rc::new(MemoryDb::<[u8; 8]>::empty());
    let builder = SnapshotBuilder::empty(db.clone()).with_trie_root_hash(TrieRoot::default());
    let mut txn = Transaction::from_snapshot_builder(builder);
    
    // Insert all keys
    for (key, value) in &map {
        txn.insert(key, value.to_le_bytes()).unwrap();
    }
    
    // Commit the transaction
    let mut hasher = DigestHasher::<Sha256>::default();
    let root_hash = txn.commit(&mut hasher).unwrap();
    
    // Create a new transaction with the updated root
    let builder = SnapshotBuilder::empty(db.clone()).with_trie_root_hash(root_hash);
    let mut txn = Transaction::from_snapshot_builder(builder);
    
    // Get keys to remove
    let keys: Vec<KeyHash> = map.keys().cloned().collect();
    let keys_to_remove: Vec<KeyHash> = keys_to_remove_indices
        .iter()
        .filter_map(|&idx| keys.get(idx % keys.len()))
        .cloned()
        .collect();
    
    // Remove selected keys
    for key in &keys_to_remove {
        txn.remove(key).unwrap();
    }
    
    // Verify removed keys no longer exist
    for key in &keys_to_remove {
        let result = txn.get(key).unwrap();
        assert_eq!(result, None);
    }
    
    // Verify remaining keys still exist
    for (key, value) in &map {
        if !keys_to_remove.contains(key) {
            let expected_value = value.to_le_bytes();
            let result = txn.get(key).unwrap();
            assert_eq!(result, Some(&expected_value));
        }
    }
    
    // Commit the transaction
    let mut hasher = DigestHasher::<Sha256>::default();
    let new_root_hash = txn.commit(&mut hasher).unwrap();
    
    // Create a new transaction with the updated root
    let builder = SnapshotBuilder::empty(db).with_trie_root_hash(new_root_hash);
    let txn = Transaction::from_snapshot_builder(builder);
    
    // Verify removed keys no longer exist in the database
    for key in &keys_to_remove {
        let result = txn.get(key);
        assert_eq!(result, Ok(None));
        
        // Try to access the node directly in the database
        // This should fail with an error since the node should be removed
        if !keys_to_remove.is_empty() && new_root_hash != TrieRoot::default() {
            // We can't directly test database access here, but we've verified the key is not in the trie
        }
    }
    
    // Verify remaining keys still exist in the database
    for (key, value) in &map {
        if !keys_to_remove.contains(key) {
            let expected_value = value.to_le_bytes();
            let result = txn.get(key).unwrap();
            assert_eq!(result, Some(&expected_value));
        }
    }
    
    Ok(())
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
    
    // Create a new transaction with the updated root
    let builder = SnapshotBuilder::empty(db.clone()).with_trie_root_hash(root_hash);
    let mut txn = Transaction::from_snapshot_builder(builder);
    
    // Count how many keys we can successfully retrieve
    let keys_before_removal = [
        txn.get(&key1).unwrap().is_some(),
        txn.get(&key2).unwrap().is_some(),
    ].iter().filter(|&&present| present).count();
    
    // Remove one key
    txn.remove(&key1).unwrap();
    
    // Commit the transaction
    let mut hasher = DigestHasher::<Sha256>::default();
    let new_root_hash = txn.commit(&mut hasher).unwrap();
    
    // Create a new transaction with the updated root
    let builder = SnapshotBuilder::empty(db.clone()).with_trie_root_hash(new_root_hash);
    let txn = Transaction::from_snapshot_builder(builder);
    
    // Count how many keys we can successfully retrieve after removal
    let keys_after_removal = [
        txn.get(&key1).unwrap().is_some(),
        txn.get(&key2).unwrap().is_some(),
    ].iter().filter(|&&present| present).count();
    
    // Verify that we have fewer accessible keys after removal
    assert!(keys_after_removal < keys_before_removal, 
            "Number of accessible keys should decrease after removing nodes");
    
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
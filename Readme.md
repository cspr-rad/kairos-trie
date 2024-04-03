# A ZkVM Optimized Binary Patricia Merkle Trie

This project implements a binary Patricia Merkle trie optimized for use in zero-knowledge virtual machines (ZkVMs). The trie's API closely matches that of `std::HashMap`, allowing for familiar and intuitive usage.

## Features

- Pluggable external node storage for use with any key-value database
- Multi-node transactions
- Succinct Merkle proofs of pre-transaction tree state (Snapshot)
- Incremental recalculation of post-transaction Merkle root
- Efficient Snapshot Merkle root verification
- `no_std` compatible

## Transactional Operations and Merkle Proofs

Operations (`get`, `insert`, `entry` API) on the trie are performed within Transactions. After executing a transaction, you can obtain the new Merkle root of the trie and a snapshot. The snapshot contains only the parts of the pre-transaction trie that the transaction depended on.

This allows you to verify the correctness of the operations on the trie without requiring the whole trie. You only need the Snapshot, which contains the minimum amount of data required to verify all operations in a Transaction. You can easily verify the transactions by rerunning the transaction logic against the Snapshot in a zkVM or other verifiable or trusted environment.

## Performance Characteristics

This trie is optimized for 32-bit zkVMs and small proof sizes, not real hardware. It is a binary trie, not a standard base-16 trie, we are trading an increased number of branches that must be traversed for smaller proofs.
This is a great tradeoff for a zkVM, not so much for an SSD. When using this trie, it is recommended to maintain a cache key-value database and only use the trie for proof generation and verification.

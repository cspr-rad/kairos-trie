use kairos_trie::KeyHash;
use proptest::prelude::*;

pub mod insert_get;
pub mod operations;

prop_compose! {
    pub fn arb_key_hash()(data in any::<[u8; 32]>()) -> KeyHash {
        KeyHash::from(&data)
    }
}

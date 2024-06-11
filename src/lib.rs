#![allow(clippy::type_complexity)]
#![warn(clippy::missing_inline_in_public_items)]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use core::fmt::{Debug, Display};

mod errors;
mod hash;
pub mod stored;
mod transaction;

pub use errors::TrieError;
pub use hash::{DigestHasher, PortableHash, PortableHasher, PortableUpdate};
pub use transaction::{
    nodes::{Branch, Leaf, Node, TrieRoot},
    Entry, OccupiedEntry, Transaction, VacantEntry, VacantEntryEmptyTrie,
};

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct KeyHash(pub [u32; 8]);

impl KeyHash {
    #[inline]
    pub fn from_bytes(hash_key: &[u8; 32]) -> Self {
        let mut r = [0; 8];

        hash_key
            .chunks_exact(4)
            .enumerate()
            .for_each(|(i, chunk)| r[i] = u32::from_le_bytes(chunk.try_into().unwrap()));

        Self(r)
    }

    #[inline]
    pub fn to_bytes(&self) -> [u8; 32] {
        let mut r = [0; 32];

        self.0.iter().enumerate().for_each(|(i, &word)| {
            let [a, b, c, d] = word.to_le_bytes();
            let offset = i * 4;
            r[offset] = a;
            r[offset + 1] = b;
            r[offset + 2] = c;
            r[offset + 3] = d;
        });

        r
    }
}

impl From<&[u8; 32]> for KeyHash {
    #[inline]
    fn from(hash_key: &[u8; 32]) -> Self {
        Self::from_bytes(hash_key)
    }
}

impl From<&KeyHash> for [u8; 32] {
    #[inline]
    fn from(hash: &KeyHash) -> [u8; 32] {
        hash.to_bytes()
    }
}

impl PortableHash for KeyHash {
    #[inline]
    fn portable_hash<H: PortableUpdate>(&self, hasher: &mut H) {
        self.0.portable_hash(hasher);
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct NodeHash {
    pub bytes: [u8; 32],
}

impl NodeHash {
    #[inline]
    pub fn new(bytes: [u8; 32]) -> Self {
        Self { bytes }
    }
}

impl AsRef<[u8]> for NodeHash {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

impl Display for NodeHash {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // TODO hex
        write!(f, "NodeHash({:?})", &self.bytes)
    }
}

impl From<[u8; 32]> for NodeHash {
    #[inline]
    fn from(bytes: [u8; 32]) -> Self {
        Self::new(bytes)
    }
}

impl From<&[u8; 32]> for NodeHash {
    #[inline]
    fn from(bytes: &[u8; 32]) -> Self {
        Self::new(*bytes)
    }
}

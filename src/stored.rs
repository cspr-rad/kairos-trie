pub mod merkle;

use std::hash::Hash;

use alloc::{collections::BTreeMap, fmt::Debug, string::String};

use crate::{Branch, Extension, Leaf};

pub trait MerkleStore<V>: Store<V> {
    fn get_branch_hash(&self, idx: Self::BranchRef) -> &BranchHash;
    fn get_extension_hash(&self, idx: Self::ExtensionRef) -> &ExtensionHash;
    fn get_leaf_hash(&self, idx: Self::LeafRef) -> &LeafHash;
}

pub trait Store<V> {
    /// The hash of a node or leaf.
    /// Alternatively, this could be a reference or an index that uniquely identifies a node or leaf
    type BranchRef: Copy + Clone + Eq + Ord + Hash + Debug;
    type ExtensionRef: Copy + Clone + Eq + Ord + Hash + Debug;
    type LeafRef: Copy + Clone + Eq + Ord + Hash + Debug;
    type Error: Into<String>;

    fn get_branch(
        &self,
        idx: Self::BranchRef,
    ) -> Result<&Branch<Node<Self::BranchRef, Self::ExtensionRef, Self::LeafRef>>, Self::Error>;
    fn get_extension(
        &self,
        idx: Self::ExtensionRef,
    ) -> Result<&Extension<Self::BranchRef, Self::ExtensionRef, Self::LeafRef, V>, Self::Error>;

    fn get_leaf(&self, idx: Self::LeafRef) -> Result<&Leaf<V>, Self::Error>;
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Node<B, E, L> {
    Branch(B),
    Extension(E),
    Leaf(L),
}

pub enum Error {
    NodeNotFound,
}

impl From<Error> for String {
    fn from(err: Error) -> String {
        match err {
            Error::NodeNotFound => "Node not found".into(),
        }
    }
}

pub type NodeHash = [u8; 32];

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct BranchHash(pub NodeHash);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ExtensionHash(pub NodeHash);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct LeafHash(pub NodeHash);

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct MemoryStore<V> {
    // TODO: use a indexmap
    branches: BTreeMap<BranchHash, Branch<Node<BranchHash, ExtensionHash, LeafHash>>>,
    extensions: BTreeMap<ExtensionHash, Extension<BranchHash, ExtensionHash, LeafHash, V>>,
    leaves: BTreeMap<LeafHash, Leaf<V>>,
}

impl<V> Store<V> for MemoryStore<V> {
    type BranchRef = BranchHash;
    type ExtensionRef = ExtensionHash;
    type LeafRef = LeafHash;
    type Error = Error;

    fn get_branch(
        &self,
        idx: Self::BranchRef,
    ) -> Result<&Branch<Node<Self::BranchRef, Self::ExtensionRef, Self::LeafRef>>, Self::Error>
    {
        self.branches.get(&idx).ok_or(Error::NodeNotFound)
    }

    fn get_extension(
        &self,
        idx: Self::ExtensionRef,
    ) -> Result<&Extension<Self::BranchRef, Self::ExtensionRef, Self::LeafRef, V>, Self::Error>
    {
        self.extensions.get(&idx).ok_or(Error::NodeNotFound)
    }

    fn get_leaf(&self, idx: Self::LeafRef) -> Result<&Leaf<V>, Self::Error> {
        self.leaves.get(&idx).ok_or(Error::NodeNotFound)
    }
}

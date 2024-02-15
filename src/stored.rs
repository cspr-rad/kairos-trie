pub mod merkle;

use std::hash::Hash;

use alloc::{collections::BTreeMap, fmt::Debug, string::String};

use crate::{Branch, Extension, Leaf};

pub trait Store<V> {
    /// The hash of a node or leaf.
    /// Alternatively, this could be a reference or an index that uniquely identifies a node or leaf
    type BranchRef: Copy + Clone + Eq + Ord + Hash + Debug;
    type ExtensionRef: Copy + Clone + Eq + Ord + Hash + Debug;
    type LeafRef: Copy + Clone + Eq + Ord + Hash + Debug;
    type Error: Into<String>;

    fn get_branch(
        &self,
        hash: Self::BranchRef,
    ) -> Result<&Branch<Node<Self::BranchRef, Self::ExtensionRef, Self::LeafRef>>, Self::Error>;
    fn get_extension(
        &self,
        hash: Self::ExtensionRef,
    ) -> Result<&Extension<Self::BranchRef, Self::ExtensionRef, Self::LeafRef, V>, Self::Error>;

    fn get_leaf(&self, hash: Self::LeafRef) -> Result<&Leaf<V>, Self::Error>;
    fn get_leaf_hash(&self, leaf: &Self::LeafRef) -> Result<&LeafHash, Self::Error>;
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

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct BranchHash(pub [u8; 32]);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ExtensionHash(pub [u8; 32]);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct LeafHash(pub [u8; 32]);

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
        hash: Self::BranchRef,
    ) -> Result<&Branch<Node<Self::BranchRef, Self::ExtensionRef, Self::LeafRef>>, Self::Error>
    {
        self.branches.get(&hash).ok_or(Error::NodeNotFound)
    }

    fn get_extension(
        &self,
        hash: Self::ExtensionRef,
    ) -> Result<&Extension<Self::BranchRef, Self::ExtensionRef, Self::LeafRef, V>, Self::Error>
    {
        self.extensions.get(&hash).ok_or(Error::NodeNotFound)
    }

    fn get_leaf(&self, hash: Self::LeafRef) -> Result<&Leaf<V>, Self::Error> {
        self.leaves.get(&hash).ok_or(Error::NodeNotFound)
    }

    #[inline(always)]
    fn get_leaf_hash(&self, leaf: &Self::LeafRef) -> Result<&LeafHash, Self::Error> {
        Ok(leaf)
    }
}

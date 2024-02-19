use std::ops::Index;

use alloc::vec::Vec;

use crate::{Branch, Extension, Leaf, Store};

use super::{BranchHash, Error, ExtensionHash, LeafHash, Node, NodeHash};

pub struct Snapshot<V> {
    branches: Branches<BranchIdx, ExtensionIdx, LeafIdx>,
    extension: Extensions<BranchIdx, ExtensionIdx, LeafIdx, V>,
    leaves: Leaves<V>,

    unvisited: Vec<NodeHash>,
}

impl<V> Store<V> for Snapshot<V> {
    type BranchRef = BranchIdx;
    type ExtensionRef = ExtensionIdx;
    type LeafRef = LeafIdx;
    type Error = Error;

    fn get_branch(
        &self,
        idx: Self::BranchRef,
    ) -> Result<&Branch<Node<Self::BranchRef, Self::ExtensionRef, Self::LeafRef>>, Self::Error>
    {
        self.branches
            .0
            .get(idx.0 as usize)
            .ok_or(Error::NodeNotFound)
    }

    fn get_extension(
        &self,
        idx: Self::ExtensionRef,
    ) -> Result<&Extension<Self::BranchRef, Self::ExtensionRef, Self::LeafRef, V>, Self::Error>
    {
        self.extension
            .0
            .get(idx.0 as usize)
            .ok_or(Error::NodeNotFound)
    }

    fn get_leaf(&self, idx: Self::LeafRef) -> Result<&Leaf<V>, Self::Error> {
        self.leaves.0.get(idx.0 as usize).ok_or(Error::NodeNotFound)
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Branches<B, E, L>(pub Vec<Branch<Node<B, E, L>>>);

impl<B, E, L> Default for Branches<B, E, L> {
    fn default() -> Self {
        Branches(Vec::new())
    }
}

impl<B, E, L> Index<BranchIdx> for Branches<B, E, L> {
    type Output = Branch<Node<B, E, L>>;
    fn index(&self, idx: BranchIdx) -> &Self::Output {
        &self.0[idx.0 as usize]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct BranchIdx(pub u32);

impl From<usize> for BranchIdx {
    fn from(idx: usize) -> Self {
        BranchIdx(idx as u32)
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct Extensions<B, E, L, V>(pub Vec<Extension<B, E, L, V>>);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct ExtensionIdx(pub u32);

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Leaves<V>(pub Vec<Leaf<V>>);

impl<V> Index<LeafIdx> for Leaves<V> {
    type Output = Leaf<V>;
    fn index(&self, idx: LeafIdx) -> &Leaf<V> {
        &self.0[idx.0 as usize]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct LeafIdx(pub u32);

impl From<usize> for LeafIdx {
    fn from(idx: usize) -> Self {
        LeafIdx(idx as u32)
    }
}

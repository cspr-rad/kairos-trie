use std::ops::Index;

use alloc::vec::Vec;

use crate::{stored, KeyHash, Leaf};

type ModBranch<SBR, SLR> = crate::Branch<NodeRef<SBR, SLR>>;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum NodeRef<SBR, SLR> {
    ModBranch(BranchIdx),
    ModLeaf(LeafIdx),
    StoredBranch(SBR),
    StoredLeaf(SLR),
}

impl<SBR, SLR> From<stored::Node<SBR, SLR>> for NodeRef<SBR, SLR> {
    fn from(node_ref: stored::Node<SBR, SLR>) -> Self {
        match node_ref {
            stored::Node::Branch(branch) => NodeRef::StoredBranch(branch),
            stored::Node::Leaf(leaf) => NodeRef::StoredLeaf(leaf),
        }
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Branches<SBR, SLR>(pub Vec<ModBranch<SBR, SLR>>);

impl<SBR, SLR> Branches<SBR, SLR> {
    pub fn push(&mut self, branch: ModBranch<SBR, SLR>) -> BranchIdx {
        let idx = BranchIdx(self.0.len() as u32);
        self.0.push(branch);
        idx
    }
}

impl<SBR, SLR> Default for Branches<SBR, SLR> {
    fn default() -> Self {
        Branches(Vec::new())
    }
}

impl<SBR, SLR> Index<BranchIdx> for Branches<SBR, SLR> {
    type Output = ModBranch<SBR, SLR>;
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

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Leaves(pub Vec<(KeyHash, Leaf)>);

impl Leaves {
    pub fn push(&mut self, key: KeyHash, leaf: Leaf) -> LeafIdx {
        let idx = LeafIdx(self.0.len() as u32);
        self.0.push((key, leaf));
        idx
    }
}

impl Index<LeafIdx> for Leaves {
    type Output = (KeyHash, Leaf);
    fn index(&self, idx: LeafIdx) -> &(KeyHash, Leaf) {
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

use std::ops::Index;

use alloc::vec::Vec;

use crate::{stored, Leaf};

type ModBranch<SBR, SER, SLR> = crate::Branch<NodeRef<SBR, SER, SLR>>;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum NodeRef<SBR, SER, SLR> {
    ModBranch(BranchIdx),
    ModExtension(usize),

    ModLeaf(LeafIdx),
    StoredBranch(SBR),
    StoredExtension(SER),
    StoredLeaf(SLR),
}

impl<SBR, SER, SLR> From<stored::Node<SBR, SER, SLR>> for NodeRef<SBR, SER, SLR> {
    fn from(node_ref: stored::Node<SBR, SER, SLR>) -> Self {
        match node_ref {
            stored::Node::Branch(branch) => NodeRef::StoredBranch(branch),
            stored::Node::Extension(extension) => NodeRef::StoredExtension(extension),
            stored::Node::Leaf(leaf) => NodeRef::StoredLeaf(leaf),
        }
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Branches<SBR, SER, SLR>(pub Vec<ModBranch<SBR, SER, SLR>>);

impl<SBR, SER, SLR> Branches<SBR, SER, SLR> {
    pub fn push(&mut self, branch: ModBranch<SBR, SER, SLR>) -> BranchIdx {
        let idx = BranchIdx(self.0.len() as u32);
        self.0.push(branch);
        idx
    }
}

impl<SBR, SER, SLR> Default for Branches<SBR, SER, SLR> {
    fn default() -> Self {
        Branches(Vec::new())
    }
}

impl<SBR, SER, SLR> Index<BranchIdx> for Branches<SBR, SER, SLR> {
    type Output = ModBranch<SBR, SER, SLR>;
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
pub struct Leaves(pub Vec<Leaf>);

impl Leaves {
    pub fn push(&mut self, leaf: Leaf) -> LeafIdx {
        let idx = LeafIdx(self.0.len() as u32);
        self.0.push(leaf);
        idx
    }
}

impl Index<LeafIdx> for Leaves {
    type Output = Leaf;
    fn index(&self, idx: LeafIdx) -> &Leaf {
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

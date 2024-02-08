use std::ops::Index;

use crate::{KeyHash, Leaf, NodeHash};

/// We may need to merge the `Branch` and `NodeRef` types into a single type.
/// That would give us a 16 byte layout, where bit_idx: u32.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Branch {
    pub bit_idx: u8,
    pub left: NodeRef,
    pub right: NodeRef,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum NodeRef {
    ModLeaf(LeafIdx),
    ModNode(BranchIdx),
    // TODO: take NodeHash by reference or index
    StoredNode(NodeHash),
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Branches(pub Vec<Branch>);

impl Index<BranchIdx> for Branches {
    type Output = Branch;
    fn index(&self, idx: BranchIdx) -> &Branch {
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

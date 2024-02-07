use std::ops::Index;

use crate::{stored::HashIdx, Leaf};

/// We may need to merge the `Branch` and `NodeRef` types into a single type.
/// That would give us a 16 byte layout, where bit_idx: u32.
pub struct Branch {
    bit_idx: u8,
    left: NodeRef,
    right: NodeRef,
}

pub enum NodeRef {
    ModLeaf(LeafIdx),
    ModNode(NodeIdx),
    StoredNode(HashIdx),
}

pub struct Branches(pub Vec<Branch>);

impl Index<NodeIdx> for Branches {
    type Output = Branch;
    fn index(&self, idx: NodeIdx) -> &Branch {
        &self.0[idx.0 as usize]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct NodeIdx(pub u32);

impl From<usize> for NodeIdx {
    fn from(idx: usize) -> Self {
        NodeIdx(idx as u32)
    }
}

pub struct Leaves(pub Vec<Leaf>);

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

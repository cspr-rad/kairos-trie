use crate::Leaf;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum StoredNode {
    Branch {
        bit_idx: u8,
        left: NodeHash,
        right: NodeHash,
    },
    Leaf(Leaf),
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct NodeHash(pub [u8; 32]);

pub struct HashIdx(pub u32);

impl From<usize> for HashIdx {
    fn from(idx: usize) -> Self {
        HashIdx(idx as u32)
    }
}

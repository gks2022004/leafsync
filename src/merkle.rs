use crate::chunk::ChunkInfo;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MerkleNode {
    pub hash: [u8; 32],
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MerkleTree {
    pub leaves: Vec<MerkleNode>,
    pub upper: Vec<Vec<MerkleNode>>, // upper[0] is parents of leaves
}

pub fn build_merkle(chunks: &[ChunkInfo]) -> MerkleTree {
    let leaves: Vec<MerkleNode> = chunks
        .iter()
        .map(|c| MerkleNode { hash: c.hash })
        .collect();
    let mut level = leaves.clone();
    let mut upper: Vec<Vec<MerkleNode>> = Vec::new();
    while level.len() > 1 {
        let mut next = Vec::new();
        for pair in level.chunks(2) {
            let h = if pair.len() == 2 {
                hash_pair(pair[0].hash, pair[1].hash)
            } else {
                // duplicate last
                hash_pair(pair[0].hash, pair[0].hash)
            };
            next.push(MerkleNode { hash: h });
        }
        upper.push(next.clone());
        level = next;
    }
    MerkleTree { leaves, upper }
}

pub fn root_hash(tree: &MerkleTree) -> [u8; 32] {
    if tree.leaves.is_empty() {
        [0u8; 32]
    } else if tree.upper.is_empty() {
        tree.leaves[0].hash
    } else {
        // top level is last in upper
        tree.upper.last().unwrap()[0].hash
    }
}

fn hash_pair(a: [u8; 32], b: [u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(&a);
    hasher.update(&b);
    hasher.finalize().into()
}

// Compute which chunk indices differ by comparing two trees.
#[allow(dead_code)]
pub fn diff_chunks(a: &MerkleTree, b: &MerkleTree) -> Vec<u64> {
    if a.leaves.len() != b.leaves.len() {
        // lengths differ, request all from b
        return (0..b.leaves.len() as u64).collect();
    }
    let mut diffs = Vec::new();
    for i in 0..a.leaves.len() {
        if a.leaves[i].hash != b.leaves[i].hash {
            diffs.push(i as u64);
        }
    }
    diffs
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum Msg {
    Version { major: u16, minor: u16 },
    Hello { folder: String },
    Summary { files: Vec<FileSummary> },
    RequestFile { rel_path: String },
    FileMeta { rel_path: String, size: u64, chunk_count: u64, root: [u8; 32], chunk_hashes: Vec<[u8; 32]> },
    RequestChunks { rel_path: String, indices: Vec<u64> },
    ChunkData { rel_path: String, index: u64, data: Vec<u8> },
    Done,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileSummary {
    pub rel_path: String,
    pub size: u64,
    pub chunk_count: u64,
    pub root: [u8; 32],
}

pub fn encode(msg: &Msg) -> Vec<u8> {
    bincode::serialize(msg).expect("serialize")
}

pub fn decode(buf: &[u8]) -> Msg {
    bincode::deserialize(buf).expect("deserialize")
}

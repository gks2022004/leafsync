use crate::{chunk::{chunk_file, rel_paths_in_dir, ChunkInfo, write_chunk}, merkle::{build_merkle, root_hash}, protocol::{FileSummary}};
use anyhow::{Result};
use std::path::{Path, PathBuf};

pub fn build_file_summary(root: &Path, rel: &Path) -> Result<(FileSummary, Vec<ChunkInfo>)> {
    let abs = root.join(rel);
    let meta = std::fs::metadata(&abs)?;
    let chunks = chunk_file(&abs)?;
    let tree = build_merkle(&chunks);
    let root_hash_v = root_hash(&tree);
    Ok((FileSummary {
        rel_path: rel.to_string_lossy().to_string(),
        size: meta.len(),
        chunk_count: chunks.len() as u64,
        root: root_hash_v,
    }, chunks))
}

pub fn all_summaries(root: &Path) -> Result<Vec<(FileSummary, Vec<ChunkInfo>)>> {
    let mut out = Vec::new();
    for rel in rel_paths_in_dir(root)? {
        let (s, chunks) = build_file_summary(root, &rel)?;
        out.push((s, chunks));
    }
    Ok(out)
}

pub fn diff_needed_indices(local_chunks: &[ChunkInfo], remote_chunk_hashes: &[[u8; 32]]) -> Vec<u64> {
    // Compare chunk-by-chunk hashes
    let mut need = Vec::new();
    for (i, remote_h) in remote_chunk_hashes.iter().enumerate() {
        let miss = match local_chunks.get(i) {
            Some(c) => &c.hash != remote_h,
            None => true,
        };
        if miss { need.push(i as u64); }
    }
    need
}

pub fn apply_chunk(root: &Path, rel_path: &str, index: u64, data: &[u8]) -> Result<()> {
    let abs = root.join(rel_path);
    if let Some(parent) = abs.parent() { std::fs::create_dir_all(parent)?; }
    write_chunk(&abs, index, data)?;
    Ok(())
}

pub fn path_from_rel(root: &Path, rel: &str) -> PathBuf {
    root.join(rel)
}

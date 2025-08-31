use crate::{chunk::{chunk_file, rel_paths_in_dir, ChunkInfo, write_chunk, CHUNK_SIZE}, merkle::{build_merkle, root_hash}, protocol::{FileSummary}};
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

/// Ensure the file is truncated to the expected number of chunks.
pub fn truncate_to_chunks(root: &Path, rel_path: &str, chunk_count: u64, last_chunk_size: Option<usize>) -> Result<()> {
    use std::io::{Seek, SeekFrom, Write};
    let abs = root.join(rel_path);
    if !abs.exists() { return Ok(()); }
    let mut f = std::fs::OpenOptions::new().read(true).write(true).open(&abs)?;
    let expected_size = if chunk_count == 0 { 0 } else { (chunk_count - 1) * CHUNK_SIZE as u64 + last_chunk_size.unwrap_or(CHUNK_SIZE) as u64 };
    f.set_len(expected_size)?;
    f.seek(SeekFrom::Start(expected_size))?;
    f.flush()?;
    Ok(())
}

/// Truncate or extend the file to an exact byte size.
pub fn truncate_to_size(root: &Path, rel_path: &str, size: u64) -> Result<()> {
    let abs = root.join(rel_path);
    if !abs.exists() { return Ok(()); }
    let f = std::fs::OpenOptions::new().write(true).open(&abs)?;
    f.set_len(size)?;
    Ok(())
}

pub fn path_from_rel(root: &Path, rel: &str) -> PathBuf {
    root.join(rel)
}

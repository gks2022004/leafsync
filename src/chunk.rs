use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::{fs::File, io::{Read, Seek, SeekFrom}, path::{Path, PathBuf}};

pub const CHUNK_SIZE: usize = 1024 * 1024; // 1 MiB

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChunkInfo {
    pub index: u64,
    pub hash: [u8; 32],
    pub size: u32,
}

pub fn hash_bytes(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let out = hasher.finalize();
    out.into()
}

pub fn hash_file(path: &Path) -> Result<[u8; 32]> {
    let mut f = File::open(path).with_context(|| format!("open file {path:?}"))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().into())
}

pub fn chunk_file(path: &Path) -> Result<Vec<ChunkInfo>> {
    let mut f = File::open(path).with_context(|| format!("open file {path:?}"))?;
    let len = f.metadata()?.len();
    let mut chunks = Vec::new();
    let mut index = 0u64;
    let mut offset = 0u64;
    let mut buf = vec![0u8; CHUNK_SIZE];
    while offset < len {
        f.seek(SeekFrom::Start(offset))?;
        let read_len = std::cmp::min(CHUNK_SIZE as u64, len - offset) as usize;
        f.read_exact(&mut buf[..read_len])?;
        let hash = hash_bytes(&buf[..read_len]);
        chunks.push(ChunkInfo { index, hash, size: read_len as u32 });
        offset += read_len as u64;
        index += 1;
    }
    Ok(chunks)
}

pub fn read_chunk(path: &Path, index: u64) -> Result<Vec<u8>> {
    let mut f = File::open(path)?;
    let start = index * CHUNK_SIZE as u64;
    f.seek(SeekFrom::Start(start))?;
    let mut buf = vec![0u8; CHUNK_SIZE];
    let n = f.read(&mut buf)?;
    buf.truncate(n);
    Ok(buf)
}

pub fn write_chunk(path: &Path, index: u64, data: &[u8]) -> Result<()> {
    use std::io::{Seek, Write};
    let mut f = if path.exists() { std::fs::OpenOptions::new().read(true).write(true).open(path)? } else { std::fs::OpenOptions::new().create(true).write(true).open(path)? };
    let start = index * CHUNK_SIZE as u64;
    f.seek(SeekFrom::Start(start))?;
    f.write_all(data)?;
    Ok(())
}

pub fn rel_paths_in_dir(dir: &Path) -> Result<Vec<PathBuf>> {
    use walkdir::WalkDir;
    let mut out = Vec::new();
    for e in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        if e.file_type().is_file() {
            let p = e.path().to_path_buf();
            let rp = p.strip_prefix(dir).unwrap().to_path_buf();
            out.push(rp);
        }
    }
    Ok(out)
}

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, path::PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResumeEntry {
    pub size: u64,
    pub chunk_count: u64,
    pub root: [u8; 32],
    pub have: Vec<u8>, // bitset, length in bytes == ceil(chunk_count/8)
}

#[derive(Default, Serialize, Deserialize)]
pub struct ResumeStore { pub entries: HashMap<String, ResumeEntry> }

fn resume_path() -> Result<PathBuf> {
    let dir = crate::identity::state_dir()?;
    Ok(dir.join("resume.json"))
}

pub fn load_store() -> Result<ResumeStore> {
    let p = resume_path()?;
    if !p.exists() { return Ok(ResumeStore::default()); }
    let data = fs::read(&p).with_context(|| format!("read {p:?}"))?;
    Ok(serde_json::from_slice(&data).with_context(|| "parse resume.json")?)
}

pub fn save_store(store: &ResumeStore) -> Result<()> {
    let p = resume_path()?;
    let data = serde_json::to_vec_pretty(store)?;
    fs::write(&p, data).with_context(|| format!("write {p:?}"))
}

pub fn key(addr: &str, rel_path: &str, root_hex: &str) -> String {
    format!("{}|{}|{}", addr, rel_path, root_hex)
}

pub fn get(addr: &str, rel_path: &str, root: &[u8; 32]) -> Result<Option<ResumeEntry>> {
    let key_s = key(addr, rel_path, &hex(root));
    let store = load_store()?;
    Ok(store.entries.get(&key_s).cloned())
}

pub fn upsert_mark(addr: &str, rel_path: &str, size: u64, chunk_count: u64, root: [u8;32], index: u64) -> Result<()> {
    upsert_mark_many(addr, rel_path, size, chunk_count, root, &[index])
}

pub fn upsert_mark_many(addr: &str, rel_path: &str, size: u64, chunk_count: u64, root: [u8;32], indices: &[u64]) -> Result<()> {
    let mut store = load_store()?;
    let key_s = key(addr, rel_path, &hex(&root));
    let entry = store.entries.entry(key_s).or_insert_with(|| ResumeEntry {
        size,
        chunk_count,
        root,
        have: vec![0u8; bytes_len(chunk_count)],
    });
    // If metadata changed, reset entry
    if entry.size != size || entry.chunk_count != chunk_count || entry.root != root {
        *entry = ResumeEntry { size, chunk_count, root, have: vec![0u8; bytes_len(chunk_count)] };
    }
    for &i in indices {
        set_bit(&mut entry.have, i as usize);
    }
    save_store(&store)
}

pub fn missing_indices_for(addr: &str, rel_path: &str, size: u64, chunk_count: u64, root: [u8;32]) -> Result<Option<Vec<u64>>> {
    if let Some(entry) = get(addr, rel_path, &root)? {
        if entry.size == size && entry.chunk_count == chunk_count && entry.root == root {
            let mut need: Vec<u64> = Vec::new();
            for i in 0..(chunk_count as usize) {
                if !get_bit(&entry.have, i) { need.push(i as u64); }
            }
            return Ok(Some(need));
        }
    }
    Ok(None)
}

pub fn clear(addr: &str, rel_path: &str, root: [u8;32]) -> Result<()> {
    let mut store = load_store()?;
    let key_s = key(addr, rel_path, &hex(&root));
    store.entries.remove(&key_s);
    save_store(&store)
}

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn bytes_len(chunk_count: u64) -> usize {
    ((chunk_count + 7) / 8) as usize
}

fn set_bit(bits: &mut [u8], index: usize) {
    let byte = index / 8;
    let bit = index % 8;
    if byte < bits.len() { bits[byte] |= 1u8 << bit; }
}

fn get_bit(bits: &[u8], index: usize) -> bool {
    let byte = index / 8;
    let bit = index % 8;
    if byte >= bits.len() { return false; }
    (bits[byte] & (1u8 << bit)) != 0
}

pub fn list_all() -> Result<Vec<(String, ResumeEntry)>> {
    let s = load_store()?;
    Ok(s.entries.into_iter().collect())
}

pub fn parse_hex32(s: &str) -> Result<[u8;32]> {
    let s = s.trim();
    if s.len() != 64 { return Err(anyhow::anyhow!("expected 64 hex chars")); }
    let mut out = [0u8; 32];
    for i in 0..32 {
        let byte_str = &s[i*2..i*2+2];
        out[i] = u8::from_str_radix(byte_str, 16).map_err(|_| anyhow::anyhow!("invalid hex"))?;
    }
    Ok(out)
}

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, path::PathBuf};

#[derive(Default, Serialize, Deserialize)]
pub struct TrustStore {
    pub servers: HashMap<String, String>, // addr -> hex fingerprint
}

fn trust_path() -> Result<PathBuf> {
    let dir = crate::identity::state_dir()?;
    Ok(dir.join("trust.json"))
}

pub fn load() -> Result<TrustStore> {
    let p = trust_path()?;
    if !p.exists() { return Ok(TrustStore::default()); }
    let data = fs::read(&p).with_context(|| format!("read {p:?}"))?;
    Ok(serde_json::from_slice(&data).with_context(|| "parse trust.json")?)
}

pub fn save(store: &TrustStore) -> Result<()> {
    let p = trust_path()?;
    let data = serde_json::to_vec_pretty(store)?;
    fs::write(&p, data).with_context(|| format!("write {p:?}"))
}

pub fn get(addr: &str) -> Result<Option<String>> {
    let store = load()?;
    Ok(store.servers.get(addr).cloned())
}

pub fn set(addr: &str, fp_hex: &str) -> Result<()> {
    let mut store = load()?;
    store.servers.insert(addr.to_string(), fp_hex.to_string());
    save(&store)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    let out = h.finalize();
    out.iter().map(|b| format!("{:02x}", b)).collect()
}

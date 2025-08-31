use anyhow::{Context, Result};
use directories::ProjectDirs;
use rcgen::generate_simple_self_signed;
use rustls::{Certificate, PrivateKey};
use std::{fs, path::PathBuf, sync::Arc};

/// Returns the app-specific state directory, creating it if needed.
pub fn state_dir() -> Result<PathBuf> {
    let proj = ProjectDirs::from("com", "LeafSync", "LeafSync")
        .ok_or_else(|| anyhow::anyhow!("could not determine state directory"))?;
    let dir = proj.data_dir().to_path_buf();
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Load persisted TLS cert/key, or generate and persist new ones.
pub fn load_or_generate_cert() -> Result<(Certificate, PrivateKey, Vec<u8>)> {
    let dir = state_dir()?;
    let cert_path = dir.join("server_cert.der");
    let key_path = dir.join("server_key.der");

    if cert_path.exists() && key_path.exists() {
        let cert_der = fs::read(&cert_path).with_context(|| format!("read {:?}", cert_path))?;
        let key_der = fs::read(&key_path).with_context(|| format!("read {:?}", key_path))?;
        return Ok((Certificate(cert_der.clone()), PrivateKey(key_der), cert_der));
    }

    // Generate a new self-signed cert
    let cert = generate_simple_self_signed(["localhost".into()])?;
    let cert_der = cert.serialize_der()?;
    let key_der = cert.serialize_private_key_der();

    fs::write(&cert_path, &cert_der).with_context(|| format!("write {:?}", cert_path))?;
    fs::write(&key_path, &key_der).with_context(|| format!("write {:?}", key_path))?;

    Ok((Certificate(cert_der.clone()), PrivateKey(key_der), cert_der))
}

/// Build a QUIC server config using the persisted certificate.
pub fn make_server_config() -> Result<(quinn::ServerConfig, Vec<u8>)> {
    let (cert, key, cert_der) = load_or_generate_cert()?;
    let mut server_config = quinn::ServerConfig::with_single_cert(vec![cert], key)?;
    let mut transport = quinn::TransportConfig::default();
    transport.max_concurrent_bidi_streams(64u32.into());
    server_config.transport = Arc::new(transport);
    Ok((server_config, cert_der))
}

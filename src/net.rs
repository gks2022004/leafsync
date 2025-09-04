use anyhow::Result;
use quinn::{Endpoint, RecvStream, SendStream};
use rustls::{ClientConfig as RustlsClientConfig, RootCertStore};
use rustls::client::{ServerCertVerifier, ServerCertVerified};
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tokio::io::AsyncWriteExt;

use crate::{protocol::{Msg, FileSummary}, syncer, chunk::{chunk_file, read_chunk, ChunkInfo}};
use crate::identity;
use crate::trust;
use crate::resume;

fn normalize_rel(p: &str) -> String {
    let s = p.replace('\\', "/");
    let s = s.trim_start_matches('/');
    s.to_string()
}

pub async fn run_server(folder: PathBuf, port: u16) -> Result<()> {
    run_server_filtered(folder, port, None).await
}

pub async fn run_server_filtered(folder: PathBuf, port: u16, only_file: Option<String>) -> Result<()> {
    let (server_config, cert_der) = identity::make_server_config()?;
    let addr: SocketAddr = format!("0.0.0.0:{port}").parse().unwrap();
    let endpoint = Endpoint::server(server_config, addr)?;
    println!("Server cert SHA-256 fingerprint: {}", sha256_hex(&cert_der));
    if let Ok(dir) = identity::state_dir() { println!("Identity dir: {}", dir.display()); }
    println!("Listening on {addr}");

    while let Some(connecting) = endpoint.accept().await {
        let folder = folder.clone();
        let only_file = only_file.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection_server(folder, only_file, connecting).await {
                eprintln!("connection error: {e:?}");
            }
        });
    }
    Ok(())
}

async fn handle_connection_server(folder: PathBuf, only_file: Option<String>, conn: quinn::Connecting) -> Result<()> {
    let connection = conn.await?;
    println!("Peer connected: {}", connection.remote_address());
    let (mut send, mut recv) = connection.accept_bi().await?;
    println!("Opened bi stream with client");

    // version negotiation
    send_msg(&mut send, &Msg::Version { major: 1, minor: 0 }).await?;
    match recv_msg(&mut recv).await? {
        Some(Msg::Version { major, .. }) if major == 1 => {}
        other => { println!("Version mismatch or missing: {:?}", other); return Ok(()); }
    }
    // handshake: receive Hello
    if let Some(msg) = recv_msg(&mut recv).await? {
        if let Msg::Hello { folder: _ } = msg {
            println!("Received Hello");
            // send summary
            let summaries = syncer::all_summaries(&folder)?;
            let filter_norm: Option<String> = only_file.as_ref().map(|s| normalize_rel(s));
            let files: Vec<FileSummary> = summaries
                .iter()
                .map(|(s, _)| s.clone())
                .filter(|fs| match &filter_norm { Some(f) => normalize_rel(&fs.rel_path) == *f, None => true })
                .collect();
            send_msg(&mut send, &Msg::Summary { files }).await?;
            // Then serve per-file requests and accept client-initiated pushes
            loop {
        match recv_msg(&mut recv).await? {
                    Some(Msg::RequestFile { rel_path }) => {
            println!("RequestFile: {}", rel_path);
                        // honor filter: if requested file doesn't match, ignore
                        if let Some(ref f) = filter_norm { if normalize_rel(&rel_path) != *f { let _ = send_msg(&mut send, &Msg::Done).await; continue; } }
                        let abs = folder.join(&rel_path);
                        let chunks = if abs.exists() { chunk_file(&abs)? } else { Vec::new() };
                        let size = std::fs::metadata(&abs).map(|m| m.len()).unwrap_or(0);
                        let chunk_hashes: Vec<[u8; 32]> = chunks.iter().map(|c| c.hash).collect();
                        send_msg(&mut send, &Msg::FileMeta { rel_path: rel_path.clone(), size, chunk_count: chunks.len() as u64, root: merkle_root_from_chunks(&chunks), chunk_hashes }).await?;
                    }
                    Some(Msg::FileMeta { rel_path, size, chunk_count, root, chunk_hashes }) => {
                        println!("Incoming push offer for {} ({} chunks)", rel_path, chunk_count);
                        // honor filter: if not the allowed file, immediately respond Done
                        if let Some(ref f) = filter_norm { if normalize_rel(&rel_path) != *f { let _ = send_msg(&mut send, &Msg::Done).await; continue; } }
                        let abs = folder.join(&rel_path);
                        let local_chunks: Vec<ChunkInfo> = if abs.exists() { chunk_file(&abs)? } else { Vec::new() };
                        let need = crate::syncer::diff_needed_indices(&local_chunks, &chunk_hashes);
                        // Request needed chunks from client
                        send_msg(&mut send, &Msg::RequestChunks { rel_path: rel_path.clone(), indices: need.clone() }).await?;
                        // Receive the chunk data then Done
                        let mut received: Vec<u64> = Vec::new();
                        loop {
                            match recv_msg(&mut recv).await? {
                                Some(Msg::ChunkData { rel_path: rp, index, data }) if rp == rel_path => {
                                    let _ = crate::syncer::apply_chunk_staging(&folder, &rel_path, index, &data)?;
                                    received.push(index);
                                }
                                Some(Msg::Done) => {
                                    // finalize
                                    let _ = crate::syncer::truncate_staging_to_size(&folder, &rel_path, size);
                                    // verify and finalize
                                    let staged = crate::syncer::staging_path(&folder, &rel_path);
                                    let mut ok = false;
                                    if let Ok(chunks_now) = crate::chunk::chunk_file(&staged) {
                                        let tree = crate::merkle::build_merkle(&chunks_now);
                                        let root_now = crate::merkle::root_hash(&tree);
                                        if root_now == root { ok = true; }
                                    }
                                    if ok { let _ = crate::syncer::finalize_staging(&folder, &rel_path); }
                                    else { println!("Push verify failed for {}", rel_path); }
                                    break;
                                }
                                other => { if other.is_none() { break; } }
                            }
                        }
                    }
                    Some(Msg::RequestChunks { rel_path, indices }) => {
            println!("RequestChunks: {} ({} indices)", rel_path, indices.len());
                        let abs = folder.join(&rel_path);
                        for idx in indices {
                            let data = read_chunk(&abs, idx).unwrap_or_default();
                            send_msg(&mut send, &Msg::ChunkData { rel_path: rel_path.clone(), index: idx, data }).await?;
                        }
                        send_msg(&mut send, &Msg::Done).await?;
            println!("Sent Done for {}", rel_path);
                    }
                    Some(Msg::Done) => break,
                    None => break,
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

pub async fn run_client(addr: String, folder: PathBuf, accept_first: bool, fingerprint: Option<String>) -> Result<()> {
    run_client_filtered(addr, folder, accept_first, fingerprint, None).await
}

pub async fn run_client_filtered(addr: String, folder: PathBuf, accept_first: bool, fingerprint: Option<String>, only_file: Option<String>) -> Result<()> {
    let server_addr: SocketAddr = addr.parse()?;
    // Determine expected fingerprint from CLI or trust store
    let expected = if let Some(fp) = fingerprint { Some(fp) } else { trust::get(&addr)? };
    let client_cfg = make_client_config_pinned(addr.clone(), expected, accept_first)?;
    let mut endpoint = Endpoint::client("0.0.0.0:0".parse().unwrap())?;
    endpoint.set_default_client_config(client_cfg);

    let connection = endpoint.connect(server_addr, "localhost")?.await?;
    let (mut send, mut recv) = connection.open_bi().await?;
    println!("Opened bi stream to server");
    crate::status::set_active(true).await;

    // version negotiation: send first, then expect server's version
    send_msg(&mut send, &Msg::Version { major: 1, minor: 0 }).await?;
    match recv_msg(&mut recv).await? {
        Some(Msg::Version { major, .. }) if major == 1 => {}
        other => { println!("Expected Version from server, got {:?}", other); return Ok(()); }
    }

    // hello + get summary
    send_msg(&mut send, &Msg::Hello { folder: folder.to_string_lossy().to_string() }).await?;
    let summary = match recv_msg(&mut recv).await? { Some(Msg::Summary { files }) => files, other => { println!("Expected Summary, got {:?}", other); vec![] } };
    println!("Server reported {} files", summary.len());

    // Normalize filter for comparison
    let filter_norm: Option<String> = only_file.map(|s| normalize_rel(&s));

    // for each remote file, compare and request missing
    for remote in summary {
        if let Some(ref f) = filter_norm { if &normalize_rel(&remote.rel_path) != f { continue; } }
        println!("Syncing {} ({} chunks)", remote.rel_path, remote.chunk_count);
    crate::status::start_file(&remote.rel_path, remote.size).await;
    send_msg(&mut send, &Msg::RequestFile { rel_path: remote.rel_path.clone() }).await?;
        let meta = match recv_msg(&mut recv).await? {
            Some(Msg::FileMeta { rel_path, size, chunk_count, root, chunk_hashes }) => (rel_path, size, chunk_count, root, chunk_hashes),
            _ => continue,
        };

        // compute local chunk hashes
    let abs_local = folder.join(&meta.0);
        let local_chunks: Vec<ChunkInfo> = if abs_local.exists() { chunk_file(&abs_local)? } else { Vec::new() };
    let need_base = crate::syncer::diff_needed_indices(&local_chunks, &meta.4);
    // Merge with resume store missing list if present
    let need = if let Some(mut missing) = resume::missing_indices_for(&addr, &meta.0, meta.1, meta.2, meta.3)? {
        // intersect resume missing with base diff (only request what differs)
        missing.retain(|i| need_base.contains(i));
        if missing.is_empty() { need_base } else { missing }
    } else { need_base };
    if need.is_empty() { println!("Up to date: {}", meta.0); continue; }

        println!("Requesting {} chunks for {}", need.len(), meta.0);
        send_msg(&mut send, &Msg::RequestChunks { rel_path: meta.0.clone(), indices: need.clone() }).await?;
        // receive chunks until Done
        let mut received_indices: Vec<u64> = Vec::new();
    let mut bytes_received: u64 = 0;
        loop {
            match recv_msg(&mut recv).await? {
                Some(Msg::ChunkData { rel_path, index, data }) => {
                    // Write into staging area for atomic finalize
                    let _ = crate::syncer::apply_chunk_staging(&folder, &rel_path, index, &data)?;
                    received_indices.push(index);
            bytes_received = bytes_received.saturating_add(data.len() as u64);
            crate::status::progress(bytes_received).await;
            print!(". ");
                    let _ = std::io::Write::flush(&mut std::io::stdout());
                }
                Some(Msg::Done) => {
                    if !received_indices.is_empty() {
                        let _ = resume::upsert_mark_many(&addr, &meta.0, meta.1, meta.2, meta.3, &received_indices);
                        received_indices.clear();
                    }
                    // Ensure staged size matches expected
                    let _ = crate::syncer::truncate_staging_to_size(&folder, &meta.0, meta.1);
                    // Verify Merkle root of staged file before finalizing
                    let staged = crate::syncer::staging_path(&folder, &meta.0);
                    let mut ok = false;
                    if let Ok(chunks_now) = crate::chunk::chunk_file(&staged) {
                        let tree = crate::merkle::build_merkle(&chunks_now);
                        let root_now = crate::merkle::root_hash(&tree);
                        if root_now == meta.3 { ok = true; }
                    }
                    if ok {
                        let _ = crate::syncer::finalize_staging(&folder, &meta.0);
                        // clear resume entry on completion
                        let _ = resume::clear(&addr, &meta.0, meta.3);
                        crate::status::file_done(true, "finalized").await;
                    } else {
                        println!("Warning: Merkle root mismatch for {}. Kept staged file; will not finalize.", meta.0);
                        crate::status::file_done(false, "merkle_mismatch").await;
                    }
                    println!("\nDone.");
                    break;
                }
                None => break,
                _ => {}
            }
        }
    }

    // Push phase: offer local files to server so it can request missing chunks
    let locals = syncer::all_summaries(&folder)?;
    for (sum, chunks) in locals {
        if let Some(ref f) = filter_norm { if &normalize_rel(&sum.rel_path) != f { continue; } }
        // announce local file
        let chunk_hashes: Vec<[u8;32]> = chunks.iter().map(|c| c.hash).collect();
        send_msg(&mut send, &Msg::FileMeta { rel_path: sum.rel_path.clone(), size: sum.size, chunk_count: sum.chunk_count, root: sum.root, chunk_hashes }).await?;
        // wait either for RequestChunks or Done/next
        let mut to_send: Option<Vec<u64>> = None;
        match recv_msg(&mut recv).await? {
            Some(Msg::RequestChunks { rel_path, indices }) if rel_path == sum.rel_path => {
                to_send = Some(indices);
            }
            Some(Msg::Done) | None => {}
            _ => {}
        }
        if let Some(indices) = to_send {
            let abs = folder.join(&sum.rel_path);
            for idx in indices {
                let data = read_chunk(&abs, idx).unwrap_or_default();
                send_msg(&mut send, &Msg::ChunkData { rel_path: sum.rel_path.clone(), index: idx, data }).await?;
            }
            send_msg(&mut send, &Msg::Done).await?;
        }
    }

    // signal done
    let _ = send_msg(&mut send, &Msg::Done).await;
    crate::status::session_done(true, "client_done").await;
    Ok(())
}

fn merkle_root_from_chunks(chunks: &[ChunkInfo]) -> [u8; 32] {
    let tree = crate::merkle::build_merkle(chunks);
    crate::merkle::root_hash(&tree)
}

async fn send_msg(send: &mut SendStream, msg: &Msg) -> Result<()> {
    let bytes = crate::protocol::encode(msg);
    let len = (bytes.len() as u32).to_be_bytes();
    send.write_all(&len).await?;
    send.write_all(&bytes).await?;
    send.flush().await?;
    Ok(())
}

async fn recv_msg(recv: &mut RecvStream) -> Result<Option<Msg>> {
    let mut len_buf = [0u8; 4];
    if recv.read_exact(&mut len_buf).await.is_err() { return Ok(None); }
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    if recv.read_exact(&mut buf).await.is_err() { return Ok(None); }
    Ok(Some(crate::protocol::decode(&buf)))
}

fn make_client_config_pinned(addr: String, expected: Option<String>, accept_first: bool) -> Result<quinn::ClientConfig> {
    let roots = RootCertStore::empty();
    let mut crypto = RustlsClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(roots)
        .with_no_client_auth();
    crypto.dangerous().set_certificate_verifier(Arc::new(PinVerifier { addr, expected, accept_first }));
    let crypto = Arc::new(crypto);
    Ok(quinn::ClientConfig::new(crypto))
}

struct PinVerifier {
    addr: String,
    expected: Option<String>,
    accept_first: bool,
}

impl ServerCertVerifier for PinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::Certificate,
        _intermediates: &[rustls::Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item=&[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let fp = trust::sha256_hex(&end_entity.0);
        match self.expected.as_ref() {
            Some(exp) => {
                if exp.eq_ignore_ascii_case(&fp) {
                    return Ok(ServerCertVerified::assertion());
                }
                return Err(rustls::Error::General(format!("fingerprint mismatch: expected {}, got {}", exp, fp)));
            }
            None => {
                if self.accept_first {
                    // Try to persist; ignore errors here, still accept
                    let _ = trust::set(&self.addr, &fp);
                    return Ok(ServerCertVerified::assertion());
                }
                return Err(rustls::Error::General(format!(
                    "untrusted server {} with fingerprint {}. Re-run with --accept-first or --fingerprint {}",
                    self.addr, fp, fp
                )));
            }
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    let out = h.finalize();
    out.iter().map(|b| format!("{:02x}", b)).collect()
}

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

#[allow(dead_code)]
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
    // Accept streams forever; each stream can be a control stream (Version/Hello) or a chunk/push stream.
    loop {
        match connection.accept_bi().await {
            Ok((mut send, mut recv)) => {
                let folder_c = folder.clone();
                let only_c = only_file.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_server_stream(folder_c, only_c, &mut send, &mut recv).await {
                        eprintln!("stream error: {:?}", e);
                    }
                });
            }
            Err(quinn::ConnectionError::ApplicationClosed { .. }) | Err(quinn::ConnectionError::LocallyClosed) => break,
            Err(e) => { eprintln!("accept_bi error: {:?}", e); break; }
        }
    }
    Ok(())
}

async fn handle_server_stream(folder: PathBuf, only_file: Option<String>, send: &mut SendStream, recv: &mut RecvStream) -> Result<()> {
    // Try to read first message and branch
    if let Some(first) = recv_msg(recv).await? {
        match first {
            Msg::Version { major, .. } => {
                if major != 1 { return Ok(()); }
                // respond with our version
                send_msg(send, &Msg::Version { major: 1, minor: 0 }).await?;
                // expect Hello
                if let Some(Msg::Hello { folder: _ }) = recv_msg(recv).await? {
                    let summaries = syncer::all_summaries(&folder)?;
                    let filter_norm: Option<String> = only_file.as_ref().map(|s| normalize_rel(s));
                    let files: Vec<FileSummary> = summaries
                        .iter()
                        .map(|(s, _)| s.clone())
                        .filter(|fs| match &filter_norm { Some(f) => normalize_rel(&fs.rel_path) == *f, None => true })
                        .collect();
                    send_msg(send, &Msg::Summary { files }).await?;
                    // control loop for this stream
                    loop {
                        match recv_msg(recv).await? {
                            Some(Msg::RequestFile { rel_path }) => {
                                let filter_norm: Option<String> = only_file.as_ref().map(|s| normalize_rel(s));
                                if let Some(ref f) = filter_norm { if normalize_rel(&rel_path) != *f { let _ = send_msg(send, &Msg::Done).await; continue; } }
                                let abs = folder.join(&rel_path);
                                let chunks = if abs.exists() { chunk_file(&abs)? } else { Vec::new() };
                                let size = std::fs::metadata(&abs).map(|m| m.len()).unwrap_or(0);
                                let chunk_hashes: Vec<[u8; 32]> = chunks.iter().map(|c| c.hash).collect();
                                send_msg(send, &Msg::FileMeta { rel_path: rel_path.clone(), size, chunk_count: chunks.len() as u64, root: merkle_root_from_chunks(&chunks), chunk_hashes }).await?;
                            }
                            Some(Msg::FileMeta { rel_path, size, chunk_count, root, chunk_hashes }) => {
                                let filter_norm: Option<String> = only_file.as_ref().map(|s| normalize_rel(s));
                                if let Some(ref f) = filter_norm { if normalize_rel(&rel_path) != *f { let _ = send_msg(send, &Msg::Done).await; continue; } }
                                let abs = folder.join(&rel_path);
                                let local_chunks: Vec<ChunkInfo> = if abs.exists() { chunk_file(&abs)? } else { Vec::new() };
                                let need = crate::syncer::diff_needed_indices(&local_chunks, &chunk_hashes);
                                send_msg(send, &Msg::RequestChunks { rel_path: rel_path.clone(), indices: need.clone() }).await?;
                                // Receive the chunk data then Done
                                loop {
                                    match recv_msg(recv).await? {
                                        Some(Msg::ChunkData { rel_path: rp, index, data }) if rp == rel_path => {
                                            let _ = crate::syncer::apply_chunk_staging(&folder, &rel_path, index, &data)?;
                                        }
                                        Some(Msg::Done) => {
                                            let _ = crate::syncer::truncate_staging_to_size(&folder, &rel_path, size);
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
                                let abs = folder.join(&rel_path);
                                for idx in indices {
                                    let data = read_chunk(&abs, idx).unwrap_or_default();
                                    send_msg(send, &Msg::ChunkData { rel_path: rel_path.clone(), index: idx, data }).await?;
                                }
                                send_msg(send, &Msg::Done).await?;
                            }
                            Some(Msg::Done) | None => break,
                            _ => {}
                        }
                    }
                }
            }
            Msg::RequestChunks { rel_path, indices } => {
                // Chunk-only stream
                let abs = folder.join(&rel_path);
                for idx in indices {
                    let data = read_chunk(&abs, idx).unwrap_or_default();
                    send_msg(send, &Msg::ChunkData { rel_path: rel_path.clone(), index: idx, data }).await?;
                }
                send_msg(send, &Msg::Done).await?;
            }
            // For completeness, allow RequestFile/FileMeta without Version on a dedicated stream
            Msg::RequestFile { rel_path } => {
                let filter_norm: Option<String> = only_file.as_ref().map(|s| normalize_rel(s));
                if let Some(ref f) = filter_norm { if normalize_rel(&rel_path) != *f { let _ = send_msg(send, &Msg::Done).await; return Ok(()); } }
                let abs = folder.join(&rel_path);
                let chunks = if abs.exists() { chunk_file(&abs)? } else { Vec::new() };
                let size = std::fs::metadata(&abs).map(|m| m.len()).unwrap_or(0);
                let chunk_hashes: Vec<[u8; 32]> = chunks.iter().map(|c| c.hash).collect();
                send_msg(send, &Msg::FileMeta { rel_path: rel_path.clone(), size, chunk_count: chunks.len() as u64, root: merkle_root_from_chunks(&chunks), chunk_hashes }).await?;
            }
            other => {
                // ignore or unhandled on non-control stream
                eprintln!("unexpected first message on stream: {:?}", other);
            }
        }
    }
    Ok(())
}

#[allow(dead_code)]
pub async fn run_client(addr: String, folder: PathBuf, accept_first: bool, fingerprint: Option<String>) -> Result<()> {
    run_client_filtered(addr, folder, accept_first, fingerprint, None, false, 4, None).await
}

pub async fn run_client_filtered(addr: String, folder: PathBuf, accept_first: bool, fingerprint: Option<String>, only_file: Option<String>, mirror: bool, streams: usize, rate_mbps: Option<f64>) -> Result<()> {
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
    
    // Helper: skip internal/ignored patterns for deletion
    fn is_ignored_rel(rel: &str) -> bool {
        let r = rel.replace('\\', "/");
        r.starts_with(".leafsync_tmp/") || r.contains("/.git/") || r.ends_with(".part") || r.contains("/~$")
    }
    
    // Optional mirror: remove local files missing on server (move to trash)
    if mirror {
        use std::collections::HashSet;
        let remote_set: HashSet<String> = summary.iter()
            .map(|f| normalize_rel(&f.rel_path))
            .collect();
        let locals = crate::syncer::all_summaries(&folder)?;
        let local_set: HashSet<String> = locals.iter()
            .map(|(s, _)| normalize_rel(&s.rel_path))
            .collect();
        for rel in local_set.difference(&remote_set) {
            if let Some(ref f) = filter_norm { if rel != f { continue; } }
            if is_ignored_rel(rel) { continue; }
            // move to .leafsync_trash with timestamped root
            if let Err(e) = move_to_trash(&folder, rel) { eprintln!("mirror trash failed for {}: {:?}", rel, e); }
            else { println!("Mirrored delete (moved to trash): {}", rel); }
        }
    }

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

        println!("Requesting {} chunks for {} using {} streams", need.len(), meta.0, streams);
        // Shared progress
        use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
        let bytes_received = Arc::new(AtomicU64::new(0));
        let received_indices = Arc::new(tokio::sync::Mutex::new(Vec::<u64>::new()));
        // Optional rate limiter shared across streams
        let rate = rate_mbps.map(|mb| (mb * 1024.0 * 1024.0) as u64);
        let limiter = Arc::new(tokio::sync::Mutex::new(RateLimiter::new(rate)));

        // Partition indices across streams
        let n_streams = streams.max(1).min(16);
        let mut parts: Vec<Vec<u64>> = vec![Vec::new(); n_streams];
        for (i, idx) in need.iter().cloned().enumerate() { parts[i % n_streams].push(idx); }
        // Spawn tasks
        let mut tasks = Vec::new();
        for part in parts.into_iter().filter(|p| !p.is_empty()) {
            let connection_c = connection.clone();
            let rel = meta.0.clone();
            let folder_c = folder.clone();
            let bytes_c = bytes_received.clone();
            let recv_idx = received_indices.clone();
            let limiter_c = limiter.clone();
            tasks.push(tokio::spawn(async move {
                if let Ok((mut s, mut r)) = connection_c.open_bi().await {
                    // request these indices
                    let _ = send_msg(&mut s, &Msg::RequestChunks { rel_path: rel.clone(), indices: part.clone() }).await;
                    loop {
                        match recv_msg(&mut r).await {
                            Ok(Some(Msg::ChunkData { rel_path: rp, index, data })) if rp == rel => {
                                // rate limit
                                if let Some(_) = limiter_c.lock().await.consume(data.len() as u64).await {}
                                let _ = crate::syncer::apply_chunk_staging(&folder_c, &rel, index, &data);
                                recv_idx.lock().await.push(index);
                                let new = bytes_c.fetch_add(data.len() as u64, AtomicOrdering::SeqCst) + data.len() as u64;
                                crate::status::progress(new).await;
                            }
                            Ok(Some(Msg::Done)) => break,
                            Ok(None) | Err(_) => break,
                            _ => {}
                        }
                    }
                }
            }));
        }
        for t in tasks { let _ = t.await; }
        // Upsert resume for all received indices
        let all_recv = received_indices.lock().await.clone();
        if !all_recv.is_empty() { let _ = resume::upsert_mark_many(&addr, &meta.0, meta.1, meta.2, meta.3, &all_recv); }
        // Ensure staged size and finalize
        let _ = crate::syncer::truncate_staging_to_size(&folder, &meta.0, meta.1);
        let staged = crate::syncer::staging_path(&folder, &meta.0);
        let mut ok = false;
        if let Ok(chunks_now) = crate::chunk::chunk_file(&staged) {
            let tree = crate::merkle::build_merkle(&chunks_now);
            let root_now = crate::merkle::root_hash(&tree);
            if root_now == meta.3 { ok = true; }
        }
        if ok {
            let _ = crate::syncer::finalize_staging(&folder, &meta.0);
            let _ = resume::clear(&addr, &meta.0, meta.3);
            crate::status::file_done(true, "finalized").await;
        } else {
            println!("Warning: Merkle root mismatch for {}. Kept staged file; will not finalize.", meta.0);
            crate::status::file_done(false, "merkle_mismatch").await;
        }
        println!("\nDone.");
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

fn move_to_trash(root: &std::path::Path, rel: &str) -> Result<()> {
    use chrono::Local;
    let ts = Local::now().format("%Y%m%d_%H%M%S").to_string();
    let base = root.join(".leafsync_trash").join(ts);
    let from = root.join(rel);
    let to = base.join(rel);
    if let Some(p) = to.parent() { std::fs::create_dir_all(p)?; }
    if from.exists() {
        std::fs::rename(&from, &to)?;
    }
    Ok(())
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

struct RateLimiter {
    bytes_per_sec: Option<u64>,
    available: f64,
    last: std::time::Instant,
}

impl RateLimiter {
    fn new(bytes_per_sec: Option<u64>) -> Self {
        Self { bytes_per_sec, available: bytes_per_sec.unwrap_or(u64::MAX) as f64, last: std::time::Instant::now() }
    }
    async fn consume(&mut self, n: u64) -> Option<()> {
        if let Some(bps) = self.bytes_per_sec {
            // Refill
            let now = std::time::Instant::now();
            let dt = now.duration_since(self.last).as_secs_f64();
            self.last = now;
            self.available = (self.available + dt * (bps as f64)).min(bps as f64);
            let need = n as f64;
            if self.available >= need {
                self.available -= need;
                return None;
            }
            let deficit = need - self.available;
            let sleep_secs = deficit / (bps as f64);
            let dur = std::time::Duration::from_secs_f64(sleep_secs.max(0.0));
            self.available = 0.0;
            tokio::time::sleep(dur).await;
            return None;
        }
        None
    }
}

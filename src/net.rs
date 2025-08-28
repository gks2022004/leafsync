use anyhow::{Result};
use quinn::{Endpoint, ServerConfig, RecvStream, SendStream};
use rcgen::generate_simple_self_signed;
use rustls::{Certificate, ClientConfig as RustlsClientConfig, RootCertStore, PrivateKey};
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tokio::io::AsyncWriteExt;

use crate::{protocol::{Msg, FileSummary}, syncer, chunk::{chunk_file, read_chunk, ChunkInfo}};

pub async fn run_server(folder: PathBuf, port: u16) -> Result<()> {
    let (server_config, cert_der) = make_server_config()?;
    let addr: SocketAddr = format!("0.0.0.0:{port}").parse().unwrap();
    let endpoint = Endpoint::server(server_config, addr)?;
    println!("Server cert SHA-256 fingerprint: {}", sha256_hex(&cert_der));
    println!("Listening on {addr}");

    while let Some(connecting) = endpoint.accept().await {
        let folder = folder.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection_server(folder, connecting).await {
                eprintln!("connection error: {e:?}");
            }
        });
    }
    Ok(())
}

async fn handle_connection_server(folder: PathBuf, conn: quinn::Connecting) -> Result<()> {
    let connection = conn.await?;
    println!("Peer connected: {}", connection.remote_address());
    let (mut send, mut recv) = connection.accept_bi().await?;

    // handshake: receive Hello
    if let Some(msg) = recv_msg(&mut recv).await? {
        if let Msg::Hello { folder: _ } = msg {
            // send summary
            let summaries = syncer::all_summaries(&folder)?;
            let files: Vec<FileSummary> = summaries.iter().map(|(s, _)| s.clone()).collect();
            send_msg(&mut send, &Msg::Summary { files }).await?;

            // Then serve per-file requests
            loop {
                match recv_msg(&mut recv).await? {
                    Some(Msg::RequestFile { rel_path }) => {
                        let abs = folder.join(&rel_path);
                        let chunks = if abs.exists() { chunk_file(&abs)? } else { Vec::new() };
                        let chunk_hashes: Vec<[u8; 32]> = chunks.iter().map(|c| c.hash).collect();
                        send_msg(&mut send, &Msg::FileMeta { rel_path: rel_path.clone(), chunk_count: chunks.len() as u64, root: merkle_root_from_chunks(&chunks), chunk_hashes }).await?;
                    }
                    Some(Msg::RequestChunks { rel_path, indices }) => {
                        let abs = folder.join(&rel_path);
                        for idx in indices {
                            let data = read_chunk(&abs, idx).unwrap_or_default();
                            send_msg(&mut send, &Msg::ChunkData { rel_path: rel_path.clone(), index: idx, data }).await?;
                        }
                        send_msg(&mut send, &Msg::Done).await?;
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

pub async fn run_client(addr: String, folder: PathBuf) -> Result<()> {
    let server_addr: SocketAddr = addr.parse()?;
    let client_cfg = make_client_config_insecure()?; // trust any cert for prototype
    let mut endpoint = Endpoint::client("0.0.0.0:0".parse().unwrap())?;
    endpoint.set_default_client_config(client_cfg);

    let connection = endpoint.connect(server_addr, "localhost")?.await?;
    let (mut send, mut recv) = connection.open_bi().await?;

    // hello + get summary
    send_msg(&mut send, &Msg::Hello { folder: folder.to_string_lossy().to_string() }).await?;
    let summary = match recv_msg(&mut recv).await? { Some(Msg::Summary { files }) => files, _ => vec![] };

    // for each remote file, compare and request missing
    for remote in summary {
        println!("Syncing {} ({} chunks)", remote.rel_path, remote.chunk_count);
        send_msg(&mut send, &Msg::RequestFile { rel_path: remote.rel_path.clone() }).await?;
        let meta = match recv_msg(&mut recv).await? {
            Some(Msg::FileMeta { rel_path, chunk_count: _, root: _, chunk_hashes }) => (rel_path, chunk_hashes),
            _ => continue,
        };

        // compute local chunk hashes
        let abs_local = folder.join(&meta.0);
        let local_chunks: Vec<ChunkInfo> = if abs_local.exists() { chunk_file(&abs_local)? } else { Vec::new() };
        let need = crate::syncer::diff_needed_indices(&local_chunks, &meta.1);
        if need.is_empty() { println!("Up to date: {}", meta.0); continue; }

        println!("Requesting {} chunks for {}", need.len(), meta.0);
        send_msg(&mut send, &Msg::RequestChunks { rel_path: meta.0.clone(), indices: need.clone() }).await?;
        // receive chunks until Done
        loop {
            match recv_msg(&mut recv).await? {
                Some(Msg::ChunkData { rel_path, index, data }) => {
                    crate::syncer::apply_chunk(&folder, &rel_path, index, &data)?;
                    print!(". ");
                    let _ = std::io::Write::flush(&mut std::io::stdout());
                }
                Some(Msg::Done) => { println!("\nDone."); break; }
                None => break,
                _ => {}
            }
        }
    }

    // signal done
    let _ = send_msg(&mut send, &Msg::Done).await;
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

fn make_server_config() -> Result<(ServerConfig, Vec<u8>)> {
    let cert = generate_simple_self_signed(["localhost".into()])?;
    let cert_der = cert.serialize_der()?;
    let key_der = cert.serialize_private_key_der();

    let chain = vec![Certificate(cert_der.clone())];
    let key = PrivateKey(key_der);
    let mut server_config = quinn::ServerConfig::with_single_cert(chain, key)?;
    let mut transport = quinn::TransportConfig::default();
    transport.max_concurrent_bidi_streams(64u32.into());
    server_config.transport = Arc::new(transport);
    Ok((server_config, cert_der))
}

fn make_client_config_insecure() -> Result<quinn::ClientConfig> {
    // For prototype: accept any certificate (DANGEROUS).
    let roots = RootCertStore::empty();
    let mut crypto = RustlsClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(roots)
        .with_no_client_auth();
    crypto.dangerous().set_certificate_verifier(Arc::new(NoVerifier));
    let crypto = Arc::new(crypto);
    Ok(quinn::ClientConfig::new(crypto))
}

struct NoVerifier;
impl rustls::client::ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::Certificate,
        _intermediates: &[rustls::Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item=&[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> Result<rustls::client::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::ServerCertVerified::assertion())
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    let out = h.finalize();
    out.iter().map(|b| format!("{:02x}", b)).collect()
}

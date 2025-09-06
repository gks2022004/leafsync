mod net;
mod chunk;
mod merkle;
mod protocol;
mod syncer;
mod identity;
mod trust;
mod resume;
mod web;
mod status;
// mod watch; // removed watch mode; continuous sync handled by Connect

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "leafsync", version, about = "P2P QUIC file sync with Merkle delta", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start a QUIC server and serve a folder
    Serve {
        folder: PathBuf,
        #[arg(long, default_value_t = 4455)]
        port: u16,
        /// Sync only a specific file (relative to folder)
        #[arg(long)]
        file: Option<String>,
    },
    /// Connect to a peer and sync a folder
    Connect {
        addr: String,
        folder: PathBuf,
    /// Accept and pin the server fingerprint on first connect without prompting (dev only)
    #[arg(long)]
    accept_first: bool,
    /// Provide a known fingerprint (hex) to pin on this connect
    #[arg(long)]
    fingerprint: Option<String>,
    /// Sync only a specific file (relative to folder)
    #[arg(long)]
    file: Option<String>,
    /// Mirror deletes (move local-only files into .leafsync_trash)
    #[arg(long)]
    mirror: bool,
    /// Number of concurrent download streams (1-16)
    #[arg(long, default_value_t = 4)]
    streams: usize,
    /// Rate limit in Mbps (omit for unlimited)
    #[arg(long)]
    rate_mbps: Option<f64>,
    },
    /// Manage trusted server fingerprints (TOFU)
    #[command(subcommand)]
    Trust(TrustCmd),
    /// Launch local web UI
    Ui { #[arg(long, default_value_t = 8080)] port: u16 },
}

#[derive(Subcommand, Debug)]
enum TrustCmd {
    /// List all pinned servers
    List,
    /// Add a pinned fingerprint for an address
    Add { addr: String, fingerprint: String },
    /// Remove a pinned server by address
    Remove { addr: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Serve { folder, port, file } => {
            println!("LeafSync server starting on 0.0.0.0:{port}");
            net::run_server_filtered(folder, port, file).await?;
        }
        Commands::Connect { addr, folder, accept_first, fingerprint, file, mirror, streams, rate_mbps } => {
            println!("LeafSync connecting to {addr}");
            net::run_client_filtered(addr, folder, accept_first, fingerprint, file, mirror, streams, rate_mbps).await?;
        }
        Commands::Trust(cmd) => {
            match cmd {
                TrustCmd::List => {
                    let store = trust::load().unwrap_or_default();
                    if store.servers.is_empty() {
                        println!("No trusted servers.");
                    } else {
                        for (addr, fp) in store.servers { println!("{}  {}", addr, fp); }
                    }
                }
                TrustCmd::Add { addr, fingerprint } => {
                    trust::set(&addr, &fingerprint)?;
                    println!("Pinned {} => {}", addr, fingerprint);
                }
                TrustCmd::Remove { addr } => {
                    let mut store = trust::load().unwrap_or_default();
                    if store.servers.remove(&addr).is_some() {
                        trust::save(&store)?;
                        println!("Removed {}", addr);
                    } else {
                        println!("{} was not pinned", addr);
                    }
                }
            }
        }
        Commands::Ui { port } => {
            println!("Starting LeafSync web UI on http://127.0.0.1:{port}");
            web::run_ui(port).await?;
        }
    }
    Ok(())
}

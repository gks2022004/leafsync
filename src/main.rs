mod net;
mod chunk;
mod merkle;
mod protocol;
mod syncer;
mod identity;
mod trust;
mod resume;
mod web;

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
    },
    /// Manage trusted server fingerprints (TOFU)
    #[command(subcommand)]
    Trust(TrustCmd),
    /// Watch a folder and sync on changes (placeholder)
    Watch { folder: PathBuf },
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
        Commands::Serve { folder, port } => {
            println!("LeafSync server starting on 0.0.0.0:{port}");
            net::run_server(folder, port).await?;
        }
        Commands::Connect { addr, folder, accept_first, fingerprint } => {
            println!("LeafSync connecting to {addr}");
            net::run_client(addr, folder, accept_first, fingerprint).await?;
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
        Commands::Watch { folder } => {
            println!("Watch mode not implemented yet. Folder: {}", folder.display());
        }
        Commands::Ui { port } => {
            println!("Starting LeafSync web UI on http://127.0.0.1:{port}");
            web::run_ui(port).await?;
        }
    }
    Ok(())
}

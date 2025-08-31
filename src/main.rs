mod net;
mod chunk;
mod merkle;
mod protocol;
mod syncer;
mod identity;

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
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Serve { folder, port } => {
            println!("LeafSync server starting on 0.0.0.0:{port}");
            net::run_server(folder, port).await?;
        }
        Commands::Connect { addr, folder } => {
            println!("LeafSync connecting to {addr}");
            net::run_client(addr, folder).await?;
        }
    }
    Ok(())
}

# LeafSync

Peer-to-peer file sync over QUIC with Merkle-based delta transfers.

## v1 Prototype (this repo)
- QUIC transport using quinn
- Fixed-size chunking (default 1 MiB)
- Merkle tree file identity + delta sync
- Simple CLI:
  - `leafsync serve <folder>`
  - `leafsync connect <addr:port> <folder>`

## Quick start
1. Build:
   - Windows (PowerShell): `cargo build --release`
2. Run one peer as server (listener):
   - `target\release\leafsync serve .\shared`
3. Run the other peer as client (connector):
   - `target\release\leafsync connect 127.0.0.1:4455 .\shared`

By default the server listens on 0.0.0.0:4455 with a self-signed TLS cert generated on startup. Client will trust this ephemeral cert fingerprint printed at startup (development-only).

## Demo
1. Make some changes in test_server folder, ex: add a statement in readme.txt file and save it.
2. Terminal 1: start the server (serving test_server)
   ```
   cargo run -- serve .\test_server --port 4455

   ```
3. Terminal 2: run the client (sync into test_client)
   ```
   cargo run -- connect 127.0.0.1:4455 .\test_client

   ```
4. Results:
   ```
   LeafSync connecting to 127.0.0.1:4455
   Syncing hello.txt (1 chunks)
   Up to date: hello.txt
   Syncing new.txt (1 chunks)
   Up to date: new.txt
   Syncing readme.txt (1 chunks)
   Requesting 1 chunks for readme.txt
   . 
   Done.
   Syncing test.md (1 chunks)
   Up to date: test.md    
   ```  

## Notes
- For prototype simplicity, authentication uses an ephemeral self-signed cert. Do not use in production. v2 adds persistent identities.
- Delta sync walks Merkle subtrees and only requests missing chunks.

## License
MIT or Apache-2.0
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

## Notes
- For prototype simplicity, authentication uses an ephemeral self-signed cert. Do not use in production. v2 adds persistent identities.
- Delta sync walks Merkle subtrees and only requests missing chunks.

## License
MIT or Apache-2.0
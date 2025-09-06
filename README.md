# <img src="https://github.com/user-attachments/assets/77c7b4b1-9dce-4dde-99f7-593ef111b5b5" alt="leafsync" width="72" height="72" align="middle" /> **LeafSync**

Peer‑to‑peer file sync over QUIC with Merkle‑based delta transfers.

<img width="1272" height="863" alt="image" src="https://github.com/user-attachments/assets/67d903d2-4bd1-46c4-9a91-69090920d928" />

## What’s inside
- QUIC transport (quinn) + TLS (rustls), single UDP port
- Fixed‑size chunking (1 MiB) with Merkle trees for delta sync
- Atomic staging + verification before finalize (no partial/corrupt files)
- Resume partial transfers (chunk‑level bitmaps)
- TOFU trust pinning (accept‑first or pinned fingerprint)
- Watch mode with bidirectional behavior (pull periodically + push on local change)
- Web UI:
  - Folder/file picker with Windows quick links (Desktop/Downloads/Documents/Pictures/Music/Videos/Home)
  - “Select File” support in Serve, Connect, and Watch (single‑file sync)
  - Live status with per‑file progress bar and MB/s speed
  - Polished, consistent control sizing
  - Optional “Mirror deletes” for Connect/Watch (safe delete: move local‑only files to .leafsync_trash)

## Quick start (Web UI)
1) Launch the UI
```powershell
cargo run -- ui --port 8080
```
2) Open http://127.0.0.1:8080 and use the cards:
- Serve a folder
  - Pick a folder to expose. Optional: click “Browse…” and “Select File” to serve only one file.
- Connect to a peer
  - Enter peer IP:port and choose your local destination folder.
  - Optional: pick a specific file to sync (relative to the chosen folder).
  - First time: check “Accept first” to pin the server fingerprint automatically.
- Watch mode
  - Pick the local folder you work in and the peer IP:port.
  - Optional: pick a specific file to sync continuously.
  - Runs bidirectionally: periodic pulls plus push‑on‑change.

Notes
- The picker lets you browse directories and also select a file. When you click “Select File”, the folder field is set to the current directory and the file field is populated with a relative path.
- On Windows, the picker shows quick links (Desktop, Downloads, Documents, Pictures, Music, Videos, Home).

## CLI usage
The CLI supports single‑file sync and mirror deletes flags.

```powershell
# Start a server (listener)
cargo run -- serve .\shared --port 4455 [--file relative\\path\\to\\file]

# Connect to a server and sync (first time: trust on first use)
cargo run -- connect 127.0.0.1:4455 .\shared --accept-first [--fingerprint <hex>] [--file relative\\path\\to\\file] [--mirror]

# Watch a folder and sync on changes (bidirectional: also pulls periodically)
cargo run -- watch .\shared 127.0.0.1:4455 --accept-first [--fingerprint <hex>] [--file relative\\path\\to\\file] [--mirror]

# Manage trusted fingerprints (TOFU store)
cargo run -- trust list
cargo run -- trust add 127.0.0.1:4455 <hex-fingerprint>
cargo run -- trust remove 127.0.0.1:4455
```

Tips
- After the first successful connect, the fingerprint is pinned and reused.
- Allow UDP on your chosen port in Windows Firewall.
 - Mirror deletes is safe by design: instead of hard‑deleting, it moves local‑only files into a timestamped folder under .leafsync_trash so you can undo.

## How it works
1) Summary + diff
   - Server summarizes files; client requests per‑file metadata (chunk hashes).
   - Client diffs by chunk index and requests only missing/different chunks.
2) Transfer
   - Chunks stream over a QUIC bidirectional stream with length‑prefixed, bincode‑encoded messages.
3) Integrity + atomic finalize
   - Chunks write to a staging path; the Merkle root is recomputed and verified.
   - On success, the staged file is atomically renamed into place.
4) Resume
   - If interrupted, the next session requests only the missing chunk indices.
5) Watch
   - Debounced local changes trigger a sync; a periodic pull catches remote‑only edits.

## Why LeafSync vs “normal” protocols
- Sends only what changed
  - Merkle‑based, fixed‑size chunk diffs avoid re‑sending whole files; ideal for large binaries and VM images.
- Robust on flaky links
  - QUIC multiplexing avoids TCP head‑of‑line blocking; per‑chunk resume skips already‑received data after interruptions.
- Safe writes
  - Atomic staging + verification prevents corrupt or partially written files from replacing good ones.
- Low ceremony peer trust
  - TOFU pinning gives encrypted, authenticated connections without a CA or external service.
- Practical UX
  - Built‑in Web UI, real‑time progress/speed, and a native‑feeling picker with Windows quick folders.

## Performance characteristics & tips
- Bandwidth efficiency
  - Only changed chunks transfer; unchanged chunks are skipped entirely.
- Latency tolerance
  - QUIC avoids head‑of‑line blocking; a lost packet doesn’t stall the whole stream.
- Disk I/O
  - Chunk‑aligned writes to a staging file reduce random I/O on finalize.
- Resume at scale
  - Chunk bitmaps prevent re‑downloading already received data after a drop or restart.
- Single‑file mode
  - Targeting one file reduces metadata exchange and scanning overhead.
- Practical tuning
  - Run on wired/LAN or strong Wi‑Fi for peak throughput.
  - Exclude large caches/temp folders via .leafsyncignore to reduce scanning.

## Security
- Self‑signed TLS with certificate fingerprint pinning (TOFU).
- Use `--accept-first` only in trusted environments; fingerprints persist locally.
- No external servers or cloud dependency for LAN/Wi‑Fi usage.

## Troubleshooting
- Timeout on connect
  - Ensure the server is running and Windows Firewall allows UDP on the chosen port.
- File looks unchanged after shrink
  - Atomic finalize truncates staged files to the exact size before rename; re‑run connect if needed.
- Watch doesn’t fire
  - Confirm Watch is started on the correct local folder; .leafsyncignore may exclude the path.

## Roadmap
- Parallel transfers (multiple streams)
- mDNS peer discovery + UPnP mapping
- Mirror retention policy and history view; optional true delete
- Conflict detection/resolve UX
- Mobile apps (Android/iOS)


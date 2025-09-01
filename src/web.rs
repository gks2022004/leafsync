use axum::{routing::{get, post}, Router, extract::State, Json};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, path::PathBuf, sync::Arc};

#[derive(Clone)]
pub struct AppState {}

#[derive(Deserialize)]
struct ServeReq { folder: String, port: u16 }

#[derive(Deserialize)]
struct ConnectReq { addr: String, folder: String, accept_first: bool, fingerprint: Option<String> }

#[derive(Serialize)]
struct Resp { ok: bool, msg: String }

pub async fn run_ui(port: u16) -> anyhow::Result<()> {
    let state = AppState{};
    let app = Router::new()
        .route("/", get(index))
        .route("/api/serve", post(api_serve))
        .route("/api/connect", post(api_connect))
        .with_state(Arc::new(state));

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    println!("Web UI listening on http://{}", addr);
    axum::serve(tokio::net::TcpListener::bind(addr).await?, app).await?;
    Ok(())
}

async fn index() -> axum::response::Html<&'static str> {
    axum::response::Html(r#"<!doctype html>
<html>
<head><meta charset='utf-8'><title>LeafSync UI</title>
<style>body{font-family:system-ui;margin:2rem;max-width:900px} input,button{margin:.25rem;padding:.4rem}</style>
</head>
<body>
  <h1>LeafSync</h1>
  <section style="margin-bottom:1rem">
    <h3>Serve a folder</h3>
    <input id="serve-folder" placeholder="Folder path" size="50" />
    <input id="serve-port" placeholder="Port" value="4455" size="8" />
    <button onclick="serve()">Start Server</button>
    <div id="serve-out"></div>
  </section>
  <section>
    <h3>Connect to a peer</h3>
    <input id="connect-addr" placeholder="IP:port" size="20" />
    <input id="connect-folder" placeholder="Local folder" size="50" />
    <label><input type="checkbox" id="accept-first"/> Accept first</label>
    <input id="fingerprint" placeholder="Fingerprint (hex, optional)" size="70" />
    <button onclick="connectPeer()">Connect</button>
    <div id="connect-out"></div>
  </section>
<script>
async function serve(){
  const folder = document.getElementById('serve-folder').value;
  const port = parseInt(document.getElementById('serve-port').value||'4455');
  const r = await fetch('/api/serve',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({folder,port})});
  const j = await r.json();
  document.getElementById('serve-out').textContent = j.msg;
}
async function connectPeer(){
  const addr = document.getElementById('connect-addr').value;
  const folder = document.getElementById('connect-folder').value;
  const accept_first = document.getElementById('accept-first').checked;
  const fingerprint = document.getElementById('fingerprint').value || null;
  const r = await fetch('/api/connect',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({addr,folder,accept_first,fingerprint})});
  const j = await r.json();
  document.getElementById('connect-out').textContent = j.msg;
}
</script>
</body></html>"#)
}

async fn api_serve(State(_state): State<Arc<AppState>>, Json(req): Json<ServeReq>) -> Json<Resp> {
    let folder = PathBuf::from(req.folder);
    tokio::spawn(async move {
        if let Err(e) = crate::net::run_server(folder, req.port).await {
            eprintln!("server error: {e:?}");
        }
    });
    Json(Resp { ok: true, msg: format!("Server starting on 0.0.0.0:{}", req.port) })
}

async fn api_connect(State(_state): State<Arc<AppState>>, Json(req): Json<ConnectReq>) -> Json<Resp> {
    let folder = PathBuf::from(req.folder);
    tokio::spawn(async move {
        if let Err(e) = crate::net::run_client(req.addr, folder, req.accept_first, req.fingerprint).await {
            eprintln!("client error: {e:?}");
        }
    });
    Json(Resp { ok: true, msg: "Connect started".to_string() })
}

use axum::{routing::{get, post}, Router, extract::State, Json};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use chrono::Utc;

#[derive(Clone)]
pub struct AppState {
  watch: Arc<tokio::sync::Mutex<Option<crate::watch::WatchHandle>>>,
  status: Arc<tokio::sync::Mutex<SyncStatus>>,
}
type SyncStatus = crate::status::SyncStatus;

#[derive(Deserialize)]
struct ServeReq { folder: String, port: u16 }

#[derive(Deserialize)]
struct ConnectReq { addr: String, folder: String, accept_first: bool, fingerprint: Option<String> }

#[derive(Serialize)]
struct Resp { ok: bool, msg: String }

#[derive(Deserialize)]
struct WatchReq { folder: String, addr: String, accept_first: bool, fingerprint: Option<String> }

#[derive(Deserialize)]
struct StopReq {}

pub async fn run_ui(port: u16) -> anyhow::Result<()> {
  let status = Arc::new(tokio::sync::Mutex::new(SyncStatus::default()));
  crate::status::init(status.clone());
  let state = AppState{ 
    watch: Arc::new(tokio::sync::Mutex::new(None)),
    status,
  };
    let app = Router::new()
        .route("/", get(index))
        .route("/api/serve", post(api_serve))
        .route("/api/connect", post(api_connect))
    .route("/api/watch/start", post(api_watch_start))
    .route("/api/watch/stop", post(api_watch_stop))
    .route("/api/status", get(api_status))
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
<style>
  body{font-family:system-ui;margin:2rem;max-width:900px}
  input,button{margin:.25rem;padding:.4rem}
  .bar{height:12px;background:#eee;width:100%;border-radius:6px;overflow:hidden}
  #bar-fill{height:100%;background:#4caf50;width:0%}
  .row{display:flex;gap:1rem;align-items:center}
</style>
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
  <section>
    <h3>Watch mode</h3>
    <div>
      <input id="watch-folder" placeholder="Folder to watch" size="50" />
      <input id="watch-addr" placeholder="IP:port" size="20" />
      <label><input type="checkbox" id="watch-accept-first"/> Accept first</label>
      <input id="watch-fp" placeholder="Fingerprint (hex, optional)" size="70" />
      <button onclick="startWatch()">Start Watch</button>
      <button onclick="stopWatch()">Stop Watch</button>
      <div id="watch-out"></div>
    </div>
  </section>
  <section>
    <h3>Status</h3>
  <div class="row"><div>Active:</div><div id="active">false</div></div>
  <div class="row"><div>File:</div><div id="file">-</div></div>
  <div class="bar"><div id="bar-fill"></div></div>
  <div class="row"><div>Progress:</div><div id="progress">0 / 0</div></div>
  <div class="row"><div>Speed:</div><div id="speed">0 MB/s</div></div>
  <div class="row"><div>Last:</div><div id="last">-</div></div>
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
async function startWatch(){
  const folder = document.getElementById('watch-folder').value;
  const addr = document.getElementById('watch-addr').value;
  const accept_first = document.getElementById('watch-accept-first').checked;
  const fingerprint = document.getElementById('watch-fp').value || null;
  const r = await fetch('/api/watch/start',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({folder,addr,accept_first,fingerprint})});
  const j = await r.json();
  document.getElementById('watch-out').textContent = j.msg;
}
async function stopWatch(){
  const r = await fetch('/api/watch/stop',{method:'POST'});
  const j = await r.json();
  document.getElementById('watch-out').textContent = j.msg;
}
let lastBytes = 0;
let lastTs = 0;
function fmtBytes(b){
  const units=['B','KB','MB','GB','TB'];
  let u=0, x=b;
  while(x>=1024 && u<units.length-1){x/=1024;u++;}
  return `${x.toFixed( u?1:0)} ${units[u]}`;
}
async function refreshStatus(){
  try{
    const r = await fetch('/api/status');
    const s = await r.json();
    document.getElementById('active').textContent = s.active ? 'true' : 'false';
    document.getElementById('file').textContent = s.current_file || '-';
    const rec = s.current_received || 0;
    const tot = s.current_total || 0;
    const pct = tot>0 ? Math.min(100, Math.max(0, (rec*100.0)/tot)) : 0;
    document.getElementById('bar-fill').style.width = pct.toFixed(1)+'%';
    document.getElementById('progress').textContent = `${fmtBytes(rec)} / ${fmtBytes(tot)} (${pct.toFixed(1)}%)`;
    const now = performance.now();
    if(lastTs>0 && rec>=lastBytes){
      const dt = (now - lastTs)/1000.0;
      const db = rec - lastBytes;
      const bps = db/dt;
      const mbps = bps/1024/1024;
      if(isFinite(mbps)) document.getElementById('speed').textContent = `${mbps.toFixed(2)} MB/s`;
    }
    lastBytes = rec; lastTs = now;
    const when = s.last_sync_time || null;
    const ok = s.last_sync_ok;
    const ev = s.last_event || '-';
    const msg = s.last_message || '';
    document.getElementById('last').textContent = `${ev}${ok==null?'':(' ok='+ok)}${when?(' at '+when):''}${msg?(' ('+msg+')'):''}`;
  }catch{ /* ignore */ }
}
setInterval(refreshStatus, 1000);
refreshStatus();
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

async fn api_watch_start(State(state): State<Arc<AppState>>, Json(req): Json<WatchReq>) -> Json<Resp> {
  let mut guard = state.watch.lock().await;
  if guard.is_some() {
    return Json(Resp { ok: false, msg: "Watch already running".to_string() });
  }
  match crate::watch::spawn_watch(PathBuf::from(req.folder), req.addr, req.accept_first, req.fingerprint) {
    Ok(handle) => { 
      *guard = Some(handle);
      let mut st = state.status.lock().await;
      st.last_event = Some("watch_started".into());
      st.last_sync_ok = None;
      st.last_sync_time = Some(Utc::now());
      Json(Resp { ok: true, msg: "Watch started".into() }) 
    },
    Err(e) => Json(Resp { ok: false, msg: format!("Failed to start watch: {e}") }),
  }
}

async fn api_watch_stop(State(state): State<Arc<AppState>>) -> Json<Resp> {
  let mut guard = state.watch.lock().await;
  if let Some(h) = guard.take() {
    tokio::spawn(async move { h.stop().await; });
  let mut st = state.status.lock().await;
  st.last_event = Some("watch_stopping".into());
  st.last_sync_time = Some(Utc::now());
  Json(Resp { ok: true, msg: "Watch stopping".into() })
  } else {
    Json(Resp { ok: false, msg: "No watch running".into() })
  }
}

async fn api_status(State(state): State<Arc<AppState>>) -> Json<SyncStatus> {
  Json(state.status.lock().await.clone())
}

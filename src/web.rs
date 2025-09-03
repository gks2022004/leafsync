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
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>LeafSync</title>
  <style>
    :root{
      --bg:#0f172a;        /* slate-900 */
      --panel:#0b1226;     /* slightly darker */
      --card:#111827;      /* gray-900 */
      --muted:#94a3b8;     /* slate-400 */
      --fg:#e5e7eb;        /* gray-200 */
      --primary:#4f46e5;   /* indigo-600 */
      --primary-2:#6366f1; /* indigo-500 */
      --ok:#10b981;        /* emerald-500 */
      --warn:#f59e0b;      /* amber-500 */
      --err:#ef4444;       /* red-500 */
      --border:#1f2937;    /* gray-800 */
      --shadow: 0 6px 24px rgba(0,0,0,.25), 0 2px 6px rgba(0,0,0,.2);
      --radius: 12px;
    }
  html,body{height:100%}
    body{
      margin:0; font-family: ui-sans-serif, system-ui, -apple-system, Segoe UI, Roboto, Ubuntu, Cantarell, Noto Sans, Helvetica Neue, Arial;
      background: radial-gradient(1200px 800px at 20% -10%, #1e293b, transparent),
                  radial-gradient(1200px 800px at 120% 10%, #1e293b, transparent), var(--bg);
      color:var(--fg);
    }
  .container{max-width:1000px;margin:0 auto;padding:28px}
    header{display:flex;align-items:center;justify-content:space-between;margin-bottom:18px}
    .brand{display:flex;gap:12px;align-items:center}
    .logo{width:32px;height:32px;border-radius:8px;background:linear-gradient(135deg,#22c55e,#4f46e5);box-shadow:var(--shadow)}
    .title{font-weight:700;letter-spacing:.2px}
    .subtitle{color:var(--muted);font-size:.9rem}
    .grid{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:18px}
    @media (max-width: 900px){.grid{grid-template-columns:1fr}}
    .card{background:linear-gradient(180deg, rgba(255,255,255,.02), transparent 40%), var(--card);border:1px solid var(--border);border-radius:var(--radius);padding:18px;box-shadow:var(--shadow)}
    .card h3{margin:0 0 12px 0;font-size:1.05rem}
    .row{display:flex;gap:12px;align-items:center;flex-wrap:wrap}
    .stack{display:flex;flex-direction:column;gap:10px}
    label{font-size:.85rem;color:var(--muted)}
    :root{--control-h:44px;--gap:12px;--fs:14px}
    input[type="text"], input[type="number"], input[type="password"], input[type="search"]{
      width:100%;background:#0b1226;color:var(--fg);border:1px solid var(--border);border-radius:10px;padding:10px 12px;outline:none;height:var(--control-h);line-height:calc(var(--control-h) - 2px);box-sizing:border-box;font-size:var(--fs);
    }
    .controls{display:grid;grid-template-columns:1fr 140px;gap:var(--gap)}
    .controls-3{display:grid;grid-template-columns:1fr 1fr auto;gap:var(--gap)}
    .btn{cursor:pointer;border:none;border-radius:10px;padding:0 16px;font-weight:600;height:var(--control-h);line-height:var(--control-h);font-size:var(--fs);min-width:130px}
    .btn:disabled{opacity:.6;cursor:not-allowed}
    .btn-primary{background:linear-gradient(180deg,var(--primary),var(--primary-2));color:white;box-shadow:0 6px 16px rgba(79,70,229,.35)}
    .btn-outline{background:transparent;color:var(--fg);border:1px solid var(--border)}
    .hint{color:var(--muted);font-size:.85rem}
    .kpi{font-size:.95rem;color:var(--muted)}
    .kpi b{color:var(--fg)}
    .bar{height:14px;background:#0b1226;border:1px solid var(--border);width:100%;border-radius:999px;overflow:hidden}
    #bar-fill{height:100%;background:linear-gradient(90deg,#22c55e,#16a34a);width:0%}
    footer{margin-top:20px;color:var(--muted);font-size:.85rem}
    .toast{position:fixed;right:20px;bottom:20px;background:#0b1226;border:1px solid var(--border);color:var(--fg);padding:10px 14px;border-radius:10px;opacity:0;transform:translateY(10px);transition:.25s;box-shadow:var(--shadow)}
    .toast.show{opacity:1;transform:translateY(0)}
  </style>
  <script>
    function $(id){return document.getElementById(id)}
    let lastBytes=0,lastTs=0;
    function fmtBytes(b){const u=['B','KB','MB','GB','TB'];let i=0,x=b;while(x>=1024&&i<u.length-1){x/=1024;i++;}return `${x.toFixed(i?1:0)} ${u[i]}`}
    function toast(msg){const t=$('toast');t.textContent=msg;t.classList.add('show');setTimeout(()=>t.classList.remove('show'),2200)}
    async function serve(){
      const folder = $('serve-folder').value.trim();
      const port = parseInt($('serve-port').value||'4455');
      if(!folder){toast('Folder is required');return}
      $('serve-btn').disabled=true;
      const r = await fetch('/api/serve',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({folder,port})});
      const j = await r.json(); $('serve-out').textContent=j.msg; $('serve-btn').disabled=false; toast('Server starting')
    }
    async function connectPeer(){
      const addr=$('connect-addr').value.trim(); const folder=$('connect-folder').value.trim();
      const accept_first=$('accept-first').checked; const fingerprint=$('fingerprint').value.trim()||null;
      if(!addr||!folder){toast('Address and local folder are required');return}
      $('connect-btn').disabled=true;
      const r=await fetch('/api/connect',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({addr,folder,accept_first,fingerprint})});
      const j=await r.json(); $('connect-out').textContent=j.msg; $('connect-btn').disabled=false; toast('Connect started')
    }
    async function startWatch(){
      const folder=$('watch-folder').value.trim(); const addr=$('watch-addr').value.trim();
      const accept_first=$('watch-accept-first').checked; const fingerprint=$('watch-fp').value.trim()||null;
      if(!folder||!addr){toast('Watch folder and address are required');return}
      $('watch-start').disabled=true; $('watch-stop').disabled=true;
      const r=await fetch('/api/watch/start',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({folder,addr,accept_first,fingerprint})});
      const j=await r.json(); $('watch-out').textContent=j.msg; $('watch-start').disabled=false; $('watch-stop').disabled=false; toast('Watch started')
    }
    async function stopWatch(){
      $('watch-start').disabled=true; $('watch-stop').disabled=true;
      const r=await fetch('/api/watch/stop',{method:'POST'}); const j=await r.json(); $('watch-out').textContent=j.msg; $('watch-start').disabled=false; $('watch-stop').disabled=false; toast('Watch stopped')
    }
    async function refreshStatus(){
      try{
        const r=await fetch('/api/status'); const s=await r.json();
        $('active').textContent = s.active?'true':'false';
        $('file').textContent = s.current_file||'-';
        const rec=s.current_received||0, tot=s.current_total||0;
        const pct=tot>0?Math.min(100,Math.max(0,(rec*100.0)/tot)):0;
        $('bar-fill').style.width=pct.toFixed(1)+'%';
        $('progress').textContent=`${fmtBytes(rec)} / ${fmtBytes(tot)} (${pct.toFixed(1)}%)`;
        const now=performance.now(); if(lastTs>0 && rec>=lastBytes){const dt=(now-lastTs)/1000.0; const db=rec-lastBytes; const mbps=(db/dt)/1024/1024; if(isFinite(mbps)) $('speed').textContent=`${mbps.toFixed(2)} MB/s`}
        lastBytes=rec; lastTs=now;
        const ev=s.last_event||'-'; const ok=s.last_sync_ok; const when=s.last_sync_time||''; const msg=s.last_message||'';
        $('last').textContent = `${ev}${ok==null?'':(' ok='+ok)}${when?(' at '+when):''}${msg?(' ('+msg+')'):''}`
      }catch{}
    }
    setInterval(refreshStatus,1000); window.addEventListener('load',refreshStatus)
  </script>
</head>
<body>
  <div class="container">
    <header>
      <div class="brand">
        <div class="logo"></div>
        <div>
          <div class="title">LeafSync</div>
          <div class="subtitle">P2P QUIC file sync with Merkle delta</div>
        </div>
      </div>
    </header>

    <div class="grid">
      <div class="card stack">
        <h3>Serve a folder</h3>
        <div class="controls">
          <input id="serve-folder" type="text" placeholder="Folder path (e.g. C:\\path\\to\\serve)" />
          <input id="serve-port" type="number" value="4455" min="1" max="65535" />
        </div>
        <div class="row">
          <button id="serve-btn" class="btn btn-primary" onclick="serve()">Start Server</button>
          <div id="serve-out" class="hint"></div>
        </div>
      </div>

      <div class="card stack">
        <h3>Connect to a peer</h3>
        <div class="controls">
          <input id="connect-addr" type="text" placeholder="IP:port (e.g. 127.0.0.1:4455)" />
          <input id="connect-folder" type="text" placeholder="Local folder (destination)" />
        </div>
        <div class="controls-3">
          <label><input type="checkbox" id="accept-first"/> Accept first</label>
          <input id="fingerprint" type="text" placeholder="Fingerprint (hex, optional)" />
          <button id="connect-btn" class="btn btn-primary" onclick="connectPeer()">Connect</button>
        </div>
        <div id="connect-out" class="hint"></div>
      </div>

      <div class="card stack">
        <h3>Watch mode</h3>
        <div class="controls">
          <input id="watch-folder" type="text" placeholder="Folder to watch (source)" />
          <input id="watch-addr" type="text" placeholder="Peer IP:port" />
        </div>
        <div class="controls-3">
          <label><input type="checkbox" id="watch-accept-first"/> Accept first</label>
          <input id="watch-fp" type="text" placeholder="Fingerprint (hex, optional)" />
          <div class="row">
            <button id="watch-start" class="btn btn-primary" onclick="startWatch()">Start Watch</button>
            <button id="watch-stop" class="btn btn-outline" onclick="stopWatch()">Stop Watch</button>
          </div>
        </div>
        <div id="watch-out" class="hint"></div>
      </div>

      <div class="card stack">
        <h3>Status</h3>
        <div class="row kpi"><div>Active:</div><div><b id="active">false</b></div></div>
        <div class="row kpi"><div>File:</div><div><b id="file">-</b></div></div>
        <div class="bar"><div id="bar-fill"></div></div>
        <div class="row kpi"><div>Progress:</div><div><b id="progress">0 / 0</b></div></div>
        <div class="row kpi"><div>Speed:</div><div><b id="speed">0 MB/s</b></div></div>
        <div class="row kpi"><div>Last:</div><div><b id="last">-</b></div></div>
      </div>
    </div>

    <footer>Tip: First connection can use “Accept first”; later runs will use the pinned fingerprint.</footer>
  </div>
  <div id="toast" class="toast"></div>
</body>
</html>"#)
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

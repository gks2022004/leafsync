use axum::{routing::{get, post}, Router, extract::{State, Query}, Json};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use chrono::Utc;
use directories::UserDirs;

#[derive(Clone)]
pub struct AppState {
  watch: Arc<tokio::sync::Mutex<Option<crate::watch::WatchHandle>>>,
  status: Arc<tokio::sync::Mutex<SyncStatus>>,
}
type SyncStatus = crate::status::SyncStatus;

#[derive(Deserialize)]
struct ServeReq { folder: String, port: u16, rel_file: Option<String> }

#[derive(Deserialize)]
struct ConnectReq { addr: String, folder: String, accept_first: bool, fingerprint: Option<String>, rel_file: Option<String>, mirror: Option<bool>, streams: Option<usize>, rate_mbps: Option<f64> }

#[derive(Serialize)]
struct Resp { ok: bool, msg: String }

#[derive(Deserialize)]
struct WatchReq { folder: String, addr: String, accept_first: bool, fingerprint: Option<String>, rel_file: Option<String>, mirror: Option<bool>, streams: Option<usize>, rate_mbps: Option<f64> }

#[derive(Deserialize)]
struct StopReq {}

#[derive(Serialize)]
struct DirEntry { name: String, path: String, has_children: bool }

#[derive(Serialize)]
struct FileEntry { name: String, path: String, size: u64 }

#[derive(Serialize)]
struct FsListResp { path: String, dirs: Vec<DirEntry>, files: Vec<FileEntry> }

#[derive(Deserialize)]
struct PathQuery { path: String }

#[derive(Serialize)]
struct QuickDir { name: String, path: String }

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
  .route("/api/fs/roots", get(api_fs_roots))
  .route("/api/fs/list", get(api_fs_list))
  .route("/api/fs/quick", get(api_fs_quick))
  .route("/assets/leafsync.png", get(asset_logo))
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
  .logo{width:32px;height:32px;border-radius:8px;overflow:hidden;box-shadow:var(--shadow);background:#0b1226;display:flex;align-items:center;justify-content:center}
  .logo img{width:32px;height:32px;display:block}
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

    /* File picker modal */
    .modal-backdrop{position:fixed;inset:0;background:rgba(0,0,0,.5);display:none;align-items:center;justify-content:center;z-index:50}
    .modal{width:min(720px,90vw);background:var(--card);border:1px solid var(--border);border-radius:12px;box-shadow:var(--shadow);padding:16px}
    .pathbar{display:flex;gap:8px;align-items:center;margin-bottom:10px}
    .pathbar input{flex:1}
  .filelist{border:1px solid var(--border);border-radius:10px;max-height:360px;overflow:auto;background:#0b1226}
  .filelist .row{display:flex;justify-content:space-between;padding:10px 12px;border-bottom:1px solid rgba(255,255,255,.05)}
    .filelist .row:last-child{border-bottom:none}
  .filelist .row.sel{background:rgba(99,102,241,.18)}
    .picker-actions{display:flex;justify-content:flex-end;gap:10px;margin-top:12px}
  .quick{display:flex;flex-wrap:wrap;gap:8px;margin:8px 0 12px 0}
  .chip{background:#0b1226;border:1px solid var(--border);padding:6px 10px;border-radius:999px;cursor:pointer;font-size:.9rem}
    .controls-folder-port{display:grid;grid-template-columns:1fr 140px auto;gap:var(--gap)}
    .controls-addr-folder{display:grid;grid-template-columns:1fr 1fr auto;gap:var(--gap)}
  </style>
  <script>
    function $(id){return document.getElementById(id)}
    let lastBytes=0,lastTs=0;
    function fmtBytes(b){const u=['B','KB','MB','GB','TB'];let i=0,x=b;while(x>=1024&&i<u.length-1){x/=1024;i++;}return `${x.toFixed(i?1:0)} ${u[i]}`}
    function toast(msg){const t=$('toast');t.textContent=msg;t.classList.add('show');setTimeout(()=>t.classList.remove('show'),2200)}
  let pickerTarget=null; let fileTarget=null; let currentPath=''; let selectedFile='';
  async function showPicker(targetId){ pickerTarget=targetId; fileTarget=(targetId==='connect-folder')?'connect-file': (targetId==='serve-folder')?'serve-file': (targetId==='watch-folder')?'watch-file': null; selectedFile=''; $('picker').style.display='flex'; await loadRoots(); updatePickerButtons(); }
    function hidePicker(){ $('picker').style.display='none'; }
    async function loadRoots(){
      try{ const r=await fetch('/api/fs/roots'); const roots=await r.json();
        if(!roots || !roots.length){ toast('No roots found'); return; }
        currentPath = roots[0]; $('picker-path').value=currentPath; await listDir(currentPath);
      }catch(e){ console.error(e); toast('Failed to load system roots'); }
      // Load quick-access folders
      try{ const r=await fetch('/api/fs/quick'); const q=await r.json(); const el=$('picker-quick'); if(el){ el.innerHTML='';
        for(const it of q){ const b=document.createElement('div'); b.className='chip'; b.textContent=it.name; b.title=it.path; b.onclick=()=>listDir(it.path); el.appendChild(b); }
      }}catch(e){ /* ignore */ }
    }
    function updatePickerButtons(){ const btn=$('picker-select-file'); if(btn){ btn.style.display = fileTarget? 'inline-block':'none'; btn.disabled = !selectedFile; } }
    function basename(p){ const i=Math.max(p.lastIndexOf('\\'), p.lastIndexOf('/')); return i>=0? p.slice(i+1): p; }
    function relPath(dir, file){ let d=dir.replace(/[\\\/]+$/,''); let f=file; if(f.toLowerCase().startsWith(d.toLowerCase()+"\\") || f.toLowerCase().startsWith(d.toLowerCase()+"/")){ let r=f.substring(d.length+1); return r.replaceAll('\\','/'); } return basename(f); }
    async function listDir(path){ try{
        const r=await fetch('/api/fs/list?'+new URLSearchParams({path})); const j=await r.json(); currentPath=j.path; $('picker-path').value=currentPath;
        const el=$('picker-list'); el.innerHTML=''; selectedFile=''; updatePickerButtons();
        const upPath = parentPath(currentPath);
        const upBtn=$('picker-up'); if(upBtn) upBtn.disabled = !upPath;
        if(upPath){
          const upRow = document.createElement('div'); upRow.className='row';
          upRow.innerHTML = `<div>..</div><div></div>`; upRow.style.cursor='pointer';
          upRow.onclick=()=> listDir(upPath);
          el.appendChild(upRow);
        }
        for(const d of j.dirs){ const row=document.createElement('div'); row.className='row'; row.style.cursor='pointer'; row.onclick=()=>listDir(d.path); row.innerHTML=`<div>📁 ${d.name}</div><div>${d.has_children?'›':''}</div>`; el.appendChild(row); }
        if(j.files&&j.files.length){
          for(const f of j.files){ const row=document.createElement('div'); row.className='row'; row.style.cursor='pointer'; row.onclick=()=>{ Array.from(el.children).forEach(c=>c.classList&&c.classList.remove('sel')); row.classList.add('sel'); selectedFile=f.path; updatePickerButtons(); };
            const size = (f.size>=1024)? (f.size>=1048576? (f.size/1048576).toFixed(1)+' MB' : (f.size/1024).toFixed(1)+' KB') : f.size+' B';
            row.innerHTML=`<div>📄 ${f.name}</div><div>${size}</div>`; el.appendChild(row); }
        }
      }catch(e){ toast('Cannot list directory') }
    }
    function parentPath(p){
      if(!p) return '';
      // strip trailing separators
      const pp = p.replace(/[\\\/]+$/,'');
      // Windows drive root like C:\ or C:
      if(/^[A-Za-z]:$/.test(pp)) return '';
      if(/^[A-Za-z]:$/.test(pp.replace(/\\+$/,''))) return '';
      const i = Math.max(pp.lastIndexOf('\\'), pp.lastIndexOf('/'));
      if(i<=0) return '';
      return pp.slice(0,i);
    }
  function chooseCurrent(){ if(!pickerTarget||!currentPath){toast('No folder selected');return} $(pickerTarget).value=currentPath; hidePicker(); }
  function chooseFile(){ if(!fileTarget){ return; } if(!selectedFile){ toast('Select a file'); return; } if(!pickerTarget||!currentPath){ toast('No folder selected'); return; } $(pickerTarget).value=currentPath; $(fileTarget).value = relPath(currentPath, selectedFile); hidePicker(); }
    async function serve(){
      const folder = $('serve-folder').value.trim();
      const port = parseInt($('serve-port').value||'4455');
      const rel_file = ($('serve-file')?.value.trim()||'')||null;
      if(!folder){toast('Folder is required');return}
      $('serve-btn').disabled=true;
      const r = await fetch('/api/serve',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({folder,port,rel_file})});
      const j = await r.json(); $('serve-out').textContent=j.msg; $('serve-btn').disabled=false; toast('Server starting')
    }
    async function connectPeer(){
      const addr=$('connect-addr').value.trim(); const folder=$('connect-folder').value.trim();
      const accept_first=$('accept-first').checked; const fingerprint=$('fingerprint').value.trim()||null;
      const rel_file=($('connect-file')?.value.trim()||'')||null;
      const mirror=$('connect-mirror')?.checked||false;
      const streams=parseInt($('connect-streams')?.value||'4');
      const rate_mbps=parseFloat($('connect-rate')?.value||'');
      const rate = isNaN(rate_mbps)? null : rate_mbps;
      if(!addr||!folder){toast('Address and local folder are required');return}
      $('connect-btn').disabled=true;
      const r=await fetch('/api/connect',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({addr,folder,accept_first,fingerprint,rel_file,mirror,streams,rate_mbps:rate})});
      const j=await r.json(); $('connect-out').textContent=j.msg; $('connect-btn').disabled=false; toast('Connect started')
    }
    async function startWatch(){
      const folder=$('watch-folder').value.trim(); const addr=$('watch-addr').value.trim();
      const accept_first=$('watch-accept-first').checked; const fingerprint=$('watch-fp').value.trim()||null;
      const rel_file=($('watch-file')?.value.trim()||'')||null;
      const mirror=$('watch-mirror')?.checked||false;
      const streams=parseInt($('watch-streams')?.value||'4');
      const rate_mbps=parseFloat($('watch-rate')?.value||'');
      const rate = isNaN(rate_mbps)? null : rate_mbps;
      if(!folder||!addr){toast('Watch folder and address are required');return}
      $('watch-start').disabled=true; $('watch-stop').disabled=true;
      const r=await fetch('/api/watch/start',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({folder,addr,accept_first,fingerprint,rel_file,mirror,streams,rate_mbps:rate})});
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
  <div class="logo"><img src="/assets/leafsync.png" alt="LeafSync"/></div>
        <div>
          <div class="title">LeafSync</div>
          <div class="subtitle">P2P QUIC file sync with Merkle delta</div>
        </div>
      </div>
    </header>

    <div class="grid">
      <div class="card stack">
        <h3>Serve a folder</h3>
        <div class="controls-folder-port">
          <input id="serve-folder" type="text" placeholder="Folder path (e.g. C:\\path\\to\\serve)" />
          <input id="serve-port" type="number" value="4455" min="1" max="65535" />
          <button class="btn btn-outline" onclick="showPicker('serve-folder')">Browse…</button>
        </div>
        <div class="controls">
          <input id="serve-file" type="text" placeholder="Specific file to serve/sync (relative to folder, optional)" />
        </div>
        <div class="row">
          <button id="serve-btn" class="btn btn-primary" onclick="serve()">Start Server</button>
          <div id="serve-out" class="hint"></div>
        </div>
      </div>

      <div class="card stack">
        <h3>Connect to a peer</h3>
        <div class="controls-addr-folder">
          <input id="connect-addr" type="text" placeholder="IP:port (e.g. 127.0.0.1:4455)" />
          <input id="connect-folder" type="text" placeholder="Local folder (destination)" />
          <button class="btn btn-outline" onclick="showPicker('connect-folder')">Browse…</button>
        </div>
        <div class="controls-3">
          <label><input type="checkbox" id="accept-first"/> Accept first</label>
          <label><input type="checkbox" id="connect-mirror"/> Mirror deletes</label>
          <input id="fingerprint" type="text" placeholder="Fingerprint (hex, optional)" />
          <button id="connect-btn" class="btn btn-primary" onclick="connectPeer()">Connect</button>
        </div>
        <div class="controls-3">
          <input id="connect-streams" type="number" min="1" max="16" value="4" placeholder="Streams (1-16)" />
          <input id="connect-rate" type="number" min="0" step="0.1" placeholder="Rate limit (Mbps, optional)" />
          <div></div>
        </div>
        <div class="controls">
          <input id="connect-file" type="text" placeholder="Specific file to sync (relative to folder, optional)" />
        </div>
        <div id="connect-out" class="hint"></div>
      </div>

      <div class="card stack">
        <h3>Watch mode</h3>
        <div class="controls-addr-folder">
          <input id="watch-folder" type="text" placeholder="Folder to watch (source)" />
          <input id="watch-addr" type="text" placeholder="Peer IP:port" />
          <button class="btn btn-outline" onclick="showPicker('watch-folder')">Browse…</button>
        </div>
        <div class="controls-3">
          <label><input type="checkbox" id="watch-accept-first"/> Accept first</label>
          <label><input type="checkbox" id="watch-mirror"/> Mirror deletes</label>
          <input id="watch-fp" type="text" placeholder="Fingerprint (hex, optional)" />
          <div class="row">
            <button id="watch-start" class="btn btn-primary" onclick="startWatch()">Start Watch</button>
            <button id="watch-stop" class="btn btn-outline" onclick="stopWatch()">Stop Watch</button>
          </div>
        </div>
        <div class="controls-3">
          <input id="watch-streams" type="number" min="1" max="16" value="4" placeholder="Streams (1-16)" />
          <input id="watch-rate" type="number" min="0" step="0.1" placeholder="Rate limit (Mbps, optional)" />
          <div></div>
        </div>
        <div class="controls">
          <input id="watch-file" type="text" placeholder="Specific file to sync (relative to folder, optional)" />
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
  <!-- Folder picker modal -->
  <div id="picker" class="modal-backdrop" onclick="if(event.target.id==='picker')hidePicker()">
    <div class="modal">
      <div class="pathbar">
        <input id="picker-path" type="text" readonly />
  <button id="picker-up" class="btn btn-outline" onclick="const p=parentPath(currentPath); if(p) listDir(p)" title="Up one level">Up</button>
      </div>
  <div id="picker-quick" class="quick"></div>
  <div id="picker-list" class="filelist"></div>
      <div class="picker-actions">
        <button class="btn btn-outline" onclick="hidePicker()">Cancel</button>
        <button id="picker-select-file" class="btn btn-outline" onclick="chooseFile()" style="display:none">Select File</button>
        <button class="btn btn-primary" onclick="chooseCurrent()">Select Folder</button>
      </div>
    </div>
  </div>
</body>
</html>"#)
}

async fn api_serve(State(_state): State<Arc<AppState>>, Json(req): Json<ServeReq>) -> Json<Resp> {
    let folder = PathBuf::from(req.folder);
    tokio::spawn(async move {
  if let Err(e) = crate::net::run_server_filtered(folder, req.port, req.rel_file).await {
            eprintln!("server error: {e:?}");
        }
    });
    Json(Resp { ok: true, msg: format!("Server starting on 0.0.0.0:{}", req.port) })
}

async fn api_connect(State(_state): State<Arc<AppState>>, Json(req): Json<ConnectReq>) -> Json<Resp> {
    let folder = PathBuf::from(req.folder);
    tokio::spawn(async move {
  if let Err(e) = crate::net::run_client_filtered(
      req.addr,
      folder,
      req.accept_first,
      req.fingerprint,
      req.rel_file,
      req.mirror.unwrap_or(false),
      req.streams.unwrap_or(4),
      req.rate_mbps,
    ).await {
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
  match crate::watch::spawn_watch_filtered(
      PathBuf::from(req.folder),
      req.addr,
      req.accept_first,
      req.fingerprint,
      req.rel_file,
      req.mirror.unwrap_or(false),
      req.streams.unwrap_or(4),
      req.rate_mbps,
    ) {
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

async fn asset_logo() -> Result<axum::response::Response, (StatusCode, String)> {
  let p = std::path::Path::new("assets/leafsync.png");
  match tokio::fs::read(p).await {
    Ok(bytes) => {
      let mut resp = axum::response::Response::new(bytes.into());
      resp.headers_mut().insert(axum::http::header::CONTENT_TYPE, axum::http::HeaderValue::from_static("image/png"));
      Ok(resp)
    }
    Err(e) => Err((StatusCode::NOT_FOUND, format!("asset not found: {}", e)))
  }
}

async fn api_fs_roots() -> Json<Vec<String>> {
  #[cfg(windows)]
  {
    let mut roots = Vec::new();
    for letter in 'A'..='Z' {
      let p = format!("{}:\\", letter);
      if std::path::Path::new(&p).exists() {
        roots.push(p);
      }
    }
    return Json(roots);
  }
  #[cfg(not(windows))]
  {
    Json(vec!["/".to_string()])
  }
}

async fn api_fs_list(Query(q): Query<PathQuery>) -> Json<FsListResp> {
  let path = PathBuf::from(&q.path);
  let mut dirs: Vec<DirEntry> = Vec::new();
  let mut files: Vec<FileEntry> = Vec::new();
  if let Ok(rd) = std::fs::read_dir(&path) {
    for e in rd {
      if let Ok(entry) = e {
        if let Ok(ft) = entry.file_type() {
          if ft.is_dir() {
            let p = entry.path();
            // Fast child-dir probe (up to a handful)
            let mut has_children = false;
            if let Ok(mut it) = std::fs::read_dir(&p) {
              for _ in 0..8 { // cap to 8 entries
                if let Some(Ok(ch)) = it.next() {
                  if ch.file_type().map(|ft| ft.is_dir()).unwrap_or(false) { has_children = true; break; }
                } else { break; }
              }
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let path_str = p.to_string_lossy().to_string();
            dirs.push(DirEntry{ name, path: path_str, has_children });
          } else if ft.is_file() {
            let name = entry.file_name().to_string_lossy().to_string();
            let p = entry.path();
            let path_str = p.to_string_lossy().to_string();
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            files.push(FileEntry{ name, path: path_str, size });
          }
        }
      }
    }
  }
  dirs.sort_by(|a,b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
  files.sort_by(|a,b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
  Json(FsListResp{ path: path.to_string_lossy().to_string(), dirs, files })
}

async fn api_fs_quick() -> Json<Vec<QuickDir>> {
  let mut v: Vec<QuickDir> = Vec::new();
  if let Some(ud) = UserDirs::new() {
    let push = |v: &mut Vec<QuickDir>, name: &str, p: Option<&std::path::Path>| {
      if let Some(p) = p { v.push(QuickDir{ name: name.to_string(), path: p.to_string_lossy().to_string() }); }
    };
    push(&mut v, "Desktop", ud.desktop_dir());
    // directories crate doesn't expose Downloads directly on all platforms; try common locations
    #[cfg(windows)]
    {
      // Try %USERPROFILE%/Downloads
      if let Some(home) = ud.home_dir().to_str() { let dl=std::path::Path::new(home).join("Downloads"); if dl.exists() { v.push(QuickDir{ name:"Downloads".into(), path: dl.to_string_lossy().to_string() }); } }
    }
    #[cfg(not(windows))]
    {
      if let Some(home) = ud.home_dir().to_str() { let dl = std::path::Path::new(home).join("Downloads"); if dl.exists() { v.push(QuickDir{ name:"Downloads".into(), path: dl.to_string_lossy().to_string() }); } }
    }
    push(&mut v, "Documents", ud.document_dir());
    push(&mut v, "Pictures", ud.picture_dir());
    push(&mut v, "Music", ud.audio_dir());
    push(&mut v, "Videos", ud.video_dir());
    push(&mut v, "Home", Some(ud.home_dir()));
  }
  // Augment with Windows Known Folders if available
  #[cfg(windows)]
  {
    use windows::Win32::UI::Shell::{FOLDERID_Downloads, FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Pictures, FOLDERID_Music, FOLDERID_Videos, SHGetKnownFolderPath, KNOWN_FOLDER_FLAG};
    use windows::Win32::Foundation::HANDLE;
  use windows::core::GUID;
    unsafe fn known_folder(id: &GUID) -> Option<String> {
      match SHGetKnownFolderPath(id as *const _, KNOWN_FOLDER_FLAG(0), HANDLE(0)) {
        Ok(p) => {
          let mut len = 0usize; while *p.0.add(len) != 0 { len+=1; }
          let s = String::from_utf16_lossy(std::slice::from_raw_parts(p.0, len));
          windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *mut _));
          Some(s)
        }
        Err(_) => None,
      }
    }
    let mut add = |name: &str, id: &GUID| { if let Some(p)=unsafe{known_folder(id)} { v.push(QuickDir{name:name.into(), path:p}); } };
    add("Desktop", &FOLDERID_Desktop);
    add("Downloads", &FOLDERID_Downloads);
    add("Documents", &FOLDERID_Documents);
    add("Pictures", &FOLDERID_Pictures);
    add("Music", &FOLDERID_Music);
    add("Videos", &FOLDERID_Videos);
  }
  Json(v)
}

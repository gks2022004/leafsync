use chrono::{DateTime, Utc};
use once_cell::sync::OnceCell;
use serde::Serialize;
use std::sync::Arc;

pub static STATUS: OnceCell<Arc<tokio::sync::Mutex<SyncStatus>>> = OnceCell::new();

#[derive(Default, Clone, Serialize)]
pub struct SyncStatus {
    pub last_event: Option<String>,
    pub last_sync_ok: Option<bool>,
    pub last_sync_time: Option<DateTime<Utc>>,
    pub active: bool,
    pub current_file: Option<String>,
    pub current_received: u64,
    pub current_total: u64,
    pub last_message: Option<String>,
}

pub fn init(handle: Arc<tokio::sync::Mutex<SyncStatus>>) {
    let _ = STATUS.set(handle);
}

pub async fn set_active(active: bool) {
    if let Some(h) = STATUS.get() {
        let mut s = h.lock().await;
        s.active = active;
        s.last_event = Some(if active { "sync_started" } else { "sync_idle" }.into());
        s.last_sync_time = Some(Utc::now());
    }
}

pub async fn start_file(name: &str, total: u64) {
    if let Some(h) = STATUS.get() {
        let mut s = h.lock().await;
        s.current_file = Some(name.to_string());
        s.current_total = total;
        s.current_received = 0;
        s.last_event = Some("file_started".into());
        s.last_sync_time = Some(Utc::now());
    }
}

pub async fn progress(received: u64) {
    if let Some(h) = STATUS.get() {
        let mut s = h.lock().await;
        s.current_received = received;
        s.last_event = Some("progress".into());
        s.last_sync_time = Some(Utc::now());
    }
}

pub async fn file_done(ok: bool, msg: &str) {
    if let Some(h) = STATUS.get() {
        let mut s = h.lock().await;
        s.last_sync_ok = Some(ok);
        s.last_message = Some(msg.to_string());
        s.last_event = Some("file_done".into());
        s.last_sync_time = Some(Utc::now());
    }
}

pub async fn session_done(ok: bool, msg: &str) {
    if let Some(h) = STATUS.get() {
        let mut s = h.lock().await;
        s.active = false;
        s.last_sync_ok = Some(ok);
        s.last_message = Some(msg.to_string());
        s.last_event = Some("session_done".into());
        s.current_file = None;
        s.current_total = 0;
        s.current_received = 0;
        s.last_sync_time = Some(Utc::now());
    }
}

use anyhow::Result;
use globset::{Glob, GlobSet, GlobSetBuilder};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{path::{Path, PathBuf}, time::{Duration, Instant}, sync::{Arc, Mutex}, sync::atomic::{AtomicBool, Ordering}};

const DEFAULT_POLL_INTERVAL_SECS: u64 = 10; // periodic remote check to pull changes

fn load_ignore(root: &Path) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    let default_ignores = vec![".leafsync_tmp/**", "**/.git/**", "**/~$*", "**/*.part"];
    for pat in default_ignores { let _ = builder.add(Glob::new(pat).unwrap()); }
    let file = root.join(".leafsyncignore");
    if let Ok(text) = std::fs::read_to_string(file) {
        for line in text.lines() {
            let pat = line.trim();
            if pat.is_empty() || pat.starts_with('#') { continue; }
            if let Ok(g) = Glob::new(pat) { let _ = builder.add(g); }
        }
    }
    builder.build().unwrap_or_else(|_| GlobSetBuilder::new().build().unwrap())
}

fn is_ignored(gs: &GlobSet, root: &Path, p: &Path) -> bool {
    let rel = p.strip_prefix(root).unwrap_or(p);
    gs.is_match(rel)
}

pub async fn watch_and_sync(root: PathBuf, addr: String, accept_first: bool, fingerprint: Option<String>) -> Result<()> {
    // Wrapper that runs until process exit
    let cancel = Arc::new(AtomicBool::new(false));
    watch_and_sync_with_cancel(root, addr, accept_first, fingerprint, None, cancel).await
}

async fn watch_and_sync_with_cancel(root: PathBuf, addr: String, accept_first: bool, fingerprint: Option<String>, only_file: Option<String>, cancel: Arc<AtomicBool>) -> Result<()> {
    println!("Watch: {} -> {}", root.display(), addr);
    let ignores = Arc::new(Mutex::new(load_ignore(&root)));
    let pending = Arc::new(Mutex::new(false));
    let last_fire = Arc::new(Mutex::new(Instant::now()));
    let running = Arc::new(AtomicBool::new(false));
    let mut last_poll = Instant::now();

    let mut watcher = RecommendedWatcher::new({
        let root = root.clone();
        let ignores = ignores.clone();
        let pending = pending.clone();
        let last_fire = last_fire.clone();
    move |res: notify::Result<Event>| {
            if let Ok(ev) = res {
                let relevant = matches!(ev.kind, EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) | EventKind::Any);
                if !relevant { return; }
                if ev.paths.iter().any(|p| is_ignored(&ignores.lock().unwrap(), &root, p)) { return; }
                // debounce: mark pending and record time
                *pending.lock().unwrap() = true;
                *last_fire.lock().unwrap() = Instant::now();
            }
        }
    }, Config::default())?;
    watcher.watch(&root, RecursiveMode::Recursive)?;

    // Helper to trigger a sync if there isn't one running
    let trigger_sync = {
        let running = running.clone();
        let addr = addr.clone();
        let root = root.clone();
        let fingerprint = fingerprint.clone();
        let ignores = ignores.clone();
        move || {
            if running
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                println!("Change detected; syncing...");
                let addr_c = addr.clone();
                let root_c = root.clone();
                let fp = fingerprint.clone();
                let running_c = running.clone();
                let ignores_c = ignores.clone();
                let root_for_ignore = root.clone();
                let only_file_c = only_file.clone();
                tokio::spawn(async move {
                    let _ = crate::net::run_client_filtered(addr_c, root_c, accept_first, fp, only_file_c).await;
                    // reload ignore patterns in case file changed
                    *ignores_c.lock().unwrap() = load_ignore(&root_for_ignore);
                    running_c.store(false, Ordering::SeqCst);
                });
            }
        }
    };

    // Periodic loop to debounce and trigger sync
    loop {
        if cancel.load(Ordering::Relaxed) { break; }
        tokio::time::sleep(Duration::from_millis(400)).await;
        let mut do_run = false;
        {
            let mut p = pending.lock().unwrap();
            if *p && last_fire.lock().unwrap().elapsed() > Duration::from_millis(350) {
                do_run = true; *p = false;
            }
        }
    if do_run { trigger_sync(); }

        // periodic poll to pull remote-only changes (bidirectional mode)
        if last_poll.elapsed() >= Duration::from_secs(DEFAULT_POLL_INTERVAL_SECS) {
            last_poll = Instant::now();
            trigger_sync();
        }
    }
    Ok(())
}

    pub struct WatchHandle {
        cancel: Arc<AtomicBool>,
        join: tokio::task::JoinHandle<()>,
    }

    impl WatchHandle {
        pub async fn stop(self) {
            self.cancel.store(true, Ordering::Relaxed);
            let _ = self.join.await;
        }
    }

    pub fn spawn_watch(root: PathBuf, addr: String, accept_first: bool, fingerprint: Option<String>) -> Result<WatchHandle> {
        spawn_watch_filtered(root, addr, accept_first, fingerprint, None)
    }

    pub fn spawn_watch_filtered(root: PathBuf, addr: String, accept_first: bool, fingerprint: Option<String>, rel_file: Option<String>) -> Result<WatchHandle> {
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_c = cancel.clone();
        let join = tokio::spawn(async move {
            let _ = watch_and_sync_with_cancel(root, addr, accept_first, fingerprint, rel_file, cancel_c).await;
        });
        Ok(WatchHandle { cancel, join })
    }

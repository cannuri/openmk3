//! Incremental filesystem watcher that drives re-scans.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::IndexError;

/// Non-blocking watcher that batches change events and calls `on_batch`
/// once per debounce window.
pub struct LibraryWatcher {
    _watcher: RecommendedWatcher,
    _thread: std::thread::JoinHandle<()>,
}

impl LibraryWatcher {
    pub fn start(
        roots: Vec<PathBuf>,
        debounce: Duration,
        on_batch: impl Fn(&[PathBuf]) + Send + 'static,
    ) -> Result<Self, IndexError> {
        let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
        let mut watcher = notify::recommended_watcher(move |res| {
            let _ = tx.send(res);
        }).map_err(|e| IndexError::Other(format!("watcher: {e}")))?;
        watcher
            .configure(Config::default())
            .map_err(|e| IndexError::Other(format!("watcher config: {e}")))?;
        for r in &roots {
            watcher
                .watch(r, RecursiveMode::Recursive)
                .map_err(|e| IndexError::Other(format!("watch {}: {e}", r.display())))?;
        }
        let handle = std::thread::spawn(move || {
            let mut pending: Vec<PathBuf> = Vec::new();
            let mut deadline: Option<Instant> = None;
            loop {
                let timeout = deadline
                    .map(|d| d.saturating_duration_since(Instant::now()))
                    .unwrap_or(Duration::from_secs(60));
                match rx.recv_timeout(timeout) {
                    Ok(Ok(ev)) => {
                        if matches!(ev.kind, EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)) {
                            for p in ev.paths {
                                if is_interesting(&p) { pending.push(p); }
                            }
                            deadline = Some(Instant::now() + debounce);
                        }
                    }
                    Ok(Err(_)) => {}
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if !pending.is_empty() && deadline.map_or(false, |d| d <= Instant::now()) {
                            on_batch(&pending);
                            pending.clear();
                            deadline = None;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        Ok(Self { _watcher: watcher, _thread: handle })
    }
}

fn is_interesting(p: &Path) -> bool {
    p.extension().and_then(|s| s.to_str()) == Some("nksf")
}

//! Plugin session manager.
//!
//! Each session owns one `maschine-pluginhost` child process. Control
//! messages travel as line-delimited JSON on the child's stdio; audio will
//! travel through a separate shm ring buffer (handled JUCE-side, M5.2).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, oneshot, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub id: u64,
    pub kind: String,
    pub body: serde_json::Value,
}

/// One live plugin instance.
#[allow(dead_code)]
pub struct PluginSession {
    /// Held solely for its `kill_on_drop` behavior — the dispatch loop owns
    /// its own `stdout` reader and we push commands through `stdin`.
    child: Child,
    stdin: Mutex<ChildStdin>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Envelope>>>,
    events_tx: mpsc::Sender<Envelope>,
    next_id: AtomicU64,
}

impl PluginSession {
    /// Spawn a `maschine-pluginhost` binary and start the dispatch task.
    ///
    /// `events_tx` receives every envelope with id=0 (host→daemon events).
    pub async fn spawn(
        binary: impl AsRef<Path>,
        events_tx: mpsc::Sender<Envelope>,
    ) -> Result<std::sync::Arc<Self>> {
        let mut child = Command::new(binary.as_ref())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("spawn {}", binary.as_ref().display()))?;

        let stdin = child.stdin.take().context("no stdin")?;
        let stdout = child.stdout.take().context("no stdout")?;
        let session = std::sync::Arc::new(Self {
            child,
            stdin: Mutex::new(stdin),
            pending: Mutex::new(HashMap::new()),
            events_tx,
            next_id: AtomicU64::new(1),
        });
        tokio::spawn(Self::read_loop(session.clone(), stdout));
        Ok(session)
    }

    async fn read_loop(this: std::sync::Arc<Self>, stdout: ChildStdout) {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("pluginhost read error: {e}");
                    break;
                }
            }
            let env: Envelope = match serde_json::from_str(line.trim()) {
                Ok(e) => e,
                Err(e) => {
                    tracing::debug!("pluginhost non-JSON line: {e}: {line}");
                    continue;
                }
            };
            if env.id == 0 {
                let _ = this.events_tx.send(env).await;
                continue;
            }
            let mut pending = this.pending.lock().await;
            if let Some(tx) = pending.remove(&env.id) {
                let _ = tx.send(env);
            }
        }
    }

    /// Send a request and await the matching reply.
    pub async fn call(&self, kind: &str, body: serde_json::Value) -> Result<Envelope> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        let env = Envelope { id, kind: kind.into(), body };
        let line = serde_json::to_string(&env)? + "\n";
        self.stdin.lock().await.write_all(line.as_bytes()).await?;
        rx.await.map_err(|_| anyhow::anyhow!("pluginhost closed before replying"))
    }

    #[allow(dead_code)] // used from SessionManager::shutdown in v0.2.
    pub async fn shutdown(self: std::sync::Arc<Self>) -> Result<()> {
        // Best-effort: request a clean exit, then rely on kill_on_drop when
        // the last Arc goes out of scope.
        let _ = self.call("shutdown", serde_json::json!({})).await;
        Ok(())
    }
}

/// Map `maschined` manages — one session per loaded plugin bundle.
pub struct SessionManager {
    bin: PathBuf,
    events_tx: mpsc::Sender<Envelope>,
    by_bundle: Mutex<HashMap<PathBuf, std::sync::Arc<PluginSession>>>,
}

impl SessionManager {
    pub fn new(binary: PathBuf) -> (std::sync::Arc<Self>, mpsc::Receiver<Envelope>) {
        let (tx, rx) = mpsc::channel(128);
        let mgr = std::sync::Arc::new(Self {
            bin: binary,
            events_tx: tx,
            by_bundle: Mutex::new(HashMap::new()),
        });
        (mgr, rx)
    }

    pub async fn load(&self, bundle: &Path, state_base64: &str) -> Result<Envelope> {
        let mut map = self.by_bundle.lock().await;
        let session = match map.get(bundle) {
            Some(s) => s.clone(),
            None => {
                let s = PluginSession::spawn(&self.bin, self.events_tx.clone()).await?;
                map.insert(bundle.to_path_buf(), s.clone());
                s
            }
        };
        drop(map);
        session.call("load", serde_json::json!({
            "bundle": bundle.display().to_string(),
            "state_base64": state_base64,
        })).await
    }

    #[allow(dead_code)] // wired up in M6 MIDI routing work.
    pub async fn midi(&self, bundle: &Path, status: u8, data1: u8, data2: u8) -> Result<()> {
        let session = self.by_bundle.lock().await.get(bundle).cloned()
            .ok_or_else(|| anyhow::anyhow!("no session for {}", bundle.display()))?;
        session.call("midi", serde_json::json!({
            "status": status, "note": data1, "vel": data2,
        })).await?;
        Ok(())
    }
}

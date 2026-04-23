//! UDS / line-delimited JSON surface used by `msctl`.

use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", content = "arg")]
pub enum Op {
    #[serde(rename = "status")]           Status,
    #[serde(rename = "scan_libraries")]   ScanLibraries,
    #[serde(rename = "browse")]           Browse(BrowseArgs),
    #[serde(rename = "load")]             Load { preset_id: i64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowseArgs {
    #[serde(default)] pub text:   Option<String>,
    #[serde(default)] pub vendor: Option<String>,
    #[serde(default)] pub r#type: Option<String>,
    #[serde(default)] pub limit:  Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    pub payload: serde_json::Value,
}

pub async fn serve(path: PathBuf, handler: mpsc::Sender<(Op, mpsc::Sender<Reply>)>) -> Result<()> {
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    tokio::spawn(async move {
        while let Ok((sock, _)) = listener.accept().await {
            let h = handler.clone();
            tokio::spawn(async move {
                let (r, mut w) = sock.into_split();
                let mut lines = BufReader::new(r).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let op: Op = match serde_json::from_str(&line) {
                        Ok(o) => o,
                        Err(e) => {
                            let _ = w.write_all(format!("{{\"ok\":false,\"payload\":{{\"error\":\"{e}\"}}}}\n").as_bytes()).await;
                            continue;
                        }
                    };
                    let (tx, mut rx) = mpsc::channel::<Reply>(1);
                    let _ = h.send((op, tx)).await;
                    let reply = rx.recv().await.unwrap_or(Reply { ok: false, payload: serde_json::json!({"error":"no reply"}) });
                    let _ = w.write_all((serde_json::to_string(&reply).unwrap_or_default() + "\n").as_bytes()).await;
                }
            });
        }
    });
    Ok(())
}

//! `msctl` — CLI client for maschined over its UDS JSON surface.

use std::env;
use std::path::PathBuf;

use anyhow::{bail, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

fn usage() -> ! {
    eprintln!("usage: msctl <status | browse [--vendor V] [--type T] [--text TEXT] | load PRESET_ID>");
    std::process::exit(2);
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() { usage(); }

    let payload = match args[0].as_str() {
        "status" => serde_json::json!({"op": "status"}),
        "browse" => {
            let mut text: Option<String> = None;
            let mut vendor: Option<String> = None;
            let mut type_: Option<String> = None;
            let mut iter = args.iter().skip(1);
            while let Some(a) = iter.next() {
                match a.as_str() {
                    "--text"   => { text = iter.next().cloned(); }
                    "--vendor" => { vendor = iter.next().cloned(); }
                    "--type"   => { type_ = iter.next().cloned(); }
                    _ => usage(),
                }
            }
            serde_json::json!({"op":"browse","arg":{"text":text,"vendor":vendor,"type":type_,"limit":50}})
        }
        "load" => {
            let id: i64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or_else(|| usage());
            serde_json::json!({"op":"load","arg":{"preset_id":id}})
        }
        _ => usage(),
    };

    let path = runtime_dir().join("maschined.sock");
    let stream = UnixStream::connect(&path).await
        .map_err(|e| anyhow::anyhow!("connect {}: {e}. Is maschined running?", path.display()))?;
    let (r, mut w) = stream.into_split();
    let line = serde_json::to_string(&payload)? + "\n";
    w.write_all(line.as_bytes()).await?;
    let mut lines = BufReader::new(r).lines();
    let Some(reply) = lines.next_line().await? else { bail!("daemon closed without replying"); };
    println!("{}", reply);
    Ok(())
}

fn runtime_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") { return PathBuf::from(xdg); }
    PathBuf::from("/tmp")
}

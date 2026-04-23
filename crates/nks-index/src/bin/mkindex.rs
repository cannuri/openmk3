//! `mkindex` — standalone CLI that builds the NKS library index and runs
//! queries without involving the Mk3 daemon.
//!
//! Examples:
//!   mkindex scan                              # scan default NI locations
//!   mkindex scan /path/to/some/folder         # scan a specific root
//!   mkindex list --vendor "Native Instruments"
//!   mkindex list --type Lead --text lead

use std::path::PathBuf;

use anyhow::Result;
use nks_index::{default_roots, query, Scanner};

fn usage() -> ! {
    eprintln!("usage: mkindex (scan [ROOT...] | list [--text T] [--vendor V] [--type T] [--limit N])");
    std::process::exit(2);
}

fn db_path() -> PathBuf {
    let base = if let Ok(h) = std::env::var("HOME") {
        PathBuf::from(h).join("Library/Application Support/maschined")
    } else {
        PathBuf::from("/tmp/maschined")
    };
    std::fs::create_dir_all(&base).ok();
    base.join("index.sqlite")
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() { usage(); }
    let db = db_path();
    eprintln!("index db: {}", db.display());

    match args[0].as_str() {
        "scan" => {
            let mut scanner = Scanner::open(&db)?;
            let roots: Vec<PathBuf> = if args.len() > 1 {
                args[1..].iter().map(PathBuf::from).collect()
            } else {
                default_roots()
            };
            if roots.is_empty() {
                eprintln!("no library roots found — pass one explicitly or install NI content");
            }
            let start = std::time::Instant::now();
            for r in roots {
                eprintln!("scanning {}", r.display());
                let st = scanner.scan_root(&r)?;
                eprintln!("  seen={} added={} updated={} skipped={} failed={}",
                          st.seen, st.added, st.updated, st.skipped, st.failed);
            }
            eprintln!("done in {:?}", start.elapsed());
        }
        "list" => {
            let mut q = query::Query::default();
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--text" => { q.text = args.get(i + 1).cloned(); i += 2; }
                    "--vendor" => { q.vendor = args.get(i + 1).cloned(); i += 2; }
                    "--type" => { q.type_filter = args.get(i + 1).cloned(); i += 2; }
                    "--limit" => {
                        q.limit = args.get(i + 1).and_then(|s| s.parse().ok());
                        i += 2;
                    }
                    _ => usage(),
                }
            }
            if q.limit.is_none() { q.limit = Some(50); }
            let conn = rusqlite::Connection::open(&db)?;
            let rows = query::run(&conn, &q)?;
            println!("{} presets:", rows.len());
            for r in rows {
                println!("  [{:>5}] {:<32}  {:<20}  {}", r.id, r.name, r.vendor, r.bank_chain);
            }
        }
        _ => usage(),
    }
    Ok(())
}

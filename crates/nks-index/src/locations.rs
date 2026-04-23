//! Auto-detect standard NKS library roots on macOS.

use std::path::PathBuf;

pub fn default_roots() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let home = std::env::var("HOME").ok().map(PathBuf::from);
    push_if_exists(&mut out, PathBuf::from("/Library/Application Support/Native Instruments"));
    if let Some(h) = &home {
        push_if_exists(&mut out, h.join("Documents/Native Instruments/User Content"));
        push_if_exists(&mut out, h.join("Library/Application Support/Native Instruments"));
    }
    out
}

/// Path to the Komplete Kontrol SQLite database if it exists.
pub fn komplete_db_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let p = PathBuf::from(home).join("Library/Application Support/Native Instruments/Komplete Kontrol/komplete.db3");
    p.exists().then_some(p)
}

fn push_if_exists(out: &mut Vec<PathBuf>, p: PathBuf) {
    if p.exists() { out.push(p); }
}

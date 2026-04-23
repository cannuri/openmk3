//! Library scanner: walks filesystem roots, parses `.nksf` metadata, and
//! upserts rows into the SQLite/FTS5 index.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rusqlite::{params, Connection};
use tracing::{debug, warn};

use nks_parse::{NksFile, NksPluginId};

use crate::IndexError;

pub struct Scanner {
    db: Connection,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ScanStats {
    pub seen: usize,
    pub added: usize,
    pub updated: usize,
    pub skipped: usize,
    pub failed: usize,
}

impl Scanner {
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self, IndexError> {
        let db = Connection::open(db_path)?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "synchronous", "NORMAL")?;
        db.execute_batch(SCHEMA_SQL)?;
        Ok(Self { db })
    }

    pub fn scan_root(&mut self, root: impl AsRef<Path>) -> Result<ScanStats, IndexError> {
        let mut stats = ScanStats::default();
        walk(root.as_ref(), &mut |p| {
            if p.extension().and_then(|s| s.to_str()) != Some("nksf") { return; }
            stats.seen += 1;
            match self.upsert(p) {
                Ok(true) => stats.added += 1,
                Ok(false) => stats.updated += 1,
                Err(e) => {
                    warn!(path = %p.display(), "scan failed: {e}");
                    stats.failed += 1;
                }
            }
        })?;
        Ok(stats)
    }

    fn upsert(&mut self, path: &Path) -> Result<bool, IndexError> {
        let md = std::fs::metadata(path)?;
        let mtime_ns = md.modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos() as i64)
            .unwrap_or(0);
        let size = md.len() as i64;

        let path_str = path.to_string_lossy();
        let existing: Option<(i64, i64)> = self.db.query_row(
            "SELECT mtime_ns, size FROM presets WHERE path = ?1",
            params![&path_str],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).ok();
        if let Some((prev_mtime, prev_size)) = existing {
            if prev_mtime == mtime_ns && prev_size == size {
                return Ok(false);
            }
        }

        let nks = NksFile::scan(path)?;
        debug!(path = %path.display(), "indexing {:?}", nks.summary.name);

        let plugin_key = match &nks.plugin {
            NksPluginId::Vst3 { uid, .. } => format!(
                "vst3:{:08x}{:08x}{:08x}{:08x}",
                uid[0] as u32, uid[1] as u32, uid[2] as u32, uid[3] as u32
            ),
            NksPluginId::AudioUnit { ty, subtype, manufacturer, .. } =>
                format!("au:{:08x}:{:08x}:{:08x}", ty, subtype, manufacturer),
            NksPluginId::Vst2 { magic } => format!("vst2:{:08x}", magic),
            NksPluginId::Unknown => "unknown".into(),
        };
        let bank_chain = nks.summary.bankchain.clone().unwrap_or_default().join(" / ");

        let tx = self.db.transaction()?;
        tx.execute(
            "INSERT INTO presets (path, name, vendor, author, comment, plugin_ref, bank_chain, size, mtime_ns)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(path) DO UPDATE SET
               name = excluded.name,
               vendor = excluded.vendor,
               author = excluded.author,
               comment = excluded.comment,
               plugin_ref = excluded.plugin_ref,
               bank_chain = excluded.bank_chain,
               size = excluded.size,
               mtime_ns = excluded.mtime_ns",
            params![
                &path_str,
                nks.summary.name.as_deref().unwrap_or(""),
                nks.summary.vendor.as_deref().unwrap_or(""),
                nks.summary.author.as_deref().unwrap_or(""),
                nks.summary.comment.as_deref().unwrap_or(""),
                &plugin_key,
                &bank_chain,
                size,
                mtime_ns,
            ],
        )?;
        let preset_id: i64 = tx.query_row(
            "SELECT id FROM presets WHERE path = ?1",
            params![&path_str],
            |r| r.get(0),
        )?;
        tx.execute("DELETE FROM preset_types WHERE preset_id = ?1", params![preset_id])?;
        tx.execute("DELETE FROM preset_modes WHERE preset_id = ?1", params![preset_id])?;
        if let Some(types) = &nks.summary.types {
            for pair in types {
                let ty = pair.first().cloned().unwrap_or_default();
                let sub = pair.get(1).cloned().unwrap_or_default();
                tx.execute(
                    "INSERT INTO preset_types (preset_id, type, subtype) VALUES (?1, ?2, ?3)",
                    params![preset_id, ty, sub],
                )?;
            }
        }
        if let Some(modes) = &nks.summary.modes {
            for m in modes {
                tx.execute(
                    "INSERT INTO preset_modes (preset_id, mode) VALUES (?1, ?2)",
                    params![preset_id, m],
                )?;
            }
        }
        // SQLite FTS5 virtual tables don't support UPSERT, so we delete +
        // insert to keep updates idempotent.
        tx.execute("DELETE FROM presets_fts WHERE rowid = ?1", params![preset_id])?;
        tx.execute(
            "INSERT INTO presets_fts(rowid, name, vendor, author, comment, bank_chain)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                preset_id,
                nks.summary.name.as_deref().unwrap_or(""),
                nks.summary.vendor.as_deref().unwrap_or(""),
                nks.summary.author.as_deref().unwrap_or(""),
                nks.summary.comment.as_deref().unwrap_or(""),
                &bank_chain,
            ],
        )?;
        tx.commit()?;
        Ok(true)
    }
}

fn walk(root: &Path, visit: &mut dyn FnMut(&Path)) -> std::io::Result<()> {
    if !root.exists() { return Ok(()); }
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        match std::fs::read_dir(&dir) {
            Ok(rd) => for entry in rd.flatten() {
                let p = entry.path();
                if p.is_dir() { stack.push(p); } else { visit(&p); }
            },
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                warn!(path = %dir.display(), "skip (permission denied)");
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

const SCHEMA_SQL: &str = include_str!("schema.sql");

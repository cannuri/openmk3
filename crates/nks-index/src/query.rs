//! Structured + full-text querying against the index.

use std::path::PathBuf;

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::IndexError;

#[derive(Debug, Clone, Default)]
pub struct Query {
    pub text: Option<String>,
    pub vendor: Option<String>,
    pub type_filter: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PresetRow {
    pub id: i64,
    pub path: PathBuf,
    pub name: String,
    pub vendor: String,
    pub plugin_ref: String,
    pub bank_chain: String,
}

pub fn run(db: &Connection, q: &Query) -> Result<Vec<PresetRow>, IndexError> {
    let mut sql = String::from(
        "SELECT DISTINCT p.id, p.path, p.name, p.vendor, p.plugin_ref, p.bank_chain \
         FROM presets p",
    );
    let mut where_clauses = Vec::<String>::new();
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    let mut arg_i = 1usize;

    if let Some(t) = &q.type_filter {
        sql.push_str(" JOIN preset_types pt ON pt.preset_id = p.id");
        where_clauses.push(format!("pt.type = ?{}", arg_i));
        args.push(Box::new(t.clone())); arg_i += 1;
    }
    if let Some(text) = &q.text {
        if !text.is_empty() {
            sql.push_str(" JOIN presets_fts f ON f.rowid = p.id");
            where_clauses.push(format!("presets_fts MATCH ?{}", arg_i));
            args.push(Box::new(text.clone())); arg_i += 1;
        }
    }
    if let Some(v) = &q.vendor {
        where_clauses.push(format!("p.vendor = ?{}", arg_i));
        args.push(Box::new(v.clone()));
    }
    if !where_clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_clauses.join(" AND "));
    }
    sql.push_str(" ORDER BY p.name COLLATE NOCASE");
    if let Some(lim) = q.limit {
        sql.push_str(&format!(" LIMIT {lim}"));
    }

    let mut stmt = db.prepare(&sql)?;
    let args_ref: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
    let rows = stmt.query_map(rusqlite::params_from_iter(args_ref), |r| {
        Ok(PresetRow {
            id: r.get(0)?,
            path: PathBuf::from(r.get::<_, String>(1)?),
            name: r.get(2)?,
            vendor: r.get(3)?,
            plugin_ref: r.get(4)?,
            bank_chain: r.get(5)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows { out.push(row?); }
    Ok(out)
}

pub fn touch_recent(db: &Connection, preset_id: i64) -> Result<(), IndexError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    db.execute(
        "INSERT INTO recents(preset_id, last_used) VALUES (?1, ?2)
         ON CONFLICT(preset_id) DO UPDATE SET last_used = excluded.last_used",
        params![preset_id, now],
    )?;
    Ok(())
}

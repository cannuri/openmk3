//! Filesystem scanner + SQLite/FTS5 index for NKS libraries.

pub mod scanner;
pub mod query;
pub mod watch;
pub mod locations;

pub use scanner::{Scanner, ScanStats};
pub use query::{Query, PresetRow};
pub use locations::{default_roots, komplete_db_path};

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse: {0}")]
    Parse(#[from] nks_parse::NksError),
    #[error("{0}")]
    Other(String),
}

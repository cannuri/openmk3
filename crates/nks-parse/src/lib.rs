//! NKS (Native Kontrol Standard) preset file parser.

pub mod riff;
pub mod nisi;
pub mod plid;
pub mod nica;

pub use nisi::NksSummary;
pub use plid::NksPluginId;
pub use nica::NksController;

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum NksError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not an NKS file: missing NIKS RIFF header")]
    NotNks,
    #[error("required chunk missing: {0}")]
    MissingChunk(&'static str),
    #[error("msgpack decode error: {0}")]
    MsgPack(#[from] rmp_serde::decode::Error),
    #[error("riff error: {0}")]
    Riff(String),
}

/// Metadata-only view of a `.nksf` file. The plugin state (`PCHK`) is kept as
/// an offset + length so we can stream it on demand without holding the whole
/// preset in memory during library indexing.
#[derive(Debug, Clone)]
pub struct NksFile {
    pub path: PathBuf,
    pub summary: NksSummary,
    pub plugin: NksPluginId,
    pub controller: Option<NksController>,
    pub pchk_offset: u64,
    pub pchk_len: u64,
}

impl NksFile {
    /// Read metadata only; leaves `PCHK` on disk.
    pub fn scan(path: impl AsRef<Path>) -> Result<Self, NksError> {
        let path = path.as_ref().to_path_buf();
        riff::read_metadata(&path)
    }

    /// Lazy load of the plugin state blob.
    pub fn read_state(&self) -> Result<Vec<u8>, NksError> {
        use std::io::{Read, Seek, SeekFrom};
        let mut f = std::fs::File::open(&self.path)?;
        f.seek(SeekFrom::Start(self.pchk_offset))?;
        let mut buf = vec![0u8; self.pchk_len as usize];
        f.read_exact(&mut buf)?;
        Ok(buf)
    }
}

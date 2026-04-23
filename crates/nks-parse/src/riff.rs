//! RIFF container reader for `.nksf` files.
//!
//! An NKSF is a standard RIFF container whose form type is `NIKS`, carrying
//! four interesting sub-chunks:
//!
//! * `NISI` — MessagePack summary (preset name, vendor, author, bank chain,
//!   types, modes, …).
//! * `NICA` — MessagePack controller-assignment metadata.
//! * `PLID` — MessagePack plugin-identification descriptor.
//! * `PCHK` — opaque plugin-state blob. Kept on disk; we store its offset +
//!   length so callers can stream it later without holding megabytes in RAM.
//!
//! Other chunks (`hsin`, `hasi`, …) may appear in future versions and are
//! ignored.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::{NksController, NksError, NksFile, NksPluginId, NksSummary};

const MAGIC_RIFF: [u8; 4] = *b"RIFF";
const MAGIC_NIKS: [u8; 4] = *b"NIKS";

fn read4(f: &mut File) -> std::io::Result<[u8; 4]> {
    let mut b = [0u8; 4];
    f.read_exact(&mut b)?;
    Ok(b)
}

fn read_u32_le(f: &mut File) -> std::io::Result<u32> {
    let b = read4(f)?;
    Ok(u32::from_le_bytes(b))
}

/// Walk chunks in a NIKS RIFF and collect the metadata we care about.
pub fn read_metadata(path: &Path) -> Result<NksFile, NksError> {
    let mut f = File::open(path)?;

    if read4(&mut f)? != MAGIC_RIFF {
        return Err(NksError::NotNks);
    }
    let _total_len = read_u32_le(&mut f)?;
    if read4(&mut f)? != MAGIC_NIKS {
        return Err(NksError::NotNks);
    }

    let mut summary: Option<NksSummary> = None;
    let mut plugin: Option<NksPluginId> = None;
    let mut controller: Option<NksController> = None;
    let mut pchk: Option<(u64, u64)> = None;

    loop {
        let id = match read4(&mut f) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(NksError::Io(e)),
        };
        let len = read_u32_le(&mut f)? as u64;
        let data_start = f.stream_position()?;
        match &id {
            b"NISI" => {
                let mut buf = vec![0u8; len as usize];
                f.read_exact(&mut buf)?;
                summary = Some(rmp_serde::from_slice(&buf)?);
            }
            b"PLID" => {
                let mut buf = vec![0u8; len as usize];
                f.read_exact(&mut buf)?;
                plugin = Some(rmp_serde::from_slice(&buf)?);
            }
            b"NICA" => {
                let mut buf = vec![0u8; len as usize];
                f.read_exact(&mut buf)?;
                controller = Some(NksController { raw: buf });
            }
            b"PCHK" => {
                pchk = Some((data_start, len));
                f.seek(SeekFrom::Current(len as i64))?;
            }
            _ => {
                f.seek(SeekFrom::Current(len as i64))?;
            }
        }
        // RIFF chunks are padded to even length.
        if len % 2 == 1 {
            f.seek(SeekFrom::Current(1))?;
        }
    }

    let summary = summary.ok_or(NksError::MissingChunk("NISI"))?;
    let plugin = plugin.ok_or(NksError::MissingChunk("PLID"))?;
    let (pchk_offset, pchk_len) = pchk.unwrap_or((0, 0));

    Ok(NksFile {
        path: path.to_path_buf(),
        summary,
        plugin,
        controller,
        pchk_offset,
        pchk_len,
    })
}

//! Display command-stream encoder for the two 480×272 RGB565 screens.
//!
//! # Wire format
//!
//! Each display update is a single USB bulk transfer on interface 5,
//! endpoint `0x04`. The transfer begins with a 32-byte header followed by a
//! sequence of 8-byte command chunks, terminated by a flush and end command.
//!
//! ```text
//! bytes 0..16   header part A
//!   [0]  = 0x84
//!   [1]  = 0x00
//!   [2]  = display_id (0 = left, 1 = right)
//!   [3]  = 0x60
//!   [4..8]   = 0x00 × 4
//!   [8..10]  = 0x00 × 2
//!   [10..12] = 0x00 × 2
//!   [12..16] = 0x00 × 4
//! bytes 16..32  header part B — framed rect in big-endian u16
//!   [16..18] = x_start
//!   [18..20] = y_start
//!   [20..22] = width
//!   [22..24] = height
//!   [24..32] = 0x00 × 8
//!
//! commands (8 bytes each):
//!   0x00 n n n dddd      blit: next (n) × 2 pixels are in the payload
//!   0x01 n n n pppp      repeat: emit pixel pair `pppp` (n) times
//!   0x03 0 0 0 0 0 0 0   flush
//!   0x40 0 0 0 0 0 0 0   end of transmission
//! ```
//!
//! Source: Drachenkaetzchen/cabl doc/hardware/maschine-mk3/MaschineMK3-Display.md
//! cross-checked against ni-controllers-lib `LCDDisplays.paintDisplay`.

use crate::types::*;

/// Size of the fixed header prefix sent at the start of every display frame.
pub const DISPLAY_HEADER_LEN: usize = 32;
/// Size of one command chunk.
pub const DISPLAY_CMD_LEN: usize = 8;

/// Dirty rectangle to refresh. Coordinates in pixels; must satisfy
/// `x + w ≤ 480` and `y + h ≤ 272` and be even in `w` (the device transmits
/// two pixels at a time).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl Rect {
    pub const FULL: Self = Self { x: 0, y: 0, w: 480, h: 272 };

    pub const fn pixel_count(&self) -> usize {
        self.w as usize * self.h as usize
    }
}

/// Write the 32-byte frame header for a single display update.
pub fn encode_header(display: DisplayId, rect: Rect, out: &mut [u8]) -> Result<(), ProtoError> {
    if out.len() < DISPLAY_HEADER_LEN {
        return Err(ProtoError::OutputTooSmall { need: DISPLAY_HEADER_LEN, have: out.len() });
    }
    for b in &mut out[..DISPLAY_HEADER_LEN] {
        *b = 0;
    }
    out[0] = 0x84;
    out[1] = 0x00;
    out[2] = display as u8;
    out[3] = 0x60;
    out[16..18].copy_from_slice(&rect.x.to_be_bytes());
    out[18..20].copy_from_slice(&rect.y.to_be_bytes());
    out[20..22].copy_from_slice(&rect.w.to_be_bytes());
    out[22..24].copy_from_slice(&rect.h.to_be_bytes());
    Ok(())
}

/// Emit a "blit" command for `count` pixel-pairs, followed by that many
/// pixel-pair payload bytes. Returns the number of bytes written into `out`.
///
/// Pixels are in RGB565 **big-endian**. The caller is expected to provide
/// `pair_count * 2` u16 pixels through `pixels_be`.
///
/// `count` is encoded as a 24-bit big-endian integer in the command's low
/// three bytes.
pub fn encode_blit_cmd(pair_count: u32, out: &mut [u8]) -> Result<usize, ProtoError> {
    if out.len() < DISPLAY_CMD_LEN {
        return Err(ProtoError::OutputTooSmall { need: DISPLAY_CMD_LEN, have: out.len() });
    }
    // Opcode 0x00 + 24-bit big-endian count in bytes [1..4]. The remaining
    // four bytes of the 8-byte command are zero; pixel data follows separately.
    out[0] = 0x00;
    out[1] = ((pair_count >> 16) & 0xff) as u8;
    out[2] = ((pair_count >> 8) & 0xff) as u8;
    out[3] = (pair_count & 0xff) as u8;
    out[4..8].copy_from_slice(&[0; 4]);
    Ok(DISPLAY_CMD_LEN)
}

/// Emit a "repeat pixel pair" command: writes `(pair) × 2` pixels `count` times.
pub fn encode_repeat_cmd(count: u32, pair_be: [u8; 4], out: &mut [u8]) -> Result<usize, ProtoError> {
    if out.len() < DISPLAY_CMD_LEN {
        return Err(ProtoError::OutputTooSmall { need: DISPLAY_CMD_LEN, have: out.len() });
    }
    out[0] = 0x01;
    out[1] = ((count >> 16) & 0xff) as u8;
    out[2] = ((count >> 8) & 0xff) as u8;
    out[3] = (count & 0xff) as u8;
    out[4..8].copy_from_slice(&pair_be);
    Ok(DISPLAY_CMD_LEN)
}

/// Emit the flush opcode.
pub fn encode_flush_cmd(out: &mut [u8]) -> Result<usize, ProtoError> {
    if out.len() < DISPLAY_CMD_LEN {
        return Err(ProtoError::OutputTooSmall { need: DISPLAY_CMD_LEN, have: out.len() });
    }
    out[..DISPLAY_CMD_LEN].copy_from_slice(&[0x03, 0, 0, 0, 0, 0, 0, 0]);
    Ok(DISPLAY_CMD_LEN)
}

/// Emit the end-of-transmission opcode.
pub fn encode_end_cmd(out: &mut [u8]) -> Result<usize, ProtoError> {
    if out.len() < DISPLAY_CMD_LEN {
        return Err(ProtoError::OutputTooSmall { need: DISPLAY_CMD_LEN, have: out.len() });
    }
    out[..DISPLAY_CMD_LEN].copy_from_slice(&[0x40, 0, 0, 0, 0, 0, 0, 0]);
    Ok(DISPLAY_CMD_LEN)
}

/// Write a full-frame solid-color clear for one display into `out` (must be
/// `DISPLAY_HEADER_LEN + 3 × DISPLAY_CMD_LEN` = 56 bytes). Returns the byte
/// length written.
///
/// Uses a single `repeat` command to fill every pixel pair with `color`.
pub fn encode_solid_frame(display: DisplayId, color: Rgb, out: &mut [u8]) -> Result<usize, ProtoError> {
    let total = DISPLAY_HEADER_LEN + 3 * DISPLAY_CMD_LEN;
    if out.len() < total {
        return Err(ProtoError::OutputTooSmall { need: total, have: out.len() });
    }
    encode_header(display, Rect::FULL, out)?;
    let pixels = (DisplayId::PIXEL_COUNT / 2) as u32;
    let word = color.to_rgb565().to_be_bytes();
    let pair = [word[0], word[1], word[0], word[1]];
    let mut off = DISPLAY_HEADER_LEN;
    off += encode_repeat_cmd(pixels, pair, &mut out[off..])?;
    off += encode_flush_cmd(&mut out[off..])?;
    off += encode_end_cmd(&mut out[off..])?;
    Ok(off)
}

/// Convert a native-endian RGB565 row into a big-endian byte stream. Pass the
/// result as the payload following a [`encode_blit_cmd`] command.
pub fn pack_rgb565_be(native: &[u16], out: &mut [u8]) {
    debug_assert!(out.len() >= native.len() * 2);
    for (i, &w) in native.iter().enumerate() {
        let b = w.to_be_bytes();
        out[i * 2] = b[0];
        out[i * 2 + 1] = b[1];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_layout_matches_spec() {
        let mut buf = [0u8; DISPLAY_HEADER_LEN];
        encode_header(DisplayId::Right, Rect { x: 0x10, y: 0x20, w: 480, h: 272 }, &mut buf).unwrap();
        assert_eq!(buf[0], 0x84);
        assert_eq!(buf[1], 0x00);
        assert_eq!(buf[2], 1);
        assert_eq!(buf[3], 0x60);
        assert_eq!(&buf[16..18], &[0x00, 0x10]);
        assert_eq!(&buf[18..20], &[0x00, 0x20]);
        assert_eq!(&buf[20..22], &[0x01, 0xe0]);
        assert_eq!(&buf[22..24], &[0x01, 0x10]);
        // Everything else must be zero.
        for (i, &b) in buf.iter().enumerate() {
            if ![0, 1, 2, 3, 17, 19, 20, 21, 22, 23].contains(&i) {
                assert_eq!(b, 0, "byte {i} not zero: 0x{b:02x}");
            }
        }
    }

    #[test]
    fn solid_frame_uses_single_repeat() {
        let mut buf = [0u8; DISPLAY_HEADER_LEN + 3 * DISPLAY_CMD_LEN];
        let n = encode_solid_frame(DisplayId::Left, Rgb::new(0, 0xff, 0), &mut buf).unwrap();
        assert_eq!(n, buf.len());
        // Repeat opcode
        assert_eq!(buf[DISPLAY_HEADER_LEN], 0x01);
        // Pair count = 480*272/2 = 65280 = 0x00_FF_00
        assert_eq!(&buf[DISPLAY_HEADER_LEN + 1..DISPLAY_HEADER_LEN + 4], &[0x00, 0xff, 0x00]);
        // Pixel pair = green (0x07e0) big-endian, repeated
        assert_eq!(&buf[DISPLAY_HEADER_LEN + 4..DISPLAY_HEADER_LEN + 8], &[0x07, 0xe0, 0x07, 0xe0]);
        // Flush + end
        assert_eq!(buf[DISPLAY_HEADER_LEN + DISPLAY_CMD_LEN], 0x03);
        assert_eq!(buf[DISPLAY_HEADER_LEN + 2 * DISPLAY_CMD_LEN], 0x40);
    }

    #[test]
    fn blit_cmd_encodes_count() {
        let mut out = [0u8; DISPLAY_CMD_LEN];
        encode_blit_cmd(0xabcdef, &mut out).unwrap();
        assert_eq!(out, [0x00, 0xab, 0xcd, 0xef, 0, 0, 0, 0]);
    }

    #[test]
    fn pack_rgb565_is_big_endian() {
        let mut out = [0u8; 6];
        pack_rgb565_be(&[0x1234, 0xabcd, 0x0001], &mut out);
        assert_eq!(out, [0x12, 0x34, 0xab, 0xcd, 0x00, 0x01]);
    }
}

//! Turn a dirty framebuffer into a bulk-transfer payload.
//!
//! Strategy:
//! * Walk the dirty-tile tracker row-wise, coalescing adjacent dirty tiles
//!   into runs (from `DirtyTracker::runs`).
//! * For each run, emit a command block: 32-byte header locked to the run's
//!   bounding rectangle, followed by either a single `repeat` command (if
//!   the run is a solid color) or a `blit` command with the run's pixels.
//! * Always close the transfer with a `flush` + `end` command pair.
//!
//! After encoding, the tracker is cleared — callers can push the returned
//! bytes straight to the transport.

use maschine_proto::display::{
    encode_blit_cmd, encode_end_cmd, encode_flush_cmd, encode_header,
    encode_repeat_cmd, pack_rgb565_be, Rect, DISPLAY_CMD_LEN, DISPLAY_HEADER_LEN,
};
use maschine_proto::DisplayId;

use super::dirty::TILE;
use super::framebuffer::Framebuffer;

const SCREEN_W: usize = DisplayId::WIDTH as usize;
const SCREEN_H: usize = DisplayId::HEIGHT as usize;

pub fn encode_frame(id: DisplayId, fb: &mut Framebuffer) -> Vec<u8> {
    let runs = fb.dirty.runs();
    let mut out = Vec::<u8>::with_capacity(estimate_capacity(&runs));
    for (row, c0, c1) in runs {
        let y = row * TILE;
        let x = c0 * TILE;
        let h = TILE.min(SCREEN_H - y);
        let w = ((c1 - c0 + 1) * TILE).min(SCREEN_W - x);
        encode_run(id, fb, x, y, w, h, &mut out);
    }
    // Close the transfer.
    let mut tail = [0u8; DISPLAY_CMD_LEN * 2];
    let mut off = 0;
    off += encode_flush_cmd(&mut tail[off..]).unwrap();
    off += encode_end_cmd(&mut tail[off..]).unwrap();
    out.extend_from_slice(&tail[..off]);
    fb.dirty.clear();
    out
}

fn encode_run(
    id: DisplayId,
    fb: &Framebuffer,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    out: &mut Vec<u8>,
) {
    // Header
    let header_offset = out.len();
    out.resize(header_offset + DISPLAY_HEADER_LEN, 0);
    encode_header(
        id,
        Rect { x: x as u16, y: y as u16, w: w as u16, h: h as u16 },
        &mut out[header_offset..],
    ).expect("header buffer sized correctly");

    // Collect the rect's pixels into a contiguous Vec<u16> native-endian.
    let mut native = Vec::with_capacity(w * h);
    for yy in y..y + h {
        let row = &fb.pixels[yy * SCREEN_W + x..yy * SCREEN_W + x + w];
        native.extend_from_slice(row);
    }

    // If every pixel is identical, emit a single repeat command.
    let solid = native.iter().all(|&p| p == native[0]);
    if solid {
        let be = native[0].to_be_bytes();
        let mut cmd = [0u8; DISPLAY_CMD_LEN];
        // Pair count: total pixels / 2 (the rect is guaranteed even-width
        // because it's tile-aligned, and TILE=16).
        let pairs = (native.len() / 2) as u32;
        encode_repeat_cmd(pairs, [be[0], be[1], be[0], be[1]], &mut cmd).unwrap();
        out.extend_from_slice(&cmd);
        return;
    }

    // Blit: opcode + payload.
    let pairs = (native.len() / 2) as u32;
    let mut cmd = [0u8; DISPLAY_CMD_LEN];
    encode_blit_cmd(pairs, &mut cmd).unwrap();
    out.extend_from_slice(&cmd);
    let pay_len = native.len() * 2;
    let pay_offset = out.len();
    out.resize(pay_offset + pay_len, 0);
    pack_rgb565_be(&native, &mut out[pay_offset..]);
    // Payload must be a multiple of the 8-byte command granularity to keep
    // the stream framed. With `pairs` guaranteed even (tile-aligned) this
    // already holds — add an assertion to fail fast if a future caller
    // passes an odd-width rect.
    debug_assert_eq!(pay_len % DISPLAY_CMD_LEN, 0);
}

fn estimate_capacity(runs: &[(usize, usize, usize)]) -> usize {
    let mut n = 0usize;
    for (_, c0, c1) in runs {
        let tiles = c1 - c0 + 1;
        let pixels = tiles * TILE * TILE;
        n += DISPLAY_HEADER_LEN + DISPLAY_CMD_LEN + pixels * 2;
    }
    n + DISPLAY_CMD_LEN * 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use maschine_proto::Rgb;

    #[test]
    fn empty_frame_still_closes_transfer() {
        let mut fb = Framebuffer::new();
        // No touch() → no dirty tiles → only flush+end.
        let bytes = encode_frame(DisplayId::Left, &mut fb);
        assert_eq!(bytes.len(), DISPLAY_CMD_LEN * 2);
        assert_eq!(bytes[0], 0x03);
        assert_eq!(bytes[DISPLAY_CMD_LEN], 0x40);
    }

    #[test]
    fn solid_run_uses_repeat() {
        let mut fb = Framebuffer::new();
        fb.fill_rect(0, 0, 16, 16, Rgb::new(0, 0xff, 0));
        let bytes = encode_frame(DisplayId::Left, &mut fb);
        // header + 1 repeat cmd + flush + end = 32 + 8 + 8 + 8 = 56
        assert_eq!(bytes.len(), 56);
        assert_eq!(bytes[DISPLAY_HEADER_LEN], 0x01);
        // green = 0x07e0, big-endian
        assert_eq!(&bytes[DISPLAY_HEADER_LEN + 4..DISPLAY_HEADER_LEN + 8], &[0x07, 0xe0, 0x07, 0xe0]);
    }
}

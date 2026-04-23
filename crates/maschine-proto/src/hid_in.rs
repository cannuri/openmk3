//! HID input report parsers.
//!
//! The Mk3 emits two report types on interface #4's interrupt IN endpoint:
//!
//! * `0x01` — ~42-byte report carrying button bitmask, 8 macro encoder
//!   positions (10 bits each), master encoder absolute value, touch-strip
//!   position + pressure, and three analog volume knobs (mic/headphones/master).
//! * `0x02` — pad pressure stream, up to 16 pads per report; each pad entry
//!   is (index, u16 pressure).
//!
//! Byte layouts are adapted from the Drachenkaetzchen/cabl Mk3 docs and
//! cross-checked against asutherland/ni-controllers-lib
//! (`lib/maschine_mk3_config.json`). Fields we don't yet interpret are kept
//! around in the raw struct so consumers can experiment without re-opening
//! this parser.

use crate::types::*;

/// Decoded contents of a single control-report (`0x01`) frame.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ControlsReport {
    /// Raw 64-bit button bitmask. Meaning of each bit is stable per firmware
    /// but volatile across the Mk3 family; diff against previous frames via
    /// [`Self::buttons_diff`] rather than hard-coding bit positions for now.
    pub buttons: u64,
    /// 8 macro encoder absolute 10-bit positions. Encoders are endless, so
    /// treat these as wrapping: delta = (new - old + 0x400) % 0x400, then
    /// sign-extend for the short way around.
    pub macros: [u16; MACRO_ENCODER_COUNT],
    /// Master encoder absolute 10-bit position (same semantics).
    pub master: u16,
    /// Touch strip: (position 0..0x3ff, pressure 0..0x3ff). `None` when the
    /// strip is untouched.
    pub touch_strip: Option<(u16, u16)>,
    /// Mic gain / headphone volume / master volume analog knobs (10-bit each).
    pub mic_gain: u16,
    pub headphones: u16,
    pub master_vol: u16,
}

impl ControlsReport {
    /// Yield `(bit_index, now_pressed)` for every button whose state differs
    /// between `prev` and `self`.
    pub fn buttons_diff(&self, prev: u64) -> impl Iterator<Item = (u8, bool)> {
        let changed = self.buttons ^ prev;
        let now = self.buttons;
        (0..64u8).filter_map(move |i| {
            let mask = 1u64 << i;
            if changed & mask != 0 {
                Some((i, now & mask != 0))
            } else {
                None
            }
        })
    }

    /// Encoder rotation delta in ticks, sign-extended for the shortest path
    /// around the 10-bit wrap. Positive = clockwise.
    pub fn encoder_delta(prev: u16, now: u16) -> i16 {
        let raw = (now as i32 - prev as i32) & 0x3ff;
        let delta = if raw > 0x200 { raw - 0x400 } else { raw };
        delta as i16
    }
}

/// One pad event extracted from a `0x02` report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PadSample {
    pub pad: u8,
    /// Raw pressure; 0x0000 at rest, ~0x4000..=0x4FFD when pressed.
    pub pressure: u16,
}

/// Pad report contents: up to 16 pad samples in arbitrary order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PadsReport {
    pub samples: Vec<PadSample>,
}

/// Either kind of report, tagged by the first byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InReport {
    Controls(ControlsReport),
    Pads(PadsReport),
}

/// Parse a raw HID IN transfer into an [`InReport`]. The byte at index 0 is
/// the HID report id.
pub fn parse(buf: &[u8]) -> Result<InReport, ProtoError> {
    if buf.is_empty() {
        return Err(ProtoError::Short { need: 1, have: 0 });
    }
    match buf[0] {
        REPORT_IN_CONTROLS => parse_controls(buf).map(InReport::Controls),
        REPORT_IN_PADS => parse_pads(buf).map(InReport::Pads),
        other => Err(ProtoError::UnknownReport(other)),
    }
}

fn u16_le(hi: u8, lo: u8) -> u16 {
    // The Mk3 encodes its 10-bit analog values as LSB-first byte pairs, not
    // the big-endian framing used by its RGB displays.
    u16::from_le_bytes([hi, lo])
}

fn parse_controls(buf: &[u8]) -> Result<ControlsReport, ProtoError> {
    // Minimum sane length: report id + 8 button bytes + 9 × u16 + strip + analogs.
    // We read defensively and tolerate a variable tail length because firmware
    // revisions have been observed to add padding.
    const MIN_LEN: usize = 1 + 8 + 2 * 9 + 4 + 6;
    if buf.len() < MIN_LEN {
        return Err(ProtoError::Short { need: MIN_LEN, have: buf.len() });
    }
    // [1..9] = button bitmask, little-endian u64
    let mut btn = [0u8; 8];
    btn.copy_from_slice(&buf[1..9]);
    let buttons = u64::from_le_bytes(btn);

    // [9..25] = 8 × u16 macro encoders
    let mut macros = [0u16; MACRO_ENCODER_COUNT];
    for (i, m) in macros.iter_mut().enumerate() {
        let off = 9 + i * 2;
        *m = u16_le(buf[off], buf[off + 1]) & 0x3ff;
    }
    // [25..27] = master encoder
    let master = u16_le(buf[25], buf[26]) & 0x3ff;
    // [27..29] = touch strip position, [29..31] = touch strip pressure
    let strip_pos = u16_le(buf[27], buf[28]) & 0x3ff;
    let strip_prs = u16_le(buf[29], buf[30]) & 0x3ff;
    let touch_strip = if strip_prs == 0 { None } else { Some((strip_pos, strip_prs)) };
    // [31..37] = mic / headphones / master_vol
    let mic_gain = u16_le(buf[31], buf[32]) & 0x3ff;
    let headphones = u16_le(buf[33], buf[34]) & 0x3ff;
    let master_vol = u16_le(buf[35], buf[36]) & 0x3ff;

    Ok(ControlsReport {
        buttons,
        macros,
        master,
        touch_strip,
        mic_gain,
        headphones,
        master_vol,
    })
}

fn parse_pads(buf: &[u8]) -> Result<PadsReport, ProtoError> {
    // Report body after the id is a sequence of 3-byte entries: pad_index, lo, hi.
    // Unused trailing slots are zeroed.
    let body = &buf[1..];
    let mut samples = Vec::with_capacity(PAD_COUNT);
    for chunk in body.chunks_exact(3) {
        let pad = chunk[0];
        let pressure = u16_le(chunk[1], chunk[2]);
        if pad == 0 && pressure == 0 {
            continue; // empty slot
        }
        if pad as usize >= PAD_COUNT {
            return Err(ProtoError::BadPad(pad));
        }
        samples.push(PadSample { pad, pressure });
    }
    Ok(PadsReport { samples })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_controls_report() {
        // Construct a minimal controls frame with buttons bit 0 and 5 set,
        // macro 3 = 0x123, master = 0x200, touch strip touched at pos=0x0ff pres=0x10,
        // analog knobs at distinct values.
        let mut buf = [0u8; 48];
        buf[0] = REPORT_IN_CONTROLS;
        buf[1] = 0b0010_0001;
        let put16 = |b: &mut [u8], off: usize, v: u16| {
            b[off] = (v & 0xff) as u8;
            b[off + 1] = (v >> 8) as u8;
        };
        put16(&mut buf, 9 + 3 * 2, 0x123);
        put16(&mut buf, 25, 0x200);
        put16(&mut buf, 27, 0x0ff);
        put16(&mut buf, 29, 0x010);
        put16(&mut buf, 31, 0x111);
        put16(&mut buf, 33, 0x222);
        put16(&mut buf, 35, 0x333);

        let r = parse(&buf).unwrap();
        let c = match r { InReport::Controls(c) => c, _ => panic!() };
        assert_eq!(c.buttons & 0x21, 0x21);
        assert_eq!(c.macros[3], 0x123);
        assert_eq!(c.master, 0x200);
        assert_eq!(c.touch_strip, Some((0x0ff, 0x010)));
        assert_eq!(c.mic_gain, 0x111);
        assert_eq!(c.headphones, 0x222);
        assert_eq!(c.master_vol, 0x333);
    }

    #[test]
    fn parses_pads_report_and_rejects_bad_index() {
        let mut buf = vec![REPORT_IN_PADS];
        buf.extend_from_slice(&[5, 0x34, 0x42]); // pad 5 pressure 0x4234
        buf.extend_from_slice(&[0, 0, 0]);       // ignored empty slot
        buf.extend_from_slice(&[15, 0x00, 0x40]); // pad 15 pressure 0x4000
        let r = parse(&buf).unwrap();
        let p = match r { InReport::Pads(p) => p, _ => panic!() };
        assert_eq!(p.samples.len(), 2);
        assert_eq!(p.samples[0], PadSample { pad: 5, pressure: 0x4234 });
        assert_eq!(p.samples[1], PadSample { pad: 15, pressure: 0x4000 });

        // A pad index of 16 must be rejected.
        let bad = [REPORT_IN_PADS, 16, 0x00, 0x40];
        assert!(parse(&bad).is_err());
    }

    #[test]
    fn encoder_delta_wraps_both_ways() {
        assert_eq!(ControlsReport::encoder_delta(0x3ff, 0x001), 2);
        assert_eq!(ControlsReport::encoder_delta(0x001, 0x3ff), -2);
        assert_eq!(ControlsReport::encoder_delta(0x100, 0x100), 0);
        assert_eq!(ControlsReport::encoder_delta(0x100, 0x110), 0x10);
    }

    #[test]
    fn button_diff_reports_only_changed() {
        let c = ControlsReport { buttons: 0b0110, ..Default::default() };
        let diff: Vec<_> = c.buttons_diff(0b0101).collect();
        assert_eq!(diff, vec![(0, false), (1, true)]);
    }
}

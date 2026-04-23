//! Byte-level protocol regressions.
//!
//! These assert the exact output of our encoders against hand-checked
//! reference byte sequences. If we ever accidentally byte-swap RGB, shift an
//! offset, or miscode a command opcode, these break before the hardware
//! comes out of the box.

use maschine_proto::display::{encode_solid_frame, Rect, DISPLAY_CMD_LEN, DISPLAY_HEADER_LEN};
use maschine_proto::hid_in::{parse, InReport, PadSample};
use maschine_proto::hid_out::{encode_button_leds, encode_pad_leds, encode_pads_solid, BUTTON_LED_SLOTS, PAD_LED_PAYLOAD};
use maschine_proto::{DisplayId, Rgb, PAD_COUNT, TOUCHSTRIP_LED_COUNT};

#[test]
fn golden_solid_green_frame() {
    let mut buf = [0u8; DISPLAY_HEADER_LEN + 3 * DISPLAY_CMD_LEN];
    encode_solid_frame(DisplayId::Right, Rgb::new(0, 0xff, 0), &mut buf).unwrap();
    // Full reference bytes for the 56-byte transfer.
    let expected: &[u8] = &[
        // header
        0x84, 0x00, 0x01, 0x60, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0,
        0x00, 0x00,  // x
        0x00, 0x00,  // y
        0x01, 0xe0,  // w = 480
        0x01, 0x10,  // h = 272
        0, 0, 0, 0, 0, 0, 0, 0,
        // repeat opcode + 3-byte pair count (480*272/2 = 65280 = 0x00ff00)
        0x01, 0x00, 0xff, 0x00, 0x07, 0xe0, 0x07, 0xe0,
        // flush
        0x03, 0, 0, 0, 0, 0, 0, 0,
        // end
        0x40, 0, 0, 0, 0, 0, 0, 0,
    ];
    assert_eq!(&buf[..], expected);
}

#[test]
fn pads_solid_layout() {
    let mut out = [0u8; 1 + PAD_LED_PAYLOAD];
    encode_pads_solid(Rgb::new(0x10, 0x20, 0x30), &mut out).unwrap();
    assert_eq!(out[0], 0x81);
    // First TOUCHSTRIP_LED_COUNT RGB triples are black (report-header side).
    for i in 0..TOUCHSTRIP_LED_COUNT {
        let off = 1 + i * 3;
        assert_eq!(&out[off..off + 3], &[0, 0, 0], "strip LED {i}");
    }
    // Every pad carries the solid color.
    for p in 0..PAD_COUNT {
        let off = 1 + TOUCHSTRIP_LED_COUNT * 3 + p * 3;
        assert_eq!(&out[off..off + 3], &[0x10, 0x20, 0x30]);
    }
}

#[test]
fn button_leds_only_touch_requested_slots() {
    let mut vals = [0u8; BUTTON_LED_SLOTS];
    vals[7] = 0x55;
    vals[30] = 0x7f;
    let mut out = [0u8; 1 + BUTTON_LED_SLOTS];
    encode_button_leds(&vals, &mut out).unwrap();
    assert_eq!(out[0], 0x80);
    assert_eq!(out[1 + 7], 0x55);
    assert_eq!(out[1 + 30], 0x7f);
    assert_eq!(out[1], 0);
    assert_eq!(out[1 + 29], 0);
}

#[test]
fn round_trip_strip_and_pad_grid() {
    let mut strip = [Rgb::BLACK; TOUCHSTRIP_LED_COUNT];
    for (i, c) in strip.iter_mut().enumerate() {
        *c = Rgb::new(i as u8 * 10, 0, 0);
    }
    let mut pads = [Rgb::BLACK; PAD_COUNT];
    for (i, c) in pads.iter_mut().enumerate() {
        *c = Rgb::new(0, i as u8 * 16, i as u8 * 8);
    }
    let mut out = [0u8; 1 + PAD_LED_PAYLOAD];
    encode_pad_leds(&strip, &pads, &mut out).unwrap();
    for i in 0..TOUCHSTRIP_LED_COUNT {
        let off = 1 + i * 3;
        assert_eq!(out[off], (i as u8) * 10);
    }
    for p in 0..PAD_COUNT {
        let off = 1 + TOUCHSTRIP_LED_COUNT * 3 + p * 3;
        assert_eq!(out[off + 1], (p as u8) * 16);
        assert_eq!(out[off + 2], (p as u8) * 8);
    }
}

#[test]
fn parse_rejects_empty_and_unknown_reports() {
    assert!(parse(&[]).is_err());
    assert!(parse(&[0x99, 0, 0, 0]).is_err());
}

#[test]
fn parse_pads_tolerates_multiple_frames_sequentially() {
    // Two pad events in one report.
    let mut buf = vec![0x02];
    buf.extend_from_slice(&[3, 0x00, 0x42]);   // pad 3, 0x4200
    buf.extend_from_slice(&[9, 0xFD, 0x4F]);   // pad 9, 0x4FFD (max)
    buf.extend_from_slice(&[0, 0, 0]);         // empty trailing slots…
    buf.extend_from_slice(&[0, 0, 0]);
    let r = parse(&buf).unwrap();
    match r {
        InReport::Pads(p) => {
            assert_eq!(p.samples.len(), 2);
            assert_eq!(p.samples[0], PadSample { pad: 3, pressure: 0x4200 });
            assert_eq!(p.samples[1], PadSample { pad: 9, pressure: 0x4FFD });
        }
        _ => panic!(),
    }
}

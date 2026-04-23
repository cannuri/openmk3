//! HID output report encoders.
//!
//! Two output reports drive every LED on the Mk3:
//!
//! * `0x80` — 1-byte-per-LED array for buttons, encoder rings, and the group
//!   A..H RGB LEDs packed into successive slots. Most LEDs are monochromatic
//!   "intensity 0..127"; group buttons occupy 3 consecutive slots (r, g, b).
//! * `0x81` — touch-strip RGB array (25 × 3 bytes) followed by the 16 pad
//!   RGB colors (16 × 3 bytes).
//!
//! The exact bit-layout inside each LED byte is firmware-sensitive; the
//! documented convention is:
//!   * monochrome LEDs: `0b0???_iiii` where `iiii` is intensity 0..15, and
//!     bit 6 is "dim" vs "bright". In practice, writing the raw 0..127 range
//!     works across firmware revisions — we pass it through unchanged and
//!     leave calibration to the caller.
//!   * RGB pads use the packed byte `0bppccc_bbb` documented in cabl, where
//!     the top 2 bits are brightness (0..3) and the next 6 encode a 64-hue
//!     palette. We hide that behind [`Rgb`] → [`encode_pad_color`], which
//!     picks the nearest palette entry.

use crate::types::*;

/// Number of monochrome LED slots driven by the `0x80` report.
///
/// Enough for every transport/mode/workflow button plus the group LED block
/// plus the encoder ring LEDs. The device silently ignores trailing bytes we
/// don't write, so treating this as a fixed upper bound is safe.
pub const BUTTON_LED_SLOTS: usize = 62;

/// Length of the pad-LED report payload (excluding the report id).
pub const PAD_LED_PAYLOAD: usize = TOUCHSTRIP_LED_COUNT * 3 + PAD_COUNT * 3;

/// Encode the button/encoder-LED report (`0x80`).
///
/// `values` is a fully-populated array of intensity bytes, one per LED slot.
/// Returns the output slice length (including the leading report id).
pub fn encode_button_leds(values: &[u8; BUTTON_LED_SLOTS], out: &mut [u8]) -> Result<usize, ProtoError> {
    let need = 1 + BUTTON_LED_SLOTS;
    if out.len() < need {
        return Err(ProtoError::OutputTooSmall { need, have: out.len() });
    }
    out[0] = REPORT_OUT_BUTTON_LEDS;
    out[1..1 + BUTTON_LED_SLOTS].copy_from_slice(values);
    Ok(need)
}

/// Encode the pad + touch-strip LED report (`0x81`).
///
/// * `strip` — 25 RGB values, index 0 = leftmost LED.
/// * `pads`  — 16 RGB values, index 0 = top-left pad, row-major.
pub fn encode_pad_leds(
    strip: &[Rgb; TOUCHSTRIP_LED_COUNT],
    pads: &[Rgb; PAD_COUNT],
    out: &mut [u8],
) -> Result<usize, ProtoError> {
    let need = 1 + PAD_LED_PAYLOAD;
    if out.len() < need {
        return Err(ProtoError::OutputTooSmall { need, have: out.len() });
    }
    out[0] = REPORT_OUT_PAD_LEDS;
    let mut i = 1;
    for c in strip {
        out[i] = c.r; out[i + 1] = c.g; out[i + 2] = c.b;
        i += 3;
    }
    for c in pads {
        out[i] = c.r; out[i + 1] = c.g; out[i + 2] = c.b;
        i += 3;
    }
    Ok(need)
}

/// Convenience: build a pad-LED report setting all pads to a single color.
pub fn encode_pads_solid(color: Rgb, out: &mut [u8]) -> Result<usize, ProtoError> {
    let strip = [Rgb::BLACK; TOUCHSTRIP_LED_COUNT];
    let pads = [color; PAD_COUNT];
    encode_pad_leds(&strip, &pads, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_leds_write_report_id_and_payload() {
        let mut vals = [0u8; BUTTON_LED_SLOTS];
        vals[0] = 0x7f;
        vals[10] = 0x40;
        let mut out = [0u8; 1 + BUTTON_LED_SLOTS];
        let n = encode_button_leds(&vals, &mut out).unwrap();
        assert_eq!(n, out.len());
        assert_eq!(out[0], REPORT_OUT_BUTTON_LEDS);
        assert_eq!(out[1], 0x7f);
        assert_eq!(out[11], 0x40);
    }

    #[test]
    fn pad_leds_layout_is_strip_then_pads() {
        let strip = core::array::from_fn::<Rgb, TOUCHSTRIP_LED_COUNT, _>(|_| Rgb::BLACK);
        let pads = core::array::from_fn::<Rgb, PAD_COUNT, _>(|i| Rgb::new(i as u8, 0, 0));
        let mut out = [0u8; 1 + PAD_LED_PAYLOAD];
        let n = encode_pad_leds(&strip, &pads, &mut out).unwrap();
        assert_eq!(n, out.len());
        assert_eq!(out[0], REPORT_OUT_PAD_LEDS);
        let first_pad_off = 1 + TOUCHSTRIP_LED_COUNT * 3;
        assert_eq!(out[first_pad_off], 0);
        assert_eq!(out[first_pad_off + 3], 1);
        assert_eq!(out[first_pad_off + 6], 2);
    }

    #[test]
    fn output_too_small_errors() {
        let vals = [0u8; BUTTON_LED_SLOTS];
        let mut tiny = [0u8; 4];
        assert!(matches!(
            encode_button_leds(&vals, &mut tiny),
            Err(ProtoError::OutputTooSmall { .. })
        ));
    }
}

//! Shared identifier types for the Maschine Mk3 protocol.
//!
//! Coordinates and button names follow the physical layout of the device as
//! documented in the Drachenkaetzchen/cabl Mk3 reference
//! (`doc/hardware/maschine-mk3/MaschineMK3-HIDInput.md`).

use core::fmt;

/// Native Instruments USB vendor id.
pub const VID_NI: u16 = 0x17cc;
/// Maschine Mk3 USB product id.
pub const PID_MK3: u16 = 0x1600;

/// HID interface (controls + LEDs).
pub const IFACE_HID: u8 = 4;
/// Vendor-defined bulk interface (displays).
pub const IFACE_DISPLAY: u8 = 5;
/// Bulk OUT endpoint used for display pixel writes.
pub const EP_DISPLAY_OUT: u8 = 0x04;

/// Incoming HID report: buttons + encoders + touch strip + volume knobs.
pub const REPORT_IN_CONTROLS: u8 = 0x01;
/// Incoming HID report: pad pressure stream.
pub const REPORT_IN_PADS: u8 = 0x02;
/// Outgoing HID report: button/encoder LED values.
pub const REPORT_OUT_BUTTON_LEDS: u8 = 0x80;
/// Outgoing HID report: pad RGB + touch strip LEDs.
pub const REPORT_OUT_PAD_LEDS: u8 = 0x81;

/// Pad identifier (0..16). Pad 0 is the top-left pad; index increments
/// left-to-right, bottom-to-top (standard Maschine convention).
pub type Pad = u8;
/// Number of RGB pads on the Mk3.
pub const PAD_COUNT: usize = 16;
/// Number of RGB LEDs on the touch strip.
pub const TOUCHSTRIP_LED_COUNT: usize = 25;
/// Number of rotary encoders under the displays (macro knobs).
pub const MACRO_ENCODER_COUNT: usize = 8;

/// Which of the two color displays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum DisplayId {
    Left = 0,
    Right = 1,
}

impl DisplayId {
    pub const WIDTH: u16 = 480;
    pub const HEIGHT: u16 = 272;
    pub const PIXEL_COUNT: usize = Self::WIDTH as usize * Self::HEIGHT as usize;
}

/// Logical button identifiers. Byte offsets in the HID input report come from
/// the cabl-fork reference; see [`ButtonBit`] for the exact mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Button {
    // Transport / top row
    Play, Rec, Stop, Restart, Erase, Tap, Follow,
    // Workflow
    Shift, Fixed, Pad,
    // Mode row
    Scene, Pattern, Events, Variation, Duplicate, Select, Solo, Mute,
    // Groups A..H (RGB, addressable separately for color)
    Group(u8),
    // Perform / edit
    Pitch, Mod, Perform, Notes,
    // Browser / transport-context
    Browser, Plugin, Mixer, Channel, Arranger, Sampling,
    // Navigation cluster
    MacroSet, Volume, Swing, Tempo,
    // Softkeys under displays (8 beneath each display = 16, "DisplayButton(0..16)")
    DisplayButton(u8),
    // Touch strip capacitive touch (treated as a button edge)
    TouchStripTouch,
    // Encoder push / touch states
    EncoderPush(u8),
    EncoderTouch(u8),
    // Master encoder (the big one in the middle)
    MasterEncoderPush,
    MasterEncoderTouch,
    // Step-sequencer-ish
    Auto, Lock, NoteRepeat,
    // Pagination
    Left, Right,
}

/// Identifier for a macro encoder (the 8 knobs beneath the displays).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MacroEncoder(pub u8);

/// Identifier for the single large master encoder (the wheel).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MasterEncoder;

/// RGB color as 8-bit components. Not the wire format; the encoder converts
/// this to the device's native palette/brightness representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const BLACK: Self = Self { r: 0, g: 0, b: 0 };
    pub const WHITE: Self = Self { r: 0xff, g: 0xff, b: 0xff };

    pub const fn new(r: u8, g: u8, b: u8) -> Self { Self { r, g, b } }

    /// Encode to the Mk3's packed RGB565 word (big-endian over the wire).
    /// Stored here in native endian — byte-swapped by the display encoder.
    pub const fn to_rgb565(self) -> u16 {
        let r = (self.r as u16 >> 3) & 0x1f;
        let g = (self.g as u16 >> 2) & 0x3f;
        let b = (self.b as u16 >> 3) & 0x1f;
        (r << 11) | (g << 5) | b
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

/// Errors raised by protocol parsers and encoders.
#[derive(Debug, thiserror::Error)]
pub enum ProtoError {
    #[error("input too short: need {need} bytes, have {have}")]
    Short { need: usize, have: usize },
    #[error("unexpected report id 0x{0:02x}")]
    UnknownReport(u8),
    #[error("invalid pad index {0}")]
    BadPad(u8),
    #[error("invalid encoder index {0}")]
    BadEncoder(u8),
    #[error("output buffer too small: need {need}, have {have}")]
    OutputTooSmall { need: usize, have: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb565_red_is_top_bits() {
        assert_eq!(Rgb::new(0xff, 0, 0).to_rgb565(), 0xf800);
        assert_eq!(Rgb::new(0, 0xff, 0).to_rgb565(), 0x07e0);
        assert_eq!(Rgb::new(0, 0, 0xff).to_rgb565(), 0x001f);
        assert_eq!(Rgb::BLACK.to_rgb565(), 0x0000);
        assert_eq!(Rgb::WHITE.to_rgb565(), 0xffff);
    }
}

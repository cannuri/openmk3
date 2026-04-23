//! Pure wire-format parsers and encoders for the Native Instruments Maschine
//! Mk3 USB/HID protocol. No I/O, no allocations on the hot path beyond
//! parsing results.
//!
//! Submodules:
//! * [`types`] — identifiers, constants, `Rgb`, error enum.
//! * [`hid_in`] — parse HID IN reports (pads, buttons, encoders, touch strip).
//! * [`hid_out`] — encode HID OUT reports (button/pad LEDs).
//! * [`display`] — encode display bulk-transfer command streams.

pub mod types;
pub mod hid_in;
pub mod hid_out;
pub mod display;

pub use types::{
    Button, DisplayId, MacroEncoder, MasterEncoder, Pad, ProtoError, Rgb,
    EP_DISPLAY_OUT, IFACE_DISPLAY, IFACE_HID, MACRO_ENCODER_COUNT, PAD_COUNT, PID_MK3,
    REPORT_IN_CONTROLS, REPORT_IN_PADS, REPORT_OUT_BUTTON_LEDS, REPORT_OUT_PAD_LEDS,
    TOUCHSTRIP_LED_COUNT, VID_NI,
};

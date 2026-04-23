//! USB/HID transport, event loop, and display pipeline for the Mk3.

pub mod transport;
pub mod device;
pub mod event;
pub mod display;
pub mod platform;

pub use device::{Maschine, OpenOptions};
pub use event::{Event, EventStream, PadPhase};

//! Public event model.

use futures::Stream;
use std::pin::Pin;

use maschine_proto as proto;

/// Lifecycle phase of a pad press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadPhase {
    /// First frame crossing the on-threshold. Carries a velocity hint.
    Attack,
    /// Pressure change while held (rate-limited, jitter-filtered).
    Pressure,
    /// Crossed back below the off-threshold.
    Release,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Pad { pad: u8, pressure: u16, velocity: Option<u8>, phase: PadPhase },
    Button { bit: u8, pressed: bool },
    MacroEncoder { index: u8, delta: i16, absolute: u16 },
    MasterEncoder { delta: i16, absolute: u16 },
    TouchStrip { position: u16, pressure: u16 },
    TouchStripReleased,
    Analog { which: AnalogKnob, value: u16 },
    Raw(proto::hid_in::InReport),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalogKnob {
    MicGain,
    Headphones,
    MasterVolume,
}

pub type EventStream = Pin<Box<dyn Stream<Item = Event> + Send>>;

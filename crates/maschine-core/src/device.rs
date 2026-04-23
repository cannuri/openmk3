//! High-level `Maschine` handle: assembles transport + parser + event stream.

use std::pin::Pin;
use std::sync::Arc;

use futures::Stream;
use tokio::sync::{mpsc, Mutex};

use maschine_proto as proto;
use proto::hid_in::{ControlsReport, InReport, PadSample};
use proto::hid_out::{BUTTON_LED_SLOTS, PAD_LED_PAYLOAD};

use crate::event::{AnalogKnob, Event, PadPhase};
use crate::transport::{Transport, TransportError};

/// Tunable thresholds and behavior at open time.
#[derive(Debug, Clone)]
pub struct OpenOptions {
    /// Raw pressure below which a pad is considered released.
    pub pad_off_threshold: u16,
    /// Raw pressure at which we emit a new `Attack`.
    pub pad_on_threshold: u16,
    /// Minimum absolute pressure delta between consecutive `Pressure`
    /// events. Filters ADC jitter.
    pub pad_pressure_step: u16,
    /// If `true`, every pad sample is emitted as an `Event::Pad` without
    /// hysteresis or debouncing. Lets clients build custom velocity curves.
    pub raw_pad_stream: bool,
    /// Event channel capacity before we start dropping pressures.
    pub event_capacity: usize,
}

impl Default for OpenOptions {
    fn default() -> Self {
        Self {
            pad_off_threshold: 0x4005,
            pad_on_threshold: 0x4010,
            pad_pressure_step: 3,
            raw_pad_stream: false,
            event_capacity: 1024,
        }
    }
}

/// Opened Mk3 handle.
pub struct Maschine {
    transport: Arc<Transport>,
    events: Mutex<Option<mpsc::Receiver<Event>>>,
    pub options: OpenOptions,
}

impl Maschine {
    pub async fn open() -> Result<Self, TransportError> {
        Self::open_with(OpenOptions::default()).await
    }

    pub async fn open_with(options: OpenOptions) -> Result<Self, TransportError> {
        let transport = Arc::new(Transport::open().await?);
        let raw_rx = transport.spawn_hid_reader();
        let (evt_tx, evt_rx) = mpsc::channel::<Event>(options.event_capacity);
        spawn_diff_task(raw_rx, evt_tx, options.clone());
        Ok(Self {
            transport,
            events: Mutex::new(Some(evt_rx)),
            options,
        })
    }

    /// Consume the event stream. Only callable once; subsequent calls return
    /// `None`.
    pub async fn take_events(&self) -> Option<Pin<Box<dyn Stream<Item = Event> + Send>>> {
        let rx = self.events.lock().await.take()?;
        Some(Box::pin(ReceiverStream::new(rx)))
    }

    /// Write raw HID output bytes (including the report id).
    pub async fn write_hid(&self, bytes: Vec<u8>) -> Result<(), TransportError> {
        self.transport.write_hid(bytes).await
    }

    /// Convenience: set every pad to the same color and every strip LED off.
    pub async fn solid_pads(&self, color: proto::Rgb) -> Result<(), TransportError> {
        let mut buf = vec![0u8; 1 + PAD_LED_PAYLOAD];
        proto::hid_out::encode_pads_solid(color, &mut buf).unwrap();
        self.write_hid(buf).await
    }

    /// Convenience: write a fully-specified pad LED frame.
    pub async fn set_pads(
        &self,
        strip: &[proto::Rgb; proto::TOUCHSTRIP_LED_COUNT],
        pads: &[proto::Rgb; proto::PAD_COUNT],
    ) -> Result<(), TransportError> {
        let mut buf = vec![0u8; 1 + PAD_LED_PAYLOAD];
        proto::hid_out::encode_pad_leds(strip, pads, &mut buf).unwrap();
        self.write_hid(buf).await
    }

    /// Set every button/encoder-ring LED intensity at once.
    pub async fn set_button_leds(&self, values: &[u8; BUTTON_LED_SLOTS]) -> Result<(), TransportError> {
        let mut buf = vec![0u8; 1 + BUTTON_LED_SLOTS];
        proto::hid_out::encode_button_leds(values, &mut buf).unwrap();
        self.write_hid(buf).await
    }

    /// Push a prepared display command stream.
    pub async fn write_display(&self, bytes: Vec<u8>) -> Result<(), TransportError> {
        self.transport.write_display(bytes).await
    }

    /// Expose the transport so higher-level modules can spawn display tasks.
    pub fn transport(&self) -> Arc<Transport> {
        self.transport.clone()
    }
}

/// State held between frames so we can emit edges instead of duplicates.
#[derive(Default)]
struct DiffState {
    controls: Option<ControlsReport>,
    pad_pressures: [u16; proto::PAD_COUNT],
    pad_on: [bool; proto::PAD_COUNT],
    pad_peak_since_attack: [u16; proto::PAD_COUNT],
}

fn spawn_diff_task(
    mut raw_rx: mpsc::Receiver<Vec<u8>>,
    tx: mpsc::Sender<Event>,
    opts: OpenOptions,
) {
    tokio::spawn(async move {
        let mut state = DiffState::default();
        while let Some(raw) = raw_rx.recv().await {
            let parsed = match proto::hid_in::parse(&raw) {
                Ok(p) => p,
                Err(e) => {
                    tracing::debug!("drop unparsable HID IN frame: {e}");
                    continue;
                }
            };
            match parsed {
                InReport::Controls(c) => emit_controls(&mut state, c, &tx).await,
                InReport::Pads(p) => emit_pads(&mut state, p, &opts, &tx).await,
            }
            if tx.is_closed() { break; }
        }
    });
}

async fn emit_controls(state: &mut DiffState, c: ControlsReport, tx: &mpsc::Sender<Event>) {
    let prev = state.controls.clone().unwrap_or_default();
    for (bit, pressed) in c.buttons_diff(prev.buttons) {
        let _ = tx.send(Event::Button { bit, pressed }).await;
    }
    for (i, (&now, &was)) in c.macros.iter().zip(prev.macros.iter()).enumerate() {
        let delta = ControlsReport::encoder_delta(was, now);
        if delta != 0 {
            let _ = tx.send(Event::MacroEncoder { index: i as u8, delta, absolute: now }).await;
        }
    }
    let mdelta = ControlsReport::encoder_delta(prev.master, c.master);
    if mdelta != 0 {
        let _ = tx.send(Event::MasterEncoder { delta: mdelta, absolute: c.master }).await;
    }
    match (prev.touch_strip, c.touch_strip) {
        (_, Some((p, pr))) => { let _ = tx.send(Event::TouchStrip { position: p, pressure: pr }).await; }
        (Some(_), None) => { let _ = tx.send(Event::TouchStripReleased).await; }
        _ => {}
    }
    if prev.mic_gain != c.mic_gain {
        let _ = tx.send(Event::Analog { which: AnalogKnob::MicGain, value: c.mic_gain }).await;
    }
    if prev.headphones != c.headphones {
        let _ = tx.send(Event::Analog { which: AnalogKnob::Headphones, value: c.headphones }).await;
    }
    if prev.master_vol != c.master_vol {
        let _ = tx.send(Event::Analog { which: AnalogKnob::MasterVolume, value: c.master_vol }).await;
    }
    state.controls = Some(c);
}

async fn emit_pads(
    state: &mut DiffState,
    pads: proto::hid_in::PadsReport,
    opts: &OpenOptions,
    tx: &mpsc::Sender<Event>,
) {
    for PadSample { pad, pressure } in pads.samples {
        let i = pad as usize;
        if opts.raw_pad_stream {
            let _ = tx.send(Event::Pad { pad, pressure, velocity: None, phase: PadPhase::Pressure }).await;
            state.pad_pressures[i] = pressure;
            continue;
        }
        let was_on = state.pad_on[i];
        if !was_on && pressure >= opts.pad_on_threshold {
            state.pad_on[i] = true;
            state.pad_peak_since_attack[i] = pressure;
            let v = pressure_to_velocity(pressure);
            let _ = tx.send(Event::Pad { pad, pressure, velocity: Some(v), phase: PadPhase::Attack }).await;
        } else if was_on && pressure <= opts.pad_off_threshold {
            state.pad_on[i] = false;
            let _ = tx.send(Event::Pad { pad, pressure, velocity: None, phase: PadPhase::Release }).await;
        } else if was_on {
            if pressure > state.pad_peak_since_attack[i] {
                state.pad_peak_since_attack[i] = pressure;
            }
            let diff = pressure.abs_diff(state.pad_pressures[i]);
            if diff >= opts.pad_pressure_step {
                let _ = tx.send(Event::Pad { pad, pressure, velocity: None, phase: PadPhase::Pressure }).await;
            }
        }
        state.pad_pressures[i] = pressure;
    }
}

fn pressure_to_velocity(p: u16) -> u8 {
    let base = 0x4000u16;
    let range = 0x0FFDu16;
    let clipped = p.saturating_sub(base).min(range);
    ((clipped as u32 * 127) / range as u32) as u8
}

// -------- ReceiverStream shim (avoids pulling in tokio-stream dep) --------
struct ReceiverStream<T> {
    rx: mpsc::Receiver<T>,
}
impl<T> ReceiverStream<T> {
    fn new(rx: mpsc::Receiver<T>) -> Self { Self { rx } }
}
impl<T> Stream for ReceiverStream<T> {
    type Item = T;
    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<T>> {
        self.rx.poll_recv(cx)
    }
}

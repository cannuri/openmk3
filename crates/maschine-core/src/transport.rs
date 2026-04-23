//! USB transport abstraction.
//!
//! On macOS, nusb-based exclusive USB access to the Mk3's non-audio
//! interfaces is blocked by Interface-Association-Descriptor matching in
//! IOUSBHostFamily. We work around it by using two different IOKit paths:
//!
//! * **HID interface #4 (pads, encoders, buttons, LEDs)** → `hidapi-rs`,
//!   which talks to `IOHIDManager`. That subsystem sees the HID interface
//!   cleanly with no sudo, no process kills, and no launchctl fiddling.
//! * **Vendor-bulk interface #5 (dual 480×272 displays)** → `nusb`.
//!   On macOS today this interface isn't enumerated as an
//!   `IOUSBHostInterface`, so `Transport::open` logs a warning and runs
//!   with display disabled. Pads + LEDs keep working.
//!
//! Linux and Windows will use nusb for both interfaces once their
//! platform backends land in v0.2.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, Mutex};

use maschine_proto as proto;

use crate::platform::{ClaimGuard, DeviceClaim};

/// Errors returned by the transport layer.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("hid: {0}")]
    Hid(#[from] hidapi::HidError),
    #[error("usb: {0}")]
    Usb(#[from] nusb::Error),
    #[error("transfer: {0}")]
    Transfer(#[from] nusb::transfer::TransferError),
    #[error("device 17cc:1600 not found — is the Mk3 plugged in and powered?")]
    NotFound,
    #[error("display bulk interface not available on this platform yet")]
    DisplayUnavailable,
    #[error("platform: {0}")]
    Platform(String),
}

/// Inbound HID report queue.
pub type InboundRx = mpsc::Receiver<Vec<u8>>;

/// Handle to an opened Mk3.
pub struct Transport {
    hid_dev: Arc<Mutex<hidapi::HidDevice>>,
    /// Optional nusb handle for the display bulk interface. `None` when we
    /// couldn't claim it (currently always the case on macOS v0.1).
    display: Option<DisplayHandle>,
    /// Held for its `Drop` side-effects (restore NI agent on macOS).
    _guard: ClaimGuard,
}

struct DisplayHandle {
    #[allow(dead_code)]
    device: nusb::Device,
    iface: nusb::Interface,
}

impl Transport {
    /// Find and open the Mk3. Always succeeds if the HID interface is
    /// present; the display bulk interface is opened best-effort and its
    /// absence degrades gracefully.
    pub async fn open() -> Result<Self, TransportError> {
        let guard = crate::platform::current().prepare()
            .map_err(|e| TransportError::Platform(e.to_string()))?;

        // HID — this is the one that must work.
        let api = hidapi::HidApi::new()?;
        let hid_path = api.device_list()
            .find(|d| d.vendor_id() == proto::VID_NI && d.product_id() == proto::PID_MK3)
            .map(|d| d.path().to_owned())
            .ok_or(TransportError::NotFound)?;
        let hid_dev = api.open_path(&hid_path)?;
        hid_dev.set_blocking_mode(false)?;
        tracing::info!("HID interface opened ({:?})", hid_path);

        // Display bulk — best-effort.
        let display = match open_display() {
            Ok(h) => {
                tracing::info!("display bulk interface claimed");
                Some(h)
            }
            Err(e) => {
                tracing::warn!(
                    "display bulk interface unavailable: {e}. \
                     pads/LEDs still work; screens will be blank in v0.1"
                );
                None
            }
        };

        Ok(Self {
            hid_dev: Arc::new(Mutex::new(hid_dev)),
            display,
            _guard: guard,
        })
    }

    /// Start a background task that drains the HID IN endpoint into an mpsc
    /// channel.
    pub fn spawn_hid_reader(&self) -> InboundRx {
        let (tx, rx) = mpsc::channel::<Vec<u8>>(256);
        let dev = self.hid_dev.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 128];
            loop {
                // hidapi is a blocking API even with non-blocking mode;
                // run reads on a blocking thread so we don't stall the
                // runtime.
                let dev = dev.clone();
                let read = tokio::task::spawn_blocking(move || {
                    let guard = dev.blocking_lock();
                    let mut local = [0u8; 128];
                    let r = guard.read_timeout(&mut local, 20).map(|n| (n, local));
                    r
                }).await;
                match read {
                    Ok(Ok((n, local))) if n > 0 => {
                        buf[..n].copy_from_slice(&local[..n]);
                        if tx.send(buf[..n].to_vec()).await.is_err() { break; }
                    }
                    Ok(Ok(_)) => {
                        // No data this tick — yield then retry.
                        tokio::time::sleep(Duration::from_millis(2)).await;
                    }
                    Ok(Err(e)) => {
                        tracing::warn!("HID read error: {e}");
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                    Err(e) => {
                        tracing::error!("HID reader task panicked: {e}");
                        break;
                    }
                }
            }
        });
        rx
    }

    /// Write a HID OUT report (caller supplies the report id as byte 0).
    pub async fn write_hid(&self, payload: Vec<u8>) -> Result<(), TransportError> {
        let dev = self.hid_dev.clone();
        let result = tokio::task::spawn_blocking(move || {
            let guard = dev.blocking_lock();
            guard.write(&payload).map(|_| ())
        }).await;
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e.into()),
            Err(e) => Err(TransportError::Platform(format!("hid write task: {e}"))),
        }
    }

    /// Submit a display bulk-OUT transfer on EP `0x04`. Returns
    /// `DisplayUnavailable` if the platform could not claim interface #5.
    pub async fn write_display(&self, payload: Vec<u8>) -> Result<(), TransportError> {
        let Some(display) = self.display.as_ref() else {
            return Err(TransportError::DisplayUnavailable);
        };
        let completion = display.iface.bulk_out(proto::EP_DISPLAY_OUT, payload).await;
        completion.status?;
        Ok(())
    }

    /// True if this transport can draw to the physical displays.
    pub fn has_display(&self) -> bool {
        self.display.is_some()
    }
}

fn open_display() -> Result<DisplayHandle, TransportError> {
    let dev_info = nusb::list_devices()?
        .find(|d| d.vendor_id() == proto::VID_NI && d.product_id() == proto::PID_MK3)
        .ok_or(TransportError::NotFound)?;
    let device = dev_info.open()?;
    let iface = device.detach_and_claim_interface(proto::IFACE_DISPLAY)
        .map_err(|e| TransportError::Usb(e))?;
    Ok(DisplayHandle { device, iface })
}

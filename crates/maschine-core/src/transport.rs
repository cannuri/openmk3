//! USB transport abstraction. The concrete impl uses `nusb` 0.1.

use std::time::Duration;

use nusb::transfer::{RequestBuffer, TransferError};
use tokio::sync::mpsc;

use maschine_proto as proto;

use crate::platform::{ClaimGuard, DeviceClaim};

/// Errors returned by the transport layer.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("usb: {0}")]
    Usb(#[from] nusb::Error),
    #[error("transfer: {0}")]
    Transfer(#[from] TransferError),
    #[error("device 17cc:1600 not found — is the Mk3 plugged in and powered?")]
    NotFound,
    #[error(
        "failed to claim USB interface {iface}: {source}. \
        On macOS this usually means the system `MIDIServer` has the Mk3 \
        opened exclusively. Run `cargo run --example restore_agent -p maschine-core` \
        first (restores any frozen NI agents), then disable MIDIServer \
        temporarily with:\n\
        \x20 sudo launchctl unload /System/Library/LaunchAgents/com.apple.midiserver.plist\n\
        \x20 cargo run --release -p maschined\n\
        \x20 sudo launchctl load   /System/Library/LaunchAgents/com.apple.midiserver.plist\n\
        A permanent fix using USBInterfaceOpenSeize is tracked as v0.1.1 work."
    )]
    Claim { iface: u8, #[source] source: nusb::Error },
    #[error("platform: {0}")]
    Platform(String),
}

/// Direction of HID I/O.
pub type InboundRx = mpsc::Receiver<Vec<u8>>;

/// Handle to an opened Mk3. Keeps the platform claim guard alive for the
/// lifetime of the transport.
pub struct Transport {
    #[allow(dead_code)]
    pub(crate) device: nusb::Device,
    pub(crate) hid_iface: nusb::Interface,
    pub(crate) display_iface: nusb::Interface,
    /// Held for its `Drop` side-effects (restore NI agent on macOS).
    _guard: ClaimGuard,
}

fn try_claim(dev_info: &nusb::DeviceInfo, guard: ClaimGuard) -> Result<Transport, TransportError> {
    let device = dev_info.open()?;
    let hid_iface = device.detach_and_claim_interface(proto::IFACE_HID)
        .map_err(|e| TransportError::Claim { iface: proto::IFACE_HID, source: e })?;
    let display_iface = device.detach_and_claim_interface(proto::IFACE_DISPLAY)
        .map_err(|e| TransportError::Claim { iface: proto::IFACE_DISPLAY, source: e })?;
    Ok(Transport { device, hid_iface, display_iface, _guard: guard })
}

impl Transport {
    /// Locate + open the first Mk3 and claim interfaces #4 (HID) and
    /// #5 (display bulk).
    ///
    /// On macOS the system `MIDIServer` opens the whole device
    /// exclusively because it presents a USB Audio Class descriptor on
    /// interfaces 0..3. `launchd` respawns `MIDIServer` on demand, so we
    /// kill it and race to claim before the new instance finishes
    /// re-registering. A small retry loop makes the race deterministic.
    pub async fn open() -> Result<Self, TransportError> {
        const MAX_ATTEMPTS: usize = 8;
        let mut last_err: Option<TransportError> = None;
        for attempt in 0..MAX_ATTEMPTS {
            let guard = crate::platform::current().prepare()
                .map_err(|e| TransportError::Platform(e.to_string()))?;

            let Some(dev_info) = nusb::list_devices()?
                .find(|d| d.vendor_id() == proto::VID_NI && d.product_id() == proto::PID_MK3)
            else {
                return Err(TransportError::NotFound);
            };

            match try_claim(&dev_info, guard) {
                Ok(t) => {
                    if attempt > 0 {
                        tracing::info!("claimed Mk3 after {} retries", attempt);
                    }
                    return Ok(t);
                }
                Err(e) => {
                    tracing::debug!("claim attempt {attempt}: {e}");
                    last_err = Some(e);
                    tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                }
            }
        }
        Err(last_err.unwrap_or(TransportError::NotFound))
    }

    /// Start a background task that drains the HID IN endpoint into an mpsc
    /// channel. The Mk3 schedules HID IN on interface 4, address `0x84`.
    pub fn spawn_hid_reader(&self) -> InboundRx {
        const HID_IN_EP: u8 = 0x84;
        const BUF_LEN: usize = 64;
        let (tx, rx) = mpsc::channel::<Vec<u8>>(256);
        let iface = self.hid_iface.clone();
        tokio::spawn(async move {
            let mut queue = iface.interrupt_in_queue(HID_IN_EP);
            for _ in 0..4 {
                queue.submit(RequestBuffer::new(BUF_LEN));
            }
            loop {
                let completion = queue.next_complete().await;
                match completion.status {
                    Ok(()) => {
                        if tx.send(completion.data.clone()).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("HID IN transfer error: {e:?}");
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                }
                queue.submit(RequestBuffer::new(BUF_LEN));
            }
        });
        rx
    }

    /// Write a HID OUT report (interrupt OUT on interface 4, address `0x03`).
    pub async fn write_hid(&self, payload: Vec<u8>) -> Result<(), TransportError> {
        const HID_OUT_EP: u8 = 0x03;
        let completion = self.hid_iface.interrupt_out(HID_OUT_EP, payload).await;
        completion.status?;
        Ok(())
    }

    /// Submit a display bulk-OUT transfer on EP `0x04`.
    pub async fn write_display(&self, payload: Vec<u8>) -> Result<(), TransportError> {
        let completion = self.display_iface.bulk_out(proto::EP_DISPLAY_OUT, payload).await;
        completion.status?;
        Ok(())
    }
}

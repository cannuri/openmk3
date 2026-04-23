//! USB transport — direct USB I/O via our patched nusb fork.
//!
//! Our vendored fork of nusb 0.1.14 replaces macOS's `USBDeviceOpen()` /
//! `USBInterfaceOpen()` with the `…OpenSeize()` variants, which evict
//! existing kernel-side drivers (MIDIServer, usbaudiod, IOHIDFamily) so
//! that we can claim the Mk3's HID and bulk interfaces directly — the
//! same approach WebUSB uses in Chrome.

use std::time::Duration;

use nusb::transfer::{RequestBuffer, TransferError};
use tokio::sync::mpsc;

use maschine_proto as proto;

use crate::platform::{ClaimGuard, DeviceClaim};

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("usb: {0}")]
    Usb(#[from] nusb::Error),
    #[error("transfer: {0}")]
    Transfer(#[from] TransferError),
    #[error("device 17cc:1600 not found — is the Mk3 plugged in and powered?")]
    NotFound,
    #[error("failed to claim USB interface {iface}: {source}")]
    Claim { iface: u8, #[source] source: nusb::Error },
    #[error("platform: {0}")]
    Platform(String),
}

pub type InboundRx = mpsc::Receiver<Vec<u8>>;

pub struct Transport {
    #[allow(dead_code)]
    device: nusb::Device,
    hid_iface: nusb::Interface,
    display_iface: nusb::Interface,
    _guard: ClaimGuard,
}

impl Transport {
    pub async fn open() -> Result<Self, TransportError> {
        let guard = crate::platform::current().prepare()
            .map_err(|e| TransportError::Platform(e.to_string()))?;
        let dev_info = nusb::list_devices()?
            .find(|d| d.vendor_id() == proto::VID_NI && d.product_id() == proto::PID_MK3)
            .ok_or(TransportError::NotFound)?;
        let device = dev_info.open()?;
        let hid_iface = device.detach_and_claim_interface(proto::IFACE_HID)
            .map_err(|e| TransportError::Claim { iface: proto::IFACE_HID, source: e })?;
        tracing::info!("claimed HID interface #{}", proto::IFACE_HID);
        let display_iface = device.detach_and_claim_interface(proto::IFACE_DISPLAY)
            .map_err(|e| TransportError::Claim { iface: proto::IFACE_DISPLAY, source: e })?;
        tracing::info!("claimed display bulk interface #{}", proto::IFACE_DISPLAY);
        Ok(Self { device, hid_iface, display_iface, _guard: guard })
    }

    /// Start a background task that drains the HID IN endpoint into an
    /// mpsc channel. Interrupt IN on interface 4, endpoint 0x84.
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
                        if !completion.data.is_empty() {
                            if tx.send(completion.data.clone()).await.is_err() { break; }
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

    /// Write a HID OUT report (interrupt OUT on endpoint 0x03).
    pub async fn write_hid(&self, payload: Vec<u8>) -> Result<(), TransportError> {
        const HID_OUT_EP: u8 = 0x03;
        let completion = self.hid_iface.interrupt_out(HID_OUT_EP, payload).await;
        completion.status?;
        Ok(())
    }

    /// Submit a display bulk-OUT transfer on EP 0x04.
    pub async fn write_display(&self, payload: Vec<u8>) -> Result<(), TransportError> {
        let completion = self.display_iface.bulk_out(proto::EP_DISPLAY_OUT, payload).await;
        completion.status?;
        Ok(())
    }

    pub fn has_display(&self) -> bool { true }
}

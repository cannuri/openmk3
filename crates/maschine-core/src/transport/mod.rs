//! Transport façade.
//!
//! Two backends:
//! * `nusb_impl::NusbTransport` — direct USB via our patched nusb fork.
//!   Used on Linux/Windows, and on macOS as a fallback when the DriverKit
//!   extension is not installed.
//! * `dext_impl::DextTransport` — IOKit user client talking to our
//!   `MaschineMk3UserClient` DriverKit extension. macOS-only; authoritative
//!   wire protocol lives in `dext/docs/A1-architecture.md` (section 3,
//!   "IPC wire protocol") and is mirrored in `dext_wire.rs`.
//!
//! Consumers use `Transport` / `TransportError` by those names — the enum
//! dispatches to the live backend so no call-site has to change.

use nusb::transfer::TransferError;
use tokio::sync::mpsc;

pub mod nusb_impl;

#[cfg(target_os = "macos")]
pub mod dext_wire;
#[cfg(target_os = "macos")]
pub mod dext_impl;

pub type InboundRx = mpsc::Receiver<Vec<u8>>;

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

pub enum Transport {
    #[cfg(target_os = "macos")]
    Dext(dext_impl::DextTransport),
    Nusb(nusb_impl::NusbTransport),
}

impl Transport {
    pub async fn open() -> Result<Self, TransportError> {
        #[cfg(target_os = "macos")]
        {
            if std::env::var_os("MASCHINE_FORCE_NUSB").is_none() {
                match dext_impl::DextTransport::open().await {
                    Ok(t) => {
                        tracing::info!("opened via DriverKit extension (dext)");
                        return Ok(Transport::Dext(t));
                    }
                    Err(e) => {
                        tracing::warn!("dext unavailable ({e}); falling back to nusb");
                    }
                }
            }
        }
        Ok(Transport::Nusb(nusb_impl::NusbTransport::open().await?))
    }

    pub fn spawn_hid_reader(&self) -> InboundRx {
        match self {
            #[cfg(target_os = "macos")]
            Transport::Dext(t) => t.spawn_hid_reader(),
            Transport::Nusb(t) => t.spawn_hid_reader(),
        }
    }

    pub async fn write_hid(&self, payload: Vec<u8>) -> Result<(), TransportError> {
        match self {
            #[cfg(target_os = "macos")]
            Transport::Dext(t) => t.write_hid(payload).await,
            Transport::Nusb(t) => t.write_hid(payload).await,
        }
    }

    pub async fn write_display(&self, payload: Vec<u8>) -> Result<(), TransportError> {
        match self {
            #[cfg(target_os = "macos")]
            Transport::Dext(t) => t.write_display(payload).await,
            Transport::Nusb(t) => t.write_display(payload).await,
        }
    }

    pub fn has_display(&self) -> bool {
        match self {
            #[cfg(target_os = "macos")]
            Transport::Dext(t) => t.has_display(),
            Transport::Nusb(t) => t.has_display(),
        }
    }
}

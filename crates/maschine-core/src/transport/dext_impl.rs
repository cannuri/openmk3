//! macOS DriverKit-extension transport.
//!
//! Authority: `dext/docs/A1-architecture.md` §3 ("IPC wire protocol") and
//! §4.2 ("DextTransport internals"). This client talks to the
//! `MaschineMk3UserClient` `IOUserClient` subclass shipped inside the
//! `com.cannuri.maschine.dext` DriverKit extension.
//!
//! Two IOConnect handles per transport — one for the HID personality
//! (if#4), one for the display personality (if#5). HID IN reports arrive
//! asynchronously via a stashed `OSAction` whose completion port is
//! serviced on a dedicated `CFRunLoop` thread; display-done completions
//! travel the same way. Outbound HID / display writes are synchronous
//! `IOConnectCallStructMethod` calls with internal flow control enforced
//! by the dext (`kIOReturnNoResources` ⇒ `TransportError::Platform` for
//! now; see TODO below).
//!
//! The actual IOKit FFI is stubbed until I1 lands the dext binary — every
//! `unimplemented!()` here is paired with a TODO(A1) explaining what's
//! still open.

use std::os::raw::c_void;
use std::sync::Arc;
use std::thread::JoinHandle;

use tokio::sync::mpsc;

use super::{InboundRx, TransportError};
use super::dext_wire::{self as wire, MaschineOpenIn};

// ---------- IOKit FFI surface ------------------------------------------------
//
// io-kit-sys exposes these already but we type them explicitly here so the
// crate still compiles without the dependency until I1 lands the dext. The
// moment someone wires in io-kit-sys we delete these externs and re-import
// from that crate instead.
//
// TODO(A1): swap these externs for `io_kit_sys::{IOServiceOpen, ...}` and
// `core_foundation` matching dictionaries once the dext is buildable.
#[allow(non_camel_case_types, dead_code)]
type io_service_t = u32;
#[allow(non_camel_case_types)]
type io_connect_t = u32;
#[allow(non_camel_case_types)]
type kern_return_t = i32;
#[allow(non_camel_case_types)]
type mach_port_t = u32;

const KERN_SUCCESS: kern_return_t = 0;

#[allow(dead_code)]
#[inline]
fn map_kr(kr: kern_return_t) -> Result<(), TransportError> {
    if kr == KERN_SUCCESS {
        Ok(())
    } else {
        // TODO(A1): map kIOReturnNoResources / kIOReturnNotOpen /
        // kIOReturnAborted / kIOUSBPipeStalled onto richer variants
        // (Busy/Closed/Aborted/Stalled). A1 §3.5 defines the table; the
        // TransportError enum needs the extra variants before we can surface
        // them without breaking the nusb path, so for v0.1 we flatten to
        // `Platform(hex)`.
        Err(TransportError::Platform(format!("IOKit error 0x{:08x}", kr as u32)))
    }
}

// ---------- DextTransport ----------------------------------------------------

pub struct DextTransport {
    #[allow(dead_code)]
    hid_conn: io_connect_t,
    #[allow(dead_code)]
    display_conn: io_connect_t,
    /// Receiver populated by the CFRunLoop thread that services the
    /// `kSel_RegisterHidCallback` async push. `spawn_hid_reader` hands this
    /// receiver back to the caller exactly once (same semantics as the
    /// nusb path — the caller owns the channel from there on).
    hid_inbound: std::sync::Mutex<Option<mpsc::Receiver<Vec<u8>>>>,
    /// Present only so `write_display` can check / assert display is open.
    /// `true` iff the display personality was matched at `open()` time.
    has_display_iface: bool,
    #[allow(dead_code)]
    notify_thread: Option<JoinHandle<()>>,
}

impl DextTransport {
    pub async fn open() -> Result<Self, TransportError> {
        // --- Step 1: find the HID user-client service ---
        //
        // Canonical call shape (A1 §4.2):
        //   let svc = IOServiceGetMatchingService(
        //       kIOMainPortDefault,
        //       IOServiceNameMatching("MaschineMk3HidTransport"));
        //   if svc == 0 { return Err(NotFound); }
        //   IOServiceOpen(svc, mach_task_self_, 0, &mut hid_conn);
        //
        // We return `NotFound` so the enum façade in `mod.rs` cleanly falls
        // back to the nusb path when the dext isn't installed.
        //
        // TODO(A1): real IOKit lookup. Until the dext is built and the
        // userclient-access entitlement is granted, the caller will always
        // see `NotFound` here and fall back to nusb — which is exactly the
        // behaviour v0.1 needs.
        let _ = wire::K_SEL_OPEN;
        Err(TransportError::NotFound)
    }

    /// Stream HID IN reports. The dext pushes them via the `OSAction`
    /// stashed by `kSel_RegisterHidCallback`; the CFRunLoop thread decodes
    /// `MaschineHidInEvent` and forwards the `data[..length]` slice here.
    pub fn spawn_hid_reader(&self) -> InboundRx {
        if let Some(rx) = self.hid_inbound.lock().unwrap().take() {
            return rx;
        }
        // Second call: hand back an empty, immediately-closed channel. The
        // nusb path can technically be called twice without issue because
        // it spawns fresh; we match that shape on the dext side by never
        // panicking even if someone re-subscribes.
        let (_tx, rx) = mpsc::channel::<Vec<u8>>(1);
        rx
    }

    /// Submit a HID OUT report via `kSel_HidOutReport`.
    pub async fn write_hid(&self, _payload: Vec<u8>) -> Result<(), TransportError> {
        // TODO(A1): pack into `MaschineHidOut { length, data }` and call
        // IOConnectCallStructMethod(hid_conn, K_SEL_HID_OUT_REPORT, &in,
        // 4 + length, NULL, NULL). Map the return via map_kr().
        unimplemented!("DextTransport::write_hid — awaiting dext binary from I1")
    }

    /// Submit one display bulk-OUT frame via `kSel_BulkOut`.
    pub async fn write_display(&self, _payload: Vec<u8>) -> Result<(), TransportError> {
        // TODO(A1): pack into `MaschineBulkOut { length, seq, data }` and
        // call IOConnectCallStructMethod(display_conn, K_SEL_BULK_OUT, ...)
        // with the variable-size struct. A1 §2.2 says the dext is the rate
        // limiter, so no need to poll or wait for display-done here —
        // completions surface on the async channel.
        unimplemented!("DextTransport::write_display — awaiting dext binary from I1")
    }

    pub fn has_display(&self) -> bool { self.has_display_iface }
}

// ---------- CFRunLoop async-notification thread ------------------------------
//
// Shape per A1 §4.2 step 6:
//   - create IONotificationPortRef
//   - pass its mach port to IOConnectCallAsyncStructMethod for each of the
//     two selectors (K_SEL_REGISTER_HID_CALLBACK, K_SEL_REGISTER_DISPLAY_CALLBACK)
//   - spawn a dedicated OS thread, get its CFRunLoop, add the notification
//     port's run-loop source, CFRunLoopRun() forever.
//   - C callback decodes the incoming `MaschineHidInEvent` and posts its
//     `data[..length]` bytes as Vec<u8> on `hid_tx`.
//
// TODO(A1): implement the thread + C trampolines once io-kit-sys /
// core-foundation deps land. The key decision A1 already locked in is "one
// stashed OSAction per interface, no shared-memory ring" — so the
// receiving side is a plain `Sender<Vec<u8>>`, no serialization ring, no
// bespoke cache invalidation dance.
#[allow(dead_code)]
fn _notify_thread_shape(
    _hid_tx: mpsc::Sender<Vec<u8>>,
    _display_tx: mpsc::Sender<(u32, i32)>,
    _notify_port_mach: mach_port_t,
) -> JoinHandle<()> {
    std::thread::spawn(|| {
        // CFRunLoopGetCurrent() → add source → CFRunLoopRun()
        unimplemented!("notify thread — see A1 §4.2 step 6")
    })
}

// Silence the "unused" churn while the implementation is a stub but keep
// the link-site visible so deleting any of these triggers a compile error.
#[allow(dead_code)]
const _SHAPE: fn(MaschineOpenIn, *const c_void, Arc<()>) = |_, _, _| {};

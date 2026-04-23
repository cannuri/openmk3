//! Rust mirror of `dext/MaschineDext/MaschineIPC.h`.
//!
//! Authority: `dext/docs/A1-architecture.md` §3.1 ("Selector constants").
//! Keep this file in lock-step with the C header. Struct sizes are verified
//! in `size_of` tests at the bottom of the file so a drift will fail CI
//! before anyone fires up hardware.

#![allow(non_camel_case_types)]
#![allow(dead_code)]

/// ExternalMethod selector IDs. Values are authoritative per A1 §3.1.
pub const K_SEL_OPEN: u32                    = 0;
pub const K_SEL_CLOSE: u32                   = 1;
pub const K_SEL_REGISTER_HID_CALLBACK: u32   = 2;
pub const K_SEL_REGISTER_DISPLAY_CALLBACK: u32 = 3;
pub const K_SEL_HID_OUT_REPORT: u32          = 4;
pub const K_SEL_BULK_OUT: u32                = 5;
pub const K_SEL_DEVICE_STATE: u32            = 6;
pub const K_SEL_ABORT: u32                   = 7;
pub const K_MASCHINE_SELECTOR_COUNT: u32     = 8;

pub const MASCHINE_IPC_VERSION: u32      = 1;
pub const MASCHINE_HID_REPORT_MAX: usize = 512;
pub const MASCHINE_BULK_FRAME_MAX: usize = 524_288;

/// `kSel_Open` input payload bit flags.
pub const OPEN_FLAG_WANT_DISPLAY: u32 = 1 << 0;
pub const OPEN_FLAG_WANT_HID: u32     = 1 << 1;

/// Argument values for `kSel_Abort` scalar input.
pub const ABORT_ALL: u64     = 0;
pub const ABORT_HID_OUT: u64 = 1;
pub const ABORT_DISPLAY: u64 = 2;

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct MaschineOpenIn {
    pub client_version: u32,
    pub flags: u32,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct MaschineOpenOut {
    pub dext_version: u32,
    pub vendor_id: u32,
    pub product_id: u32,
    pub interface_number: u8,
    pub _pad: [u8; 3],
}

/// HID OUT report. `data[..length]` is the payload submitted to EP 0x03.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct MaschineHidOut {
    pub length: u32,
    pub data: [u8; MASCHINE_HID_REPORT_MAX],
}

/// Bulk OUT (display) frame. `data[..length]` is the payload submitted to
/// the display bulk endpoint; `seq` is echoed back in the display-done
/// async completion so Rust can match completions to submissions.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct MaschineBulkOut {
    pub length: u32,
    pub seq: u32,
    pub data: [u8; MASCHINE_BULK_FRAME_MAX],
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct MaschineDeviceState {
    pub vendor_id: [u8; 2],
    pub product_id: [u8; 2],
    pub b_interface_number: u8,
    pub ep_in_addr: u8,
    pub ep_out_addr: u8,
    pub ep_bulk_addr: u8,
    pub in_max_packet: u16,
    pub out_max_packet: u16,
    pub bulk_max_packet: u16,
}

/// Payload of the async HID-IN callback pushed back from the dext over the
/// stashed OSAction. `data[..length]` is the raw 64-byte HID report.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct MaschineHidInEvent {
    pub length: u32,
    pub seq: u32,
    pub timestamp: u64,
    pub data: [u8; 64],
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;

    // Sizes per A1 §3.1. If these fail, someone edited one side of the wire
    // without the other — fix the drift before touching anything else.
    #[test]
    fn open_structs_sized() {
        assert_eq!(size_of::<MaschineOpenIn>(), 8);
        assert_eq!(size_of::<MaschineOpenOut>(), 16);
    }

    #[test]
    fn hid_out_is_516() {
        assert_eq!(size_of::<MaschineHidOut>(), 4 + MASCHINE_HID_REPORT_MAX);
    }

    #[test]
    fn bulk_out_is_524296() {
        assert_eq!(size_of::<MaschineBulkOut>(), 4 + 4 + MASCHINE_BULK_FRAME_MAX);
    }

    #[test]
    fn device_state_sized() {
        // 2+2+1+1+1+1 + 2+2+2 = 14
        assert_eq!(size_of::<MaschineDeviceState>(), 14);
    }

    #[test]
    fn hid_in_event_sized() {
        // 4 + 4 + 8 + 64 = 80
        assert_eq!(size_of::<MaschineHidInEvent>(), 80);
    }
}

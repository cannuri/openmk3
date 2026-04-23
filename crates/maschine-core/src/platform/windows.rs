use super::{ClaimError, ClaimGuard, DeviceClaim};

pub struct WindowsClaim;

impl DeviceClaim for WindowsClaim {
    fn prepare(&self) -> Result<ClaimGuard, ClaimError> {
        Err(ClaimError::UnsupportedPlatform("windows"))
    }
}

impl WindowsClaim {
    #[allow(dead_code)]
    fn _keep_impl_hint() -> ClaimGuard {
        // v0.2 impl: detect if if#5 is still on the composite HID driver and
        // surface libwdi/Zadig instructions; on WinUSB, no further claim.
        ClaimGuard::none()
    }
}

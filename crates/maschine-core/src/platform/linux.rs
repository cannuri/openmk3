use super::{ClaimError, ClaimGuard, DeviceClaim};

pub struct LinuxClaim;

impl DeviceClaim for LinuxClaim {
    fn prepare(&self) -> Result<ClaimGuard, ClaimError> {
        Err(ClaimError::UnsupportedPlatform("linux"))
    }
}

impl LinuxClaim {
    #[allow(dead_code)]
    fn _keep_impl_hint() -> ClaimGuard {
        // v0.2 impl: install resources/99-maschine.rules via pkexec if needed,
        // then USBDEVFS_DISCONNECT any stray kernel driver on if#4/#5.
        ClaimGuard::none()
    }
}

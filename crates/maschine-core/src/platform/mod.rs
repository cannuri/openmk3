//! Per-OS "claim the device away from NI's agent" logic.
//!
//! Each backend exposes a [`DeviceClaim`] impl whose `prepare()` must run
//! before we open the Mk3's USB interfaces. The returned [`ClaimGuard`] is
//! held for the transport's lifetime; `Drop` reverses any environmental
//! changes (e.g. restarting the NI agent).
//!
//! Only the macOS backend is implemented in v0.1. Linux and Windows return a
//! clear `UnsupportedPlatform` error so users get immediate feedback rather
//! than obscure USB failures.

use std::fmt;

#[cfg(target_os = "macos")] mod macos;
#[cfg(target_os = "linux")] mod linux;
#[cfg(target_os = "windows")] mod windows;

#[derive(Debug)]
pub enum ClaimError {
    UnsupportedPlatform(&'static str),
    Command(String),
}

impl fmt::Display for ClaimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform(p) => write!(f, "v0.1 only supports macOS; {p} lands in v0.2"),
            Self::Command(c) => write!(f, "claim subcommand failed: {c}"),
        }
    }
}

impl std::error::Error for ClaimError {}

/// RAII guard that restores agent state when dropped.
pub struct ClaimGuard {
    #[allow(dead_code)] // backend-specific
    pub(crate) inner: Box<dyn std::any::Any + Send + Sync>,
}

impl ClaimGuard {
    pub(crate) fn new<T: std::any::Any + Send + Sync>(t: T) -> Self {
        Self { inner: Box::new(t) }
    }
    #[allow(dead_code)] // used by the stub platform backends
    pub(crate) fn none() -> Self {
        Self { inner: Box::new(()) }
    }
}

pub trait DeviceClaim {
    fn prepare(&self) -> Result<ClaimGuard, ClaimError>;
}

#[cfg(target_os = "macos")]
pub fn current() -> impl DeviceClaim {
    macos::MacOsClaim
}
#[cfg(target_os = "linux")]
pub fn current() -> impl DeviceClaim {
    linux::LinuxClaim
}
#[cfg(target_os = "windows")]
pub fn current() -> impl DeviceClaim {
    windows::WindowsClaim
}

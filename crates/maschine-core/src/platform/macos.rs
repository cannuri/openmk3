//! macOS device-claim: stop NIHostIntegrationAgent (and friends) before
//! opening the Mk3, restart them on drop.
//!
//! Strategy:
//! 1. Check whether `NIHostIntegrationAgent` is running via `pgrep -x`.
//! 2. If yes, run `launchctl bootout gui/$UID/com.native-instruments.NIHostIntegrationAgent`.
//! 3. If `launchctl` doesn't know about it (e.g. the agent is launched by
//!    the user session rather than a LaunchAgent), fall back to sending
//!    `SIGSTOP` to the pid so the kernel unhooks it from the USB device
//!    without killing its state.
//! 4. Also suspend `Maschine 2` / `Maschine 3` / `Komplete Kontrol` host
//!    apps if running, since they claim the device directly.
//! 5. The returned guard reverses steps 3–4 on drop: `SIGCONT` the pids,
//!    `launchctl bootstrap` the agent.

use std::process::Command;

use super::{ClaimError, ClaimGuard, DeviceClaim};

const NI_AGENT: &str = "NIHostIntegrationAgent";
const AGENT_SERVICE: &str = "com.native-instruments.NIHostIntegrationAgent";
/// Other NI processes that hold the device's USB interfaces and must be
/// suspended for our direct-claim path to succeed. Kept as a conservative
/// list — every entry here is restored via SIGCONT in `Drop`.
const CONFLICTING_APPS: &[&str] = &[
    "NIHardwareAgent",     // background service; grabs interface #4
    "Maschine 2",
    "Maschine",
    "Komplete Kontrol",
];

pub struct MacOsClaim;

impl DeviceClaim for MacOsClaim {
    fn prepare(&self) -> Result<ClaimGuard, ClaimError> {
        let state = MacOsClaimState::capture_and_suspend()?;
        Ok(ClaimGuard::new(state))
    }
}

struct MacOsClaimState {
    /// `launchctl`-booted agent that we must re-bootstrap on drop.
    agent_was_bootable: bool,
    /// Pids we sent SIGSTOP to; we'll SIGCONT them.
    stopped_pids: Vec<libc::pid_t>,
}

impl MacOsClaimState {
    fn capture_and_suspend() -> Result<Self, ClaimError> {
        let uid = unsafe { libc::getuid() };
        let mut stopped_pids = Vec::new();
        let mut agent_was_bootable = false;

        // 1. NI Host Integration Agent
        if let Some(pid) = pgrep(NI_AGENT) {
            // Try the clean path first.
            let target = format!("gui/{uid}/{AGENT_SERVICE}");
            let status = Command::new("launchctl")
                .args(["bootout", &target])
                .status();
            if matches!(status, Ok(s) if s.success()) {
                agent_was_bootable = true;
                tracing::info!("stopped {NI_AGENT} via launchctl");
            } else {
                sigstop(pid)?;
                stopped_pids.push(pid);
                tracing::info!("SIGSTOP'd {NI_AGENT} (pid {pid})");
            }
        }

        // 2. Conflicting host apps
        for app in CONFLICTING_APPS {
            if let Some(pid) = pgrep(app) {
                sigstop(pid)?;
                stopped_pids.push(pid);
                tracing::info!("SIGSTOP'd {app} (pid {pid})");
            }
        }

        // 3. coremidi / MIDIServer.
        //
        // The Mk3 advertises itself as a USB Audio Class device on
        // interfaces 0..3 (for the audio interface + class-compliant MIDI),
        // which causes `MIDIServer` to open the *whole* device exclusively.
        // That blocks our claim on interface 4.
        //
        // SIGKILL'ing `coremidid` forces launchd to respawn it. The kernel
        // briefly releases the exclusive device handle, giving us a window
        // to claim interfaces 4 and 5. Once those claims land, the
        // respawned `coremidid` can no longer steal them back — it just
        // re-opens the audio interfaces it already owned.
        //
        // We do *not* add `coremidid` to `stopped_pids` because there is no
        // "SIGCONT" restoration step — launchd already handled the restart
        // transparently.
        // Exact process name on current macOS is `MIDIServer` (not
        // `coremidid` — that binary hasn't existed since 10.13).
        //
        // We DO NOT sleep after the kill: launchd respawns MIDIServer
        // within ~20–50 ms, so every millisecond of sleep here loses us
        // the race to claim the device. The caller (`Transport::open`)
        // runs this claim in a retry loop, so if we lose the first race
        // we simply kill again and retry.
        for name in ["MIDIServer", "usbaudiod"] {
            if let Some(pid) = pgrep(name) {
                unsafe { libc::kill(pid, libc::SIGKILL); }
                tracing::debug!("SIGKILL {name} (pid {pid})");
            }
        }

        Ok(Self { agent_was_bootable, stopped_pids })
    }
}

impl Drop for MacOsClaimState {
    fn drop(&mut self) {
        for pid in &self.stopped_pids {
            let _ = unsafe { libc::kill(*pid, libc::SIGCONT) };
        }
        if self.agent_was_bootable {
            let uid = unsafe { libc::getuid() };
            // Best-effort. Don't panic on failure.
            let plist = format!(
                "/Library/LaunchAgents/{AGENT_SERVICE}.plist"
            );
            let _ = Command::new("launchctl")
                .args(["bootstrap", &format!("gui/{uid}"), &plist])
                .status();
        }
    }
}

fn pgrep(name: &str) -> Option<libc::pid_t> {
    // `pgrep -x` matches on the process' short name. NI ships agents under
    // .app bundles whose executable is the same name, so the short-match is
    // reliable enough and avoids false positives from `pgrep -f`.
    let out = Command::new("pgrep").arg("-x").arg(name).output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    s.lines().next().and_then(|l| l.trim().parse().ok())
}

fn sigstop(pid: libc::pid_t) -> Result<(), ClaimError> {
    let rc = unsafe { libc::kill(pid, libc::SIGSTOP) };
    if rc == 0 {
        Ok(())
    } else {
        Err(ClaimError::Command(format!("kill({pid}, SIGSTOP) failed: {}", std::io::Error::last_os_error())))
    }
}

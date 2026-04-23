//! If a crash or `kill -9` leaves `NIHostIntegrationAgent` / `NIHardwareAgent`
//! stuck in a SIGSTOP'd state, this one-shot tool restores them.
//!
//! Run: `cargo run --example restore_agent -p maschine-core`

use std::process::Command;

fn main() {
    for name in ["NIHostIntegrationAgent", "NIHardwareAgent", "Maschine 2", "Maschine", "Komplete Kontrol"] {
        let Ok(out) = Command::new("pgrep").arg("-x").arg(name).output() else { continue };
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            if let Ok(pid) = line.trim().parse::<libc::pid_t>() {
                let rc = unsafe { libc::kill(pid, libc::SIGCONT) };
                println!("SIGCONT {name} pid={pid} rc={rc}");
            }
        }
    }
}

//! Audio Unit plugin discovery on macOS.
//!
//! v0.1 implementation shells out to `auval -a` (ships with Xcode & the OS),
//! which enumerates every registered component. Moving to a direct
//! `AudioComponentFindNext` via a C shim is an M5 polish item.

use std::process::Command;

#[derive(Debug, Clone)]
pub struct AuPlugin {
    pub ty: u32,
    pub subtype: u32,
    pub manufacturer: u32,
    pub name: String,
    pub manufacturer_name: String,
}

pub fn scan_macos() -> Vec<AuPlugin> {
    let Ok(out) = Command::new("auval").arg("-a").output() else { return Vec::new() };
    let text = String::from_utf8_lossy(&out.stdout);
    parse_auval(&text)
}

fn parse_auval(text: &str) -> Vec<AuPlugin> {
    // `auval -a` lines of the form:
    //   aumu  NIMX  -NI-  -  Native Instruments: Massive X
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("--") { continue; }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 { continue; }
        let (ty, subty, manu) = (fourcc(parts[0]), fourcc(parts[1]), fourcc(parts[2]));
        if ty.is_none() || subty.is_none() || manu.is_none() { continue; }
        let rest = line.splitn(4, char::is_whitespace).nth(3).unwrap_or("");
        let (manufacturer_name, name) = match rest.split_once(": ") {
            Some((m, n)) => (m.trim().to_string(), n.trim().to_string()),
            None => (String::new(), rest.trim().to_string()),
        };
        out.push(AuPlugin {
            ty: ty.unwrap(),
            subtype: subty.unwrap(),
            manufacturer: manu.unwrap(),
            name,
            manufacturer_name,
        });
    }
    out
}

fn fourcc(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    if b.len() != 4 { return None; }
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

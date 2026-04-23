//! VST3 plugin discovery on macOS.
//!
//! Standard VST3 bundle layout:
//!   `Plugin.vst3/Contents/moduleinfo.json`  (VST3 SDK ≥ 3.7.5)
//!   `Plugin.vst3/Contents/MacOS/Plugin`     (binary — only needed as fallback)
//!
//! We fast-path the JSON. If absent, we fall back to scanning via the
//! `maschine-pluginhost --scan` child process (M5 work), which is out of
//! scope here — we note the path for the caller.

use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct Vst3Plugin {
    pub bundle: PathBuf,
    pub name: String,
    pub vendor: String,
    pub class_uid: [u32; 4],
    pub categories: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ModuleInfo {
    #[serde(rename = "Factory Info")]
    factory: FactoryInfo,
    #[serde(rename = "Classes")]
    classes: Vec<ClassInfo>,
}

#[derive(Debug, Deserialize)]
struct FactoryInfo { #[serde(rename = "Vendor")] vendor: String }

#[derive(Debug, Deserialize)]
struct ClassInfo {
    #[serde(rename = "Name")] name: String,
    #[serde(rename = "Category")] category: String,
    #[serde(rename = "CID")] cid: String,
    #[serde(default, rename = "Sub Categories")] sub_categories: Vec<String>,
}

pub fn scan_mac_system_and_user() -> Vec<Vst3Plugin> {
    let mut out = Vec::new();
    for root in ["/Library/Audio/Plug-Ins/VST3", "~/Library/Audio/Plug-Ins/VST3"] {
        let root = expand(root);
        if !root.exists() { continue; }
        let Ok(entries) = std::fs::read_dir(&root) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("vst3") {
                if let Some(pl) = try_read_moduleinfo(&p) { out.push(pl); }
            }
        }
    }
    out
}

fn try_read_moduleinfo(bundle: &Path) -> Option<Vst3Plugin> {
    let info_path = bundle.join("Contents/moduleinfo.json");
    let text = std::fs::read_to_string(&info_path).ok()?;
    let info: ModuleInfo = serde_json::from_str(&text).ok()?;
    let class = info.classes.iter().find(|c| c.category == "Audio Module Class")?;
    let cid = parse_cid(&class.cid)?;
    let mut categories = class.sub_categories.clone();
    categories.insert(0, class.category.clone());
    Some(Vst3Plugin {
        bundle: bundle.to_path_buf(),
        name: class.name.clone(),
        vendor: info.factory.vendor.clone(),
        class_uid: cid,
        categories,
    })
}

fn parse_cid(cid: &str) -> Option<[u32; 4]> {
    // CID is a 32-char hex string: 4 × u32 big-endian.
    let s = cid.trim_start_matches("0x");
    if s.len() != 32 { return None; }
    let w0 = u32::from_str_radix(&s[0..8], 16).ok()?;
    let w1 = u32::from_str_radix(&s[8..16], 16).ok()?;
    let w2 = u32::from_str_radix(&s[16..24], 16).ok()?;
    let w3 = u32::from_str_radix(&s[24..32], 16).ok()?;
    Some([w0, w1, w2, w3])
}

fn expand(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(p)
}

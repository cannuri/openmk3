//! Registry + PLID → installed plugin resolution.

use std::collections::HashMap;
use std::path::PathBuf;

use nks_parse::NksPluginId;

use crate::{au::AuPlugin, vst3::Vst3Plugin};

#[derive(Debug, Clone)]
pub enum PluginEntry {
    Vst3(Vst3Plugin),
    AudioUnit(AuPlugin),
}

/// Stable string identifier used as a join key in the NKS index.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PluginKey(pub String);

impl PluginKey {
    pub fn from_vst3_uid(uid: [u32; 4]) -> Self {
        Self(format!("vst3:{:08x}{:08x}{:08x}{:08x}", uid[0], uid[1], uid[2], uid[3]))
    }
    pub fn from_au(ty: u32, subtype: u32, manu: u32) -> Self {
        Self(format!("au:{:08x}:{:08x}:{:08x}", ty, subtype, manu))
    }
    pub fn from_vst2(magic: u32) -> Self { Self(format!("vst2:{:08x}", magic)) }
}

pub struct Registry {
    by_key: HashMap<PluginKey, PluginEntry>,
}

impl Registry {
    pub fn scan() -> Self {
        let mut by_key = HashMap::new();
        for v in crate::vst3::scan_mac_system_and_user() {
            let uid = [v.class_uid[0], v.class_uid[1], v.class_uid[2], v.class_uid[3]];
            by_key.insert(PluginKey::from_vst3_uid(uid), PluginEntry::Vst3(v));
        }
        for a in crate::au::scan_macos() {
            by_key.insert(PluginKey::from_au(a.ty, a.subtype, a.manufacturer), PluginEntry::AudioUnit(a));
        }
        Self { by_key }
    }

    pub fn resolve(&self, plid: &NksPluginId) -> Option<&PluginEntry> {
        let key = match plid {
            NksPluginId::Vst3 { uid, .. } => {
                let u = [uid[0] as u32, uid[1] as u32, uid[2] as u32, uid[3] as u32];
                PluginKey::from_vst3_uid(u)
            }
            NksPluginId::AudioUnit { ty, subtype, manufacturer, .. } =>
                PluginKey::from_au(*ty, *subtype, *manufacturer),
            NksPluginId::Vst2 { magic } => PluginKey::from_vst2(*magic),
            NksPluginId::Unknown => return None,
        };
        self.by_key.get(&key)
    }

    pub fn len(&self) -> usize { self.by_key.len() }
    pub fn is_empty(&self) -> bool { self.by_key.is_empty() }

    pub fn bundles(&self) -> Vec<PathBuf> {
        self.by_key.values().filter_map(|e| match e {
            PluginEntry::Vst3(v) => Some(v.bundle.clone()),
            _ => None,
        }).collect()
    }
}

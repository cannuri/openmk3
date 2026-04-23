//! `PLID` chunk — plugin identification.
//!
//! Observed shape: a MessagePack map with a small set of recognized keys.
//! Rather than forcing an `untagged` enum (which collides when plugins write
//! both VST3 and AU keys), we parse into a catch-all struct and classify.

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct RawPlid {
    #[serde(rename = "VST3.uid")]
    vst3_uid: Option<Vec<i64>>, // 4 × i32 on disk
    #[serde(rename = "VST3.plugin_name")]
    vst3_plugin_name: Option<String>,
    #[serde(rename = "VST.magic")]
    vst2_magic: Option<i64>,
    #[serde(rename = "AU.type")]
    au_type: Option<u32>,
    #[serde(rename = "AU.subtype")]
    au_subtype: Option<u32>,
    #[serde(rename = "AU.manufacturer")]
    au_manufacturer: Option<u32>,
    #[serde(rename = "AU.name")]
    au_name: Option<String>,
    plugin_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NksPluginId {
    Vst3 { uid: [i32; 4], name: Option<String> },
    Vst2 { magic: u32 },
    AudioUnit { ty: u32, subtype: u32, manufacturer: u32, name: Option<String> },
    Unknown,
}

impl<'de> serde::Deserialize<'de> for NksPluginId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = RawPlid::deserialize(d)?;
        if let Some(v) = raw.vst3_uid {
            if v.len() == 4 {
                let uid = [v[0] as i32, v[1] as i32, v[2] as i32, v[3] as i32];
                return Ok(NksPluginId::Vst3 { uid, name: raw.vst3_plugin_name.or(raw.plugin_name) });
            }
        }
        if let (Some(t), Some(s), Some(m)) = (raw.au_type, raw.au_subtype, raw.au_manufacturer) {
            return Ok(NksPluginId::AudioUnit { ty: t, subtype: s, manufacturer: m, name: raw.au_name.or(raw.plugin_name) });
        }
        if let Some(m) = raw.vst2_magic {
            return Ok(NksPluginId::Vst2 { magic: m as u32 });
        }
        Ok(NksPluginId::Unknown)
    }
}

//! `NISI` chunk — summary metadata.
//!
//! The MessagePack payload is a map whose keys are human-readable strings.
//! Observed keys across Kontakt, Massive X, Battery, Reaktor, u-he, and
//! Plugin-Alliance factory content:
//!
//! * `author`, `vendor`, `comment`, `name`
//! * `bankchain` — list of strings forming the library/bank hierarchy
//! * `types` — list of `[type, subtype]` string pairs
//! * `modes` — list of mode/character tag strings
//! * `deviceType` — `"INST"`, `"FX"`, `"MIDIFX"`
//!
//! Vendors omit fields liberally, so every field is optional.

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct NksSummary {
    pub author: Option<String>,
    pub vendor: Option<String>,
    pub comment: Option<String>,
    pub name: Option<String>,
    pub bankchain: Option<Vec<String>>,
    pub types: Option<Vec<Vec<String>>>,
    /// Mode/character tag strings. NKS v1 uses the key "modes"; v2 NISI
    /// chunks from Massive X and Kontakt 7 use "characters" instead. We
    /// accept either and deserialize into the same field.
    #[serde(alias = "characters")]
    pub modes: Option<Vec<String>>,
    #[serde(rename = "deviceType")]
    pub device_type: Option<String>,
    pub uuid: Option<String>,
}

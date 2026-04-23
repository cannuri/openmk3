//! `NICA` chunk — controller assignments (NKS page/knob mappings).
//!
//! Shape varies across vendors and NKS versions. For v0.1 we keep the raw
//! MessagePack bytes and let consumers decode selectively. Exposing a typed
//! surface is tracked as v0.2 work once a representative corpus is sampled.

#[derive(Debug, Clone)]
pub struct NksController {
    pub raw: Vec<u8>,
}

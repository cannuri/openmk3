//! VST3 + Audio Unit plugin registry; resolves NKS PLID → installed plugin.

pub mod vst3;
pub mod au;
pub mod resolve;

pub use resolve::{PluginEntry, PluginKey, Registry};

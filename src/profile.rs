//! Identity attached to every managed save. Names and ROM header codes are not
//! sufficient: two hacks can share both while assigning entirely different IDs.

use serde::{Deserialize, Serialize};

/// Exact interpretation context. Every field participates in compatibility.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GameProfile {
    /// Profile serialization version, independent from database schema version.
    pub schema_version: u32,
    /// Original unmodified cartridge image, even when a virtual image is larger.
    pub rom_sha256: String,
    /// Source-specific rules namespace and its explicit version.
    pub definition_id: String,
    pub definition_version: String,
    /// Reviewed codec family; this is never inferred from a ROM header alone.
    pub engine: String,
    /// Battery-save layout/version, distinct from an emulator savestate format.
    pub save_format: String,
    /// Ordered overlay manifest digest, including the exact baseline image.
    pub overlay_sha256: String,
    /// Complete immutable definition bundle digest, including custom resources.
    pub content_sha256: String,
}

impl GameProfile {
    /// Hash the serialized context, including the ordered overlays and content.
    pub fn fingerprint(&self) -> String {
        crate::sha256(&serde_json::to_vec(self).expect("profile is serializable"))
    }
}

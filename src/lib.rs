//! Universal Pokemon Vault backend. ROMs are read-only. Explicitly chosen saves
//! support backed-up, journaled writes; legacy imports preserve their source file.
//!
//! Start with `rom` for identification, `overlay` for virtual images, `gen3` for
//! verified Emerald saves, and `transfer` for the boundary between saves and Vault.
//! Native emulator ownership is isolated in `emulator`; storage never calls it.

pub mod desktop_service;
pub mod emulator;
pub mod extraction;
pub mod gen3;
pub mod linked_save;
pub mod overlay;
pub mod profile;
pub mod registry;
pub mod rom;
pub mod store;
pub mod transfer;

/// Stable content identity used for ROMs, configuration and save receipts.
pub fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}

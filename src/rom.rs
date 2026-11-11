//! Header hints are deliberately separate from exact game recognition.
//! A hack may retain every byte of a retail header while replacing its tables.

use anyhow::{ensure, Result};
use serde::Serialize;
use sha1::{Digest, Sha1};

/// SHA-256 established by a fresh `make compare` build at pret commit
/// 731ad5bfd6e6f265508d0efcca0ba42f9dcf5881. The public SHA-1 is also checked.
pub const EMERALD_RETAIL_SHA256: &str =
    "a9dec84dfe7f62ab2220bafaef7479da0929d066ece16a6885f6226db19085af";

/// Public inspection result. SHA-256 is always the source identity, even when an
/// older published SHA-1 fingerprint provides independent retail recognition.
#[derive(Clone, Debug, Serialize)]
pub struct RomIdentity {
    pub sha256: String,
    pub size: usize,
    pub platform: String,
    pub title: String,
    pub game_code: Option<String>,
    pub revision: Option<u8>,
    pub classification: String,
    pub known_game: Option<String>,
    pub header_checksum_valid: bool,
    pub diagnostics: Vec<String>,
}

/// Inspect an untrusted plain ROM. Compressed archives, NDS and fan-game packages
/// need separate adapters and are reported as unsupported rather than guessed.
pub fn identify(bytes: &[u8]) -> Result<RomIdentity> {
    ensure!(!bytes.is_empty(), "empty ROM");
    let mut out = RomIdentity {
        sha256: crate::sha256(bytes),
        size: bytes.len(),
        platform: "unsupported".into(),
        title: String::new(),
        game_code: None,
        revision: None,
        classification: "unsupported-format".into(),
        known_game: None,
        header_checksum_valid: false,
        diagnostics: Vec::new(),
    };
    // Cartridge fixed-byte/header checks are hints, not trusted engine signatures.
    if bytes.len() >= 0xc0 && bytes[0xb2] == 0x96 {
        out.platform = "GBA".into();
        out.title = ascii_label(&bytes[0xa0..0xac]);
        out.game_code = Some(ascii_label(&bytes[0xac..0xb0]));
        out.revision = Some(bytes[0xbc]);
        let checksum = bytes[0xa0..=0xbc]
            .iter()
            .fold(0u8, |v, b| v.wrapping_sub(*b))
            .wrapping_sub(0x19);
        out.header_checksum_valid = checksum == bytes[0xbd];
        out.classification = "unknown-derivative".into();
        // Published pret retail checksum. It selects only this exact vanilla
        // format. Every recognised image still carries its own SHA-256 identity.
        if out.sha256 == EMERALD_RETAIL_SHA256
            && hex::encode(Sha1::digest(bytes)) == "f3ae088181bf583e55daf962a92bb46f4f1d07b7"
        {
            out.classification = "known-game".into();
            out.known_game = Some("Pokemon Emerald (USA/Europe, retail)".into());
        }
    } else if bytes.len() >= 0x150
        && bytes[0x104..0x10c] == [0xce, 0xed, 0x66, 0x66, 0xcc, 0x0d, 0x00, 0x0b]
    {
        let color = bytes[0x143] & 0x80 != 0;
        out.platform = if color { "GBC" } else { "GB" }.into();
        out.title = ascii_label(&bytes[0x134..if color { 0x143 } else { 0x144 }]);
        out.revision = Some(bytes[0x14c]);
        out.header_checksum_valid = bytes[0x134..=0x14c]
            .iter()
            .fold(0u8, |v, b| v.wrapping_sub(*b).wrapping_sub(1))
            == bytes[0x14d];
        out.classification = "unknown-derivative".into();
        out.diagnostics
            .push("GB/GBC save adapters and exact game fingerprints are not installed".into());
    }
    if out.classification == "unknown-derivative" {
        out.diagnostics.push("Header is a platform hint only; exact engine definitions are required for save parsing".into());
    }
    if !out.header_checksum_valid {
        out.diagnostics
            .push("No valid supported cartridge header checksum".into());
    }
    Ok(out)
}

// Keep non-ASCII header bytes visible in diagnostics without terminal controls.
fn ascii_label(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                char::from(b)
            } else {
                '?'
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

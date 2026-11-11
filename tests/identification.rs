//! Synthetic headers verify that format hints never grant engine compatibility.
use std::{collections::BTreeMap, sync::Arc};
use universal_pokemon_vault::{
    overlay::{OverlayManifest, VirtualRom},
    registry::DefinitionBundle,
    rom, transfer,
};

/// Construct a valid header that deliberately resembles Emerald, with no actual
/// game code/data. Unknown hacks with unchanged headers must receive this outcome.
fn header_only() -> Vec<u8> {
    let mut bytes = vec![0u8; 512];
    bytes[0xa0..0xac].copy_from_slice(b"POKEMON EMER");
    bytes[0xac..0xb0].copy_from_slice(b"BPEE");
    bytes[0xb2] = 0x96;
    bytes[0xbd] = bytes[0xa0..=0xbc]
        .iter()
        .fold(0u8, |sum, b| sum.wrapping_sub(*b))
        .wrapping_sub(0x19);
    bytes
}

#[test]
fn matching_header_and_external_engine_label_cannot_admit_a_hack() {
    let bytes = header_only();
    let identity = rom::identify(&bytes).unwrap();
    assert!(identity.header_checksum_valid);
    assert_eq!(identity.classification, "unknown-derivative");
    assert!(identity.known_game.is_none());
    let defs = DefinitionBundle {
        schema_version: 1,
        id: "untrusted".into(),
        version: "1".into(),
        rom_sha256: identity.sha256,
        engine: "emerald-vanilla-v1".into(),
        provenance: "author claim only".into(),
        species: BTreeMap::new(),
        moves: BTreeMap::new(),
        evolutions: Vec::new(),
        extensions: BTreeMap::new(),
    };
    let base: Arc<[u8]> = bytes.into();
    let image = VirtualRom::new(base.clone(), OverlayManifest::identity(&base)).unwrap();
    assert!(transfer::profile_for(&image, &defs).is_err());
    assert!(
        universal_pokemon_vault::extraction::extract_reviewed_emerald(&image.materialize())
            .is_err()
    );
}

#[test]
fn truncated_random_and_invalid_checksum_inputs_have_clear_diagnostics() {
    assert!(rom::identify(&[]).is_err());
    assert_eq!(
        rom::identify(&[0; 20]).unwrap().classification,
        "unsupported-format"
    );
    let mut bytes = header_only();
    bytes[0xbd] ^= 1;
    let report = rom::identify(&bytes).unwrap();
    assert!(!report.header_checksum_valid);
    assert!(!report.diagnostics.is_empty());
    assert!(report.known_game.is_none());
}

#[test]
fn gb_color_header_is_a_platform_hint_only() {
    let mut bytes = vec![0u8; 0x150];
    bytes[0x104..0x10c].copy_from_slice(&[0xce, 0xed, 0x66, 0x66, 0xcc, 0x0d, 0, 0x0b]);
    bytes[0x134..0x13b].copy_from_slice(b"FIXTURE");
    bytes[0x143] = 0x80;
    bytes[0x14d] = bytes[0x134..=0x14c]
        .iter()
        .fold(0u8, |sum, b| sum.wrapping_sub(*b).wrapping_sub(1));
    let report = rom::identify(&bytes).unwrap();
    assert_eq!(report.platform, "GBC");
    assert_eq!(report.title, "FIXTURE");
    assert!(report.header_checksum_valid);
    assert!(report.known_game.is_none());
}

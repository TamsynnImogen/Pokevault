//! Real CLI I/O tests for source preservation. The store's opaque test payload
//! below is deliberately NOT a Pokémon save and is never presented as extraction.
use std::process::Command;
use universal_pokemon_vault::{profile::GameProfile, store::ManagedStore};

#[test]
fn export_and_backup_refuse_original_paths_and_keep_profiles_with_exports() {
    let directory = tempfile::tempdir().unwrap();
    let db = directory.path().join("vault.sqlite");
    let original = directory.path().join("original.sav");
    let bytes = b"opaque synthetic preservation test";
    std::fs::write(&original, bytes).unwrap();
    let profile = GameProfile {
        schema_version: 1,
        rom_sha256: "a".repeat(64),
        definition_id: "opaque-test".into(),
        definition_version: "1".into(),
        engine: "opaque-test".into(),
        save_format: "opaque-test".into(),
        overlay_sha256: "b".repeat(64),
        content_sha256: "c".repeat(64),
    };
    let mut store = ManagedStore::open(&db).unwrap();
    let id = store.import_save(&profile, bytes).unwrap();
    drop(store);
    let cli = env!("CARGO_BIN_EXE_universal-pokemon-vault");
    let export = directory.path().join("export");
    let result = Command::new(cli)
        .arg("export-save")
        .arg(&db)
        .arg(id.to_string())
        .arg(&export)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(std::fs::read(export.join("working.sav")).unwrap(), bytes);
    let exported: GameProfile =
        serde_json::from_slice(&std::fs::read(export.join("profile.json")).unwrap()).unwrap();
    assert_eq!(exported, profile);
    assert!(export.join("receipt.json").is_file());
    // Both an existing directory and an original save path must fail before writes.
    for path in [&export, &original] {
        let result = Command::new(cli)
            .arg("export-save")
            .arg(&db)
            .arg(id.to_string())
            .arg(path)
            .output()
            .unwrap();
        assert!(!result.status.success());
    }
    assert!(!Command::new(cli)
        .arg("backup")
        .arg(&db)
        .arg(&original)
        .output()
        .unwrap()
        .status
        .success());
    assert_eq!(std::fs::read(&original).unwrap(), bytes);
    assert!(Command::new(cli)
        .arg("check")
        .arg(&db)
        .output()
        .unwrap()
        .status
        .success());
}

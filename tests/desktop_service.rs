//! Headless checks of the exact service used by desktop IPC. Synthetic headers
//! exercise refusal paths; actual game data is accepted only by an opt-in test.
use serde_json::{json, Value};
use std::{fs, path::Path};
use tempfile::tempdir;
use universal_pokemon_vault::{
    desktop_service::DesktopService, profile::GameProfile, store::ManagedStore,
};

/// A valid-looking Emerald header deliberately contains no real game structures.
fn derivative_header() -> Vec<u8> {
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
fn unknown_rom_stays_inspectable_without_receiving_a_save_codec() {
    let dir = tempdir().unwrap();
    let rom = dir.path().join("unverified.gba");
    let original = derivative_header();
    fs::write(&rom, &original).unwrap();
    let root = dir.path().join("workspace");
    let mut service = DesktopService::open(&root).unwrap();
    let game = service
        .dispatch(json!({"action":"add_rom","path":rom}))
        .unwrap();
    assert!(!game["import_supported"].as_bool().unwrap());
    assert_eq!(game["classification"], "unknown-derivative");
    assert!(service
        .dispatch(json!({"action":"import_save","rom_hash":game["hash"],"path":rom}))
        .is_err());
    assert!(service
        .dispatch(json!({"action":"overview","unexpected":true}))
        .is_err());
    assert!(service
        .dispatch(json!({"action":"backup","output_path":rom}))
        .is_err());
    drop(service);
    let mut service = DesktopService::open(&root).unwrap();
    assert_eq!(
        service.dispatch(json!({"action":"overview"})).unwrap()["games"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(fs::read(&rom).unwrap(), original);
    fs::write(&rom, b"a changed file").unwrap();
    let error = service
        .dispatch(json!({"action":"import_save","rom_hash":game["hash"],"path":rom}))
        .unwrap_err();
    assert!(format!("{error:#}").contains("ROM changed"));
}

#[test]
fn workspace_refuses_existing_non_database_files_without_modifying_them() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vault.sqlite");
    let original = b"unrelated original bytes that must not become a database";
    fs::write(&path, original).unwrap();
    assert!(DesktopService::open(dir.path()).is_err());
    assert_eq!(fs::read(path).unwrap(), original);
}

#[test]
fn service_preserves_schema_one_backup_before_migration_and_shows_missing_rules() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vault.sqlite");
    // Opaque storage fixtures exercise schema migration, not Pokemon extraction.
    let profile = GameProfile {
        schema_version: 1,
        rom_sha256: "a".repeat(64),
        definition_id: "opaque".into(),
        definition_version: "1".into(),
        engine: "opaque".into(),
        save_format: "opaque".into(),
        overlay_sha256: "b".repeat(64),
        content_sha256: "c".repeat(64),
    };
    let mut store = ManagedStore::open(&path).unwrap();
    store
        .import_save(&profile, b"untouched opaque original")
        .unwrap();
    drop(store);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("DROP TABLE rom_library; PRAGMA user_version=1;")
        .unwrap();
    drop(conn);
    let mut service = DesktopService::open(dir.path()).unwrap();
    let overview = service.dispatch(json!({"action":"overview"})).unwrap();
    assert_eq!(overview["saves"][0]["party_count"], Value::Null);
    let backups = fs::read_dir(dir.path().join("backups"))
        .unwrap()
        .collect::<std::io::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(backups.len(), 1);
    let conn = rusqlite::Connection::open_with_flags(
        backups[0].path(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1, "backup must retain the original schema");
    let original: Vec<u8> = conn
        .query_row(
            "SELECT bytes FROM original_imports WHERE save_id=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(original, b"untouched opaque original");
}

/// Run the entire UI request sequence against real user-owned game/save inputs.
/// External files are read-only and all mutations stay under a temporary workspace.
#[test]
#[ignore = "set UPV_ROM and UPV_SAVE to reviewed Emerald inputs; optional UPV_CORE"]
fn desktop_requests_handle_real_import_transfer_undo_export_and_reopen() {
    let rom_path = std::env::var("UPV_ROM").expect("set UPV_ROM");
    let save_path = std::env::var("UPV_SAVE").expect("set UPV_SAVE");
    let rom_bytes = fs::read(&rom_path).unwrap();
    let save_bytes = fs::read(&save_path).unwrap();
    let dir = tempdir().unwrap();
    let mut service = DesktopService::open(dir.path()).unwrap();
    let game = service
        .dispatch(json!({"action":"add_rom","path":rom_path}))
        .unwrap();
    assert_eq!(game["import_supported"], true);
    let imported = service
        .dispatch(json!({"action":"import_save","rom_hash":game["hash"],"path":save_path}))
        .unwrap();
    let id = imported["save_id"].as_i64().unwrap();
    let original = service
        .dispatch(json!({"action":"save_view","save_id":id}))
        .unwrap();
    let boxed = original["pokemon"]
        .as_array()
        .unwrap()
        .iter()
        .find(|mon| mon["location"].get("Box").is_some())
        .expect("test needs a PC Pokemon");
    let location = boxed["location"].clone();
    let raw_hash = boxed["raw_sha256"].clone();
    let deposit = json!({"action":"deposit","save_id":id,"expected_revision":0,"locations":[location],"move_from_managed":true});
    service.dispatch(deposit.clone()).unwrap();
    // A repeated stale UI request must fail without changing the new revision.
    assert!(service.dispatch(deposit).is_err());
    let vault = service
        .dispatch(json!({"action":"vault_list","query":""}))
        .unwrap();
    assert_eq!(vault.as_array().unwrap().len(), 1);
    let record = &vault[0];
    let vault_id = record["vault_id"].clone();
    let uuid = record["uuid"].clone();
    assert_eq!(record["raw_sha256"], raw_hash);
    let empty = service
        .dispatch(json!({"action":"save_view","save_id":id}))
        .unwrap();
    let mut target = None;
    for slot in 0..30 {
        if !empty["pokemon"]
            .as_array()
            .unwrap()
            .iter()
            .any(|mon| mon["location"] == json!({"Box":{"box_index":0,"slot":slot}}))
        {
            target = Some(slot);
            break;
        }
    }
    let slot = target.unwrap();
    let preview=service.dispatch(json!({"action":"preview_withdraw","save_id":id,"vault_id":vault_id,"box_index":0,"slot":slot})).unwrap();
    assert_eq!(preview["destination_slot_empty"], true);
    assert!(preview["report"]["blocking_problems"]
        .as_array()
        .unwrap()
        .is_empty());
    let withdraw = json!({"action":"withdraw","save_id":id,"expected_revision":preview["revision"],"vault_id":vault_id,"box_index":0,"slot":slot});
    let committed = service.dispatch(withdraw.clone()).unwrap();
    assert!(service.dispatch(withdraw).is_err());
    assert!(service
        .dispatch(json!({"action":"undo","save_id":id,"expected_revision":0}))
        .is_err());
    service
        .dispatch(json!({"action":"undo","save_id":id,"expected_revision":committed["revision"]}))
        .unwrap();
    let vault = service
        .dispatch(json!({"action":"vault_list","query":uuid}))
        .unwrap();
    assert_eq!(vault[0]["uuid"], uuid);
    assert_eq!(vault[0]["raw_sha256"], raw_hash);
    let export = service
        .dispatch(json!({"action":"export_save","save_id":id}))
        .unwrap();
    let output = Path::new(export["path"].as_str().unwrap());
    assert!(
        output.join("working.sav").is_file()
            && output.join("profile.json").is_file()
            && output.join("definitions.json").is_file()
    );
    assert!(output.starts_with(dir.path()));
    service
        .dispatch(json!({"action":"integrity_check"}))
        .unwrap();
    service.dispatch(json!({"action":"backup"})).unwrap();
    if let Ok(core) = std::env::var("UPV_CORE") {
        let result=service.dispatch(json!({"action":"run_headless","rom_hash":game["hash"],"save_id":id,"core_path":core,"frames":120})).unwrap();
        assert_eq!(result["video_frames"], 120);
    }
    drop(service);
    let mut reopened = DesktopService::open(dir.path()).unwrap();
    let overview = reopened.dispatch(json!({"action":"overview"})).unwrap();
    assert_eq!(overview["games"].as_array().unwrap().len(), 1);
    assert_eq!(overview["saves"].as_array().unwrap().len(), 1);
    assert_eq!(overview["vault_count"], 1);
    assert!(!reopened
        .dispatch(json!({"action":"history","save_id":id}))
        .unwrap()
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(fs::read(rom_path).unwrap(), rom_bytes);
    assert_eq!(fs::read(save_path).unwrap(), save_bytes);
}

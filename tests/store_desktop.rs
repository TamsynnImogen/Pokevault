//! Storage contract checks for the desktop ROM library and revisioned history.

use rusqlite::Connection;
use serde_json::json;
use tempfile::tempdir;
use universal_pokemon_vault::{
    profile::GameProfile,
    store::{ManagedStore, NewVaultRecord, RomLibraryEntry},
};

fn profile() -> GameProfile {
    GameProfile {
        schema_version: 1,
        rom_sha256: "rom".into(),
        definition_id: "emerald".into(),
        definition_version: "1".into(),
        engine: "gen3".into(),
        save_format: "emerald".into(),
        overlay_sha256: "overlay".into(),
        content_sha256: "content".into(),
    }
}

fn rom(hash: &str, path: &str, definition_hash: Option<String>) -> RomLibraryEntry {
    RomLibraryEntry {
        hash: hash.into(),
        source_path: path.into(),
        identity: json!({"sha256": hash, "title": "test"}),
        definition_hash,
    }
}

#[test]
fn schema_one_migration_preserves_imports_revisions_events_uuids_and_receipts() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vault.sqlite");
    let mut store = ManagedStore::open(&path).unwrap();
    let save_id = store.import_save(&profile(), b"original").unwrap();
    let record = NewVaultRecord {
        raw: b"pokemon".to_vec(),
        normalized: json!({"species": 1}),
        source_profile: profile(),
        source_definition: json!({"version": 1}),
        import_key: Some("source:slot:0".into()),
    };
    let snapshot = store.save(save_id).unwrap();
    store
        .commit(&snapshot, b"changed", "deposit", &[record], &[])
        .unwrap();
    let vault = store.list_vault().unwrap().remove(0);
    drop(store);

    // Reconstruct the pre-migration schema from populated current tables. Only
    // schema-2 objects are removed; all schema-1 content remains byte-for-byte.
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("DROP TABLE rom_library; PRAGMA user_version = 1;")
        .unwrap();
    let before: Vec<(String, i64)> = [
        "original_imports",
        "save_revisions",
        "vault_events",
        "import_receipts",
    ]
    .iter()
    .map(|table| {
        let count: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        ((*table).into(), count)
    })
    .collect();
    drop(conn);

    let store = ManagedStore::open(&path).unwrap();
    store.integrity_check().unwrap();
    assert_eq!(store.save(save_id).unwrap().bytes, b"changed");
    assert_eq!(
        store.original_save_hash(save_id).unwrap(),
        universal_pokemon_vault::sha256(b"original")
    );
    assert_eq!(store.vault(vault.id).unwrap().uuid, vault.uuid);
    assert_eq!(store.history(Some(save_id)).unwrap().len(), 1);
    let conn = Connection::open(&path).unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 3);
    for (table, count) in before {
        let after: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(after, count, "{table} changed during migration");
    }
    assert!(conn.execute("INSERT INTO import_receipts (import_key, vault_uuid, operation_id) VALUES ('source:slot:0', 'other', 1)", []).is_err());
}

#[test]
fn rom_registration_checks_hash_definition_and_metadata() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vault.sqlite");
    let mut store = ManagedStore::open(&path).unwrap();
    let hash = "a".repeat(64);
    let definition_hash = store
        .register_definition(&json!({"name": "reviewed"}))
        .unwrap();
    let first = rom(&hash, "/old", Some(definition_hash.clone()));
    store.register_rom(&first).unwrap();
    assert_eq!(store.rom(&hash).unwrap(), first);
    let second = rom(&hash, "/new", Some(definition_hash));
    store.register_rom(&second).unwrap();
    assert_eq!(store.list_roms().unwrap(), vec![second.clone()]);
    assert!(store.register_rom(&rom("A", "/bad", None)).is_err());
    let mut mismatched = rom(&"b".repeat(64), "/bad", None);
    mismatched.identity["sha256"] = json!("c".repeat(64));
    assert!(store.register_rom(&mismatched).is_err());
    assert!(store
        .register_rom(&rom(&"b".repeat(64), "/missing", Some("c".repeat(64))))
        .is_err());
    let mut changed = second.clone();
    changed.identity = json!({"sha256": hash, "title": "changed"});
    assert!(store.register_rom(&changed).is_err());
    assert_eq!(store.rom(&hash).unwrap().source_path, "/new");
    let conn = Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE rom_library SET identity = X'00' WHERE hash = ?1",
        [&hash],
    )
    .unwrap();
    assert!(store.rom(&hash).is_err());
    assert!(store.list_roms().is_err());
    assert!(store.integrity_check().is_err());
    conn.execute(
        "UPDATE rom_library SET identity = ?1 WHERE hash = ?2",
        rusqlite::params![serde_json::to_vec(&second.identity).unwrap(), hash],
    )
    .unwrap();
    conn.execute(
        "UPDATE rom_library SET source_path = '/tampered' WHERE hash = ?1",
        [&hash],
    )
    .unwrap();
    assert!(store.rom(&hash).is_err());
}

#[test]
fn history_and_expected_revision_undo_are_atomic() {
    let mut store = ManagedStore::open(":memory:").unwrap();
    let id = store.import_save(&profile(), b"original").unwrap();
    assert_eq!(store.list_save_ids().unwrap(), vec![id]);
    let snapshot = store.save(id).unwrap();
    store
        .commit(&snapshot, b"changed", "deposit", &[], &[])
        .unwrap();
    assert!(store.undo_at_revision(id, 0).is_err());
    assert_eq!(store.save(id).unwrap().bytes, b"changed");
    assert_eq!(store.undo_at_revision(id, 1).unwrap(), 2);
    let history = store.history(Some(id)).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].kind, "undo");
    assert_eq!(history[0].after_revision, 2);
    assert!(history[1].undone);
    assert_eq!(store.history(None).unwrap(), history);
}

/// Removing a game is a visibility change, never a cascade into collections.
#[test]
fn removing_game_keeps_rules_originals_revisions_and_vault_records() {
    let mut store = ManagedStore::open(":memory:").unwrap();
    let definition = store
        .register_definition(&json!({"rules":"immutable"}))
        .unwrap();
    let hash = "a".repeat(64);
    store
        .register_rom(&rom(&hash, "/read-only.gba", Some(definition.clone())))
        .unwrap();
    let id = store.import_save(&profile(), b"original").unwrap();
    let record = NewVaultRecord {
        raw: b"record".to_vec(),
        normalized: json!({}),
        source_profile: profile(),
        source_definition: json!({}),
        import_key: Some("unique-original-slot".into()),
    };
    store
        .commit(
            &store.save(id).unwrap(),
            b"updated",
            "deposit",
            &[record],
            &[],
        )
        .unwrap();
    let before = store.list_vault().unwrap();
    store.remove_rom(&hash).unwrap();
    assert!(store.list_roms().unwrap().is_empty());
    assert_eq!(store.list_vault().unwrap(), before);
    assert_eq!(store.save(id).unwrap().bytes, b"updated");
    assert_eq!(
        store.original_save_hash(id).unwrap(),
        universal_pokemon_vault::sha256(b"original")
    );
    assert_eq!(
        store.definition(&definition).unwrap(),
        json!({"rules":"immutable"})
    );
    store.integrity_check().unwrap();
}

use rusqlite::Connection;
use tempfile::tempdir;
use universal_pokemon_vault::{
    profile::GameProfile,
    store::{ManagedStore, NewVaultRecord},
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

fn pokemon(raw: &[u8]) -> NewVaultRecord {
    NewVaultRecord {
        raw: raw.to_vec(),
        normalized: serde_json::json!({"species": "test"}),
        source_profile: profile(),
        source_definition: serde_json::json!({"version": 1}),
        import_key: None,
    }
}

#[test]
fn commit_and_undo_restore_save_and_vault_and_preserve_original() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("vault.sqlite");
    let mut store = ManagedStore::open(&db).unwrap();
    let id = store.import_save(&profile(), b"original save").unwrap();
    let snapshot = store.save(id).unwrap();
    assert_eq!(
        store
            .commit(
                &snapshot,
                b"after transfer",
                "deposit",
                &[pokemon(b"pokemon")],
                &[]
            )
            .unwrap(),
        1
    );
    assert_eq!(store.save(id).unwrap().bytes, b"after transfer");
    let records = store.list_vault().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].raw, b"pokemon");
    assert_eq!(store.undo(id).unwrap(), 2);
    assert_eq!(store.save(id).unwrap().bytes, b"original save");
    assert!(store.list_vault().unwrap().is_empty());
    let original: Vec<u8> = Connection::open(&db)
        .unwrap()
        .query_row(
            "SELECT bytes FROM original_imports WHERE save_id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(original, b"original save");
    assert!(Connection::open(&db)
        .unwrap()
        .execute(
            "UPDATE original_imports SET bytes = X'00' WHERE save_id = ?1",
            [id]
        )
        .is_err());
    assert!(Connection::open(&db)
        .unwrap()
        .execute(
            "UPDATE save_revisions SET bytes = X'00' WHERE save_id = ?1 AND revision = 0",
            [id]
        )
        .is_err());
}

#[test]
fn missing_removal_rolls_back_save_and_vault() {
    let dir = tempdir().unwrap();
    let mut store = ManagedStore::open(dir.path().join("vault.sqlite")).unwrap();
    let id = store.import_save(&profile(), b"initial").unwrap();
    let snapshot = store.save(id).unwrap();
    assert!(store
        .commit(
            &snapshot,
            b"changed",
            "withdraw",
            &[pokemon(b"extra")],
            &[999]
        )
        .is_err());
    assert_eq!(store.save(id).unwrap().revision, 0);
    assert_eq!(store.save(id).unwrap().bytes, b"initial");
    assert!(store.list_vault().unwrap().is_empty());
}

#[test]
fn stale_reader_cannot_commit() {
    let dir = tempdir().unwrap();
    let mut store = ManagedStore::open(dir.path().join("vault.sqlite")).unwrap();
    let id = store.import_save(&profile(), b"initial").unwrap();
    let stale = store.save(id).unwrap();
    store.commit(&stale, b"first", "edit", &[], &[]).unwrap();
    assert!(store
        .commit(&stale, b"second", "edit", &[pokemon(b"extra")], &[])
        .is_err());
    assert_eq!(store.save(id).unwrap().bytes, b"first");
    assert!(store.list_vault().unwrap().is_empty());
}

#[test]
fn corrupt_save_or_vault_hash_is_rejected() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("vault.sqlite");
    let mut store = ManagedStore::open(&db).unwrap();
    let id = store.import_save(&profile(), b"initial").unwrap();
    let snapshot = store.save(id).unwrap();
    store
        .commit(&snapshot, b"later", "deposit", &[pokemon(b"pokemon")], &[])
        .unwrap();
    let vault_id = store.list_vault().unwrap()[0].id;
    let conn = Connection::open(&db).unwrap();
    conn.execute("UPDATE vault SET raw = X'00' WHERE id = ?1", [vault_id])
        .unwrap();
    assert!(store.vault(vault_id).is_err());
    assert!(store.list_vault().is_err());
    conn.execute("UPDATE saves SET bytes = X'00' WHERE id = ?1", [id])
        .unwrap();
    assert!(store.save(id).is_err());
}

#[test]
fn undo_refuses_vault_entry_touched_by_other_save() {
    let dir = tempdir().unwrap();
    let mut store = ManagedStore::open(dir.path().join("vault.sqlite")).unwrap();
    let first = store.import_save(&profile(), b"first").unwrap();
    let second = store.import_save(&profile(), b"second").unwrap();
    let first_snapshot = store.save(first).unwrap();
    store
        .commit(
            &first_snapshot,
            b"first deposited",
            "deposit",
            &[pokemon(b"pokemon")],
            &[],
        )
        .unwrap();
    let vault_id = store.list_vault().unwrap()[0].id;
    let second_snapshot = store.save(second).unwrap();
    store
        .commit(
            &second_snapshot,
            b"second withdrew",
            "withdraw",
            &[],
            &[vault_id],
        )
        .unwrap();
    assert!(store.undo(first).is_err());
    assert_eq!(store.save(first).unwrap().bytes, b"first deposited");
    assert_eq!(store.save(second).unwrap().bytes, b"second withdrew");
    assert!(store.list_vault().unwrap().is_empty());
    // Undoing the later transfer restores the record before the first undo.
    store.undo(second).unwrap();
    assert_eq!(store.vault(vault_id).unwrap().raw, b"pokemon");
    assert!(store.undo(first).is_err()); // The record has still been touched later.
}

#[test]
fn session_lease_blocks_other_writers_and_allows_save_only_commit() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("vault.sqlite");
    let mut first = ManagedStore::open(&db).unwrap();
    let mut second = ManagedStore::open(&db).unwrap();
    let id = first.import_save(&profile(), b"initial").unwrap();
    let snapshot = first.save(id).unwrap();
    assert!(first.acquire_session(id, 1).is_err());
    let lease = first.acquire_session(id, 0).unwrap();
    assert!(second.acquire_session(id, 0).is_err());
    assert!(second
        .commit(&snapshot, b"transfer", "deposit", &[pokemon(b"x")], &[])
        .is_err());
    assert!(second.undo(id).is_err());
    assert_eq!(
        first.commit_session(&snapshot, b"played", &lease).unwrap(),
        1
    );
    assert_eq!(second.save(id).unwrap().bytes, b"played");
    assert!(second
        .commit(&second.save(id).unwrap(), b"transfer", "deposit", &[], &[])
        .is_err());
    assert!(second.recover_session(id, uuid::Uuid::new_v4()).is_err());
    first.release_session(&lease).unwrap();
    let fresh = second.save(id).unwrap();
    second
        .commit(&fresh, b"transfer", "deposit", &[pokemon(b"x")], &[])
        .unwrap();
}

#[test]
fn vault_uuid_survives_withdrawal_undo() {
    let dir = tempdir().unwrap();
    let mut store = ManagedStore::open(dir.path().join("vault.sqlite")).unwrap();
    let id = store.import_save(&profile(), b"initial").unwrap();
    let snapshot = store.save(id).unwrap();
    store
        .commit(&snapshot, b"deposited", "deposit", &[pokemon(b"x")], &[])
        .unwrap();
    let record = store.list_vault().unwrap().remove(0);
    let snapshot = store.save(id).unwrap();
    store
        .commit(&snapshot, b"withdrawn", "withdraw", &[], &[record.id])
        .unwrap();
    store.undo(id).unwrap();
    assert_eq!(store.vault(record.id).unwrap().uuid, record.uuid);
}

#[test]
fn newer_schema_version_is_rejected_without_changes() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("vault.sqlite");
    let store = ManagedStore::open(&db).unwrap();
    drop(store);
    let conn = Connection::open(&db).unwrap();
    conn.pragma_update(None, "user_version", 4).unwrap();
    drop(conn);
    assert!(ManagedStore::open(&db).is_err());
    let conn = Connection::open(&db).unwrap();
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 4);
}

#[test]
fn backup_exports_checked_database_without_overwriting() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source.sqlite");
    let backup = dir.path().join("backup.sqlite");
    let mut store = ManagedStore::open(&source).unwrap();
    let id = store.import_save(&profile(), b"original").unwrap();
    store.integrity_check().unwrap();
    store.export_backup(&backup).unwrap();
    assert_eq!(
        ManagedStore::open(&backup).unwrap().save(id).unwrap().bytes,
        b"original"
    );
    assert!(store.export_backup(&backup).is_err());
}

#[test]
fn import_receipt_blocks_replay_but_allows_other_slot_with_same_bytes() {
    let dir = tempdir().unwrap();
    let mut store = ManagedStore::open(dir.path().join("vault.sqlite")).unwrap();
    let id = store.import_save(&profile(), b"initial").unwrap();
    let snapshot = store.save(id).unwrap();
    let mut first = pokemon(b"identical");
    first.import_key = Some("save:0:box:0:rawhash".into());
    store
        .commit(&snapshot, b"first", "deposit", &[first.clone()], &[])
        .unwrap();
    store.undo(id).unwrap();
    let snapshot = store.save(id).unwrap();
    assert!(store
        .commit(&snapshot, b"replay", "deposit", &[first], &[])
        .is_err());
    assert_eq!(store.save(id).unwrap().bytes, b"initial");
    let mut other_slot = pokemon(b"identical");
    other_slot.import_key = Some("save:0:box:1:rawhash".into());
    store
        .commit(&snapshot, b"other slot", "deposit", &[other_slot], &[])
        .unwrap();
    assert_eq!(store.list_vault().unwrap().len(), 1);
}

#[test]
fn definition_snapshots_are_deduplicated_immutable_and_hash_checked() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("vault.sqlite");
    let mut store = ManagedStore::open(&db).unwrap();
    let definition = serde_json::json!({"species": [1, 2], "version": "a"});
    let hash = store.register_definition(&definition).unwrap();
    assert_eq!(store.register_definition(&definition).unwrap(), hash);
    assert_eq!(store.definition(&hash).unwrap(), definition);
    let conn = Connection::open(&db).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM definition_snapshots", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
    assert!(conn
        .execute(
            "UPDATE definition_snapshots SET bytes = X'00' WHERE hash = ?1",
            [&hash]
        )
        .is_err());
    // A direct legacy/corrupt row with a false digest is rejected on both paths.
    conn.execute(
        "INSERT INTO definition_snapshots (hash, bytes) VALUES ('false', X'00')",
        [],
    )
    .unwrap();
    assert!(store.definition("false").is_err());
    assert!(store.integrity_check().is_err());
}

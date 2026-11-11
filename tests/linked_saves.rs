//! Headless publication tests with synthetic opaque save bytes. Engine admission
//! belongs to the desktop; these tests exercise only its storage boundary.

use std::{fs, path::PathBuf, sync::Arc};

use serde_json::json;
use tempfile::tempdir;
use universal_pokemon_vault::{
    linked_save::{read_regular, LinkedSaves},
    profile::GameProfile,
    sha256,
    store::{ManagedStore, NewVaultRecord},
};

fn profile() -> GameProfile {
    GameProfile {
        schema_version: 1,
        rom_sha256: "synthetic-rom".into(),
        definition_id: "synthetic".into(),
        definition_version: "1".into(),
        engine: "synthetic".into(),
        save_format: "opaque".into(),
        overlay_sha256: "synthetic-overlay".into(),
        content_sha256: "synthetic-content".into(),
    }
}

fn record(raw: &[u8]) -> NewVaultRecord {
    NewVaultRecord {
        raw: raw.to_vec(),
        normalized: json!({"species": "synthetic"}),
        source_profile: profile(),
        source_definition: json!({"version": 1}),
        import_key: None,
    }
}

fn setup() -> (
    tempfile::TempDir,
    ManagedStore,
    Arc<LinkedSaves>,
    i64,
    PathBuf,
) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("chosen.sav");
    fs::write(&path, b"original").unwrap();
    let path = path.canonicalize().unwrap();
    let mut store = ManagedStore::open(dir.path().join("vault.sqlite")).unwrap();
    let id = store.import_save(&profile(), b"original").unwrap();
    let links = Arc::new(LinkedSaves::open(dir.path()).unwrap());
    links.bind(path.clone(), &store.save(id).unwrap()).unwrap();
    let writer = Arc::clone(&links);
    store.set_save_writer(Some(Box::new(move |id, rev, before, after| {
        writer.publish(id, rev, before, after)
    })));
    (dir, store, links, id, path)
}

fn backup_images(dir: &tempfile::TempDir) -> Vec<Vec<u8>> {
    fs::read_dir(dir.path().join("linked-saves/backups"))
        .unwrap()
        .map(|entry| fs::read(entry.unwrap().path()).unwrap())
        .collect()
}

#[test]
fn committed_publication_updates_chosen_file_and_keeps_both_backups() {
    let (dir, mut store, links, id, path) = setup();
    assert_eq!(links.status(&store.save(id).unwrap()).unwrap().0, "linked");
    let snapshot = store.save(id).unwrap();
    assert_eq!(
        store
            .commit(&snapshot, b"deposited", "deposit", &[record(b"mon")], &[])
            .unwrap(),
        1
    );
    assert_eq!(fs::read(&path).unwrap(), b"deposited");
    assert_eq!(store.list_vault().unwrap()[0].raw, b"mon");
    assert!(links.pending());
    let images = backup_images(&dir);
    assert_eq!(images.len(), 2);
    assert!(images.contains(&b"original".to_vec()));
    assert!(images.contains(&b"deposited".to_vec()));

    store.set_save_writer(None);
    links.recover(&store).unwrap();
    assert!(!links.pending());
    assert_eq!(links.status(&store.save(id).unwrap()).unwrap().0, "linked");
    assert_eq!(links.link(id).unwrap().unwrap().path, path);
    assert_eq!(backup_images(&dir).len(), 2);
}

#[test]
fn failed_publication_after_external_edit_rolls_back_sql_and_preserves_file() {
    let (_dir, mut store, links, id, path) = setup();
    fs::write(&path, b"emulator edit").unwrap();
    let snapshot = store.save(id).unwrap();
    assert!(store
        .commit(&snapshot, b"new", "deposit", &[record(b"mon")], &[])
        .is_err());
    assert_eq!(fs::read(path).unwrap(), b"emulator edit");
    assert_eq!(store.save(id).unwrap().revision, 0);
    assert_eq!(store.save(id).unwrap().bytes, b"original");
    assert!(store.list_vault().unwrap().is_empty());
    assert!(store.history(Some(id)).unwrap().is_empty());
    assert!(!links.pending());
}

#[test]
fn recovery_restores_file_when_database_did_not_commit() {
    let (dir, mut store, links, id, path) = setup();
    links.publish(id, 0, b"original", b"uncommitted").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"uncommitted");
    store.set_save_writer(None);
    drop(links);
    let reopened = LinkedSaves::open(dir.path()).unwrap();
    reopened.recover(&store).unwrap();
    assert_eq!(fs::read(path).unwrap(), b"original");
    assert!(!reopened.pending());
    assert_eq!(
        reopened.status(&store.save(id).unwrap()).unwrap().0,
        "linked"
    );
    assert_eq!(backup_images(&dir).len(), 2);
}

#[test]
fn recovery_finalizes_committed_database_after_reopen() {
    let (dir, mut store, links, id, path) = setup();
    let snapshot = store.save(id).unwrap();
    store
        .commit(&snapshot, b"committed", "edit", &[], &[])
        .unwrap();
    store.set_save_writer(None);
    drop(links);
    drop(store);
    let store = ManagedStore::open(dir.path().join("vault.sqlite")).unwrap();
    let reopened = LinkedSaves::open(dir.path()).unwrap();
    assert!(reopened.pending());
    reopened.recover(&store).unwrap();
    assert_eq!(fs::read(path).unwrap(), b"committed");
    assert_eq!(store.save(id).unwrap().revision, 1);
    assert_eq!(
        reopened.status(&store.save(id).unwrap()).unwrap().0,
        "linked"
    );
}

#[test]
fn recovery_conflict_keeps_external_bytes_intent_and_backups() {
    let (dir, mut store, links, id, path) = setup();
    links.publish(id, 0, b"original", b"uncommitted").unwrap();
    fs::write(&path, b"external third version").unwrap();
    store.set_save_writer(None);
    assert!(links.recover(&store).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"external third version");
    assert_eq!(store.save(id).unwrap().revision, 0);
    assert!(links.pending());
    assert_eq!(backup_images(&dir).len(), 2);
    assert!(dir.path().join("linked-saves/pending.json").is_file());
}

#[test]
fn explicit_resolution_requires_current_preview_and_restores_uncommitted_database_image() {
    let (dir, mut store, links, id, path) = setup();
    links.publish(id, 0, b"original", b"uncommitted").unwrap();
    fs::write(&path, b"external third version").unwrap();
    store.set_save_writer(None);
    assert!(links.recover(&store).is_err());
    let pending = dir.path().join("linked-saves/pending.json");
    let intent = fs::read(&pending).unwrap();
    let backup_count = backup_images(&dir).len();
    assert!(links
        .resolve(&store, id, 1, &sha256(b"external third version"))
        .is_err());
    assert!(links
        .resolve(&store, id, 0, &sha256(b"stale preview"))
        .is_err());
    assert_eq!(fs::read(&path).unwrap(), b"external third version");
    assert_eq!(fs::read(&pending).unwrap(), intent);
    assert_eq!(backup_images(&dir).len(), backup_count);

    links
        .resolve(&store, id, 0, &sha256(b"external third version"))
        .unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"original");
    assert_eq!(store.save(id).unwrap().revision, 0);
    assert_eq!(links.status(&store.save(id).unwrap()).unwrap().0, "linked");
    assert!(!links.pending());
    assert!(!pending.exists());
    assert!(backup_images(&dir).contains(&b"external third version".to_vec()));
    assert_eq!(backup_images(&dir).len(), backup_count + 1);
}

#[test]
fn explicit_resolution_preserves_committed_database_and_vault_record() {
    let (dir, mut store, links, id, path) = setup();
    let snapshot = store.save(id).unwrap();
    store
        .commit(&snapshot, b"committed", "deposit", &[record(b"mon")], &[])
        .unwrap();
    store.set_save_writer(None);
    fs::write(&path, b"external edit").unwrap();
    assert!(links.recover(&store).is_err());
    links
        .resolve(&store, id, 1, &sha256(b"external edit"))
        .unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"committed");
    assert_eq!(store.save(id).unwrap().revision, 1);
    assert_eq!(store.list_vault().unwrap()[0].raw, b"mon");
    assert!(!links.pending());
    assert_eq!(links.status(&store.save(id).unwrap()).unwrap().0, "linked");
    assert!(backup_images(&dir).contains(&b"external edit".to_vec()));
}

#[test]
fn linked_vault_removal_and_insertion_commit_together() {
    let (_dir, mut store, links, id, path) = setup();
    let snapshot = store.save(id).unwrap();
    store
        .commit(&snapshot, b"first", "deposit", &[record(b"first mon")], &[])
        .unwrap();
    store.set_save_writer(None);
    links.recover(&store).unwrap();
    let old_id = store.list_vault().unwrap()[0].id;
    let writer = Arc::clone(&links);
    store.set_save_writer(Some(Box::new(move |id, rev, before, after| {
        writer.publish(id, rev, before, after)
    })));
    let snapshot = store.save(id).unwrap();
    store
        .commit(
            &snapshot,
            b"second",
            "swap",
            &[record(b"second mon")],
            &[old_id],
        )
        .unwrap();
    assert_eq!(fs::read(path).unwrap(), b"second");
    assert_eq!(store.save(id).unwrap().revision, 2);
    assert_eq!(store.list_vault().unwrap().len(), 1);
    assert_eq!(store.list_vault().unwrap()[0].raw, b"second mon");
    store.set_save_writer(None);
    links.recover(&store).unwrap();
    assert_eq!(links.status(&store.save(id).unwrap()).unwrap().0, "linked");
}

#[test]
fn workspace_lock_excludes_a_second_link_manager() {
    let dir = tempdir().unwrap();
    let first = LinkedSaves::open(dir.path()).unwrap();
    assert!(LinkedSaves::open(dir.path()).is_err());
    drop(first);
    assert!(LinkedSaves::open(dir.path()).is_ok());
}

#[cfg(unix)]
#[test]
fn symbolic_and_hard_linked_saves_are_refused() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let source = dir.path().join("source.sav");
    let symbolic = dir.path().join("symbolic.sav");
    let hard = dir.path().join("hard.sav");
    fs::write(&source, b"opaque").unwrap();
    symlink(&source, &symbolic).unwrap();
    assert!(read_regular(&symbolic, 128 * 1024).is_err());
    fs::hard_link(&source, &hard).unwrap();
    assert!(read_regular(&source, 128 * 1024).is_err());
    assert!(read_regular(&hard, 128 * 1024).is_err());
}

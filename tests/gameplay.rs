//! Opt-in validation of user-owned gameplay saves. No ROM or real save is bundled,
//! and all database/native mutations target temporary application-owned copies.
use std::{path::Path, sync::Arc};
use universal_pokemon_vault::{
    extraction,
    gen3::{EmeraldSave, Location},
    overlay::{OverlayManifest, VirtualRom},
    store::ManagedStore,
    transfer,
};

/// Exercise real extraction, read-only import, permanent UUIDs, exact raw bytes,
/// historical definitions, atomic undo, and an optional native save session.
#[test]
#[ignore = "set UPV_ROM and UPV_SAVE to exact supported ROM and gameplay battery save; optional UPV_CORE"]
fn gameplay_save_is_preserved_across_vault_and_native_operations() {
    let rom_path = std::env::var("UPV_ROM").expect("set UPV_ROM");
    let save_path = std::env::var("UPV_SAVE").expect("set UPV_SAVE");
    let rom = std::fs::read(&rom_path).unwrap();
    let original = std::fs::read(&save_path).unwrap();
    let source_hash = universal_pokemon_vault::sha256(&original);
    let definitions = extraction::extract_reviewed_emerald(&rom).unwrap();
    let image = VirtualRom::new(Arc::from(rom.clone()), OverlayManifest::identity(&rom)).unwrap();
    let profile = transfer::profile_for(&image, &definitions).unwrap();
    let pokemon = EmeraldSave::parse(&original).unwrap().pokemon().unwrap();
    assert!(
        !pokemon.is_empty(),
        "gameplay save needs at least one Pokemon"
    );
    let locations = pokemon
        .iter()
        .map(|mon| mon.location.clone())
        .collect::<Vec<_>>();
    let directory = tempfile::tempdir().unwrap();
    let mut store = ManagedStore::open(directory.path().join("vault.sqlite")).unwrap();
    let id = transfer::import_save(&mut store, &profile, &definitions, &original).unwrap();
    transfer::deposit(&mut store, id, &definitions, &locations, false).unwrap();
    assert_eq!(
        store.save(id).unwrap().bytes,
        original,
        "copy import must not edit working bytes"
    );
    let entries = store.list_vault().unwrap();
    assert_eq!(entries.len(), pokemon.len());
    for (record, mon) in entries.iter().zip(&pokemon) {
        assert_eq!(
            record.raw, mon.raw,
            "all encrypted source bytes must survive"
        );
        let hash = record.source_definition["bundle_sha256"].as_str().unwrap();
        let snapshot: universal_pokemon_vault::registry::DefinitionBundle =
            serde_json::from_value(store.definition(hash).unwrap()).unwrap();
        assert_eq!(snapshot, definitions);
        if matches!(mon.location, Location::Party { .. }) {
            assert!(
                !transfer::preview(&store, record, &store.save(id).unwrap(), &definitions)
                    .unwrap()
                    .blocking_problems
                    .is_empty(),
                "unimplemented party-to-PC conversion must remain blocked"
            );
        }
    }
    // Undo removes the active imported instances but preserves immutable import
    // backups/history. This test never writes back to the supplied battery file.
    store.undo(id).unwrap();
    assert!(store.list_vault().unwrap().is_empty());
    assert_eq!(store.save(id).unwrap().bytes, original);
    assert_eq!(store.original_save_hash(id).unwrap(), source_hash);
    store.integrity_check().unwrap();

    // PC transfer proof is conditional on actual boxed records. Report the scope
    // explicitly when an early-game save has only a party starter.
    let boxed = pokemon
        .iter()
        .find(|mon| matches!(mon.location, Location::Box { .. }));
    if let Some(mon) = boxed {
        let mut pc_store = ManagedStore::open(":memory:").unwrap();
        let pc_id =
            transfer::import_save(&mut pc_store, &profile, &definitions, &original).unwrap();
        transfer::deposit(
            &mut pc_store,
            pc_id,
            &definitions,
            std::slice::from_ref(&mon.location),
            true,
        )
        .unwrap();
        let entry = pc_store.list_vault().unwrap().remove(0);
        let Location::Box { box_index, slot } = mon.location else {
            unreachable!()
        };
        transfer::withdraw(
            &mut pc_store,
            pc_id,
            entry.id,
            &definitions,
            box_index,
            slot,
        )
        .unwrap();
        assert_eq!(
            pc_store.save(pc_id).unwrap().bytes,
            original,
            "roundtrip must recover the complete image"
        );
        pc_store.undo(pc_id).unwrap();
        assert_eq!(pc_store.list_vault().unwrap()[0].uuid, entry.uuid);
        pc_store.integrity_check().unwrap();
    }
    println!(
        "gameplay records={}, pc_roundtrip_exercised={}",
        pokemon.len(),
        boxed.is_some()
    );

    if let Ok(core) = std::env::var("UPV_CORE") {
        let result =
            transfer::emulate_managed(&mut store, id, &definitions, Path::new(&core), &image, 120)
                .unwrap();
        assert_eq!(result.video_frames, 120);
        // Startup has no user input; it must not drop or replace the saved party.
        let after = EmeraldSave::parse(&store.save(id).unwrap().bytes)
            .unwrap()
            .pokemon()
            .unwrap();
        assert_eq!(
            after.iter().map(|p| &p.raw).collect::<Vec<_>>(),
            pokemon.iter().map(|p| &p.raw).collect::<Vec<_>>()
        );
        assert_eq!(store.original_save_hash(id).unwrap(), source_hash);
        store.integrity_check().unwrap();
        println!("native gameplay-save video frames={}", result.video_frames);
    }
    assert_eq!(std::fs::read(save_path).unwrap(), original);
    assert_eq!(std::fs::read(rom_path).unwrap(), rom);
}

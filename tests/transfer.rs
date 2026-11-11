//! Synthetic end-to-end integrity tests, not evidence of in-game save acceptance.
#![allow(dead_code)]
mod common;

use serde_json::json;
use std::collections::BTreeMap;
use universal_pokemon_vault::{
    gen3::{EmeraldSave, Location},
    profile::GameProfile,
    registry::{DefinitionBundle, MoveDefinition, SpeciesDefinition},
    store::ManagedStore,
    transfer,
};

/// Tiny rules fixture describes only handcrafted fixture records, never a ROM.
fn definitions() -> DefinitionBundle {
    DefinitionBundle {
        schema_version: 1,
        id: "synthetic".into(),
        version: "1".into(),
        rom_sha256: "a".repeat(64),
        engine: "emerald-vanilla-v1".into(),
        provenance: "synthetic format tests only".into(),
        species: BTreeMap::from([(
            25,
            SpeciesDefinition {
                namespace: "synthetic".into(),
                name: "fixture-species".into(),
                canonical_id: None,
                data: json!({}),
                missing: vec!["all species mechanics/resources".into()],
            },
        )]),
        moves: (1..=4)
            .map(|id| {
                (
                    id,
                    MoveDefinition {
                        namespace: "synthetic".into(),
                        name: format!("fixture-move-{id}"),
                        canonical_id: None,
                        aliases: Vec::new(),
                        mechanics: json!({"synthetic_id":id}),
                    },
                )
            })
            .collect(),
        evolutions: Vec::new(),
        extensions: BTreeMap::new(),
    }
}

/// No ROM is admitted here; service tests directly construct a known context.
fn profile(bundle: &DefinitionBundle) -> GameProfile {
    GameProfile {
        schema_version: 1,
        rom_sha256: bundle.rom_sha256.clone(),
        definition_id: bundle.id.clone(),
        definition_version: bundle.version.clone(),
        engine: bundle.engine.clone(),
        save_format: "emerald-flash128-v1".into(),
        overlay_sha256: "b".repeat(64),
        content_sha256: bundle.digest(),
    }
}

#[test]
fn managed_deposit_withdraw_and_undo_preserve_individual_and_original() {
    let mut store = ManagedStore::open(":memory:").unwrap();
    let defs = definitions();
    let original = common::fixture_save();
    let id = transfer::import_save(&mut store, &profile(&defs), &defs, &original).unwrap();
    let location = Location::Box {
        box_index: 0,
        slot: 0,
    };
    transfer::deposit(&mut store, id, &defs, &[location], true).unwrap();
    assert!(EmeraldSave::parse(&store.save(id).unwrap().bytes)
        .unwrap()
        .box_record(0, 0)
        .unwrap()
        .iter()
        .all(|b| *b == 0));
    let record = store.list_vault().unwrap().remove(0);
    let report = transfer::preview(&store, &record, &store.save(id).unwrap(), &defs).unwrap();
    assert!(report.blocking_problems.is_empty());
    transfer::withdraw(&mut store, id, record.id, &defs, 1, 19).unwrap();
    assert!(store.list_vault().unwrap().is_empty());
    assert_eq!(
        EmeraldSave::parse(&store.save(id).unwrap().bytes)
            .unwrap()
            .box_record(1, 19)
            .unwrap(),
        record.raw
    );
    store.undo(id).unwrap();
    assert_eq!(store.list_vault().unwrap()[0].uuid, record.uuid);
    assert_eq!(
        store.original_save_hash(id).unwrap(),
        universal_pokemon_vault::sha256(&original)
    );
    store.integrity_check().unwrap();
}

/// Earlier copy-only imports must not force a second Vault individual when the
/// user later chooses a real deposit. Its undo keeps that pre-existing copy.
#[test]
fn copied_source_can_be_deposited_without_duplicating_its_uuid() {
    let mut store = ManagedStore::open(":memory:").unwrap();
    let defs = definitions();
    let bytes = common::fixture_save();
    let id = transfer::import_save(&mut store, &profile(&defs), &defs, &bytes).unwrap();
    let location = Location::Box {
        box_index: 0,
        slot: 0,
    };
    transfer::deposit(
        &mut store,
        id,
        &defs,
        std::slice::from_ref(&location),
        false,
    )
    .unwrap();
    let copied = store.list_vault().unwrap().remove(0);
    // Choosing a previously copied external source creates a new managed ID;
    // its exact receipt must still reuse the existing Vault individual.
    let chosen_id = transfer::import_save(&mut store, &profile(&defs), &defs, &bytes).unwrap();
    transfer::deposit(&mut store, chosen_id, &defs, &[location], true).unwrap();
    assert_eq!(store.list_vault().unwrap(), vec![copied.clone()]);
    assert!(EmeraldSave::parse(&store.save(chosen_id).unwrap().bytes)
        .unwrap()
        .box_record(0, 0)
        .unwrap()
        .iter()
        .all(|b| *b == 0));
    // An older copy undo cannot delete the record protecting an emptied source.
    assert!(store.undo(id).is_err());
    store.undo(chosen_id).unwrap();
    assert_eq!(store.save(chosen_id).unwrap().bytes, bytes);
    assert_eq!(store.list_vault().unwrap(), vec![copied]);
    store.integrity_check().unwrap();
}

/// A return to the original PC slot reuses the verified withdrawn identity;
/// receipt replay after unrelated operations remains covered separately.
#[test]
fn deposit_withdraw_redeposit_preserves_vault_identity() {
    let mut store = ManagedStore::open(":memory:").unwrap();
    let defs = definitions();
    let id =
        transfer::import_save(&mut store, &profile(&defs), &defs, &common::fixture_save()).unwrap();
    let location = Location::Box {
        box_index: 0,
        slot: 0,
    };
    transfer::deposit(&mut store, id, &defs, std::slice::from_ref(&location), true).unwrap();
    let first = store.list_vault().unwrap().remove(0);
    transfer::withdraw(&mut store, id, first.id, &defs, 0, 0).unwrap();
    assert!(store.list_vault().unwrap().is_empty());
    transfer::deposit(&mut store, id, &defs, &[location], true).unwrap();
    let returned = store.list_vault().unwrap().remove(0);
    assert_eq!(first.uuid, returned.uuid);
    assert_eq!(first.raw, returned.raw);
    store.undo(id).unwrap();
    assert!(store.list_vault().unwrap().is_empty());
    store.integrity_check().unwrap();
}

#[test]
fn copy_import_dedupes_source_replay_but_distinguishes_slots() {
    let mut store = ManagedStore::open(":memory:").unwrap();
    let defs = definitions();
    let bytes = common::fixture_save();
    let id = transfer::import_save(&mut store, &profile(&defs), &defs, &bytes).unwrap();
    transfer::deposit(
        &mut store,
        id,
        &defs,
        &[Location::Box {
            box_index: 0,
            slot: 0,
        }],
        false,
    )
    .unwrap();
    assert_eq!(store.save(id).unwrap().bytes, bytes);
    let revision = store.save(id).unwrap().revision;
    assert!(transfer::deposit(
        &mut store,
        id,
        &defs,
        &[Location::Box {
            box_index: 0,
            slot: 0
        }],
        false
    )
    .is_err());
    assert_eq!(store.save(id).unwrap().revision, revision);
    // Re-importing the same original does not evade historical receipts.
    let duplicate_save = transfer::import_save(&mut store, &profile(&defs), &defs, &bytes).unwrap();
    assert!(transfer::deposit(
        &mut store,
        duplicate_save,
        &defs,
        &[Location::Box {
            box_index: 0,
            slot: 0
        }],
        false
    )
    .is_err());
    // Party and PC fixture records are legitimately byte-identical in their core,
    // yet their source locations identify separate individuals and both may import.
    transfer::deposit(&mut store, id, &defs, &[Location::Party { slot: 0 }], false).unwrap();
    assert_eq!(store.list_vault().unwrap().len(), 2);
}

#[test]
fn blocks_profile_drift_occupied_slots_live_session_and_atomic_partial_box() {
    let mut store = ManagedStore::open(":memory:").unwrap();
    let defs = definitions();
    let id =
        transfer::import_save(&mut store, &profile(&defs), &defs, &common::fixture_save()).unwrap();
    let original = store.save(id).unwrap().bytes;
    // An empty later selection cannot leave an earlier removal committed.
    assert!(transfer::deposit(
        &mut store,
        id,
        &defs,
        &[
            Location::Box {
                box_index: 0,
                slot: 0
            },
            Location::Box {
                box_index: 0,
                slot: 1
            }
        ],
        true
    )
    .is_err());
    assert_eq!(store.save(id).unwrap().bytes, original);
    assert!(store.list_vault().unwrap().is_empty());
    let lease = store.acquire_session(id, 0).unwrap();
    assert!(transfer::deposit(
        &mut store,
        id,
        &defs,
        &[Location::Box {
            box_index: 0,
            slot: 0
        }],
        true
    )
    .is_err());
    store.release_session(&lease).unwrap();
    transfer::deposit(
        &mut store,
        id,
        &defs,
        &[Location::Box {
            box_index: 0,
            slot: 0,
        }],
        false,
    )
    .unwrap();
    let record = store.list_vault().unwrap().remove(0);
    assert!(transfer::withdraw(&mut store, id, record.id, &defs, 0, 0).is_err());
    let mut changed = defs.clone();
    changed.version = "changed".into();
    assert!(transfer::withdraw(&mut store, id, record.id, &changed, 0, 1).is_err());
    let other = transfer::import_save(&mut store, &profile(&changed), &changed, &original).unwrap();
    assert!(
        !transfer::preview(&store, &record, &store.save(other).unwrap(), &changed)
            .unwrap()
            .blocking_problems
            .is_empty()
    );
    assert_eq!(store.save(id).unwrap().bytes, original);
}

#[test]
fn dependency_preview_uses_definitions_instead_of_shared_numeric_ids() {
    let mut store = ManagedStore::open(":memory:").unwrap();
    let defs = definitions();
    let original = common::fixture_save();
    let id = transfer::import_save(&mut store, &profile(&defs), &defs, &original).unwrap();
    transfer::deposit(
        &mut store,
        id,
        &defs,
        &[Location::Box {
            box_index: 0,
            slot: 0,
        }],
        false,
    )
    .unwrap();
    let record = store.list_vault().unwrap().remove(0);
    let mut incompatible = defs.clone();
    incompatible.version = "other-mechanics".into();
    incompatible.species.get_mut(&25).unwrap().data = json!({"changed_mechanic":true});
    incompatible.moves.get_mut(&1).unwrap().mechanics = json!({"changed_effect":true});
    let other = transfer::import_save(
        &mut store,
        &profile(&incompatible),
        &incompatible,
        &original,
    )
    .unwrap();
    let report =
        transfer::preview(&store, &record, &store.save(other).unwrap(), &incompatible).unwrap();
    // Same IDs and labels do not conceal changed mechanics.
    assert!(report
        .required_dependencies
        .iter()
        .any(|s| s.contains("source species")));
    assert!(report
        .required_dependencies
        .iter()
        .any(|s| s.contains("source move")));
    assert!(!report.blocking_problems.is_empty());
}

#[test]
fn desktop_transfer_revision_checks_reject_stale_selections_and_previews() {
    let mut store = ManagedStore::open(":memory:").unwrap();
    let defs = definitions();
    let original = common::fixture_save();
    let id = transfer::import_save(&mut store, &profile(&defs), &defs, &original).unwrap();
    let selected = [Location::Box {
        box_index: 0,
        slot: 0,
    }];
    assert!(transfer::deposit_at_revision(&mut store, id, 99, &defs, &selected, true).is_err());
    assert_eq!(store.save(id).unwrap().bytes, original);
    assert!(store.list_vault().unwrap().is_empty());
    transfer::deposit_at_revision(&mut store, id, 0, &defs, &selected, true).unwrap();
    let record = store.list_vault().unwrap().remove(0);
    let deposited = store.save(id).unwrap().bytes;
    assert!(transfer::withdraw_at_revision(&mut store, id, 0, record.id, &defs, 0, 1).is_err());
    assert_eq!(store.save(id).unwrap().bytes, deposited);
    assert_eq!(store.vault(record.id).unwrap().uuid, record.uuid);
    transfer::withdraw_at_revision(&mut store, id, 1, record.id, &defs, 0, 1).unwrap();
    assert!(store.list_vault().unwrap().is_empty());
}

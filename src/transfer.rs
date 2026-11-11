//! Transactional operations above the opaque store and the Emerald codec.
//!
//! Preview is pure analysis. Imports copy by default; explicit managed deposits
//! remove only PC slots in SQLite copies. No function accepts an original save
//! output path, and session leases exclude emulation from all transfer commits.

use anyhow::{ensure, Context, Result};
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeSet;

use crate::{
    gen3::{EmeraldSave, Location},
    overlay::VirtualRom,
    profile::GameProfile,
    registry::DefinitionBundle,
    store::{ManagedSave, ManagedStore, NewVaultRecord, VaultRecord},
};

/// Exact locally analysed modern build, distinct from pret's matching retail ROM.
/// Its table extraction is diagnostic; native real-save acceptance remains open.
pub const LOCAL_MODERN_SHA256: &str =
    "688999ce6350ff6369e9e1bb7be86d1a0230aadebe54da12d5ad20ed05b24bd5";

/// Bind a verified engine codec to exact original/effective images and definitions.
/// A matching Emerald header or an author-supplied engine label cannot admit a
/// new hack into the vanilla codec. Adding a build requires source/layout review.
pub fn profile_for(rom: &VirtualRom, definitions: &DefinitionBundle) -> Result<GameProfile> {
    definitions.validate()?;
    let image = rom.materialize();
    let identity = crate::rom::identify(&image)?;
    ensure!(
        identity.header_checksum_valid && identity.platform == "GBA",
        "invalid GBA header"
    );
    ensure!(
        definitions.rom_sha256 == identity.sha256,
        "definitions do not match effective ROM hash"
    );
    ensure!(
        definitions.engine == "emerald-vanilla-v1",
        "no verified save codec for engine {}",
        definitions.engine
    );
    ensure!(identity.known_game.is_some() || identity.sha256 == LOCAL_MODERN_SHA256,
        "this image has no reviewed Emerald save layout; header and external engine label are insufficient");
    Ok(GameProfile {
        schema_version: 1,
        rom_sha256: rom.base_hash().into(),
        definition_id: definitions.id.clone(),
        definition_version: definitions.version.clone(),
        engine: definitions.engine.clone(),
        save_format: "emerald-flash128-v1".into(),
        overlay_sha256: rom.manifest_hash(),
        content_sha256: definitions.digest(),
    })
}

/// Ensure a save is never reinterpreted using an incompatible definition bundle.
pub fn validate_context(profile: &GameProfile, definitions: &DefinitionBundle) -> Result<()> {
    definitions.validate()?;
    ensure!(
        profile.schema_version == 1
            && profile.engine == "emerald-vanilla-v1"
            && profile.save_format == "emerald-flash128-v1",
        "unsupported managed save profile"
    );
    ensure!(
        profile.definition_id == definitions.id
            && profile.definition_version == definitions.version
            && profile.content_sha256 == definitions.digest()
            && profile.engine == definitions.engine,
        "game definition profile mismatch; explicit migration is required"
    );
    Ok(())
}

/// Validate every occupied record and register an immutable rules snapshot before
/// copying an original battery save into application-owned storage.
pub fn import_save(
    store: &mut ManagedStore,
    profile: &GameProfile,
    definitions: &DefinitionBundle,
    bytes: &[u8],
) -> Result<i64> {
    validate_context(profile, definitions)?;
    let parsed = EmeraldSave::parse(bytes)?;
    for pokemon in parsed.pokemon()? {
        validate_dependencies(&pokemon, definitions)?;
    }
    store.register_definition(&serde_json::to_value(definitions)?)?;
    store.import_save(profile, bytes)
}

// A same-profile transfer still must not install unresolved IDs accidentally.
// Nonzero move IDs need definitions; item/ability semantics remain raw and intact
// under the exact same engine/content profile, never mapped to another game's IDs.
fn validate_dependencies(
    pokemon: &crate::gen3::Pokemon,
    definitions: &DefinitionBundle,
) -> Result<()> {
    ensure!(
        definitions.species.contains_key(&pokemon.species_id),
        "species {} absent from source definitions",
        pokemon.species_id
    );
    for mov in pokemon.moves.into_iter().filter(|id| *id != 0) {
        ensure!(
            definitions.moves.contains_key(&mov),
            "move {mov} absent from source definitions"
        );
    }
    Ok(())
}

/// Import selected party/PC records, optionally moving PC entries from a managed
/// save. An entire box is represented by many locations and commits only once.
pub fn deposit(
    store: &mut ManagedStore,
    save_id: i64,
    definitions: &DefinitionBundle,
    locations: &[Location],
    remove_from_managed: bool,
) -> Result<i64> {
    deposit_checked(
        store,
        save_id,
        definitions,
        locations,
        remove_from_managed,
        None,
    )
}

/// UI selections carry the revision they were viewed against. Keep that check
/// on the very snapshot eventually committed, so another process cannot change
/// a slot between a separate UI revision check and the actual transfer read.
pub fn deposit_at_revision(
    store: &mut ManagedStore,
    save_id: i64,
    expected_revision: i64,
    definitions: &DefinitionBundle,
    locations: &[Location],
    remove_from_managed: bool,
) -> Result<i64> {
    deposit_checked(
        store,
        save_id,
        definitions,
        locations,
        remove_from_managed,
        Some(expected_revision),
    )
}

// Both CLI and desktop paths share one parser/transaction implementation.
fn deposit_checked(
    store: &mut ManagedStore,
    save_id: i64,
    definitions: &DefinitionBundle,
    locations: &[Location],
    remove_from_managed: bool,
    expected_revision: Option<i64>,
) -> Result<i64> {
    ensure!(!locations.is_empty(), "no Pokemon selected");
    let snapshot = store.save(save_id)?;
    ensure!(
        expected_revision.is_none_or(|revision| revision == snapshot.revision),
        "save changed since selection; refresh and review the Pokemon again"
    );
    validate_context(&snapshot.profile, definitions)?;
    let original_hash = store.original_save_hash(save_id)?;
    let mut parsed = EmeraldSave::parse(&snapshot.bytes)?;
    let pokemon = parsed.pokemon()?;
    let definition_hash = store.register_definition(&serde_json::to_value(definitions)?)?;
    let mut selected = BTreeSet::new();
    let mut additions = Vec::new();
    for location in locations {
        let slot_identity = serde_json::to_string(location)?;
        ensure!(
            selected.insert(slot_identity.clone()),
            "duplicate selection"
        );
        let mon = pokemon
            .iter()
            .find(|mon| &mon.location == location)
            .context("selected slot is empty")?;
        validate_dependencies(mon, definitions)?;
        let mut individual = serde_json::to_value(mon)?;
        // Binary bytes live once in RawPokemonRecord; normalised fields are a
        // reusable view with source-local IDs, not a second binary copy.
        individual
            .as_object_mut()
            .context("individual must be an object")?
            .remove("raw");
        let import_key = crate::sha256(
            format!(
                "{}:{original_hash}:{slot_identity}:{}",
                snapshot.profile.fingerprint(),
                crate::sha256(&mon.raw)
            )
            .as_bytes(),
        );
        additions.push(NewVaultRecord { raw:mon.raw.clone(), normalized:json!({
            "record":individual, "origin":{"managed_save_id":save_id,"revision":snapshot.revision,
            "save_sha256":crate::sha256(&snapshot.bytes)}, "conversion_state":"unchanged"
        }), source_profile:snapshot.profile.clone(),
            source_definition:json!({"bundle_sha256":definition_hash,"species_id":mon.species_id}),
            import_key:Some(import_key) });
        if remove_from_managed {
            match location {
                Location::Box { box_index, slot } => {
                    parsed.replace_box(*box_index, *slot, &[0; 80])?
                }
                Location::Party { .. } => anyhow::bail!(
                    "managed party removal is not implemented; copy import is available"
                ),
            }
        }
    }
    let bytes = parsed.into_bytes();
    EmeraldSave::parse(&bytes)?.pokemon()?;
    store.commit(
        &snapshot,
        &bytes,
        if remove_from_managed {
            "managed-deposit"
        } else {
            "copy-import"
        },
        &additions,
        &[],
    )
}

/// A reviewable transfer preview. Empty conversion lists mean that no conversion
/// is being silently attempted; missing engine injectors are explicit blockers.
#[derive(Debug, Serialize)]
pub struct CompatibilityReport {
    pub mode: String,
    pub fully_compatible_fields: Vec<String>,
    pub converted_fields: Vec<String>,
    pub unsupported_fields: Vec<String>,
    pub required_dependencies: Vec<String>,
    pub blocking_problems: Vec<String>,
    pub reversible: bool,
}

/// Resolve and verify historical rules before comparing anything to the target.
pub fn preview(
    store: &ManagedStore,
    record: &VaultRecord,
    destination: &ManagedSave,
    definitions: &DefinitionBundle,
) -> Result<CompatibilityReport> {
    validate_context(&destination.profile, definitions)?;
    let hash = record
        .source_definition
        .get("bundle_sha256")
        .and_then(|s| s.as_str())
        .context("missing immutable source definition reference")?;
    let source: DefinitionBundle = serde_json::from_value(store.definition(hash)?)?;
    validate_context(&record.source_profile, &source)?;
    let mut report = CompatibilityReport {
        mode: "blocked".into(),
        fully_compatible_fields: Vec::new(),
        converted_fields: Vec::new(),
        unsupported_fields: Vec::new(),
        required_dependencies: Vec::new(),
        blocking_problems: Vec::new(),
        reversible: true,
    };
    let mon = crate::gen3::decode_record(
        &record.raw,
        Location::Box {
            box_index: 0,
            slot: 0,
        },
    )?;
    validate_dependencies(&mon, &source)?;
    let species = &source.species[&mon.species_id];
    // Source numeric IDs cannot answer whether a target has this species. Even
    // this conservative diagnostic requires identity AND equal source mechanics.
    let species_exists = definitions.species.values().any(|candidate| {
        let canonical = species
            .canonical_id
            .as_ref()
            .is_some_and(|id| candidate.canonical_id.as_ref() == Some(id));
        (canonical || (species.namespace == candidate.namespace && species.name == candidate.name))
            && species.data == candidate.data
    });
    if !species_exists {
        report.required_dependencies.push(format!(
            "source species {}/{} (source ID {}; verified identity mapping/injection required)",
            species.namespace, species.name, mon.species_id
        ));
    }
    for id in mon.moves.into_iter().filter(|id| *id != 0) {
        let source_move = &source.moves[&id];
        let exact = definitions.moves.values().any(|candidate| {
            crate::registry::match_move(source_move, candidate)
                == crate::registry::MatchConfidence::Exact
        });
        if !exact {
            report.required_dependencies.push(format!(
                "source move {}/{} (source ID {id}; verified identity mapping/injection required)",
                source_move.namespace, source_move.name
            ));
        }
    }
    // Equal numeric IDs across profiles prove nothing. These are diagnostics,
    // not mappings; the first implementation intentionally blocks all conversion.
    if record.source_profile != destination.profile {
        report.blocking_problems.push("Profiles differ; verified ID remapping, conversion and engine-specific injection are not implemented".into());
        report
            .unsupported_fields
            .push("cross-profile species/move/ability/item/form/evolution conversion".into());
    }
    if record.raw.len() != 80 {
        report.blocking_problems.push("Party-to-PC conversion requires a reviewed conversion report; only original boxed records can be withdrawn".into());
    }
    if report.blocking_problems.is_empty() {
        report.mode = "direct-exact-profile".into();
        report
            .fully_compatible_fields
            .push("all 80 original encrypted bytes and source-specific IDs".into());
    }
    Ok(report)
}

/// Withdraw into an explicitly empty PC slot, after a successful preview. The
/// original record remains in immutable event history after leaving active Vault.
pub fn withdraw(
    store: &mut ManagedStore,
    save_id: i64,
    vault_id: i64,
    definitions: &DefinitionBundle,
    box_index: usize,
    slot: usize,
) -> Result<i64> {
    withdraw_checked(store, save_id, vault_id, definitions, box_index, slot, None)
}

/// Apply a preview only to the same managed revision the user reviewed.
pub fn withdraw_at_revision(
    store: &mut ManagedStore,
    save_id: i64,
    expected_revision: i64,
    vault_id: i64,
    definitions: &DefinitionBundle,
    box_index: usize,
    slot: usize,
) -> Result<i64> {
    withdraw_checked(
        store,
        save_id,
        vault_id,
        definitions,
        box_index,
        slot,
        Some(expected_revision),
    )
}

// The optimistic store commit rechecks this captured snapshot atomically.
fn withdraw_checked(
    store: &mut ManagedStore,
    save_id: i64,
    vault_id: i64,
    definitions: &DefinitionBundle,
    box_index: usize,
    slot: usize,
    expected_revision: Option<i64>,
) -> Result<i64> {
    let snapshot = store.save(save_id)?;
    ensure!(
        expected_revision.is_none_or(|revision| revision == snapshot.revision),
        "save changed since preview; refresh and preview the withdrawal again"
    );
    let record = store.vault(vault_id)?;
    let report = preview(store, &record, &snapshot, definitions)?;
    ensure!(
        report.blocking_problems.is_empty(),
        "transfer blocked: {}",
        report.blocking_problems.join("; ")
    );
    let mut parsed = EmeraldSave::parse(&snapshot.bytes)?;
    ensure!(
        parsed.box_record(box_index, slot)?.iter().all(|b| *b == 0),
        "destination PC slot is occupied"
    );
    parsed.replace_box(box_index, slot, &record.raw)?;
    let bytes = parsed.into_bytes();
    let checked = EmeraldSave::parse(&bytes)?;
    ensure!(
        checked.box_record(box_index, slot)? == record.raw,
        "withdrawal verification mismatch"
    );
    checked.pokemon()?;
    store.commit(&snapshot, &bytes, "withdraw", &[], &[record.id])
}

/// Run a managed session while holding a cross-process database lease. Only
/// after native cleanup and save revalidation may a new working revision commit.
pub fn emulate_managed(
    store: &mut ManagedStore,
    save_id: i64,
    definitions: &DefinitionBundle,
    core: &std::path::Path,
    rom: &VirtualRom,
    frames: usize,
) -> Result<crate::emulator::EmulationResult> {
    let snapshot = store.save(save_id)?;
    ensure!(
        snapshot.profile == profile_for(rom, definitions)?,
        "ROM/overlay profile does not match managed save"
    );
    let lease = store.acquire_session(save_id, snapshot.revision)?;
    let result: Result<crate::emulator::EmulationResult> = (|| {
        let result = crate::emulator::run_headless(core, rom, Some(&snapshot.bytes), frames)?;
        let parsed = EmeraldSave::parse(&result.save)?;
        for mon in parsed.pokemon()? {
            validate_dependencies(&mon, definitions)?;
        }
        store.commit_session(&snapshot, &result.save, &lease)?;
        Ok(result)
    })();
    // On normal Rust errors the native session has already cleaned up. A process
    // crash leaves a lease behind; recovery must be deliberate, never time-based.
    let release = store.release_session(&lease);
    let result = result?;
    release?;
    Ok(result)
}

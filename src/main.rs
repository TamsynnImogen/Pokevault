//! Functional CLI over the shared backend. Run `--help` for commands and remember
//! that box/slot indices are zero-based. Every JSON output is reviewable evidence;
//! no command opens a GUI or controls input devices. Most commands preserve
//! source files; desktop-request also exposes explicitly linked save writes.

use anyhow::{ensure, Context, Result};
use clap::{Parser, Subcommand};
use serde::de::DeserializeOwned;
use serde_json::json;
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};
use universal_pokemon_vault as vault;
use vault::{
    gen3::{EmeraldSave, Location},
    overlay::{OverlayManifest, VirtualRom},
    registry::DefinitionBundle,
    store::ManagedStore,
};

/// Source-preserving inspection, Vault operations and mGBA headless sessions.
#[derive(Parser)]
#[command(version, about)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

/// All source paths are input-only. Exports require new files or a new directory.
#[derive(Subcommand)]
enum Command {
    /// Send one desktop-service JSON request headlessly to a private workspace.
    DesktopRequest {
        workspace: PathBuf,
        request: PathBuf,
    },
    /// Inspect a plain ROM's hash/header without assuming hack compatibility.
    Identify { rom: PathBuf },
    /// Extract partial rules from hash-pinned reviewed retail/modern Emerald ROMs.
    ExtractDefinitions { rom: PathBuf, output: PathBuf },
    /// Inspect party/PC records using matching, supported definitions.
    InspectSave {
        rom: PathBuf,
        definitions: PathBuf,
        save: PathBuf,
    },
    /// Preserve an original save in SQLite and create a separate working revision.
    ImportSave {
        rom: PathBuf,
        definitions: PathBuf,
        save: PathBuf,
        database: PathBuf,
    },
    /// List active Vault entries, optionally filtering species ID or shininess.
    Vault {
        database: PathBuf,
        #[arg(long)]
        species: Option<u16>,
        #[arg(long)]
        shiny: bool,
    },
    /// Copy a party/PC selection to Vault; optionally move PC entries in the managed copy.
    Deposit {
        database: PathBuf,
        save_id: i64,
        definitions: PathBuf,
        #[arg(long)]
        box_index: Option<usize>,
        #[arg(long)]
        slot: Option<usize>,
        #[arg(long)]
        party_slot: Option<usize>,
        #[arg(long)]
        move_from_managed: bool,
    },
    /// Preview a withdrawal; --apply commits to an empty managed PC slot.
    Withdraw {
        database: PathBuf,
        save_id: i64,
        vault_id: i64,
        definitions: PathBuf,
        #[arg(long)]
        box_index: usize,
        #[arg(long)]
        slot: usize,
        #[arg(long)]
        apply: bool,
    },
    /// Atomically undo the last operation and its affected Vault entries.
    Undo { database: PathBuf, save_id: i64 },
    /// Export working save, exact profile and hash receipt into a NEW directory.
    ExportSave {
        database: PathBuf,
        save_id: i64,
        output_dir: PathBuf,
    },
    /// Create a no-overwrite consistent SQLite backup, including immutable history.
    Backup { database: PathBuf, output: PathBuf },
    /// Run SQLite and stored content-hash integrity checks.
    Check { database: PathBuf },
    /// Validate an overlay manifest and report its private effective-image hash.
    OverlayValidate { rom: PathBuf, manifest: PathBuf },
    /// Run a REAL mGBA core headlessly; no keyboard/mouse/window or gameplay hooks.
    Emulate {
        rom: PathBuf,
        #[arg(long)]
        core: PathBuf,
        #[arg(long)]
        overlay: Option<PathBuf>,
        #[arg(long, default_value_t = 120)]
        frames: usize,
        #[arg(long)]
        database: Option<PathBuf>,
        #[arg(long)]
        save_id: Option<i64>,
        #[arg(long)]
        definitions: Option<PathBuf>,
    },
}

/// Use a bounded JSON loader so third-party metadata cannot exhaust memory before
/// schema validation. Native libraries are trusted executables, not metadata.
fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = read_bounded(path, 64 * 1024 * 1024)?;
    serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))
}

// Read at most limit+1 even when metadata lies or a file changes during the read.
fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= limit,
        "{} exceeds supported input size",
        path.display()
    );
    Ok(bytes)
}

/// Load immutable source bytes and validate persistent layers before a core can
/// see them. Overlay manifests are data; they cannot execute host scripts.
fn virtual_rom(path: &Path, overlay: Option<&Path>) -> Result<VirtualRom> {
    let bytes: Arc<[u8]> = read_bounded(path, 32 * 1024 * 1024)?.into();
    let manifest = match overlay {
        Some(path) => read_json(path)?,
        None => OverlayManifest::identity(&bytes),
    };
    VirtualRom::new(bytes, manifest)
}

// File creation, never replacement: even exports aimed at original filenames fail.
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("create new output {}", path.display()))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

// A single JSON receipt makes CLI actions easy to pipe, inspect and archive.
fn print_json(value: impl serde::Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn main() -> Result<()> {
    match Args::parse().command {
        Command::DesktopRequest { workspace, request } => {
            let request: serde_json::Value = read_json(&request)?;
            print_json(
                vault::desktop_service::DesktopService::open(workspace)?.dispatch(request)?,
            )?;
        }
        Command::Identify { rom } => print_json(vault::rom::identify(&read_bounded(
            &rom,
            32 * 1024 * 1024,
        )?)?)?,
        Command::ExtractDefinitions { rom, output } => {
            let bundle = vault::extraction::extract_reviewed_emerald(&read_bounded(
                &rom,
                32 * 1024 * 1024,
            )?)?;
            write_new(&output, &serde_json::to_vec_pretty(&bundle)?)?;
            print_json(
                json!({"output":output,"species":bundle.species.len(),"moves":bundle.moves.len(),
                "evolutions":bundle.evolutions.len(),"completeness":"partial","definition_sha256":bundle.digest()}),
            )?;
        }
        Command::InspectSave {
            rom,
            definitions,
            save,
        } => {
            let rom = virtual_rom(&rom, None)?;
            let bundle: DefinitionBundle = read_json(&definitions)?;
            let profile = vault::transfer::profile_for(&rom, &bundle)?;
            let parsed = EmeraldSave::parse(&read_bounded(&save, 128 * 1024)?)?;
            let pokemon = parsed
                .pokemon()?
                .iter()
                .map(|mon| {
                    let mut value = serde_json::to_value(mon)?;
                    value
                        .as_object_mut()
                        .context("record object")?
                        .remove("raw");
                    value["raw_sha256"] = json!(vault::sha256(&mon.raw));
                    Ok(value)
                })
                .collect::<Result<Vec<_>>>()?;
            print_json(json!({"profile":profile,"warnings":parsed.warnings,"pokemon":pokemon}))?;
        }
        Command::ImportSave {
            rom,
            definitions,
            save,
            database,
        } => {
            let rom = virtual_rom(&rom, None)?;
            let bundle: DefinitionBundle = read_json(&definitions)?;
            let profile = vault::transfer::profile_for(&rom, &bundle)?;
            let bytes = read_bounded(&save, 128 * 1024)?;
            let warnings = EmeraldSave::parse(&bytes)?.warnings;
            let mut store = ManagedStore::open(database)?;
            let id = vault::transfer::import_save(&mut store, &profile, &bundle, &bytes)?;
            print_json(
                json!({"managed_save_id":id,"source_sha256":vault::sha256(&bytes),"revision":0,"warnings":warnings}),
            )?;
        }
        Command::Vault {
            database,
            species,
            shiny,
        } => {
            let store = ManagedStore::open(database)?;
            let entries = store.list_vault()?.into_iter().filter(|entry| {
                species.is_none_or(|id| entry.normalized["record"]["species_id"].as_u64() == Some(id as u64))
                && (!shiny || entry.normalized["record"]["shiny"] == true)
            }).map(|entry| json!({"id":entry.id,"uuid":entry.uuid,"raw_sha256":vault::sha256(&entry.raw),
                "source_profile":entry.source_profile,"source_definition":entry.source_definition,"individual":entry.normalized})).collect::<Vec<_>>();
            print_json(entries)?;
        }
        Command::Deposit {
            database,
            save_id,
            definitions,
            box_index,
            slot,
            party_slot,
            move_from_managed,
        } => {
            ensure!(
                box_index.is_some() != party_slot.is_some(),
                "choose --box-index or --party-slot"
            );
            ensure!(
                slot.is_none() || box_index.is_some(),
                "--slot needs --box-index"
            );
            let bundle = read_json(&definitions)?;
            let mut store = ManagedStore::open(database)?;
            let locations = match (box_index, slot, party_slot) {
                (Some(box_index), Some(slot), None) => vec![Location::Box { box_index, slot }],
                (Some(box_index), None, None) => {
                    ensure!(box_index < 14, "box index must be 0..13");
                    EmeraldSave::parse(&store.save(save_id)?.bytes)?
                        .pokemon()?
                        .into_iter()
                        .filter_map(|mon| match mon.location {
                            Location::Box { box_index: b, .. } if b == box_index => {
                                Some(mon.location)
                            }
                            _ => None,
                        })
                        .collect()
                }
                (None, None, Some(slot)) => vec![Location::Party { slot }],
                _ => anyhow::bail!("invalid deposit selection"),
            };
            let revision = vault::transfer::deposit(
                &mut store,
                save_id,
                &bundle,
                &locations,
                move_from_managed,
            )?;
            print_json(
                json!({"managed_save_id":save_id,"revision":revision,"imported":locations.len(),"removed_from_managed":move_from_managed}),
            )?;
        }
        Command::Withdraw {
            database,
            save_id,
            vault_id,
            definitions,
            box_index,
            slot,
            apply,
        } => {
            let mut store = ManagedStore::open(database)?;
            let bundle = read_json(&definitions)?;
            let snapshot = store.save(save_id)?;
            let report =
                vault::transfer::preview(&store, &store.vault(vault_id)?, &snapshot, &bundle)?;
            let empty = EmeraldSave::parse(&snapshot.bytes)?
                .box_record(box_index, slot)?
                .iter()
                .all(|b| *b == 0);
            if apply {
                let revision = vault::transfer::withdraw(
                    &mut store, save_id, vault_id, &bundle, box_index, slot,
                )?;
                print_json(json!({"report":report,"applied":true,"revision":revision}))?;
            } else {
                print_json(
                    json!({"report":report,"destination_slot_empty":empty,"expected_save_modification":
                    format!("PC box {box_index}, slot {slot}; original trainer and other slots preserved"),"applied":false}),
                )?;
            }
        }
        Command::Undo { database, save_id } => {
            print_json(json!({"revision":ManagedStore::open(database)?.undo(save_id)?}))?;
        }
        Command::ExportSave {
            database,
            save_id,
            output_dir,
        } => {
            let save = ManagedStore::open(database)?.save(save_id)?;
            // A new directory keeps save + required interpretation profile together.
            // Existing directories are rejected, so unrelated files cannot be replaced.
            std::fs::create_dir(&output_dir)?;
            write_new(
                &output_dir.join("profile.json"),
                &serde_json::to_vec_pretty(&save.profile)?,
            )?;
            write_new(&output_dir.join("working.sav"), &save.bytes)?;
            let receipt = json!({"save_id":save_id,"revision":save.revision,"save_sha256":vault::sha256(&save.bytes),
                "profile_sha256":save.profile.fingerprint()});
            write_new(
                &output_dir.join("receipt.json"),
                &serde_json::to_vec_pretty(&receipt)?,
            )?;
            print_json(receipt)?;
        }
        Command::Backup { database, output } => {
            ManagedStore::open(database)?.export_backup(&output)?;
            print_json(json!({"backup":output,"sha256":vault::sha256(&std::fs::read(&output)?)}))?;
        }
        Command::Check { database } => {
            ManagedStore::open(database)?.integrity_check()?;
            print_json(json!({"integrity":"ok"}))?;
        }
        Command::OverlayValidate { rom, manifest } => {
            let rom = virtual_rom(&rom, Some(&manifest))?;
            print_json(
                json!({"base_sha256":rom.base_hash(),"manifest_sha256":rom.manifest_hash(),"effective_sha256":vault::sha256(&rom.materialize())}),
            )?;
        }
        Command::Emulate {
            rom,
            core,
            overlay,
            frames,
            database,
            save_id,
            definitions,
        } => {
            let image = virtual_rom(&rom, overlay.as_deref())?;
            let result = match (database, save_id, definitions) {
                (None, None, None) => vault::emulator::run_headless(&core, &image, None, frames)?,
                (Some(database), Some(save_id), Some(definitions)) => {
                    let mut store = ManagedStore::open(database)?;
                    vault::transfer::emulate_managed(
                        &mut store,
                        save_id,
                        &read_json(&definitions)?,
                        &core,
                        &image,
                        frames,
                    )?
                }
                _ => anyhow::bail!(
                    "managed emulation needs --database, --save-id and --definitions together"
                ),
            };
            print_json(result)?;
        }
    }
    Ok(())
}

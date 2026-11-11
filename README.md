# Universal Pokémon Vault

A functional Rust backend, CLI and initial Tauri desktop application for
source-preserving Pokémon storage, managed Emerald saves and virtual-ROM mGBA
playback. Desktop library, PC/Vault browsing, previews, transfers, undo,
exports, and the initial playable game view use the real Rust services. Manual
native gameplay/audio acceptance and the mobile frontend remain pending. See
[PLAYABLE_EMULATOR.md](docs/PLAYABLE_EMULATOR.md) for setup and save behavior,
and [DESKTOP_MILESTONE.md](docs/DESKTOP_MILESTONE.md) for the native app and
verified boundaries.

A matching clean Emerald test ROM has now been built separately; see
[CLEAN_EMERALD.md](docs/CLEAN_EMERALD.md) for its verified hashes and save-creation steps.
The first real Emerald PC transfer has passed automated checks and user-confirmed
in-game placement; see [PC_TRANSFER_VALIDATION.md](docs/PC_TRANSFER_VALIDATION.md)
for the exact scenario and preserved-save evidence.

ROM images remain read-only. Saves can be copied into managed storage with
`import_save`, or explicitly linked to a chosen regular save file in the desktop
app. Linked transfers publish to that same file with verified backups and
recoverable history; see [LINKED_SAVES.md](docs/LINKED_SAVES.md). SQLite stores
immutable imports, working revisions, UUID-based Vault entries, original record
bytes, historical definition snapshots and transactional transfer history. No Pokémon assets,
ROMs, BIOS images or gameplay-derived saves are bundled.

## Build and test

```bash
git clone https://github.com/TamsynnImogen/Pokevault.git
cd Pokevault
```

Canonical repository: [TamsynnImogen/Pokevault](https://github.com/TamsynnImogen/Pokevault).

```bash
cargo build --locked
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo run -- --help
```

The headless backend remains the workspace's default build target. For desktop:

```bash
npm --prefix ui ci --legacy-peer-deps
npm --prefix ui run build
cargo build --locked --release -p pokemon-vault-desktop
# Run this command manually; it opens a native window.
target/release/pokemon-vault-desktop
```

The desktop embeds the built frontend and needs no development server. It uses
its own application data directory, or an explicit `--workspace PATH`. No original
game or save is automatically imported. Native system prerequisites follow
[Tauri's platform guide](https://v2.tauri.app/start/prerequisites/).

SQLite builds from bundled source, so install a native C compiler in addition to
Rust. mGBA is optional for ordinary backend tests. To run actual native regressions,
install a trusted mGBA Libretro core and GNU ARM binutils:

```bash
UPV_CORE=/path/to/mgba_libretro.so cargo test --test native -- --ignored --nocapture
```

This assembles a small original homebrew cartridge, compares baseline sessions,
patches a ROM color read by DMA and patches an executed ARM instruction. Optional
`UPV_ROM=/path/to/reviewed/pokeemerald_modern.gba` adds a native managed-save test.
Its save is clearly labelled synthetic; passing it is not gameplay acceptance.

For user-owned gameplay saves, set `UPV_ROM` and `UPV_SAVE` (prefer a verified
backup), optionally `UPV_CORE`, and run `cargo test --test gameplay -- --ignored --nocapture`.
This test uses private temporary databases and reports whether a real PC
roundtrip was possible. See [GAMEPLAY_VALIDATION.md](docs/GAMEPLAY_VALIDATION.md)
for the first actual save's results and remaining acceptance gates.

## Use the implemented services

```bash
# Header/fingerprint inspection does not assume a hack's save layout.
cargo run -- identify /path/to/game.gba

# Real emulation, headless, from private ROM bytes. No GUI/input automation.
cargo run -- emulate /path/to/game.gba --core /path/to/mgba_libretro.so --frames 120

# Rules extraction supports two exact, independently hash-pinned Emerald images:
# matching retail and one reviewed local modern build. Each needs its own bundle.
cargo run -- extract-definitions /path/to/reviewed/emerald-clean.gba retail-definitions.json
cargo run -- inspect-save /path/to/reviewed/emerald-clean.gba retail-definitions.json /path/to/emerald-clean.srm
cargo run -- import-save /path/to/reviewed/emerald-clean.gba retail-definitions.json /path/to/emerald-clean.srm vault.sqlite

# The earlier modern image remains supported under its separate exact profile.
cargo run -- extract-definitions /path/to/reviewed/pokeemerald_modern.gba modern-definitions.json

# Save IDs come from import-save. All party/box/slot indices are zero-based.
cargo run -- deposit vault.sqlite 1 retail-definitions.json --box-index 0 --slot 0
cargo run -- deposit vault.sqlite 1 retail-definitions.json --party-slot 0
cargo run -- vault vault.sqlite

# Omit --slot to copy every occupied slot in a box atomically. An explicit
# --move-from-managed removes PC entries from the managed working save. CLI saves
# are unlinked; desktop-linked saves also publish the committed revision.
cargo run -- deposit vault.sqlite 1 retail-definitions.json --box-index 1 --move-from-managed

# Preview comes first. Use --apply only after reviewing the report.
cargo run -- withdraw vault.sqlite 1 1 retail-definitions.json --box-index 2 --slot 0
cargo run -- withdraw vault.sqlite 1 1 retail-definitions.json --box-index 2 --slot 0 --apply
cargo run -- undo vault.sqlite 1

# Save export creates a NEW directory holding working.sav + profile + receipt.
cargo run -- export-save vault.sqlite 1 export-001
cargo run -- backup vault.sqlite backup-001.sqlite
cargo run -- check vault.sqlite

# Managed emulation validates ROM/overlay/definition profile and leases the save.
cargo run -- emulate /path/to/reviewed/emerald-clean.gba --core /path/to/mgba_libretro.so \
  --database vault.sqlite --save-id 1 --definitions retail-definitions.json --frames 120
```

The `.srm` file above is accepted as the same raw 128 KiB Emerald battery-save
image; it is not an emulator savestate or a generic wrapper format.

Only exact-profile boxed records can currently be withdrawn. Cross-profile
conversion, ROM expansion, party removal and unknown hack writes are blocked.
ROM files are never used as emulator save outputs. Explicit exports remain
available for unlinked workflows and create new directories; linked desktop
transfers do not require export. Backups can be restored by opening a separate
copy as a database.

## Persistent virtual overlays

An overlay JSON manifest contains `schema_version`, `base_sha256`, `virtual_size`
and ordered `layers`. Each layer has `id`, `version`, `dependencies` and `patches`.
Patches have byte `offset`, `expected` bytes and equal-length `replacement` bytes.
Dependencies require an exact earlier layer version. Every overlap is rejected.
Appended address space is FF-filled and limited to ordinary GBA's 32 MiB window.

```json
{
  "schema_version": 1,
  "base_sha256": "replace-with-the-exact-64-character-sha256",
  "virtual_size": 16777216,
  "layers": [{
    "id": "example-reviewed-range",
    "version": 1,
    "dependencies": [],
    "patches": [{"offset": 4096, "expected": [1, 2], "replacement": [3, 4]}]
  }]
}
```

The example is schema guidance, **not** an executable Pokémon patch. Supply a
reviewed range for your own exact image; mismatching expected bytes fail closed.

```bash
cargo run -- overlay-validate /path/to/game.gba overlay.json
cargo run -- emulate /path/to/game.gba --overlay overlay.json --core /path/to/mgba_libretro.so
```

The sparse source stays immutable. A private effective image is materialized at
launch so CPU instruction fetch and DMA share the same virtual bytes. Changes
require a new session; hot-patching and emulator gameplay extensions are disabled.
Managed saves cannot be attached to an unreviewed effective image.

## Contributor guide

Read [ARCHITECTURE.md](docs/ARCHITECTURE.md), [IMPLEMENTATION_PLAN.md](docs/IMPLEMENTATION_PLAN.md)
and [FORMATS.md](docs/FORMATS.md) before adding an engine. [VERIFICATION.md](docs/VERIFICATION.md)
records tested artifacts and the remaining gates. Module docs and inline
comments explain the untrusted-data checks and transaction/native ownership rules.

- `rom.rs`: exact recognition vs header hints.
- `gen3.rs`: strict vanilla Emerald codec and encrypted individual records.
- `registry.rs` / `extraction.rs`: source rules, cautious matching, bounded extraction.
- `overlay.rs`: versioned copy-on-write ranges and native-image materialization.
- `store.rs`: SQLite schema, immutable backups, leases and coherent undo.
- `transfer.rs`: previews, source references and safe managed operations.
- `emulator.rs`: isolated native ABI boundary with no guessed mGBA structures.
- `main.rs`: real CLI services; frontend-independent.
- `desktop_service.rs`: typed IPC requests and frontend-independent app workflows.
- `desktop/`: Tauri shell, local-only capabilities and embedded assets.
- `ui/`: commented TypeScript interface with headless rendering/workflow checks.

The project was started in an isolated directory because no existing Universal
Pokémon Vault repository was found. Adjacent Pokémon projects were inspected as
references and were not modified. Native mGBA licensing/distribution must follow
its MPL-2.0 and its exact build's third-party notices; the separate Rust code is MIT.

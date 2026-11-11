# Desktop milestone and IPC contract

The desktop app wraps the verified Emerald backend in a functional Tauri v2
application. No mock collection, copyrighted artwork, implicit save linking or
unknown-hack interpretation is permitted. Initial playable mGBA hosting is now
implemented; see [PLAYABLE_EMULATOR.md](PLAYABLE_EMULATOR.md). Custom injection
remains a later milestone. Pokémon Pieces is
a planned future Fakémon candidate, not an installed engine definition.

Work split: **LEAD** owns service orchestration, profiles, optimistic concurrency,
integration and headless tests. **MEDIUM** storage owns schema-2 ROM-library/read
queries. **MEDIUM** frontend owns `ui/`. **CHEAP** desktop glue owns `desktop/`.
Independent review follows integration. All new code needs module and explanatory
comments for contributors. No GUI/input automation is allowed during verification.

## Transport and ownership

The Tauri command `vault_request` accepts `{ request: <JSON below> }` and returns
JSON or a human-readable error string. The root Rust module is `desktop_service`:
`DesktopService::open(path) -> Result<Self>` and `dispatch(Value) -> Result<Value>`.
The native shell holds `Arc<Mutex<DesktopService>>`, executes requests using
`spawn_blocking`, and uses an application-owned data directory. Optional native
argument `--workspace PATH` selects a separate workspace; no original database
or test collection is automatically imported.

Only local packaged frontend content is loaded. Tauri's dialog plugin may choose
input files. No filesystem/shell/opener plugins or remote content are enabled.
All text from ROMs/saves must be inserted as text, never interpreted HTML.

Every request has an `action` field, written below in snake_case. Other fields
use snake_case. Mutations return `{ save_id, revision, message }` unless noted.
Save files are only linked after the user explicitly chooses them. Writes to a
chosen save are backed up and recoverable.

Playable startup uses the separate native command `emulator_start` with
`{save_id,core_path,on_frame:Channel<ArrayBuffer>}`. Rust uses
`Channel<Response>` with `Response::new(Vec<u8>)` for raw binary packets. Channel
handles are not accepted as ordinary JSON game metadata or CLI request data.

## Commands

- `overview`: returns `{ workspace_path, games: GameView[], saves: SaveSummary[],
  vault_count }`.
- `add_rom`, `path: string`: identify a plain ROM, automatically extract only
  reviewed hashes, persist its library row; returns `GameView`. Unknown cartridges
  remain inspectable with `import_supported:false` and diagnostics.
- `import_save`, `rom_hash:string, path:string`: copy a matching supported save
  into managed SQLite storage, leaving the source unchanged; returns
  `{save_id,revision:0,message,warnings}`. This does not create a link.
- `choose_save`, `rom_hash:string, path:string`: explicitly link the chosen
  canonical regular 128 KiB Emerald save file and load its validated current
  contents. Choosing the same path again refreshes the same save ID with external
  progress. Returns `{save_id,revision,linked_path,message}`. Existing saves are
  never linked automatically during upgrade.
- `remove_game`, `rom_hash:string`: remove only the library entry. The ROM,
  saves, definitions, Vault records and history remain retained.
- `recover_save`, `save_id, expected_revision, expected_file_sha256`: after
  reviewing a pending conflict, explicitly back up conflicting file bytes and
  restore the committed database save to the chosen path.
- `save_view`, `save_id:number`: returns `{id,revision,profile,warnings,
  pokemon:PokemonView[],box_count:14,slots_per_box:30,linked_path,sync_status,
  external_sha256}`. Only occupied slots appear. `external_sha256` is the current
  file hash used to bind an explicit conflict-recovery request to the viewed file.
- `vault_list`, `query:string`: returns `PokemonView[]`; query may be empty and
  matches nickname, species, source definition, UUID or numeric species ID.
- `deposit`, `save_id, expected_revision, locations:Location[],
  move_from_managed:boolean`: the desktop always sends `true`; legacy callers can
  explicitly pass `false` for a copy. Moving party records is blocked.
  On a linked save, a requested PC move also updates the same chosen file.
  Multiple occupied selections/whole box commit atomically.
- `preview_withdraw`, `save_id,vault_id,box_index,slot`: returns
  `{report:CompatibilityReport,revision,destination_slot_empty}`. It is read-only.
- `withdraw`, `save_id,expected_revision,vault_id,box_index,slot`: requires the
  revision from the preview and an empty destination. On a linked save, the
  selected file is updated too. All indices are zero-based.
- `undo`, `save_id,expected_revision`: coherent latest-operation rollback; for a
  linked save, the file is restored to the matching prior revision as well.
- `export_save`, `save_id`: creates a new unique directory under the application's
  `exports/`, containing `working.sav`, `profile.json`, `receipt.json`; returns
  `{path,save_sha256,message}`. No arbitrary output path is accepted from the UI.
  Explicit export is optional for linked transfers.
- `backup`: creates a new SQLite backup under the application's `backups/`;
  returns `{path,message}`.
- `integrity_check`: returns `{message}` after SQLite and content checks.
- `history`, `save_id:number|null`: returns `OperationView[]` in newest-first
  order. There are no invented timestamps for old schema-1 operations.
- `run_headless`, `rom_hash:string,save_id:number|null,core_path:string,
  frames:number`: real mGBA diagnostics; a selected managed save is leased and
  validated. Returns existing `EmulationResult` receipt (save bytes omitted).
  Label this **headless diagnostics**, not playable emulator controls.
- `emulator_defaults`: returns `{core_path:string|null}` for an already-installed
  mGBA library. Discovery reads paths only; actual code executes on explicit Start.
- `emulator_status`: returns `EmulatorStatus` below. It does not release a lease
  or automatically commit a stopped/failed session.
- `emulator_control`, `session_id:string,buttons:u16,ack:u64|null,
  paused:boolean|null`: validates the active session ID, permitted emulated joypad
  bits and last delivered sequence. No operating-system input APIs are used.
- `emulator_stop`, `session_id`: stops, joins and unloads the core, preserves raw
  SRAM, validates it, publishes through the linked-save journal, commits SQL and
  releases the session lease. Returns inactive `EmulatorStatus` on success.
- `emulator_discard`, `session_id`: explicit escape for a stopped `save_pending`
  session. Keeps its recovery copy and releases the lease; does not forcibly
  replace external conflicting bytes or modify Vault entries.

`EmulatorStatus`: `{active,session_id,save_id,state,frames?,core_name?,fps?,
sample_rate?,error?}`. IDs are null when inactive. States include `starting`,
`running`, `paused`, `finished`, `save_pending`, `stopped`. An active paused or
finished session still owns its save until Stop & save or explicit recovery-copy
release. Initial and stopped save images live below `emulator-sessions/<UUID>/`;
lease tokens are local recovery metadata and are never exposed in this view.

PVF1 packets: magic `PVF1` at byte 0, little-endian u32 width at 4, height at 8,
sample rate at 12, audio stereo-frame count at 16, u64 sequence at 20; then RGBA8
pixels from byte 28 followed by interleaved signed stereo PCM16 little-endian.
The core reports its own FPS/sample rate. At most three packets remain
unacknowledged; video, audio and decode allocations are bounded.

## View types

`Location` uses the existing Rust representation:
`{"Party":{"slot":0}}` or `{"Box":{"box_index":0,"slot":0}}`.

`GameView`: `{hash,source_path,title,platform,game_code,classification,
import_supported,diagnostics:string[]}`. `game_code` may be null.

`SaveSummary`: `{id,revision,game_hash,definition_id,party_count,pc_count,
warnings:string[],linked_path:string|null,sync_status:string}`. Status is
`linked`, `unlinked`, `changed`, `pending` or `emulating`. A runtime-owned save
remains `emulating` while paused. Counts may be null when an older workspace needs its ROM
definition added; unknown counts must not be displayed as zero. Source filenames
are not fabricated.

`PokemonView`:

```text
{
  location: Location|null, vault_id:number|null, uuid:string|null,
  species_id:number, species_name:string, nickname:string, level:number|null,
  shiny:boolean, type_names:string[], source_game:string, raw_sha256:string,
  moves:[{id:number,name:string,pp:number,pp_ups:number,definition:object}],
  individual:object, species:object,
  evolutions:[{from_name:string,to_name:string,method_namespace:string,requirements:any}],
  limitations:string[]
}
```

`individual` is decoded data without its raw byte vector. `species` is the
source-specific species snapshot; move IDs stay source-local. PC level may be
null until the source growth table is decoded. Never substitute a guessed level.

`CompatibilityReport` reuses the backend: `{mode,fully_compatible_fields,
converted_fields,unsupported_fields,required_dependencies,blocking_problems,
reversible}`. The frontend must show blockers and vacancy before enabling Apply.

`OperationView`: `{id,save_id,before_revision,after_revision,kind,undone}`.

## Headless acceptance

Use real reviewed ROM/save inputs in a separate service workspace, exercise the
same requests used by IPC, verify source hashes, stale-revision rejection, atomic
box actions, preview/withdraw/undo/export and database reopening. UI unit tests
may stub transport only; the running product must never display fixture data.
Build/type-check the frontend and compile the native shell. Manual desktop
acceptance remains separate because input-device automation is prohibited.

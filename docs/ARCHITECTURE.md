# Universal Pokémon Vault: preservation, transfers and virtual ROMs

## Scope and trust boundaries

The three project prompts describe successive milestones of one application.
Read-only source imports remain the default; direct transfers write only managed
copies after a verified codec exists. Unknown derivatives can be identified and
reported without receiving vanilla semantics. No claim of universal hack support
follows from a header, filename, matching move name or a successful emulator boot.

ROMs and legacy imported saves are input bytes only. The desktop's explicit
Choose save action enables backed-up, journaled publication to that chosen battery
file; see [LINKED_SAVES.md](LINKED_SAVES.md). SQLite owns working saves, immutable
import backups, revision history, Pokémon records and transfer events. A save's
interpretation context includes original ROM SHA-256, definition ID/version,
definition digest, ordered overlay digest and save format. A changed context
requires an explicit migration; automatic rebinding is forbidden.

## Modules and interfaces

| Component | Responsibility | First implementation boundary |
| --- | --- | --- |
| Emulator Core | Own native core, frames, audio/video and detached save export | mGBA through Libretro ABI v1; headless diagnostics and initial playable desktop view; no memory hooks |
| Virtual ROM Manager | Immutable baseline + sparse, ordered, versioned layers | Equal-length guarded ranges, appended FF space up to ordinary GBA 32 MiB |
| Pokémon Vault | Permanent UUID, raw record and normalised source-aware data | SQLite atomic save/Vault commit and coherent undo |
| Game Definition Registry | Versioned species/move/rule/resource snapshots | Explicit JSON bundles with provenance; no scripts |
| Compatibility Engine | Preview, dependencies, mappings, conversions, blockers | Exact-profile direct PC transfers; other conversions blocked |
| Runtime Extension System | Verified engine-specific behaviour beyond tables | Capability is disabled; no generic arbitrary-memory interpreter |
| Virtual Save Manager | Private sessions, import/export, backup, migration | Immutable revisions and session lease; no source-path write API |

The Rust library is UI-independent. The CLI exercises real services. The Tauri
desktop frontend calls these services for library/storage/transfers and hosts an
initial playable game view over a bounded binary frame/audio channel. Android
can use a native/JNI frontend. Neither framework is required by parsers, SQLite,
registries or compatibility logic. No mock UI substitutes for these units.

## Identification and extraction

SHA-256 establishes exact image identity. GB/GBC and GBA headers provide candidate
platforms/titles/codes/revisions and checksum diagnostics. Exact known fingerprints
select verified adapters; unknown codes or hacks remain unknown derivatives.
Externally supplied definitions are data, never executable plugin code, and are
explicitly trusted interpretation inputs. Their claimed engine is not independent
proof. Production support requires checked build manifests and regression fixtures.

The initial Emerald codec follows pret's `save.c`, `save.h`, `pokemon.h` and
`pokemon_storage_system.h`. Sector checksums, slot completeness/counters, XOR
encryption and all 24 substructure permutations are validated. Individual data is
separate from species rules. Unknown fields remain in raw records. Unsupported
save wrappers, expansion record formats and compressed containers are rejected.

A source build extractor must bind ROM + ELF/map + source commit/config hashes,
validate every symbol/table extent and expose completeness per resource. It may
not call the local modern Emerald build a retail dump or assume expansion structs
match it. Graphics, cries, learnsets, breeding, evolutions and Pokédex text each
need their own validated decoder and provenance before custom species installation.

## Virtual ROM execution

Sparse copy-on-write data is the persistent configuration; source bytes never
change. For the initial emulator adapter, materialize a private contiguous image
at launch and pass it as in-memory content. The core's CPU fetch, ROM mapping and
DMA then see the same patched image. Intercepting VFile.read alone is insufficient:
mGBA maps ROM buffers and has active CPU regions and DMA paths.

Changing a manifest requires unloading and rebuilding the session. No hot-patch
claim is made. Table relocation needs a verified engine injector that updates
all pointers, literal pools, bounds checks and save implications. Replacement
bytes alone cannot add move effects or evolution code. Ordinary GBA cartridge
mapping stops at 32 MiB; mapper-based expansion needs its own supported adapter.
GB/GBC bank controllers, open-bus behaviour, EEPROM/GPIO windows and flash protocols
remain emulator responsibilities, never blanket offset arithmetic.

Layers require schema versions, unique IDs, exact earlier dependencies, checked
arithmetic, expected bytes, and no overlap in the first version. Overrides may be
introduced later only with explicit conflict declarations and tests. A profile
digest includes layer ordering. Hardware accuracy is unchanged when layers and
extensions are absent; verify this with equal frame/save traces, not assumption.

## Transfers and synchronisation

1. Freeze and validate the source instance, raw bytes and source rules snapshot.
2. Compare destination definitions by namespace, canonical identity and mechanics.
3. Produce a report of exact mappings, candidate matches, losses and blockers.
4. Traverse species/move/ability/item/form/evolution/resource dependency graphs.
5. Select an installed verified injector or block; never allocate arbitrary IDs.
6. Prepare a new virtual profile; validate tables, pointers, engine limits and saves.
7. Migrate *every* affected party/PC record into a new managed save if required.
8. Convert the individual, preserving all source data and unsupported attributes.
9. Atomically commit the managed save, Vault status, mappings and transfer history.
10. Reparse/check checksums and verify the result before exposing success.

The implemented subset may stop at step 3 for unsupported engines. Read-only
import copies records; an explicit managed deposit can remove PC slots only from
the working save. Same-profile withdrawal preserves raw data rather than guessing
cross-profile conversions. Whole-box operations are one transaction.

First safe boundary: emulator session fully stopped and its detached SRAM
validated and published. A database lease prevents
transfers/undo during a managed session, including another process. A crash leaves
a stale lease that fails closed pending manual recovery. Pausing gameplay or
seeing the title screen is not proof that RAM, flash and disk are synchronized.
Never restore a savestate over a transferred battery save. Live RAM writes and
savestate synchronization are not implemented.

Undo restores both the prior save and affected Vault entries atomically. Refuse
undo if a later transfer touched an affected entry. Save-only rollback after a
deposit could duplicate or erase individuals and is deliberately unavailable.

## Emulator integration and distribution

mGBA provides GB/GBC/GBA emulation. This project's reviewed save codec and
managed-save publication currently support its specific Emerald format; do not
infer universal hack, GB or GBC save support. Its native `mCore` contains
build-dependent fields and function pointers; do not transmute a Rust guessed
layout. The adapter uses the documented stable Libretro C ABI. It verifies
API version and mGBA core name, supplies in-memory ROMs and callbacks, copies save
RAM before unload and never gives the core original file paths or a save directory.
Only one session may exist per process because Libretro cores use global state.
The desktop can prefill a detected installed core path, preferring
`/usr/lib64/libretro/mgba_libretro.so`; other standard locations are searched.
The user must choose a trusted native library: loading it executes native code.
The core, BIOS, ROMs and Pokémon assets are not bundled. The local core remains
an unbundled user-installed dependency.

mGBA's core is MPL-2.0. Distribution planning must preserve notices and provide
the corresponding covered source (including modifications) and a clear source
location with binaries. Separate Rust files may use MIT as a Larger Work, subject
to the MPL's terms. Static vs dynamic linking does not remove these obligations.
Audit the exact native build's third-party dependencies, build flags and assets;
exclude Qt/SDL frontends and optional GPL components unless intentionally licensed.
Use the built-in BIOS or an explicitly user-provided BIOS; ship no Nintendo BIOS,
ROMs, saves or proprietary sprites. Libretro's header carries its own MIT notice.
No distribution package is declared licence-cleared by this prototype.

Primary references checked 2026-11-10:

- [mGBA 0.10.5 mCore API](https://github.com/mgba-emu/mgba/blob/0.10.5/include/mgba/core/core.h)
- [mGBA VFile mapping API](https://github.com/mgba-emu/mgba/blob/0.10.5/include/mgba-util/vfs.h)
- [mGBA ROM loading](https://github.com/mgba-emu/mgba/blob/0.10.5/src/gba/gba.c)
- [mGBA memory execution paths](https://github.com/mgba-emu/mgba/blob/0.10.5/src/gba/memory.c)
- [mGBA Libretro adapter](https://github.com/mgba-emu/mgba/blob/0.10.5/src/platform/libretro/libretro.c)
- [MPL-2.0](https://github.com/mgba-emu/mgba/blob/0.10.5/LICENSE)
- [Libretro ABI and notice](https://github.com/libretro/libretro-common/blob/master/include/libretro.h)
- [pret Emerald save implementation](https://github.com/pret/pokeemerald/blob/master/src/save.c)
- [PKHeX Emerald adapter, independent format reference](https://github.com/kwsch/PKHeX/blob/master/PKHeX.Core/Saves/SAV3E.cs)

PKHeX is researched as a format reference; its implementation is not copied or
linked. A future exchange adapter must review its licence and distinguish encrypted
80-byte boxed records from decrypted `.pk3` exchange files.

## Roadmap and release gates

M1: backend, SQLite migrations, registry, identification, CLI and integrity tests.
M2: strict Emerald party/PC reads, binary-preserving imports, exact-profile managed
PC transfers, synthetic fixtures and separately recorded real-save acceptance.
M3/M4: verified ROM extraction and build manifests for Emerald/expansion; explicit
source content installation, dependency closure and validated ID remapping.
M5: functional Tauri frontend and user-supplied artwork. M6: reviewed cross-game
conversions with preview and retained source metadata. M7: additional codecs.
M8: source rebuilds, BPS export and verified binary injection, mobile packaging,
engine-specific gameplay hooks only after runtime regression suites exist.

BPS/source patches are optional exports from independently validated builds;
virtual profiles remain the default. A patch adds definitions; a save transaction
adds the individual. They are separate artifacts, receipts and failure boundaries.

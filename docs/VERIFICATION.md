# Verified milestone — 2026-11-10

## Later playable-emulator follow-up

The standalone Linux desktop now builds an initial **Play game** page backed by
real mGBA video, stereo PCM and an emulated joypad. **61 default Rust tests** pass
(eight external/native cases ignored by default), along with **14 frontend tests**,
strict workspace Clippy and the Windows backend compile check.

Five opt-in native regressions passed using the trusted installed core: the
existing DMA/instruction-overlay test; 120 consecutive realtime packets and
immediate pre-frame stop; 60-frame original homebrew input red→green→red; and
two real-save service lifecycle/conflict tests. These cover running/paused lease
exclusion, startup-error cleanup, close-handler service shutdown, repeated starts,
post-stop transfers, save-write retry and recovery-copy release. Source ROM/save
inputs remained unchanged; all mutation tests used temporary copies.

Tauri's binary channel command and asynchronous window-close hook compile and
passed independent source review. No native window or audio device was opened by
the agent. Native canvas delivery, audible playback, real user controls and window
close acceptance remain manual checks. Process crashes deliberately leave a
stranded lease for manual recovery; RTC persistence/gamepad/mobile support remain
outside this implementation. Logs are in `local/emulator-milestone/` and usage is
documented in [PLAYABLE_EMULATOR.md](PLAYABLE_EMULATOR.md).

## Later linked-save follow-up

The current suite passes **58 Rust tests** (four external/native tests ignored by
default), plus **8 frontend tests**, TypeScript/Vite build, formatting and strict
workspace Clippy. The Windows backend cross-check also passes; Windows desktop,
file-replacement durability and Android runtime remain unvalidated.

The opt-in `tests/linked_desktop.rs` passed against a temporary copy of the real
reviewed Emerald save: same-path/ID reselection, deposit file removal, withdrawal
file restoration with an empty Vault, UUID-preserving repeat transfers, rejection
of unexpected external changes, explicit refresh, undo, game removal, retained
definition access and reopening. Both original inputs remained unchanged.
Ten independent file-journal tests cover backups, SQL rollback on file failure,
both crash outcomes, preserved external conflicts, guarded explicit resolution,
workspace locking and Unix symlink/hard-link refusal.

A private copy of the user's real schema-2 database migrated to schema 3 with
all original imports, revision/event/receipt counts and Vault UUIDs preserved.
Its pre-migration backup retains schema 2. Evidence is under
`local/linked-saves-milestone/`. No original user save was automatically linked
or rewritten. Native UI and in-game acceptance of this new direct-write workflow
still require manual confirmation; earlier export tests below remain historical.

See [LINKED_SAVES.md](LINKED_SAVES.md) for the current behavior. Close the external
emulator before file edits and reload through the game's Continue screen.

## Historical foundation milestone

The first backend milestone is implemented; the full application roadmap remains
open. No pre-existing Vault repository was located. A new isolated Git repository
was initialized here, and no adjacent Pokémon checkout or original ROM/save was
modified.

## Automated and native evidence

- `cargo fmt --check`: passed.
- `cargo test --offline --locked`: **37 passed**, one optional native test ignored.
- `cargo clippy --offline --locked --all-targets -- -D warnings`: passed.
- `cargo check --offline --locked --target x86_64-pc-windows-gnu`: passed.
- `cargo build --offline --locked --release`: passed, Linux CLI v0.1.0.
- Independent read-only review: no blocking findings after glyph overflow and
  synthetic party-placement fixes; no native gameplay acceptance claimed.

The opt-in `tests/native.rs` test was separately run successfully using
`/usr/lib64/libretro/mgba_libretro.so` and the reviewed local modern Emerald ROM.
It assembled original homebrew ARM code and checked:

1. Two unmodified sessions produce the same framebuffer hash.
2. A virtual ROM color replacement reaches a DMA3 read and changes output.
3. A virtual ARM instruction replacement changes executed behaviour and output.
4. Emerald runs for 120 frames with a **synthetic** managed battery save, outputs
   240 × 160 video and 65,367 stereo audio frames, and commits a validated working
   revision after native cleanup. The immutable imported save is preserved.

Homebrew framebuffer hashes:

| Session | SHA-256 |
| --- | --- |
| Baseline/color | `7de76e66723de3e05803585e150f4c3ec773fbe554593271eb3ef1ecfba65851` |
| DMA color overlay | `ff430cd2149227616ee76b085b2023da4cc222717f8e67e6d4c08904f4a1e40f` |
| Executed instruction overlay | `e2cc2a1fa6131cf4d86faa3baf78851f35a36853e2467c257b3df9d89e85cce5` |

Native core reports `mGBA 0.11-dev%{shortcommit}`; the concrete binary SHA-256 is
`d4ca95eeec5986b40e64823fde92f7d7fb5504d5d1e8499e7188c114d3ce72a7`.
The inspected API reference is upstream 0.10.5; execution evidence applies to this
exact installed binary using the Libretro ABI, not to every mGBA version.

Original ROM before/after SHA-256:
`688999ce6350ff6369e9e1bb7be86d1a0230aadebe54da12d5ad20ed05b24bd5`.
Linux release CLI SHA-256:
`be63dbefef7c8f6aa742a1b657f0f0734395ac9167cdc33d7552d7eede95e620`.

The initial modern-ROM extraction returned **412 species table rows**, **355 move
rows**, and **184 directed evolution rules**, plus learnsets/bitmaps and explicit
missing-resource diagnostics. The extractor now recognizes that exact modern
image and the separately pinned retail image, using independently reviewed table
layouts. Retail extraction also returned 412 species rows, 355 move rows, and 184
directed evolution rules. These are partial table definitions, not a complete
transferable resource bundle or 412 playable species.

Local command logs and a machine-readable receipt are in
`local/verification-2026-11-10/`. Generated local definitions and verification data
are ignored by Git; no source ROM, copyrighted artwork or original save is tracked.

## Follow-up evidence and remaining validation

The initial milestone statements above are historical. A matching clean retail
ROM and a user-created save have since been inspected and imported through the
CLI; the source bytes remained unchanged and one party record was copied into
the Vault. The save is raw 128 KiB data named `.srm`, not a savestate. Full
commands, backup hash and per-check evidence are recorded in
[GAMEPLAY_VALIDATION.md](GAMEPLAY_VALIDATION.md). The later updated save now covers
real managed PC deposit, withdrawal and rollback with independent byte review;
see [PC_TRANSFER_VALIDATION.md](PC_TRANSFER_VALIDATION.md). The user confirmed
Poochyena in the exported save's intended PC slot, and the subsequent saved test
data validates with both original records unchanged. This acceptance is scoped
to that exact-profile scenario. The separate native real-save
regression has now passed; the current default suite has 38 passing tests, with
two opt-in tests ignored by default. These are follow-up results rather than
changes to the historical test counts above.

The initial Tauri storage interface now builds as a standalone Linux executable;
its current suite has 45 passing Rust tests and five frontend tests. The opt-in
real-save desktop service test also passed, including 120 native emulator frames.
See [DESKTOP_MILESTONE.md](DESKTOP_MILESTONE.md) for the separate acceptance
workspace and evidence. The user has confirmed basic native launch/display;
desktop copy/withdraw/undo controls and visible in-game placement of the exported
withdrawal are now user-confirmed. Native picker interaction remains unconfirmed.

Windows has a backend compile check only; Windows desktop and Android runtime
builds are untested. Playable emulator UI, RTC persistence, GB/GBC save adapters, expansion save
codecs, cross-profile conversion, resource installation, binary/source patch
generation and emulator-assisted gameplay hooks remain unimplemented. Unknown
derivatives are blocked from managed writes. The first runtime synchronization
boundary is closing the native session, not pausing live gameplay.

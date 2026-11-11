# Initial desktop milestone — 2026-11-11

The `~/...` paths and `local/...` artifacts below describe local developer
build and acceptance evidence; local ROMs, saves, databases and binaries are not
included in this public repository.

**Workflow update:** the later [linked-save implementation](LINKED_SAVES.md)
supersedes the export-only workflow described in the historical tests below.
The desktop now offers Remove game and Choose save. Deposit removes PC records
from the selected file, withdrawal removes them from the Vault, and undo updates
that same filename. Internal revision history is retained. Old workspace saves
remain unlinked until explicitly chosen; no existing user save was automatically
rewritten during this upgrade. The original transfer tests below remain useful
evidence but are not instructions to create a new ROM after each transfer.

The first desktop workflow connects a Tauri v2 native shell to the tested Rust
backend. It displays real application data and makes real transactional service
calls. A plain browser preview has no native access and shows an explanatory
message; no demonstration collection is substituted.

## Implemented workflow

- **Deposit selected to Vault** is the primary PC storage action: it removes
  selected PC records from the managed save and inserts them into the Vault in
  one transaction. **Copy selected to Vault (keep in save)** is a separate,
  secondary preservation action. The earlier two-Poochyena test deliberately
  exercised copying; this UI correction does not remove those existing copies.
- Deposits/withdrawals affect the selected managed save. An explicit export is
  required to use its changed revision in an external emulator. Party deposits
  remain blocked until safe party writing exists; move Pokémon to PC in-game
  first. Exact source replays, including depositing a previously copied source
  slot, are still blocked by retained import receipts rather than silently
  merging or deleting a Vault entry.
- Add a user-owned ROM, view its identity/hash and compatibility diagnostics.
- Import a matching `.sav`/`.srm` as a managed copy with immutable original bytes.
- Browse the party and all 14 PC boxes, each with 30 numbered slots.
- Inspect individual/source species definitions, moves, evolution rules and missing
  resource diagnostics. Unknown PC levels are shown as unknown rather than guessed.
- Copy selections or atomically move occupied PC slots into the Vault. Original
  battery files are unchanged even when managed PC entries are removed.
- Search the Vault, preview withdrawal, and apply only to the same save revision,
  Vault record and empty slot that were reviewed.
- Undo eligible latest transfers coherently with their Vault changes. Older
  operations and headless sessions are not offered by the desktop Undo button.
- Inspect operation history, export save/profile/definitions into a new directory,
  create consistent backups and run content/database integrity checks.
- Optionally run real mGBA headless diagnostics with a trusted user-selected core.
  This is labelled diagnostics; no playable video/controller interface is claimed.

The desktop uses a single `vault_request` IPC command. All services are also
testable without a window using `DesktopService::dispatch` or the CLI:

```bash
cargo run -- desktop-request /path/to/private-workspace request.json
```

The complete request schema and view shapes are in [DESKTOP_API.md](DESKTOP_API.md).

## Data integrity and migration

Schema 2 adds ROM library metadata and checked read/history queries. It preserves
all schema-1 imported saves, revisions, original records, UUIDs and receipts.
The desktop service makes a consistent schema-1 database backup before migrating
an older workspace, and rejects unrelated files named `vault.sqlite`.

ROM library paths are re-read and SHA-256 checked before import/runtime use.
Only reviewed exact Emerald images receive save codecs. Unknown derivatives stay
inspectable, with imports disabled. The ordered overlays and custom-injection
engine are not exposed as an unverified UI shortcut.

Source labels are inserted as text, never HTML. Only packaged local content and
the native input-file picker are enabled; there is no shell or general filesystem
plugin permission. Output paths are generated below the owned workspace. Native
commands run off the UI thread; SQLite leases and captured revision checks also
exclude conflicting external writers.

Import receipts deliberately survive removal/undo. Re-importing the exact same
source instance is blocked, with its record/history retained. A general history
restore/redo interface is not part of this first desktop milestone.

## Verification and review

- Rust backend suite: **45 passed**, three external-data/native tests ignored by
  default. Schema migration, original preservation, unsupported ROM refusal,
  stale UI selections and transactional rollback have explicit coverage.
- Frontend: TypeScript/Vite build, Prettier check and **5 headless tests** passed.
  Tests cover hostile labels as text, 30-slot rendering, exact preview context,
  unavailable source definitions and Undo eligibility without synthesizing input.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- Linux standalone native desktop release: built with embedded frontend assets.
- Windows Rust backend compile check: passed. Windows desktop and Android runtime
  builds have not been validated.
- The opt-in desktop-service test passed using the actual matching Emerald ROM
  and preserved boxed-Pokémon save. It exercises the same requests as IPC: import,
  deposit, preview, withdraw, stale-revision refusal, coherent undo, export,
  integrity, backup, reopening and a 120-frame managed mGBA session. Inputs remained
  unchanged.
- Independent read-only review found no blocking source-preservation, request-shape,
  preview/apply or migration-backup defect. The identified Undo availability issue
  was corrected and regression-tested.

No native window was opened by the agent and no keyboard, mouse or GUI-input
automation was used. After manually launching the installed build, the user
reported “it shown” in response to the request to check that the window loads and
shows the Pokémon. This confirms the basic launch/display check only. File-picker,
transfer-button and undo interaction were confirmed in the follow-up below.
Native file-picker interaction remains outside that confirmation.

### Next manual transfer check

Use the existing separate acceptance workspace, leaving the NOXgames saves alone.

1. In **Managed save**, select Poochyena in Box 1, slot 2 and choose
   **Copy selected to vault**. Poochyena should remain in that managed PC slot and
   appear in **Vault** alongside Torchic.
2. Select that Vault Poochyena, choose Box 1, slot 3 (if empty), then
   **Preview compatibility**. Confirm the report says the destination is empty and
   lists no blocking problems before selecting **Apply withdrawal**.
3. Return to **Managed save**. The copied individual should now occupy slot 3;
   the existing Poochyena in slot 2 should remain present.
4. Choose **Undo latest**. Slot 3 should become empty again, with slot 2 and the
   copied Vault entry retained. Check **History** for the withdrawal and undo.

This intentionally tests copy behaviour, so two matching Poochyena in the managed
PC after step 3 are expected. Do not repeat the import if it already succeeded;
duplicate receipts persist across undo and will block the same source replay.
Stop at a reported error and retain its text. These steps are a checklist, not a
claim that the manual transfer test has already passed.

### Desktop copy/withdrawal follow-up

The user confirmed that copying Poochyena left it visible in both Vault and PC.
They subsequently saw Poochyena in managed Box 1 slots 2 and 3, but only the old
placement when launching the original or earlier PC-test ROM in NOXgames.

Read-only database inspection established that the desktop transfer belongs to
**Save 1, revision 3**. The existing desktop export was **Save 2, revision 0**;
separately imported saves do not share working revisions. The original
`emerald-clean.srm` and earlier `emerald-vault-pc-test.srm` still matched their
preserved hashes. The desktop does not synchronize managed revisions back into
these source files automatically. A ROM selects the emulator's associated save;
launching it does not select a Vault-managed database revision.

Save 1 revision 3 was exported and parsed: Torchic remains in the party, and
Poochyena occupies Box 1 slots 2 and 3. Both boxed records match the preserved raw
record hash and the parser reports no warnings. A new acceptance copy uses:

- ROM: `~/Games/PokemonVaultTests/Emerald-Vault-Desktop-Test/emerald-vault-desktop-test.gba`
- Battery save: `~/NOXgames/Saves/mGBA/emerald-vault-desktop-test.srm`
- Save SHA-256: `ba5a998c7a0a27227d05f9f4b8b076c61ccc465b17fd1163624ef304ed6ded54`

No existing ROM/save was replaced. The user must launch this distinct ROM using
mGBA and choose the game's **Continue**, avoiding an older emulator savestate.
The user then confirmed: **“that rom shows both poochyena”**. This establishes
in-game visible placement for the exported desktop withdrawal in this exact
Emerald profile. The user subsequently confirmed the requested desktop undo:
**“ok that worked in the app”**. A read-only database check found Save 1 at
revision 4, byte-for-byte equal to its pre-withdrawal revision 2. The withdrawal
is marked undone, and the same Vault UUID and raw Pokémon bytes were restored.
This completes manual acceptance of this copy/withdraw/export/undo workflow;
summary-screen and battle behaviour remain unconfirmed.
Inspection, staging and the separate user-acceptance receipts are in
`local/desktop-milestone-20261111/withdrawal-game-test/`.
For subsequent exports, select the intended managed save explicitly; the Tools
page exports the currently selected save, not every save with the same ROM hash.

### Deposit-removal export follow-up

After the Deposit/Copy label correction, the user reported that depositing
removed Poochyena in the app but left it visible in their externally loaded game.
Read-only inspection found the new operation on **Save 2, revision 1**; Save 1
retained its separately undone state. This confirms that managed revisions are
independent and that external emulator files are still explicit snapshots.

Save 2 revision 1 was exported and parsed with no warnings: Torchic remains in
the party and no boxed Pokémon remain. A newly named acceptance pair was staged:

- ROM: `~/Games/PokemonVaultTests/Emerald-Vault-Deposit-Test/emerald-vault-deposit-test.gba`
- Battery save: `~/NOXgames/Saves/mGBA/emerald-vault-deposit-test.srm`
- Save SHA-256: `019e7be4143aed8357e3f923bc4afdeda5dd95ffb836bd115c28148566693858`

No earlier test file was replaced. After being asked to load this distinct ROM,
Continue without a savestate and check that Poochyena was absent, the user
confirmed **“that worked”**. This establishes user-reported in-game removal for
this exact-profile managed PC deposit/export. Evidence, including the separate
user-acceptance receipt, is in
`local/desktop-milestone-20261111/deposit-game-test/`. Automatic external
emulator save synchronization/reload is not implemented; the current UI's managed
deposit message and export note describe this required boundary.

## Prepared local acceptance workspace

`local/desktop-acceptance-20261111/` contains a separate real-data workspace:

- Matching retail Emerald library entry.
- Managed save 1: one party Torchic and one boxed Poochyena, copied from the saved
  test file the user confirmed in-game.
- One copy-only Torchic entry in the Vault for immediate summary inspection.
- Poochyena remains in the managed PC, ready for the user to test selection,
  deposit and compatibility preview through the desktop.
- An initial verified database backup.

The source snapshot SHA-256 is
`c0254a060f7500aa8ac4742790239cbd30a49a2b788e9542223ffaf9e9aab189`.
The existing NOXgames playthroughs, test saves and all earlier validation databases
were preserved. This workspace is ignored by Git; no real game/save is bundled
into source distributions. Command logs and the build receipt live under
`local/desktop-milestone-20261111/`.

## Build, launch and next gates

The verified local Linux build is copied to
`~/Applications/UniversalPokemonVault-20261111/`.
Run its `launch-vault` script manually to open the prepared acceptance workspace.
The executable SHA-256 is
`bbe61c26cebd23cd769687a30be33b767bcdeafb92946d025a6e4837292ddc7b`.
All dynamic libraries resolved on this machine. The adjacent `build-receipt.json`
records artifacts, checks and preserved source hashes; the project copy is
`local/desktop-milestone-20261111/receipt.json`.
No native window was launched during installation.

The later Deposit/Copy wording correction is installed as
`pokemon-vault-desktop-deposit` in that directory; `launch-vault` now selects it.
Its SHA-256 is `d69d8e96f6c5841405958bb91a8c9bd17a7f05b505f44c182c81a8bd826e49ee`.
The prior executable and `launch-vault.before-deposit-update` remain available.
`local/desktop-milestone-20261111/deposit-ui-update.json` records the passing UI
build, five frontend tests, formatting, managed-deposit regression and native
release build. This correction changes presentation, not existing user data or
the tested transfer transaction. Close the old window and launch manually again
to use the revised labels.

```bash
npm --prefix ui ci --legacy-peer-deps
npm --prefix ui run build
cargo build --locked --release -p pokemon-vault-desktop
# Launch manually with the separate acceptance workspace:
target/release/pokemon-vault-desktop --workspace "$PWD/local/desktop-acceptance-20261111"
```

The default build enables `custom-protocol`, embedding `ui/dist`. Deliberate Vite
development can use `--no-default-features` and the local server on port 1420.
The first npm installation needed `--legacy-peer-deps` with this machine's npm;
the committed lockfile pins the tested dependency set. No system-wide npm setting
was changed.

Next: check the native picker separately, then deepen verified source
extraction/expansion support. Pokémon Pieces is recorded as a future Fakémon test
candidate. Its engine/version and complete dependencies still need verification;
no claim of support or safe custom-species insertion is made here.

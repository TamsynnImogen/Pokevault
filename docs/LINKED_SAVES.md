# Linked save files

The desktop app offers two distinct ways to bring a save into the workspace:

- `import_save` copies a supported file into managed SQLite storage. It remains
  unlinked, and later operations do not write back to the source.
- `choose_save` explicitly links a user-selected regular 128 KiB Emerald save
  file. Its validated contents become the managed working save. Choosing that
  same canonical filename again loads new emulator progress into the same
  managed save ID and advances its history.

Existing workspace saves are not linked automatically during upgrade. A ROM is
never modified. Identification alone does not establish save compatibility:
the ROM needs a reviewed definition bundle and the chosen save must parse with
the supported Emerald codec. The current scope does not claim general identity
or compatibility across games, later generations, or custom hacks.

The linked desktop workspace uses schema 3. Before migrating schema 1 or 2,
the service preserves a consistent database backup. Verified receipts support
reusing the active Vault entry when a legacy copied record is deposited, and
when that same save slot is withdrawn and deposited again; these cases do not
create a replacement UUID.

## Transfers and history

The desktop's Deposit action always removes selected PC records from the managed
working save and updates the linked file. The legacy API still supports explicit
copy-only imports, separate from the desktop transfer controls. Withdrawal
removes the record from the Vault and writes the new save to the same linked
file. Undo restores the corresponding prior database state and publishes that
save state to the same file.

The external filename stays stable while SQLite retains immutable revisions,
backup images, and transfer history. Revision numbers are not implemented by
renaming or overwriting a sequence of external files. Explicit `export_save`
remains available as a separate convenience copy, but exports are not required
for linked transfers.

Before each selected-file write, the service verifies the expected file identity
and hash, records before/after images and a durable intent, then atomically
replaces the chosen file. The operation journal coordinates publication with
the SQLite commit. Startup recovery checks database state against the journal
and completes or restores the matching image. Unexpected external edits are
preserved and block transfers. The explicit recovery action backs up those
conflicting bytes before restoring the committed Vault save.

Close the external emulator before choosing, refreshing, transferring, undoing,
or recovering a save. Reload the game title and choose Continue after a transfer.
Old emulator savestates can restore stale game state and should not be resumed.
A workspace file lock prevents two cooperating Vault app instances from
writing at once; it cannot lock an external emulator.

## Desktop actions

`choose_save {rom_hash, path}` creates or refreshes the explicit link.
`recover_save {save_id, expected_revision, expected_file_sha256}` resolves a
reviewed pending conflict and verifies that the selected file has not changed
since review. `remove_game {rom_hash}` removes only the ROM library entry; its
ROM file, saves, definitions, Vault records, and history remain retained.

See [DESKTOP_API.md](DESKTOP_API.md) for IPC request and response shapes.

## Verified local build and manual check

The existing local `~/Applications/UniversalPokemonVault-20261111/launch-vault`
now selects `pokemon-vault-desktop-linked-saves`. Its SHA-256 is
`8e30240c9cecd6c4fbcacfbab21a33c416d2134b6e4f19d9bcbe03d6e59c9ce8`.
No window was opened automatically. Close the old Vault window and external
emulator, relaunch manually, then use **Choose save** for the actual emulator
battery file. A PC deposit should disappear in-game after title-screen Continue;
a withdrawal should appear there and disappear from the active Vault.

Validation: 58 default Rust tests, eight frontend tests, strict workspace Clippy,
Windows backend compile check and Linux native release build passed. The opt-in
linked desktop service test passed on a temporary copy of the real reviewed
Emerald save. A private copy of the existing schema-2 database migrated with all
record counts and UUIDs preserved. Logs and receipt:
`local/linked-saves-milestone/receipt.json`. Native interaction and in-game
acceptance of this new direct-write workflow remain a human check.

Old executables and launcher backups remain installed. Schema 3 is intentionally
rejected by older binaries: rollback also requires restoring the matching
pre-migration database backup. Do not replace a live database while the app runs.

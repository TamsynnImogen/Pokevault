# Native desktop shell

This Tauri v2 crate is a transport layer for the root Rust vault service and
the initial playable mGBA view. The service remains the owner of
source-preserving storage and its documented operations. ROMs remain read-only;
explicit Choose save links a battery file for backed-up writes through the
shared service. See [playable emulator setup](../docs/PLAYABLE_EMULATOR.md).

## Build and run manually

Install the Tauri v2 system prerequisites, then from the repository root run
`npm --prefix ui ci --legacy-peer-deps`, `npm --prefix ui run build` and
`cargo build --release -p pokemon-vault-desktop`. The default `custom-protocol`
feature embeds the local frontend; no server is needed for that binary.
The initial crate intentionally has installer bundling disabled.

For a separate validation workspace, manually run
`target/release/pokemon-vault-desktop --workspace PATH` from the repository root.
Without that option, the app uses its platform application data directory.
Neither mode automatically imports a ROM, save, database, or test collection.
Unknown arguments and a missing workspace path produce a diagnostic and exit.

Deliberate development can use `npm --prefix ui run dev` and
`cargo run -p pokemon-vault-desktop --no-default-features`; the latter uses
`http://localhost:1420`. Build commands do not launch the native window.

The native window must be launched manually by a human. Do not automate GUI or
input devices during verification. The app can prefill an installed core,
preferring `/usr/lib64/libretro/mgba_libretro.so`, and searches two other
standard Linux locations. The user must select and trust the native core; no
core, BIOS, ROM or Pokémon assets are bundled. Choose save links gameplay to
that selected battery file with backups and recovery. Save normally in-game,
then use Stop & save before transfers; reload through the game's Continue menu,
not a savestate. See [linked-save workflow](../docs/LINKED_SAVES.md) and the
playable emulator guide.

# Clean Emerald play-test build

The paths below are local developer validation examples; ROMs, source trees,
compiler outputs and evidence artifacts are not part of this public repository.

Built 2026-11-10 from a fresh official [pret/pokeemerald](https://github.com/pret/pokeemerald)
checkout, using a fresh official [pret/agbcc](https://github.com/pret/agbcc) compiler
build. Existing Pokémon projects were left untouched.

- Source: `~/NVMeData/Projects/Pokemon/Decomps/Gen3/pokeemerald-clean-vault`
- Source commit: `731ad5bfd6e6f265508d0efcca0ba42f9dcf5881`
- Compiler: `~/NVMeData/Projects/Pokemon/Tools/agbcc-vault-clean`
- Compiler commit: `da598c1d918402c42c0c0d7128ba14567f3175e9`
- Build command: `make -j6 compare` using the default original compiler target.
- Tracked source status after building: clean.
- `sha1sum -c rom.sha1`: **pokeemerald.gba: OK**.
- ROM size: 16,777,216 bytes.
- SHA-1: `f3ae088181bf583e55daf962a92bb46f4f1d07b7`
- SHA-256: `a9dec84dfe7f62ab2220bafaef7479da0929d066ece16a6885f6226db19085af`

The verified play-test copy is:

`~/Games/PokemonVaultTests/Emerald-Vanilla/emerald-clean.gba`

The user completed manual gameplay on this ROM and produced a real battery save.
The observed save is `~/NOXgames/Saves/mGBA/emerald-clean.srm`.
An `.srm` extension is acceptable because this file is the same raw 128 KiB
Emerald battery-save image; it is not an emulator savestate. The original file is
read-only input to the Vault.

The expected default battery-save location at build time (the user instead found
the flushed file in the mGBA save directory above):

`~/Games/PokemonVaultTests/Emerald-Vanilla/emerald-clean.sav`

An existing executable mGBA GUI AppImage is available at:

`~/NVMeData/Projects/Pokemon/Decomps/Gen3/pokeemerald-expansion-mapping/mGBA-0.10.5-appimage-x64.appimage`

No GUI or input devices were automated. The Vault's native headless adapter ran
this exact clean ROM for 120 frames: 240×160 video, 65,367 audio frames, unchanged
ROM SHA-256. No `.sav` was written by that diagnostic.

The real save was CLI-inspected and imported into a managed database; one party
Pokémon (Torchic, level 5, Scratch/Growl) was copied into the Vault. The source
save's bytes remained unchanged. Its verified pre-import backup is
`local/emerald-gameplay-20261110T231918Z/original-save.srm` with SHA-256
`7e0a8fd6d25e2c12a036303fc848f3a71226669243b652b8366c26edb684743e`.
The record had valid checksums and the first normal save slot; the other slot was
erased as expected for a first save. There was no PC Pokémon, so a real-save PC
withdrawal check was pending for that first snapshot. The later boxed-Pokémon
save has now passed managed deposit/withdrawal/rollback checks; see
[PC_TRANSFER_VALIDATION.md](PC_TRANSFER_VALIDATION.md). The user confirmed the
intended PC placement in the separate playable copy; saved-data checks also
preserved both original Pokémon records. This is scenario-specific acceptance.

The source ELF/map and compiler outputs remain in the clean checkout. Logs,
artifact hashes, source/compiler commits and extracted retail table symbol
addresses are retained in `local/clean-emerald-build/`. A copy of the JSON build
receipt is beside the play-test ROM. These are local validation artifacts and
are not bundled into the Vault's public source.

This build supplies the retail ELF/map evidence used by the Vault's current
exact-hash-pinned retail extractor. Do not use a definition bundle from the
separate modern build with this retail ROM.

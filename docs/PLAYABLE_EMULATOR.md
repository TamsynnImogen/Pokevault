# Play a linked Emerald save

The desktop now has an initial playable game view. It opens the ROM already
registered in the library with the selected **linked managed save** and an
explicitly selected local mGBA Libretro core. The reviewed save codec and Vault
integration cover the supported Emerald profile only; mGBA's ability to run
other systems or hacks does not imply their save formats are supported.

## Setup and controls

1. Close external emulators using this save. Add the game ROM to the library and choose the battery save file you normally
   use. The app links that exact file and keeps its backup/recovery history.
2. Open **Play game** and select a trusted mGBA Libretro native library. Loading
   a core executes native code. On Linux, the app pre-fills the first installed
   path it finds, preferring `/usr/lib64/libretro/mgba_libretro.so`, then
   `/usr/lib/x86_64-linux-gnu/libretro/mgba_libretro.so` and
   `/usr/lib/libretro/mgba_libretro.so`.
3. Select **Start game**. Click the game display to direct keyboard controls to
   it. Arrow keys move; **Z** is B, **X** is A, **Enter** is Start, **Shift** is
   Select, **A** is L and **S** is R. The on-screen buttons provide the same
   controls.
4. Use **Pause** and **Resume** as needed. If browser focus is lost, the page is
   hidden, or you navigate away, input buttons are released and the game pauses.
   The managed-save lease stays held until the session is stopped and closed.
5. WebAudio may be blocked by the platform until enabled from a user action. If
   there is no sound, press **Enable audio**. The UI also attempts to enable it
   from the Start game and Resume actions.

The frame channel uses the `PVF1` binary packet: a 28-byte little-endian header,
RGBA pixels and interleaved stereo signed PCM16 samples. Packet dimensions,
sample rate and sequence are carried in the packet; display pacing uses the core
reported FPS rather than assuming 60 FPS. The sender bounds unacknowledged
packets to three and audio buffers are bounded.

## Saving, stopping and recovery

Save normally through the game's own in-game save menu. Then use **Stop & save**
before editing Pokémon in the Vault, transferring, or closing the app. Stop joins
and unloads the native core, checks the detached SRAM with the selected Emerald
save codec, and publishes it back to the same chosen battery file through the
existing journal and backup flow. Starting/restarting the session loads that
updated battery save. Do not use savestates to synchronize with Vault edits; the
app does not write live game RAM or synchronize savestates.

Closing the native window attempts Stop & save on a worker thread. If stopping,
validation or publication fails, the close is cancelled and the error is shown
so the session can be retried or recovered. **Keep recovery copy & close
session** is available for a stopped session whose save could not be published.
After the core has stopped, it retains the raw SRAM as `stopped.sav` under
`<workspace>/emulator-sessions/<UUID>/`, alongside `initial.sav`,
`profile.json` and `lease.json`, then releases the lease without replacing the
selected battery file or changing Vault records. Inspect or recover that copy
manually before continuing.

A process crash can leave a stale database lease. It fails closed and requires
manual recovery; do not assume it was safely released. RTC metadata persistence,
gamepad support, input remapping and mobile behavior have not been implemented
or manually tested.

## Verification boundary

Automated/native evidence currently includes 120 streaming packets, two service
checks and a 60-frame homebrew input check. UI tests cover the view logic. These
checks do not establish manual native-window gameplay or audio acceptance; those
remain pending. See [VERIFICATION.md](VERIFICATION.md) for the project evidence
and outstanding acceptance gates.

The implementation follows the public [Libretro API header](https://github.com/libretro/libretro-common/blob/master/include/libretro.h)
and the local mGBA 0.10.5 adapter and license references. The mGBA component is
an unbundled user-installed dependency; distribution/license clearance for a
particular core build has not been established here. No core, BIOS, ROM, save or
Pokémon assets are bundled.

## Local standalone artifact

The existing `~/Applications/UniversalPokemonVault-20261111/launch-vault` now
selects `pokemon-vault-desktop-playable`. Close the old Vault window and NOXgames,
then launch it manually. SHA-256:
`7d0292e0c692224f5757dbe556b163edb745c732862e70c17f4f5fd94c11a8a2`.
The previous binary and `launch-vault.before-playable` remain available.

The build receipt is `local/emulator-milestone/receipt.json` (also copied beside
the installed executable). It records 61 passing Rust tests, 14 frontend tests,
five opt-in native regressions, all build checks and preserved source hashes.
The installed system core identifies itself as `0.11-dev%{shortcommit}` with hash
`d4ca95eeec5986b40e64823fde92f7d7fb5504d5d1e8499e7188c114d3ce72a7`;
the upstream 0.10.5 source was an API reference, not a substituted version claim.
No core library was copied into this application installation.

Installation inspected the existing workspace read-only: its Vault remained
empty, seven managed saves were retained and no native session lease was active.
No new window was launched. Video, audio, actual user controls and window-close
acceptance remain pending manual confirmation.

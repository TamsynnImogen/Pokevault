# Implementation plan and work split

The linked-save follow-up is implemented: Remove game, Choose save with stable
path/ID, deposit and withdrawal to the same selected battery filename, backed-up
publication and guarded recovery. Internal revisions remain immutable. See
[LINKED_SAVES.md](LINKED_SAVES.md); historical read-only-source milestones below
refer to legacy imports. Manual native gameplay/audio acceptance remains
pending.

No pre-existing Universal Pokémon Vault checkout was located in the Pokémon
workspace on 2026-11-10. This isolated starter preserves all adjacent projects;
integrate it into the existing repository if the owner supplies another path.

1. **LEAD — integrity architecture:** identify by SHA-256, define exact save
   profiles, inspect pret Emerald layouts, implement strict box-record handling,
   and prohibit unknown hacks from entering the vanilla adapter.
2. **MEDIUM — managed persistence:** SQLite immutable save revisions, original
   import backups, atomic save/Vault changes, optimistic revision checks and
   coherent undo. This unit must not interpret Pokémon bytes.
3. **MEDIUM — virtual images:** ordered, versioned copy-on-write range layers,
   exact dependencies, overlap rejection and expected-byte checks; produce a
   contiguous private image for the emulator. No game-specific injection guessed.
4. **LEAD — emulator adapter:** investigate mGBA licensing/API; implement real
   headless diagnostics and the initial playable desktop view through an
   explicitly selected local mGBA core. Feed only private ROM bytes and managed
   save bytes. Runtime hooks remain off. Manual native gameplay/audio acceptance
   is still pending.
5. **LEAD — service/CLI and registry:** direct same-profile Emerald PC transfers,
   whole-box atomic operations, conservative compatibility reports, export/undo,
   and persistent overlay manifests. Document the full target architecture.
6. **MEDIUM — independent review:** integrity tests and native boundary review.

Acceptance: adversarial parser/overlay/transaction tests; actual mGBA frame
execution with unchanged original hashes; truthful support matrix. Linux native
proof does not imply Windows/Android or manual in-game acceptance.

Since this initial plan, partial species/move/evolution extraction was added for
the two exact retail and modern ROM hashes; it is not a complete transferable
resource bundle. Next gates: pinned expansion build manifests and save codecs;
validated missing resource definitions; verified injection plans; manual native
gameplay/audio acceptance; cross-platform native packaging; only then
engine-specific runtime hooks. GB/GBC save codecs are not implemented or implied
by mGBA's ability to emulate those systems. No arbitrary memory structure
inference or live save writes.

The initial functional Tauri desktop workflow is now implemented and built; see
[DESKTOP_MILESTONE.md](DESKTOP_MILESTONE.md). It calls the real shared services,
with schema-2 migration backups and revision-bound previews. Native UI interaction
has user-confirmed launch/display and copy/withdraw/undo acceptance, including
in-game placement of the separate export. Native picker interaction remains
unconfirmed. The playable view is implemented; its native window/gameplay/audio
behavior still requires manual acceptance.

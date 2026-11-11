# Format evidence and current support

## ROMs and game definitions

Plain GB/GBC/GBA headers give platform/title/revision/checksum hints. GB/GBC have
no installed game-specific save adapters. Other Gen 3 games, archives, NDS, 3DS,
Essentials and Evolution-native formats are unsupported. A matching BPEE header
does **not** select the Emerald codec.

Retail Emerald USA/Europe is recognised by an exact SHA-256 and pret's published
SHA-1 fingerprint `f3ae088181bf583e55daf962a92bb46f4f1d07b7`; all source identities
and profiles use SHA-256. Partial extraction is implemented for two exact images, each
with a separate profile and independently checked layout. Retail is pinned to
SHA-256 `a9dec84dfe7f62ab2220bafaef7479da0929d066ece16a6885f6226db19085af`;
the earlier local modern build remains pinned separately to
`688999ce6350ff6369e9e1bb7be86d1a0230aadebe54da12d5ad20ed05b24bd5`.

One local modern Emerald build has a reviewed diagnostic table layout:

- ROM SHA-256: `688999ce6350ff6369e9e1bb7be86d1a0230aadebe54da12d5ad20ed05b24bd5`
- Length: 16,777,216 bytes; `pokeemerald_modern.gba`, distinct from matching retail.
- Source tree HEAD inspected: `c2dcc629fd97b3d58fedc07c9e02653eabed9136`.
- ELF `readelf -sW` table evidence:

| Symbol | ROM address | ELF size | Decoder |
| --- | --- | ---: | --- |
| gSpeciesNames | 0x0863740c | 4532 | 412 × 11-byte names |
| gSpeciesInfo | 0x0877c8dc | 11536 | 412 × 28-byte records |
| gMoveNames | 0x086385c0 | 4615 | 355 × 13-byte names |
| gBattleMoves | 0x08778d38 | 4260 | 355 × 12-byte records, raw bytes retained |
| gLevelUpLearnsets | 0x08785a08 | 1648 | 412 pointers; bounded 16-bit terminated lists |
| gEvolutionTable | 0x087819a8 | 16480 | 412 × 5 × 8-byte source-local rules |
| gTMHMLearnsets | 0x0877ada4 | 3296 | 412 × 8-byte bitmaps, move ordering unresolved |
| gEggMoves | 0x086397c8 | 2278 | Raw preservation only; decoding unresolved |

The retail extraction layout was checked against the clean build's ELF symbols;
its byte offsets in the ROM are `0x3185c8` (species names), `0x3203cc` (species
info), `0x31977c` (move names), `0x31c898` (battle moves), `0x32937c` (level-up
learnsets), `0x32531c` (evolutions), `0x31e898` (TM/HM bitmaps), and `0x32add8`
(egg moves). These offsets apply only to that exact retail SHA-256. The separate
modern layout is documented in the table above.

The extractor is pinned to **exact bytes**. Source HEAD alone is insufficient to
reproduce an old modern build. Entry zero and reserved species entries count among
the 412 table rows; this does not mean 412 playable species. Move effect IDs,
abilities, items, types and evolution methods remain source-local. No automatic
canonical mapping is asserted. Western text decoding is partial and preserves
unimplemented glyphs as `\xHH`; Japanese strings are not translated.

Resources missing from this partial bundle: validated sprites, animations, icons,
palettes, cries, Pokédex text, TM/HM move ordering, tutor/egg learnset decoding and
complete item/ability definitions. Same-profile transfers preserve their existing
numeric references byte-for-byte. The bundle cannot install a custom species.

## Vanilla Emerald battery save

The codec accepts exactly 128 KiB, including a raw `.srm` file containing those
same bytes. The extension does not make it a generic emulator format. The codec
rejects savestates, RTC trailers, compressed saves, 64 KiB truncations and other
emulator wrappers. It follows the inspected pret
Emerald layout: two 14-sector slots; 4096-byte sectors with up to 3968 data bytes;
IDs/checksum/signature/counter at 0xff4/0xff6/0xff8/0xffc.

Logical checksum lengths:
`[0xf2c, 0xf80, 0xf80, 0xf80, 0xf08, 0xf80, 0xf80, 0xf80, 0xf80,
0xf80, 0xf80, 0xf80, 0xf80, 0x7d0]`.

Section IDs establish ordering; slots must be complete with unique IDs, matching
counters, correct parity, signatures and checksums. The backup may be unavailable;
that produces a diagnostic. No mixing partial sectors to reconstruct a save.
MAX-to-zero rollover follows the game. SaveBlock1 party count/records are at
0x234/0x238; records are 100 bytes. PokemonStorage is 0x83d0 bytes, and its boxed
array begins at aligned offset **4**, despite an old source comment saying 1.
Boxed records are 80 bytes; PID/OT XOR encryption and all 24 permutations apply.

Writes are PC-only, to private images. Party data, trainer data, backup slot and
four non-slot sectors are retained. The active counter stays unchanged; rewritten
storage checksums are verified before SQLite commit. Pokédex flags are not updated
by withdrawal yet; party stat recalculation is not implemented. Deposits copy by
default; explicit managed removal supports PC entries only. Empty selection,
occupied destination, Bad Eggs, invalid checksums and mismatched contexts block.

Every Vault instance gets an independent UUID. Duplicate receipts bind original
save hash, full profile, source location and raw record hash. Identical records
in separate slots are distinct. Replaying a previous receipt is blocked even
after withdrawal/undo; re-importing an undone receipt requires a future explicit
restore workflow. Cross-save duplicate heuristics are not automatic identity merges.

## Expansion is a separate engine

[rh-hideout/pokeemerald-expansion](https://github.com/rh-hideout/pokeemerald-expansion)
changes Pokémon fields and save layout. The checked upstream `save.h` uses 116
bytes after sector data for SaveBlock3; its current Pokémon structure differs
from the vanilla 80-byte format. The separate installed checkout
`7644501f3065b38aea2cfc64b5b58ca011bce4f5` also changes language/nature, ability and
status fields. Neither is admitted by the vanilla parser because it shares a header.

Expansion support needs build-pinned struct sizes/offsets, feature configuration,
save version/migration rules and source-derived species/move/resource definitions.
No source patches, binary injectors, BPS export or gameplay hooks are implemented
yet. The generic virtual byte overlay is implemented; custom-species installation
is not implied by that capability.

## Platform and runtime boundaries

Linux native core execution is tested. The initial Linux Tauri storage interface
builds with embedded assets; native window interaction still requires human
validation. Windows has a backend compile check; Windows desktop, Android's
SQLite/C toolchain and packaged native cores remain unvalidated. Controller input,
interactive video/audio playback, persistent RTC metadata and save-state UI remain
future work. See [DESKTOP_MILESTONE.md](DESKTOP_MILESTONE.md).
No native core is redistributed by this repository. See ARCHITECTURE.md for MPL
source/notice obligations and the planned native C shim.

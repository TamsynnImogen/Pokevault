# First gameplay-save validation — 2026-11-10

The `~/...` paths and `local/...` artifacts below describe local developer
validation data; no ROM, save, database or generated definition bundle is included.

The user created an in-game save from the matching clean Emerald ROM using
NOXgames' mGBA core. This validates actual save extraction and copy import; it
does not establish in-game acceptance of a modified or withdrawn Pokémon.

## Preserved inputs and profile

- User save: `~/NOXgames/Saves/mGBA/emerald-clean.srm`
- Resolved path: `~/NVMeData/NOXgames/Saves/mGBA/emerald-clean.srm`
- Size: 131,072 bytes; raw Emerald flash despite the `.srm` filename extension.
- Original and backup SHA-256:
  `7e0a8fd6d25e2c12a036303fc848f3a71226669243b652b8366c26edb684743e`
- Exact ROM SHA-256:
  `a9dec84dfe7f62ab2220bafaef7479da0929d066ece16a6885f6226db19085af`
- Immutable validation snapshot:
  `local/emerald-gameplay-20261110T231918Z/original-save.srm`

The original save was read only. Before import, a separate backup was copied,
flushed, reread and hash-verified. Final original, backup and managed working
bytes remained identical. No ROM, source save, NOXgames settings, GUI or input
device was modified or automated.

Retail definitions now have their own hash-pinned extractor layout derived from
the fresh matching build's ELF/map. It returned 412 species-table rows, 355 move
rows and 184 evolution rules, with explicit missing-resource diagnostics. An
independent reviewer confirmed every table offset/extent, all learnset pointers
and terminators, and evolution targets. The earlier modern definition output
remained unchanged (digest `68ffcffd420b2b79a752b6000f32485e0188c3cb5248e76c67f8fe0b427692b1`).

## Actual save contents and copy import

The active slot is slot 1, counter 1. All 14 section signatures/checksums validate.
Slot 0 is entirely erased flash; the diagnostic now identifies it as an unused
slot, which is expected for this first save.

One party Pokémon was decoded:

| Species/nickname | Level | Moves | Location |
| --- | ---: | --- | --- |
| Torchic | 5 | Scratch, Growl | Party slot 0 |

There are no occupied PC slots. The 100-byte encrypted party record was preserved
unchanged with SHA-256
`f8fd76d04420da4e4a4f5c333d2742cffdae1e46bf10884ace75de11d52c15ce`.

The CLI successfully performed managed-save import, copy-only party deposit,
Vault listing, SQLite/content integrity checks and a consistent Vault backup.
The instance has an independent Vault UUID and an immutable reference to its
source definition bundle. It was **copied**, so Torchic remains in the game save.

Local validation artifacts:

- `local/emerald-gameplay-20261110T231918Z/retail-definitions.json`
- `local/emerald-gameplay-20261110T231918Z/gameplay-vault.sqlite`
- `local/emerald-gameplay-20261110T231918Z/vault-after-import.sqlite`
- `local/emerald-gameplay-20261110T231918Z/receipt.json`
- CLI output/diagnostic captures in that same directory.

Real saves, the database and generated definitions stay under ignored `local/`;
they are not public fixtures and are not bundled into source distributions.

## Checks and limits

- Default suite: **38 passed**, two external-data/native tests ignored by default.
- `cargo clippy --offline --locked --all-targets -- -D warnings`: passed.
- The new opt-in `tests/gameplay.rs` passed using the verified backup, exact retail
  ROM and trusted mGBA core. It checked binary-preserving copy import, source rule
  references, atomic undo, immutable original hashes, and a 120-frame managed
  native session. The party record survived the native session unchanged.
- Actual PC deposit/withdrawal branch: **not exercised**, because the save has no
  boxed Pokémon. Synthetic PC roundtrip tests remain separate evidence.
- Manual acceptance after a managed save transfer: **pending**.

To reproduce the external-data test, set `UPV_ROM`, `UPV_SAVE` and optionally
`UPV_CORE`, then run:

```bash
cargo test --locked --test gameplay -- --ignored --nocapture
```

Use a verified backup for `UPV_SAVE`. The test's databases and native save buffers
are private temporary copies, and it verifies input files again afterward.

## Follow-up acceptance

The user supplied the requested updated save with a boxed Poochyena. Real managed
PC deposit, withdrawal, rollback and independent byte comparisons have since
passed; see [PC_TRANSFER_VALIDATION.md](PC_TRANSFER_VALIDATION.md). The user has
confirmed the exported save's intended PC placement in-game, and the saved test
data was reparsed with both original records intact. The first-save observations
above remain historical. Party-to-PC conversion is still deliberately blocked.

# Real PC transfer and rollback validation — 2026-11-10

The `~/...` paths and `local/...` artifacts below describe local developer
validation data; no ROM, save, database or generated definition bundle is included.

The user's updated NOXgames save contains level-7 Torchic in the party and
Poochyena in Box 1's first slot. Both normal flash-save slots validate without
warnings. Active counter is 2 in slot 0; backup counter is 1 in slot 1.

## Preserved source and private operations

The source `~/NOXgames/Saves/mGBA/emerald-clean.srm` was backed up before
testing. Original and snapshot SHA-256:

`f430a049805320a76309800f12bb29905c4a3234b1ae70085071d09e54798f24`

The new snapshot and evidence live under
`local/emerald-pc-20261110T233253Z/`. The previous first-save snapshot and Vault
database were retained. The ROM still matches the retail SHA-256 recorded in
[CLEAN_EMERALD.md](CLEAN_EMERALD.md).

The actual CLI exercised these operations on a new application-owned database:

1. Import the verified snapshot as managed save 1, revision 0.
2. Deposit Poochyena from zero-based box 0/slot 0, explicitly removing it from
   the managed copy only. Revision 1 contains an empty source slot and one Vault entry.
3. Preview withdrawal into zero-based box 0/slot 1. It is empty and exact-profile
   compatible; no field conversion or ROM modification is needed.
4. Commit withdrawal at revision 2. The first slot remains empty; the second
   contains the exact original Poochyena record, and the active Vault is empty.
5. Preserve an independent revision-2 database backup and save export.
6. Undo the withdrawal at revision 3. The save returns byte-for-byte to revision 1,
   and the same Vault UUID and raw record return together in one transaction.
7. Check database/content integrity and preserve the rollback database separately.

The source record is 80 bytes. Its SHA-256 before deposit, after withdrawal and
after rollback into the Vault is identical:

`eeda048e73906d39a1c396c0e5200230cfd47bdc1f71a9584a6150d3486ad339`

| Private save state | SHA-256 |
| --- | --- |
| After deposit | `019e7be4143aed8357e3f923bc4afdeda5dd95ffb836bd115c28148566693858` |
| After withdrawal into second slot | `1e079d3831f8385d715c46f50e8eb2c183b7fa758be433e5d5d9b5189f2e2809` |
| After withdrawal rollback | `019e7be4143aed8357e3f923bc4afdeda5dd95ffb836bd115c28148566693858` |

## Independent and runtime checks

An independent read-only reviewer checked sector placement against the clean pret
source and compared all four complete flash images. Both 14-sector slots retain
valid signatures/checksums and their original counters. Only the intended PC
record ranges and affected checksum bytes change; Torchic and all unrelated
bytes remain unchanged. Read-only SQLite inspection confirms UUID/raw restoration
and the undone withdrawal event.

The opt-in gameplay integration regression passed with this updated real save:
two records, `pc_roundtrip_exercised=true`, and a 120-frame mGBA managed session.
It also checks a same-slot roundtrip restores the complete input image. This is
headless startup/save preservation evidence; manual game acceptance remains a
separate check. No application code change was needed for these real PC records.
The same external-data regression also passed using the relocated revision-2
export as input, including its PC branch and 120 native frames. Its log is
`local/emerald-pc-20261110T233253Z/relocated-native-test.log`.

## Separate playable acceptance copy

An unchanged ROM copy is staged at:

`~/Games/PokemonVaultTests/Emerald-Vault-PC-Test/emerald-vault-pc-test.gba`

Its relocated battery save uses a new basename in NOXgames' existing mGBA folder:

`~/NOXgames/Saves/mGBA/emerald-vault-pc-test.srm`

The staged save has the revision-2 withdrawal hash above. The source ROM and
`emerald-clean.srm` were not replaced. A profile and transfer receipt are beside
the new ROM. No savestate or GUI automation was used.

## User confirmation and saved-data validation

The user confirmed: **“Poochyena is in box 1 slot 2.”** This establishes visible
in-game placement for this exact-profile Emerald test in NOXgames.

The resulting battery save was separately snapshotted and reparsed successfully:

- Save SHA-256: `c0254a060f7500aa8ac4742790239cbd30a49a2b788e9542223ffaf9e9aab189`.
- Both party/PC records validate without save warnings.
- Poochyena's original 80-byte record remains unchanged in Box 1's second slot.
- The first slot is empty, and Torchic's complete party record remains unchanged.
- The original `emerald-clean.srm` still matches its preserved source snapshot.

Evidence is retained under
`local/emerald-pc-20261110T233253Z/user-confirmation-20261110T235031Z/` and in the
updated transfer receipt. No source or test battery file was rewritten during
this inspection.

Acceptance is scoped to the reported PC placement and the validated saved data.
The user has not reported summary-screen or battle behaviour. Cross-game
conversion, custom content injection and party-to-PC conversion are outside
this exact-profile test.

## Desktop withdrawal acceptance — 2026-11-11

The subsequent Tauri workflow copied Poochyena into the Vault and withdrew it
into another empty managed PC slot. Save 1 revision 3 was exported into a newly
named `emerald-vault-desktop-test.srm`, alongside a separately named unchanged ROM
copy. Parsing validated Poochyena in Box 1 slots 2 and 3, both preserving its
original 80-byte record, with no parser warnings.

The user confirmed **“that rom shows both poochyena”**. This is visible in-game
placement acceptance for the desktop-produced export, separate from the earlier
CLI relocation test. The later requested desktop undo was also confirmed by the
user. Read-only inspection found Save 1 revision 4 identical to pre-withdrawal
revision 2 and the same Vault UUID/raw record restored. Battle behaviour and
cross-game compatibility remain outside this test.
See [DESKTOP_MILESTONE.md](DESKTOP_MILESTONE.md) for
the save-selection diagnosis, artifact paths and receipt location.

The later desktop **Deposit** operation on Save 2 revision 1 removed Poochyena
from its managed PC. Its separately staged `emerald-vault-deposit-test.srm`
parsed without warnings, with Torchic in the party and no boxed Pokémon. The
user confirmed **“that worked”** after being asked to check the exported removal
in-game. This validates visible deposit removal as well as the earlier withdrawal
placement. External NOXgames saves still require explicit export; automatic
save synchronization is not implemented.

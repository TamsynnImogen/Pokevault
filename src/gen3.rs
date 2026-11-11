//! Verified vanilla Emerald battery-save codec, not a generic ROM-hack parser.
//!
//! The format follows pret/pokeemerald `save.c`, `save.h`, `pokemon.h` and
//! `pokemon_storage_system.h`. All offsets here belong ONLY to this codec.
//! See docs/FORMATS.md before adding an engine or changing a checksum boundary.

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};

/// Standard Emerald flash image; savestates and save wrappers are not accepted.
pub const SAVE_SIZE: usize = 128 * 1024;
const SECTOR_SIZE: usize = 4096;
const SECTIONS: usize = 14;
const SIGNATURE: u32 = 0x08012025;
// SaveBlock2, four SaveBlock1 chunks, then nine PokemonStorage chunks.
const LENGTHS: [usize; SECTIONS] = [
    0xf2c, 3968, 3968, 3968, 0xf08, 3968, 3968, 3968, 3968, 3968, 3968, 3968, 3968, 0x7d0,
];

// For each PID % 24, these are the physical positions of growth, attacks, EVs,
// and miscellaneous substructures. This is NOT a list of physical block names.
const POSITIONS: [[usize; 4]; 24] = [
    [0, 1, 2, 3],
    [0, 1, 3, 2],
    [0, 2, 1, 3],
    [0, 3, 1, 2],
    [0, 2, 3, 1],
    [0, 3, 2, 1],
    [1, 0, 2, 3],
    [1, 0, 3, 2],
    [2, 0, 1, 3],
    [3, 0, 1, 2],
    [2, 0, 3, 1],
    [3, 0, 2, 1],
    [1, 2, 0, 3],
    [1, 3, 0, 2],
    [2, 1, 0, 3],
    [3, 1, 0, 2],
    [2, 3, 0, 1],
    [3, 2, 0, 1],
    [1, 2, 3, 0],
    [1, 3, 2, 0],
    [2, 1, 3, 0],
    [3, 1, 2, 0],
    [2, 3, 1, 0],
    [3, 2, 1, 0],
];

/// Location within a save; locations distinguish genuinely identical individuals.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Location {
    Party { slot: usize },
    Box { box_index: usize, slot: usize },
}

/// Normalised individual fields with source-local IDs, not universal IDs.
/// Names/definitions are resolved by the source registry; raw bytes retain every
/// field that is not yet decoded, including unknown bits and text glyphs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pokemon {
    pub location: Location,
    pub personality: u32,
    pub trainer_id: u16,
    pub secret_id: u16,
    pub nickname: String,
    pub trainer_name: String,
    pub language: u8,
    pub species_id: u16,
    pub held_item_id: u16,
    pub experience: u32,
    pub friendship: u8,
    pub moves: [u16; 4],
    pub pp: [u8; 4],
    pub pp_ups: [u8; 4],
    pub evs: [u8; 6],
    pub ivs: [u8; 6],
    pub nature_id: u8,
    pub ability_slot: u8,
    pub shiny: bool,
    pub is_egg: bool,
    pub pokerus: u8,
    pub met_location_id: u8,
    pub met_level: u8,
    pub met_game_id: u8,
    pub ball_id: u8,
    pub ribbons_bits: u32,
    /// Only party records contain a stored level/current stats; PC level needs
    /// the source species growth curve and is deliberately left unknown here.
    pub party_level: Option<u8>,
    pub current_hp: Option<u16>,
    pub maximum_hp: Option<u16>,
    pub status: Option<u32>,
    /// Encrypted 80-byte boxed or 100-byte party record, unchanged.
    pub raw: Vec<u8>,
}

/// Decode conservative Western glyphs. Unimplemented glyphs stay visible as hex
/// escapes instead of being silently replaced; Japanese text needs another codec.
pub fn decode_text(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes.iter().take_while(|&&b| b != 0xff) {
        match b {
            0 => out.push(' '),
            0xa1..=0xaa => out.push(char::from(b'0' + b - 0xa1)),
            0xbb..=0xd4 => out.push(char::from(b'A' + (b - 0xbb))),
            0xd5..=0xee => out.push(char::from(b'a' + (b - 0xd5))),
            0xab => out.push('!'),
            0xac => out.push('?'),
            0xad => out.push('.'),
            0xae => out.push('-'),
            0xb8 => out.push(','),
            _ => out.push_str(&format!("\\x{b:02X}")),
        }
    }
    out
}

// All callers establish slice sizes before using these small endian readers.
fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes[at..at + 2].try_into().expect("checked format extent"))
}
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("checked format extent"))
}

/// Decode one encrypted record. A corrupt record fails the import, rather than
/// manufacturing a Bad Egg or substituting guessed species data.
pub fn decode_record(raw: &[u8], location: Location) -> Result<Pokemon> {
    ensure!(
        raw.len() == 80 || raw.len() == 100,
        "expected encrypted 80/100-byte record"
    );
    ensure!(raw[19] & 1 == 0, "Bad Egg flag is set");
    ensure!(raw[19] & 2 != 0, "record has no species flag");
    let pid = u32_at(raw, 0);
    let ot = u32_at(raw, 4);
    let key = pid ^ ot;
    let mut decrypted = [0u8; 48];
    for (i, word) in raw[32..80].chunks_exact(4).enumerate() {
        let value = u32::from_le_bytes(word.try_into()?) ^ key;
        decrypted[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    let checksum = decrypted.chunks_exact(2).fold(0u16, |sum, w| {
        sum.wrapping_add(u16::from_le_bytes([w[0], w[1]]))
    });
    ensure!(checksum == u16_at(raw, 28), "Pokemon checksum mismatch");
    let positions = POSITIONS[(pid % 24) as usize];
    let block = |kind: usize| &decrypted[positions[kind] * 12..positions[kind] * 12 + 12];
    let growth = block(0);
    let attacks = block(1);
    let ev = block(2);
    let misc = block(3);
    let ivword = u32_at(misc, 4);
    let origins = u16_at(misc, 2);
    ensure!(u16_at(growth, 0) != 0, "species zero in nonempty record");
    Ok(Pokemon {
        location,
        personality: pid,
        trainer_id: ot as u16,
        secret_id: (ot >> 16) as u16,
        nickname: decode_text(&raw[8..18]),
        trainer_name: decode_text(&raw[20..27]),
        language: raw[18],
        species_id: u16_at(growth, 0),
        held_item_id: u16_at(growth, 2),
        experience: u32_at(growth, 4),
        friendship: growth[9],
        moves: std::array::from_fn(|i| u16_at(attacks, i * 2)),
        pp: attacks[8..12].try_into()?,
        pp_ups: std::array::from_fn(|i| (growth[8] >> (2 * i)) & 3),
        evs: ev[..6].try_into()?,
        ivs: std::array::from_fn(|i| ((ivword >> (5 * i)) & 31) as u8),
        nature_id: (pid % 25) as u8,
        ability_slot: (ivword >> 31) as u8,
        shiny: ((ot as u16) ^ ((ot >> 16) as u16) ^ (pid as u16) ^ ((pid >> 16) as u16)) < 8,
        is_egg: ivword & (1 << 30) != 0,
        pokerus: misc[0],
        met_location_id: misc[1],
        met_level: (origins & 127) as u8,
        met_game_id: ((origins >> 7) & 15) as u8,
        ball_id: ((origins >> 11) & 15) as u8,
        ribbons_bits: u32_at(misc, 8),
        party_level: (raw.len() == 100).then(|| raw[84]),
        current_hp: (raw.len() == 100).then(|| u16_at(raw, 86)),
        maximum_hp: (raw.len() == 100).then(|| u16_at(raw, 88)),
        status: (raw.len() == 100).then(|| u32_at(raw, 80)),
        raw: raw.to_vec(),
    })
}

/// Flash-sector checksum, summing little-endian words with 32-bit wrapping then
/// folding the high half once, exactly as the game does (not a CRC).
pub fn section_checksum(bytes: &[u8]) -> u16 {
    let sum = bytes.chunks_exact(4).fold(0u32, |sum, word| {
        sum.wrapping_add(u32::from_le_bytes(word.try_into().expect("four bytes")))
    });
    (sum.wrapping_add(sum >> 16)) as u16
}

/// A validated active slot plus original image and diagnostics about its backup.
pub struct EmeraldSave {
    bytes: Vec<u8>,
    sectors: [usize; SECTIONS],
    counter: u32,
    pub warnings: Vec<String>,
}

// Slots rotate section positions, so IDs determine logical order. Mixed counters,
// duplicate IDs and parity mismatches are rejected even if checksums happen to match.
fn validate_slot(bytes: &[u8], slot: usize) -> Result<([usize; SECTIONS], u32)> {
    // A newly created save legitimately leaves its other flash slot erased. Give
    // that case a useful diagnostic instead of calling the erased 0xffff ID bad.
    let start = slot * SECTIONS * SECTOR_SIZE;
    if bytes[start..start + SECTIONS * SECTOR_SIZE]
        .iter()
        .all(|b| *b == 0xff)
    {
        bail!("unused slot (erased flash)");
    }
    let mut sectors = [usize::MAX; SECTIONS];
    let mut counter = None;
    for position in 0..SECTIONS {
        let offset = (slot * SECTIONS + position) * SECTOR_SIZE;
        let section = &bytes[offset..offset + SECTOR_SIZE];
        let id = u16_at(section, 0xff4) as usize;
        ensure!(id < SECTIONS, "invalid section id");
        ensure!(sectors[id] == usize::MAX, "duplicate section id");
        ensure!(
            u32_at(section, 0xff8) == SIGNATURE,
            "invalid sector signature"
        );
        ensure!(
            section_checksum(&section[..LENGTHS[id]]) == u16_at(section, 0xff6),
            "section {id} checksum mismatch"
        );
        let value = u32_at(section, 0xffc);
        ensure!(
            value as usize % 2 == slot,
            "save counter/slot parity mismatch"
        );
        ensure!(
            counter.is_none() || counter == Some(value),
            "mixed sector counters"
        );
        counter = Some(value);
        sectors[id] = offset;
    }
    Ok((sectors, counter.context("empty slot")?))
}

impl EmeraldSave {
    /// Require a full battery image and at least one complete valid slot. The
    /// other slot's failure is reported; no partial-sector reconstruction occurs.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() == SAVE_SIZE,
            "Emerald requires a raw 128 KiB battery save"
        );
        let first = validate_slot(bytes, 0);
        let second = validate_slot(bytes, 1);
        let mut warnings = Vec::new();
        let (sectors, counter) = match (first, second) {
            (Ok(a), Ok(b)) => {
                // Emerald handles the MAX -> 0 rollover specially; ordinary
                // counters use unsigned ordering, not a generic signed difference.
                if (a.1 == u32::MAX && b.1 == 0) || (!(a.1 == 0 && b.1 == u32::MAX) && b.1 > a.1) {
                    b
                } else {
                    a
                }
            }
            (Ok(a), Err(e)) => {
                warnings.push(format!("backup slot 1 unavailable: {e}"));
                a
            }
            (Err(e), Ok(b)) => {
                warnings.push(format!("backup slot 0 unavailable: {e}"));
                b
            }
            (Err(a), Err(b)) => bail!("no complete valid Emerald save slot: slot0={a}; slot1={b}"),
        };
        Ok(Self {
            bytes: bytes.to_vec(),
            sectors,
            counter,
            warnings,
        })
    }

    // Assemble one logical block, discarding footer bytes. PokemonStorage is
    // 0x83d0 bytes, with four alignment bytes before the 14*30 80-byte records.
    fn block(&self, first: usize, last: usize) -> Vec<u8> {
        (first..=last)
            .flat_map(|id| {
                self.bytes[self.sectors[id]..self.sectors[id] + LENGTHS[id]]
                    .iter()
                    .copied()
            })
            .collect()
    }

    /// Inspect party and every PC slot; corrupt occupied records fail explicitly.
    pub fn pokemon(&self) -> Result<Vec<Pokemon>> {
        let block1 = self.block(1, 4);
        let count = block1[0x234] as usize;
        ensure!(count <= 6, "party count exceeds six");
        let mut out = Vec::new();
        for slot in 0..count {
            let offset = 0x238 + slot * 100;
            out.push(
                decode_record(&block1[offset..offset + 100], Location::Party { slot })
                    .with_context(|| format!("party slot {slot}"))?,
            );
        }
        let storage = self.block(5, 13);
        for box_index in 0..14 {
            for slot in 0..30 {
                let offset = 4 + (box_index * 30 + slot) * 80;
                let raw = &storage[offset..offset + 80];
                if raw.iter().all(|b| *b == 0) {
                    continue;
                }
                out.push(
                    decode_record(raw, Location::Box { box_index, slot })
                        .with_context(|| format!("box {box_index}, slot {slot}"))?,
                );
            }
        }
        Ok(out)
    }

    /// Read exact PC bytes. Empty slots are all-zero; invalid indices are errors.
    pub fn box_record(&self, box_index: usize, slot: usize) -> Result<Vec<u8>> {
        ensure!(box_index < 14 && slot < 30, "PC box/slot out of range");
        let offset = 4 + (box_index * 30 + slot) * 80;
        Ok(self.block(5, 13)[offset..offset + 80].to_vec())
    }

    /// Replace a PC slot in a private image only. The transfer service checks
    /// vacancy/occupancy and definition compatibility before reaching this codec.
    /// No party-writing API exists: stat recalculation and party rules need work.
    pub fn replace_box(&mut self, box_index: usize, slot: usize, raw: &[u8]) -> Result<()> {
        ensure!(
            box_index < 14 && slot < 30 && raw.len() == 80,
            "invalid boxed replacement"
        );
        if raw.iter().any(|b| *b != 0) {
            decode_record(raw, Location::Box { box_index, slot })?;
        }
        let mut storage = self.block(5, 13);
        let offset = 4 + (box_index * 30 + slot) * 80;
        storage[offset..offset + 80].copy_from_slice(raw);
        let mut cursor = 0;
        for (id, &length) in LENGTHS.iter().enumerate().skip(5) {
            let start = self.sectors[id];
            self.bytes[start..start + length].copy_from_slice(&storage[cursor..cursor + length]);
            let checksum = section_checksum(&self.bytes[start..start + length]);
            self.bytes[start + 0xff6..start + 0xff8].copy_from_slice(&checksum.to_le_bytes());
            cursor += length;
        }
        // Reparse before handing any bytes to persistence. Counter/slot ordering
        // stays unchanged so the game continues selecting the same active slot.
        let checked = Self::parse(&self.bytes)?;
        ensure!(
            checked.counter == self.counter,
            "active slot unexpectedly changed"
        );
        Ok(())
    }

    /// Own the validated private flash image; the caller decides how to persist it.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

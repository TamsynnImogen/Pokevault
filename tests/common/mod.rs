//! Clearly labelled synthetic fixtures for the vanilla Emerald codec tests.
//! These bytes are handcrafted format exercises, not gameplay-derived saves.

use universal_pokemon_vault::gen3::section_checksum;

const SECTOR: usize = 4096;
const SIGNATURE: u32 = 0x0801_2025;
const LENGTHS: [usize; 14] = [
    0xf2c, 3968, 3968, 3968, 0xf08, 3968, 3968, 3968, 3968, 3968, 3968, 3968, 3968, 0x7d0,
];
const PERMUTATIONS: [[usize; 4]; 24] = [
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

/// Build one synthetic 80-byte encrypted boxed record with checksum-valid data.
pub fn boxed_record(pid: u32, species: u16) -> Vec<u8> {
    let ot = 0x1234_5678u32;
    let mut raw = vec![0u8; 80];
    raw[0..4].copy_from_slice(&pid.to_le_bytes());
    raw[4..8].copy_from_slice(&ot.to_le_bytes());
    // Synthetic digit names are arbitrary fixture labels; alphabet decoding has
    // separate exhaustive coverage in tests/gen3.rs.
    raw[8..13].copy_from_slice(&[0xa2, 0xa3, 0xa4, 0xa5, 0xff]);
    raw[18] = 2;
    raw[19] = 2; // Mark this non-egg record as having species data.
    raw[20..23].copy_from_slice(&[0xa2, 0xa3, 0xff]);

    let mut logical = [0u8; 48];
    let mut growth = [0u8; 12];
    growth[0..2].copy_from_slice(&species.to_le_bytes());
    growth[2..4].copy_from_slice(&25u16.to_le_bytes());
    growth[4..8].copy_from_slice(&1_000u32.to_le_bytes());
    growth[8] = 0b_10_01_00_11;
    growth[9] = 70;
    logical[0..12].copy_from_slice(&growth);
    let attacks = &mut logical[12..24];
    for (i, value) in [1u16, 2, 3, 4].into_iter().enumerate() {
        attacks[i * 2..i * 2 + 2].copy_from_slice(&value.to_le_bytes());
    }
    attacks[8..12].copy_from_slice(&[35, 30, 25, 20]);
    logical[24..30].copy_from_slice(&[1, 2, 3, 4, 5, 6]);
    logical[36..38].copy_from_slice(&((12u16) | (3 << 7) | (4 << 11)).to_le_bytes());
    logical[40..44].copy_from_slice(&0x0012_3456u32.to_le_bytes());
    logical[44..48].copy_from_slice(&0xa5a5_5a5au32.to_le_bytes());

    // Emerald permutes the four 12-byte blocks by PID modulo 24 before saving.
    let order = PERMUTATIONS[(pid % 24) as usize];
    let mut physical = [0u8; 48];
    for (kind, physical_index) in order.into_iter().enumerate() {
        physical[physical_index * 12..physical_index * 12 + 12]
            .copy_from_slice(&logical[kind * 12..kind * 12 + 12]);
    }
    let checksum = physical.chunks_exact(2).fold(0u16, |sum, word| {
        sum.wrapping_add(u16::from_le_bytes([word[0], word[1]]))
    });
    raw[28..30].copy_from_slice(&checksum.to_le_bytes());
    let key = pid ^ ot;
    for (dst, word) in raw[32..80]
        .chunks_exact_mut(4)
        .zip(physical.chunks_exact(4))
    {
        let encrypted = u32::from_le_bytes(word.try_into().unwrap()) ^ key;
        dst.copy_from_slice(&encrypted.to_le_bytes());
    }
    raw
}

/// Build a synthetic 128 KiB two-slot Emerald image with distinct counters.
pub fn fixture_save() -> Vec<u8> {
    let mut bytes = vec![0u8; 128 * 1024];
    let boxed = boxed_record(0x1020_3040, 25);
    let mut party = vec![0u8; 100];
    party[..80].copy_from_slice(&boxed);
    party[80..84].copy_from_slice(&0u32.to_le_bytes()); // Synthetic status.
    party[84] = 12;
    party[86..88].copy_from_slice(&35u16.to_le_bytes());
    party[88..90].copy_from_slice(&42u16.to_le_bytes());
    party[90..92].copy_from_slice(&37u16.to_le_bytes());
    party[92..94].copy_from_slice(&24u16.to_le_bytes());
    party[94..96].copy_from_slice(&17u16.to_le_bytes());
    party[96..98].copy_from_slice(&18u16.to_le_bytes());

    for slot in 0..2 {
        let physical = (0..14).map(|id| (id + 3) % 14).collect::<Vec<_>>();
        for (position, id) in physical.into_iter().enumerate() {
            let offset = (slot * 14 + position) * SECTOR;
            let length = LENGTHS[id];
            let data = &mut bytes[offset..offset + length];
            if id == 0 {
                data[0x10..0x12].copy_from_slice(&2u16.to_le_bytes()); // Synthetic box flag.
            } else if id == 1 {
                data[0x234] = 1; // One occupied synthetic party slot.
                data[0x238..0x238 + 100].copy_from_slice(&party);
            } else if id == 4 {
                // Recognizable synthetic source/Hall-of-Fame marker bytes.
                data[0x700..0x708].copy_from_slice(b"SYNTHHOF");
            } else if id == 5 {
                // PokemonStorage has four alignment bytes before its 14x30 records.
                data[4..84].copy_from_slice(&boxed);
            }
            let footer = offset + 0xff4;
            bytes[footer..footer + 2].copy_from_slice(&(id as u16).to_le_bytes());
            let checksum = section_checksum(&bytes[offset..offset + length]);
            bytes[offset + 0xff6..offset + 0xff8].copy_from_slice(&checksum.to_le_bytes());
            bytes[offset + 0xff8..offset + 0xffc].copy_from_slice(&SIGNATURE.to_le_bytes());
            let counter = if slot == 0 { 2u32 } else { 1u32 };
            bytes[offset + 0xffc..offset + 0x1000].copy_from_slice(&counter.to_le_bytes());
        }
    }
    bytes
}

/// Recompute a sector data checksum after deliberately corrupting its payload.
pub fn repair_sector_checksum(bytes: &mut [u8], sector_offset: usize, id: usize) {
    let checksum = section_checksum(&bytes[sector_offset..sector_offset + LENGTHS[id]]);
    bytes[sector_offset + 0xff6..sector_offset + 0xff8].copy_from_slice(&checksum.to_le_bytes());
}

/// Return a logical sector's physical offset for a fixture's rotated layout.
pub fn sector_offset(bytes: &[u8], slot: usize, wanted_id: usize) -> usize {
    (0..14)
        .map(|position| (slot * 14 + position) * SECTOR)
        .find(|&offset| {
            u16::from_le_bytes([bytes[offset + 0xff4], bytes[offset + 0xff5]]) as usize == wanted_id
        })
        .expect("fixture has each sector id once")
}

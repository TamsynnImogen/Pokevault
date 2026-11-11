mod common;

use common::{boxed_record, fixture_save, repair_sector_checksum, sector_offset};
use universal_pokemon_vault::gen3::{decode_record, decode_text, EmeraldSave, Location};

#[test]
fn decodes_the_full_uppercase_and_lowercase_glyph_ranges() {
    let uppercase = decode_text(&(0xbb..=0xd4).collect::<Vec<_>>());
    let lowercase = decode_text(&(0xd5..=0xee).collect::<Vec<_>>());
    assert_eq!(uppercase, "ABCDEFGHIJKLMNOPQRSTUVWXYZ");
    assert_eq!(lowercase, "abcdefghijklmnopqrstuvwxyz");
}

#[test]
fn first_save_with_erased_backup_slot_has_clear_diagnostic() {
    let mut bytes = fixture_save();
    // A genuine first in-game save writes counter 1 into slot 1. The other slot
    // is still erased flash; its 0xffff IDs are not evidence of corrupt gameplay.
    bytes[..14 * 4096].fill(0xff);
    let save = EmeraldSave::parse(&bytes).unwrap();
    assert_eq!(save.pokemon().unwrap().len(), 2);
    assert_eq!(save.warnings.len(), 1);
    assert!(save.warnings[0].contains("unused slot (erased flash)"));
}

#[test]
fn parses_party_and_pc_records_with_their_distinct_fields() {
    let save = EmeraldSave::parse(&fixture_save()).unwrap();
    let pokemon = save.pokemon().unwrap();
    assert_eq!(pokemon.len(), 2);
    assert_eq!(pokemon[0].location, Location::Party { slot: 0 });
    assert_eq!(pokemon[0].species_id, 25);
    assert_eq!(pokemon[0].party_level, Some(12));
    assert_eq!(pokemon[0].current_hp, Some(35));
    assert_eq!(pokemon[0].maximum_hp, Some(42));
    assert_eq!(pokemon[0].status, Some(0));
    assert_eq!(pokemon[0].moves, [1, 2, 3, 4]);
    assert_eq!(pokemon[0].evs, [1, 2, 3, 4, 5, 6]);
    assert_eq!(
        pokemon[1].location,
        Location::Box {
            box_index: 0,
            slot: 0
        }
    );
    assert_eq!(pokemon[1].species_id, 25);
    assert_eq!(pokemon[1].party_level, None);
    assert_eq!(pokemon[1].current_hp, None);
    assert_eq!(
        save.box_record(0, 0).unwrap(),
        boxed_record(0x1020_3040, 25)
    );
}

#[test]
fn decodes_all_twenty_four_pid_substructure_permutations() {
    for pid in 0..24u32 {
        let mon = decode_record(
            &boxed_record(pid, 100 + pid as u16),
            Location::Box {
                box_index: 2,
                slot: 3,
            },
        )
        .unwrap_or_else(|error| panic!("PID permutation {pid} failed: {error}"));
        assert_eq!(mon.personality, pid);
        assert_eq!(mon.species_id, 100 + pid as u16);
        assert_eq!(mon.moves, [1, 2, 3, 4]);
        assert_eq!(mon.evs, [1, 2, 3, 4, 5, 6]);
        assert_eq!(mon.friendship, 70);
        assert_eq!(mon.ability_slot, 0);
    }
}

#[test]
fn rejects_corrupt_pokemon_even_when_containing_sector_checksum_is_repaired() {
    let mut bytes = fixture_save();
    let sector = sector_offset(&bytes, 0, 5);
    bytes[sector + 4 + 32] ^= 1;
    repair_sector_checksum(&mut bytes, sector, 5);
    let save = EmeraldSave::parse(&bytes).unwrap();
    let error = save.pokemon().unwrap_err();
    assert!(format!("{error:#}").contains("Pokemon checksum mismatch"));
}

#[test]
fn rejects_save_when_both_sector_slots_are_invalid() {
    let mut bytes = fixture_save();
    let first = sector_offset(&bytes, 0, 0);
    let second = sector_offset(&bytes, 1, 0);
    bytes[first] ^= 1;
    bytes[second] ^= 1;
    assert!(EmeraldSave::parse(&bytes).is_err());
}

#[test]
fn section_ids_rotate_and_active_counter_selects_slot_zero() {
    let bytes = fixture_save();
    let save = EmeraldSave::parse(&bytes).unwrap();
    assert!(save
        .warnings
        .iter()
        .all(|warning| !warning.contains("unavailable")));
    assert_eq!(
        save.box_record(0, 0).unwrap(),
        boxed_record(0x1020_3040, 25)
    );
}

#[test]
fn rejects_invalid_ids_duplicates_and_mixed_counters() {
    let mut invalid_id = fixture_save();
    for slot in 0..2 {
        let footer = sector_offset(&invalid_id, slot, 0) + 0xff4;
        invalid_id[footer..footer + 2].copy_from_slice(&14u16.to_le_bytes());
    }
    assert!(EmeraldSave::parse(&invalid_id).is_err());

    let mut duplicate_id = fixture_save();
    for slot in 0..2 {
        let first = sector_offset(&duplicate_id, slot, 0) + 0xff4;
        let second = sector_offset(&duplicate_id, slot, 1) + 0xff4;
        let duplicate_value = duplicate_id[first..first + 2].to_vec();
        duplicate_id[second..second + 2].copy_from_slice(&duplicate_value);
    }
    assert!(EmeraldSave::parse(&duplicate_id).is_err());

    let mut mixed_counter = fixture_save();
    for slot in 0..2 {
        let counter = sector_offset(&mixed_counter, slot, 1) + 0xffc;
        let mixed_value = if slot == 0 { 4u32 } else { 3u32 };
        mixed_counter[counter..counter + 4].copy_from_slice(&mixed_value.to_le_bytes());
    }
    assert!(EmeraldSave::parse(&mixed_counter).is_err());
}

#[test]
fn box_write_crossing_section_boundary_preserves_source_hof_and_backup() {
    let original = fixture_save();
    let mut save = EmeraldSave::parse(&original).unwrap();
    let replacement = boxed_record(0xabcdef12, 133);
    // Box 1 slot 19 begins at storage byte 3924 and spans the 3968-byte cut.
    save.replace_box(1, 19, &replacement).unwrap();
    let written = save.into_bytes();
    let reparsed = EmeraldSave::parse(&written).unwrap();
    assert_eq!(reparsed.box_record(1, 19).unwrap(), replacement);

    for physical in 0..14 {
        let start = physical * 4096;
        let end = start + 4096;
        let id = u16::from_le_bytes([original[start + 0xff4], original[start + 0xff5]]);
        if id < 5 {
            assert_eq!(
                &written[start..end],
                &original[start..end],
                "slot 0 section {id} changed"
            );
        }
    }
    assert_eq!(
        &written[14 * 4096..],
        &original[14 * 4096..],
        "backup slot changed"
    );
}

#[test]
fn malformed_records_and_indices_return_errors_without_panicking() {
    assert!(EmeraldSave::parse(&fixture_save()[..128 * 1024 - 1]).is_err());

    let mut bad_egg = boxed_record(7, 25);
    bad_egg[19] |= 1;
    assert!(decode_record(
        &bad_egg,
        Location::Box {
            box_index: 0,
            slot: 0
        }
    )
    .unwrap_err()
    .to_string()
    .contains("Bad Egg"));
    assert!(decode_record(
        &bad_egg[..79],
        Location::Box {
            box_index: 0,
            slot: 0
        }
    )
    .is_err());

    let mut party_overflow = fixture_save();
    let sector = sector_offset(&party_overflow, 0, 1);
    party_overflow[sector + 0x234] = 7;
    repair_sector_checksum(&mut party_overflow, sector, 1);
    let save = EmeraldSave::parse(&party_overflow).unwrap();
    let error = save.pokemon().unwrap_err();
    assert!(format!("{error:#}").contains("party count exceeds six"));

    let mut save = EmeraldSave::parse(&fixture_save()).unwrap();
    assert!(save.box_record(14, 0).is_err());
    assert!(save.box_record(0, 30).is_err());
    assert!(save.replace_box(0, 0, &[0; 79]).is_err());
    assert!(save.replace_box(usize::MAX, 0, &[0; 80]).is_err());
}

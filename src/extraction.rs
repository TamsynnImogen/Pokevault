//! Partial table extraction for two exact, hash-pinned Emerald builds.
//!
//! Offsets/sizes were checked against its ELF symbols, not guessed from headers.
//! The matching retail and earlier modern builds have separate layouts. Neither
//! layout applies to pokeemerald-expansion. New builds need review, never probing
//! arbitrary pointers until plausible Pokémon happen to appear.

use crate::registry::{DefinitionBundle, EvolutionRule, MoveDefinition, SpeciesDefinition};
use anyhow::{ensure, Context, Result};
use serde_json::json;
use std::collections::BTreeMap;

/// Reviewed addresses are byte offsets in a specific ROM, not CPU bus addresses.
/// Entry sizes/counts are verified against that build's ELF symbol extents.
struct EmeraldLayout {
    definition_id: &'static str,
    provenance: &'static str,
    species_names: usize,
    species_info: usize,
    move_names: usize,
    battle_moves: usize,
    level_up: usize,
    evolutions: usize,
    tmhm: usize,
    egg_moves: usize,
}

/// Select layouts only by SHA-256; a convincing header cannot select this codec.
fn reviewed_layout(hash: &str) -> Result<EmeraldLayout> {
    match hash {
        crate::rom::EMERALD_RETAIL_SHA256 => Ok(EmeraldLayout {
            definition_id: "emerald-retail-en",
            provenance: "Matching retail Emerald; fresh pret/pokeemerald 731ad5bfd6e6f265508d0efcca0ba42f9dcf5881 + pret/agbcc da598c1d918402c42c0c0d7128ba14567f3175e9. make -j6 compare passed; SHA-1 f3ae088181bf583e55daf962a92bb46f4f1d07b7. ELF addresses/extents inspected 2026-11-10. Partial resources; see docs/FORMATS.md.",
            species_names:0x3185c8,species_info:0x3203cc,move_names:0x31977c,
            battle_moves:0x31c898,level_up:0x32937c,evolutions:0x32531c,
            tmhm:0x31e898,egg_moves:0x32add8,
        }),
        crate::transfer::LOCAL_MODERN_SHA256 => Ok(EmeraldLayout {
            definition_id:"emerald-modern-local",
            provenance:"Local pokeemerald_modern.gba; ELF symbol addresses and extents inspected 2026-11-10. Source checkout c2dcc629fd97b3d58fedc07c9e02653eabed9136; old build reproducibility and real-save acceptance not established. See docs/FORMATS.md.",
            species_names:0x63740c,species_info:0x77c8dc,move_names:0x6385c0,
            battle_moves:0x778d38,level_up:0x785a08,evolutions:0x7819a8,
            tmhm:0x77ada4,egg_moves:0x6397c8,
        }),
        _ => anyhow::bail!("no verified table layout for this ROM; a matching header is insufficient"),
    }
}

/// Extract partial, source-specific rules from an exact reviewed image.
/// Resources that have no validated decoder are explicitly listed as missing.
pub fn extract_reviewed_emerald(rom: &[u8]) -> Result<DefinitionBundle> {
    let hash = crate::sha256(rom);
    let layout = reviewed_layout(&hash)?;
    let namespace = format!("rom:{hash}");
    let mut bundle = DefinitionBundle {
        schema_version: 1,
        id: layout.definition_id.into(),
        version: "1".into(),
        rom_sha256: hash,
        engine: "emerald-vanilla-v1".into(),
        provenance: layout.provenance.into(),
        species: BTreeMap::new(),
        moves: BTreeMap::new(),
        evolutions: Vec::new(),
        extensions: BTreeMap::new(),
    };
    // gSpeciesNames 4532/11=412; gSpeciesInfo 11536/28=412.
    for id in 0..412u16 {
        let stats = range(rom, layout.species_info + id as usize * 28, 28)?;
        let name =
            crate::gen3::decode_text(range(rom, layout.species_names + id as usize * 11, 11)?);
        let pointer = read32(range(rom, layout.level_up + id as usize * 4, 4)?);
        let learnset = level_up_moves(rom, pointer)?;
        let tmhm = range(rom, layout.tmhm + id as usize * 8, 8)?;
        bundle.species.insert(id,SpeciesDefinition { namespace:namespace.clone(),name,
            canonical_id:None, data:json!({"base_stats":&stats[..6],"type_ids":&stats[6..8],
                "catch_rate":stats[8],"experience_yield":stats[9],"ev_yield_bits":read16(&stats[10..12]),
                "held_item_ids":[read16(&stats[12..14]),read16(&stats[14..16])],
                "gender_ratio":stats[16],"egg_cycles":stats[17],"base_friendship":stats[18],
                "growth_rate_id":stats[19],"egg_group_ids":&stats[20..22],"ability_ids":&stats[22..24],
                "level_up_moves":learnset,"tmhm_bitmap_hex":hex::encode(tmhm),"raw_info_hex":hex::encode(stats)}),
            missing:["ability_definitions","item_definitions","tmhm_move_order","tutor_moves","egg_moves_decoding",
                "front_sprite","back_sprite","animations","icons","palettes","cry","pokedex_text"]
                .into_iter().map(str::to_string).collect() });
    }
    // gMoveNames 4615/13=355; gBattleMoves 4260/12=355. Preserve all
    // bytes including padding; never apply this stride to a different compiler/build.
    for id in 0..355u16 {
        let mov = range(rom, layout.battle_moves + id as usize * 12, 12)?;
        let name = crate::gen3::decode_text(range(rom, layout.move_names + id as usize * 13, 13)?);
        bundle.moves.insert(
            id,
            MoveDefinition {
                namespace: namespace.clone(),
                name,
                canonical_id: None,
                aliases: Vec::new(),
                mechanics: json!({"effect_id":mov[0],"power":mov[1],"type_id":mov[2],
                "accuracy":mov[3],"pp":mov[4],"secondary_effect_chance":mov[5],"target_id":mov[6],
                "priority":mov[7] as i8,"flags_byte":mov[8],"raw_hex":hex::encode(mov),
                "effect_namespace":namespace}),
            },
        );
    }
    // gEvolutionTable has five 8-byte entries per species. Preserve unknown
    // methods/parameters as source-local requirements; branching is natural here.
    for from in 0..412u16 {
        for index in 0..5 {
            let evo = range(rom, layout.evolutions + (from as usize * 5 + index) * 8, 8)?;
            let method = read16(evo);
            if method == 0 {
                continue;
            }
            let to = read16(&evo[4..6]);
            ensure!(
                to != 0 && bundle.species.contains_key(&to),
                "invalid evolution target"
            );
            bundle.evolutions.push(EvolutionRule { from,to,method_namespace:namespace.clone(),
                requirements:json!({"method_id":method,"parameter":read16(&evo[2..4]),"raw_hex":hex::encode(evo)}) });
        }
    }
    // Retain undecoded egg data as source evidence without calling it a complete
    // breeding graph. No proprietary graphics/audio are bundled with the program.
    bundle.extensions.insert("vault:extraction".into(),json!({"completeness":"partial",
        "egg_moves_raw_hex":hex::encode(range(rom,layout.egg_moves,2278)?),
        "limitations":["No graphics/audio/Pokedex decoder","No expansion layout","No canonical cross-ROM mappings"]}));
    bundle.validate()?;
    Ok(bundle)
}

// Extents are checked even for a pinned image: the same discipline is required
// when future signed build manifests replace this single built-in layout.
fn range(bytes: &[u8], offset: usize, length: usize) -> Result<&[u8]> {
    bytes
        .get(
            offset
                ..offset
                    .checked_add(length)
                    .context("table offset overflow")?,
        )
        .context("table outside ROM")
}
fn read16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}
fn read32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

// Vanilla's packed learnset words use nine move bits and seven level bits. Follow
// only checked ROM pointers and require a terminator within a bounded 512 entries.
fn level_up_moves(rom: &[u8], pointer: u32) -> Result<Vec<serde_json::Value>> {
    let start = pointer
        .checked_sub(0x08000000)
        .context("learnset pointer outside cartridge")? as usize;
    let mut moves = Vec::new();
    for i in 0..512 {
        let word = read16(range(rom, start + i * 2, 2)?);
        if word == 0xffff {
            return Ok(moves);
        }
        ensure!(
            (word & 511) < 355,
            "learnset move ID outside reviewed table"
        );
        moves.push(json!({"move_id":word & 511,"level":word >> 9}));
    }
    anyhow::bail!("unterminated learnset")
}

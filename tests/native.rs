//! Opt-in REAL native-core regressions. These require a trusted mGBA library and
//! GNU ARM assembler tools; default cargo tests remain entirely self-contained.
#![allow(dead_code)]
mod common;

use std::{path::Path, process::Command, sync::Arc};
use universal_pokemon_vault::{
    emulator,
    overlay::{BytePatch, OverlayLayer, OverlayManifest, VirtualRom},
};

/// Assemble original homebrew code that fills a mode-3 screen using DMA3 from a
/// fixed ROM color. No Pokémon/Nintendo code, logo, artwork or BIOS is included.
fn synthetic_cartridge() -> (Vec<u8>, usize, usize) {
    let directory = tempfile::tempdir().unwrap();
    let assembly = directory.path().join("fixture.s");
    std::fs::write(
        &assembly,
        r#"
        .syntax unified
        .arm
        .section .text
        .global _start, color, display_instruction
_start: b boot
        .space 0xa0-(.-_start),0
        .ascii "VAULT TEST  "
        .ascii "VLT0"
        .ascii "00"
        .byte 0x96
        .space 0xc0-(.-_start),0
boot:   ldr r0, =0x04000000
display_instruction:
        ldr r1, =0x0403      @ Mode 3, BG2 enabled.
        strh r1, [r0]
        ldr r0, =0x040000d4  @ DMA3 source, destination, count/control.
        ldr r1, =color
        str r1, [r0]
        ldr r1, =0x06000000
        str r1, [r0,#4]
        ldr r1, =0x81009600  @ Enable; fixed source; 38400 halfwords.
        str r1, [r0,#8]
forever:b forever
color:  .hword 0x001f        @ GBA RGB555 red.
        .align 2
        .ltorg
        .space 0x200-(.-_start),0
"#,
    )
    .unwrap();
    let object = directory.path().join("fixture.o");
    let elf = directory.path().join("fixture.elf");
    let binary = directory.path().join("fixture.gba");
    run(Command::new("arm-none-eabi-as")
        .arg("-mcpu=arm7tdmi")
        .arg(&assembly)
        .arg("-o")
        .arg(&object));
    run(Command::new("arm-none-eabi-ld")
        .args(["-Ttext=0x08000000", "-e", "_start"])
        .arg(&object)
        .arg("-o")
        .arg(&elf));
    run(Command::new("arm-none-eabi-objcopy")
        .args(["-O", "binary"])
        .arg(&elf)
        .arg(&binary));
    let symbols = Command::new("arm-none-eabi-nm").arg(&elf).output().unwrap();
    assert!(symbols.status.success());
    let symbols = String::from_utf8(symbols.stdout).unwrap();
    let offset = |name: &str| {
        let line = symbols
            .lines()
            .find(|line| line.split_whitespace().last() == Some(name))
            .unwrap();
        usize::from_str_radix(line.split_whitespace().next().unwrap(), 16).unwrap() - 0x08000000
    };
    let mut bytes = std::fs::read(binary).unwrap();
    bytes[0xbd] = bytes[0xa0..=0xbc]
        .iter()
        .fold(0u8, |sum, b| sum.wrapping_sub(*b))
        .wrapping_sub(0x19);
    (bytes, offset("color"), offset("display_instruction"))
}

// Print compiler failures in one place instead of hiding a failed fixture build.
fn run(command: &mut Command) {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Native regression covers ROM data DMA, instruction fetch from patched bytes,
/// baseline repeatability, cleanup, and an optional actual Emerald managed save.
#[test]
#[ignore = "set UPV_CORE to trusted mGBA; optional UPV_ROM for local Emerald proof"]
fn mgba_executes_virtual_data_code_and_managed_save() {
    let core = std::env::var("UPV_CORE").expect("set UPV_CORE");
    let core = Path::new(&core);
    let (bytes, color, instruction) = synthetic_cartridge();
    let original_hash = universal_pokemon_vault::sha256(&bytes);
    let base: Arc<[u8]> = bytes.clone().into();
    let baseline = VirtualRom::new(base.clone(), OverlayManifest::identity(&base)).unwrap();
    let first = emulator::run_headless(core, &baseline, None, 10).unwrap();
    let repeated = emulator::run_headless(core, &baseline, None, 10).unwrap();
    assert_eq!(first.last_frame_sha256, repeated.last_frame_sha256);
    assert_eq!(
        (first.width, first.height, first.video_frames),
        (240, 160, 10)
    );
    let mut manifest = OverlayManifest::identity(&base);
    manifest.layers.push(OverlayLayer {
        id: "dma-blue".into(),
        version: 1,
        dependencies: Vec::new(),
        patches: vec![BytePatch {
            offset: color,
            expected: bytes[color..color + 2].to_vec(),
            replacement: 0x7c00u16.to_le_bytes().to_vec(),
        }],
    });
    let blue = emulator::run_headless(
        core,
        &VirtualRom::new(base.clone(), manifest).unwrap(),
        None,
        10,
    )
    .unwrap();
    assert_ne!(
        first.last_frame_sha256, blue.last_frame_sha256,
        "DMA must read overlay color"
    );
    let mut manifest = OverlayManifest::identity(&base);
    manifest.layers.push(OverlayLayer {
        id: "disable-background".into(),
        version: 1,
        dependencies: Vec::new(),
        patches: vec![BytePatch {
            offset: instruction,
            expected: bytes[instruction..instruction + 4].to_vec(),
            replacement: 0xe3a01000u32.to_le_bytes().to_vec(),
        }],
    });
    let disabled = emulator::run_headless(
        core,
        &VirtualRom::new(base.clone(), manifest).unwrap(),
        None,
        10,
    )
    .unwrap();
    assert_ne!(
        first.last_frame_sha256, disabled.last_frame_sha256,
        "CPU must fetch overlay instruction"
    );
    assert_eq!(universal_pokemon_vault::sha256(&base), original_hash);
    println!(
        "native homebrew frames: red={}, blue={}, code={}",
        first.last_frame_sha256, blue.last_frame_sha256, disabled.last_frame_sha256
    );

    // Actual ROM bytes are optional user-owned inputs and are never bundled.
    if let Ok(path) = std::env::var("UPV_ROM") {
        let rom = std::fs::read(&path).unwrap();
        let source_hash = universal_pokemon_vault::sha256(&rom);
        let definitions =
            universal_pokemon_vault::extraction::extract_reviewed_emerald(&rom).unwrap();
        let image = VirtualRom::new(rom.clone().into(), OverlayManifest::identity(&rom)).unwrap();
        let profile = universal_pokemon_vault::transfer::profile_for(&image, &definitions).unwrap();
        let mut store = universal_pokemon_vault::store::ManagedStore::open(":memory:").unwrap();
        let original = common::fixture_save();
        let id = universal_pokemon_vault::transfer::import_save(
            &mut store,
            &profile,
            &definitions,
            &original,
        )
        .unwrap();
        let result = universal_pokemon_vault::transfer::emulate_managed(
            &mut store,
            id,
            &definitions,
            core,
            &image,
            120,
        )
        .unwrap();
        assert_eq!(result.video_frames, 120);
        assert_eq!(store.save(id).unwrap().revision, 1);
        assert_eq!(
            store.original_save_hash(id).unwrap(),
            universal_pokemon_vault::sha256(&original)
        );
        store.integrity_check().unwrap();
        assert_eq!(
            universal_pokemon_vault::sha256(&std::fs::read(path).unwrap()),
            source_hash
        );
        println!(
            "native Emerald receipt: {}",
            serde_json::to_string(&result).unwrap()
        );
    }
}

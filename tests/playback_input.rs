//! Original homebrew tests the core's emulated joypad path. This sends Libretro
//! bits directly, never operating-system keyboard/mouse events or GUI input.
use std::{
    path::Path,
    process::Command,
    sync::{mpsc, Arc},
    time::Duration,
};
use universal_pokemon_vault::{
    emulator::{self, RuntimeControl},
    overlay::{OverlayManifest, VirtualRom},
};

fn run(command: &mut Command) {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Paint red while GBA A is released, green while held, using KEYINPUT and DMA3.
fn cartridge() -> Vec<u8> {
    let temp = tempfile::tempdir().unwrap();
    let asm = temp.path().join("input.s");
    fs_write(
        &asm,
        r#"
        .syntax unified
        .arm
        .section .text
        .global _start
_start: b boot
        .space 0xa0-(.-_start),0
        .ascii "VAULT INPUT "
        .ascii "VLT1"
        .ascii "00"
        .byte 0x96
        .space 0xc0-(.-_start),0
boot:   ldr r0, =0x04000000
        ldr r1, =0x0403
        strh r1, [r0]
loop:   ldr r0, =0x04000130
        ldrh r1, [r0]
        tst r1, #1
        ldreq r1, =0x03e0
        ldrne r1, =0x001f
        ldr r0, =0x02000000
        strh r1, [r0]
        ldr r0, =0x040000d4
        ldr r1, =0x02000000
        str r1, [r0]
        ldr r1, =0x06000000
        str r1, [r0,#4]
        ldr r1, =0x81009600
        str r1, [r0,#8]
        b loop
        .ltorg
        .space 0x200-(.-_start),0
"#,
    );
    let object = temp.path().join("input.o");
    let elf = temp.path().join("input.elf");
    let binary = temp.path().join("input.gba");
    run(Command::new("arm-none-eabi-as")
        .arg("-mcpu=arm7tdmi")
        .arg(&asm)
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
    let mut bytes = std::fs::read(binary).unwrap();
    bytes[0xbd] = bytes[0xa0..=0xbc]
        .iter()
        .fold(0u8, |sum, b| sum.wrapping_sub(*b))
        .wrapping_sub(0x19);
    bytes
}

fn fs_write(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
}

#[test]
#[ignore = "set UPV_CORE to trusted mGBA; requires GNU ARM assembler tools"]
fn emulated_a_press_and_release_change_real_native_pixels() {
    let core = std::env::var("UPV_CORE").expect("UPV_CORE");
    let bytes = cartridge();
    let image = VirtualRom::new(bytes.clone().into(), OverlayManifest::identity(&bytes)).unwrap();
    let control = Arc::new(RuntimeControl::default());
    let observed = Arc::new(std::sync::Mutex::new([false; 3]));
    let seen = observed.clone();
    let controls = control.clone();
    let worker_controls = control.clone();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        emulator::run_realtime(
            Path::new(&core),
            &image,
            &vec![0xff; 128 * 1024],
            worker_controls,
            ready_tx,
            Box::new(move |packet| {
                let sequence = u64::from_le_bytes(packet[20..28].try_into()?);
                let width = u32::from_le_bytes(packet[4..8].try_into()?) as usize;
                let height = u32::from_le_bytes(packet[8..12].try_into()?) as usize;
                let center = 28 + ((height / 2) * width + width / 2) * 4;
                let red = packet[center] > 200 && packet[center + 1] < 80;
                let green = packet[center + 1] > 200 && packet[center] < 80;
                let mut seen = seen.lock().unwrap();
                if sequence <= 20 {
                    seen[0] |= red;
                } else if sequence <= 40 {
                    seen[1] |= green;
                } else {
                    seen[2] |= red;
                }
                controls.update(
                    if (20..40).contains(&sequence) {
                        1 << 8
                    } else {
                        0
                    },
                    Some(sequence),
                    None,
                )?;
                if sequence >= 60 {
                    controls.stop();
                }
                Ok(())
            }),
        )
    });
    let metadata = ready_rx.recv_timeout(Duration::from_secs(5));
    if !matches!(metadata, Ok(Ok(_))) {
        control.stop();
        panic!("core startup failed: {metadata:?}");
    }
    let result = worker.join().unwrap().unwrap();
    assert_eq!(*observed.lock().unwrap(), [true, true, true]);
    assert_eq!(result.requested_frames, 60);
    assert_eq!(
        result.virtual_rom_sha256,
        universal_pokemon_vault::sha256(&bytes)
    );
}

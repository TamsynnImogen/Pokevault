use std::{
    path::Path,
    sync::{mpsc, Arc},
    time::Duration,
};

use anyhow::{ensure, Result};
use universal_pokemon_vault::{
    emulator::{self, RuntimeControl, GBA_BUTTONS},
    overlay::{OverlayManifest, VirtualRom},
};

#[test]
fn converts_all_libretro_pixel_formats() {
    for (format, words) in [
        (0, [0x7c00u32, 0x03e0, 0x001f]),
        (1, [0x00ff0000, 0x0000ff00, 0x000000ff]),
        (2, [0xf800, 0x07e0, 0x001f]),
    ] {
        let pixels: Vec<u8> = words
            .iter()
            .flat_map(|word| {
                if format == 1 {
                    word.to_ne_bytes().to_vec()
                } else {
                    (*word as u16).to_ne_bytes().to_vec()
                }
            })
            .collect();
        assert_eq!(
            emulator::rgba_pixels(format, &pixels).unwrap(),
            [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255],
            "format {format}"
        );
    }
    assert!(emulator::rgba_pixels(3, &[0, 0]).is_err());
    assert!(emulator::rgba_pixels(0, &[0]).is_err());
    assert!(emulator::rgba_pixels(1, &[0, 0]).is_err());
}

#[test]
fn packet_has_exact_binary_layout_and_rejects_bad_extents() {
    let pixels = 0x7c00u16.to_ne_bytes();
    let samples = [-32768i16, 32767, -1, 0];
    let packet =
        emulator::frame_packet(1, 1, 0, &pixels, 32768, &samples, 0x0102_0304_0506_0708).unwrap();
    assert_eq!(packet.len(), 28 + 4 + 8);
    assert_eq!(&packet[..4], b"PVF1");
    assert_eq!(u32::from_le_bytes(packet[4..8].try_into().unwrap()), 1);
    assert_eq!(u32::from_le_bytes(packet[8..12].try_into().unwrap()), 1);
    assert_eq!(
        u32::from_le_bytes(packet[12..16].try_into().unwrap()),
        32768
    );
    assert_eq!(u32::from_le_bytes(packet[16..20].try_into().unwrap()), 2);
    assert_eq!(
        u64::from_le_bytes(packet[20..28].try_into().unwrap()),
        0x0102_0304_0506_0708
    );
    assert_eq!(&packet[28..32], &[255, 0, 0, 255]);
    assert_eq!(&packet[32..], &[0, 128, 255, 127, 255, 255, 0, 0]);

    for (width, height) in [(0, 1), (1, 0), (1025, 1), (1, 1025)] {
        assert!(emulator::frame_packet(width, height, 0, &pixels, 32768, &[], 1).is_err());
    }
    assert!(emulator::frame_packet(2, 1, 0, &pixels, 32768, &[], 1).is_err());
    assert!(emulator::frame_packet(1, 1, 0, &[pixels[0]], 32768, &[], 1).is_err());
    assert!(emulator::frame_packet(1, 1, 0, &pixels, 7999, &[], 1).is_err());
    assert!(emulator::frame_packet(1, 1, 0, &pixels, 192001, &[], 1).is_err());
    assert!(emulator::frame_packet(1, 1, 0, &pixels, 32768, &[1], 1).is_err());
    assert!(emulator::frame_packet(1, 1, 0, &pixels, 32768, &vec![0; 16386], 1).is_err());
}

#[test]
fn controls_reject_invalid_input_and_acknowledgement() {
    let control = RuntimeControl::default();
    assert_eq!(control.frames(), 0);
    assert!(!control.is_paused());
    assert!(control.update(!GBA_BUTTONS, None, None).is_err());
    assert!(control.update(0, Some(1), None).is_err());
    control.update(0, Some(0), Some(true)).unwrap();
    assert!(control.is_paused());
    control.update(0, Some(0), Some(false)).unwrap();
    assert!(!control.is_paused());
    control.stop();
}

/// Uses only reviewed user-owned inputs; source files are read and checked again.
#[test]
#[ignore = "set UPV_CORE, UPV_ROM and UPV_SAVE to reviewed local inputs"]
fn native_realtime_stream_is_bounded_and_preserves_sources() {
    let core_path = std::env::var("UPV_CORE").expect("UPV_CORE");
    let rom_path = std::env::var("UPV_ROM").expect("UPV_ROM");
    let save_path = std::env::var("UPV_SAVE").expect("UPV_SAVE");
    let core_before = std::fs::read(&core_path).unwrap();
    let rom_before = std::fs::read(&rom_path).unwrap();
    let save_before = std::fs::read(&save_path).unwrap();
    let core_hash = universal_pokemon_vault::sha256(&core_before);
    let rom_hash = universal_pokemon_vault::sha256(&rom_before);
    let save_hash = universal_pokemon_vault::sha256(&save_before);
    let image = VirtualRom::new(
        rom_before.clone().into(),
        OverlayManifest::identity(&rom_before),
    )
    .unwrap();
    let control = Arc::new(RuntimeControl::default());
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let worker_control = control.clone();
    let worker_core_path = core_path.clone();
    let worker = std::thread::spawn(move || {
        let sink_control = worker_control.clone();
        let mut packets = 0u64;
        let sink = Box::new(move |packet: Vec<u8>| -> Result<()> {
            ensure!(
                packet.len() >= 28 && &packet[..4] == b"PVF1",
                "invalid PVF1 header"
            );
            let width = u32::from_le_bytes(packet[4..8].try_into()?);
            let height = u32::from_le_bytes(packet[8..12].try_into()?);
            let rate = u32::from_le_bytes(packet[12..16].try_into()?);
            let audio_frames = u32::from_le_bytes(packet[16..20].try_into()?);
            let sequence = u64::from_le_bytes(packet[20..28].try_into()?);
            ensure!(width == 240 && height == 160, "unexpected GBA geometry");
            ensure!((8000..=192000).contains(&rate), "invalid sample rate");
            let extent = 28usize + width as usize * height as usize * 4 + audio_frames as usize * 4;
            ensure!(packet.len() == extent, "invalid packet extent");
            packets += 1;
            ensure!(sequence == packets, "nonconsecutive packet sequence");
            sink_control.update(0, Some(sequence), None)?;
            if packets >= 120 {
                sink_control.stop();
            }
            Ok(())
        });
        emulator::run_realtime(
            Path::new(&worker_core_path),
            &image,
            &save_before,
            worker_control,
            ready_tx,
            sink,
        )
    });
    let metadata = match ready_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(metadata)) => metadata,
        other => {
            control.stop();
            panic!("native core did not become ready: {other:?}");
        }
    };
    assert!(metadata.core_name.to_lowercase().contains("mgba"));
    assert!(metadata.fps.is_finite() && (1.0..=240.0).contains(&metadata.fps));
    assert!(
        metadata.sample_rate.is_finite() && (8000.0..=192000.0).contains(&metadata.sample_rate)
    );
    assert_eq!(metadata.core_sha256, core_hash);
    let result = worker.join().unwrap().unwrap();
    assert!(result.requested_frames >= 120);
    assert!(result.video_frames > 0 && result.audio_frames > 0);
    assert_eq!((result.width, result.height), (240, 160));
    assert_eq!(result.virtual_rom_sha256, rom_hash);
    assert_eq!(result.core_sha256, core_hash);
    assert!(result.stop_reason.is_none(), "{:?}", result.stop_reason);
    assert_eq!(result.save, std::fs::read(&save_path).unwrap());
    assert_eq!(
        universal_pokemon_vault::sha256(&std::fs::read(&core_path).unwrap()),
        core_hash
    );
    assert_eq!(
        universal_pokemon_vault::sha256(&std::fs::read(&rom_path).unwrap()),
        rom_hash
    );
    assert_eq!(
        universal_pokemon_vault::sha256(&std::fs::read(&save_path).unwrap()),
        save_hash
    );
    // Closing immediately after native readiness is a normal user action, not
    // a failed video diagnostic. It must preserve SRAM without running a frame.
    let stop = Arc::new(RuntimeControl::default());
    stop.stop();
    let bytes = std::fs::read(&rom_path).unwrap();
    let image = VirtualRom::new(bytes.clone().into(), OverlayManifest::identity(&bytes)).unwrap();
    let save = std::fs::read(&save_path).unwrap();
    let (ready, _receiver) = mpsc::sync_channel(1);
    let stopped = emulator::run_realtime(
        Path::new(&core_path),
        &image,
        &save,
        stop,
        ready,
        Box::new(|_| panic!("a stopped session must not emit frames")),
    )
    .unwrap();
    assert_eq!(stopped.requested_frames, 0);
    assert_eq!(stopped.save, save);
}

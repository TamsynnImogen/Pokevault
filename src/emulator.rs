//! Real mGBA integration through the Libretro v1 C ABI.
//!
//! This module is the only native-code boundary. The caller explicitly chooses a
//! trusted native library: loading a core executes native code, unlike JSON game
//! definitions. We do not guess mGBA's build-dependent `mCore` layout. No original
//! ROM/save path, operating-system input device, savestate or engine hook is
//! exposed. Interactive input is only an emulated joypad bitmask supplied by UI.

use anyhow::{ensure, Context, Result};
use libloading::Library;
use serde::Serialize;
use std::{
    ffi::{c_char, c_void, CStr},
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering},
        mpsc::SyncSender,
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use crate::overlay::VirtualRom;

// Libretro cores use process-global state. Hold this guard through native cleanup;
// callbacks use a different lock so a frame cannot deadlock its own video callback.
static SESSION: Mutex<()> = Mutex::new(());
static CALLBACKS: Mutex<CallbackState> = Mutex::new(CallbackState {
    format: 0,
    frames: 0,
    audio_frames: 0,
    pixels: Vec::new(),
    width: 0,
    height: 0,
    buttons: 0,
    capture_audio: false,
    samples: Vec::new(),
});

struct CallbackState {
    format: u32,
    frames: usize,
    audio_frames: usize,
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    buttons: u16,
    capture_audio: bool,
    samples: Vec<i16>,
}

/// Stable Libretro joypad bits used by the GBA; no global keyboard/mouse APIs.
pub const GBA_BUTTONS: u16 = (1 << 0)
    | (1 << 2)
    | (1 << 3)
    | (1 << 4)
    | (1 << 5)
    | (1 << 6)
    | (1 << 7)
    | (1 << 8)
    | (1 << 10)
    | (1 << 11);

/// Atomics cross the native-owner thread boundary; the core/library never do.
#[derive(Default)]
pub struct RuntimeControl {
    stop: AtomicBool,
    paused: AtomicBool,
    buttons: AtomicU16,
    acknowledged: AtomicU64,
    sent: AtomicU64,
    frames: AtomicU64,
}

impl RuntimeControl {
    pub fn update(&self, buttons: u16, ack: Option<u64>, paused: Option<bool>) -> Result<()> {
        ensure!(
            buttons & !GBA_BUTTONS == 0,
            "unsupported emulated joypad bits"
        );
        if let Some(ack) = ack {
            ensure!(
                ack <= self.sent.load(Ordering::Acquire),
                "frame acknowledgement exceeds sent sequence"
            );
            self.acknowledged.fetch_max(ack, Ordering::AcqRel);
        }
        if let Some(paused) = paused {
            self.paused.store(paused, Ordering::Release);
        }
        self.buttons.store(
            if self.is_paused() { 0 } else { buttons },
            Ordering::Release,
        );
        Ok(())
    }
    pub fn stop(&self) {
        self.buttons.store(0, Ordering::Release);
        self.stop.store(true, Ordering::Release);
    }
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Acquire)
    }
    pub fn frames(&self) -> u64 {
        self.frames.load(Ordering::Acquire)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct CoreMetadata {
    pub core_name: String,
    pub core_version: String,
    pub core_sha256: String,
    pub fps: f64,
    pub sample_rate: f64,
}

pub type FrameSink = Box<dyn FnMut(Vec<u8>) -> Result<()> + Send>;

enum RunMode {
    Headless(usize),
    Interactive {
        control: Arc<RuntimeControl>,
        ready: SyncSender<std::result::Result<CoreMetadata, String>>,
        sink: FrameSink,
    },
}

// Exact geometry/timing layout from Libretro v1's public header, not mCore.
#[repr(C)]
#[derive(Default)]
struct Geometry {
    base_width: u32,
    base_height: u32,
    max_width: u32,
    max_height: u32,
    aspect_ratio: f32,
}
#[repr(C)]
#[derive(Default)]
struct Timing {
    fps: f64,
    sample_rate: f64,
}
#[repr(C)]
#[derive(Default)]
struct AvInfo {
    geometry: Geometry,
    timing: Timing,
}

// These C-compatible structures mirror only the stable Libretro public header.
#[repr(C)]
struct GameInfo {
    path: *const c_char,
    data: *const c_void,
    size: usize,
    meta: *const c_char,
}
#[repr(C)]
struct SystemInfo {
    library_name: *const c_char,
    library_version: *const c_char,
    valid_extensions: *const c_char,
    need_fullpath: bool,
    block_extract: bool,
}
type Environment = unsafe extern "C" fn(u32, *mut c_void) -> bool;
type Video = unsafe extern "C" fn(*const c_void, u32, u32, usize);
type Audio = unsafe extern "C" fn(i16, i16);
type AudioBatch = unsafe extern "C" fn(*const i16, usize) -> usize;
type Poll = unsafe extern "C" fn();
type Input = unsafe extern "C" fn(u32, u32, u32, u32) -> i16;

/// Evidence returned after unloading the native game. Save bytes are a detached
/// copy; only the managed-save service can persist them after codec validation.
#[derive(Debug, Serialize)]
pub struct EmulationResult {
    pub core_name: String,
    pub core_version: String,
    pub core_sha256: String,
    pub virtual_rom_sha256: String,
    pub requested_frames: usize,
    pub video_frames: usize,
    pub audio_frames: usize,
    pub width: u32,
    pub height: u32,
    pub pixel_format: u32,
    pub last_frame_sha256: String,
    pub save_sha256: String,
    /// Binary save image is omitted from receipts and printed diagnostics.
    #[serde(skip)]
    pub save: Vec<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
}

// The ABI supplies valid pointers for each supported command. Unknown commands
// return false, including filesystem, camera, input, logging and engine hooks.
// No Rust panic is permitted to cross a C callback boundary.
unsafe extern "C" fn environment(command: u32, data: *mut c_void) -> bool {
    if data.is_null() {
        return false;
    }
    match command {
        3 => {
            // GET_CAN_DUPE: null video frames can reuse the preceding frame.
            unsafe {
                data.cast::<bool>().write(true);
            }
            true
        }
        10 => {
            // SET_PIXEL_FORMAT: accept the three software framebuffer formats.
            let format = unsafe { data.cast::<u32>().read() };
            if format > 2 {
                return false;
            }
            if let Ok(mut state) = CALLBACKS.lock() {
                state.format = format;
                true
            } else {
                false
            }
        }
        17 => {
            // GET_VARIABLE_UPDATE: no runtime core options change in this host.
            unsafe {
                data.cast::<bool>().write(false);
            }
            true
        }
        // SET_VARIABLES / input descriptors are declarations, not callbacks that
        // require a UI. Decline richer option APIs so the core uses its defaults.
        11 | 16 => true,
        _ => false,
    }
}

unsafe extern "C" fn video(data: *const c_void, width: u32, height: u32, pitch: usize) {
    if data.is_null() || data as usize == usize::MAX {
        return;
    }
    let Ok(mut state) = CALLBACKS.lock() else {
        return;
    };
    let bpp = if state.format == 1 { 4 } else { 2 };
    let row = width as usize * bpp;
    // Bound the copy and check pitch before constructing foreign slices. The
    // trusted core owns storage until this synchronous callback returns.
    if width == 0 || height == 0 || width > 1024 || height > 1024 || pitch < row || pitch > 16384 {
        return;
    }
    state.pixels.clear();
    for y in 0..height as usize {
        let line = unsafe { std::slice::from_raw_parts(data.cast::<u8>().add(y * pitch), row) };
        state.pixels.extend_from_slice(line);
    }
    state.width = width;
    state.height = height;
    state.frames += 1;
}

// Audio is consumed without opening an audio device; counts prove that the core
// generated samples. A future frontend can route these buffers to its audio sink.
unsafe extern "C" fn audio(left: i16, right: i16) {
    if let Ok(mut state) = CALLBACKS.lock() {
        state.audio_frames = state.audio_frames.saturating_add(1);
        if state.capture_audio && state.samples.len() < 16384 {
            state.samples.extend_from_slice(&[left, right]);
        }
    }
}
unsafe extern "C" fn audio_batch(data: *const i16, frames: usize) -> usize {
    if let Ok(mut state) = CALLBACKS.lock() {
        state.audio_frames = state.audio_frames.saturating_add(frames);
        if state.capture_audio && !data.is_null() && frames <= 8192 {
            let count = (frames * 2).min(16384usize.saturating_sub(state.samples.len()));
            // The public ABI guarantees interleaved signed stereo samples.
            state
                .samples
                .extend_from_slice(unsafe { std::slice::from_raw_parts(data, count) });
        }
    }
    frames
}
unsafe extern "C" fn poll() {}
unsafe extern "C" fn input(port: u32, device: u32, index: u32, id: u32) -> i16 {
    if port != 0 || device != 1 || index != 0 || id >= 16 {
        return 0;
    }
    CALLBACKS
        .lock()
        .map(|state| i16::from(state.buttons & (1 << id) != 0))
        .unwrap_or(0)
}

// Cleanup runs on every error path after init and before the library is unloaded.
struct CoreCleanup {
    deinit: unsafe extern "C" fn(),
    unload: unsafe extern "C" fn(),
    loaded: bool,
}
impl Drop for CoreCleanup {
    fn drop(&mut self) {
        unsafe {
            if self.loaded {
                (self.unload)();
            }
            (self.deinit)();
        }
    }
}

/// Execute a bounded headless session from private ROM/save bytes. Engine hooks
/// are never enabled. This is emulator integration evidence, not a playable UI.
pub fn run_headless(
    core_path: &Path,
    rom: &VirtualRom,
    save: Option<&[u8]>,
    frames: usize,
) -> Result<EmulationResult> {
    ensure!(
        (1..=36000).contains(&frames),
        "frames must be between 1 and 36000"
    );
    run_native(core_path, rom, save, RunMode::Headless(frames))
}

/// Runs only on its dedicated owner thread. A sink failure requests a clean
/// stop with detached save bytes, rather than throwing away gameplay progress.
pub fn run_realtime(
    core_path: &Path,
    rom: &VirtualRom,
    save: &[u8],
    control: Arc<RuntimeControl>,
    ready: SyncSender<std::result::Result<CoreMetadata, String>>,
    sink: FrameSink,
) -> Result<EmulationResult> {
    let failure = ready.clone();
    let result = run_native(
        core_path,
        rom,
        Some(save),
        RunMode::Interactive {
            control,
            ready,
            sink,
        },
    );
    if let Err(error) = &result {
        let _ = failure.try_send(Err(format!("{error:#}")));
    }
    result
}

fn run_native(
    core_path: &Path,
    rom: &VirtualRom,
    save: Option<&[u8]>,
    mut mode: RunMode,
) -> Result<EmulationResult> {
    let _session = SESSION
        .try_lock()
        .map_err(|_| anyhow::anyhow!("another native session owns the process"))?;
    let core_sha256 = crate::sha256(&std::fs::read(core_path)?);
    let image = rom.materialize();
    let image_hash = crate::sha256(&image);
    // SAFETY: the selected native library is a trusted executable dependency.
    // Symbols are copied as ABI function pointers, but Library remains alive until
    // after CoreCleanup and all callbacks/native-owned buffers are finished.
    unsafe {
        let library = Library::new(core_path).context("load selected mGBA Libretro core")?;
        let api = *library.get::<unsafe extern "C" fn() -> u32>(b"retro_api_version\0")?;
        ensure!(api() == 1, "unsupported Libretro ABI");
        let get_info =
            *library.get::<unsafe extern "C" fn(*mut SystemInfo)>(b"retro_get_system_info\0")?;
        let mut info = SystemInfo {
            library_name: std::ptr::null(),
            library_version: std::ptr::null(),
            valid_extensions: std::ptr::null(),
            need_fullpath: true,
            block_extract: false,
        };
        get_info(&mut info);
        ensure!(
            !info.library_name.is_null() && !info.library_version.is_null(),
            "missing native core identity"
        );
        let core_name = CStr::from_ptr(info.library_name)
            .to_string_lossy()
            .into_owned();
        let core_version = CStr::from_ptr(info.library_version)
            .to_string_lossy()
            .into_owned();
        ensure!(
            core_name.to_lowercase().contains("mgba"),
            "only an explicitly selected mGBA core is supported"
        );
        ensure!(
            !info.need_fullpath,
            "core requires filesystem ROM loading; refused"
        );
        // Resolve all required symbols before initialization, so missing exports
        // cannot strand initialized native state on a partially constructed path.
        let set_env =
            *library.get::<unsafe extern "C" fn(Environment)>(b"retro_set_environment\0")?;
        let set_video =
            *library.get::<unsafe extern "C" fn(Video)>(b"retro_set_video_refresh\0")?;
        let set_audio = *library.get::<unsafe extern "C" fn(Audio)>(b"retro_set_audio_sample\0")?;
        let set_batch =
            *library.get::<unsafe extern "C" fn(AudioBatch)>(b"retro_set_audio_sample_batch\0")?;
        let set_poll = *library.get::<unsafe extern "C" fn(Poll)>(b"retro_set_input_poll\0")?;
        let set_input = *library.get::<unsafe extern "C" fn(Input)>(b"retro_set_input_state\0")?;
        let init = *library.get::<unsafe extern "C" fn()>(b"retro_init\0")?;
        let deinit = *library.get::<unsafe extern "C" fn()>(b"retro_deinit\0")?;
        let unload = *library.get::<unsafe extern "C" fn()>(b"retro_unload_game\0")?;
        let load =
            *library.get::<unsafe extern "C" fn(*const GameInfo) -> bool>(b"retro_load_game\0")?;
        let run = *library.get::<unsafe extern "C" fn()>(b"retro_run\0")?;
        let get_av =
            *library.get::<unsafe extern "C" fn(*mut AvInfo)>(b"retro_get_system_av_info\0")?;
        let controller = *library
            .get::<unsafe extern "C" fn(u32, u32)>(b"retro_set_controller_port_device\0")?;
        let memory =
            *library.get::<unsafe extern "C" fn(u32) -> *mut c_void>(b"retro_get_memory_data\0")?;
        let memory_size =
            *library.get::<unsafe extern "C" fn(u32) -> usize>(b"retro_get_memory_size\0")?;
        {
            let mut state = CALLBACKS
                .lock()
                .map_err(|_| anyhow::anyhow!("callback lock poisoned"))?;
            *state = CallbackState {
                format: 0,
                frames: 0,
                audio_frames: 0,
                pixels: Vec::new(),
                width: 0,
                height: 0,
                buttons: 0,
                capture_audio: matches!(mode, RunMode::Interactive { .. }),
                samples: Vec::new(),
            };
        }
        set_env(environment);
        set_video(video);
        set_audio(audio);
        set_batch(audio_batch);
        set_poll(poll);
        set_input(input);
        init();
        let mut cleanup = CoreCleanup {
            deinit,
            unload,
            loaded: false,
        };
        // No source path is supplied. mGBA identifies the in-memory cartridge.
        let game = GameInfo {
            path: std::ptr::null(),
            data: image.as_ptr().cast(),
            size: image.len(),
            meta: std::ptr::null(),
        };
        ensure!(load(&game), "mGBA rejected the virtual ROM");
        cleanup.loaded = true;
        controller(0, 1); // RETRO_DEVICE_JOYPAD, port zero.
        if let Some(bytes) = save {
            let size = memory_size(0); // RETRO_MEMORY_SAVE_RAM, before first frame.
            let ptr = memory(0).cast::<u8>();
            ensure!(
                size == bytes.len() && !ptr.is_null(),
                "core/save capacity mismatch: native={size}, save={}",
                bytes.len()
            );
            // mGBA defers loading this buffer until the first run; copy now,
            // never after a frame has already interpreted an empty save.
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        }
        let mut av = AvInfo::default();
        get_av(&mut av);
        ensure!(
            av.timing.fps.is_finite()
                && (1.0..=240.0).contains(&av.timing.fps)
                && av.timing.sample_rate.is_finite()
                && (8000.0..=192000.0).contains(&av.timing.sample_rate),
            "invalid native timing"
        );
        let metadata = CoreMetadata {
            core_name: core_name.clone(),
            core_version: core_version.clone(),
            core_sha256: core_sha256.clone(),
            fps: av.timing.fps,
            sample_rate: av.timing.sample_rate,
        };
        let mut requested_frames = 0;
        let mut stop_reason = None;
        match &mut mode {
            RunMode::Headless(frames) => {
                for _ in 0..*frames {
                    run();
                }
                requested_frames = *frames;
            }
            RunMode::Interactive {
                control,
                ready,
                sink,
            } => {
                ensure!(
                    ready.send(Ok(metadata)).is_ok(),
                    "emulator startup receiver disconnected"
                );
                let period = Duration::from_secs_f64(1.0 / av.timing.fps);
                let mut deadline = Instant::now();
                while !control.stop.load(Ordering::Acquire) {
                    if control.is_paused() {
                        std::thread::sleep(Duration::from_millis(5));
                        deadline = Instant::now();
                        continue;
                    }
                    if let Ok(mut state) = CALLBACKS.lock() {
                        state.buttons = control.buttons.load(Ordering::Acquire);
                    }
                    run();
                    requested_frames += 1;
                    control.frames.fetch_add(1, Ordering::AcqRel);
                    let mut state = CALLBACKS
                        .lock()
                        .map_err(|_| anyhow::anyhow!("callback lock poisoned"))?;
                    let samples = std::mem::take(&mut state.samples);
                    let sent = control.sent.load(Ordering::Acquire);
                    // Keep at most three binary packets in the WebView channel.
                    // Lagging renderers drop display/audio backlog; emulated
                    // hardware still executes every frame at its reported rate.
                    if !state.pixels.is_empty()
                        && sent.saturating_sub(control.acknowledged.load(Ordering::Acquire)) < 3
                    {
                        let sequence = sent.checked_add(1).context("frame sequence overflow")?;
                        let packet = frame_packet(
                            state.width,
                            state.height,
                            state.format,
                            &state.pixels,
                            av.timing.sample_rate.round() as u32,
                            &samples,
                            sequence,
                        )?;
                        control.sent.store(sequence, Ordering::Release);
                        drop(state);
                        if let Err(error) = sink(packet) {
                            stop_reason = Some(format!("display disconnected: {error:#}"));
                            break;
                        }
                    } else {
                        drop(state);
                    }
                    deadline += period;
                    let now = Instant::now();
                    if deadline > now {
                        std::thread::sleep(deadline - now);
                    } else if now.duration_since(deadline) > period * 4 {
                        deadline = now;
                    }
                }
            }
        }
        let size = memory_size(0);
        ensure!(size <= 1024 * 1024, "unexpected save RAM extent");
        let ptr = memory(0).cast::<u8>();
        ensure!(size == 0 || !ptr.is_null(), "native save pointer is null");
        let output_save = if size == 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(ptr, size).to_vec()
        };
        let state = CALLBACKS
            .lock()
            .map_err(|_| anyhow::anyhow!("callback lock poisoned"))?;
        // A user may close immediately after the ready handshake, before the
        // first frame. Preserve its unchanged SRAM normally; diagnostics still
        // require actual video output as part of their bounded execution proof.
        if matches!(mode, RunMode::Headless(_)) {
            ensure!(
                state.frames > 0 && !state.pixels.is_empty(),
                "core produced no software video frames"
            );
        }
        Ok(EmulationResult {
            core_name,
            core_version,
            core_sha256,
            virtual_rom_sha256: image_hash,
            requested_frames,
            video_frames: state.frames,
            audio_frames: state.audio_frames,
            width: state.width,
            height: state.height,
            pixel_format: state.format,
            last_frame_sha256: crate::sha256(&state.pixels),
            save_sha256: crate::sha256(&output_save),
            save: output_save,
            stop_reason,
        })
    }
}

/// Convert the three supported native-endian Libretro formats to explicit RGBA.
/// GBA's VRAM RGB555 layout is unrelated; conversion follows the frontend ABI.
pub fn rgba_pixels(format: u32, pixels: &[u8]) -> Result<Vec<u8>> {
    let bpp = if format == 1 { 4 } else { 2 };
    ensure!(
        format <= 2 && pixels.len().is_multiple_of(bpp),
        "invalid framebuffer format/length"
    );
    let mut rgba = Vec::with_capacity(pixels.len() / bpp * 4);
    for pixel in pixels.chunks_exact(bpp) {
        let (r, g, b) = if format == 1 {
            let p = u32::from_ne_bytes(pixel.try_into()?);
            (
                ((p >> 16) & 255) as u8,
                ((p >> 8) & 255) as u8,
                (p & 255) as u8,
            )
        } else {
            let p = u16::from_ne_bytes(pixel.try_into()?);
            let (r, g, b) = if format == 2 {
                (
                    ((p >> 11) & 31) * 255 / 31,
                    ((p >> 5) & 63) * 255 / 63,
                    (p & 31) * 255 / 31,
                )
            } else {
                (
                    ((p >> 10) & 31) * 255 / 31,
                    ((p >> 5) & 31) * 255 / 31,
                    (p & 31) * 255 / 31,
                )
            };
            (r as u8, g as u8, b as u8)
        };
        rgba.extend_from_slice(&[r, g, b, 255]);
    }
    Ok(rgba)
}

/// PVF1: 28-byte little-endian header, RGBA pixels, signed stereo PCM16.
pub fn frame_packet(
    width: u32,
    height: u32,
    format: u32,
    pixels: &[u8],
    sample_rate: u32,
    samples: &[i16],
    sequence: u64,
) -> Result<Vec<u8>> {
    ensure!(
        width > 0 && height > 0 && width <= 1024 && height <= 1024,
        "invalid frame geometry"
    );
    ensure!(
        (8000..=192000).contains(&sample_rate)
            && samples.len() <= 16384
            && samples.len().is_multiple_of(2),
        "invalid audio packet"
    );
    let rgba = rgba_pixels(format, pixels)?;
    ensure!(
        rgba.len() == width as usize * height as usize * 4,
        "framebuffer extent mismatch"
    );
    let mut out = Vec::with_capacity(28 + rgba.len() + samples.len() * 2);
    out.extend_from_slice(b"PVF1");
    for value in [width, height, sample_rate, (samples.len() / 2) as u32] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&sequence.to_le_bytes());
    out.extend_from_slice(&rgba);
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    Ok(out)
}

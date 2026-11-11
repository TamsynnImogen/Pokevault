//! Desktop session ownership. The native library stays on its worker thread;
//! SQLite and linked-file publication remain on the serialized service thread.
//! Pause never releases a lease. Stop joins/unloads, validates detached SRAM,
//! journals its publication, commits, then releases the exact session token.

use super::*;
use crate::{
    emulator::{self, CoreMetadata, EmulationResult, FrameSink, RuntimeControl},
    store::SessionLease,
};
use std::{
    sync::mpsc,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub(super) struct ActivePlayback {
    id: String,
    snapshot: ManagedSave,
    lease: SessionLease,
    definitions: Arc<DefinitionBundle>,
    control: Arc<RuntimeControl>,
    worker: Option<JoinHandle<Result<EmulationResult>>>,
    output: Option<EmulationResult>,
    metadata: Option<CoreMetadata>,
    error: Option<String>,
    directory: PathBuf,
}

impl Drop for ActivePlayback {
    fn drop(&mut self) {
        // Normal UI shutdown joins and persists first. Unexpected service drop
        // still stops its detached native loop; the durable lease fails closed.
        self.control.stop();
    }
}

impl ActivePlayback {
    fn native_stopped(&self) -> bool {
        self.worker
            .as_ref()
            .is_none_or(|worker| worker.is_finished())
    }

    /// Never detach a still-running native thread on an ordinary stop error.
    fn join(&mut self) -> Result<()> {
        self.control.stop();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !self.native_stopped() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        ensure!(
            self.native_stopped(),
            "native core has not stopped yet; its save remains leased. Retry Stop & save"
        );
        if let Some(worker) = self.worker.take() {
            self.output = Some(worker.join().map_err(|_| {
                anyhow::anyhow!(
                    "native owner thread panicked; original save and initial backup retained"
                )
            })??);
        }
        Ok(())
    }
}

impl DesktopService {
    /// Native start is a separate Tauri command because binary Channel handles
    /// are not trusted JSON game metadata. Tests supply a bounded in-process sink.
    pub fn start_emulator(
        &mut self,
        save_id: i64,
        core_path: &Path,
        sink: FrameSink,
    ) -> Result<Value> {
        ensure!(
            self.playback.is_none(),
            "a game session already exists; Stop & save it first"
        );
        self.links.recover(&self.store)?;
        let snapshot = self.store.save(save_id)?;
        self.links.preflight(&snapshot)?;
        let (rom, definitions) = self.load_rom(&snapshot.profile.rom_sha256)?;
        let definitions = definitions.context("no reviewed emulator save codec")?;
        ensure!(
            transfer::profile_for(&rom, &definitions)? == snapshot.profile,
            "runtime ROM/overlay/save profile mismatch"
        );
        let id = Uuid::new_v4().to_string();
        let parent = owned_directory(&self.root, "emulator-sessions")?;
        let directory = parent.join(&id);
        fs::create_dir(&directory)?;
        write_new(&directory.join("initial.sav"), &snapshot.bytes)?;
        write_new(
            &directory.join("profile.json"),
            &serde_json::to_vec_pretty(&snapshot.profile)?,
        )?;
        let lease = self.store.acquire_session(save_id, snapshot.revision)?;
        // The token is a local recovery authority, never part of the WebView DTO.
        if let Err(error) = write_new(
            &directory.join("lease.json"),
            &serde_json::to_vec_pretty(
                &json!({"save_id":save_id,"revision":snapshot.revision,"token":lease.token()}),
            )?,
        ) {
            self.store.release_session(&lease)?;
            return Err(error);
        }
        let control = Arc::new(RuntimeControl::default());
        let controls = control.clone();
        let core_path = core_path.to_path_buf();
        let save = snapshot.bytes.clone();
        let (ready_sender, ready) = mpsc::sync_channel(1);
        let worker = match thread::Builder::new()
            .name("vault-mgba".into())
            .spawn(move || {
                emulator::run_realtime(&core_path, &rom, &save, controls, ready_sender, sink)
            }) {
            Ok(worker) => worker,
            Err(error) => {
                self.store.release_session(&lease)?;
                return Err(error.into());
            }
        };
        self.playback = Some(ActivePlayback {
            id,
            snapshot,
            lease,
            definitions,
            control,
            worker: Some(worker),
            output: None,
            metadata: None,
            error: None,
            directory,
        });
        match ready.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(metadata)) => {
                self.playback.as_mut().unwrap().metadata = Some(metadata);
                Ok(self.emulator_status())
            }
            outcome => {
                let message = match outcome {
                    Ok(Err(message)) => message,
                    Err(error) => format!("native startup did not finish: {error}"),
                    _ => unreachable!(),
                };
                let active = self.playback.as_mut().unwrap();
                active.error = Some(message.clone());
                active.control.stop();
                let _ = active.join();
                // Before a successful ready handshake the core has run no game
                // frames. Release only after its cleanup has actually finished.
                if active.native_stopped() {
                    let active = self.playback.take().unwrap();
                    self.store.release_session(&active.lease)?;
                }
                anyhow::bail!("{message}")
            }
        }
    }

    pub(super) fn emulator_owns_save(&self, id: i64) -> bool {
        self.playback
            .as_ref()
            .is_some_and(|session| session.snapshot.id == id)
    }

    pub(super) fn emulator_status(&self) -> Value {
        let Some(active) = &self.playback else {
            return json!({"active":false,"session_id":null,"save_id":null,"state":"stopped","error":null});
        };
        let state = if active.error.is_some() && active.native_stopped() {
            "save_pending"
        } else if active.native_stopped() {
            "finished"
        } else if active.metadata.is_none() {
            "starting"
        } else if active.control.is_paused() {
            "paused"
        } else {
            "running"
        };
        json!({"active":true,"session_id":active.id,"save_id":active.snapshot.id,"state":state,
            "frames":active.control.frames(),"core_name":active.metadata.as_ref().map(|m|&m.core_name),
            "fps":active.metadata.as_ref().map(|m|m.fps),"sample_rate":active.metadata.as_ref().map(|m|m.sample_rate),
            "error":active.error})
    }

    pub(super) fn control_emulator(
        &mut self,
        id: &str,
        buttons: u16,
        ack: Option<u64>,
        paused: Option<bool>,
    ) -> Result<Value> {
        let active = self.playback.as_ref().context("no game session")?;
        ensure!(active.id == id, "game session changed; stale input refused");
        active.control.update(buttons, ack, paused)?;
        Ok(self.emulator_status())
    }

    pub(super) fn stop_emulator(&mut self, id: &str) -> Result<Value> {
        ensure!(
            self.playback.as_ref().is_some_and(|active| active.id == id),
            "game session changed or already stopped"
        );
        let mut active = self.playback.take().unwrap();
        let result = (|| -> Result<()> {
            active.join()?;
            let output = active
                .output
                .as_ref()
                .context("no detached native save; initial backup retained")?;
            // Preserve the detached image before parsing or any external write.
            // Invalid/partial SRAM can be investigated without replacing disk.
            let path = active.directory.join("stopped.sav");
            if !path.exists() {
                write_new(&path, &output.save)?;
            }
            ensure!(
                fs::read(&path)? == output.save,
                "stopped save recovery copy differs"
            );
            let parsed = EmeraldSave::parse(&output.save)?;
            for mon in parsed.pokemon()? {
                ensure!(
                    active.definitions.species.contains_key(&mon.species_id)
                        && mon
                            .moves
                            .iter()
                            .filter(|id| **id != 0)
                            .all(|id| active.definitions.moves.contains_key(id)),
                    "emulator save contains unresolved IDs"
                );
            }
            self.links.recover(&self.store)?;
            let current = self.store.save(active.snapshot.id)?;
            // A prior attempt may have committed SQL before a file conflict.
            // Recovery finishes that publication; never commit SRAM twice.
            if current.revision == active.snapshot.revision {
                self.links.preflight(&current)?;
                let links = self.links.clone();
                self.store
                    .set_save_writer(Some(Box::new(move |id, revision, before, after| {
                        links.publish(id, revision, before, after)
                    })));
                let commit =
                    self.store
                        .commit_session(&active.snapshot, &output.save, &active.lease);
                self.store.set_save_writer(None);
                self.links.recover(&self.store)?;
                commit?;
            } else {
                ensure!(
                    Some(current.revision) == active.snapshot.revision.checked_add(1)
                        && current.bytes == output.save,
                    "managed save changed outside its native session"
                );
            }
            self.store.release_session(&active.lease)?;
            Ok(())
        })();
        if let Err(error) = result {
            self.store.set_save_writer(None);
            active.error = Some(format!(
                "{error:#}. A recovery copy is retained at {}",
                active.directory.display()
            ));
            self.playback = Some(active);
            return Err(error);
        }
        Ok(self.emulator_status())
    }

    /// Explicit user escape from a stopped, unpublishable save. Never touches
    /// Vault records or overwrites the external conflict; keep raw SRAM on disk.
    pub(super) fn discard_emulator(&mut self, id: &str) -> Result<Value> {
        let active = self.playback.as_mut().context("no game session")?;
        ensure!(
            active.id == id && active.error.is_some(),
            "session has no pending stopped save"
        );
        ensure!(
            active.native_stopped(),
            "cannot release a running native session"
        );
        if let Some(output) = &active.output {
            let path = active.directory.join("stopped.sav");
            if !path.exists() {
                write_new(&path, &output.save)?;
            }
            ensure!(
                fs::read(path)? == output.save,
                "recovery copy verification failed"
            );
        }
        self.store.release_session(&active.lease)?;
        self.playback = None;
        Ok(self.emulator_status())
    }

    /// Called by the native close-request handler off the UI thread. An error
    /// keeps the window open so the user can retry or retain a recovery copy.
    pub fn shutdown_emulator(&mut self) -> Result<()> {
        if let Some(id) = self.playback.as_ref().map(|active| active.id.clone()) {
            self.stop_emulator(&id)?;
        }
        Ok(())
    }

    pub(super) fn default_emulator_core() -> Option<PathBuf> {
        [
            "/usr/lib64/libretro/mgba_libretro.so",
            "/usr/lib/x86_64-linux-gnu/libretro/mgba_libretro.so",
            "/usr/lib/libretro/mgba_libretro.so",
        ]
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
    }
}

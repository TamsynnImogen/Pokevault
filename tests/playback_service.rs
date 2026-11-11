//! Opt-in service lifecycle checks against a trusted core and real Emerald save.
//! All writes target temporary copies. No window, audio device or input device
//! is opened; the frame channel carries actual native output to this test.
use serde_json::{json, Value};
use std::{fs, path::Path, sync::mpsc, time::Duration};
use tempfile::{tempdir, TempDir};
use universal_pokemon_vault::desktop_service::DesktopService;

// Libretro owns process-global native state, including across independent test
// workspaces. Serialize these opt-in cases even with Cargo's default test pool.
static NATIVE_SERVICE_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Fixture {
    _temp: TempDir,
    service: DesktopService,
    core: String,
    file: std::path::PathBuf,
    source: String,
    rom: String,
    original: Vec<u8>,
    rom_bytes: Vec<u8>,
    id: i64,
    hash: Value,
    location: Value,
}

impl Fixture {
    fn new() -> Self {
        let core = std::env::var("UPV_CORE").expect("UPV_CORE");
        let source = std::env::var("UPV_SAVE").expect("UPV_SAVE");
        let rom = std::env::var("UPV_ROM").expect("UPV_ROM");
        let original = fs::read(&source).unwrap();
        let rom_bytes = fs::read(&rom).unwrap();
        let temp = tempdir().unwrap();
        let file = temp.path().join("play.srm");
        fs::write(&file, &original).unwrap();
        let mut service = DesktopService::open(temp.path().join("workspace")).unwrap();
        let hash = service
            .dispatch(json!({"action":"add_rom","path":rom}))
            .unwrap()["hash"]
            .clone();
        let id = service
            .dispatch(json!({"action":"choose_save","rom_hash":hash,"path":file}))
            .unwrap()["save_id"]
            .as_i64()
            .unwrap();
        let view = service
            .dispatch(json!({"action":"save_view","save_id":id}))
            .unwrap();
        let location = view["pokemon"]
            .as_array()
            .unwrap()
            .iter()
            .find(|mon| mon["location"].get("Box").is_some())
            .expect("PC record required")["location"]
            .clone();
        Self {
            _temp: temp,
            service,
            core,
            file,
            source,
            rom,
            original,
            rom_bytes,
            id,
            hash,
            location,
        }
    }
    fn start(&mut self) -> (String, mpsc::Receiver<Vec<u8>>) {
        let (sender, receiver) = mpsc::sync_channel(4);
        let status = self
            .service
            .start_emulator(
                self.id,
                Path::new(&self.core),
                Box::new(move |packet| sender.try_send(packet).map_err(Into::into)),
            )
            .unwrap();
        assert_eq!(status["active"], true);
        let packet = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        let seq = u64::from_le_bytes(packet[20..28].try_into().unwrap());
        let session = status["session_id"].as_str().unwrap().to_string();
        self.service
            .dispatch(
                json!({"action":"emulator_control","session_id":session,"buttons":0,"ack":seq}),
            )
            .unwrap();
        (session, receiver)
    }
    fn sources_unchanged(&self) {
        assert_eq!(fs::read(&self.source).unwrap(), self.original);
        assert_eq!(fs::read(&self.rom).unwrap(), self.rom_bytes);
    }
}

#[test]
#[ignore = "set UPV_CORE, UPV_ROM and UPV_SAVE to reviewed inputs"]
fn native_session_leases_pause_stop_save_and_transfers_form_one_safe_lifecycle() {
    let _serial = NATIVE_SERVICE_TEST.lock().unwrap();
    let mut f = Fixture::new();
    // A missing core must not strand a lease or prevent a subsequent valid start.
    assert!(f
        .service
        .start_emulator(
            f.id,
            Path::new("/missing/vault-core.so"),
            Box::new(|_| Ok(()))
        )
        .is_err());
    let (session, _frames) = f.start();
    assert_eq!(
        f.service
            .dispatch(json!({"action":"save_view","save_id":f.id}))
            .unwrap()["sync_status"],
        "emulating"
    );
    assert!(f.service.dispatch(json!({"action":"deposit","save_id":f.id,"expected_revision":0,"locations":[f.location],"move_from_managed":true})).is_err());
    assert!(f
        .service
        .dispatch(json!({"action":"choose_save","rom_hash":f.hash,"path":f.file}))
        .is_err());
    f.service
        .dispatch(
            json!({"action":"emulator_control","session_id":session,"buttons":0,"paused":true}),
        )
        .unwrap();
    assert_eq!(
        f.service
            .dispatch(json!({"action":"emulator_status"}))
            .unwrap()["state"],
        "paused"
    );
    assert!(f
        .service
        .dispatch(json!({"action":"undo","save_id":f.id,"expected_revision":0}))
        .is_err());
    assert_eq!(fs::read(&f.file).unwrap(), f.original);
    f.service.shutdown_emulator().unwrap();
    assert_eq!(
        f.service
            .dispatch(json!({"action":"emulator_status"}))
            .unwrap()["active"],
        false
    );
    assert!(f
        .service
        .dispatch(json!({"action":"emulator_control","session_id":session,"buttons":0}))
        .is_err());
    assert_eq!(fs::read(&f.file).unwrap(), f.original);
    let deposit=f.service.dispatch(json!({"action":"deposit","save_id":f.id,"expected_revision":1,"locations":[f.location],"move_from_managed":true})).unwrap();
    assert_eq!(deposit["revision"], 2);
    let record = f.service.dispatch(json!({"action":"vault_list"})).unwrap()[0].clone();
    let (second, _second_frames) = f.start();
    assert_ne!(session, second);
    f.service
        .dispatch(json!({"action":"emulator_stop","session_id":second}))
        .unwrap();
    let target = &f.location["Box"];
    f.service.dispatch(json!({"action":"withdraw","save_id":f.id,"expected_revision":3,"vault_id":record["vault_id"],"box_index":target["box_index"],"slot":target["slot"]})).unwrap();
    assert!(f
        .service
        .dispatch(json!({"action":"vault_list"}))
        .unwrap()
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(fs::read(&f.file).unwrap(), f.original);
    f.service
        .dispatch(json!({"action":"integrity_check"}))
        .unwrap();
    f.sources_unchanged();
}

#[test]
#[ignore = "set UPV_CORE, UPV_ROM and UPV_SAVE to reviewed inputs"]
fn external_save_conflict_can_be_retried_or_released_with_a_recovery_copy() {
    let _serial = NATIVE_SERVICE_TEST.lock().unwrap();
    let mut f = Fixture::new();
    let (session, _frames) = f.start();
    let mut external = f.original.clone();
    *external.last_mut().unwrap() ^= 1;
    fs::write(&f.file, &external).unwrap();
    assert!(f
        .service
        .dispatch(json!({"action":"emulator_stop","session_id":session}))
        .is_err());
    let pending = f
        .service
        .dispatch(json!({"action":"emulator_status"}))
        .unwrap();
    assert_eq!(pending["state"], "save_pending");
    assert_eq!(fs::read(&f.file).unwrap(), external);
    // Once the disk conflict is deliberately resolved, Stop retries the same
    // detached image; it does not rerun the core or create a second save commit.
    fs::write(&f.file, &f.original).unwrap();
    f.service
        .dispatch(json!({"action":"emulator_stop","session_id":session}))
        .unwrap();
    let (second, _second_frames) = f.start();
    fs::write(&f.file, &external).unwrap();
    assert!(f
        .service
        .dispatch(json!({"action":"emulator_stop","session_id":second}))
        .is_err());
    f.service
        .dispatch(json!({"action":"emulator_discard","session_id":second}))
        .unwrap();
    assert_eq!(fs::read(&f.file).unwrap(), external);
    let selected = f
        .service
        .dispatch(json!({"action":"choose_save","rom_hash":f.hash,"path":f.file}))
        .unwrap();
    assert_eq!(selected["save_id"], f.id);
    let workspace = f._temp.path().join("workspace/emulator-sessions");
    let backups = fs::read_dir(workspace)
        .unwrap()
        .map(|entry| entry.unwrap().path().join("stopped.sav"))
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 2);
    for backup in backups {
        assert_eq!(fs::read(backup).unwrap(), f.original);
    }
    f.sources_unchanged();
}

//! Opt-in end-to-end test using a reviewed ROM and a real save with a PC record.
//! Both external inputs remain read-only; only a temporary battery copy is linked.
use serde_json::json;
use std::{fs, path::Path};
use tempfile::tempdir;
use universal_pokemon_vault::{desktop_service::DesktopService, gen3::EmeraldSave};

#[test]
#[ignore = "set UPV_ROM and UPV_SAVE to reviewed Emerald inputs with an occupied PC slot"]
fn chosen_file_tracks_deposit_withdraw_refresh_undo_and_game_removal() {
    let rom_path = std::env::var("UPV_ROM").expect("UPV_ROM");
    let save_path = std::env::var("UPV_SAVE").expect("UPV_SAVE");
    let rom_before = fs::read(&rom_path).unwrap();
    let original = fs::read(&save_path).unwrap();
    let temp = tempdir().unwrap();
    let file = temp.path().join("play.srm");
    fs::write(&file, &original).unwrap();
    let workspace = temp.path().join("workspace");
    let mut service = DesktopService::open(&workspace).unwrap();
    let game = service
        .dispatch(json!({"action":"add_rom","path":rom_path}))
        .unwrap();
    let choose = json!({"action":"choose_save","rom_hash":game["hash"],"path":file});
    let selected = service.dispatch(choose.clone()).unwrap();
    let id = selected["save_id"].as_i64().unwrap();
    assert_eq!(service.dispatch(choose.clone()).unwrap()["save_id"], id);
    assert_eq!(
        service.dispatch(json!({"action":"overview"})).unwrap()["saves"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let view = service
        .dispatch(json!({"action":"save_view","save_id":id}))
        .unwrap();
    assert_eq!(view["sync_status"], "linked");
    let mon = view["pokemon"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["location"].get("Box").is_some())
        .expect("PC record");
    let location = mon["location"].clone();
    let box_index = location["Box"]["box_index"].as_u64().unwrap();
    let slot = location["Box"]["slot"].as_u64().unwrap();
    let raw_hash = mon["raw_sha256"].clone();

    // Deposit removes the selected record in the SAME external filename.
    service.dispatch(json!({"action":"deposit","save_id":id,"expected_revision":0,"locations":[location],"move_from_managed":true})).unwrap();
    assert_ne!(fs::read(&file).unwrap(), original);
    assert!(EmeraldSave::parse(&fs::read(&file).unwrap())
        .unwrap()
        .box_record(box_index as usize, slot as usize)
        .unwrap()
        .iter()
        .all(|b| *b == 0));
    let record = service.dispatch(json!({"action":"vault_list"})).unwrap()[0].clone();
    let uuid = record["uuid"].clone();
    assert_eq!(record["raw_sha256"], raw_hash);
    service.dispatch(json!({"action":"withdraw","save_id":id,"expected_revision":1,"vault_id":record["vault_id"],"box_index":box_index,"slot":slot})).unwrap();
    assert!(service
        .dispatch(json!({"action":"vault_list"}))
        .unwrap()
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(fs::read(&file).unwrap(), original);

    // A second real move reuses its receipt/UUID instead of failing as a replay.
    service.dispatch(json!({"action":"deposit","save_id":id,"expected_revision":2,"locations":[location],"move_from_managed":true})).unwrap();
    let record = service.dispatch(json!({"action":"vault_list"})).unwrap()[0].clone();
    assert_eq!(record["uuid"], uuid);
    service.dispatch(json!({"action":"withdraw","save_id":id,"expected_revision":3,"vault_id":record["vault_id"],"box_index":box_index,"slot":slot})).unwrap();

    // Simulate changed external progress in opaque trailing bytes. The parser
    // must still accept the entire image, and an old UI cannot overwrite it.
    let mut changed = original.clone();
    *changed.last_mut().unwrap() ^= 1;
    EmeraldSave::parse(&changed).unwrap().pokemon().unwrap();
    fs::write(&file, &changed).unwrap();
    assert!(service.dispatch(json!({"action":"deposit","save_id":id,"expected_revision":4,"locations":[location],"move_from_managed":true})).is_err());
    assert_eq!(fs::read(&file).unwrap(), changed);
    let reselected = service.dispatch(choose.clone()).unwrap();
    assert_eq!(reselected["save_id"], id);
    assert_eq!(reselected["revision"], 5);
    service.dispatch(json!({"action":"deposit","save_id":id,"expected_revision":5,"locations":[location],"move_from_managed":true})).unwrap();
    service
        .dispatch(json!({"action":"undo","save_id":id,"expected_revision":6}))
        .unwrap();
    assert_eq!(fs::read(&file).unwrap(), changed);
    assert!(service
        .dispatch(json!({"action":"vault_list"}))
        .unwrap()
        .as_array()
        .unwrap()
        .is_empty());

    service
        .dispatch(json!({"action":"remove_game","rom_hash":game["hash"]}))
        .unwrap();
    assert!(
        service.dispatch(json!({"action":"overview"})).unwrap()["games"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        service
            .dispatch(json!({"action":"save_view","save_id":id}))
            .unwrap()["sync_status"],
        "linked"
    );
    service
        .dispatch(json!({"action":"integrity_check"}))
        .unwrap();
    drop(service);
    let mut reopened = DesktopService::open(&workspace).unwrap();
    assert_eq!(
        reopened
            .dispatch(json!({"action":"save_view","save_id":id}))
            .unwrap()["sync_status"],
        "linked"
    );
    assert!(Path::new(&rom_path).is_file());
    assert_eq!(fs::read(rom_path).unwrap(), rom_before);
    assert_eq!(fs::read(save_path).unwrap(), original);
}

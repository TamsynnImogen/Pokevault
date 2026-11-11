use std::sync::Arc;

use universal_pokemon_vault::overlay::{
    BytePatch, LayerDependency, OverlayLayer, OverlayManifest, VirtualRom, MAX_GBA_ROM_SIZE,
};

fn layer(id: &str, version: u32, patches: Vec<BytePatch>) -> OverlayLayer {
    OverlayLayer {
        id: id.into(),
        version,
        dependencies: Vec::new(),
        patches,
    }
}

fn patch(offset: usize, expected: &[u8], replacement: &[u8]) -> BytePatch {
    BytePatch {
        offset,
        expected: expected.into(),
        replacement: replacement.into(),
    }
}

#[test]
fn reads_across_base_expansion_and_patch_boundaries_without_mutating_base() {
    let base: Arc<[u8]> = Arc::from(&b"abcd"[..]);
    let original = base.to_vec();
    let mut manifest = OverlayManifest::identity(&base);
    manifest.virtual_size = 8;
    manifest
        .layers
        .push(layer("one", 1, vec![patch(3, &[b'd', 0xff, 0xff], b"XYZ")]));
    let rom = VirtualRom::new(base.clone(), manifest.clone()).unwrap();

    assert_eq!(rom.read(1, 6).unwrap(), b"bcXYZ\xff");
    assert_eq!(rom.read(4, 0).unwrap(), b"");
    assert_eq!(rom.materialize(), b"abcXYZ\xff\xff");
    assert_eq!(base.as_ref(), original);
    assert_eq!(rom.base_hash(), manifest.base_sha256);
    assert_eq!(rom.manifest(), &manifest);
    assert_eq!(
        rom.manifest_hash(),
        universal_pokemon_vault::sha256(&serde_json::to_vec(&manifest).unwrap())
    );
}

#[test]
fn exact_earlier_dependency_and_unique_ids_are_required() {
    let base: Arc<[u8]> = Arc::from(&b"abc"[..]);
    let mut manifest = OverlayManifest::identity(&base);
    let mut second = layer("second", 2, vec![patch(1, b"b", b"B")]);
    second.dependencies.push(LayerDependency {
        id: "first".into(),
        version: 7,
    });
    manifest.layers = vec![
        layer("first", 7, vec![patch(0, b"a", b"A")]),
        second.clone(),
    ];
    assert_eq!(
        VirtualRom::new(base.clone(), manifest.clone())
            .unwrap()
            .materialize(),
        b"ABc"
    );

    manifest.layers.swap(0, 1);
    assert!(VirtualRom::new(base.clone(), manifest.clone()).is_err());
    manifest.layers.swap(0, 1);
    manifest.layers[1].dependencies[0].version = 8;
    assert!(VirtualRom::new(base.clone(), manifest.clone()).is_err());
    manifest.layers[1].dependencies[0].version = 7;
    manifest.layers[1].dependencies.push(LayerDependency {
        id: "first".into(),
        version: 7,
    });
    assert!(VirtualRom::new(base.clone(), manifest.clone()).is_err());
    manifest.layers[1] = layer("first", 8, vec![]);
    assert!(VirtualRom::new(base, manifest).is_err());
}

#[test]
fn overlap_and_expected_byte_conflicts_fail_closed() {
    let base: Arc<[u8]> = Arc::from(&b"abcdef"[..]);
    let mut manifest = OverlayManifest::identity(&base);
    manifest.layers = vec![
        layer("a", 1, vec![patch(1, b"bc", b"BC")]),
        layer("b", 1, vec![patch(2, b"C", b"x")]),
    ];
    assert!(VirtualRom::new(base.clone(), manifest.clone()).is_err());
    manifest.layers[1].patches[0] = patch(4, b"X", b"Y");
    assert!(VirtualRom::new(base.clone(), manifest.clone()).is_err());
    manifest.layers[1].patches[0] = patch(3, b"d", b"D");
    assert_eq!(
        VirtualRom::new(base, manifest).unwrap().materialize(),
        b"aBCDef"
    );
}

#[test]
fn bounds_lengths_hash_and_schema_are_validated() {
    let base: Arc<[u8]> = Arc::from(&b"ab"[..]);
    let mut manifest = OverlayManifest::identity(&base);
    manifest.virtual_size = 4;
    manifest
        .layers
        .push(layer("a", 1, vec![patch(usize::MAX, b"x", b"y")]));
    assert!(VirtualRom::new(base.clone(), manifest.clone()).is_err());
    manifest.layers[0].patches[0] = patch(4, b"x", b"y");
    assert!(VirtualRom::new(base.clone(), manifest.clone()).is_err());
    manifest.layers[0].patches[0] = patch(0, b"", b"");
    assert!(VirtualRom::new(base.clone(), manifest.clone()).is_err());
    manifest.layers[0].patches[0] = patch(0, b"a", b"XY");
    assert!(VirtualRom::new(base.clone(), manifest.clone()).is_err());
    manifest.layers.clear();
    let rom = VirtualRom::new(base.clone(), manifest.clone()).unwrap();
    assert!(rom.read(usize::MAX, 2).is_err());
    assert!(rom.read(4, 1).is_err());
    assert_eq!(rom.read(4, 0).unwrap(), b"");
    manifest.virtual_size = MAX_GBA_ROM_SIZE + 1;
    assert!(VirtualRom::new(base.clone(), manifest.clone()).is_err());
    manifest.virtual_size = 1;
    assert!(VirtualRom::new(base.clone(), manifest.clone()).is_err());
    manifest.virtual_size = 2;
    manifest.base_sha256 = "wrong".into();
    assert!(VirtualRom::new(base.clone(), manifest.clone()).is_err());
    manifest.base_sha256 = OverlayManifest::identity(&base).base_sha256;
    manifest.schema_version = 2;
    assert!(VirtualRom::new(base, manifest).is_err());
}

#[test]
fn json_round_trip_and_unknown_fields() {
    let mut manifest = OverlayManifest::identity(b"a");
    manifest
        .layers
        .push(layer("a", 1, vec![patch(0, b"a", b"b")]));
    let json = serde_json::to_string(&manifest).unwrap();
    assert_eq!(
        serde_json::from_str::<OverlayManifest>(&json).unwrap(),
        manifest
    );
    let typo = json.replace("replacement", "replacment");
    assert!(serde_json::from_str::<OverlayManifest>(&typo).is_err());
    let unknown = json.replace("\"schema_version\":1", "\"schema_version\":1,\"extra\":0");
    assert!(serde_json::from_str::<OverlayManifest>(&unknown).is_err());
}

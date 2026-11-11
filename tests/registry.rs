use std::collections::BTreeMap;

use serde_json::json;
use universal_pokemon_vault::registry::{
    match_move, DefinitionBundle, EvolutionRule, MatchConfidence, MoveDefinition, SpeciesDefinition,
};

fn species(name: &str) -> SpeciesDefinition {
    SpeciesDefinition {
        namespace: "source-a".into(),
        name: name.into(),
        canonical_id: None,
        data: json!({"types":["grass"],"sprites":{"front":"source://front"}}),
        missing: vec!["back_sprite".into()],
    }
}

fn mov(namespace: &str, name: &str, power: u8) -> MoveDefinition {
    MoveDefinition {
        namespace: namespace.into(),
        name: name.into(),
        canonical_id: None,
        aliases: Vec::new(),
        mechanics: json!({"power":power,"category":"physical"}),
    }
}

fn bundle() -> DefinitionBundle {
    DefinitionBundle {
        schema_version: 1,
        id: "source-a-definitions".into(),
        version: "1".into(),
        rom_sha256: "a".repeat(64),
        engine: "source-a-engine".into(),
        provenance: "verified external definitions".into(),
        species: BTreeMap::from([(1, species("Seed")), (2, species("Bloom"))]),
        moves: BTreeMap::from([(7, mov("source-a", "Vine Whip", 45))]),
        evolutions: vec![EvolutionRule {
            from: 1,
            to: 2,
            method_namespace: "source-a-level".into(),
            requirements: json!({"level":16}),
        }],
        extensions: BTreeMap::from([("source-a-extra".into(), json!({"opaque":true}))]),
    }
}

#[test]
fn validated_bundle_round_trips_and_has_stable_digest() {
    let bundle = bundle();
    bundle.validate().unwrap();
    let serialized = serde_json::to_string(&bundle).unwrap();
    let restored: DefinitionBundle = serde_json::from_str(&serialized).unwrap();
    assert_eq!(restored, bundle);
    assert_eq!(restored.digest(), bundle.digest());
    assert_eq!(
        bundle.digest(),
        universal_pokemon_vault::sha256(serialized.as_bytes())
    );
    assert_eq!(bundle.species[&1].missing, ["back_sprite"]);
}

#[test]
fn rejects_invalid_identity_hash_and_evolution_endpoints() {
    let mut value = bundle();
    value.schema_version = 2;
    assert!(value.validate().is_err());
    value = bundle();
    value.rom_sha256 = "ABC".into();
    assert!(value.validate().is_err());
    value = bundle();
    value.provenance = "  ".into();
    assert!(value.validate().is_err());
    value = bundle();
    value.species.get_mut(&1).unwrap().namespace.clear();
    assert!(value.validate().is_err());
    value = bundle();
    value.species.get_mut(&1).unwrap().missing.push(" ".into());
    assert!(value.validate().is_err());
    value = bundle();
    value.evolutions[0].to = 3;
    assert!(value.validate().is_err());
    value = bundle();
    value.evolutions[0].method_namespace.clear();
    assert!(value.validate().is_err());
}

#[test]
fn cycles_are_allowed_as_source_data_and_typos_are_rejected() {
    let mut value = bundle();
    value.evolutions.push(EvolutionRule {
        from: 2,
        to: 1,
        method_namespace: "custom".into(),
        requirements: json!(null),
    });
    value.validate().unwrap();
    let json = serde_json::to_string(&value).unwrap();
    assert!(
        serde_json::from_str::<DefinitionBundle>(&json.replace("provenance", "provennce")).is_err()
    );
    assert!(serde_json::from_str::<DefinitionBundle>(
        &json.replace("method_namespace", "method_namespce")
    )
    .is_err());
}

#[test]
fn move_match_requires_mechanics_and_proven_identity() {
    let source = mov("source-a", "Vine  Whip", 45);
    let same = mov("source-a", "vine whip", 45);
    assert_eq!(match_move(&source, &same), MatchConfidence::Exact);
    assert_eq!(
        match_move(&source, &mov("source-a", "Vine Whip", 60)),
        MatchConfidence::Different
    );
    assert_eq!(
        match_move(&source, &mov("source-b", "Vine Whip", 45)),
        MatchConfidence::Candidate
    );
    assert_eq!(
        match_move(&source, &mov("source-a", "Leaf Jab", 45)),
        MatchConfidence::Different
    );

    let mut alias = mov("source-a", "Leaf Jab", 45);
    alias.aliases.push("vine whip".into());
    assert_eq!(match_move(&source, &alias), MatchConfidence::Exact);
    alias.namespace = "source-b".into();
    assert_eq!(match_move(&source, &alias), MatchConfidence::Candidate);

    let mut canonical_source = source.clone();
    canonical_source.canonical_id = Some("move:verified-7".into());
    let mut canonical_dest = mov("source-b", "Different Label", 45);
    canonical_dest.canonical_id = canonical_source.canonical_id.clone();
    assert_eq!(
        match_move(&canonical_source, &canonical_dest),
        MatchConfidence::Exact
    );
    canonical_dest.canonical_id = Some("move:other".into());
    assert_eq!(
        match_move(&canonical_source, &canonical_dest),
        MatchConfidence::Different
    );
}

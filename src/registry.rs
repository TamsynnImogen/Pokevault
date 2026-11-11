//! Source-specific definition snapshots. These records describe externally verified
//! data; this module does not infer tables or mechanics from ROM bytes.

use std::collections::BTreeMap;

use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A versioned, source-ROM-bound definition set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefinitionBundle {
    /// Registry schema; only version 1 is supported.
    pub schema_version: u32,
    /// Nonempty stable identifier for this definition set.
    pub id: String,
    /// Nonempty version of this definition set.
    pub version: String,
    /// Lowercase SHA-256 of the exact source ROM to which these definitions apply.
    pub rom_sha256: String,
    /// Engine identifier supplied by the source-specific extractor or author.
    pub engine: String,
    /// Nonempty source or extraction provenance for review.
    pub provenance: String,
    /// Species keyed by their source-specific numeric IDs.
    pub species: BTreeMap<u16, SpeciesDefinition>,
    /// Moves keyed by their source-specific numeric IDs.
    pub moves: BTreeMap<u16, MoveDefinition>,
    /// Directed evolution rules; cycles may be valid in source content.
    pub evolutions: Vec<EvolutionRule>,
    /// Namespaced, source-specific additions with no assumed interpretation here.
    pub extensions: BTreeMap<String, Value>,
}

impl DefinitionBundle {
    /// Check the manifest's identity fields and referential integrity.
    ///
    /// An empty `missing` list means that no omissions were declared. It does
    /// not establish completeness of a particular engine's extraction.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1,
            "unsupported definition schema version"
        );
        ensure!(!self.id.trim().is_empty(), "definition id is empty");
        ensure!(
            !self.version.trim().is_empty(),
            "definition version is empty"
        );
        ensure!(!self.engine.trim().is_empty(), "engine id is empty");
        ensure!(!self.provenance.trim().is_empty(), "provenance is empty");
        ensure!(
            self.rom_sha256.len() == 64
                && self
                    .rom_sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "ROM SHA-256 must be 64 lowercase hex digits"
        );
        for (&id, species) in &self.species {
            validate_identity(
                &species.namespace,
                &species.name,
                species.canonical_id.as_deref(),
                "species",
                id,
            )?;
            ensure!(
                species.missing.iter().all(|part| !part.trim().is_empty()),
                "species {} has an empty missing-data label",
                id
            );
        }
        for (&id, mov) in &self.moves {
            validate_identity(
                &mov.namespace,
                &mov.name,
                mov.canonical_id.as_deref(),
                "move",
                id,
            )?;
            ensure!(
                mov.aliases.iter().all(|alias| !alias.trim().is_empty()),
                "move {} has an empty alias",
                id
            );
        }
        for rule in &self.evolutions {
            ensure!(
                self.species.contains_key(&rule.from),
                "evolution source {} is absent",
                rule.from
            );
            ensure!(
                self.species.contains_key(&rule.to),
                "evolution target {} is absent",
                rule.to
            );
            ensure!(
                !rule.method_namespace.trim().is_empty(),
                "evolution method namespace is empty"
            );
        }
        ensure!(
            self.extensions.keys().all(|key| !key.trim().is_empty()),
            "empty extension namespace"
        );
        Ok(())
    }

    /// SHA-256 of deterministic JSON serialization, including ordered maps and rules.
    pub fn digest(&self) -> String {
        crate::sha256(&serde_json::to_vec(self).expect("definition bundle is serializable"))
    }
}

fn validate_identity(
    namespace: &str,
    name: &str,
    canonical_id: Option<&str>,
    kind: &str,
    id: u16,
) -> Result<()> {
    ensure!(
        !namespace.trim().is_empty(),
        "{} {} namespace is empty",
        kind,
        id
    );
    ensure!(!name.trim().is_empty(), "{} {} name is empty", kind, id);
    ensure!(
        canonical_id.is_none_or(|value| !value.trim().is_empty()),
        "{} {} canonical id is empty",
        kind,
        id
    );
    Ok(())
}

/// Source-specific species identity and a structured, unmodified data snapshot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeciesDefinition {
    /// Namespace in which this species name and numeric key are defined.
    pub namespace: String,
    /// Name as reported by the external definition source.
    pub name: String,
    /// Optional externally verified cross-source identity.
    pub canonical_id: Option<String>,
    /// Raw structured species mechanics and resources from the definition source.
    pub data: Value,
    /// Explicitly identified missing fields or resources, if known.
    pub missing: Vec<String>,
}

/// Source-specific move identity and mechanics snapshot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoveDefinition {
    /// Namespace in which this move name and numeric key are defined.
    pub namespace: String,
    /// Primary source name.
    pub name: String,
    /// Optional externally verified cross-source identity.
    pub canonical_id: Option<String>,
    /// Explicit alternate names supplied by the definition source.
    pub aliases: Vec<String>,
    /// Structured move mechanics used for conservative matching.
    pub mechanics: Value,
}

/// One directed evolution edge in the source's own method namespace.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvolutionRule {
    /// Source species key.
    pub from: u16,
    /// Destination species key.
    pub to: u16,
    /// Namespace defining the meaning of `requirements`.
    pub method_namespace: String,
    /// Source-specific method details; no engine meaning is assumed here.
    pub requirements: Value,
}

/// Conservative confidence for matching move definitions across bundles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchConfidence {
    /// Verified canonical identity or same-namespace alias/name with equal mechanics.
    Exact,
    /// Equal mechanics and a possible name link, but no cross-namespace proof.
    Candidate,
    /// Conflicting mechanics or identity evidence.
    Different,
}

/// Compare move identity without treating a shared name as proof across engines.
pub fn match_move(source: &MoveDefinition, dest: &MoveDefinition) -> MatchConfidence {
    if source.mechanics != dest.mechanics {
        return MatchConfidence::Different;
    }
    let source_id = source
        .canonical_id
        .as_deref()
        .filter(|id| !id.trim().is_empty());
    let dest_id = dest
        .canonical_id
        .as_deref()
        .filter(|id| !id.trim().is_empty());
    if let (Some(a), Some(b)) = (source_id, dest_id) {
        return if a == b {
            MatchConfidence::Exact
        } else {
            MatchConfidence::Different
        };
    }

    let source_name = normalized(&source.name);
    let dest_name = normalized(&dest.name);
    let name_link = source_name == dest_name
        || source
            .aliases
            .iter()
            .any(|alias| normalized(alias) == dest_name)
        || dest
            .aliases
            .iter()
            .any(|alias| normalized(alias) == source_name);
    if !name_link {
        return MatchConfidence::Different;
    }
    if source.namespace == dest.namespace {
        MatchConfidence::Exact
    } else {
        MatchConfidence::Candidate
    }
}

// Whitespace and case normalization leaves punctuation intact to avoid merging
// distinct names through an overly broad spelling heuristic.
fn normalized(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

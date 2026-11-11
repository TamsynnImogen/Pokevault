//! Validated, memory-only ROM overlays. The source image is never modified.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};

/// Ordinary GBA ROM address space; larger images need a separately verified mapper.
pub const MAX_GBA_ROM_SIZE: usize = 32 * 1024 * 1024;

/// A persistent description of the virtual ROM and its ordered patch layers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverlayManifest {
    /// Format version. Only version 1 is accepted.
    pub schema_version: u32,
    /// SHA-256 of the exact, unmodified source ROM bytes.
    pub base_sha256: String,
    /// Output length, including any appended bytes initialized to `0xff`.
    pub virtual_size: usize,
    /// Ordered layers, applied from first to last.
    pub layers: Vec<OverlayLayer>,
}

impl OverlayManifest {
    /// Describe an unchanged virtual image of `base`.
    pub fn identity(base: &[u8]) -> Self {
        Self {
            schema_version: 1,
            base_sha256: crate::sha256(base),
            virtual_size: base.len(),
            layers: Vec::new(),
        }
    }
}

/// A named revision containing byte replacements and exact earlier-layer dependencies.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverlayLayer {
    /// Unique, nonempty layer identifier.
    pub id: String,
    /// Exact version expected by dependent layers.
    pub version: u32,
    /// Layers that must already have been applied at the listed versions.
    pub dependencies: Vec<LayerDependency>,
    /// Replacements applied in manifest order.
    pub patches: Vec<BytePatch>,
}

/// Exact dependency on an earlier layer in the same manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerDependency {
    /// Identifier of the required earlier layer.
    pub id: String,
    /// Required version of that layer.
    pub version: u32,
}

/// Equal-length replacement at a byte offset, guarded by the current virtual bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BytePatch {
    /// First byte to replace, relative to the start of the virtual image.
    pub offset: usize,
    /// Exact bytes required before this replacement is applied.
    pub expected: Vec<u8>,
    /// New bytes, with the same nonzero length as `expected`.
    pub replacement: Vec<u8>,
}

/// An immutable source ROM with validated, sparse in-memory replacements.
pub struct VirtualRom {
    base: Arc<[u8]>,
    manifest: OverlayManifest,
    // Nonoverlapping ranges are indexed by their start for efficient partial reads.
    replacements: BTreeMap<usize, Vec<u8>>,
}

impl VirtualRom {
    /// Validate the complete manifest and construct a read-only virtual view.
    pub fn new(base: Arc<[u8]>, manifest: OverlayManifest) -> Result<Self> {
        ensure!(
            manifest.schema_version == 1,
            "unsupported overlay schema version"
        );
        ensure!(
            base.len() <= MAX_GBA_ROM_SIZE,
            "base exceeds ordinary GBA ROM size"
        );
        ensure!(
            manifest.virtual_size >= base.len(),
            "virtual image truncates base"
        );
        ensure!(
            manifest.virtual_size <= MAX_GBA_ROM_SIZE,
            "virtual image exceeds ordinary GBA ROM size"
        );
        ensure!(
            manifest.base_sha256 == crate::sha256(&base),
            "base SHA-256 mismatch"
        );

        let mut rom = Self {
            base,
            manifest,
            replacements: BTreeMap::new(),
        };
        let mut seen = BTreeMap::<String, u32>::new();

        for layer in &rom.manifest.layers {
            ensure!(!layer.id.is_empty(), "layer id is empty");
            // An identifier denotes exactly one version in a manifest; repeating the
            // identifier would make dependency resolution ambiguous even if versions differ.
            ensure!(
                !seen.contains_key(&layer.id),
                "duplicate layer id: {}",
                layer.id
            );
            let mut required = BTreeSet::new();
            for dependency in &layer.dependencies {
                ensure!(!dependency.id.is_empty(), "dependency id is empty");
                ensure!(
                    required.insert(&dependency.id),
                    "duplicate dependency {} in {}",
                    dependency.id,
                    layer.id
                );
                ensure!(
                    seen.get(&dependency.id) == Some(&dependency.version),
                    "missing or wrong-version earlier dependency {}@{} for {}",
                    dependency.id,
                    dependency.version,
                    layer.id
                );
            }
            seen.insert(layer.id.clone(), layer.version);
        }

        // Validation is complete before the value escapes. Check expected bytes
        // against the view built by preceding patches, then add each replacement.
        for layer in &rom.manifest.layers {
            for patch in &layer.patches {
                ensure!(
                    !patch.expected.is_empty(),
                    "empty patch in layer {}",
                    layer.id
                );
                ensure!(
                    patch.expected.len() == patch.replacement.len(),
                    "unequal patch lengths in layer {}",
                    layer.id
                );
                let end = patch
                    .offset
                    .checked_add(patch.expected.len())
                    .context("patch offset overflows usize")?;
                ensure!(
                    end <= rom.manifest.virtual_size,
                    "patch outside virtual image"
                );

                // Adjacent ranges are fine. A predecessor and successor suffice
                // because all previously inserted ranges are nonoverlapping.
                if let Some((&start, bytes)) = rom.replacements.range(..=patch.offset).next_back() {
                    if start + bytes.len() > patch.offset {
                        bail!("overlapping patch in layer {}", layer.id);
                    }
                }
                if let Some((&start, _)) = rom.replacements.range(patch.offset..).next() {
                    if start < end {
                        bail!("overlapping patch in layer {}", layer.id);
                    }
                }
                ensure!(
                    rom.read(patch.offset, patch.expected.len())? == patch.expected,
                    "expected bytes mismatch in layer {} at {}",
                    layer.id,
                    patch.offset
                );
                rom.replacements
                    .insert(patch.offset, patch.replacement.clone());
            }
        }
        Ok(rom)
    }

    /// Read a checked range from the virtual image, including any patched bytes.
    pub fn read(&self, offset: usize, len: usize) -> Result<Vec<u8>> {
        let end = offset
            .checked_add(len)
            .context("read offset overflows usize")?;
        ensure!(
            end <= self.manifest.virtual_size,
            "read outside virtual image"
        );
        let mut result = vec![0xff; len];
        let base_end = end.min(self.base.len());
        if offset < base_end {
            result[..base_end - offset].copy_from_slice(&self.base[offset..base_end]);
        }
        if len == 0 {
            return Ok(result);
        }
        // Include a patch that begins before the request, then all starts inside it.
        let first = self
            .replacements
            .range(..=offset)
            .next_back()
            .map(|(&start, _)| start);
        if let Some(start) = first {
            self.copy_intersection(start, offset, end, &mut result);
        }
        for (&start, _) in self.replacements.range(offset.saturating_add(1)..end) {
            self.copy_intersection(start, offset, end, &mut result);
        }
        Ok(result)
    }

    /// Produce a private contiguous image for an emulator or hash calculation.
    pub fn materialize(&self) -> Vec<u8> {
        let mut image = self.base.to_vec();
        image.resize(self.manifest.virtual_size, 0xff);
        for (&start, bytes) in &self.replacements {
            image[start..start + bytes.len()].copy_from_slice(bytes);
        }
        image
    }

    /// Return the source ROM's verified SHA-256 identity.
    pub fn base_hash(&self) -> &str {
        &self.manifest.base_sha256
    }

    /// Hash the canonical field order of this validated, ordered JSON manifest.
    pub fn manifest_hash(&self) -> String {
        crate::sha256(
            &serde_json::to_vec(&self.manifest).expect("overlay manifest is serializable"),
        )
    }

    /// Borrow the exact manifest used to build this virtual image.
    pub fn manifest(&self) -> &OverlayManifest {
        &self.manifest
    }

    fn copy_intersection(&self, start: usize, offset: usize, end: usize, result: &mut [u8]) {
        let bytes = &self.replacements[&start];
        let from = start.max(offset);
        let to = (start + bytes.len()).min(end);
        if from < to {
            result[from - offset..to - offset].copy_from_slice(&bytes[from - start..to - start]);
        }
    }
}

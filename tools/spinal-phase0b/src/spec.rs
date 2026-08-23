use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::contract::{
    ANGLE_RADIANS_ABS, ANIMATION_DURATION_NS, ANIMATION_NAME, AXIS_ABS, COLOR_ABS, EVENT_FLOAT_ABS,
    EVENT_WINDOW_END_NS, EVENT_WINDOW_ID, EVENT_WINDOW_START_NS, POSITION_ABS, SAMPLE_COUNT,
    SAMPLE_SCHEDULE, TARGET_SPINE_VERSION, UNITLESS_ABS, UV_ABS,
};

const FORMAT_VERSION: u32 = 1;
const CASE_ID: &str = "generic-bevy-0.18.1";
const EVIDENCE_CLASS: &str = "non_representative_rehearsal";
const STATUS: &str = "not_run";
const BEVY_VERSION: &str = "0.18.1";
const SEMANTIC_SCHEMA: &str = "spinal-semantic-frame-v1";

const MAX_SPEC_BYTES: usize = 64 * 1024;
const MAX_RUNTIME_MANIFEST_BYTES: usize = 64 * 1024;
const MAX_PROVENANCE_BYTES: usize = 64 * 1024;
const MAX_SEMANTIC_REFERENCE_BYTES: usize = 1024 * 1024;
const MAX_EVENT_REFERENCE_BYTES: usize = 256 * 1024;
const MAX_PIXEL_REFERENCE_BYTES: usize = 4 * 1024 * 1024;
const MAX_REFERENCE_BYTES: usize = 32 * 1024 * 1024;
const REQUIRED_FEATURES: &[&str] = &[
    "bones",
    "slots",
    "attachment_only_skins",
    "region_attachments",
    "bone_rotate_timelines",
    "bone_translate_timelines",
    "slot_attachment_timelines",
    "slot_rgba_timelines",
    "draw_order_timelines",
    "event_timelines",
];

const SEMANTIC_FIELDS: &[&str] = &[
    "format_version",
    "default_skin",
    "skin_layers",
    "bones[].ordinal",
    "bones[].name",
    "bones[].local.translation",
    "bones[].local.rotation_radians",
    "bones[].local.scale",
    "bones[].local.shear_radians",
    "bones[].world.translation",
    "bones[].world.x_axis",
    "bones[].world.y_axis",
    "slots[].draw_order",
    "slots[].name",
    "slots[].attachment.skin",
    "slots[].attachment.slot",
    "slots[].attachment.placeholder",
    "slots[].attachment.name",
    "slots[].color_rgba",
    "draw_items[].kind",
    "draw_items[].slot",
    "draw_items[].attachment.skin",
    "draw_items[].attachment.slot",
    "draw_items[].attachment.placeholder",
    "draw_items[].attachment.name",
    "draw_items[].atlas_region.page",
    "draw_items[].atlas_region.region",
    "draw_items[].atlas_region.sequence_index",
    "draw_items[].blend_mode",
    "draw_items[].positions",
    "draw_items[].uvs",
    "draw_items[].triangles",
    "draw_items[].color_rgba",
    "ik_constraints[].name",
    "ik_constraints[].active",
    "ik_constraints[].preserved_underdetermined",
    "ik_constraints[].target_reach",
    "ik_constraints[].child_translation_y_zeroed",
    "ik_constraints[].issue",
    "transform_constraints[].name",
    "transform_constraints[].active",
    "transform_constraints[].issue",
    "active_diagnostics[].severity",
    "active_diagnostics[].code",
    "active_diagnostics[].scope.kind",
    "active_diagnostics[].scope.value",
    "active_diagnostics[].message",
];

const EVENT_FIELDS: &[&str] = &[
    "animation",
    "name",
    "local_time_ns",
    "loop_index",
    "integer",
    "float",
    "string",
    "volume",
    "balance",
    "diagnostic_codes[]",
];

/// A parsed and filesystem-verified Phase 0B rehearsal case.
#[derive(Debug)]
pub struct LoadedCase {
    manifest: CaseManifest,
    case_directory: PathBuf,
    source_bytes: Vec<u8>,
    source_sha256: String,
    authenticated_artifacts: BTreeMap<ArtifactKind, AuthenticatedArtifact>,
}

impl LoadedCase {
    /// Returns the strict manifest.
    #[must_use]
    pub const fn manifest(&self) -> &CaseManifest {
        &self.manifest
    }

    /// Returns the exact TOML bytes that were parsed.
    #[must_use]
    pub fn source_bytes(&self) -> &[u8] {
        &self.source_bytes
    }

    /// Returns the lowercase SHA-256 of the exact TOML bytes.
    #[must_use]
    pub fn source_sha256(&self) -> &str {
        &self.source_sha256
    }

    /// Returns the number of concrete artifact references verified from disk.
    #[must_use]
    pub fn verified_artifact_count(&self) -> usize {
        self.authenticated_artifacts.len()
    }

    /// Returns whether every required input slot had an authenticated artifact.
    ///
    /// This is input readiness only. This parser cannot claim that a rehearsal
    /// ran or that either Phase 0 gate passed.
    #[must_use]
    pub fn inputs_complete(&self) -> bool {
        self.manifest.inputs_complete()
            && self.authenticated_artifacts.len() == self.manifest.required_artifact_count()
    }

    /// Returns whether the authenticated artifacts needed for semantic execution exist.
    ///
    /// This deliberately covers only the provenance document, both runtime manifests,
    /// and both semantic references for each of the four fixed samples. Browser-pixel
    /// and event references remain mandatory for [`Self::inputs_complete`]. Semantic
    /// readiness is not evidence that a rehearsal ran or that any gate passed.
    #[must_use]
    pub fn semantic_inputs_complete(&self) -> bool {
        self.semantic_execution_plan().is_some()
    }

    /// Builds the immutable, authenticated input plan for semantic execution.
    ///
    /// Returns `None` until all eleven semantic-execution artifacts are present and
    /// authenticated. The plan intentionally excludes event and browser-pixel inputs;
    /// callers must still use [`Self::inputs_complete`] for full-case readiness.
    #[must_use]
    pub fn semantic_execution_plan(&self) -> Option<SemanticExecutionPlan<'_>> {
        let artifact = |kind| {
            self.authenticated_artifacts
                .get(&kind)
                .map(AuthenticatedArtifact::bytes)
        };
        let sample = |index: usize| {
            let specification = self.manifest.samples.get(index)?;
            Some(SemanticSampleInputs {
                id: &specification.id,
                animation: &specification.animation,
                time_ns: specification.time_ns,
                skin_layers: &specification.skin_layers,
                current_semantic: artifact(ArtifactKind::Sample {
                    index,
                    artifact: SampleArtifact::CurrentSemantic,
                })?,
                proposed_semantic: artifact(ArtifactKind::Sample {
                    index,
                    artifact: SampleArtifact::ProposedSemantic,
                })?,
            })
        };

        Some(SemanticExecutionPlan {
            provenance_document: artifact(ArtifactKind::ProvenanceDocument)?,
            current_runtime_manifest: artifact(ArtifactKind::CurrentRuntimeManifest)?,
            proposed_runtime_manifest: artifact(ArtifactKind::ProposedRuntimeManifest)?,
            samples: [sample(0)?, sample(1)?, sample(2)?, sample(3)?],
        })
    }

    pub(crate) fn runtime_manifest_inputs(&self) -> Option<RuntimeManifestInputs<'_>> {
        let current = self
            .authenticated_artifacts
            .get(&ArtifactKind::CurrentRuntimeManifest)?;
        let proposed = self
            .authenticated_artifacts
            .get(&ArtifactKind::ProposedRuntimeManifest)?;
        Some(RuntimeManifestInputs {
            current: RuntimeManifestInput::new(&self.case_directory, current),
            proposed: RuntimeManifestInput::new(&self.case_directory, proposed),
        })
    }

    pub(crate) fn authenticated_artifact_paths(&self) -> impl ExactSizeIterator<Item = &Path> {
        self.authenticated_artifacts
            .values()
            .map(|artifact| artifact.relative_path.as_path())
    }
}

#[derive(Debug)]
struct AuthenticatedArtifact {
    relative_path: PathBuf,
    bytes: Vec<u8>,
}

impl AuthenticatedArtifact {
    fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RuntimeManifestInputs<'a> {
    pub(crate) current: RuntimeManifestInput<'a>,
    pub(crate) proposed: RuntimeManifestInput<'a>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RuntimeManifestInput<'a> {
    case_directory: &'a Path,
    manifest_relative_path: &'a Path,
    manifest_bytes: &'a [u8],
}

impl<'a> RuntimeManifestInput<'a> {
    fn new(case_directory: &'a Path, artifact: &'a AuthenticatedArtifact) -> Self {
        Self {
            case_directory,
            manifest_relative_path: &artifact.relative_path,
            manifest_bytes: artifact.bytes(),
        }
    }

    pub(crate) const fn case_directory(self) -> &'a Path {
        self.case_directory
    }

    pub(crate) const fn manifest_relative_path(self) -> &'a Path {
        self.manifest_relative_path
    }

    pub(crate) const fn manifest_bytes(self) -> &'a [u8] {
        self.manifest_bytes
    }
}

/// Authenticated inputs for the fixed Phase 0B semantic rehearsal.
///
/// This borrowed plan cannot mutate the loaded case and exposes exactly the provenance
/// document, Current/Proposed runtime manifests, and eight semantic reference documents.
/// It does not include event references, browser pixels, execution results, or gate state.
#[derive(Debug)]
pub struct SemanticExecutionPlan<'a> {
    provenance_document: &'a [u8],
    current_runtime_manifest: &'a [u8],
    proposed_runtime_manifest: &'a [u8],
    samples: [SemanticSampleInputs<'a>; SAMPLE_COUNT],
}

impl<'a> SemanticExecutionPlan<'a> {
    /// Returns the authenticated project-owned provenance document bytes.
    #[must_use]
    pub const fn provenance_document(&self) -> &'a [u8] {
        self.provenance_document
    }

    /// Returns the authenticated Current runtime-manifest bytes.
    #[must_use]
    pub const fn current_runtime_manifest(&self) -> &'a [u8] {
        self.current_runtime_manifest
    }

    /// Returns the authenticated Proposed runtime-manifest bytes.
    #[must_use]
    pub const fn proposed_runtime_manifest(&self) -> &'a [u8] {
        self.proposed_runtime_manifest
    }

    /// Returns the four fixed semantic sample inputs in schedule order.
    #[must_use]
    pub const fn samples(&self) -> &[SemanticSampleInputs<'a>; SAMPLE_COUNT] {
        &self.samples
    }
}

/// Authenticated Current/Proposed references for one fixed semantic sample.
#[derive(Debug)]
pub struct SemanticSampleInputs<'a> {
    id: &'a str,
    animation: &'a str,
    time_ns: u64,
    skin_layers: &'a [String],
    current_semantic: &'a [u8],
    proposed_semantic: &'a [u8],
}

impl<'a> SemanticSampleInputs<'a> {
    /// Returns the fixed sample identifier.
    #[must_use]
    pub const fn id(&self) -> &'a str {
        self.id
    }

    /// Returns the fixed animation name.
    #[must_use]
    pub const fn animation(&self) -> &'a str {
        self.animation
    }

    /// Returns the fixed sample time in nanoseconds.
    #[must_use]
    pub const fn time_ns(&self) -> u64 {
        self.time_ns
    }

    /// Returns the fixed ordered skin-layer selection.
    #[must_use]
    pub fn skin_layers(&self) -> impl ExactSizeIterator<Item = &'a str> + '_ {
        self.skin_layers.iter().map(String::as_str)
    }

    /// Returns the authenticated Current semantic-reference bytes.
    #[must_use]
    pub const fn current_semantic(&self) -> &'a [u8] {
        self.current_semantic
    }

    /// Returns the authenticated Proposed semantic-reference bytes.
    #[must_use]
    pub const fn proposed_semantic(&self) -> &'a [u8] {
        self.proposed_semantic
    }
}

/// The fixed v1 generic-rehearsal specification.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseManifest {
    format_version: u32,
    case_id: String,
    evidence_class: String,
    gate_eligible: bool,
    status: String,
    target_spine_version: String,
    bevy_version: String,
    semantic_schema: String,
    required_features: Vec<String>,
    semantic_fields: Vec<String>,
    event_fields: Vec<String>,
    allowed_nonblocking_diagnostics: Vec<String>,
    reference_provenance: ReferenceProvenance,
    sources: Sources,
    hosts: Hosts,
    tolerances: Tolerances,
    animations: Vec<AnimationSpec>,
    samples: Vec<SampleSpec>,
    event_windows: Vec<EventWindowSpec>,
}

impl CaseManifest {
    /// Returns the stable generic case identifier.
    #[must_use]
    pub fn case_id(&self) -> &str {
        &self.case_id
    }

    /// Returns whether this case may ever satisfy the authoritative gate.
    ///
    /// Version 1 accepts only `false`.
    #[must_use]
    pub const fn gate_eligible(&self) -> bool {
        self.gate_eligible
    }

    /// Returns the declared run status. Version 1 accepts only `not_run`.
    #[must_use]
    pub fn status(&self) -> &str {
        &self.status
    }

    /// Returns the number of mandatory artifact slots in this exact schedule.
    #[must_use]
    pub fn required_artifact_count(&self) -> usize {
        self.artifact_slots().len()
    }

    /// Returns the number of mandatory slots that contain concrete references.
    #[must_use]
    pub fn provided_artifact_count(&self) -> usize {
        self.artifact_slots()
            .into_iter()
            .filter(|slot| slot.slot.provided())
            .count()
    }

    /// Returns whether every required evidence input is concretely referenced.
    #[must_use]
    pub fn inputs_complete(&self) -> bool {
        self.provided_artifact_count() == self.required_artifact_count()
    }

    fn validate(&self) -> Result<(), CaseError> {
        require_equal("format_version", self.format_version, FORMAT_VERSION)?;
        require_equal("case_id", self.case_id.as_str(), CASE_ID)?;
        require_equal(
            "evidence_class",
            self.evidence_class.as_str(),
            EVIDENCE_CLASS,
        )?;
        if self.gate_eligible {
            return invalid("gate_eligible must be false for a generic rehearsal");
        }
        require_equal("status", self.status.as_str(), STATUS)?;
        require_equal(
            "target_spine_version",
            self.target_spine_version.as_str(),
            TARGET_SPINE_VERSION,
        )?;
        require_equal("bevy_version", self.bevy_version.as_str(), BEVY_VERSION)?;
        require_equal(
            "semantic_schema",
            self.semantic_schema.as_str(),
            SEMANTIC_SCHEMA,
        )?;
        require_exact_list(
            "required_features",
            &self.required_features,
            REQUIRED_FEATURES,
        )?;
        require_exact_list("semantic_fields", &self.semantic_fields, SEMANTIC_FIELDS)?;
        require_exact_list("event_fields", &self.event_fields, EVENT_FIELDS)?;
        if !self.allowed_nonblocking_diagnostics.is_empty() {
            return invalid("allowed_nonblocking_diagnostics must be exactly empty");
        }

        self.reference_provenance.validate()?;
        self.hosts.validate()?;
        self.tolerances.validate()?;
        self.validate_schedule()?;
        self.validate_artifacts()
    }

    fn validate_schedule(&self) -> Result<(), CaseError> {
        let [animation] = self.animations.as_slice() else {
            return invalid("animations must contain exactly the fixed `sway` animation");
        };
        require_equal(
            "animations[0].name",
            animation.name.as_str(),
            ANIMATION_NAME,
        )?;
        require_equal(
            "animations[0].duration_ns",
            animation.duration_ns,
            ANIMATION_DURATION_NS,
        )?;

        if self.samples.len() != SAMPLE_COUNT {
            return invalid("samples must contain exactly the four fixed v1 samples");
        }
        for (index, (sample, expected)) in self.samples.iter().zip(SAMPLE_SCHEDULE).enumerate() {
            require_equal(
                &format!("samples[{index}].id"),
                sample.id.as_str(),
                expected.id(),
            )?;
            require_equal(
                &format!("samples[{index}].animation"),
                sample.animation.as_str(),
                ANIMATION_NAME,
            )?;
            require_equal(
                &format!("samples[{index}].time_ns"),
                sample.time_ns,
                expected.time_ns(),
            )?;
            if !sample
                .skin_layers
                .iter()
                .map(String::as_str)
                .eq(expected.skin_layers().iter().copied())
            {
                return invalid(format!(
                    "samples[{index}].skin_layers must equal the fixed v1 selection"
                ));
            }
        }

        let [window] = self.event_windows.as_slice() else {
            return invalid("event_windows must contain exactly the fixed v1 event window");
        };
        require_equal("event_windows[0].id", window.id.as_str(), EVENT_WINDOW_ID)?;
        require_equal(
            "event_windows[0].animation",
            window.animation.as_str(),
            ANIMATION_NAME,
        )?;
        require_equal(
            "event_windows[0].start_ns",
            window.start_ns,
            EVENT_WINDOW_START_NS,
        )?;
        require_equal(
            "event_windows[0].end_ns",
            window.end_ns,
            EVENT_WINDOW_END_NS,
        )?;
        Ok(())
    }

    fn validate_artifacts(&self) -> Result<(), CaseError> {
        let mut paths = BTreeSet::<PathBuf>::new();
        let mut total = 0_usize;
        for artifact in self.artifact_slots() {
            artifact
                .slot
                .validate(&artifact.label, artifact.max_bytes)?;
            let Some(reference) = artifact.slot.reference() else {
                continue;
            };
            if !paths.insert(reference.path.to_path_buf()) {
                return invalid(format!(
                    "artifact path `{}` is used by more than one evidence slot",
                    reference.path.display()
                ));
            }
            total = total
                .checked_add(reference.byte_length)
                .ok_or_else(|| CaseError::Invalid("artifact byte total overflowed".into()))?;
            if total > MAX_REFERENCE_BYTES {
                return invalid(format!(
                    "declared artifacts exceed the {MAX_REFERENCE_BYTES}-byte aggregate limit"
                ));
            }
        }
        Ok(())
    }

    fn artifact_slots(&self) -> Vec<ArtifactSlot<'_>> {
        let mut slots = vec![
            ArtifactSlot::new(
                ArtifactKind::ProvenanceDocument,
                "reference_provenance.method_document",
                &self.reference_provenance.method_document,
                MAX_PROVENANCE_BYTES,
            ),
            ArtifactSlot::new(
                ArtifactKind::CurrentRuntimeManifest,
                "sources.current.runtime_manifest",
                &self.sources.current.runtime_manifest,
                MAX_RUNTIME_MANIFEST_BYTES,
            ),
            ArtifactSlot::new(
                ArtifactKind::ProposedRuntimeManifest,
                "sources.proposed.runtime_manifest",
                &self.sources.proposed.runtime_manifest,
                MAX_RUNTIME_MANIFEST_BYTES,
            ),
        ];
        for (index, sample) in self.samples.iter().enumerate() {
            for (artifact, name, slot, max_bytes) in [
                (
                    SampleArtifact::CurrentSemantic,
                    "current_semantic",
                    &sample.current_semantic,
                    MAX_SEMANTIC_REFERENCE_BYTES,
                ),
                (
                    SampleArtifact::ProposedSemantic,
                    "proposed_semantic",
                    &sample.proposed_semantic,
                    MAX_SEMANTIC_REFERENCE_BYTES,
                ),
                (
                    SampleArtifact::CurrentBrowserPixels,
                    "current_browser_pixels",
                    &sample.current_browser_pixels,
                    MAX_PIXEL_REFERENCE_BYTES,
                ),
                (
                    SampleArtifact::ProposedBrowserPixels,
                    "proposed_browser_pixels",
                    &sample.proposed_browser_pixels,
                    MAX_PIXEL_REFERENCE_BYTES,
                ),
            ] {
                slots.push(ArtifactSlot::new(
                    ArtifactKind::Sample { index, artifact },
                    format!("samples.{}.{}", sample.id, name),
                    slot,
                    max_bytes,
                ));
            }
        }
        for (index, window) in self.event_windows.iter().enumerate() {
            slots.push(ArtifactSlot::new(
                ArtifactKind::EventWindow {
                    index,
                    artifact: EventArtifact::CurrentEvents,
                },
                format!("event_windows.{}.current_events", window.id),
                &window.current_events,
                MAX_EVENT_REFERENCE_BYTES,
            ));
            slots.push(ArtifactSlot::new(
                ArtifactKind::EventWindow {
                    index,
                    artifact: EventArtifact::ProposedEvents,
                },
                format!("event_windows.{}.proposed_events", window.id),
                &window.proposed_events,
                MAX_EVENT_REFERENCE_BYTES,
            ));
        }
        slots
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceProvenance {
    class: String,
    spinal_generated_expected_results_allowed: bool,
    method_document: EvidenceSlot,
}

impl ReferenceProvenance {
    fn validate(&self) -> Result<(), CaseError> {
        require_equal(
            "reference_provenance.class",
            self.class.as_str(),
            "project_owned_analytical",
        )?;
        if self.spinal_generated_expected_results_allowed {
            return invalid(
                "reference_provenance.spinal_generated_expected_results_allowed must be false",
            );
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sources {
    current: SourceSpec,
    proposed: SourceSpec,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceSpec {
    runtime_manifest: EvidenceSlot,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Hosts {
    native_semantic_required: bool,
    wasm_semantic_required: bool,
    native_framebuffer_required: bool,
    browser_appearance_required: bool,
    browser: BrowserProfile,
}

impl Hosts {
    fn validate(&self) -> Result<(), CaseError> {
        if !self.native_semantic_required
            || !self.wasm_semantic_required
            || self.native_framebuffer_required
            || !self.browser_appearance_required
        {
            return invalid(
                "hosts must require native/WASM semantics and browser appearance, with native framebuffer explicitly not required",
            );
        }
        require_equal(
            "hosts.browser.engine",
            self.browser.engine.as_str(),
            "chromium",
        )?;
        require_equal(
            "hosts.browser.graphics_api",
            self.browser.graphics_api.as_str(),
            "webgl2",
        )?;
        require_equal(
            "hosts.browser.angle_backend",
            self.browser.angle_backend.as_str(),
            "swiftshader",
        )?;
        if !self.browser.headless
            || self.browser.width_px != 640
            || self.browser.height_px != 480
            || self.browser.device_scale_factor != 1
        {
            return invalid("browser profile must be headless 640x480 at device scale factor 1");
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BrowserProfile {
    engine: String,
    graphics_api: String,
    angle_backend: String,
    headless: bool,
    width_px: u32,
    height_px: u32,
    device_scale_factor: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Tolerances {
    semantic: SemanticTolerances,
    browser_pixels: PixelTolerances,
}

impl Tolerances {
    fn validate(&self) -> Result<(), CaseError> {
        for (name, actual, expected) in [
            ("position_abs", self.semantic.position_abs, POSITION_ABS),
            ("axis_abs", self.semantic.axis_abs, AXIS_ABS),
            (
                "angle_radians_abs",
                self.semantic.angle_radians_abs,
                ANGLE_RADIANS_ABS,
            ),
            ("unitless_abs", self.semantic.unitless_abs, UNITLESS_ABS),
            ("uv_abs", self.semantic.uv_abs, UV_ABS),
            ("color_abs", self.semantic.color_abs, COLOR_ABS),
            (
                "event_float_abs",
                self.semantic.event_float_abs,
                EVENT_FLOAT_ABS,
            ),
        ] {
            if actual.to_bits() != expected.to_bits() {
                return invalid(format!(
                    "tolerances.semantic.{name} must be exactly {expected}"
                ));
            }
        }
        if self.browser_pixels.max_channel_delta != 8
            || self.browser_pixels.max_changed_pixel_fraction.to_bits() != 0.02_f64.to_bits()
            || self.browser_pixels.max_mean_channel_delta.to_bits() != 0.5_f64.to_bits()
        {
            return invalid(
                "browser pixel tolerances must be exactly delta=8, changed_fraction=0.02, mean_delta=0.5",
            );
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SemanticTolerances {
    position_abs: f64,
    axis_abs: f64,
    angle_radians_abs: f64,
    unitless_abs: f64,
    uv_abs: f64,
    color_abs: f64,
    event_float_abs: f64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PixelTolerances {
    max_channel_delta: u8,
    max_changed_pixel_fraction: f64,
    max_mean_channel_delta: f64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnimationSpec {
    name: String,
    duration_ns: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SampleSpec {
    id: String,
    animation: String,
    time_ns: u64,
    skin_layers: Vec<String>,
    current_semantic: EvidenceSlot,
    proposed_semantic: EvidenceSlot,
    current_browser_pixels: EvidenceSlot,
    proposed_browser_pixels: EvidenceSlot,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EventWindowSpec {
    id: String,
    animation: String,
    start_ns: u64,
    end_ns: u64,
    current_events: EvidenceSlot,
    proposed_events: EvidenceSlot,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceSlot {
    required: bool,
    path: Option<PathBuf>,
    byte_length: Option<u64>,
    sha256: Option<String>,
}

impl EvidenceSlot {
    fn validate(&self, label: &str, max_bytes: usize) -> Result<(), CaseError> {
        if !self.required {
            return invalid(format!("{label}.required must be true"));
        }
        match (&self.path, self.byte_length, &self.sha256) {
            (None, None, None) => Ok(()),
            (Some(path), Some(byte_length), Some(sha256)) => {
                validate_artifact_path(label, path)?;
                let byte_length = usize::try_from(byte_length).map_err(|_error| {
                    CaseError::Invalid(format!("{label}.byte_length does not fit this host"))
                })?;
                if byte_length == 0 || byte_length > max_bytes {
                    return invalid(format!("{label}.byte_length must be 1-{max_bytes}"));
                }
                if !is_sha256(sha256) {
                    return invalid(format!(
                        "{label}.sha256 must be 64 lowercase hexadecimal characters"
                    ));
                }
                Ok(())
            }
            _partial => invalid(format!(
                "{label} must omit path, byte_length, and sha256 together or provide all three"
            )),
        }
    }

    fn reference(&self) -> Option<ArtifactReference<'_>> {
        match (&self.path, self.byte_length, &self.sha256) {
            (Some(path), Some(byte_length), Some(sha256)) => Some(ArtifactReference {
                path,
                byte_length: usize::try_from(byte_length)
                    .expect("validated artifact lengths fit this host"),
                sha256,
            }),
            _missing => None,
        }
    }

    fn provided(&self) -> bool {
        self.path.is_some() && self.byte_length.is_some() && self.sha256.is_some()
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ArtifactKind {
    ProvenanceDocument,
    CurrentRuntimeManifest,
    ProposedRuntimeManifest,
    Sample {
        index: usize,
        artifact: SampleArtifact,
    },
    EventWindow {
        index: usize,
        artifact: EventArtifact,
    },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum SampleArtifact {
    CurrentSemantic,
    ProposedSemantic,
    CurrentBrowserPixels,
    ProposedBrowserPixels,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum EventArtifact {
    CurrentEvents,
    ProposedEvents,
}

struct ArtifactSlot<'a> {
    kind: ArtifactKind,
    label: String,
    slot: &'a EvidenceSlot,
    max_bytes: usize,
}

impl<'a> ArtifactSlot<'a> {
    fn new(
        kind: ArtifactKind,
        label: impl Into<String>,
        slot: &'a EvidenceSlot,
        max_bytes: usize,
    ) -> Self {
        Self {
            kind,
            label: label.into(),
            slot,
            max_bytes,
        }
    }
}

struct ArtifactReference<'a> {
    path: &'a Path,
    byte_length: usize,
    sha256: &'a str,
}

/// Errors produced while parsing or authenticating a rehearsal case.
#[derive(Debug, Error)]
pub enum CaseError {
    /// The case specification could not be read.
    #[error("failed to read Phase 0B case `{path}`: {source}")]
    Read {
        /// Case path.
        path: PathBuf,
        /// Filesystem error.
        #[source]
        source: std::io::Error,
    },
    /// TOML did not match the closed v1 schema.
    #[error("failed to parse Phase 0B case: {0}")]
    Parse(#[from] toml::de::Error),
    /// A parsed value violated fixed rehearsal policy.
    #[error("invalid Phase 0B case: {0}")]
    Invalid(String),
    /// A concrete artifact could not be read safely.
    #[error("failed to read artifact slot `{slot}` at `{path}`: {source}")]
    ArtifactRead {
        /// Stable evidence-slot label.
        slot: String,
        /// Declared artifact path.
        path: PathBuf,
        /// Filesystem error.
        #[source]
        source: std::io::Error,
    },
}

/// Parses and validates TOML without claiming that referenced files were read.
pub fn parse_case(text: &str) -> Result<CaseManifest, CaseError> {
    if text.is_empty() || text.len() > MAX_SPEC_BYTES {
        return invalid(format!(
            "specification must be 1-{MAX_SPEC_BYTES} UTF-8 bytes"
        ));
    }
    let manifest: CaseManifest = toml::from_str(text)?;
    manifest.validate()?;
    Ok(manifest)
}

/// Reads a case and authenticates every concrete artifact reference.
///
/// Missing required evidence slots are preserved as input incompleteness, not
/// misreported as a failed run. The checked-in generic case intentionally has
/// no concrete artifacts and therefore remains `not_run`. Relative case paths
/// are anchored to the process's absolute working directory before any read;
/// later working-directory changes cannot redirect runtime-bundle acquisition.
/// Anchoring does not canonicalize the path or relax any link checks.
pub fn load_case(path: impl AsRef<Path>) -> Result<LoadedCase, CaseError> {
    let requested_path = path.as_ref();
    let path = if requested_path.is_absolute() {
        requested_path.to_path_buf()
    } else {
        let current_directory = std::env::current_dir().map_err(|source| CaseError::Read {
            path: requested_path.to_path_buf(),
            source,
        })?;
        anchor_case_path(requested_path, &current_directory)?
    };
    let metadata = fs::symlink_metadata(&path).map_err(|source| CaseError::Read {
        path: path.clone(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return invalid("case specification must be a regular non-symlink file");
    }
    let declared_len = usize::try_from(metadata.len())
        .map_err(|_error| CaseError::Invalid("case size does not fit this host".into()))?;
    if declared_len == 0 || declared_len > MAX_SPEC_BYTES {
        return invalid(format!(
            "specification must be 1-{MAX_SPEC_BYTES} UTF-8 bytes"
        ));
    }
    let source_bytes = fs::read(&path).map_err(|source| CaseError::Read {
        path: path.clone(),
        source,
    })?;
    if source_bytes.len() != declared_len {
        return invalid("case specification changed while it was read");
    }
    let text = std::str::from_utf8(&source_bytes)
        .map_err(|error| CaseError::Invalid(format!("specification is not UTF-8: {error}")))?;
    let manifest = parse_case(text)?;
    let base = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .expect("an anchored absolute case path always has a parent");
    let mut authenticated_artifacts = BTreeMap::new();
    for artifact in manifest.artifact_slots() {
        if let Some(reference) = artifact.slot.reference() {
            let bytes = read_authenticated_artifact(base, &artifact.label, reference)?;
            if authenticated_artifacts
                .insert(artifact.kind, bytes)
                .is_some()
            {
                return invalid("internal artifact kinds must be unique");
            }
        }
    }
    Ok(LoadedCase {
        manifest,
        case_directory: base.to_path_buf(),
        source_sha256: sha256_hex(&source_bytes),
        source_bytes,
        authenticated_artifacts,
    })
}

fn anchor_case_path(path: &Path, current_directory: &Path) -> Result<PathBuf, CaseError> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    if !current_directory.is_absolute() {
        return invalid("the working-directory anchor for a relative case path must be absolute");
    }
    Ok(current_directory.join(path))
}

fn read_authenticated_artifact(
    base: &Path,
    slot: &str,
    reference: ArtifactReference<'_>,
) -> Result<AuthenticatedArtifact, CaseError> {
    let mut resolved = base.to_path_buf();
    let components = reference.path.components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(component) = component else {
            return invalid(format!("artifact slot `{slot}` has an unsafe path"));
        };
        resolved.push(component);
        let metadata =
            fs::symlink_metadata(&resolved).map_err(|source| CaseError::ArtifactRead {
                slot: slot.to_owned(),
                path: reference.path.to_path_buf(),
                source,
            })?;
        if metadata.file_type().is_symlink() {
            return invalid(format!("artifact slot `{slot}` resolves through a symlink"));
        }
        let final_component = index + 1 == components.len();
        if (!final_component && !metadata.is_dir()) || (final_component && !metadata.is_file()) {
            return invalid(format!(
                "artifact slot `{slot}` must resolve to a regular file beneath the case directory"
            ));
        }
    }

    let file = fs::File::open(&resolved).map_err(|source| CaseError::ArtifactRead {
        slot: slot.to_owned(),
        path: reference.path.to_path_buf(),
        source,
    })?;
    let metadata = file.metadata().map_err(|source| CaseError::ArtifactRead {
        slot: slot.to_owned(),
        path: reference.path.to_path_buf(),
        source,
    })?;
    if !metadata.is_file() {
        return invalid(format!(
            "artifact slot `{slot}` did not open as a regular file"
        ));
    }
    let actual_length = usize::try_from(metadata.len()).map_err(|_error| {
        CaseError::Invalid(format!(
            "artifact slot `{slot}` length does not fit this host"
        ))
    })?;
    if actual_length != reference.byte_length {
        return invalid(format!(
            "artifact slot `{slot}` has {} bytes; expected {}",
            actual_length, reference.byte_length
        ));
    }

    let read_limit = reference
        .byte_length
        .checked_add(1)
        .and_then(|length| u64::try_from(length).ok())
        .ok_or_else(|| {
            CaseError::Invalid(format!("artifact slot `{slot}` read limit overflowed"))
        })?;
    let mut bounded = file.take(read_limit);
    let mut bytes = Vec::with_capacity(reference.byte_length);
    bounded
        .read_to_end(&mut bytes)
        .map_err(|source| CaseError::ArtifactRead {
            slot: slot.to_owned(),
            path: reference.path.to_path_buf(),
            source,
        })?;
    if bytes.len() != reference.byte_length {
        return invalid(format!(
            "artifact slot `{slot}` changed while it was read; observed {} bytes, expected {}",
            bytes.len(),
            reference.byte_length
        ));
    }
    if sha256_hex(&bytes) != reference.sha256 {
        return invalid(format!("artifact slot `{slot}` failed its SHA-256 check"));
    }
    Ok(AuthenticatedArtifact {
        relative_path: reference.path.to_path_buf(),
        bytes,
    })
}

fn validate_artifact_path(label: &str, path: &Path) -> Result<(), CaseError> {
    let text = path
        .to_str()
        .ok_or_else(|| CaseError::Invalid(format!("{label}.path must be UTF-8")))?;
    let is_invalid = text.is_empty()
        || text.len() > 2_048
        || text.starts_with('/')
        || text.starts_with('\\')
        || looks_like_windows_drive(text)
        || text.contains(['\\', ':', '#', '?', '%'])
        || text.chars().any(char::is_control)
        || text
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)));
    if is_invalid {
        return invalid(format!(
            "{label}.path must be a safe normalized portable relative path"
        ));
    }
    Ok(())
}

fn require_exact_list(label: &str, actual: &[String], expected: &[&str]) -> Result<(), CaseError> {
    if actual
        .iter()
        .map(String::as_str)
        .eq(expected.iter().copied())
    {
        Ok(())
    } else {
        invalid(format!("{label} must equal the fixed v1 list"))
    }
}

fn require_equal<T>(label: &str, actual: T, expected: T) -> Result<(), CaseError>
where
    T: PartialEq + std::fmt::Display,
{
    if actual == expected {
        Ok(())
    } else {
        invalid(format!("{label} must be `{expected}`, got `{actual}`"))
    }
}

fn looks_like_windows_drive(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn invalid<T>(message: impl Into<String>) -> Result<T, CaseError> {
    Err(CaseError::Invalid(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const CASE: &str = include_str!("../cases/generic-bevy-0.18.1.toml");

    fn replace_once(source: &str, from: &str, to: &str) -> String {
        assert_eq!(
            source.matches(from).count(),
            1,
            "ambiguous test replacement"
        );
        source.replacen(from, to, 1)
    }

    fn provide_next_artifact(
        directory: &Path,
        source: String,
        placeholder: &str,
        name: &str,
        bytes: &[u8],
    ) -> String {
        assert!(source.contains(placeholder), "missing slot `{placeholder}`");
        fs::write(directory.join(name), bytes).expect("write authenticated artifact");
        let (field, _value) = placeholder.split_once('=').expect("slot has field name");
        let reference = format!(
            "{}= {{ required = true, path = \"{name}\", byte_length = {}, sha256 = \"{}\" }}",
            field,
            bytes.len(),
            sha256_hex(bytes)
        );
        source.replacen(placeholder, &reference, 1)
    }

    #[test]
    fn relative_case_paths_receive_one_stable_absolute_anchor() {
        let directory = tempfile::tempdir().expect("temporary absolute anchor");
        assert!(directory.path().is_absolute());

        let anchored = anchor_case_path(Path::new("private/case.toml"), directory.path())
            .expect("absolute working directory anchors a relative case");
        assert_eq!(anchored, directory.path().join("private/case.toml"));
        assert!(anchored.is_absolute());

        let already_absolute = directory.path().join("elsewhere/case.toml");
        assert_eq!(
            anchor_case_path(&already_absolute, Path::new("ignored-relative-base"))
                .expect("absolute inputs do not inspect a working-directory anchor"),
            already_absolute
        );
        assert!(
            anchor_case_path(Path::new("case.toml"), Path::new("unsafe-relative-anchor")).is_err()
        );
    }

    #[test]
    fn checked_in_case_is_strictly_not_run_and_incomplete() {
        let manifest = parse_case(CASE).expect("checked-in case parses");
        assert_eq!(manifest.case_id(), "generic-bevy-0.18.1");
        assert!(!manifest.gate_eligible());
        assert_eq!(manifest.status(), "not_run");
        assert_eq!(manifest.provided_artifact_count(), 0);
        assert!(!manifest.inputs_complete());
        assert_eq!(manifest.required_artifact_count(), 21);
    }

    #[test]
    fn schema_and_fixed_scope_fail_closed() {
        for invalid_case in [
            replace_once(CASE, "format_version = 1", "format_version = 2"),
            replace_once(
                CASE,
                "evidence_class = \"non_representative_rehearsal\"",
                "evidence_class = \"authoritative_gate\"",
            ),
            replace_once(CASE, "gate_eligible = false", "gate_eligible = true"),
            replace_once(CASE, "status = \"not_run\"", "status = \"passed\""),
            replace_once(
                CASE,
                "target_spine_version = \"4.3.23\"",
                "target_spine_version = \"4.3.24\"",
            ),
            replace_once(
                CASE,
                "bevy_version = \"0.18.1\"",
                "bevy_version = \"0.19.0\"",
            ),
            format!("{CASE}\nunknown = true\n"),
        ] {
            assert!(parse_case(&invalid_case).is_err());
        }
    }

    #[test]
    fn fields_hosts_tolerances_and_diagnostics_are_not_relaxable() {
        for invalid_case in [
            replace_once(CASE, "    \"diagnostic_codes[]\",\n", ""),
            replace_once(CASE, "    \"event_timelines\",\n", ""),
            replace_once(
                CASE,
                "allowed_nonblocking_diagnostics = []",
                "allowed_nonblocking_diagnostics = [\"unknown_field\"]",
            ),
            replace_once(
                CASE,
                "native_framebuffer_required = false",
                "native_framebuffer_required = true",
            ),
            replace_once(CASE, "position_abs = 0.0001", "position_abs = 0.001"),
            replace_once(
                CASE,
                "max_changed_pixel_fraction = 0.02",
                "max_changed_pixel_fraction = 0.2",
            ),
        ] {
            assert!(parse_case(&invalid_case).is_err());
        }
    }

    #[test]
    fn evidence_slots_reject_partial_unsafe_or_unbounded_references() {
        let slot = "runtime_manifest = { required = true }";
        for replacement in [
            "runtime_manifest = { required = false }",
            "runtime_manifest = { required = true, path = \"fixture.json\" }",
            "runtime_manifest = { required = true, path = \"../fixture.json\", byte_length = 1, sha256 = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\" }",
            "runtime_manifest = { required = true, path = \"fixture.json\", byte_length = 0, sha256 = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\" }",
            "runtime_manifest = { required = true, path = \"fixture.json\", byte_length = 65537, sha256 = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\" }",
            "runtime_manifest = { required = true, path = \"fixture.json\", byte_length = 1, sha256 = \"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\" }",
        ] {
            let invalid_case = replace_once(CASE, slot, replacement);
            assert!(parse_case(&invalid_case).is_err(), "accepted {replacement}");
        }
    }

    #[test]
    fn v1_case_and_schedule_are_literal() {
        for invalid_case in [
            replace_once(
                CASE,
                "case_id = \"generic-bevy-0.18.1\"",
                "case_id = \"generic-bevy-0.18.2\"",
            ),
            replace_once(
                CASE,
                "name = \"sway\"\nduration_ns = 1000000000",
                "name = \"idle\"\nduration_ns = 1000000000",
            ),
            replace_once(CASE, "duration_ns = 1000000000", "duration_ns = 999999999"),
            replace_once(CASE, "time_ns = 0\n", "time_ns = 1\n"),
            replace_once(CASE, "id = \"sway-middle\"", "id = \"renamed-middle\""),
            replace_once(CASE, "time_ns = 500000000", "time_ns = 400000000"),
            replace_once(CASE, "skin_layers = [\"alternate\"]", "skin_layers = []"),
            replace_once(CASE, "time_ns = 750000000", "time_ns = 700000000"),
            replace_once(CASE, "time_ns = 1000000000\n", "time_ns = 999999999\n"),
            replace_once(CASE, "id = \"sway-events\"", "id = \"renamed-events\""),
            replace_once(CASE, "end_ns = 1000000000", "end_ns = 999999999"),
        ] {
            assert!(parse_case(&invalid_case).is_err());
        }

        let missing_sample = replace_once(
            CASE,
            "[[samples]]\nid = \"sway-alternate-skin\"\nanimation = \"sway\"\ntime_ns = 750000000\nskin_layers = [\"alternate\"]\ncurrent_semantic = {required = true}\nproposed_semantic = {required = true}\ncurrent_browser_pixels = {required = true}\nproposed_browser_pixels = {required = true}\n\n",
            "",
        );
        assert!(parse_case(&missing_sample).is_err());
    }

    #[test]
    fn load_case_authenticates_each_concrete_artifact() {
        let directory = tempfile::tempdir().expect("temporary case directory");
        let artifact = b"real manifest bytes";
        fs::write(directory.path().join("current.json"), artifact).expect("write artifact");
        let reference = format!(
            "runtime_manifest = {{ required = true, path = \"current.json\", byte_length = {}, sha256 = \"{}\" }}",
            artifact.len(),
            sha256_hex(artifact)
        );
        let case = replace_once(CASE, "runtime_manifest = { required = true }", &reference);
        let case_path = directory.path().join("case.toml");
        fs::write(&case_path, case).expect("write case");

        let loaded = load_case(&case_path).expect("authentic partial input");
        assert_eq!(loaded.verified_artifact_count(), 1);
        assert!(!loaded.semantic_inputs_complete());
        assert!(loaded.semantic_execution_plan().is_none());
        assert!(!loaded.inputs_complete());

        fs::write(directory.path().join("current.json"), b"changed").expect("mutate artifact");
        assert!(load_case(&case_path).is_err());
    }

    #[test]
    fn semantic_plan_borrows_retained_authenticated_bytes_without_claiming_full_readiness() {
        let directory = tempfile::tempdir().expect("temporary case directory");
        let provenance = b"project-owned analytical method";
        let current_manifest = b"current runtime manifest";
        let proposed_manifest = b"proposed runtime manifest";
        let mut current_semantics = Vec::new();
        let mut proposed_semantics = Vec::new();
        let mut case = CASE.to_owned();

        case = provide_next_artifact(
            directory.path(),
            case,
            "method_document = { required=true }",
            "provenance.md",
            provenance,
        );
        case = provide_next_artifact(
            directory.path(),
            case,
            "runtime_manifest = { required = true }",
            "current-runtime.json",
            current_manifest,
        );
        case = provide_next_artifact(
            directory.path(),
            case,
            "runtime_manifest = {required = true}",
            "proposed-runtime.json",
            proposed_manifest,
        );

        for index in 0..SAMPLE_COUNT {
            let current = format!("current semantic reference {index}").into_bytes();
            let proposed = format!("proposed semantic reference {index}").into_bytes();
            case = provide_next_artifact(
                directory.path(),
                case,
                "current_semantic = {required = true}",
                &format!("current-semantic-{index}.json"),
                &current,
            );
            case = provide_next_artifact(
                directory.path(),
                case,
                "proposed_semantic = {required = true}",
                &format!("proposed-semantic-{index}.json"),
                &proposed,
            );
            current_semantics.push(current);
            proposed_semantics.push(proposed);
        }

        let case_path = directory.path().join("case.toml");
        fs::write(&case_path, case).expect("write semantic case");
        let loaded = load_case(&case_path).expect("authenticate semantic execution inputs");

        assert_eq!(loaded.verified_artifact_count(), 11);
        assert!(loaded.semantic_inputs_complete());
        assert!(!loaded.inputs_complete());
        assert_eq!(loaded.manifest().status(), "not_run");
        assert!(!loaded.manifest().gate_eligible());

        fs::write(
            directory.path().join("current-runtime.json"),
            b"mutated after loading",
        )
        .expect("mutate source after authentication");

        let plan = loaded
            .semantic_execution_plan()
            .expect("complete semantic plan");
        assert_eq!(plan.provenance_document(), provenance);
        assert_eq!(plan.current_runtime_manifest(), current_manifest);
        assert_eq!(plan.proposed_runtime_manifest(), proposed_manifest);
        for (index, sample) in plan.samples().iter().enumerate() {
            let expected = SAMPLE_SCHEDULE[index];
            assert_eq!(sample.id(), expected.id());
            assert_eq!(sample.animation(), ANIMATION_NAME);
            assert_eq!(sample.time_ns(), expected.time_ns());
            assert!(
                sample
                    .skin_layers()
                    .eq(expected.skin_layers().iter().copied())
            );
            assert_eq!(sample.current_semantic(), current_semantics[index]);
            assert_eq!(sample.proposed_semantic(), proposed_semantics[index]);
        }
    }

    #[test]
    fn load_case_rejects_oversized_actual_before_bounded_read() {
        let directory = tempfile::tempdir().expect("temporary case directory");
        let artifact_path = directory.path().join("current.json");
        let artifact = fs::File::create(&artifact_path).expect("create sparse artifact");
        artifact
            .set_len((MAX_RUNTIME_MANIFEST_BYTES + 1) as u64)
            .expect("size sparse artifact");
        let reference = format!(
            "runtime_manifest = {{ required = true, path = \"current.json\", byte_length = 1, sha256 = \"{}\" }}",
            sha256_hex(b"x")
        );
        let case = replace_once(CASE, "runtime_manifest = { required = true }", &reference);
        let case_path = directory.path().join("case.toml");
        fs::write(&case_path, case).expect("write case");

        let error = load_case(&case_path)
            .expect_err("metadata length mismatch must fail before reading")
            .to_string();
        assert!(error.contains(&format!(
            "has {} bytes; expected 1",
            MAX_RUNTIME_MANIFEST_BYTES + 1
        )));
    }

    #[cfg(unix)]
    #[test]
    fn load_case_rejects_artifact_symlinks() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().expect("temporary case directory");
        let artifact = b"real manifest bytes";
        fs::write(directory.path().join("target.json"), artifact).expect("write target");
        symlink("target.json", directory.path().join("current.json")).expect("create symlink");
        let reference = format!(
            "runtime_manifest = {{ required = true, path = \"current.json\", byte_length = {}, sha256 = \"{}\" }}",
            artifact.len(),
            sha256_hex(artifact)
        );
        let case = replace_once(CASE, "runtime_manifest = { required = true }", &reference);
        let case_path = directory.path().join("case.toml");
        fs::write(&case_path, case).expect("write case");
        assert!(load_case(case_path).is_err());
    }
}

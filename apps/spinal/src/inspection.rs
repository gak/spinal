//! Stable host-neutral inspection snapshots for validated source bundles.

use std::{fmt, io};

use bevy_spinal::spinal::{
    Diagnostic, DiagnosticScope, SemanticDiagnosticCode, SemanticDiagnosticSeverity, SkeletonAsset,
    TARGET_SPINE_VERSION,
};
use serde::Serialize;

use crate::bundle::SourceBundle;

/// The source-inspection JSON schema emitted by this application version.
pub(crate) const SOURCE_INSPECTION_FORMAT_VERSION: u16 = 1;
pub(crate) const MAX_CANONICAL_JSON_BYTES: usize = 4 * 1024 * 1024;
const MAX_CATALOG_ENTRIES: usize = 256;
const MAX_AUTHORED_NAME_BYTES: usize = 256;
const MAX_DIAGNOSTIC_MESSAGE_BYTES: usize = 768;

/// Whether the source can be reproduced by the current runtime profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InspectionOutcome {
    Compatible,
    Degraded,
}

impl InspectionOutcome {
    /// Returns whether the runtime deliberately degrades authored behavior.
    pub(crate) const fn is_degraded(self) -> bool {
        matches!(self, Self::Degraded)
    }
}

/// One immutable, versioned inspection of a validated source bundle.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct SourceInspection {
    format_version: u16,
    #[serde(rename = "status")]
    outcome: InspectionOutcome,
    source: InspectionSource,
    inventory: InspectionInventory,
    diagnostics: Box<[InspectionDiagnostic]>,
}

impl SourceInspection {
    /// Captures a deterministic inspection without reloading source bytes.
    pub(crate) fn capture(bundle: &SourceBundle) -> Self {
        let asset = bundle.skeleton().as_ref();
        let mut diagnostics = asset
            .diagnostics()
            .iter()
            .map(|diagnostic| InspectionDiagnostic::capture(asset, diagnostic))
            .collect::<Vec<_>>();
        diagnostics.sort_by_cached_key(|diagnostic| {
            (
                matches!(
                    diagnostic.code(),
                    SemanticDiagnosticCode::DiagnosticsTruncated
                ),
                serde_json::to_vec(diagnostic)
                    .expect("a semantic diagnostic contains only serializable values"),
            )
        });

        Self {
            format_version: SOURCE_INSPECTION_FORMAT_VERSION,
            outcome: if asset.has_degradations() {
                InspectionOutcome::Degraded
            } else {
                InspectionOutcome::Compatible
            },
            source: InspectionSource::capture(bundle, asset),
            inventory: InspectionInventory::capture(asset),
            diagnostics: diagnostics.into_boxed_slice(),
        }
    }

    /// Serializes deterministic compact JSON for automation and comparison.
    pub(crate) fn to_canonical_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        let mut writer = BoundedJsonWriter::new();
        serde_json::to_writer(&mut writer, self)?;
        Ok(writer.into_bytes())
    }

    /// Returns the source-inspection schema version.
    pub(crate) const fn format_version(&self) -> u16 {
        self.format_version
    }

    /// Returns the aggregate compatibility outcome.
    pub(crate) const fn outcome(&self) -> InspectionOutcome {
        self.outcome
    }

    /// Returns stable source identity and bounded bundle totals.
    pub(crate) const fn source(&self) -> &InspectionSource {
        &self.source
    }

    /// Returns compact runtime inventory in authored order.
    pub(crate) const fn inventory(&self) -> &InspectionInventory {
        &self.inventory
    }

    /// Returns diagnostics in canonical order, with truncation last.
    pub(crate) fn diagnostics(&self) -> &[InspectionDiagnostic] {
        &self.diagnostics
    }
}

/// One bounded, stable-name diagnostic shared by check and Diagnostics UI.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct InspectionDiagnostic {
    severity: SemanticDiagnosticSeverity,
    code: SemanticDiagnosticCode,
    scope: InspectionDiagnosticScope,
    scope_truncated: bool,
    message: Box<str>,
    message_truncated: bool,
}

impl InspectionDiagnostic {
    fn capture(asset: &SkeletonAsset, diagnostic: &Diagnostic) -> Self {
        let (scope, scope_truncated) =
            InspectionDiagnosticScope::capture(asset, diagnostic.scope());
        let (message, message_truncated) =
            bounded_text(diagnostic.message(), MAX_DIAGNOSTIC_MESSAGE_BYTES);
        Self {
            severity: diagnostic.severity().into(),
            code: diagnostic.code().into(),
            scope,
            scope_truncated,
            message,
            message_truncated,
        }
    }

    pub(crate) const fn severity(&self) -> SemanticDiagnosticSeverity {
        self.severity
    }

    pub(crate) const fn code(&self) -> SemanticDiagnosticCode {
        self.code
    }

    pub(crate) const fn scope(&self) -> &InspectionDiagnosticScope {
        &self.scope
    }

    pub(crate) const fn scope_was_truncated(&self) -> bool {
        self.scope_truncated
    }

    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    pub(crate) const fn message_was_truncated(&self) -> bool {
        self.message_truncated
    }
}

/// One bounded stable-name diagnostic scope.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub(crate) enum InspectionDiagnosticScope {
    Asset,
    Bone(Box<str>),
    Slot(Box<str>),
    Skin(Box<str>),
    Animation(Box<str>),
    Event(Box<str>),
    Attachment(InspectionAttachmentScope),
    IkConstraint(Box<str>),
    Constraint(Box<str>),
    AtlasPage(Box<str>),
    AtlasRegion(InspectionAtlasRegionScope),
    Unknown,
}

impl InspectionDiagnosticScope {
    fn capture(asset: &SkeletonAsset, scope: DiagnosticScope) -> (Self, bool) {
        match scope {
            DiagnosticScope::Asset => (Self::Asset, false),
            DiagnosticScope::Bone(id) => {
                let bone = asset
                    .bone(id)
                    .expect("a diagnostic bone belongs to its immutable asset");
                bounded_named_scope(bone.name(), Self::Bone)
            }
            DiagnosticScope::Slot(id) => {
                let slot = asset
                    .slot(id)
                    .expect("a diagnostic slot belongs to its immutable asset");
                bounded_named_scope(slot.name(), Self::Slot)
            }
            DiagnosticScope::Skin(id) => {
                let skin = asset
                    .skin(id)
                    .expect("a diagnostic skin belongs to its immutable asset");
                bounded_named_scope(skin.name(), Self::Skin)
            }
            DiagnosticScope::Animation(id) => {
                let animation = asset
                    .animation(id)
                    .expect("a diagnostic animation belongs to its immutable asset");
                bounded_named_scope(animation.name(), Self::Animation)
            }
            DiagnosticScope::Event(id) => {
                let event = asset
                    .event_definition(id)
                    .expect("a diagnostic event belongs to its immutable asset");
                bounded_named_scope(event.name(), Self::Event)
            }
            DiagnosticScope::Attachment(id) => {
                let attachment = asset
                    .attachment(id)
                    .expect("a diagnostic attachment belongs to its immutable asset");
                let skin = asset
                    .skin(attachment.skin())
                    .expect("a linked attachment skin belongs to its immutable asset");
                let slot = asset
                    .slot(attachment.slot())
                    .expect("a linked attachment slot belongs to its immutable asset");
                let (skin, skin_truncated) = bounded_name(skin.name());
                let (slot, slot_truncated) = bounded_name(slot.name());
                let (placeholder, placeholder_truncated) =
                    bounded_name(attachment.placeholder_name());
                let (name, name_truncated) = bounded_name(attachment.name());
                (
                    Self::Attachment(InspectionAttachmentScope {
                        skin,
                        slot,
                        placeholder,
                        name,
                    }),
                    skin_truncated || slot_truncated || placeholder_truncated || name_truncated,
                )
            }
            DiagnosticScope::IkConstraint(id) => {
                let constraint = asset
                    .ik_constraint(id)
                    .expect("a diagnostic IK constraint belongs to its immutable asset");
                bounded_named_scope(constraint.name(), Self::IkConstraint)
            }
            DiagnosticScope::Constraint(id) => {
                let constraint = asset
                    .constraint(id)
                    .expect("a diagnostic constraint belongs to its immutable asset");
                bounded_named_scope(constraint.name(), Self::Constraint)
            }
            DiagnosticScope::AtlasPage(id) => {
                let page = asset
                    .atlas_page(id)
                    .expect("a diagnostic atlas page belongs to its immutable asset");
                bounded_named_scope(page.name(), Self::AtlasPage)
            }
            DiagnosticScope::AtlasRegion(id) => {
                let region = asset
                    .atlas_region(id)
                    .expect("a diagnostic atlas region belongs to its immutable asset");
                let page = asset
                    .atlas_page(region.page())
                    .expect("a linked atlas page belongs to its immutable asset");
                let (page, page_truncated) = bounded_name(page.name());
                let (region_name, region_truncated) = bounded_name(region.name());
                (
                    Self::AtlasRegion(InspectionAtlasRegionScope {
                        page,
                        region: region_name,
                        sequence_index: region.index(),
                    }),
                    page_truncated || region_truncated,
                )
            }
            _ => (Self::Unknown, false),
        }
    }
}

impl fmt::Display for InspectionDiagnosticScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Asset => formatter.write_str("asset"),
            Self::Bone(name) => write!(formatter, "bone {name:?}"),
            Self::Slot(name) => write!(formatter, "slot {name:?}"),
            Self::Skin(name) => write!(formatter, "skin {name:?}"),
            Self::Animation(name) => write!(formatter, "animation {name:?}"),
            Self::Event(name) => write!(formatter, "event {name:?}"),
            Self::Attachment(attachment) => write!(formatter, "{attachment}"),
            Self::IkConstraint(name) => write!(formatter, "IK constraint {name:?}"),
            Self::Constraint(name) => write!(formatter, "constraint {name:?}"),
            Self::AtlasPage(name) => write!(formatter, "atlas page {name:?}"),
            Self::AtlasRegion(region) => write!(formatter, "{region}"),
            Self::Unknown => formatter.write_str("unknown scope"),
        }
    }
}

/// Bounded authored identity for an attachment diagnostic.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct InspectionAttachmentScope {
    skin: Box<str>,
    slot: Box<str>,
    placeholder: Box<str>,
    name: Box<str>,
}

impl fmt::Display for InspectionAttachmentScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "attachment {:?} in skin {:?}, slot {:?}, placeholder {:?}",
            self.name, self.skin, self.slot, self.placeholder
        )
    }
}

/// Bounded authored identity for an atlas-region diagnostic.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct InspectionAtlasRegionScope {
    page: Box<str>,
    region: Box<str>,
    sequence_index: Option<u32>,
}

impl fmt::Display for InspectionAtlasRegionScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "atlas region {:?} on page {:?}",
            self.region, self.page
        )?;
        if let Some(index) = self.sequence_index {
            write!(formatter, " at sequence index {index}")?;
        }
        Ok(())
    }
}

/// Stable source identity and bounded bundle totals.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct InspectionSource {
    target_spine_version: Box<str>,
    declared_spine_version: Box<str>,
    json_path: Box<str>,
    atlas_path: Box<str>,
    manifest_sha256: Box<str>,
    content_sha256: Box<str>,
    file_count: u64,
    encoded_bytes: u64,
    decoded_texture_bytes: u64,
}

impl InspectionSource {
    fn capture(bundle: &SourceBundle, asset: &SkeletonAsset) -> Self {
        Self {
            target_spine_version: TARGET_SPINE_VERSION.into(),
            declared_spine_version: asset.spine_version().into(),
            json_path: virtual_path(bundle.json_asset_path()).into(),
            atlas_path: virtual_path(bundle.atlas_asset_path()).into(),
            manifest_sha256: bundle.provenance().manifest_sha256().into(),
            content_sha256: bundle.provenance().content_sha256().into(),
            file_count: bundle_size(bundle.file_count()),
            encoded_bytes: bundle_size(bundle.encoded_bytes()),
            decoded_texture_bytes: bundle_size(bundle.decoded_texture_bytes()),
        }
    }

    pub(crate) fn target_spine_version(&self) -> &str {
        &self.target_spine_version
    }

    pub(crate) fn declared_spine_version(&self) -> &str {
        &self.declared_spine_version
    }

    pub(crate) fn json_path(&self) -> &str {
        &self.json_path
    }

    pub(crate) fn atlas_path(&self) -> &str {
        &self.atlas_path
    }

    pub(crate) fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
    }

    pub(crate) fn content_sha256(&self) -> &str {
        &self.content_sha256
    }

    pub(crate) const fn file_count(&self) -> u64 {
        self.file_count
    }

    pub(crate) const fn encoded_bytes(&self) -> u64 {
        self.encoded_bytes
    }

    pub(crate) const fn decoded_texture_bytes(&self) -> u64 {
        self.decoded_texture_bytes
    }
}

/// Compact runtime inventory with ordered user-facing selections.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct InspectionInventory {
    counts: InspectionCounts,
    animations: Box<[InspectedAnimation]>,
    animations_omitted: u32,
    skins: Box<[InspectedSkin]>,
    skins_omitted: u32,
}

impl InspectionInventory {
    fn capture(asset: &SkeletonAsset) -> Self {
        let default_skin = asset.default_skin().map(|skin| skin.ordinal());
        let animation_count = asset.animations().len();
        let animations = asset
            .animations()
            .take(MAX_CATALOG_ENTRIES)
            .map(|animation| {
                let (name, name_truncated) = bounded_name(animation.name());
                InspectedAnimation {
                    ordinal: source_ordinal(animation.ordinal()),
                    name,
                    name_truncated,
                    duration_ns: u64::try_from(animation.duration().as_nanos())
                        .expect("Spinal stores animation durations as u64 nanoseconds"),
                }
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let skin_count = asset.skins().len();
        let skins = asset
            .skins()
            .take(MAX_CATALOG_ENTRIES)
            .map(|skin| {
                let (name, name_truncated) = bounded_name(skin.name());
                InspectedSkin {
                    ordinal: source_ordinal(skin.ordinal()),
                    name,
                    name_truncated,
                    is_default: default_skin == Some(skin.ordinal()),
                }
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();

        Self {
            counts: InspectionCounts {
                bones: source_count(asset.bones().len()),
                slots: source_count(asset.slots().len()),
                skins: source_count(asset.skins().len()),
                attachments: source_count(asset.attachments().len()),
                animations: source_count(asset.animations().len()),
                ik_constraints: source_count(asset.ik_constraints().len()),
                transform_constraints: source_count(asset.transform_constraints().len()),
                constraints: source_count(asset.constraints().len()),
                events: source_count(asset.event_definitions().len()),
                atlas_pages: source_count(asset.atlas_pages().len()),
                atlas_regions: source_count(asset.atlas_regions().len()),
            },
            animations_omitted: source_count(animation_count.saturating_sub(animations.len())),
            animations,
            skins_omitted: source_count(skin_count.saturating_sub(skins.len())),
            skins,
        }
    }

    pub(crate) const fn counts(&self) -> &InspectionCounts {
        &self.counts
    }

    pub(crate) fn animations(&self) -> &[InspectedAnimation] {
        &self.animations
    }

    pub(crate) const fn omitted_animation_count(&self) -> u32 {
        self.animations_omitted
    }

    pub(crate) const fn animations_are_truncated(&self) -> bool {
        self.animations_omitted > 0
    }

    pub(crate) fn skins(&self) -> &[InspectedSkin] {
        &self.skins
    }

    pub(crate) const fn omitted_skin_count(&self) -> u32 {
        self.skins_omitted
    }

    pub(crate) const fn skins_are_truncated(&self) -> bool {
        self.skins_omitted > 0
    }
}

/// Fixed-size aggregate counts used by both check output and viewer UI.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct InspectionCounts {
    bones: u32,
    slots: u32,
    skins: u32,
    attachments: u32,
    animations: u32,
    ik_constraints: u32,
    transform_constraints: u32,
    constraints: u32,
    events: u32,
    atlas_pages: u32,
    atlas_regions: u32,
}

impl InspectionCounts {
    pub(crate) const fn bones(self) -> u32 {
        self.bones
    }

    pub(crate) const fn slots(self) -> u32 {
        self.slots
    }

    pub(crate) const fn skins(self) -> u32 {
        self.skins
    }

    pub(crate) const fn attachments(self) -> u32 {
        self.attachments
    }

    pub(crate) const fn animations(self) -> u32 {
        self.animations
    }

    pub(crate) const fn ik_constraints(self) -> u32 {
        self.ik_constraints
    }

    pub(crate) const fn transform_constraints(self) -> u32 {
        self.transform_constraints
    }

    pub(crate) const fn constraints(self) -> u32 {
        self.constraints
    }

    pub(crate) const fn events(self) -> u32 {
        self.events
    }

    pub(crate) const fn atlas_pages(self) -> u32 {
        self.atlas_pages
    }

    pub(crate) const fn atlas_regions(self) -> u32 {
        self.atlas_regions
    }
}

/// One source-ordered animation selection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct InspectedAnimation {
    ordinal: u32,
    name: Box<str>,
    name_truncated: bool,
    duration_ns: u64,
}

impl InspectedAnimation {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) const fn name_was_truncated(&self) -> bool {
        self.name_truncated
    }

    pub(crate) const fn duration_ns(&self) -> u64 {
        self.duration_ns
    }
}

/// One source-ordered skin selection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct InspectedSkin {
    ordinal: u32,
    name: Box<str>,
    name_truncated: bool,
    is_default: bool,
}

impl InspectedSkin {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) const fn name_was_truncated(&self) -> bool {
        self.name_truncated
    }

    pub(crate) const fn is_default(&self) -> bool {
        self.is_default
    }
}

fn virtual_path(path: &std::path::Path) -> &str {
    path.to_str()
        .expect("validated runtime-bundle paths are UTF-8")
}

fn bundle_size(value: usize) -> u64 {
    u64::try_from(value).expect("validated runtime-bundle limits fit u64")
}

fn source_count(value: usize) -> u32 {
    u32::try_from(value).expect("the Spinal loader bounds authored inventories to u32")
}

fn source_ordinal(value: usize) -> u32 {
    u32::try_from(value).expect("the Spinal loader bounds authored ordinals to u32")
}

fn bounded_name(name: &str) -> (Box<str>, bool) {
    bounded_text(name, MAX_AUTHORED_NAME_BYTES)
}

fn bounded_named_scope(
    name: &str,
    constructor: impl FnOnce(Box<str>) -> InspectionDiagnosticScope,
) -> (InspectionDiagnosticScope, bool) {
    let (name, truncated) = bounded_name(name);
    (constructor(name), truncated)
}

fn bounded_text(value: &str, maximum_bytes: usize) -> (Box<str>, bool) {
    if value.len() <= maximum_bytes {
        return (value.into(), false);
    }
    let mut end = maximum_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].into(), true)
}

struct BoundedJsonWriter {
    bytes: Vec<u8>,
}

impl BoundedJsonWriter {
    fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

impl io::Write for BoundedJsonWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let remaining = MAX_CANONICAL_JSON_BYTES.saturating_sub(self.bytes.len());
        if bytes.len() > remaining {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("source inspection exceeds the {MAX_CANONICAL_JSON_BYTES}-byte JSON limit"),
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::Path};

    use serde_json::Value;

    use super::*;
    use crate::bundle::TEST_BLUE_PIXEL_PNG;

    const FIXTURE_JSON: &[u8] = br#"{
      "skeleton":{"spine":"4.3.23"},
      "bones":[{"name":"root"},{"name":"child","parent":"root"}],
      "slots":[{"name":"body","bone":"root"}],
      "skins":[{"name":"default"},{"name":"winter"}],
      "events":{"step":{}},
      "animations":{
        "idle":{"bones":{"root":{"rotate":[{}, {"time":1.25,"value":5}]}}},
        "blink":{}
      }
    }"#;
    const FIXTURE_ATLAS: &[u8] = b"textures/rig.png\n\
\tsize: 1, 1\n\
\tformat: RGBA8888\n\
\tfilter: Linear, Linear\n\
\trepeat: none\n\
\tpma: false\n";

    fn bundle_from_host_root(label: &str, host_root: &Path, json: &[u8]) -> SourceBundle {
        let physical_json = host_root.join("cat/rig.json");
        let physical_atlas = host_root.join("cat/rig.atlas");
        let physical_page = host_root.join("cat/textures/rig.png");
        let json_path = physical_json
            .strip_prefix(host_root)
            .expect("fixture JSON is below its host root")
            .to_path_buf();
        let atlas_path = physical_atlas
            .strip_prefix(host_root)
            .expect("fixture atlas is below its host root")
            .to_path_buf();
        let page_path = physical_page
            .strip_prefix(host_root)
            .expect("fixture page is below its host root")
            .to_path_buf();
        let files = BTreeMap::from([
            (json_path.clone(), json.to_vec()),
            (atlas_path.clone(), FIXTURE_ATLAS.to_vec()),
            (page_path, TEST_BLUE_PIXEL_PNG.to_vec()),
        ]);

        SourceBundle::from_test_files(label, &json_path, &atlas_path, files)
    }

    fn fixture_bundle(host_root: &Path) -> SourceBundle {
        bundle_from_host_root("Stable fixture", host_root, FIXTURE_JSON)
    }

    fn diagnostic_json() -> Vec<u8> {
        let mut json = String::from(r#"{"skeleton":{"spine":"4.3.23"},"bones":[{"name":"root"}]"#);
        for index in (0..260).rev() {
            json.push_str(&format!(r#", "unsupported_{index:03}":{{}}"#));
        }
        json.push('}');
        json.into_bytes()
    }

    fn large_catalog_json() -> Vec<u8> {
        let long_name = "é".repeat(MAX_AUTHORED_NAME_BYTES);
        let animations = (0..MAX_CATALOG_ENTRIES + 4)
            .map(|index| format!(r#""{long_name}-animation-{index}":{{}}"#))
            .collect::<Vec<_>>()
            .join(",");
        let skins = std::iter::once(r#"{"name":"default"}"#.to_owned())
            .chain(
                (1..MAX_CATALOG_ENTRIES + 4)
                    .map(|index| format!(r#"{{"name":"{long_name}-skin-{index}"}}"#)),
            )
            .collect::<Vec<_>>()
            .join(",");
        format!(
            r#"{{"skeleton":{{"spine":"4.3.23"}},"bones":[{{"name":"root"}}],"skins":[{skins}],"animations":{{{animations}}}}}"#
        )
        .into_bytes()
    }

    #[test]
    fn canonical_bytes_are_stable_across_independent_loads() {
        let first_bundle = fixture_bundle(Path::new("/fixture-sources/artist/export"));
        let second_bundle = fixture_bundle(Path::new("/fixture-sources/artist/export"));
        let first = SourceInspection::capture(&first_bundle);
        let second = SourceInspection::capture(&second_bundle);

        assert_eq!(
            first
                .to_canonical_json()
                .expect("canonical inspection JSON"),
            second
                .to_canonical_json()
                .expect("canonical inspection JSON")
        );
        assert_eq!(first.format_version(), SOURCE_INSPECTION_FORMAT_VERSION);
        assert_eq!(first.outcome(), InspectionOutcome::Compatible);
        assert!(!first.outcome().is_degraded());
        assert!(first.diagnostics().is_empty());

        let source = first.source();
        assert_eq!(source.target_spine_version(), "4.3.23");
        assert_eq!(source.declared_spine_version(), "4.3.23");
        assert_eq!(source.json_path(), "cat/rig.json");
        assert_eq!(source.atlas_path(), "cat/rig.atlas");
        assert_eq!(source.manifest_sha256().len(), 64);
        assert_eq!(source.content_sha256().len(), 64);
        assert_eq!(source.file_count(), 3);
        assert!(source.encoded_bytes() > 0);
        assert_eq!(source.decoded_texture_bytes(), 4);

        let inventory = first.inventory();
        let counts = *inventory.counts();
        assert_eq!(counts.bones(), 2);
        assert_eq!(counts.slots(), 1);
        assert_eq!(counts.skins(), 2);
        assert_eq!(counts.attachments(), 0);
        assert_eq!(counts.animations(), 2);
        assert_eq!(counts.ik_constraints(), 0);
        assert_eq!(counts.transform_constraints(), 0);
        assert_eq!(counts.constraints(), 0);
        assert_eq!(counts.events(), 1);
        assert_eq!(counts.atlas_pages(), 1);
        assert_eq!(counts.atlas_regions(), 0);

        let animations = inventory.animations();
        assert_eq!(animations[0].ordinal, 0);
        assert_eq!(animations[0].name(), "idle");
        assert_eq!(animations[0].duration_ns(), 1_250_000_000);
        assert_eq!(animations[1].ordinal, 1);
        assert_eq!(animations[1].name(), "blink");
        assert_eq!(animations[1].duration_ns(), 0);

        let skins = inventory.skins();
        assert_eq!(skins[0].ordinal, 0);
        assert_eq!(skins[0].name(), "default");
        assert!(skins[0].is_default());
        assert_eq!(skins[1].ordinal, 1);
        assert_eq!(skins[1].name(), "winter");
        assert!(!skins[1].is_default());

        let value: Value = serde_json::from_slice(
            &first
                .to_canonical_json()
                .expect("canonical inspection JSON"),
        )
        .expect("inspection JSON should parse");
        assert_eq!(value["format_version"], 1);
        assert_eq!(value["status"], "compatible");
        assert!(value.get("outcome").is_none());
    }

    #[test]
    fn equivalent_virtual_bundles_ignore_different_host_roots() {
        let artist_root = Path::new("/fixture-sources/animator/creature-project");
        let coordinator_root = Path::new("/fixture-sources/coordinator/creature-project");
        let artist = SourceInspection::capture(&fixture_bundle(artist_root));
        let coordinator = SourceInspection::capture(&fixture_bundle(coordinator_root));
        let artist_json = artist.to_canonical_json().expect("artist inspection JSON");
        let coordinator_json = coordinator
            .to_canonical_json()
            .expect("coordinator inspection JSON");

        assert_eq!(artist_json, coordinator_json);
        let text = String::from_utf8(artist_json).expect("inspection JSON is UTF-8");
        assert!(!text.contains(artist_root.to_str().expect("UTF-8 fixture path")));
        assert!(!text.contains(coordinator_root.to_str().expect("UTF-8 fixture path")));
        assert!(
            !text.contains("Stable fixture"),
            "display labels stay out of the schema"
        );
    }

    #[test]
    fn diagnostics_are_canonical_and_truncation_is_always_last() {
        let json = diagnostic_json();
        let first_bundle = bundle_from_host_root(
            "Diagnostic fixture",
            Path::new("/fixture-sources/artist/export"),
            &json,
        );
        let second_bundle = bundle_from_host_root(
            "Diagnostic fixture",
            Path::new("/fixture-sources/coordinator/incoming"),
            &json,
        );
        let first = SourceInspection::capture(&first_bundle);
        let second = SourceInspection::capture(&second_bundle);

        assert_eq!(first.outcome(), InspectionOutcome::Degraded);
        assert!(first.outcome().is_degraded());
        assert_eq!(first.diagnostics().len(), 256);
        assert!(matches!(
            first
                .diagnostics()
                .last()
                .expect("truncated diagnostics have a sentinel")
                .code(),
            SemanticDiagnosticCode::DiagnosticsTruncated
        ));
        for pair in first.diagnostics()[..255].windows(2) {
            let left = serde_json::to_vec(&pair[0]).expect("semantic diagnostic JSON");
            let right = serde_json::to_vec(&pair[1]).expect("semantic diagnostic JSON");
            assert!(
                left <= right,
                "ordinary diagnostics must use canonical byte order"
            );
        }
        assert_eq!(
            first.to_canonical_json().expect("first inspection JSON"),
            second.to_canonical_json().expect("second inspection JSON")
        );
    }

    #[test]
    fn catalogs_retain_source_order_with_bounded_names_and_omitted_counts() {
        let json = large_catalog_json();
        let bundle = bundle_from_host_root(
            "Large catalog fixture",
            Path::new("/generic/catalog"),
            &json,
        );

        let inspection = SourceInspection::capture(&bundle);
        let inventory = inspection.inventory();

        assert_eq!(inventory.counts().animations(), 260);
        assert_eq!(inventory.animations().len(), MAX_CATALOG_ENTRIES);
        assert_eq!(inventory.omitted_animation_count(), 4);
        assert!(inventory.animations_are_truncated());
        assert_eq!(inventory.animations()[0].ordinal, 0);
        assert_eq!(
            inventory.animations()[MAX_CATALOG_ENTRIES - 1].ordinal,
            u32::try_from(MAX_CATALOG_ENTRIES - 1).expect("fixture ordinal fits u32")
        );
        assert!(inventory.animations()[0].name_was_truncated());
        assert!(inventory.animations()[0].name().len() <= MAX_AUTHORED_NAME_BYTES);
        assert!(std::str::from_utf8(inventory.animations()[0].name().as_bytes()).is_ok());

        assert_eq!(inventory.counts().skins(), 260);
        assert_eq!(inventory.skins().len(), MAX_CATALOG_ENTRIES);
        assert_eq!(inventory.omitted_skin_count(), 4);
        assert!(inventory.skins_are_truncated());
        assert_eq!(inventory.skins()[0].name(), "default");
        assert!(!inventory.skins()[0].name_was_truncated());
        assert!(inventory.skins()[0].is_default());
        assert!(inventory.skins()[1].name_was_truncated());
        assert!(inventory.skins()[1].name().len() <= MAX_AUTHORED_NAME_BYTES);

        let value: Value = serde_json::from_slice(
            &inspection
                .to_canonical_json()
                .expect("bounded catalog inspection JSON"),
        )
        .expect("inspection JSON parses");
        assert_eq!(value["inventory"]["animations_omitted"], 4);
        assert_eq!(value["inventory"]["skins_omitted"], 4);
        assert_eq!(value["inventory"]["animations"][0]["name_truncated"], true);
    }

    #[test]
    fn diagnostic_scope_and_message_are_utf8_safely_bounded_and_inspectable() {
        let long_name = "é".repeat(MAX_DIAGNOSTIC_MESSAGE_BYTES);
        let json = format!(
            r#"{{"skeleton":{{"spine":"4.3.23"}},"bones":[{{"name":"{long_name}","transform":"onlyTranslation"}}]}}"#
        );
        let bundle = bundle_from_host_root(
            "Bounded scope fixture",
            Path::new("/generic/scope"),
            json.as_bytes(),
        );

        let inspection = SourceInspection::capture(&bundle);
        let diagnostic = inspection
            .diagnostics()
            .first()
            .expect("unsupported inheritance produces a diagnostic");

        assert!(matches!(
            diagnostic.severity(),
            SemanticDiagnosticSeverity::Degraded
        ));
        assert!(matches!(
            diagnostic.code(),
            SemanticDiagnosticCode::UnsupportedBoneTransformMode
        ));
        let InspectionDiagnosticScope::Bone(name) = diagnostic.scope() else {
            panic!("expected a bone diagnostic scope");
        };
        assert!(name.len() <= MAX_AUTHORED_NAME_BYTES);
        assert!(std::str::from_utf8(name.as_bytes()).is_ok());
        assert!(diagnostic.scope_was_truncated());
        assert!(diagnostic.scope().to_string().starts_with("bone \""));
        assert!(diagnostic.message().len() <= MAX_DIAGNOSTIC_MESSAGE_BYTES);
        assert!(diagnostic.message_was_truncated());
    }

    #[test]
    fn canonical_json_has_a_hard_encoded_size_limit() {
        let bundle = fixture_bundle(Path::new("/generic/canonical-limit"));
        let mut inspection = SourceInspection::capture(&bundle);
        inspection.source.json_path = "\u{1}".repeat(MAX_CANONICAL_JSON_BYTES).into();

        let error = inspection
            .to_canonical_json()
            .expect_err("oversized canonical JSON must fail closed");

        assert!(error.is_io());
        assert!(error.to_string().contains("JSON limit"));
    }

    #[test]
    fn diagnostic_messages_are_utf8_safely_bounded() {
        let long_name = "é".repeat(MAX_DIAGNOSTIC_MESSAGE_BYTES);
        let json = format!(
            r#"{{"skeleton":{{"spine":"4.3.23"}},"bones":[{{"name":"root"}}],"{long_name}":{{}}}}"#
        );
        let bundle = bundle_from_host_root(
            "Bounded diagnostic fixture",
            Path::new("/generic/check"),
            json.as_bytes(),
        );

        let inspection = SourceInspection::capture(&bundle);
        let diagnostic = inspection
            .diagnostics()
            .first()
            .expect("the unknown field produces a diagnostic");

        assert!(diagnostic.message_was_truncated());
        assert!(diagnostic.message().len() <= MAX_DIAGNOSTIC_MESSAGE_BYTES);
        assert!(std::str::from_utf8(diagnostic.message().as_bytes()).is_ok());
    }
}

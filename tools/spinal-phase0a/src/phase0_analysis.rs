//! Pure, closed semantic analysis for the Phase 0A JSON and package evidence.

use crate::case::LoadedCase;
use crate::digest::{hex_digest, sha256_bytes};
use crate::json_evidence::{JsonDifference, JsonEvidence, JsonEvidenceError, JsonLimits};
use crate::package::{CasePackageInventories, EntryKind, PackageInventory, TreeEntry};
use crate::process::NewAnimationCollisionEvidence;
use crate::report::EvidenceScope;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::Path;
use thiserror::Error;

const DOCUMENT_COUNT: usize = 10;
const COMPARISON_ARTIFACT_FORMAT_VERSION: u32 = 1;
const MAX_COMPLETE_TEXT_DIFF_BYTES: usize = 8 * 1024 * 1024;
const APPROVED_VOLATILE_POINTERS: [&str; 1] = ["/skeleton/hash"];
const COMPARISON_COVERAGE: &str = "Only properties represented in the supplied Spine JSON documents are compared. Editor-only, binary-only, and unexported project data are outside this evidence.";

/// Closed roles for the ten JSON documents required by Phase 0A.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum JsonDocumentRole {
    CurrentA,
    ReplacementSubmission,
    NewSubmission,
    ReconstructedA,
    CurrentB,
    ReconstructedB,
    ExistingFirst,
    ExistingRepeat,
    NewFirst,
    NewCollisionControl,
}

impl JsonDocumentRole {
    const ORDER: [Self; DOCUMENT_COUNT] = [
        Self::CurrentA,
        Self::ReplacementSubmission,
        Self::NewSubmission,
        Self::ReconstructedA,
        Self::CurrentB,
        Self::ReconstructedB,
        Self::ExistingFirst,
        Self::ExistingRepeat,
        Self::NewFirst,
        Self::NewCollisionControl,
    ];

    const fn index(self) -> usize {
        match self {
            Self::CurrentA => 0,
            Self::ReplacementSubmission => 1,
            Self::NewSubmission => 2,
            Self::ReconstructedA => 3,
            Self::CurrentB => 4,
            Self::ReconstructedB => 5,
            Self::ExistingFirst => 6,
            Self::ExistingRepeat => 7,
            Self::NewFirst => 8,
            Self::NewCollisionControl => 9,
        }
    }

    const fn slug(self) -> &'static str {
        match self {
            Self::CurrentA => "current_a",
            Self::ReplacementSubmission => "replacement_submission",
            Self::NewSubmission => "new_submission",
            Self::ReconstructedA => "reconstructed_a",
            Self::CurrentB => "current_b",
            Self::ReconstructedB => "reconstructed_b",
            Self::ExistingFirst => "existing_first",
            Self::ExistingRepeat => "existing_repeat",
            Self::NewFirst => "new_first",
            Self::NewCollisionControl => "new_collision_control",
        }
    }
}

/// Owned JSON bytes for every fixed Phase 0A document role.
///
/// Digests, parsing, comparison roles, and approvals are derived internally;
/// callers cannot submit any of them separately.
pub(crate) struct Phase0JsonSources {
    pub(crate) current_a: Vec<u8>,
    pub(crate) replacement_submission: Vec<u8>,
    pub(crate) new_submission: Vec<u8>,
    pub(crate) reconstructed_a: Vec<u8>,
    pub(crate) current_b: Vec<u8>,
    pub(crate) reconstructed_b: Vec<u8>,
    pub(crate) existing_first: Vec<u8>,
    pub(crate) existing_repeat: Vec<u8>,
    pub(crate) new_first: Vec<u8>,
    pub(crate) new_collision_control: Vec<u8>,
    pub(crate) new_animation_collision: NewAnimationCollisionEvidence,
}

impl Phase0JsonSources {
    fn into_parts(
        self,
    ) -> (
        [(JsonDocumentRole, Vec<u8>); DOCUMENT_COUNT],
        NewAnimationCollisionEvidence,
    ) {
        let documents = [
            (JsonDocumentRole::CurrentA, self.current_a),
            (
                JsonDocumentRole::ReplacementSubmission,
                self.replacement_submission,
            ),
            (JsonDocumentRole::NewSubmission, self.new_submission),
            (JsonDocumentRole::ReconstructedA, self.reconstructed_a),
            (JsonDocumentRole::CurrentB, self.current_b),
            (JsonDocumentRole::ReconstructedB, self.reconstructed_b),
            (JsonDocumentRole::ExistingFirst, self.existing_first),
            (JsonDocumentRole::ExistingRepeat, self.existing_repeat),
            (JsonDocumentRole::NewFirst, self.new_first),
            (
                JsonDocumentRole::NewCollisionControl,
                self.new_collision_control,
            ),
        ];
        (documents, self.new_animation_collision)
    }
}

/// A role-qualified source identity. Equal content in different roles remains
/// two distinct identities by construction.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct JsonSourceIdentity {
    role: JsonDocumentRole,
    sha256: String,
}

impl JsonSourceIdentity {
    pub(crate) fn role(&self) -> JsonDocumentRole {
        self.role
    }

    pub(crate) fn sha256(&self) -> &str {
        &self.sha256
    }
}

struct AnalyzedDocument {
    identity: JsonSourceIdentity,
    raw: Vec<u8>,
    json: JsonEvidence,
}

impl AnalyzedDocument {
    fn parse(role: JsonDocumentRole, raw: Vec<u8>) -> Result<Self, Phase0AnalysisError> {
        let json = JsonEvidence::from_slice(&raw, JsonLimits::default())
            .map_err(|source| Phase0AnalysisError::Json { role, source })?;
        Ok(Self {
            identity: JsonSourceIdentity {
                role,
                sha256: sha256_bytes(&raw),
            },
            raw,
            json,
        })
    }
}

/// Closed comparisons retained as exact artifact evidence.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum ComparisonId {
    ReplacementFixture,
    NewFixture,
    RoundTripA,
    RoundTripB,
    CurrentDeterminism,
    ReconstructionDeterminism,
    ExistingMutation,
    ExistingRepeat,
    NewMutation,
    NewCollisionControl,
}

impl ComparisonId {
    const fn slug(self) -> &'static str {
        match self {
            Self::ReplacementFixture => "replacement_fixture",
            Self::NewFixture => "new_fixture",
            Self::RoundTripA => "round_trip_a",
            Self::RoundTripB => "round_trip_b",
            Self::CurrentDeterminism => "current_determinism",
            Self::ReconstructionDeterminism => "reconstruction_determinism",
            Self::ExistingMutation => "existing_mutation",
            Self::ExistingRepeat => "existing_repeat",
            Self::NewMutation => "new_mutation",
            Self::NewCollisionControl => "new_collision_control",
        }
    }
}

const COMPARISONS: [(ComparisonId, JsonDocumentRole, JsonDocumentRole); 10] = [
    (
        ComparisonId::ReplacementFixture,
        JsonDocumentRole::CurrentA,
        JsonDocumentRole::ReplacementSubmission,
    ),
    (
        ComparisonId::NewFixture,
        JsonDocumentRole::CurrentA,
        JsonDocumentRole::NewSubmission,
    ),
    (
        ComparisonId::RoundTripA,
        JsonDocumentRole::CurrentA,
        JsonDocumentRole::ReconstructedA,
    ),
    (
        ComparisonId::RoundTripB,
        JsonDocumentRole::CurrentB,
        JsonDocumentRole::ReconstructedB,
    ),
    (
        ComparisonId::CurrentDeterminism,
        JsonDocumentRole::CurrentA,
        JsonDocumentRole::CurrentB,
    ),
    (
        ComparisonId::ReconstructionDeterminism,
        JsonDocumentRole::ReconstructedA,
        JsonDocumentRole::ReconstructedB,
    ),
    (
        ComparisonId::ExistingMutation,
        JsonDocumentRole::CurrentA,
        JsonDocumentRole::ExistingFirst,
    ),
    (
        ComparisonId::ExistingRepeat,
        JsonDocumentRole::ExistingFirst,
        JsonDocumentRole::ExistingRepeat,
    ),
    (
        ComparisonId::NewMutation,
        JsonDocumentRole::CurrentA,
        JsonDocumentRole::NewFirst,
    ),
    (
        ComparisonId::NewCollisionControl,
        JsonDocumentRole::NewSubmission,
        JsonDocumentRole::NewCollisionControl,
    ),
];

/// Hashes and semantic differences for one exact pair of documents.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExactComparisonEvidence {
    id: ComparisonId,
    before: JsonSourceIdentity,
    after: JsonSourceIdentity,
    raw_equal: bool,
    canonical_before_sha256: String,
    canonical_after_sha256: String,
    canonical_equal: bool,
    normalized_before_sha256: String,
    normalized_after_sha256: String,
    normalized_equal: bool,
    semantic_differences: Vec<JsonDifference>,
}

impl ExactComparisonEvidence {
    fn between(id: ComparisonId, before: &AnalyzedDocument, after: &AnalyzedDocument) -> Self {
        let canonical_before = before.json.canonical_pretty_json();
        let canonical_after = after.json.canonical_pretty_json();
        let normalized_before = before.json.normalized_pretty_json();
        let normalized_after = after.json.normalized_pretty_json();
        Self {
            id,
            before: before.identity.clone(),
            after: after.identity.clone(),
            raw_equal: before.raw == after.raw,
            canonical_before_sha256: sha256_bytes(canonical_before.as_bytes()),
            canonical_after_sha256: sha256_bytes(canonical_after.as_bytes()),
            canonical_equal: canonical_before == canonical_after,
            normalized_before_sha256: sha256_bytes(normalized_before.as_bytes()),
            normalized_after_sha256: sha256_bytes(normalized_after.as_bytes()),
            normalized_equal: normalized_before == normalized_after,
            semantic_differences: before.json.semantic_differences(&after.json),
        }
    }

    pub(crate) fn before(&self) -> &JsonSourceIdentity {
        &self.before
    }

    pub(crate) fn after(&self) -> &JsonSourceIdentity {
        &self.after
    }

    #[cfg(test)]
    pub(crate) fn raw_equal(&self) -> bool {
        self.raw_equal
    }

    pub(crate) fn canonical_hashes(&self) -> (&str, &str) {
        (&self.canonical_before_sha256, &self.canonical_after_sha256)
    }

    #[cfg(test)]
    pub(crate) fn canonical_equal(&self) -> bool {
        self.canonical_equal
    }

    pub(crate) fn normalized_hashes(&self) -> (&str, &str) {
        (
            &self.normalized_before_sha256,
            &self.normalized_after_sha256,
        )
    }

    #[cfg(test)]
    pub(crate) fn normalized_equal(&self) -> bool {
        self.normalized_equal
    }

    pub(crate) fn semantic_differences(&self) -> &[JsonDifference] {
        &self.semantic_differences
    }
}

/// Borrowed exact payloads for raw, canonical, and normalized comparison
/// artifacts. No lossy diff reconstruction is required by the writer.
pub(crate) struct ComparisonArtifactPayloads<'a> {
    pub(crate) raw_before: &'a [u8],
    pub(crate) raw_after: &'a [u8],
    pub(crate) canonical_before: &'a str,
    pub(crate) canonical_after: &'a str,
    pub(crate) normalized_before: &'a str,
    pub(crate) normalized_after: &'a str,
}

/// The three and only three semantic artifact views emitted by Phase 0A.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ComparisonArtifactKind {
    Roundtrip,
    ExistingImport,
    NewImport,
}

impl ComparisonArtifactKind {
    const fn comparisons(self) -> &'static [ComparisonId] {
        match self {
            Self::Roundtrip => &[
                ComparisonId::RoundTripA,
                ComparisonId::RoundTripB,
                ComparisonId::CurrentDeterminism,
                ComparisonId::ReconstructionDeterminism,
            ],
            Self::ExistingImport => &[
                ComparisonId::ReplacementFixture,
                ComparisonId::ExistingMutation,
                ComparisonId::ExistingRepeat,
            ],
            Self::NewImport => &[
                ComparisonId::NewFixture,
                ComparisonId::NewMutation,
                ComparisonId::NewCollisionControl,
            ],
        }
    }
}

/// Deterministic, bounded, fully content-addressed comparison artifact.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ComparisonArtifactView {
    format_version: u32,
    evidence_scope: EvidenceScope,
    kind: ComparisonArtifactKind,
    coverage: &'static str,
    approved_volatile_pointers: &'static [&'static str],
    comparisons: Vec<SerializableComparison>,
    new_animation_collision: Option<NewAnimationCollisionArtifact>,
    roundtrip_losses: Vec<DerivedRoundTripLoss>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct NewAnimationCollisionArtifact {
    requested_animation: String,
    renamed_animation: String,
    submitted_content_fingerprint: String,
    renamed_content_fingerprint: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SerializableComparison {
    id: &'static str,
    before: SerializableSourceIdentity,
    after: SerializableSourceIdentity,
    raw: TextFormComparison,
    canonical: TextFormComparison,
    normalized: TextFormComparison,
    semantic_differences: Vec<SerializableSemanticDifference>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SerializableSourceIdentity {
    role: &'static str,
    sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct TextFormComparison {
    equal: bool,
    before_sha256: String,
    after_sha256: String,
    textual_difference: BoundedTextDifference,
}

/// A complete deterministic textual change under one fixed byte ceiling.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct BoundedTextDifference {
    format: &'static str,
    common_prefix_bytes: usize,
    common_suffix_bytes: usize,
    complete_diff_text: String,
    complete_diff_bytes: usize,
    difference_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SerializableSemanticDifference {
    pointer: String,
    before_json: Option<String>,
    after_json: Option<String>,
    approved_volatile: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct DerivedRoundTripLoss {
    pointer: &'static str,
    description: &'static str,
    observed_in: Vec<&'static str>,
}

/// Proof that all non-project entries in the three source packages matched.
pub(crate) struct MatchingNonProjectPackages {
    entries: Vec<TreeEntry>,
    sha256: String,
}

impl MatchingNonProjectPackages {
    pub(crate) fn entries(&self) -> &[TreeEntry] {
        &self.entries
    }

    pub(crate) fn sha256(&self) -> &str {
        &self.sha256
    }
}

/// Unforgeable proof that all fixture, round-trip, import, repeat, identity,
/// and non-project package checks completed.
pub(crate) struct CompletedPhase0Analysis {
    case_sha256: String,
    documents: [AnalyzedDocument; DOCUMENT_COUNT],
    comparisons: Vec<ExactComparisonEvidence>,
    new_animation_collision: NewAnimationCollisionEvidence,
    packages: MatchingNonProjectPackages,
}

impl CompletedPhase0Analysis {
    pub(crate) fn case_sha256(&self) -> &str {
        &self.case_sha256
    }

    #[cfg(test)]
    pub(crate) fn source_identities(&self) -> impl Iterator<Item = &JsonSourceIdentity> {
        self.documents.iter().map(|document| &document.identity)
    }

    pub(crate) fn comparison(&self, id: ComparisonId) -> &ExactComparisonEvidence {
        self.comparisons
            .iter()
            .find(|comparison| comparison.id == id)
            .expect("every closed comparison is retained")
    }

    pub(crate) fn comparison_artifact_payloads(
        &self,
        id: ComparisonId,
    ) -> ComparisonArtifactPayloads<'_> {
        let comparison = self.comparison(id);
        let before = self.document(comparison.before.role);
        let after = self.document(comparison.after.role);
        ComparisonArtifactPayloads {
            raw_before: &before.raw,
            raw_after: &after.raw,
            canonical_before: before.json.canonical_pretty_json(),
            canonical_after: after.json.canonical_pretty_json(),
            normalized_before: before.json.normalized_pretty_json(),
            normalized_after: after.json.normalized_pretty_json(),
        }
    }

    pub(crate) fn matching_packages(&self) -> &MatchingNonProjectPackages {
        &self.packages
    }

    /// Returns the exact role-qualified identity retained by semantic analysis.
    pub(crate) fn source_identity(&self, role: JsonDocumentRole) -> &JsonSourceIdentity {
        &self.document(role).identity
    }

    /// Returns the exact validated JSON bytes for a closed document role.
    pub(crate) fn raw_document(&self, role: JsonDocumentRole) -> &[u8] {
        &self.document(role).raw
    }

    /// Builds one of the three fixed deterministic comparison artifacts.
    pub(crate) fn comparison_artifact_view(
        &self,
        kind: ComparisonArtifactKind,
    ) -> Result<ComparisonArtifactView, Phase0AnalysisError> {
        let comparisons = kind
            .comparisons()
            .iter()
            .copied()
            .map(|id| self.serializable_comparison(id))
            .collect::<Result<Vec<_>, _>>()?;
        let roundtrip_losses = if kind == ComparisonArtifactKind::Roundtrip {
            self.derived_roundtrip_losses()
        } else {
            Vec::new()
        };
        let new_animation_collision = if kind == ComparisonArtifactKind::NewImport {
            Some(self.new_animation_collision_artifact()?)
        } else {
            None
        };
        Ok(ComparisonArtifactView {
            format_version: COMPARISON_ARTIFACT_FORMAT_VERSION,
            evidence_scope: EvidenceScope::GenericRehearsal,
            kind,
            coverage: COMPARISON_COVERAGE,
            approved_volatile_pointers: &APPROVED_VOLATILE_POINTERS,
            comparisons,
            new_animation_collision,
            roundtrip_losses,
        })
    }

    fn new_animation_collision_artifact(
        &self,
    ) -> Result<NewAnimationCollisionArtifact, Phase0AnalysisError> {
        let requested = self.new_animation_collision.requested_animation();
        let renamed = self.new_animation_collision.renamed_animation();
        let submission = self.document(JsonDocumentRole::NewSubmission);
        let collision = self.document(JsonDocumentRole::NewCollisionControl);
        Ok(NewAnimationCollisionArtifact {
            requested_animation: requested.to_owned(),
            renamed_animation: renamed.to_owned(),
            submitted_content_fingerprint: content_fingerprint_for(
                submission,
                JsonDocumentRole::NewSubmission,
                requested,
            )?
            .to_owned(),
            renamed_content_fingerprint: content_fingerprint_for(
                collision,
                JsonDocumentRole::NewCollisionControl,
                renamed,
            )?
            .to_owned(),
        })
    }

    /// Serializes one fixed artifact using stable field and comparison order.
    pub(crate) fn comparison_artifact_bytes(
        &self,
        kind: ComparisonArtifactKind,
    ) -> Result<Vec<u8>, Phase0AnalysisError> {
        let mut bytes = serde_json::to_vec_pretty(&self.comparison_artifact_view(kind)?)
            .map_err(Phase0AnalysisError::ArtifactSerialization)?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    fn serializable_comparison(
        &self,
        id: ComparisonId,
    ) -> Result<SerializableComparison, Phase0AnalysisError> {
        let comparison = self.comparison(id);
        let payloads = self.comparison_artifact_payloads(id);
        let raw_before = std::str::from_utf8(payloads.raw_before)
            .expect("analyzed JSON was already validated as UTF-8");
        let raw_after = std::str::from_utf8(payloads.raw_after)
            .expect("analyzed JSON was already validated as UTF-8");
        let (canonical_before_sha256, canonical_after_sha256) = comparison.canonical_hashes();
        let (normalized_before_sha256, normalized_after_sha256) = comparison.normalized_hashes();
        Ok(SerializableComparison {
            id: id.slug(),
            before: SerializableSourceIdentity {
                role: comparison.before().role().slug(),
                sha256: comparison.before().sha256().to_owned(),
            },
            after: SerializableSourceIdentity {
                role: comparison.after().role().slug(),
                sha256: comparison.after().sha256().to_owned(),
            },
            raw: TextFormComparison::between(
                id,
                "raw",
                raw_before,
                raw_after,
                comparison.before().sha256(),
                comparison.after().sha256(),
            )?,
            canonical: TextFormComparison::between(
                id,
                "canonical",
                payloads.canonical_before,
                payloads.canonical_after,
                canonical_before_sha256,
                canonical_after_sha256,
            )?,
            normalized: TextFormComparison::between(
                id,
                "normalized",
                payloads.normalized_before,
                payloads.normalized_after,
                normalized_before_sha256,
                normalized_after_sha256,
            )?,
            semantic_differences: comparison
                .semantic_differences()
                .iter()
                .map(|difference| SerializableSemanticDifference {
                    pointer: difference.pointer().to_owned(),
                    before_json: difference.before_json().map(str::to_owned),
                    after_json: difference.after_json().map(str::to_owned),
                    approved_volatile: difference.approved_volatile(),
                })
                .collect(),
        })
    }

    fn derived_roundtrip_losses(&self) -> Vec<DerivedRoundTripLoss> {
        let observed_in = [ComparisonId::RoundTripA, ComparisonId::RoundTripB]
            .into_iter()
            .filter(|id| {
                self.comparison(*id)
                    .semantic_differences()
                    .iter()
                    .any(|difference| difference.pointer() == "/skeleton/hash")
            })
            .map(ComparisonId::slug)
            .collect::<Vec<_>>();
        if observed_in.is_empty() {
            Vec::new()
        } else {
            vec![DerivedRoundTripLoss {
                pointer: "/skeleton/hash",
                description: "The editor regenerated the exported skeleton hash during reconstruction; fixed policy treats only this observed string change as volatile.",
                observed_in,
            }]
        }
    }

    fn document(&self, role: JsonDocumentRole) -> &AnalyzedDocument {
        let document = &self.documents[role.index()];
        debug_assert_eq!(document.identity.role, role);
        document
    }
}

impl TextFormComparison {
    fn between(
        comparison: ComparisonId,
        form: &'static str,
        before: &str,
        after: &str,
        before_sha256: &str,
        after_sha256: &str,
    ) -> Result<Self, Phase0AnalysisError> {
        Ok(Self {
            equal: before == after,
            before_sha256: before_sha256.to_owned(),
            after_sha256: after_sha256.to_owned(),
            textual_difference: BoundedTextDifference::between(comparison, form, before, after)?,
        })
    }
}

impl BoundedTextDifference {
    fn between(
        comparison: ComparisonId,
        form: &'static str,
        before: &str,
        after: &str,
    ) -> Result<Self, Phase0AnalysisError> {
        let prefix = common_prefix_bytes(before, after);
        let suffix = common_suffix_bytes(&before[prefix..], &after[prefix..]);
        let before_changed = &before[prefix..before.len() - suffix];
        let after_changed = &after[prefix..after.len() - suffix];
        let complete_diff_text = complete_diff_text(prefix, suffix, before_changed, after_changed);
        if complete_diff_text.len() > MAX_COMPLETE_TEXT_DIFF_BYTES {
            return Err(Phase0AnalysisError::TextualDifferenceTooLarge { comparison, form });
        }
        let complete_diff_bytes = complete_diff_text.len();
        let difference_sha256 = sha256_bytes(complete_diff_text.as_bytes());
        Ok(Self {
            format: "spinal_text_diff_v1",
            common_prefix_bytes: prefix,
            common_suffix_bytes: suffix,
            complete_diff_text,
            complete_diff_bytes,
            difference_sha256,
        })
    }
}

fn complete_diff_text(
    common_prefix_bytes: usize,
    common_suffix_bytes: usize,
    before_changed: &str,
    after_changed: &str,
) -> String {
    format!(
        concat!(
            "spinal-text-diff-v1\n",
            "common-prefix-bytes: {}\n",
            "common-suffix-bytes: {}\n",
            "before-changed-json: {}\n",
            "after-changed-json: {}\n"
        ),
        common_prefix_bytes,
        common_suffix_bytes,
        serde_json::to_string(before_changed).expect("serializing a string cannot fail"),
        serde_json::to_string(after_changed).expect("serializing a string cannot fail"),
    )
}

fn common_prefix_bytes(before: &str, after: &str) -> usize {
    let mut length = before
        .as_bytes()
        .iter()
        .zip(after.as_bytes())
        .take_while(|(left, right)| left == right)
        .count();
    while length > 0 && (!before.is_char_boundary(length) || !after.is_char_boundary(length)) {
        length -= 1;
    }
    length
}

fn common_suffix_bytes(before: &str, after: &str) -> usize {
    let mut length = before
        .as_bytes()
        .iter()
        .rev()
        .zip(after.as_bytes().iter().rev())
        .take_while(|(left, right)| left == right)
        .count();
    while length > 0
        && (!before.is_char_boundary(before.len() - length)
            || !after.is_char_boundary(after.len() - length))
    {
        length -= 1;
    }
    length
}

/// Purely parses and validates every Phase 0A semantic input.
pub(crate) fn analyze_phase0(
    case: &LoadedCase,
    sources: Phase0JsonSources,
    package_inventories: &CasePackageInventories,
) -> Result<CompletedPhase0Analysis, Phase0AnalysisError> {
    let packages = compare_non_project_packages(case, package_inventories)?;
    let (ordered_sources, new_animation_collision) = sources.into_parts();
    let documents = ordered_sources
        .into_iter()
        .map(|(role, raw)| AnalyzedDocument::parse(role, raw))
        .collect::<Result<Vec<_>, _>>()?
        .try_into()
        .map_err(|_: Vec<AnalyzedDocument>| Phase0AnalysisError::InternalDocumentCount)?;
    validate_source_identities(&documents)?;
    validate_semantics(case, &documents, &new_animation_collision)?;
    let comparisons = COMPARISONS
        .into_iter()
        .map(|(id, before, after)| {
            ExactComparisonEvidence::between(
                id,
                document(&documents, before),
                document(&documents, after),
            )
        })
        .collect();
    Ok(CompletedPhase0Analysis {
        case_sha256: case.source_sha256().to_owned(),
        documents,
        comparisons,
        new_animation_collision,
        packages,
    })
}

/// Compares only package content other than each manifest-declared `.spine`
/// project. Project filenames and bytes may differ; every asset and directory
/// entry must remain exactly identical.
pub(crate) fn compare_non_project_packages(
    case: &LoadedCase,
    inventories: &CasePackageInventories,
) -> Result<MatchingNonProjectPackages, Phase0AnalysisError> {
    let manifest = case.manifest();
    let current = non_project_entries(
        PackageRole::Current,
        &inventories.current,
        &manifest.packages.current.project,
    )?;
    let replacement = non_project_entries(
        PackageRole::ReplacementSubmission,
        &inventories.replacement_submission,
        &manifest.packages.replacement_submission.project,
    )?;
    let new_submission = non_project_entries(
        PackageRole::NewSubmission,
        &inventories.new_submission,
        &manifest.packages.new_submission.project,
    )?;
    if current != replacement || current != new_submission {
        return Err(Phase0AnalysisError::NonProjectPackageMismatch);
    }
    Ok(MatchingNonProjectPackages {
        sha256: digest_entries(&current),
        entries: current,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PackageRole {
    Current,
    ReplacementSubmission,
    NewSubmission,
}

/// Fail-closed semantic or package comparison failure.
#[derive(Debug, Error)]
pub(crate) enum Phase0AnalysisError {
    #[error("could not parse required JSON document {role:?}: {source}")]
    Json {
        role: JsonDocumentRole,
        #[source]
        source: JsonEvidenceError,
    },
    #[error("the fixed JSON document inventory did not contain ten distinct role identities")]
    DuplicateSourceIdentity,
    #[error("current fixture must contain replacement animation `{0}`")]
    CurrentMissingReplacement(String),
    #[error("current fixture must not already contain new animation `{0}`")]
    CurrentAlreadyContainsNew(String),
    #[error("{role:?} setup data differs from its fixed reference document")]
    SetupMismatch { role: JsonDocumentRole },
    #[error("{role:?} has the wrong exact animation-name set")]
    AnimationSetMismatch { role: JsonDocumentRole },
    #[error("{role:?} animation `{animation}` has the wrong fingerprint")]
    AnimationMismatch {
        role: JsonDocumentRole,
        animation: String,
    },
    #[error("replacement submission did not meaningfully change `{0}`")]
    ReplacementUnchanged(String),
    #[error(
        "collision transcript requested `{observed}`, expected manifest animation `{expected}`"
    )]
    CollisionRequestMismatch { expected: String, observed: String },
    #[error("collision control renamed animation `{0}` did not match the transcript binding")]
    CollisionRenameMismatch(String),
    #[error("{comparison:?} contained a non-approved semantic difference at `{pointer}`")]
    UnapprovedDifference {
        comparison: ComparisonId,
        pointer: String,
    },
    #[error("{comparison:?} did not normalize to the required document")]
    NormalizedMismatch { comparison: ComparisonId },
    #[error("{comparison:?} was not byte-for-byte deterministic")]
    Nondeterministic { comparison: ComparisonId },
    #[error("invalid {role:?} package inventory: {reason}")]
    InvalidPackageInventory {
        role: PackageRole,
        reason: &'static str,
    },
    #[error("current, replacement, and new packages differ outside their declared projects")]
    NonProjectPackageMismatch,
    #[error("could not serialize the fixed comparison artifact: {0}")]
    ArtifactSerialization(serde_json::Error),
    #[error(
        "the complete {form} textual difference for {comparison:?} exceeds the fixed byte ceiling"
    )]
    TextualDifferenceTooLarge {
        comparison: ComparisonId,
        form: &'static str,
    },
    #[error("internal fixed JSON document count was not ten")]
    InternalDocumentCount,
}

fn validate_source_identities(
    documents: &[AnalyzedDocument; DOCUMENT_COUNT],
) -> Result<(), Phase0AnalysisError> {
    let identities = documents
        .iter()
        .map(|document| document.identity.clone())
        .collect::<BTreeSet<_>>();
    if identities.len() != DOCUMENT_COUNT
        || documents
            .iter()
            .zip(JsonDocumentRole::ORDER)
            .any(|(document, role)| document.identity.role != role)
    {
        return Err(Phase0AnalysisError::DuplicateSourceIdentity);
    }
    Ok(())
}

fn validate_semantics(
    case: &LoadedCase,
    documents: &[AnalyzedDocument; DOCUMENT_COUNT],
    collision_evidence: &NewAnimationCollisionEvidence,
) -> Result<(), Phase0AnalysisError> {
    let replacement_name = &case.manifest().animations.replacement;
    let new_name = &case.manifest().animations.new;
    let current = document(documents, JsonDocumentRole::CurrentA);
    let replacement = document(documents, JsonDocumentRole::ReplacementSubmission);
    let new_submission = document(documents, JsonDocumentRole::NewSubmission);

    if !current
        .json
        .animation_fingerprints()
        .contains_key(replacement_name)
    {
        return Err(Phase0AnalysisError::CurrentMissingReplacement(
            replacement_name.clone(),
        ));
    }
    if current.json.animation_fingerprints().contains_key(new_name) {
        return Err(Phase0AnalysisError::CurrentAlreadyContainsNew(
            new_name.clone(),
        ));
    }

    require_same_setup(
        current,
        replacement,
        JsonDocumentRole::ReplacementSubmission,
    )?;
    require_exact_animation_set(
        current,
        replacement,
        JsonDocumentRole::ReplacementSubmission,
    )?;
    for (name, fingerprint) in current.json.animation_fingerprints() {
        let replacement_fingerprint =
            fingerprint_for(replacement, JsonDocumentRole::ReplacementSubmission, name)?;
        if name == replacement_name {
            if replacement_fingerprint == fingerprint {
                return Err(Phase0AnalysisError::ReplacementUnchanged(name.clone()));
            }
        } else if replacement_fingerprint != fingerprint {
            return Err(Phase0AnalysisError::AnimationMismatch {
                role: JsonDocumentRole::ReplacementSubmission,
                animation: name.clone(),
            });
        }
    }
    require_differences_within_animation(
        current,
        replacement,
        ComparisonId::ReplacementFixture,
        replacement_name,
    )?;

    require_same_setup(current, new_submission, JsonDocumentRole::NewSubmission)?;
    require_current_plus_new(
        current,
        new_submission,
        JsonDocumentRole::NewSubmission,
        new_name,
    )?;
    require_differences_within_animation(
        current,
        new_submission,
        ComparisonId::NewFixture,
        new_name,
    )?;

    validate_round_trip(
        document(documents, JsonDocumentRole::CurrentA),
        document(documents, JsonDocumentRole::ReconstructedA),
        ComparisonId::RoundTripA,
        JsonDocumentRole::ReconstructedA,
    )?;
    validate_round_trip(
        document(documents, JsonDocumentRole::CurrentB),
        document(documents, JsonDocumentRole::ReconstructedB),
        ComparisonId::RoundTripB,
        JsonDocumentRole::ReconstructedB,
    )?;
    require_raw_equal(
        document(documents, JsonDocumentRole::CurrentA),
        document(documents, JsonDocumentRole::CurrentB),
        ComparisonId::CurrentDeterminism,
    )?;
    require_raw_equal(
        document(documents, JsonDocumentRole::ReconstructedA),
        document(documents, JsonDocumentRole::ReconstructedB),
        ComparisonId::ReconstructionDeterminism,
    )?;

    validate_existing_candidate(
        current,
        replacement,
        document(documents, JsonDocumentRole::ExistingFirst),
        replacement_name,
    )?;
    require_raw_equal(
        document(documents, JsonDocumentRole::ExistingFirst),
        document(documents, JsonDocumentRole::ExistingRepeat),
        ComparisonId::ExistingRepeat,
    )?;

    validate_new_candidate(
        current,
        new_submission,
        document(documents, JsonDocumentRole::NewFirst),
        new_name,
    )?;
    if collision_evidence.requested_animation() != new_name {
        return Err(Phase0AnalysisError::CollisionRequestMismatch {
            expected: new_name.clone(),
            observed: collision_evidence.requested_animation().to_owned(),
        });
    }
    validate_new_collision_control(
        new_submission,
        document(documents, JsonDocumentRole::NewCollisionControl),
        collision_evidence,
    )?;
    Ok(())
}

fn validate_round_trip(
    current: &AnalyzedDocument,
    reconstructed: &AnalyzedDocument,
    comparison: ComparisonId,
    role: JsonDocumentRole,
) -> Result<(), Phase0AnalysisError> {
    require_same_setup(current, reconstructed, role)?;
    require_exact_animation_set(current, reconstructed, role)?;
    for (name, fingerprint) in current.json.animation_fingerprints() {
        if fingerprint_for(reconstructed, role, name)? != fingerprint {
            return Err(Phase0AnalysisError::AnimationMismatch {
                role,
                animation: name.clone(),
            });
        }
    }
    if current.json.normalized_pretty_json() != reconstructed.json.normalized_pretty_json() {
        return Err(Phase0AnalysisError::NormalizedMismatch { comparison });
    }
    if let Some(difference) = current
        .json
        .semantic_differences(&reconstructed.json)
        .into_iter()
        .find(|difference| !difference.approved_volatile())
    {
        return Err(Phase0AnalysisError::UnapprovedDifference {
            comparison,
            pointer: difference.pointer().to_owned(),
        });
    }
    Ok(())
}

fn validate_existing_candidate(
    current: &AnalyzedDocument,
    submission: &AnalyzedDocument,
    candidate: &AnalyzedDocument,
    replacement: &str,
) -> Result<(), Phase0AnalysisError> {
    let role = JsonDocumentRole::ExistingFirst;
    require_same_setup(current, candidate, role)?;
    require_exact_animation_set(current, candidate, role)?;
    for (name, current_fingerprint) in current.json.animation_fingerprints() {
        let candidate_fingerprint = fingerprint_for(candidate, role, name)?;
        let expected = if name == replacement {
            fingerprint_for(submission, JsonDocumentRole::ReplacementSubmission, name)?
        } else {
            current_fingerprint
        };
        if candidate_fingerprint != expected {
            return Err(Phase0AnalysisError::AnimationMismatch {
                role,
                animation: name.clone(),
            });
        }
    }
    require_differences_within_animation(
        current,
        candidate,
        ComparisonId::ExistingMutation,
        replacement,
    )
}

fn validate_new_candidate(
    current: &AnalyzedDocument,
    submission: &AnalyzedDocument,
    candidate: &AnalyzedDocument,
    new_name: &str,
) -> Result<(), Phase0AnalysisError> {
    let role = JsonDocumentRole::NewFirst;
    require_same_setup(current, candidate, role)?;
    require_current_plus_new(current, candidate, role, new_name)?;
    if fingerprint_for(candidate, role, new_name)?
        != fingerprint_for(submission, JsonDocumentRole::NewSubmission, new_name)?
    {
        return Err(Phase0AnalysisError::AnimationMismatch {
            role,
            animation: new_name.to_owned(),
        });
    }
    require_differences_within_animation(current, candidate, ComparisonId::NewMutation, new_name)
}

fn validate_new_collision_control(
    submission: &AnalyzedDocument,
    collision: &AnalyzedDocument,
    evidence: &NewAnimationCollisionEvidence,
) -> Result<(), Phase0AnalysisError> {
    let role = JsonDocumentRole::NewCollisionControl;
    let requested = evidence.requested_animation();
    let renamed = evidence.renamed_animation();
    require_same_setup(submission, collision, role)?;

    let submission_animations = submission.json.animation_fingerprints();
    let collision_animations = collision.json.animation_fingerprints();
    if requested == renamed
        || submission_animations.contains_key(renamed)
        || collision_animations.len() != submission_animations.len() + 1
        || !collision_animations.contains_key(renamed)
    {
        return Err(Phase0AnalysisError::CollisionRenameMismatch(
            renamed.to_owned(),
        ));
    }
    for (name, fingerprint) in submission_animations {
        if collision_animations.get(name) != Some(fingerprint) {
            return Err(Phase0AnalysisError::AnimationMismatch {
                role,
                animation: name.clone(),
            });
        }
    }
    if content_fingerprint_for(submission, JsonDocumentRole::NewSubmission, requested)?
        != content_fingerprint_for(collision, role, renamed)?
    {
        return Err(Phase0AnalysisError::AnimationMismatch {
            role,
            animation: renamed.to_owned(),
        });
    }
    require_differences_within_animation(
        submission,
        collision,
        ComparisonId::NewCollisionControl,
        renamed,
    )
}

fn require_same_setup(
    current: &AnalyzedDocument,
    other: &AnalyzedDocument,
    role: JsonDocumentRole,
) -> Result<(), Phase0AnalysisError> {
    if current.json.setup_fingerprint() != other.json.setup_fingerprint() {
        Err(Phase0AnalysisError::SetupMismatch { role })
    } else {
        Ok(())
    }
}

fn require_exact_animation_set(
    current: &AnalyzedDocument,
    other: &AnalyzedDocument,
    role: JsonDocumentRole,
) -> Result<(), Phase0AnalysisError> {
    let current_names = current
        .json
        .animation_fingerprints()
        .keys()
        .collect::<BTreeSet<_>>();
    let other_names = other
        .json
        .animation_fingerprints()
        .keys()
        .collect::<BTreeSet<_>>();
    if current_names != other_names {
        Err(Phase0AnalysisError::AnimationSetMismatch { role })
    } else {
        Ok(())
    }
}

fn require_current_plus_new(
    current: &AnalyzedDocument,
    other: &AnalyzedDocument,
    role: JsonDocumentRole,
    new_name: &str,
) -> Result<(), Phase0AnalysisError> {
    let current_map = current.json.animation_fingerprints();
    let other_map = other.json.animation_fingerprints();
    if other_map.len() != current_map.len() + 1 || !other_map.contains_key(new_name) {
        return Err(Phase0AnalysisError::AnimationSetMismatch { role });
    }
    for (name, fingerprint) in current_map {
        if other_map.get(name) != Some(fingerprint) {
            return Err(Phase0AnalysisError::AnimationMismatch {
                role,
                animation: name.clone(),
            });
        }
    }
    Ok(())
}

fn require_differences_within_animation(
    before: &AnalyzedDocument,
    after: &AnalyzedDocument,
    comparison: ComparisonId,
    animation: &str,
) -> Result<(), Phase0AnalysisError> {
    let prefix = format!("/animations/{}", escape_pointer_token(animation));
    for difference in before.json.semantic_differences(&after.json) {
        let inside_animation = difference.pointer() == prefix
            || difference
                .pointer()
                .strip_prefix(&prefix)
                .is_some_and(|suffix| suffix.starts_with('/'));
        if !(inside_animation || difference.approved_volatile()) {
            return Err(Phase0AnalysisError::UnapprovedDifference {
                comparison,
                pointer: difference.pointer().to_owned(),
            });
        }
    }
    Ok(())
}

fn require_raw_equal(
    before: &AnalyzedDocument,
    after: &AnalyzedDocument,
    comparison: ComparisonId,
) -> Result<(), Phase0AnalysisError> {
    if before.raw == after.raw {
        Ok(())
    } else {
        Err(Phase0AnalysisError::Nondeterministic { comparison })
    }
}

fn fingerprint_for<'a>(
    document: &'a AnalyzedDocument,
    role: JsonDocumentRole,
    animation: &str,
) -> Result<&'a str, Phase0AnalysisError> {
    document
        .json
        .animation_fingerprints()
        .get(animation)
        .map(String::as_str)
        .ok_or(Phase0AnalysisError::AnimationSetMismatch { role })
}

fn content_fingerprint_for<'a>(
    document: &'a AnalyzedDocument,
    role: JsonDocumentRole,
    animation: &str,
) -> Result<&'a str, Phase0AnalysisError> {
    document
        .json
        .animation_content_fingerprints()
        .get(animation)
        .map(String::as_str)
        .ok_or(Phase0AnalysisError::AnimationSetMismatch { role })
}

fn document(
    documents: &[AnalyzedDocument; DOCUMENT_COUNT],
    role: JsonDocumentRole,
) -> &AnalyzedDocument {
    let document = &documents[role.index()];
    debug_assert_eq!(document.identity.role, role);
    document
}

fn escape_pointer_token(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn non_project_entries(
    role: PackageRole,
    inventory: &PackageInventory,
    project: &Path,
) -> Result<Vec<TreeEntry>, Phase0AnalysisError> {
    validate_inventory_shape(role, inventory)?;
    let project = project
        .to_str()
        .ok_or(Phase0AnalysisError::InvalidPackageInventory {
            role,
            reason: "declared project path is not UTF-8",
        })?
        .replace(std::path::MAIN_SEPARATOR, "/");
    let matches = inventory
        .entries
        .iter()
        .filter(|entry| entry.path == project)
        .collect::<Vec<_>>();
    if !matches!(matches.as_slice(), [entry] if entry.kind == EntryKind::File) {
        return Err(Phase0AnalysisError::InvalidPackageInventory {
            role,
            reason: "declared project file is absent, duplicated, or not a file",
        });
    }
    Ok(inventory
        .entries
        .iter()
        .filter(|entry| entry.path != project)
        .cloned()
        .collect())
}

fn validate_inventory_shape(
    role: PackageRole,
    inventory: &PackageInventory,
) -> Result<(), Phase0AnalysisError> {
    if !valid_sha256(&inventory.tree_sha256)
        || inventory.entries.first().is_none_or(|entry| {
            entry.path != "."
                || entry.kind != EntryKind::Directory
                || entry.size != 0
                || entry.sha256.is_some()
        })
    {
        return invalid_inventory(role, "tree digest or root directory entry is invalid");
    }
    let mut previous: Option<&str> = None;
    for entry in &inventory.entries {
        if previous.is_some_and(|value| value >= entry.path.as_str()) {
            return invalid_inventory(role, "entries are not uniquely sorted");
        }
        previous = Some(&entry.path);
        if !valid_inventory_path(&entry.path) {
            return invalid_inventory(role, "entry path is not portable");
        }
        let valid_shape = match entry.kind {
            EntryKind::Directory => entry.size == 0 && entry.sha256.is_none(),
            EntryKind::File => entry.sha256.as_deref().is_some_and(valid_sha256),
        };
        if !valid_shape {
            return invalid_inventory(role, "entry kind, size, or digest is inconsistent");
        }
    }
    Ok(())
}

fn valid_inventory_path(path: &str) -> bool {
    path == "."
        || (!path.is_empty()
            && !path.starts_with('/')
            && !path.ends_with('/')
            && !path.contains('\\')
            && !path.contains('\0')
            && path
                .split('/')
                .all(|component| !component.is_empty() && component != "." && component != ".."))
}

fn invalid_inventory<T>(role: PackageRole, reason: &'static str) -> Result<T, Phase0AnalysisError> {
    Err(Phase0AnalysisError::InvalidPackageInventory { role, reason })
}

fn digest_entries(entries: &[TreeEntry]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"spinal-phase0a/non-project-package/v1\0");
    hasher.update((entries.len() as u64).to_be_bytes());
    for entry in entries {
        hasher.update((entry.path.len() as u64).to_be_bytes());
        hasher.update(entry.path.as_bytes());
        hasher.update([match entry.kind {
            EntryKind::Directory => b'd',
            EntryKind::File => b'f',
        }]);
        hasher.update(entry.size.to_be_bytes());
        if let Some(digest) = &entry.sha256 {
            hasher.update((digest.len() as u64).to_be_bytes());
            hasher.update(digest.as_bytes());
        } else {
            hasher.update(0_u64.to_be_bytes());
        }
    }
    hex_digest(hasher.finalize().as_slice())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::case::parse_case;
    use serde_json::{Map, Value, json};
    use std::collections::BTreeMap;

    #[derive(Clone)]
    struct Fixture {
        case: LoadedCase,
        documents: BTreeMap<JsonDocumentRole, Vec<u8>>,
        packages: CasePackageInventories,
    }

    impl Fixture {
        fn valid() -> Self {
            let case = parse_case(
                r#"
format_version = 2
case_id = "semantic-analysis"
target_spine_version = "4.3.23"
runtime_atlas = "character.atlas"

[editor]
expected_executable_sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[packages.current]
root = "/external/current"
project = "character.spine"
required_directories = ["images"]
asset_roots = ["images"]

[packages.replacement_submission]
root = "/external/replacement"
project = "replacement.spine"
required_directories = ["images"]
asset_roots = ["images"]

[packages.new_submission]
root = "/external/new"
project = "new.spine"
required_directories = ["images"]
asset_roots = ["images"]

[skeletons]
current = "Character"
replacement_submission = "Character"
new_submission = "Character"

[animations]
replacement = "idle"
new = "gesture"

[export]
preset = "pretty-nonessential-json"

[volatile]
approved_json_pointers = ["/skeleton/hash"]
"#,
            )
            .expect("valid case");

            let current = project(
                "current-hash",
                [("idle", animation(0)), ("walk", animation(1))],
            );
            let replacement = project(
                "replacement-hash",
                [("idle", animation(10)), ("walk", animation(1))],
            );
            let new_submission = project(
                "new-submission-hash",
                [
                    ("gesture", animation(20)),
                    ("idle", animation(0)),
                    ("walk", animation(1)),
                ],
            );
            let mut reconstructed = current.clone();
            reconstructed["skeleton"]["hash"] = json!("reconstructed-hash");
            let mut existing = current.clone();
            existing["skeleton"]["hash"] = json!("existing-hash");
            existing["animations"]["idle"] = animation(10);
            let mut added = current.clone();
            added["skeleton"]["hash"] = json!("new-candidate-hash");
            added["animations"]["gesture"] = animation(20);
            let mut collision = new_submission.clone();
            collision["skeleton"]["hash"] = json!("new-collision-control-hash");
            collision["animations"]["gesture2"] = animation(20);

            let documents = BTreeMap::from([
                (JsonDocumentRole::CurrentA, bytes(&current)),
                (JsonDocumentRole::ReplacementSubmission, bytes(&replacement)),
                (JsonDocumentRole::NewSubmission, bytes(&new_submission)),
                (JsonDocumentRole::ReconstructedA, bytes(&reconstructed)),
                (JsonDocumentRole::CurrentB, bytes(&current)),
                (JsonDocumentRole::ReconstructedB, bytes(&reconstructed)),
                (JsonDocumentRole::ExistingFirst, bytes(&existing)),
                (JsonDocumentRole::ExistingRepeat, bytes(&existing)),
                (JsonDocumentRole::NewFirst, bytes(&added)),
                (JsonDocumentRole::NewCollisionControl, bytes(&collision)),
            ]);
            let packages = CasePackageInventories {
                current: inventory("character.spine", "current-project"),
                replacement_submission: inventory("replacement.spine", "replacement-project"),
                new_submission: inventory("new.spine", "new-project"),
            };
            Self {
                case,
                documents,
                packages,
            }
        }

        fn sources(&self) -> Phase0JsonSources {
            Phase0JsonSources {
                current_a: self.source(JsonDocumentRole::CurrentA),
                replacement_submission: self.source(JsonDocumentRole::ReplacementSubmission),
                new_submission: self.source(JsonDocumentRole::NewSubmission),
                reconstructed_a: self.source(JsonDocumentRole::ReconstructedA),
                current_b: self.source(JsonDocumentRole::CurrentB),
                reconstructed_b: self.source(JsonDocumentRole::ReconstructedB),
                existing_first: self.source(JsonDocumentRole::ExistingFirst),
                existing_repeat: self.source(JsonDocumentRole::ExistingRepeat),
                new_first: self.source(JsonDocumentRole::NewFirst),
                new_collision_control: self.source(JsonDocumentRole::NewCollisionControl),
                new_animation_collision: NewAnimationCollisionEvidence::for_test(
                    "gesture", "gesture2",
                ),
            }
        }

        fn source(&self, role: JsonDocumentRole) -> Vec<u8> {
            self.documents.get(&role).expect("document role").clone()
        }

        fn analyze(&self) -> Result<CompletedPhase0Analysis, Phase0AnalysisError> {
            analyze_phase0(&self.case, self.sources(), &self.packages)
        }

        fn mutate(&mut self, role: JsonDocumentRole, mutation: impl FnOnce(&mut Value)) {
            let mut value: Value =
                serde_json::from_slice(self.documents.get(&role).expect("document role"))
                    .expect("fixture JSON");
            mutation(&mut value);
            self.documents.insert(role, bytes(&value));
        }

        fn mirror(&mut self, from: JsonDocumentRole, to: JsonDocumentRole) {
            let source = self.source(from);
            self.documents.insert(to, source);
        }
    }

    fn project<const N: usize>(hash: &str, values: [(&str, Value); N]) -> Value {
        let animations = values
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect::<Map<_, _>>();
        json!({
            "skeleton": {"hash": hash, "spine": "4.3.23", "x": 0, "y": 0},
            "bones": [{"name": "root"}],
            "slots": [{"name": "body", "bone": "root"}],
            "skins": [{"name": "default", "attachments": {}}],
            "animations": animations,
        })
    }

    fn animation(value: i64) -> Value {
        json!({"bones": {"root": {"rotate": [{"time": 0, "value": value}]}}})
    }

    fn bytes(value: &Value) -> Vec<u8> {
        serde_json::to_vec_pretty(value).expect("serialize fixture")
    }

    fn animations(value: &mut Value) -> &mut Map<String, Value> {
        value
            .get_mut("animations")
            .and_then(Value::as_object_mut)
            .expect("animations object")
    }

    fn inventory(project: &str, project_bytes: &str) -> PackageInventory {
        let mut entries = vec![
            TreeEntry {
                path: ".".to_owned(),
                kind: EntryKind::Directory,
                size: 0,
                sha256: None,
            },
            TreeEntry {
                path: "images".to_owned(),
                kind: EntryKind::Directory,
                size: 0,
                sha256: None,
            },
            TreeEntry {
                path: "images/cat.png".to_owned(),
                kind: EntryKind::File,
                size: 7,
                sha256: Some(sha256_bytes(b"texture")),
            },
            TreeEntry {
                path: project.to_owned(),
                kind: EntryKind::File,
                size: project_bytes.len() as u64,
                sha256: Some(sha256_bytes(project_bytes.as_bytes())),
            },
        ];
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        PackageInventory {
            tree_sha256: sha256_bytes(format!("tree:{project}:{project_bytes}").as_bytes()),
            entries,
        }
    }

    #[test]
    fn complete_fixture_mints_evidence_with_role_qualified_duplicate_hashes() {
        let fixture = Fixture::valid();
        let completed = fixture.analyze().expect("valid semantic fixture");
        let identities = completed.source_identities().collect::<Vec<_>>();
        assert_eq!(identities.len(), DOCUMENT_COUNT);
        assert_eq!(
            identities
                .iter()
                .map(|identity| identity.role())
                .collect::<BTreeSet<_>>()
                .len(),
            DOCUMENT_COUNT
        );
        assert!(
            identities
                .iter()
                .map(|identity| identity.sha256())
                .collect::<BTreeSet<_>>()
                .len()
                < DOCUMENT_COUNT,
            "repeat roles intentionally share exact content hashes"
        );

        let comparison = completed.comparison(ComparisonId::RoundTripA);
        assert!(!comparison.raw_equal());
        assert!(!comparison.canonical_equal());
        assert!(comparison.normalized_equal());
        assert_eq!(comparison.semantic_differences().len(), 1);
        assert!(comparison.semantic_differences()[0].approved_volatile());
        assert_ne!(
            comparison.canonical_hashes().0,
            comparison.canonical_hashes().1
        );
        assert_eq!(
            comparison.normalized_hashes().0,
            comparison.normalized_hashes().1
        );
        assert_eq!(comparison.before().role(), JsonDocumentRole::CurrentA);
        assert_eq!(comparison.after().role(), JsonDocumentRole::ReconstructedA);

        let payloads = completed.comparison_artifact_payloads(ComparisonId::RoundTripA);
        assert_eq!(
            payloads.raw_before,
            fixture.source(JsonDocumentRole::CurrentA)
        );
        assert_eq!(
            payloads.raw_after,
            fixture.source(JsonDocumentRole::ReconstructedA)
        );
        assert_ne!(payloads.canonical_before, payloads.canonical_after);
        assert_eq!(payloads.normalized_before, payloads.normalized_after);
        assert!(!completed.matching_packages().entries().is_empty());
        assert!(valid_sha256(completed.matching_packages().sha256()));
    }

    #[test]
    fn malformed_json_in_every_required_role_is_rejected_with_that_role() {
        for role in JsonDocumentRole::ORDER {
            let mut fixture = Fixture::valid();
            fixture.documents.insert(role, b"{".to_vec());
            assert!(
                matches!(
                    fixture.analyze(),
                    Err(Phase0AnalysisError::Json {
                        role: actual,
                        ..
                    }) if actual == role
                ),
                "malformed {role:?} was accepted"
            );
        }
    }

    #[test]
    fn current_fixture_must_make_both_import_cases_meaningful() {
        let mut missing_replacement = Fixture::valid();
        missing_replacement.mutate(JsonDocumentRole::CurrentA, |value| {
            animations(value).remove("idle");
        });
        assert!(matches!(
            missing_replacement.analyze(),
            Err(Phase0AnalysisError::CurrentMissingReplacement(_))
        ));

        let mut already_new = Fixture::valid();
        already_new.mutate(JsonDocumentRole::CurrentA, |value| {
            animations(value).insert("gesture".to_owned(), animation(20));
        });
        assert!(matches!(
            already_new.analyze(),
            Err(Phase0AnalysisError::CurrentAlreadyContainsNew(_))
        ));
    }

    #[test]
    fn replacement_submission_must_change_exactly_the_named_animation() {
        let mut setup = Fixture::valid();
        setup.mutate(JsonDocumentRole::ReplacementSubmission, |value| {
            value["bones"][0]["x"] = json!(1);
        });
        assert!(matches!(
            setup.analyze(),
            Err(Phase0AnalysisError::SetupMismatch {
                role: JsonDocumentRole::ReplacementSubmission
            })
        ));

        let mut wrong_set = Fixture::valid();
        wrong_set.mutate(JsonDocumentRole::ReplacementSubmission, |value| {
            animations(value).remove("walk");
        });
        assert!(matches!(
            wrong_set.analyze(),
            Err(Phase0AnalysisError::AnimationSetMismatch {
                role: JsonDocumentRole::ReplacementSubmission
            })
        ));

        let mut unchanged = Fixture::valid();
        unchanged.mutate(JsonDocumentRole::ReplacementSubmission, |value| {
            animations(value).insert("idle".to_owned(), animation(0));
        });
        assert!(matches!(
            unchanged.analyze(),
            Err(Phase0AnalysisError::ReplacementUnchanged(_))
        ));

        let mut changed_other = Fixture::valid();
        changed_other.mutate(JsonDocumentRole::ReplacementSubmission, |value| {
            animations(value).insert("walk".to_owned(), animation(99));
        });
        assert!(matches!(
            changed_other.analyze(),
            Err(Phase0AnalysisError::AnimationMismatch {
                role: JsonDocumentRole::ReplacementSubmission,
                ..
            })
        ));
    }

    #[test]
    fn new_submission_must_add_exactly_one_animation_and_preserve_every_prior_one() {
        let mut setup = Fixture::valid();
        setup.mutate(JsonDocumentRole::NewSubmission, |value| {
            value["slots"][0]["name"] = json!("changed");
        });
        assert!(matches!(
            setup.analyze(),
            Err(Phase0AnalysisError::SetupMismatch {
                role: JsonDocumentRole::NewSubmission
            })
        ));

        for mutation in ["missing", "extra"] {
            let mut fixture = Fixture::valid();
            fixture.mutate(JsonDocumentRole::NewSubmission, |value| {
                if mutation == "missing" {
                    animations(value).remove("gesture");
                } else {
                    animations(value).insert("surprise".to_owned(), animation(30));
                }
            });
            assert!(matches!(
                fixture.analyze(),
                Err(Phase0AnalysisError::AnimationSetMismatch {
                    role: JsonDocumentRole::NewSubmission
                })
            ));
        }

        let mut prior_changed = Fixture::valid();
        prior_changed.mutate(JsonDocumentRole::NewSubmission, |value| {
            animations(value).insert("idle".to_owned(), animation(90));
        });
        assert!(matches!(
            prior_changed.analyze(),
            Err(Phase0AnalysisError::AnimationMismatch {
                role: JsonDocumentRole::NewSubmission,
                ..
            })
        ));
    }

    #[test]
    fn round_trip_allows_only_the_fixed_string_hash_difference() {
        let mut setup_change = Fixture::valid();
        setup_change.mutate(JsonDocumentRole::ReconstructedA, |value| {
            value["bones"][0]["x"] = json!(1);
        });
        assert!(matches!(
            setup_change.analyze(),
            Err(Phase0AnalysisError::SetupMismatch {
                role: JsonDocumentRole::ReconstructedA
            })
        ));

        let mut animation_change = Fixture::valid();
        animation_change.mutate(JsonDocumentRole::ReconstructedA, |value| {
            animations(value).insert("walk".to_owned(), animation(99));
        });
        assert!(matches!(
            animation_change.analyze(),
            Err(Phase0AnalysisError::AnimationMismatch {
                role: JsonDocumentRole::ReconstructedA,
                ..
            })
        ));

        for invalid_hash in [json!(7), Value::Null] {
            let mut fixture = Fixture::valid();
            fixture.mutate(JsonDocumentRole::ReconstructedA, |value| {
                value["skeleton"]["hash"] = invalid_hash;
            });
            assert!(matches!(
                fixture.analyze(),
                Err(Phase0AnalysisError::UnapprovedDifference {
                    comparison: ComparisonId::RoundTripA,
                    ..
                })
            ));
        }
    }

    #[test]
    fn both_source_and_reconstruction_rehearsals_must_be_byte_deterministic() {
        let mut current = Fixture::valid();
        current
            .documents
            .get_mut(&JsonDocumentRole::CurrentB)
            .expect("current B")
            .push(b'\n');
        assert!(matches!(
            current.analyze(),
            Err(Phase0AnalysisError::Nondeterministic {
                comparison: ComparisonId::CurrentDeterminism
            })
        ));

        let mut reconstructed = Fixture::valid();
        reconstructed
            .documents
            .get_mut(&JsonDocumentRole::ReconstructedB)
            .expect("reconstructed B")
            .push(b'\n');
        assert!(matches!(
            reconstructed.analyze(),
            Err(Phase0AnalysisError::Nondeterministic {
                comparison: ComparisonId::ReconstructionDeterminism
            })
        ));
    }

    #[test]
    fn existing_candidate_may_only_adopt_the_submitted_replacement() {
        let mut setup = Fixture::valid();
        setup.mutate(JsonDocumentRole::ExistingFirst, |value| {
            value["bones"][0]["x"] = json!(1);
        });
        setup.mirror(
            JsonDocumentRole::ExistingFirst,
            JsonDocumentRole::ExistingRepeat,
        );
        assert!(matches!(
            setup.analyze(),
            Err(Phase0AnalysisError::SetupMismatch {
                role: JsonDocumentRole::ExistingFirst
            })
        ));

        let mut wrong_set = Fixture::valid();
        wrong_set.mutate(JsonDocumentRole::ExistingFirst, |value| {
            animations(value).insert("extra".to_owned(), animation(4));
        });
        wrong_set.mirror(
            JsonDocumentRole::ExistingFirst,
            JsonDocumentRole::ExistingRepeat,
        );
        assert!(matches!(
            wrong_set.analyze(),
            Err(Phase0AnalysisError::AnimationSetMismatch {
                role: JsonDocumentRole::ExistingFirst
            })
        ));

        for (name, value) in [("idle", 11), ("walk", 12)] {
            let mut fixture = Fixture::valid();
            fixture.mutate(JsonDocumentRole::ExistingFirst, |document| {
                animations(document).insert(name.to_owned(), animation(value));
            });
            fixture.mirror(
                JsonDocumentRole::ExistingFirst,
                JsonDocumentRole::ExistingRepeat,
            );
            assert!(matches!(
                fixture.analyze(),
                Err(Phase0AnalysisError::AnimationMismatch {
                    role: JsonDocumentRole::ExistingFirst,
                    ..
                })
            ));
        }

        let mut invalid_hash = Fixture::valid();
        invalid_hash.mutate(JsonDocumentRole::ExistingFirst, |value| {
            value["skeleton"]["hash"] = json!(5);
        });
        invalid_hash.mirror(
            JsonDocumentRole::ExistingFirst,
            JsonDocumentRole::ExistingRepeat,
        );
        assert!(matches!(
            invalid_hash.analyze(),
            Err(Phase0AnalysisError::UnapprovedDifference {
                comparison: ComparisonId::ExistingMutation,
                ..
            })
        ));
    }

    #[test]
    fn new_candidate_may_only_adopt_the_submitted_new_animation() {
        let mut setup = Fixture::valid();
        setup.mutate(JsonDocumentRole::NewFirst, |value| {
            value["slots"][0]["bone"] = json!("changed");
        });
        assert!(matches!(
            setup.analyze(),
            Err(Phase0AnalysisError::SetupMismatch {
                role: JsonDocumentRole::NewFirst
            })
        ));

        let mut wrong_added = Fixture::valid();
        wrong_added.mutate(JsonDocumentRole::NewFirst, |value| {
            animations(value).insert("gesture".to_owned(), animation(21));
        });
        assert!(matches!(
            wrong_added.analyze(),
            Err(Phase0AnalysisError::AnimationMismatch {
                role: JsonDocumentRole::NewFirst,
                ..
            })
        ));

        let mut prior_changed = Fixture::valid();
        prior_changed.mutate(JsonDocumentRole::NewFirst, |value| {
            animations(value).insert("walk".to_owned(), animation(22));
        });
        assert!(matches!(
            prior_changed.analyze(),
            Err(Phase0AnalysisError::AnimationMismatch {
                role: JsonDocumentRole::NewFirst,
                ..
            })
        ));

        let mut extra = Fixture::valid();
        extra.mutate(JsonDocumentRole::NewFirst, |value| {
            animations(value).insert("extra".to_owned(), animation(23));
        });
        assert!(matches!(
            extra.analyze(),
            Err(Phase0AnalysisError::AnimationSetMismatch {
                role: JsonDocumentRole::NewFirst
            })
        ));
    }

    #[test]
    fn existing_import_repeat_must_be_byte_identical() {
        let mut fixture = Fixture::valid();
        fixture
            .documents
            .get_mut(&JsonDocumentRole::ExistingRepeat)
            .expect("repeat role")
            .push(b'\n');
        assert!(matches!(
            fixture.analyze(),
            Err(Phase0AnalysisError::Nondeterministic {
                comparison: ComparisonId::ExistingRepeat
            })
        ));
    }

    #[test]
    fn collision_control_is_semantic_not_byte_idempotence() {
        let mut fixture = Fixture::valid();
        fixture
            .documents
            .get_mut(&JsonDocumentRole::NewCollisionControl)
            .expect("collision role")
            .push(b'\n');
        fixture
            .analyze()
            .expect("semantically exact collision export");
    }

    #[test]
    fn collision_transcript_names_are_bound_to_manifest_and_exported_key() {
        let fixture = Fixture::valid();

        let mut wrong_request = fixture.sources();
        wrong_request.new_animation_collision =
            NewAnimationCollisionEvidence::for_test("walk", "gesture2");
        assert!(matches!(
            analyze_phase0(&fixture.case, wrong_request, &fixture.packages),
            Err(Phase0AnalysisError::CollisionRequestMismatch {
                expected,
                observed
            }) if expected == "gesture" && observed == "walk"
        ));

        let mut wrong_rename = fixture.sources();
        wrong_rename.new_animation_collision =
            NewAnimationCollisionEvidence::for_test("gesture", "gesture3");
        assert!(matches!(
            analyze_phase0(&fixture.case, wrong_rename, &fixture.packages),
            Err(Phase0AnalysisError::CollisionRenameMismatch(name)) if name == "gesture3"
        ));

        let mut preexisting_rename = fixture.sources();
        preexisting_rename.new_animation_collision =
            NewAnimationCollisionEvidence::for_test("gesture", "idle");
        assert!(matches!(
            analyze_phase0(&fixture.case, preexisting_rename, &fixture.packages),
            Err(Phase0AnalysisError::CollisionRenameMismatch(name)) if name == "idle"
        ));
    }

    #[test]
    fn collision_export_must_add_exactly_the_renamed_submitted_animation() {
        let mut wrong_content = Fixture::valid();
        wrong_content.mutate(JsonDocumentRole::NewCollisionControl, |value| {
            animations(value).insert("gesture2".to_owned(), animation(21));
        });
        assert!(matches!(
            wrong_content.analyze(),
            Err(Phase0AnalysisError::AnimationMismatch {
                role: JsonDocumentRole::NewCollisionControl,
                animation
            }) if animation == "gesture2"
        ));

        let mut prior_changed = Fixture::valid();
        prior_changed.mutate(JsonDocumentRole::NewCollisionControl, |value| {
            animations(value).insert("idle".to_owned(), animation(99));
        });
        assert!(matches!(
            prior_changed.analyze(),
            Err(Phase0AnalysisError::AnimationMismatch {
                role: JsonDocumentRole::NewCollisionControl,
                animation
            }) if animation == "idle"
        ));

        let mut extra = Fixture::valid();
        extra.mutate(JsonDocumentRole::NewCollisionControl, |value| {
            animations(value).insert("gesture3".to_owned(), animation(20));
        });
        assert!(matches!(
            extra.analyze(),
            Err(Phase0AnalysisError::CollisionRenameMismatch(name)) if name == "gesture2"
        ));

        let mut missing = Fixture::valid();
        missing.mutate(JsonDocumentRole::NewCollisionControl, |value| {
            animations(value).remove("gesture2");
        });
        assert!(matches!(
            missing.analyze(),
            Err(Phase0AnalysisError::CollisionRenameMismatch(name)) if name == "gesture2"
        ));

        let mut setup = Fixture::valid();
        setup.mutate(JsonDocumentRole::NewCollisionControl, |value| {
            value["bones"][0]["x"] = json!(1);
        });
        assert!(matches!(
            setup.analyze(),
            Err(Phase0AnalysisError::SetupMismatch {
                role: JsonDocumentRole::NewCollisionControl
            })
        ));
    }

    #[test]
    fn project_files_are_excluded_but_every_asset_and_directory_must_match() {
        let fixture = Fixture::valid();
        assert!(
            compare_non_project_packages(&fixture.case, &fixture.packages).is_ok(),
            "different declared project paths and bytes are intentionally ignored"
        );

        let mut changed_asset = fixture.clone();
        changed_asset
            .packages
            .replacement_submission
            .entries
            .iter_mut()
            .find(|entry| entry.path == "images/cat.png")
            .expect("asset")
            .sha256 = Some(sha256_bytes(b"changed"));
        assert!(matches!(
            changed_asset.analyze(),
            Err(Phase0AnalysisError::NonProjectPackageMismatch)
        ));

        let mut missing_directory = fixture.clone();
        missing_directory
            .packages
            .new_submission
            .entries
            .retain(|entry| entry.path != "images");
        assert!(matches!(
            missing_directory.analyze(),
            Err(Phase0AnalysisError::NonProjectPackageMismatch)
        ));

        let mut missing_project = fixture;
        missing_project
            .packages
            .current
            .entries
            .retain(|entry| entry.path != "character.spine");
        assert!(matches!(
            missing_project.analyze(),
            Err(Phase0AnalysisError::InvalidPackageInventory {
                role: PackageRole::Current,
                ..
            })
        ));
    }

    #[test]
    fn malformed_or_ambiguous_package_inventories_fail_closed() {
        let mut duplicate = Fixture::valid();
        let repeated = duplicate.packages.current.entries[1].clone();
        duplicate.packages.current.entries.insert(2, repeated);
        assert!(matches!(
            duplicate.analyze(),
            Err(Phase0AnalysisError::InvalidPackageInventory {
                role: PackageRole::Current,
                ..
            })
        ));

        let mut bad_digest = Fixture::valid();
        bad_digest.packages.current.entries[1].sha256 = Some("not-a-digest".to_owned());
        assert!(matches!(
            bad_digest.analyze(),
            Err(Phase0AnalysisError::InvalidPackageInventory {
                role: PackageRole::Current,
                ..
            })
        ));

        let mut traversal = Fixture::valid();
        traversal.packages.current.entries[1].path = "../escape".to_owned();
        assert!(matches!(
            traversal.analyze(),
            Err(Phase0AnalysisError::InvalidPackageInventory {
                role: PackageRole::Current,
                ..
            })
        ));
    }

    #[test]
    fn exactly_three_deterministic_generic_comparison_views_are_emitted() {
        let completed = Fixture::valid().analyze().expect("completed analysis");
        for (kind, expected_ids, expected_losses) in [
            (
                ComparisonArtifactKind::Roundtrip,
                vec![
                    "round_trip_a",
                    "round_trip_b",
                    "current_determinism",
                    "reconstruction_determinism",
                ],
                1,
            ),
            (
                ComparisonArtifactKind::ExistingImport,
                vec![
                    "replacement_fixture",
                    "existing_mutation",
                    "existing_repeat",
                ],
                0,
            ),
            (
                ComparisonArtifactKind::NewImport,
                vec!["new_fixture", "new_mutation", "new_collision_control"],
                0,
            ),
        ] {
            let first = completed
                .comparison_artifact_bytes(kind)
                .expect("comparison artifact");
            let second = completed
                .comparison_artifact_bytes(kind)
                .expect("repeat comparison artifact");
            assert_eq!(first, second);
            assert_eq!(first.last(), Some(&b'\n'));

            let value: Value = serde_json::from_slice(&first).expect("artifact JSON");
            assert_eq!(value["format_version"], 1);
            assert_eq!(value["evidence_scope"], "generic_rehearsal");
            assert_eq!(value["kind"], serde_json::to_value(kind).expect("kind"));
            assert_eq!(value["coverage"], COMPARISON_COVERAGE);
            assert_eq!(
                value["approved_volatile_pointers"],
                serde_json::json!(["/skeleton/hash"])
            );
            assert_eq!(
                value["comparisons"]
                    .as_array()
                    .expect("comparisons")
                    .iter()
                    .map(|comparison| comparison["id"].as_str().expect("id"))
                    .collect::<Vec<_>>(),
                expected_ids
            );
            assert_eq!(
                value["roundtrip_losses"].as_array().expect("losses").len(),
                expected_losses
            );
        }

        let roundtrip = completed
            .comparison_artifact_view(ComparisonArtifactKind::Roundtrip)
            .expect("roundtrip artifact view");
        assert_eq!(roundtrip.roundtrip_losses[0].pointer, "/skeleton/hash");
        assert_eq!(
            roundtrip.roundtrip_losses[0].observed_in,
            ["round_trip_a", "round_trip_b"]
        );
        assert_eq!(roundtrip.comparisons[0].before.role, "current_a");
        assert_eq!(roundtrip.comparisons[0].after.role, "reconstructed_a");
        assert!(roundtrip.comparisons[0].normalized.equal);
        assert!(!roundtrip.comparisons[0].raw.equal);
    }

    #[test]
    fn textual_difference_is_complete_utf8_safe_and_content_addressed() {
        let before = "prefixλ\"before\n雪suffix";
        let after = "prefixλ\"after\t雪suffix";
        let first = BoundedTextDifference::between(ComparisonId::RoundTripA, "raw", before, after)
            .expect("complete difference");
        let repeat = BoundedTextDifference::between(ComparisonId::RoundTripA, "raw", before, after)
            .expect("repeat complete difference");
        assert_eq!(first, repeat);
        assert!(before.is_char_boundary(first.common_prefix_bytes));
        assert!(after.is_char_boundary(first.common_prefix_bytes));
        assert!(before.is_char_boundary(before.len().saturating_sub(first.common_suffix_bytes)));
        assert!(after.is_char_boundary(after.len().saturating_sub(first.common_suffix_bytes)));
        let before_changed =
            &before[first.common_prefix_bytes..before.len() - first.common_suffix_bytes];
        let after_changed =
            &after[first.common_prefix_bytes..after.len() - first.common_suffix_bytes];
        assert_eq!(
            first.complete_diff_text,
            complete_diff_text(
                first.common_prefix_bytes,
                first.common_suffix_bytes,
                before_changed,
                after_changed,
            )
        );
        assert_eq!(first.complete_diff_bytes, first.complete_diff_text.len());
        assert_eq!(
            first.difference_sha256,
            sha256_bytes(first.complete_diff_text.as_bytes())
        );
        assert!(first.complete_diff_text.contains("before\\n"));
        assert!(first.complete_diff_text.contains("after\\t"));

        let changed = BoundedTextDifference::between(
            ComparisonId::RoundTripA,
            "raw",
            before,
            &format!("{after}!"),
        )
        .expect("changed difference");
        assert_ne!(first.difference_sha256, changed.difference_sha256);
    }

    #[test]
    fn equal_large_text_has_an_empty_exact_change_without_duplication() {
        let value = "λ".repeat(512 * 1024);
        let difference =
            BoundedTextDifference::between(ComparisonId::RoundTripA, "normalized", &value, &value)
                .expect("equal difference");
        assert_eq!(difference.common_prefix_bytes, value.len());
        assert_eq!(difference.common_suffix_bytes, 0);
        assert!(difference.complete_diff_text.len() < 256);
        assert!(
            difference
                .complete_diff_text
                .contains("before-changed-json: \"\"")
        );
        assert!(
            difference
                .complete_diff_text
                .contains("after-changed-json: \"\"")
        );
    }

    #[test]
    fn textual_difference_over_the_fixed_ceiling_fails_artifact_assembly() {
        let before = "a".repeat(MAX_COMPLETE_TEXT_DIFF_BYTES / 2);
        let after = "b".repeat(MAX_COMPLETE_TEXT_DIFF_BYTES / 2);
        assert!(matches!(
            BoundedTextDifference::between(
                ComparisonId::ExistingMutation,
                "canonical",
                &before,
                &after,
            ),
            Err(Phase0AnalysisError::TextualDifferenceTooLarge {
                comparison: ComparisonId::ExistingMutation,
                form: "canonical",
            })
        ));
    }
}

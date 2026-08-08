use crate::case::LoadedCase;
use crate::digest::{is_sha256, sha256_bytes};
use crate::process::ProcessEvidence;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

const REPORT_FORMAT_VERSION: u32 = 2;

/// Fixed required assertions for a complete Phase 0A result.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssertionId {
    /// The case manifest passed fixed-policy validation.
    CaseManifestValidated,
    /// Every full package context was inventoried and verified.
    PackageContextsInventoried,
    /// The editor executable matched its expected identity.
    ExecutableIdentity,
    /// The exact required editor version ran.
    ExactEditorVersion,
    /// Required licensed operations were available.
    LicenseActivated,
    /// Every advanced argument used by the workflow was accepted.
    AdvancedArgumentsAccepted,
    /// Exact source and destination skeletons were found without fallback.
    TargetSkeletonsFound,
    /// The native Spinal validator was present and usable.
    NativeValidatorAvailable,
    /// Editor work was protected by the exclusive operation lock.
    EditorCallsSerialized,
    /// The first reconstruction round trip completed.
    ReconstructionRoundTripFirst,
    /// The independent reconstruction repeat completed.
    ReconstructionRoundTripRepeat,
    /// Repeated round-trip outputs were deterministic after normalization.
    RoundTripDeterministic,
    /// Every round-trip semantic difference was explained by narrow policy.
    RoundTripDifferencesExplained,
    /// Represented properties lost during reconstruction were recorded.
    RoundTripLossesRecorded,
    /// Existing-animation replacement matched the submission fingerprint.
    ExistingImportMatchesSubmission,
    /// Existing-animation replacement preserved setup data.
    ExistingImportPreservesSetup,
    /// Existing-animation replacement preserved unselected animations.
    ExistingImportPreservesOtherAnimations,
    /// Repeating existing-animation replacement was idempotent.
    ExistingImportIdempotent,
    /// New-animation import matched the submission fingerprint.
    NewImportMatchesSubmission,
    /// New-animation import preserved setup data.
    NewImportPreservesSetup,
    /// New-animation import preserved all prior animations.
    NewImportPreservesOtherAnimations,
    /// Repeating new-animation import was idempotent.
    NewImportIdempotent,
    /// All source package tree digests remained unchanged.
    SourcePackagesUnchanged,
    /// Every process transcript passed deny-first classification.
    TranscriptPolicyPassed,
    /// Omitting a required empty path was detected despite zero exit status.
    MissingPathNegativeControl,
}

impl AssertionId {
    /// Returns every required assertion in stable report order.
    pub const fn required() -> &'static [Self] {
        &[
            Self::CaseManifestValidated,
            Self::PackageContextsInventoried,
            Self::ExecutableIdentity,
            Self::ExactEditorVersion,
            Self::LicenseActivated,
            Self::AdvancedArgumentsAccepted,
            Self::TargetSkeletonsFound,
            Self::NativeValidatorAvailable,
            Self::EditorCallsSerialized,
            Self::ReconstructionRoundTripFirst,
            Self::ReconstructionRoundTripRepeat,
            Self::RoundTripDeterministic,
            Self::RoundTripDifferencesExplained,
            Self::RoundTripLossesRecorded,
            Self::ExistingImportMatchesSubmission,
            Self::ExistingImportPreservesSetup,
            Self::ExistingImportPreservesOtherAnimations,
            Self::ExistingImportIdempotent,
            Self::NewImportMatchesSubmission,
            Self::NewImportPreservesSetup,
            Self::NewImportPreservesOtherAnimations,
            Self::NewImportIdempotent,
            Self::SourcePackagesUnchanged,
            Self::TranscriptPolicyPassed,
            Self::MissingPathNegativeControl,
        ]
    }
}

/// Run metadata derived from the immutable validated case.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReportMetadata {
    case_id: String,
    case_sha256: String,
    target_spine_version: String,
    expected_executable_sha256: String,
    tool_version: String,
}

/// Result and artifact citations for one required assertion.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AssertionResult {
    /// Stable required assertion identifier.
    pub id: AssertionId,
    /// Whether the assertion passed.
    pub passed: bool,
    /// Concise explanation of the result.
    pub summary: String,
    /// Digests of recorded artifacts supporting the result.
    pub evidence_sha256: Vec<String>,
}

/// A validated, content-addressed evidence artifact record.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ArtifactEvidence {
    role: String,
    path: String,
    sha256: String,
}

impl ArtifactEvidence {
    /// Creates an artifact record whose digest is derived from the supplied bytes.
    pub fn from_bytes(
        role: impl Into<String>,
        path: impl Into<String>,
        bytes: &[u8],
    ) -> Result<Self, ArtifactError> {
        let role = role.into();
        let path = path.into();
        validate_role(&role)?;
        validate_artifact_path(&path)?;
        Ok(Self {
            role,
            path,
            sha256: sha256_bytes(bytes),
        })
    }

    /// Returns the stable artifact role.
    pub fn role(&self) -> &str {
        &self.role
    }

    /// Returns the evidence-directory-relative artifact path.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the lowercase SHA-256 derived from the artifact bytes.
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

/// Artifact construction failures.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ArtifactError {
    /// Artifact roles must be stable lowercase slugs.
    #[error("artifact role must be a nonempty lowercase ASCII slug")]
    InvalidRole,
    /// Artifact paths must be safe evidence-directory-relative paths.
    #[error("artifact path must be a platform-neutral evidence-relative path")]
    InvalidPath,
}

/// One semantic JSON difference observed during comparison.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SemanticDifference {
    pointer: String,
    before: Option<serde_json::Value>,
    after: Option<serde_json::Value>,
    approved_volatile: bool,
}

impl SemanticDifference {
    /// Creates an untrusted semantic difference for policy evaluation.
    pub fn new(
        pointer: impl Into<String>,
        before: Option<serde_json::Value>,
        after: Option<serde_json::Value>,
    ) -> Self {
        Self {
            pointer: pointer.into(),
            before,
            after,
            approved_volatile: false,
        }
    }

    /// Returns the JSON pointer identifying the changed value.
    pub fn pointer(&self) -> &str {
        &self.pointer
    }

    /// Returns whether immutable case policy approved this difference.
    pub fn approved_volatile(&self) -> bool {
        self.approved_volatile
    }
}

/// One represented property that did not survive reconstruction.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RoundTripLoss {
    /// JSON pointer or stable property identifier.
    pub pointer: String,
    /// Evidence-backed description of the loss.
    pub description: String,
}

/// Stable report-integrity failure categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportIntegrityCode {
    /// No assessed process evidence was recorded.
    MissingProcessEvidence,
    /// A recorded process assessment failed.
    FailedProcess,
    /// A recorded process lacked acquired trusted editor-lock evidence.
    MissingEditorLockEvidence,
    /// Recorded processes used different persistent editor locks.
    InconsistentEditorLockEvidence,
    /// A process executable digest did not match immutable case policy.
    ExecutableIdentityMismatch,
    /// An artifact role, path, or digest was duplicated.
    DuplicateArtifact,
    /// An assertion or process cited no matching artifact record.
    MissingArtifact,
    /// A semantic difference was not approved by checked-in policy.
    UnapprovedSemanticDifference,
}

/// One explanation for a report-level fail-closed result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReportIntegrityFailure {
    code: ReportIntegrityCode,
    detail: String,
}

impl ReportIntegrityFailure {
    /// Returns the stable integrity failure code.
    pub fn code(&self) -> ReportIntegrityCode {
        self.code
    }

    /// Returns the human-readable integrity failure detail.
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

/// Machine-readable output of a complete Phase 0A run.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EvidenceReport {
    format_version: u32,
    metadata: ReportMetadata,
    passed: bool,
    assertions: Vec<AssertionResult>,
    processes: Vec<ProcessEvidence>,
    artifacts: Vec<ArtifactEvidence>,
    integrity_failures: Vec<ReportIntegrityFailure>,
    semantic_differences: Vec<SemanticDifference>,
    roundtrip_losses: Vec<RoundTripLoss>,
}

impl EvidenceReport {
    /// Returns true only when assertions and the full evidence graph pass.
    pub fn passed(&self) -> bool {
        self.passed
    }

    /// Returns required assertions in stable catalog order.
    pub fn assertions(&self) -> &[AssertionResult] {
        &self.assertions
    }

    /// Returns report-level integrity failures.
    pub fn integrity_failures(&self) -> &[ReportIntegrityFailure] {
        &self.integrity_failures
    }

    /// Returns semantic differences with their derived approval decisions.
    pub fn semantic_differences(&self) -> &[SemanticDifference] {
        &self.semantic_differences
    }
}

/// Incremental, fail-closed builder for an evidence report.
pub struct ReportBuilder {
    metadata: ReportMetadata,
    assertions: BTreeMap<AssertionId, AssertionResult>,
    processes: Vec<ProcessEvidence>,
    artifacts: Vec<ArtifactEvidence>,
    approved_volatile_pointers: BTreeSet<String>,
    semantic_differences: Vec<SemanticDifference>,
    roundtrip_losses: Vec<RoundTripLoss>,
}

/// Errors reported while recording assertion results.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ReportBuildError {
    /// The same required assertion was recorded more than once.
    #[error("assertion `{0:?}` was recorded more than once")]
    DuplicateAssertion(AssertionId),
}

impl ReportBuilder {
    /// Creates a report builder bound to one immutable validated case.
    pub fn new(case: &LoadedCase) -> Self {
        let manifest = case.manifest();
        Self {
            metadata: ReportMetadata {
                case_id: manifest.case_id.clone(),
                case_sha256: case.source_sha256().to_owned(),
                target_spine_version: manifest.target_spine_version.clone(),
                expected_executable_sha256: manifest.editor.expected_executable_sha256.clone(),
                tool_version: env!("CARGO_PKG_VERSION").to_owned(),
            },
            assertions: BTreeMap::new(),
            processes: Vec::new(),
            artifacts: Vec::new(),
            approved_volatile_pointers: manifest
                .volatile
                .approved_json_pointers
                .iter()
                .cloned()
                .collect(),
            semantic_differences: Vec::new(),
            roundtrip_losses: Vec::new(),
        }
    }

    /// Records one required assertion exactly once.
    pub fn record_assertion(&mut self, result: AssertionResult) -> Result<(), ReportBuildError> {
        if self.assertions.contains_key(&result.id) {
            return Err(ReportBuildError::DuplicateAssertion(result.id));
        }
        self.assertions.insert(result.id, result);
        Ok(())
    }

    /// Adds one atomically assessed process record.
    pub fn push_process(&mut self, evidence: ProcessEvidence) {
        self.processes.push(evidence);
    }

    /// Adds one validated content-addressed artifact record.
    pub fn push_artifact(&mut self, evidence: ArtifactEvidence) {
        self.artifacts.push(evidence);
    }

    /// Adds one normalized semantic difference.
    pub fn push_semantic_difference(&mut self, difference: SemanticDifference) {
        self.semantic_differences.push(difference);
    }

    /// Adds one observed round-trip loss.
    pub fn push_roundtrip_loss(&mut self, loss: RoundTripLoss) {
        self.roundtrip_losses.push(loss);
    }

    /// Finalizes the report and derives its result from the complete evidence graph.
    pub fn finish(mut self) -> EvidenceReport {
        let mut integrity_failures = Vec::new();
        let artifact_digests = validate_artifacts(&self.artifacts, &mut integrity_failures);
        force_derived_process_assertions(
            &mut self.assertions,
            &self.processes,
            &self.metadata.expected_executable_sha256,
        );
        let assertions = finalize_assertions(
            &mut self.assertions,
            &artifact_digests,
            &mut integrity_failures,
        );
        validate_processes(
            &self.processes,
            &self.metadata.expected_executable_sha256,
            &artifact_digests,
            &mut integrity_failures,
        );
        for difference in &mut self.semantic_differences {
            difference.approved_volatile = self
                .approved_volatile_pointers
                .contains(difference.pointer())
                && matches!(
                    (&difference.before, &difference.after),
                    (
                        Some(serde_json::Value::String(before)),
                        Some(serde_json::Value::String(after))
                    ) if before != after
                );
            if !difference.approved_volatile() {
                integrity_failures.push(integrity(
                    ReportIntegrityCode::UnapprovedSemanticDifference,
                    format!(
                        "unapproved semantic difference at `{}`",
                        difference.pointer()
                    ),
                ));
            }
        }
        let passed =
            assertions.iter().all(|assertion| assertion.passed) && integrity_failures.is_empty();
        EvidenceReport {
            format_version: REPORT_FORMAT_VERSION,
            metadata: self.metadata,
            passed,
            assertions,
            processes: self.processes,
            artifacts: self.artifacts,
            integrity_failures,
            semantic_differences: self.semantic_differences,
            roundtrip_losses: self.roundtrip_losses,
        }
    }
}

fn validate_artifacts(
    artifacts: &[ArtifactEvidence],
    failures: &mut Vec<ReportIntegrityFailure>,
) -> BTreeSet<String> {
    let mut roles = BTreeSet::new();
    let mut portable_paths = BTreeSet::new();
    let mut digests = BTreeSet::new();
    for artifact in artifacts {
        if !roles.insert(artifact.role())
            || !portable_paths.insert(artifact.path().to_ascii_lowercase())
            || !digests.insert(artifact.sha256())
        {
            failures.push(integrity(
                ReportIntegrityCode::DuplicateArtifact,
                format!("duplicate artifact `{}`", artifact.path()),
            ));
        }
    }
    digests.into_iter().map(str::to_owned).collect()
}

fn force_derived_process_assertions(
    assertions: &mut BTreeMap<AssertionId, AssertionResult>,
    processes: &[ProcessEvidence],
    expected_executable_sha256: &str,
) {
    if !processes_share_one_trusted_lock(processes)
        && let Some(assertion) = assertions.get_mut(&AssertionId::EditorCallsSerialized)
    {
        assertion.passed = false;
        assertion.summary =
            "derived failure: editor processes did not share one trusted persistent lock"
                .to_owned();
    }
    let every_executable_matches = !processes.is_empty()
        && processes
            .iter()
            .all(|process| process.executable_identity().sha256() == expected_executable_sha256);
    if !every_executable_matches
        && let Some(assertion) = assertions.get_mut(&AssertionId::ExecutableIdentity)
    {
        assertion.passed = false;
        assertion.summary =
            "derived failure: not every process used the case-pinned executable digest".to_owned();
    }
}

fn processes_share_one_trusted_lock(processes: &[ProcessEvidence]) -> bool {
    let Some(first) = processes.first().and_then(ProcessEvidence::lock_evidence) else {
        return false;
    };
    processes.iter().all(|process| {
        process
            .lock_evidence()
            .is_some_and(|evidence| first.same_identity(evidence))
    })
}

fn finalize_assertions(
    recorded: &mut BTreeMap<AssertionId, AssertionResult>,
    artifacts: &BTreeSet<String>,
    failures: &mut Vec<ReportIntegrityFailure>,
) -> Vec<AssertionResult> {
    let mut assertions = Vec::with_capacity(AssertionId::required().len());
    for id in AssertionId::required() {
        let mut result = recorded.remove(id).unwrap_or_else(|| AssertionResult {
            id: *id,
            passed: false,
            summary: "required assertion was not recorded".to_owned(),
            evidence_sha256: Vec::new(),
        });
        let citations_valid = !result.evidence_sha256.is_empty()
            && result
                .evidence_sha256
                .iter()
                .all(|digest| is_sha256(digest) && artifacts.contains(digest));
        if !citations_valid {
            failures.push(integrity(
                ReportIntegrityCode::MissingArtifact,
                format!("assertion `{:?}` lacks valid recorded evidence", result.id),
            ));
            result.passed = false;
            result.summary = "assertion had missing or unrecorded evidence digests".to_owned();
        }
        assertions.push(result);
    }
    assertions
}

fn validate_processes(
    processes: &[ProcessEvidence],
    expected_executable_sha256: &str,
    artifacts: &BTreeSet<String>,
    failures: &mut Vec<ReportIntegrityFailure>,
) {
    if processes.is_empty() {
        failures.push(integrity(
            ReportIntegrityCode::MissingProcessEvidence,
            "no assessed process evidence was recorded",
        ));
    }
    if !processes.is_empty() && !processes_share_one_trusted_lock(processes) {
        failures.push(integrity(
            ReportIntegrityCode::InconsistentEditorLockEvidence,
            "editor processes did not share one canonical persistent lock identity",
        ));
    }
    for process in processes {
        if process.executable_identity().sha256() != expected_executable_sha256 {
            failures.push(integrity(
                ReportIntegrityCode::ExecutableIdentityMismatch,
                format!(
                    "process `{}` executable digest did not match immutable case policy",
                    process.operation()
                ),
            ));
        }
        if !process.trusted_lock_acquired() {
            failures.push(integrity(
                ReportIntegrityCode::MissingEditorLockEvidence,
                format!(
                    "process `{}` lacked acquired trusted editor-lock evidence",
                    process.operation()
                ),
            ));
        }
        if !process.assessment().passed() {
            failures.push(integrity(
                ReportIntegrityCode::FailedProcess,
                format!("process `{}` failed assessment", process.operation()),
            ));
        }
        for digest in [
            process.assessment().stdout_retained_prefix_sha256(),
            process.assessment().stderr_retained_prefix_sha256(),
        ] {
            if !artifacts.contains(digest) {
                failures.push(integrity(
                    ReportIntegrityCode::MissingArtifact,
                    format!(
                        "process `{}` retained-prefix transcript digest has no artifact",
                        process.operation()
                    ),
                ));
            }
        }
    }
}

fn validate_role(role: &str) -> Result<(), ArtifactError> {
    let bytes = role.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 64
        || !bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        || !bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_".contains(byte))
    {
        return Err(ArtifactError::InvalidRole);
    }
    Ok(())
}

fn validate_artifact_path(path: &str) -> Result<(), ArtifactError> {
    if path.is_empty() || path.len() > 512 || path.starts_with('/') || !path.is_ascii() {
        return Err(ArtifactError::InvalidPath);
    }
    for segment in path.split('/') {
        let bytes = segment.as_bytes();
        if bytes.is_empty()
            || bytes.len() > 64
            || !bytes.first().is_some_and(u8::is_ascii_alphanumeric)
            || !bytes.last().is_some_and(u8::is_ascii_alphanumeric)
            || !bytes
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
            || is_reserved_portable_stem(segment)
        {
            return Err(ArtifactError::InvalidPath);
        }
    }
    Ok(())
}

fn is_reserved_portable_stem(segment: &str) -> bool {
    let stem = segment
        .split_once('.')
        .map_or(segment, |(before_extension, _)| before_extension)
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|suffix| suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9'))
}

fn integrity(code: ReportIntegrityCode, detail: impl Into<String>) -> ReportIntegrityFailure {
    ReportIntegrityFailure {
        code,
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{ProcessCapture, TranscriptPolicy, execute_and_assess};

    #[derive(Clone)]
    struct FakeExecutor(ProcessCapture);

    impl crate::process::ProcessExecutor for FakeExecutor {
        fn execute(
            &self,
            _request: &crate::process::ProcessRequest,
        ) -> Result<ProcessCapture, crate::process::ProcessExecutionError> {
            Ok(self.0.clone())
        }
    }

    fn case() -> LoadedCase {
        crate::parse_case(include_str!("../cases/example.toml")).expect("example case")
    }

    fn complete_builder(exit_code: i32) -> ReportBuilder {
        let case = case();
        let mut builder = ReportBuilder::new(&case);
        let assertion_artifact = ArtifactEvidence::from_bytes(
            "assertion-evidence",
            "artifacts/assertions.json",
            b"checked evidence",
        )
        .expect("assertion artifact");
        let assertion_digest = assertion_artifact.sha256().to_owned();
        builder.push_artifact(assertion_artifact);
        builder.push_artifact(
            ArtifactEvidence::from_bytes("empty-transcript", "transcripts/empty.txt", b"")
                .expect("transcript artifact"),
        );
        let request = crate::process::tests::request();
        let mut capture = crate::process::tests::capture();
        capture.exit_code = Some(exit_code);
        let process = execute_and_assess(
            &FakeExecutor(capture),
            &request,
            TranscriptPolicy::spine_4_3_23(),
        )
        .expect("fake process");
        builder.push_process(process);
        for id in AssertionId::required() {
            builder
                .record_assertion(AssertionResult {
                    id: *id,
                    passed: true,
                    summary: "verified".to_owned(),
                    evidence_sha256: vec![assertion_digest.clone()],
                })
                .expect("unique assertion");
        }
        builder
    }

    #[test]
    fn missing_required_assertions_are_synthesized_as_failures() {
        let case = case();
        let report = ReportBuilder::new(&case).finish();
        assert!(!report.passed());
        assert_eq!(report.assertions().len(), AssertionId::required().len());
    }

    #[test]
    fn complete_recorded_evidence_can_pass() {
        let report = complete_builder(0).finish();
        assert!(report.passed());
        assert!(report.integrity_failures().is_empty());
        assert_eq!(report.format_version, 2);
    }

    #[test]
    fn failed_process_forces_report_failure() {
        let report = complete_builder(7).finish();
        assert!(!report.passed());
        assert!(
            report
                .integrity_failures()
                .iter()
                .any(|failure| { failure.code() == ReportIntegrityCode::FailedProcess })
        );
    }

    #[test]
    fn duplicate_artifact_forces_report_failure() {
        let mut builder = complete_builder(0);
        let duplicate = ArtifactEvidence::from_bytes(
            "duplicate-role",
            "artifacts/duplicate.txt",
            b"checked evidence",
        )
        .expect("duplicate digest artifact");
        builder.push_artifact(duplicate);
        assert!(!builder.finish().passed());
    }

    #[test]
    fn unrecorded_assertion_digest_forces_report_failure() {
        let mut builder = complete_builder(0);
        builder
            .assertions
            .get_mut(&AssertionId::CaseManifestValidated)
            .expect("recorded assertion")
            .evidence_sha256 =
            vec!["ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_owned()];
        let report = builder.finish();
        assert!(!report.passed());
        assert!(!report.assertions()[0].passed);
    }

    #[test]
    fn unapproved_semantic_difference_forces_report_failure() {
        let mut builder = complete_builder(0);
        builder.push_semantic_difference(SemanticDifference::new(
            "/bones/0/x",
            Some(serde_json::json!(0)),
            Some(serde_json::json!(1)),
        ));
        assert!(!builder.finish().passed());
    }

    #[test]
    fn malicious_non_allowlisted_approval_is_recomputed_and_rejected() {
        let mut builder = complete_builder(0);
        builder.semantic_differences.push(SemanticDifference {
            pointer: "/bones/0/x".to_owned(),
            before: Some(serde_json::json!("old")),
            after: Some(serde_json::json!("new")),
            approved_volatile: true,
        });
        let report = builder.finish();
        assert!(!report.passed());
        assert!(!report.semantic_differences()[0].approved_volatile());
    }

    #[test]
    fn exact_hash_string_change_is_derived_as_approved() {
        let mut builder = complete_builder(0);
        builder.push_semantic_difference(SemanticDifference::new(
            "/skeleton/hash",
            Some(serde_json::json!("old")),
            Some(serde_json::json!("new")),
        ));
        let report = builder.finish();
        assert!(report.passed());
        assert!(report.semantic_differences()[0].approved_volatile());
    }

    #[test]
    fn hash_approval_requires_present_different_strings() {
        for (before, after) in [
            (Some(serde_json::json!(0)), Some(serde_json::json!(1))),
            (
                Some(serde_json::json!("same")),
                Some(serde_json::json!("same")),
            ),
            (None, Some(serde_json::json!("new"))),
        ] {
            let mut builder = complete_builder(0);
            builder.push_semantic_difference(SemanticDifference::new(
                "/skeleton/hash",
                before,
                after,
            ));
            let report = builder.finish();
            assert!(!report.passed());
            assert!(!report.semantic_differences()[0].approved_volatile());
        }
    }

    #[test]
    fn invalid_artifact_paths_are_unrepresentable() {
        for path in [
            "../escape",
            "directory\\file.txt",
            "directory/file:name.txt",
            "directory/CON.txt",
            "directory/trailing.",
            "directory/emoji-🐈.txt",
            "/absolute/file.txt",
        ] {
            assert_eq!(
                ArtifactEvidence::from_bytes("artifact", path, b"bytes"),
                Err(ArtifactError::InvalidPath),
                "path should fail: {path}"
            );
        }
        ArtifactEvidence::from_bytes("artifact", "transcripts/export-01.stdout.txt", b"bytes")
            .expect("portable artifact path");
    }

    #[test]
    fn caller_cannot_claim_serialization_without_bound_lock_evidence() {
        let mut builder = complete_builder(0);
        builder.processes.clear();
        let request = crate::process::tests::request();
        let mut capture = crate::process::tests::capture();
        capture.lock_evidence = None;
        let process = execute_and_assess(
            &FakeExecutor(capture),
            &request,
            TranscriptPolicy::spine_4_3_23(),
        )
        .expect("fake process");
        builder.push_process(process);
        let report = builder.finish();
        assert!(!report.passed());
        let assertion = report
            .assertions()
            .iter()
            .find(|value| value.id == AssertionId::EditorCallsSerialized)
            .expect("serialization assertion");
        assert!(!assertion.passed);
        assert!(
            report.integrity_failures().iter().any(|failure| {
                failure.code() == ReportIntegrityCode::MissingEditorLockEvidence
            })
        );
    }

    #[test]
    fn different_valid_locks_cannot_claim_global_serialization() {
        let mut builder = complete_builder(0);
        let request = crate::process::tests::request();
        let mut capture = crate::process::tests::capture();
        capture.lock_evidence = Some(crate::process::LockEvidence::new_acquired(
            std::path::PathBuf::from("/evidence/lock/different.lock"),
            std::time::Duration::from_millis(1),
            1,
            99,
            "test-local".to_owned(),
        ));
        let process = execute_and_assess(
            &FakeExecutor(capture),
            &request,
            TranscriptPolicy::spine_4_3_23(),
        )
        .expect("fake process");
        builder.push_process(process);
        let report = builder.finish();
        assert!(!report.passed());
        assert!(report.integrity_failures().iter().any(|failure| {
            failure.code() == ReportIntegrityCode::InconsistentEditorLockEvidence
        }));
        let assertion = report
            .assertions()
            .iter()
            .find(|value| value.id == AssertionId::EditorCallsSerialized)
            .expect("serialization assertion");
        assert!(!assertion.passed);
    }

    #[test]
    fn executable_digest_assertion_is_derived_from_process_evidence() {
        let mut builder = complete_builder(0);
        builder.processes.clear();
        let request = crate::process::tests::request();
        let mut capture = crate::process::tests::capture();
        capture.executable_identity = crate::process::ExecutableIdentity::new(
            std::path::PathBuf::from("/evidence/editor"),
            sha256_bytes(b"different editor"),
            16,
            1,
            2,
            0o100700,
            0,
            0,
            0,
            0,
            0,
        );
        let process = execute_and_assess(
            &FakeExecutor(capture),
            &request,
            TranscriptPolicy::spine_4_3_23(),
        )
        .expect("fake process");
        builder.push_process(process);
        let report = builder.finish();
        assert!(!report.passed());
        assert!(
            report.integrity_failures().iter().any(|failure| {
                failure.code() == ReportIntegrityCode::ExecutableIdentityMismatch
            })
        );
        let assertion = report
            .assertions()
            .iter()
            .find(|value| value.id == AssertionId::ExecutableIdentity)
            .expect("executable assertion");
        assert!(!assertion.passed);
    }
}

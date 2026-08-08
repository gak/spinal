use crate::case::LoadedCase;
use crate::digest::sha256_bytes;
use crate::evidence_writer::{
    ArtifactPayload, ArtifactSlot, ControlledFailureArtifactPayload, ControlledFailureArtifactSlot,
    ControlledFailureEvidenceBundle, EvidenceBundle, EvidenceWriterError,
    PreparedControlledFailureEvidenceBundle, PreparedEvidenceBundle,
    prepare_controlled_failure_evidence_bundle, prepare_evidence_bundle,
};
use crate::operation_recipe::{CompletedOperationInventory, OperationId, OperationRecord};
use crate::phase0_analysis::{
    ComparisonArtifactKind, ComparisonId, CompletedPhase0Analysis, Phase0AnalysisError,
};
use crate::process::{ProcessEvidence, ProcessFailureCode};
use crate::provenance::{
    CompletePhase0aProvenance, CompleteRuntimeProvenance, ControlledPhase0aProvenance,
    ProvenanceError, complete_phase0a_provenance,
};
use crate::run_workspace::{
    ControlledSourceRechecks, WorkspaceBoundaryEvidence, WorkspaceProjectInventories,
    WorkspaceRunEvidence,
};
use crate::runtime_validations::{CompletedRuntimeValidations, RuntimeValidationsError};
use crate::stage::ControlledSourceRecheckStatus;
use serde::{Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

const REPORT_FORMAT_VERSION: u32 = 4;
const PACKAGE_INVENTORIES_FORMAT_VERSION: u32 = 1;

/// Eligibility of an evidence report for Spinal's representative gate.
///
/// The current harness can mint only generic rehearsal evidence. A future
/// representative entry point must add a closed, fixture-pinned variant; a
/// caller cannot relabel this value after the report is built.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceScope {
    /// Exercises the complete machinery but cannot unlock later phases.
    GenericRehearsal,
}

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
    /// A repeated new-animation import was proved to rename and duplicate it.
    NewImportCollisionHazardDetected,
    /// All source package tree digests remained unchanged.
    SourcePackagesUnchanged,
    /// Every process transcript passed deny-first classification.
    TranscriptPolicyPassed,
    /// Omitting a required empty path was detected despite zero exit status.
    MissingPathNegativeControl,
}

/// Closed outcome vocabulary for every required Phase 0A assertion.
///
/// A passing report contains only `passed` values. A controlled-failure report
/// may retain individually proved assertions, but its overall result is always
/// false and its closed assembler always leaves at least one non-passing value.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssertionStatus {
    /// Complete evidence proved the assertion.
    Passed,
    /// Available evidence directly disproved the assertion.
    Failed,
    /// Evidence required to decide the assertion was never produced.
    Missing,
    /// The run stopped before the assertion's stage could execute.
    Skipped,
    /// Partial evidence exists but cannot certify a pass.
    Degraded,
}

impl AssertionStatus {
    const fn passed(self) -> bool {
        matches!(self, Self::Passed)
    }
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
            Self::NewImportCollisionHazardDetected,
            Self::SourcePackagesUnchanged,
            Self::TranscriptPolicyPassed,
            Self::MissingPathNegativeControl,
        ]
    }
}

/// Run metadata derived from the immutable validated case.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReportMetadata {
    evidence_scope: EvidenceScope,
    representative_gate_eligible: bool,
    case_id: String,
    case_sha256: String,
    target_spine_version: String,
    expected_executable_sha256: String,
    tool_version: String,
    provenance: Option<CompletePhase0aProvenance>,
}

/// Fixed process failures that an intentional negative control may prove.
///
/// This is deliberately a closed catalog: callers cannot supply a predicate or
/// transcript string and thereby turn an arbitrary failed process into passing
/// evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpectedProcessFailure {
    /// Spine imported a duplicate new animation under one exact parsed rename,
    /// returned zero, and mutated only the isolated collision-control project.
    NewAnimationCollisionDiagnostic,
    /// Spine reported its fixed missing-images-path diagnostic and returned
    /// zero. Depending on editor behavior, diagnostic JSON may still exist.
    MissingImagesPathDiagnostic,
}

/// The fixed outcome required from one recorded editor process.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "expected_failure")]
pub enum ProcessExpectation {
    /// A normal editor operation must pass every process assessment rule.
    RequiredSuccess,
    /// An intentional negative control must fail in one exact approved way.
    NegativeControl(ExpectedProcessFailure),
}

/// One assessed process together with its immutable expected outcome.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedProcess {
    expectation: ProcessExpectation,
    evidence: ProcessEvidence,
    stdout_artifact: ArtifactEvidence,
    stderr_artifact: ArtifactEvidence,
}

impl RecordedProcess {
    /// Records a normal operation that must pass its process assessment.
    #[allow(
        dead_code,
        reason = "used by the upcoming closed Phase 0A orchestrator"
    )]
    pub(crate) fn required_success(
        evidence: ProcessEvidence,
        stdout_artifact: ArtifactEvidence,
        stderr_artifact: ArtifactEvidence,
    ) -> Self {
        Self::new(
            ProcessExpectation::RequiredSuccess,
            evidence,
            stdout_artifact,
            stderr_artifact,
        )
    }

    /// Records an intentional negative control with a fixed expected failure.
    #[allow(
        dead_code,
        reason = "used by the upcoming closed Phase 0A orchestrator"
    )]
    pub(crate) fn negative_control(
        expected_failure: ExpectedProcessFailure,
        evidence: ProcessEvidence,
        stdout_artifact: ArtifactEvidence,
        stderr_artifact: ArtifactEvidence,
    ) -> Self {
        Self::new(
            ProcessExpectation::NegativeControl(expected_failure),
            evidence,
            stdout_artifact,
            stderr_artifact,
        )
    }

    #[allow(
        dead_code,
        reason = "used by the upcoming closed Phase 0A orchestrator"
    )]
    fn new(
        expectation: ProcessExpectation,
        evidence: ProcessEvidence,
        stdout_artifact: ArtifactEvidence,
        stderr_artifact: ArtifactEvidence,
    ) -> Self {
        Self {
            expectation,
            evidence,
            stdout_artifact,
            stderr_artifact,
        }
    }

    /// Returns the immutable expected outcome.
    pub fn expectation(&self) -> ProcessExpectation {
        self.expectation
    }

    /// Returns the atomically assessed process evidence.
    pub fn evidence(&self) -> &ProcessEvidence {
        &self.evidence
    }

    /// Returns the exact artifact identity containing retained standard output.
    pub fn stdout_artifact(&self) -> &ArtifactEvidence {
        &self.stdout_artifact
    }

    /// Returns the exact artifact identity containing retained standard error.
    pub fn stderr_artifact(&self) -> &ArtifactEvidence {
        &self.stderr_artifact
    }
}

/// Result and artifact citations for one required assertion.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AssertionResult {
    /// Stable required assertion identifier.
    id: AssertionId,
    /// Closed status derived by the applicable report assembler.
    status: AssertionStatus,
    /// Concise explanation of the result.
    summary: String,
    /// Exact recorded artifact identities supporting the result.
    evidence: Vec<ArtifactEvidence>,
}

impl AssertionResult {
    /// Returns the stable required assertion identifier.
    pub fn id(&self) -> AssertionId {
        self.id
    }

    /// Returns the result derived by the fixed evidence graph.
    pub fn passed(&self) -> bool {
        self.status.passed()
    }

    /// Returns the closed evidence status.
    pub const fn status(&self) -> AssertionStatus {
        self.status
    }

    /// Returns the concise evidence-backed explanation.
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// Returns the exact artifact identities cited by this assertion.
    pub fn evidence(&self) -> &[ArtifactEvidence] {
        &self.evidence
    }
}

/// A validated, content-addressed evidence artifact record.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
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
    /// Complete closed provenance was absent from a nominal report.
    MissingProvenance,
    /// No assessed process evidence was recorded.
    MissingProcessEvidence,
    /// A required-success process assessment failed.
    FailedProcess,
    /// An intentional negative control unexpectedly passed its assessment.
    UnexpectedProcessSuccess,
    /// An intentional negative control failed for an unapproved reason.
    WrongProcessFailure,
    /// A recorded process lacked acquired trusted editor-lock evidence.
    MissingEditorLockEvidence,
    /// Recorded processes used different persistent editor locks.
    InconsistentEditorLockEvidence,
    /// A process executable digest did not match immutable case policy.
    ExecutableIdentityMismatch,
    /// An exact artifact identity or case-insensitive path was duplicated.
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
    processes: Vec<RecordedProcess>,
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

    /// Returns assessed processes with their fixed expected outcomes.
    pub fn processes(&self) -> &[RecordedProcess] {
        &self.processes
    }

    /// Returns the exact artifact identities recorded by the report.
    pub(crate) fn artifacts(&self) -> &[ArtifactEvidence] {
        &self.artifacts
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

/// Stable controlled-failure categories accepted by the closed assembler.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ControlledFailureCode {
    #[allow(
        dead_code,
        reason = "reserved for an admitted post-path-policy failure"
    )]
    PathPolicy,
    EditorIdentity,
    EditorEnvironment,
    WorkspacePreparation,
    EditorOperation,
    WorkspaceVerification,
    SemanticAnalysis,
    RuntimeValidation,
    ReportAssembly,
    Provenance,
}

/// Typed completion proofs that remain available at the failed stage.
pub(crate) struct ControlledFailureProofs<'a> {
    source: ControlledSourceProof<'a>,
    workspace: Option<(
        &'a WorkspaceProjectInventories,
        &'a WorkspaceBoundaryEvidence,
    )>,
    analysis: Option<&'a CompletedPhase0Analysis>,
    runtime: Option<&'a CompletedRuntimeValidations>,
}

impl<'a> ControlledFailureProofs<'a> {
    pub(crate) fn unavailable() -> Self {
        Self {
            source: ControlledSourceProof::Unavailable,
            workspace: None,
            analysis: None,
            runtime: None,
        }
    }

    pub(crate) fn from_rechecks(source_rechecks: &'a ControlledSourceRechecks) -> Self {
        Self {
            source: ControlledSourceProof::Rechecks(source_rechecks),
            workspace: None,
            analysis: None,
            runtime: None,
        }
    }

    pub(crate) fn with_workspace(
        mut self,
        project_inventories: &'a WorkspaceProjectInventories,
        boundary: &'a WorkspaceBoundaryEvidence,
    ) -> Self {
        self.source = ControlledSourceProof::CompletedBoundary(boundary);
        self.workspace = Some((project_inventories, boundary));
        self
    }

    pub(crate) fn with_analysis(mut self, analysis: &'a CompletedPhase0Analysis) -> Self {
        self.analysis = Some(analysis);
        self
    }

    pub(crate) fn with_runtime(mut self, runtime: &'a CompletedRuntimeValidations) -> Self {
        self.runtime = Some(runtime);
        self
    }

    pub(crate) fn package_inventories(&self) -> Option<crate::package::CasePackageInventories> {
        self.workspace
            .as_ref()
            .map(|(_, boundary)| boundary.case_package_inventories())
    }
}

enum ControlledSourceProof<'a> {
    Rechecks(&'a ControlledSourceRechecks),
    CompletedBoundary(&'a WorkspaceBoundaryEvidence),
    Unavailable,
}

/// Internal inputs for publishing evidence from an admitted but failed attempt.
///
/// The validated case is mandatory: failures before case admission are not
/// evidence runs. Neither the code nor the diagnostic can alter report scope,
/// overall result, or any assertion to `passed`.
pub(crate) struct ControlledFailureInputs<'a> {
    case: &'a LoadedCase,
    code: ControlledFailureCode,
    operation: Option<OperationId>,
    diagnostic: &'a str,
    processes: &'a [ProcessEvidence],
    proofs: ControlledFailureProofs<'a>,
    provenance: ControlledPhase0aProvenance,
}

impl<'a> ControlledFailureInputs<'a> {
    pub(crate) fn new(
        case: &'a LoadedCase,
        code: ControlledFailureCode,
        operation: Option<OperationId>,
        diagnostic: &'a str,
        processes: &'a [ProcessEvidence],
        proofs: ControlledFailureProofs<'a>,
        provenance: ControlledPhase0aProvenance,
    ) -> Self {
        Self {
            case,
            code,
            operation,
            diagnostic,
            processes,
            proofs,
            provenance,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ControlledFailureMetadata {
    evidence_scope: EvidenceScope,
    representative_gate_eligible: AlwaysFalse,
    case_id: String,
    case_id_withheld: bool,
    case_sha256: String,
    target_spine_version: String,
    tool_version: String,
    provenance: ControlledPhase0aProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ControlledFailureRecord {
    code: ControlledFailureCode,
    operation: Option<String>,
    diagnostic: String,
    diagnostic_withheld: bool,
    case_manifest_omitted: bool,
    unsafe_transcript_pairs_omitted: usize,
    source_integrity: ControlledSourceIntegrityEvidence,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "evidence")]
enum ControlledSourceIntegrityEvidence {
    Rechecks(Box<ControlledSourceRechecks>),
    CompletedBoundary(Box<WorkspaceBoundaryEvidence>),
    Unavailable,
}

impl ControlledSourceIntegrityEvidence {
    fn status(&self) -> ControlledSourceRecheckStatus {
        match self {
            Self::Rechecks(rechecks) => rechecks.status(),
            Self::CompletedBoundary(_) => ControlledSourceRecheckStatus::Unchanged,
            Self::Unavailable => ControlledSourceRecheckStatus::Unavailable,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ControlledFailureProcess {
    index: usize,
    operation: String,
    exit_code: Option<i32>,
    assessment_passed: bool,
    assessment_failure_codes: Vec<ProcessFailureCode>,
    stdout_retained_prefix_sha256: String,
    stderr_retained_prefix_sha256: String,
    transcript_artifacts: Option<ControlledFailureTranscriptArtifacts>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ControlledFailureTranscriptArtifacts {
    stdout: ArtifactEvidence,
    stderr: ArtifactEvidence,
}

/// Machine-readable result for one controlled generic rehearsal failure.
///
/// This type has no public constructor and is accepted only by the distinct
/// controlled-failure writer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct ControlledFailureReport {
    format_version: u32,
    metadata: ControlledFailureMetadata,
    passed: AlwaysFalse,
    failure: ControlledFailureRecord,
    assertions: Vec<AssertionResult>,
    processes: Vec<ControlledFailureProcess>,
    artifacts: Vec<ArtifactEvidence>,
}

impl ControlledFailureReport {
    pub(crate) fn passed(&self) -> bool {
        false
    }

    pub(crate) fn assertions(&self) -> &[AssertionResult] {
        &self.assertions
    }

    pub(crate) fn artifacts(&self) -> &[ArtifactEvidence] {
        &self.artifacts
    }

    pub(crate) fn processes(&self) -> &[ControlledFailureProcess] {
        &self.processes
    }
}

impl ControlledFailureProcess {
    pub(crate) fn transcript_artifacts(&self) -> Option<(&ArtifactEvidence, &ArtifactEvidence)> {
        self.transcript_artifacts
            .as_ref()
            .map(|artifacts| (&artifacts.stdout, &artifacts.stderr))
    }
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ControlledFailureArtifact<'a> {
    format_version: u32,
    evidence_scope: EvidenceScope,
    representative_gate_eligible: AlwaysFalse,
    passed: AlwaysFalse,
    failure: &'a ControlledFailureRecord,
    processes: &'a [ControlledFailureProcess],
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct AlwaysFalse;

impl Serialize for AlwaysFalse {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bool(false)
    }
}

/// Builds and privacy-preflights a closed, always-failing evidence graph.
pub(crate) fn prepare_controlled_failure_evidence(
    inputs: ControlledFailureInputs<'_>,
) -> Result<PreparedControlledFailureEvidenceBundle, ReportAssemblyError> {
    if inputs.processes.len() > OperationId::ORDER.len() {
        return Err(ReportAssemblyError::OperationInventoryMismatch);
    }
    if inputs
        .proofs
        .analysis
        .is_some_and(|analysis| analysis.case_sha256() != inputs.case.source_sha256())
        || (inputs.proofs.runtime.is_some() && inputs.proofs.analysis.is_none())
    {
        return Err(ReportAssemblyError::CaseBindingMismatch);
    }

    let mut payloads = Vec::new();
    let case_manifest_omitted =
        !crate::evidence_writer::evidence_bytes_are_privacy_safe(inputs.case.source_bytes());
    if !case_manifest_omitted {
        payloads.push(ControlledFailureArtifactPayload::new(
            ControlledFailureArtifactSlot::CaseManifest,
            inputs.case.source_bytes().to_vec(),
        ));
    }

    let mut processes = Vec::with_capacity(inputs.processes.len());
    let mut unsafe_transcript_pairs_omitted = 0_usize;
    for (index, process) in inputs.processes.iter().enumerate() {
        let stdout_bytes = process.raw_stdout_retained_prefix();
        let stderr_bytes = process.raw_stderr_retained_prefix();
        let transcript_artifacts =
            if crate::evidence_writer::evidence_bytes_are_privacy_safe(stdout_bytes)
                && crate::evidence_writer::evidence_bytes_are_privacy_safe(stderr_bytes)
            {
                let stdout = ControlledFailureArtifactPayload::new(
                    ControlledFailureArtifactSlot::ProcessStdout(index),
                    stdout_bytes.to_vec(),
                );
                let stderr = ControlledFailureArtifactPayload::new(
                    ControlledFailureArtifactSlot::ProcessStderr(index),
                    stderr_bytes.to_vec(),
                );
                let identities = ControlledFailureTranscriptArtifacts {
                    stdout: stdout.identity().clone(),
                    stderr: stderr.identity().clone(),
                };
                payloads.push(stdout);
                payloads.push(stderr);
                Some(identities)
            } else {
                unsafe_transcript_pairs_omitted = unsafe_transcript_pairs_omitted.saturating_add(1);
                None
            };
        processes.push(ControlledFailureProcess {
            index,
            operation: process.operation().to_owned(),
            exit_code: process.exit_code(),
            assessment_passed: process.assessment().passed(),
            assessment_failure_codes: process
                .assessment()
                .failures()
                .iter()
                .map(|failure| failure.code)
                .collect(),
            stdout_retained_prefix_sha256: process
                .assessment()
                .stdout_retained_prefix_sha256()
                .to_owned(),
            stderr_retained_prefix_sha256: process
                .assessment()
                .stderr_retained_prefix_sha256()
                .to_owned(),
            transcript_artifacts,
        });
    }

    let source_integrity = match &inputs.proofs.source {
        ControlledSourceProof::Rechecks(rechecks) => {
            ControlledSourceIntegrityEvidence::Rechecks(Box::new((*rechecks).clone()))
        }
        ControlledSourceProof::CompletedBoundary(boundary) => {
            ControlledSourceIntegrityEvidence::CompletedBoundary(Box::new((*boundary).clone()))
        }
        ControlledSourceProof::Unavailable => ControlledSourceIntegrityEvidence::Unavailable,
    };
    let source_status = source_integrity.status();
    let (diagnostic, diagnostic_withheld) = privacy_safe_diagnostic(inputs.diagnostic);
    let failure = ControlledFailureRecord {
        code: inputs.code,
        operation: inputs.operation.map(operation_id_name).map(str::to_owned),
        diagnostic,
        diagnostic_withheld,
        case_manifest_omitted,
        unsafe_transcript_pairs_omitted,
        source_integrity,
    };
    let failure_bytes = pretty_json_bytes(&ControlledFailureArtifact {
        format_version: 1,
        evidence_scope: EvidenceScope::GenericRehearsal,
        representative_gate_eligible: AlwaysFalse,
        passed: AlwaysFalse,
        failure: &failure,
        processes: &processes,
    })?;
    let failure_payload = ControlledFailureArtifactPayload::new(
        ControlledFailureArtifactSlot::Failure,
        failure_bytes,
    );
    let failure_identity = failure_payload.identity().clone();
    payloads.push(failure_payload);

    let mut artifacts = payloads
        .iter()
        .map(|payload| payload.identity().clone())
        .collect::<Vec<_>>();
    artifacts.sort();
    let assertions = controlled_failure_assertions(ControlledAssertionInputs {
        code: inputs.code,
        operation: inputs.operation,
        failure_identity: &failure_identity,
        artifacts: &artifacts,
        unsafe_transcript_pairs_omitted,
        case: inputs.case,
        processes: inputs.processes,
        proofs: &inputs.proofs,
        source_status,
    });
    let manifest = inputs.case.manifest();
    let (case_id, case_id_withheld) = privacy_safe_label(&manifest.case_id);
    let report = ControlledFailureReport {
        format_version: REPORT_FORMAT_VERSION,
        metadata: ControlledFailureMetadata {
            evidence_scope: EvidenceScope::GenericRehearsal,
            representative_gate_eligible: AlwaysFalse,
            case_id,
            case_id_withheld,
            case_sha256: inputs.case.source_sha256().to_owned(),
            target_spine_version: manifest.target_spine_version.clone(),
            tool_version: env!("CARGO_PKG_VERSION").to_owned(),
            provenance: inputs.provenance,
        },
        passed: AlwaysFalse,
        failure,
        assertions,
        processes,
        artifacts,
    };
    prepare_controlled_failure_evidence_bundle(ControlledFailureEvidenceBundle::new(
        report, payloads,
    ))
    .map_err(Into::into)
}

#[derive(Clone, Copy)]
enum ControlledAssertionStatus {
    Passed,
    Failed,
    Missing,
    Skipped,
    Degraded,
}

impl From<ControlledAssertionStatus> for AssertionStatus {
    fn from(value: ControlledAssertionStatus) -> Self {
        match value {
            ControlledAssertionStatus::Passed => Self::Passed,
            ControlledAssertionStatus::Failed => Self::Failed,
            ControlledAssertionStatus::Missing => Self::Missing,
            ControlledAssertionStatus::Skipped => Self::Skipped,
            ControlledAssertionStatus::Degraded => Self::Degraded,
        }
    }
}

struct ControlledAssertionInputs<'a> {
    code: ControlledFailureCode,
    operation: Option<OperationId>,
    failure_identity: &'a ArtifactEvidence,
    artifacts: &'a [ArtifactEvidence],
    unsafe_transcript_pairs_omitted: usize,
    case: &'a LoadedCase,
    processes: &'a [ProcessEvidence],
    proofs: &'a ControlledFailureProofs<'a>,
    source_status: ControlledSourceRecheckStatus,
}

fn controlled_failure_assertions(inputs: ControlledAssertionInputs<'_>) -> Vec<AssertionResult> {
    let failed_id = controlled_failure_assertion_id(inputs.code, inputs.operation);
    let failed_index = AssertionId::required()
        .iter()
        .position(|id| *id == failed_id)
        .expect("closed failure target is a required assertion");
    AssertionId::required()
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let base_status = if *id == failed_id {
                ControlledAssertionStatus::Failed
            } else if *id == AssertionId::TranscriptPolicyPassed
                && inputs.unsafe_transcript_pairs_omitted > 0
            {
                ControlledAssertionStatus::Degraded
            } else if index > failed_index {
                ControlledAssertionStatus::Skipped
            } else if !inputs.processes.is_empty() {
                ControlledAssertionStatus::Degraded
            } else {
                ControlledAssertionStatus::Missing
            };
            let status = if *id == failed_id {
                base_status
            } else {
                controlled_proven_status(
                    *id,
                    inputs.code,
                    inputs.case,
                    inputs.processes,
                    inputs.proofs,
                    inputs.source_status,
                    inputs.unsafe_transcript_pairs_omitted,
                )
                .unwrap_or(base_status)
            };
            let mut evidence = vec![inputs.failure_identity.clone()];
            if matches!(
                id,
                AssertionId::CaseManifestValidated | AssertionId::ExecutableIdentity
            ) && let Some(case) = inputs
                .artifacts
                .iter()
                .find(|artifact| artifact.path() == "attempt/case.toml")
            {
                evidence.push(case.clone());
            }
            if *id == AssertionId::TranscriptPolicyPassed {
                evidence.extend(
                    inputs
                        .artifacts
                        .iter()
                        .filter(|artifact| {
                            matches!(artifact.role(), "process-stdout" | "process-stderr")
                        })
                        .cloned(),
                );
            }
            evidence.sort();
            evidence.dedup();
            let summary = controlled_failure_summary(
                status,
                *id == AssertionId::TranscriptPolicyPassed
                    && inputs.unsafe_transcript_pairs_omitted > 0,
            );
            AssertionResult {
                id: *id,
                status: status.into(),
                summary,
                evidence,
            }
        })
        .collect()
}

fn controlled_proven_status(
    id: AssertionId,
    code: ControlledFailureCode,
    case: &LoadedCase,
    processes: &[ProcessEvidence],
    proofs: &ControlledFailureProofs<'_>,
    source_status: ControlledSourceRecheckStatus,
    unsafe_transcript_pairs_omitted: usize,
) -> Option<ControlledAssertionStatus> {
    let all_pass = |indices: &[usize]| {
        indices.iter().all(|index| {
            processes.get(*index).is_some_and(|process| {
                process.assessment().passed()
                    && process.operation() == operation_process_name(OperationId::ORDER[*index])
            })
        })
    };
    let all_expected = |indices: &[usize]| {
        indices.iter().all(|index| {
            processes.get(*index).is_some_and(|process| {
                let operation = OperationId::ORDER[*index];
                match expected_failure_for_operation(operation) {
                    Some(expected) => expected_process_failure_matches(expected, process),
                    None => {
                        process.assessment().passed()
                            && process.operation() == operation_process_name(operation)
                    }
                }
            })
        })
    };
    let analysis_complete = proofs.analysis.is_some();
    let result = match id {
        AssertionId::CaseManifestValidated => ControlledAssertionStatus::Passed,
        AssertionId::PackageContextsInventoried if proofs.workspace.is_some() => {
            ControlledAssertionStatus::Passed
        }
        AssertionId::ExecutableIdentity
            if code != ControlledFailureCode::EditorIdentity && !processes.is_empty() =>
        {
            let expected = &case.manifest().editor.expected_executable_sha256;
            if processes
                .iter()
                .all(|process| process.executable_identity().sha256() == expected)
            {
                ControlledAssertionStatus::Passed
            } else {
                ControlledAssertionStatus::Failed
            }
        }
        AssertionId::ExactEditorVersion if all_pass(&[0]) => ControlledAssertionStatus::Passed,
        AssertionId::LicenseActivated if all_pass(&[0, 5, 8, 13, 17]) => {
            ControlledAssertionStatus::Passed
        }
        AssertionId::AdvancedArgumentsAccepted
            if all_expected(&[1, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20]) =>
        {
            ControlledAssertionStatus::Passed
        }
        AssertionId::TargetSkeletonsFound if all_expected(&[2, 3, 4, 13, 15, 17, 19]) => {
            ControlledAssertionStatus::Passed
        }
        AssertionId::NativeValidatorAvailable if proofs.runtime.is_some() => {
            ControlledAssertionStatus::Passed
        }
        AssertionId::EditorCallsSerialized
            if process_evidence_shares_one_trusted_lock(processes) =>
        {
            ControlledAssertionStatus::Passed
        }
        AssertionId::ReconstructionRoundTripFirst if all_pass(&[5, 8, 9]) => {
            ControlledAssertionStatus::Passed
        }
        AssertionId::ReconstructionRoundTripRepeat if all_pass(&[10, 11, 12]) => {
            ControlledAssertionStatus::Passed
        }
        AssertionId::RoundTripDeterministic
        | AssertionId::RoundTripDifferencesExplained
        | AssertionId::RoundTripLossesRecorded
        | AssertionId::ExistingImportMatchesSubmission
        | AssertionId::ExistingImportPreservesSetup
        | AssertionId::ExistingImportPreservesOtherAnimations
        | AssertionId::ExistingImportIdempotent
        | AssertionId::NewImportMatchesSubmission
        | AssertionId::NewImportPreservesSetup
        | AssertionId::NewImportPreservesOtherAnimations
        | AssertionId::NewImportCollisionHazardDetected
            if analysis_complete =>
        {
            ControlledAssertionStatus::Passed
        }
        AssertionId::SourcePackagesUnchanged => match source_status {
            ControlledSourceRecheckStatus::Unchanged => ControlledAssertionStatus::Passed,
            ControlledSourceRecheckStatus::Changed => ControlledAssertionStatus::Failed,
            ControlledSourceRecheckStatus::Unavailable if processes.is_empty() => {
                ControlledAssertionStatus::Missing
            }
            ControlledSourceRecheckStatus::Unavailable => ControlledAssertionStatus::Degraded,
        },
        AssertionId::TranscriptPolicyPassed
            if unsafe_transcript_pairs_omitted == 0
                && processes.len() == OperationId::ORDER.len()
                && all_expected(&(0..OperationId::ORDER.len()).collect::<Vec<_>>()) =>
        {
            ControlledAssertionStatus::Passed
        }
        AssertionId::MissingPathNegativeControl
            if processes
                .get(21)
                .is_some_and(missing_images_path_failure_matches) =>
        {
            ControlledAssertionStatus::Passed
        }
        _ => return None,
    };
    Some(result)
}

fn operation_process_name(operation: OperationId) -> &'static str {
    match operation {
        OperationId::Version => "spine-version",
        OperationId::AdvancedHelp => "spine-advanced-help",
        OperationId::InfoCurrent | OperationId::InfoReplacement | OperationId::InfoNew => {
            "spine-project-info"
        }
        OperationId::ExportCurrentA
        | OperationId::ExportReplacementSubmission
        | OperationId::ExportNewSubmission
        | OperationId::ExportReconstructedA
        | OperationId::ExportCurrentB
        | OperationId::ExportReconstructedB
        | OperationId::ExportExistingFirst
        | OperationId::ExportExistingRepeat
        | OperationId::ExportNewFirst
        | OperationId::ExportNewCollisionControl => "spine-export-json",
        OperationId::ReconstructA | OperationId::ReconstructB => "spine-reconstruct-json",
        OperationId::ImportExistingFirst | OperationId::ImportExistingRepeat => {
            "spine-import-existing-animation"
        }
        OperationId::ImportNewFirst => "spine-import-new-animation",
        OperationId::ImportNewCollisionControl => "spine-new-animation-collision-control",
        OperationId::MissingImagesPathControl => "spine-missing-images-path-control",
    }
}

fn process_evidence_shares_one_trusted_lock(processes: &[ProcessEvidence]) -> bool {
    let Some(first) = processes.first().and_then(ProcessEvidence::lock_evidence) else {
        return false;
    };
    processes.iter().all(|process| {
        process
            .lock_evidence()
            .is_some_and(|evidence| first.same_identity(evidence))
    })
}

fn controlled_failure_summary(
    status: ControlledAssertionStatus,
    transcript_omitted: bool,
) -> String {
    if transcript_omitted {
        return "degraded controlled failure: at least one raw transcript pair was withheld by privacy policy; stream digests remain in attempt/failure.json".to_owned();
    }
    match status {
        ControlledAssertionStatus::Passed => {
            "controlled failure: this individual assertion remained fully proved by retained typed evidence".to_owned()
        }
        ControlledAssertionStatus::Failed => {
            "controlled failure: available evidence directly identifies this assertion as the failed stage".to_owned()
        }
        ControlledAssertionStatus::Missing => {
            "controlled failure: required success evidence was not produced".to_owned()
        }
        ControlledAssertionStatus::Skipped => {
            "controlled failure: the attempt stopped before this assertion could execute".to_owned()
        }
        ControlledAssertionStatus::Degraded => {
            "controlled failure: partial evidence exists but cannot certify a pass".to_owned()
        }
    }
}

fn controlled_failure_assertion_id(
    code: ControlledFailureCode,
    operation: Option<OperationId>,
) -> AssertionId {
    match code {
        ControlledFailureCode::PathPolicy => AssertionId::PackageContextsInventoried,
        ControlledFailureCode::WorkspacePreparation => AssertionId::PackageContextsInventoried,
        ControlledFailureCode::EditorIdentity => AssertionId::ExecutableIdentity,
        ControlledFailureCode::Provenance => AssertionId::ExecutableIdentity,
        ControlledFailureCode::EditorEnvironment => AssertionId::EditorCallsSerialized,
        ControlledFailureCode::WorkspaceVerification => AssertionId::SourcePackagesUnchanged,
        ControlledFailureCode::SemanticAnalysis => AssertionId::RoundTripDifferencesExplained,
        ControlledFailureCode::RuntimeValidation => AssertionId::NativeValidatorAvailable,
        ControlledFailureCode::ReportAssembly => AssertionId::TranscriptPolicyPassed,
        ControlledFailureCode::EditorOperation => operation
            .map(operation_failure_assertion)
            .unwrap_or(AssertionId::TranscriptPolicyPassed),
    }
}

fn operation_failure_assertion(operation: OperationId) -> AssertionId {
    match operation {
        OperationId::Version => AssertionId::ExactEditorVersion,
        OperationId::AdvancedHelp => AssertionId::AdvancedArgumentsAccepted,
        OperationId::InfoCurrent | OperationId::InfoReplacement | OperationId::InfoNew => {
            AssertionId::TargetSkeletonsFound
        }
        OperationId::ExportCurrentA
        | OperationId::ReconstructA
        | OperationId::ExportReconstructedA => AssertionId::ReconstructionRoundTripFirst,
        OperationId::ExportCurrentB
        | OperationId::ReconstructB
        | OperationId::ExportReconstructedB => AssertionId::ReconstructionRoundTripRepeat,
        OperationId::ExportReplacementSubmission
        | OperationId::ImportExistingFirst
        | OperationId::ExportExistingFirst => AssertionId::ExistingImportMatchesSubmission,
        OperationId::ImportExistingRepeat | OperationId::ExportExistingRepeat => {
            AssertionId::ExistingImportIdempotent
        }
        OperationId::ExportNewSubmission
        | OperationId::ImportNewFirst
        | OperationId::ExportNewFirst => AssertionId::NewImportMatchesSubmission,
        OperationId::ImportNewCollisionControl | OperationId::ExportNewCollisionControl => {
            AssertionId::NewImportCollisionHazardDetected
        }
        OperationId::MissingImagesPathControl => AssertionId::MissingPathNegativeControl,
    }
}

fn operation_id_name(operation: OperationId) -> &'static str {
    match operation {
        OperationId::Version => "version",
        OperationId::AdvancedHelp => "advanced_help",
        OperationId::InfoCurrent => "info_current",
        OperationId::InfoReplacement => "info_replacement",
        OperationId::InfoNew => "info_new",
        OperationId::ExportCurrentA => "export_current_a",
        OperationId::ExportReplacementSubmission => "export_replacement_submission",
        OperationId::ExportNewSubmission => "export_new_submission",
        OperationId::ReconstructA => "reconstruct_a",
        OperationId::ExportReconstructedA => "export_reconstructed_a",
        OperationId::ExportCurrentB => "export_current_b",
        OperationId::ReconstructB => "reconstruct_b",
        OperationId::ExportReconstructedB => "export_reconstructed_b",
        OperationId::ImportExistingFirst => "import_existing_first",
        OperationId::ExportExistingFirst => "export_existing_first",
        OperationId::ImportExistingRepeat => "import_existing_repeat",
        OperationId::ExportExistingRepeat => "export_existing_repeat",
        OperationId::ImportNewFirst => "import_new_first",
        OperationId::ExportNewFirst => "export_new_first",
        OperationId::ImportNewCollisionControl => "import_new_collision_control",
        OperationId::ExportNewCollisionControl => "export_new_collision_control",
        OperationId::MissingImagesPathControl => "missing_images_path_control",
    }
}

fn privacy_safe_diagnostic(diagnostic: &str) -> (String, bool) {
    let safe_shape = !diagnostic.is_empty()
        && diagnostic.len() <= 16 * 1024
        && diagnostic
            .chars()
            .all(|character| !character.is_control() || matches!(character, '\n' | '\r' | '\t'));
    if safe_shape && crate::evidence_writer::evidence_json_string_is_privacy_safe(diagnostic) {
        (diagnostic.to_owned(), false)
    } else {
        (
            "diagnostic withheld by fixed privacy and size policy".to_owned(),
            true,
        )
    }
}

fn privacy_safe_label(label: &str) -> (String, bool) {
    if !label.is_empty()
        && label.len() <= 1024
        && !label.chars().any(char::is_control)
        && crate::evidence_writer::evidence_json_string_is_privacy_safe(label)
    {
        (label.to_owned(), false)
    } else {
        ("<withheld>".to_owned(), true)
    }
}

/// Borrowed completion proofs required by the one closed report assembler.
///
/// Every field is minted by an earlier checked stage. There are deliberately
/// no caller-authored assertion results, summaries, allowlists, or citations.
pub(crate) struct Phase0aReportInputs<'a> {
    case: &'a LoadedCase,
    workspace_case_sha256: &'a str,
    operations: &'a CompletedOperationInventory,
    runs: &'a [WorkspaceRunEvidence],
    project_inventories: &'a WorkspaceProjectInventories,
    workspace_boundary: &'a WorkspaceBoundaryEvidence,
    analysis: &'a CompletedPhase0Analysis,
    runtime_validations: &'a CompletedRuntimeValidations,
    runtime_provenance: CompleteRuntimeProvenance,
}

impl<'a> Phase0aReportInputs<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        case: &'a LoadedCase,
        workspace_case_sha256: &'a str,
        operations: &'a CompletedOperationInventory,
        runs: &'a [WorkspaceRunEvidence],
        project_inventories: &'a WorkspaceProjectInventories,
        workspace_boundary: &'a WorkspaceBoundaryEvidence,
        analysis: &'a CompletedPhase0Analysis,
        runtime_validations: &'a CompletedRuntimeValidations,
        runtime_provenance: CompleteRuntimeProvenance,
    ) -> Self {
        Self {
            case,
            workspace_case_sha256,
            operations,
            runs,
            project_inventories,
            workspace_boundary,
            analysis,
            runtime_validations,
            runtime_provenance,
        }
    }
}

/// Failures while deriving a report and immutable payload graph from proofs.
#[derive(Debug, Error)]
pub(crate) enum ReportAssemblyError {
    #[error("completion proofs were not bound to the same validated case")]
    CaseBindingMismatch,
    #[error("workspace runs did not reproduce the completed operation inventory")]
    OperationInventoryMismatch,
    #[error("the internally generated artifact catalog was incomplete")]
    InternalArtifactCatalog,
    #[error("could not serialize a fixed report artifact: {0}")]
    ArtifactSerialization(serde_json::Error),
    #[error("could not parse a retained semantic JSON fragment: {0}")]
    SemanticFragment(serde_json::Error),
    #[error(transparent)]
    Comparison(#[from] Phase0AnalysisError),
    #[error(transparent)]
    Runtime(#[from] RuntimeValidationsError),
    #[error(transparent)]
    Provenance(#[from] ProvenanceError),
    #[error("the proof-derived report failed its internal integrity checks")]
    ReportIntegrity,
    #[error(transparent)]
    Writer(#[from] EvidenceWriterError),
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct PackageInventoriesArtifact<'a> {
    format_version: u32,
    evidence_scope: EvidenceScope,
    project_info: &'a WorkspaceProjectInventories,
    workspace_boundary: &'a WorkspaceBoundaryEvidence,
    matching_non_project_sha256: &'a str,
    matching_non_project_entries: &'a [crate::package::TreeEntry],
}

/// Builds and preflights the complete evidence graph from checked stage tokens.
/// No destination is created by this function.
pub(crate) fn prepare_phase0a_evidence(
    inputs: Phase0aReportInputs<'_>,
) -> Result<PreparedEvidenceBundle, ReportAssemblyError> {
    if inputs.workspace_case_sha256 != inputs.case.source_sha256()
        || inputs.analysis.case_sha256() != inputs.case.source_sha256()
    {
        return Err(ReportAssemblyError::CaseBindingMismatch);
    }
    validate_operation_inventory(inputs.operations, inputs.runs)?;
    let payloads = phase0a_payloads(&inputs)?;
    let report = phase0a_report(&inputs, &payloads)?;
    prepare_evidence_bundle(EvidenceBundle::new(report, payloads)).map_err(Into::into)
}

fn validate_operation_inventory(
    operations: &CompletedOperationInventory,
    runs: &[WorkspaceRunEvidence],
) -> Result<(), ReportAssemblyError> {
    if runs.len() != OperationId::ORDER.len() {
        return Err(ReportAssemblyError::OperationInventoryMismatch);
    }
    let reproduced = OperationId::ORDER
        .into_iter()
        .zip(runs)
        .map(|(id, run)| OperationRecord::from_run(id, run.run()))
        .collect::<Vec<_>>();
    if reproduced != operations.records() {
        return Err(ReportAssemblyError::OperationInventoryMismatch);
    }
    Ok(())
}

fn phase0a_payloads(
    inputs: &Phase0aReportInputs<'_>,
) -> Result<Vec<ArtifactPayload>, ReportAssemblyError> {
    let package_artifact = PackageInventoriesArtifact {
        format_version: PACKAGE_INVENTORIES_FORMAT_VERSION,
        evidence_scope: EvidenceScope::GenericRehearsal,
        project_info: inputs.project_inventories,
        workspace_boundary: inputs.workspace_boundary,
        matching_non_project_sha256: inputs.analysis.matching_packages().sha256(),
        matching_non_project_entries: inputs.analysis.matching_packages().entries(),
    };
    let package_bytes = pretty_json_bytes(&package_artifact)?;
    let mut payloads = vec![
        ArtifactPayload::new(
            ArtifactSlot::CaseManifest,
            inputs.case.source_bytes().to_vec(),
        ),
        ArtifactPayload::new(ArtifactSlot::PackageInventories, package_bytes),
        ArtifactPayload::new(
            ArtifactSlot::NativeValidations,
            inputs.runtime_validations.artifact_bytes()?,
        ),
        ArtifactPayload::new(
            ArtifactSlot::RoundtripComparison,
            inputs
                .analysis
                .comparison_artifact_bytes(ComparisonArtifactKind::Roundtrip)?,
        ),
        ArtifactPayload::new(
            ArtifactSlot::ExistingImportComparison,
            inputs
                .analysis
                .comparison_artifact_bytes(ComparisonArtifactKind::ExistingImport)?,
        ),
        ArtifactPayload::new(
            ArtifactSlot::NewImportComparison,
            inputs
                .analysis
                .comparison_artifact_bytes(ComparisonArtifactKind::NewImport)?,
        ),
    ];
    for (index, run) in inputs.runs.iter().enumerate() {
        payloads.push(ArtifactPayload::new(
            ArtifactSlot::ProcessStdout(index),
            run.run().process().raw_stdout_retained_prefix().to_vec(),
        ));
        payloads.push(ArtifactPayload::new(
            ArtifactSlot::ProcessStderr(index),
            run.run().process().raw_stderr_retained_prefix().to_vec(),
        ));
    }
    Ok(payloads)
}

fn pretty_json_bytes(value: &impl Serialize) -> Result<Vec<u8>, ReportAssemblyError> {
    let mut bytes =
        serde_json::to_vec_pretty(value).map_err(ReportAssemblyError::ArtifactSerialization)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn phase0a_report(
    inputs: &Phase0aReportInputs<'_>,
    payloads: &[ArtifactPayload],
) -> Result<EvidenceReport, ReportAssemblyError> {
    let catalog = fixed_artifact_catalog(payloads, inputs.runs.len())?;
    let package_inventories = inputs.workspace_boundary.case_package_inventories();
    let provenance = complete_phase0a_provenance(
        inputs.runtime_provenance.clone(),
        inputs.case,
        &package_inventories,
        inputs.runs.iter().map(|run| run.run().process()),
    )?;
    let mut builder = ReportBuilder::new_with_provenance(inputs.case, provenance);
    for artifact in catalog.values() {
        builder.push_artifact(artifact.clone());
    }
    for (index, (operation, run)) in OperationId::ORDER
        .iter()
        .zip(inputs.runs.iter())
        .enumerate()
    {
        let stdout = catalog_artifact(&catalog, &format!("processes/{index:04}.stdout.txt"))?;
        let stderr = catalog_artifact(&catalog, &format!("processes/{index:04}.stderr.txt"))?;
        let process = run.run().process().clone();
        let recorded = match expected_failure_for_operation(*operation) {
            Some(expected) => RecordedProcess::negative_control(expected, process, stdout, stderr),
            None => RecordedProcess::required_success(process, stdout, stderr),
        };
        builder.push_process(recorded);
    }
    add_roundtrip_semantics(inputs.analysis, &mut builder)?;
    record_proof_assertions(&mut builder, &catalog)?;
    let report = builder.finish();
    if !report.passed() {
        return Err(ReportAssemblyError::ReportIntegrity);
    }
    Ok(report)
}

const fn expected_failure_for_operation(operation: OperationId) -> Option<ExpectedProcessFailure> {
    match operation {
        OperationId::ImportNewCollisionControl => {
            Some(ExpectedProcessFailure::NewAnimationCollisionDiagnostic)
        }
        OperationId::MissingImagesPathControl => {
            Some(ExpectedProcessFailure::MissingImagesPathDiagnostic)
        }
        _ => None,
    }
}

fn fixed_artifact_catalog(
    payloads: &[ArtifactPayload],
    process_count: usize,
) -> Result<BTreeMap<String, ArtifactEvidence>, ReportAssemblyError> {
    let mut expected = BTreeMap::from([
        ("case.toml".to_owned(), "case-manifest"),
        ("package-inventories.json".to_owned(), "package-inventories"),
        ("native-validations.json".to_owned(), "native-validations"),
        (
            "comparisons/roundtrip.json".to_owned(),
            "roundtrip-comparison",
        ),
        (
            "comparisons/existing-import.json".to_owned(),
            "existing-import-comparison",
        ),
        (
            "comparisons/new-import.json".to_owned(),
            "new-import-comparison",
        ),
    ]);
    for index in 0..process_count {
        expected.insert(format!("processes/{index:04}.stdout.txt"), "process-stdout");
        expected.insert(format!("processes/{index:04}.stderr.txt"), "process-stderr");
    }
    if payloads.len() != expected.len() {
        return Err(ReportAssemblyError::InternalArtifactCatalog);
    }
    let mut catalog = BTreeMap::new();
    for payload in payloads {
        let identity = payload.identity();
        if expected.get(identity.path()).copied() != Some(identity.role())
            || catalog
                .insert(identity.path().to_owned(), identity.clone())
                .is_some()
        {
            return Err(ReportAssemblyError::InternalArtifactCatalog);
        }
    }
    if catalog.len() != expected.len() {
        return Err(ReportAssemblyError::InternalArtifactCatalog);
    }
    Ok(catalog)
}

fn catalog_artifact(
    catalog: &BTreeMap<String, ArtifactEvidence>,
    path: &str,
) -> Result<ArtifactEvidence, ReportAssemblyError> {
    catalog
        .get(path)
        .cloned()
        .ok_or(ReportAssemblyError::InternalArtifactCatalog)
}

fn add_roundtrip_semantics(
    analysis: &CompletedPhase0Analysis,
    builder: &mut ReportBuilder,
) -> Result<(), ReportAssemblyError> {
    let mut observed_hash_loss = false;
    for comparison_id in [ComparisonId::RoundTripA, ComparisonId::RoundTripB] {
        for difference in analysis.comparison(comparison_id).semantic_differences() {
            let before = difference
                .before_json()
                .map(serde_json::from_str)
                .transpose()
                .map_err(ReportAssemblyError::SemanticFragment)?;
            let after = difference
                .after_json()
                .map(serde_json::from_str)
                .transpose()
                .map_err(ReportAssemblyError::SemanticFragment)?;
            observed_hash_loss |= difference.pointer() == "/skeleton/hash";
            builder.semantic_differences.push(SemanticDifference::new(
                difference.pointer(),
                before,
                after,
            ));
        }
    }
    if observed_hash_loss {
        builder.roundtrip_losses.push(RoundTripLoss {
            pointer: "/skeleton/hash".to_owned(),
            description: "Spine regenerated the represented skeleton hash during JSON reconstruction; the complete observed string changes are retained in both round-trip comparisons.".to_owned(),
        });
    }
    Ok(())
}

fn record_proof_assertions(
    builder: &mut ReportBuilder,
    catalog: &BTreeMap<String, ArtifactEvidence>,
) -> Result<(), ReportAssemblyError> {
    let all_processes = (0..OperationId::ORDER.len()).collect::<Vec<_>>();
    let advanced_operations = std::iter::once(1).chain(5..=20).collect::<Vec<_>>();
    let specifications: Vec<(AssertionId, &'static str, Vec<&'static str>, Vec<usize>)> = vec![
        (
            AssertionId::CaseManifestValidated,
            "The checked case manifest supplied every fixed Phase 0A policy binding.",
            vec!["case.toml"],
            vec![],
        ),
        (
            AssertionId::PackageContextsInventoried,
            "Project-info, source, staged, and final workspace inventories were captured.",
            vec!["package-inventories.json"],
            vec![2, 3, 4],
        ),
        (
            AssertionId::ExecutableIdentity,
            "Every fixed operation used the case-pinned editor executable identity.",
            vec!["case.toml"],
            all_processes.clone(),
        ),
        (
            AssertionId::ExactEditorVersion,
            "The version probe verified exact Spine 4.3.23 execution.",
            vec!["case.toml"],
            vec![0],
        ),
        (
            AssertionId::LicenseActivated,
            "Licensed editor operations completed under redacted transcript policy.",
            vec!["case.toml"],
            vec![0, 5, 8, 13, 17],
        ),
        (
            AssertionId::AdvancedArgumentsAccepted,
            "Advanced CLI discovery and every fixed import/export operation passed.",
            vec!["case.toml"],
            advanced_operations,
        ),
        (
            AssertionId::TargetSkeletonsFound,
            "Project-info and import operations used the exact manifest skeleton names.",
            vec!["package-inventories.json"],
            vec![2, 3, 4, 13, 15, 17, 19],
        ),
        (
            AssertionId::NativeValidatorAvailable,
            "The same three fixed runtime bundles passed shared and native validation.",
            vec!["native-validations.json"],
            vec![],
        ),
        (
            AssertionId::EditorCallsSerialized,
            "All editor processes retained one trusted persistent lock identity.",
            vec!["case.toml"],
            all_processes.clone(),
        ),
        (
            AssertionId::ReconstructionRoundTripFirst,
            "The first export, reconstruction, and re-export sequence completed.",
            vec!["comparisons/roundtrip.json"],
            vec![5, 8, 9],
        ),
        (
            AssertionId::ReconstructionRoundTripRepeat,
            "The independent export, reconstruction, and re-export repeat completed.",
            vec!["comparisons/roundtrip.json"],
            vec![10, 11, 12],
        ),
        (
            AssertionId::RoundTripDeterministic,
            "Source and reconstructed repeat outputs met fixed determinism checks.",
            vec!["comparisons/roundtrip.json"],
            vec![5, 9, 10, 12],
        ),
        (
            AssertionId::RoundTripDifferencesExplained,
            "Complete raw, canonical, normalized, and semantic differences were retained.",
            vec!["comparisons/roundtrip.json"],
            vec![5, 9, 10, 12],
        ),
        (
            AssertionId::RoundTripLossesRecorded,
            "Every represented round-trip loss was derived and recorded.",
            vec!["comparisons/roundtrip.json"],
            vec![9, 12],
        ),
        (
            AssertionId::ExistingImportMatchesSubmission,
            "The replacement animation fingerprint matched its submission.",
            vec!["comparisons/existing-import.json"],
            vec![6, 13, 14],
        ),
        (
            AssertionId::ExistingImportPreservesSetup,
            "Existing-animation replacement preserved setup data.",
            vec!["comparisons/existing-import.json"],
            vec![13, 14],
        ),
        (
            AssertionId::ExistingImportPreservesOtherAnimations,
            "Existing-animation replacement preserved every unselected animation.",
            vec!["comparisons/existing-import.json"],
            vec![6, 13, 14],
        ),
        (
            AssertionId::ExistingImportIdempotent,
            "Repeating existing-animation replacement was byte-for-byte idempotent.",
            vec!["comparisons/existing-import.json"],
            vec![15, 16],
        ),
        (
            AssertionId::NewImportMatchesSubmission,
            "The added animation fingerprint matched its submission.",
            vec!["comparisons/new-import.json"],
            vec![7, 17, 18],
        ),
        (
            AssertionId::NewImportPreservesSetup,
            "New-animation import preserved setup data.",
            vec!["comparisons/new-import.json"],
            vec![17, 18],
        ),
        (
            AssertionId::NewImportPreservesOtherAnimations,
            "New-animation import preserved every prior animation.",
            vec!["comparisons/new-import.json"],
            vec![7, 17, 18],
        ),
        (
            AssertionId::NewImportCollisionHazardDetected,
            "The isolated repeat-import control added exactly one transcript-bound renamed animation with the submitted content fingerprint.",
            vec!["comparisons/new-import.json"],
            vec![19, 20],
        ),
        (
            AssertionId::SourcePackagesUnchanged,
            "All three immutable source package trees remained unchanged.",
            vec!["package-inventories.json"],
            all_processes.clone(),
        ),
        (
            AssertionId::TranscriptPolicyPassed,
            "Every retained transcript matched required-success or exact negative-control policy and redaction.",
            vec!["case.toml"],
            all_processes,
        ),
        (
            AssertionId::MissingPathNegativeControl,
            "The missing-images-path control produced only the exact expected failure.",
            vec!["case.toml"],
            vec![21],
        ),
    ];
    debug_assert_eq!(specifications.len(), AssertionId::required().len());
    for (id, summary, paths, processes) in specifications {
        let evidence = fixed_citations(catalog, &paths, &processes)?;
        if builder
            .assertions
            .insert(
                id,
                AssertionResult {
                    id,
                    status: AssertionStatus::Passed,
                    summary: summary.to_owned(),
                    evidence,
                },
            )
            .is_some()
        {
            return Err(ReportAssemblyError::InternalArtifactCatalog);
        }
    }
    Ok(())
}

fn fixed_citations(
    catalog: &BTreeMap<String, ArtifactEvidence>,
    paths: &[&str],
    process_indices: &[usize],
) -> Result<Vec<ArtifactEvidence>, ReportAssemblyError> {
    let mut evidence = BTreeSet::new();
    for path in paths {
        evidence.insert(catalog_artifact(catalog, path)?);
    }
    for index in process_indices {
        evidence.insert(catalog_artifact(
            catalog,
            &format!("processes/{index:04}.stdout.txt"),
        )?);
        evidence.insert(catalog_artifact(
            catalog,
            &format!("processes/{index:04}.stderr.txt"),
        )?);
    }
    if evidence.is_empty() {
        return Err(ReportAssemblyError::InternalArtifactCatalog);
    }
    Ok(evidence.into_iter().collect())
}

/// Incremental, fail-closed builder for an evidence report.
pub struct ReportBuilder {
    metadata: ReportMetadata,
    assertions: BTreeMap<AssertionId, AssertionResult>,
    processes: Vec<RecordedProcess>,
    artifacts: Vec<ArtifactEvidence>,
    approved_volatile_pointers: BTreeSet<String>,
    semantic_differences: Vec<SemanticDifference>,
    roundtrip_losses: Vec<RoundTripLoss>,
}

impl ReportBuilder {
    /// Creates a report builder bound to one immutable validated case.
    pub fn new(case: &LoadedCase) -> Self {
        let manifest = case.manifest();
        Self {
            metadata: ReportMetadata {
                evidence_scope: EvidenceScope::GenericRehearsal,
                representative_gate_eligible: false,
                case_id: manifest.case_id.clone(),
                case_sha256: case.source_sha256().to_owned(),
                target_spine_version: manifest.target_spine_version.clone(),
                expected_executable_sha256: manifest.editor.expected_executable_sha256.clone(),
                tool_version: env!("CARGO_PKG_VERSION").to_owned(),
                provenance: None,
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

    fn new_with_provenance(case: &LoadedCase, provenance: CompletePhase0aProvenance) -> Self {
        let mut builder = Self::new(case);
        builder.metadata.provenance = Some(provenance);
        builder
    }

    /// Adds one atomically assessed process record.
    pub fn push_process(&mut self, process: RecordedProcess) {
        self.processes.push(process);
    }

    /// Adds one validated content-addressed artifact record.
    pub fn push_artifact(&mut self, evidence: ArtifactEvidence) {
        self.artifacts.push(evidence);
    }

    /// Finalizes the report and derives its result from the complete evidence graph.
    pub fn finish(mut self) -> EvidenceReport {
        let mut integrity_failures = Vec::new();
        if self.metadata.provenance.is_none() {
            integrity_failures.push(integrity(
                ReportIntegrityCode::MissingProvenance,
                "complete closed Phase 0A provenance was absent",
            ));
        }
        let artifact_identities = validate_artifacts(&self.artifacts, &mut integrity_failures);
        force_derived_process_assertions(
            &mut self.assertions,
            &self.processes,
            &self.metadata.expected_executable_sha256,
        );
        let assertions = finalize_assertions(
            &mut self.assertions,
            &artifact_identities,
            &mut integrity_failures,
        );
        validate_processes(
            &self.processes,
            &self.metadata.expected_executable_sha256,
            &artifact_identities,
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
            assertions.iter().all(AssertionResult::passed) && integrity_failures.is_empty();
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
) -> BTreeSet<ArtifactEvidence> {
    let mut identities = BTreeSet::new();
    let mut portable_paths = BTreeSet::new();
    for artifact in artifacts {
        let duplicate_identity = !identities.insert(artifact.clone());
        let duplicate_path = !portable_paths.insert(artifact.path().to_ascii_lowercase());
        if duplicate_identity || duplicate_path {
            failures.push(integrity(
                ReportIntegrityCode::DuplicateArtifact,
                format!("duplicate artifact `{}`", artifact.path()),
            ));
        }
    }
    identities
}

fn force_derived_process_assertions(
    assertions: &mut BTreeMap<AssertionId, AssertionResult>,
    processes: &[RecordedProcess],
    expected_executable_sha256: &str,
) {
    if !processes_share_one_trusted_lock(processes)
        && let Some(assertion) = assertions.get_mut(&AssertionId::EditorCallsSerialized)
    {
        assertion.status = AssertionStatus::Failed;
        assertion.summary =
            "derived failure: editor processes did not share one trusted persistent lock"
                .to_owned();
    }
    let every_executable_matches = !processes.is_empty()
        && processes.iter().all(|process| {
            process.evidence().executable_identity().sha256() == expected_executable_sha256
        });
    if !every_executable_matches
        && let Some(assertion) = assertions.get_mut(&AssertionId::ExecutableIdentity)
    {
        assertion.status = AssertionStatus::Failed;
        assertion.summary =
            "derived failure: not every process used the case-pinned executable digest".to_owned();
    }
}

fn processes_share_one_trusted_lock(processes: &[RecordedProcess]) -> bool {
    let Some(first) = processes
        .first()
        .and_then(|process| process.evidence().lock_evidence())
    else {
        return false;
    };
    processes.iter().all(|process| {
        process
            .evidence()
            .lock_evidence()
            .is_some_and(|evidence| first.same_identity(evidence))
    })
}

fn finalize_assertions(
    recorded: &mut BTreeMap<AssertionId, AssertionResult>,
    artifacts: &BTreeSet<ArtifactEvidence>,
    failures: &mut Vec<ReportIntegrityFailure>,
) -> Vec<AssertionResult> {
    let mut assertions = Vec::with_capacity(AssertionId::required().len());
    for id in AssertionId::required() {
        let mut result = recorded.remove(id).unwrap_or_else(|| AssertionResult {
            id: *id,
            status: AssertionStatus::Missing,
            summary: "required assertion was not recorded".to_owned(),
            evidence: Vec::new(),
        });
        let citations_valid = !result.evidence.is_empty()
            && result
                .evidence
                .iter()
                .all(|identity| artifacts.contains(identity));
        if !citations_valid {
            failures.push(integrity(
                ReportIntegrityCode::MissingArtifact,
                format!("assertion `{:?}` lacks valid recorded evidence", result.id),
            ));
            result.status = AssertionStatus::Failed;
            result.summary = "assertion had missing or unrecorded evidence identities".to_owned();
        }
        assertions.push(result);
    }
    assertions
}

fn validate_processes(
    processes: &[RecordedProcess],
    expected_executable_sha256: &str,
    artifacts: &BTreeSet<ArtifactEvidence>,
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
    for (index, recorded) in processes.iter().enumerate() {
        let process = recorded.evidence();
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
        match recorded.expectation() {
            ProcessExpectation::RequiredSuccess if !process.assessment().passed() => {
                failures.push(integrity(
                    ReportIntegrityCode::FailedProcess,
                    format!(
                        "required-success process `{}` failed assessment",
                        process.operation()
                    ),
                ));
            }
            ProcessExpectation::NegativeControl(_) if process.assessment().passed() => {
                failures.push(integrity(
                    ReportIntegrityCode::UnexpectedProcessSuccess,
                    format!(
                        "negative-control process `{}` unexpectedly passed assessment",
                        process.operation()
                    ),
                ));
            }
            ProcessExpectation::NegativeControl(expected)
                if !expected_process_failure_matches(expected, process) =>
            {
                failures.push(integrity(
                    ReportIntegrityCode::WrongProcessFailure,
                    format!(
                        "negative-control process `{}` failed for the wrong reason",
                        process.operation()
                    ),
                ));
            }
            ProcessExpectation::RequiredSuccess | ProcessExpectation::NegativeControl(_) => {}
        }
        for (identity, expected_role, expected_path, expected_digest, stream) in [
            (
                recorded.stdout_artifact(),
                "process-stdout",
                format!("processes/{index:04}.stdout.txt"),
                process.assessment().stdout_retained_prefix_sha256(),
                "stdout",
            ),
            (
                recorded.stderr_artifact(),
                "process-stderr",
                format!("processes/{index:04}.stderr.txt"),
                process.assessment().stderr_retained_prefix_sha256(),
                "stderr",
            ),
        ] {
            if identity.role() != expected_role
                || identity.path() != expected_path
                || identity.sha256() != expected_digest
                || !artifacts.contains(identity)
            {
                failures.push(integrity(
                    ReportIntegrityCode::MissingArtifact,
                    format!(
                        "process `{}` {stream} transcript identity is missing or mismatched",
                        process.operation(),
                    ),
                ));
            }
        }
    }
}

fn expected_process_failure_matches(
    expected: ExpectedProcessFailure,
    process: &ProcessEvidence,
) -> bool {
    match expected {
        ExpectedProcessFailure::NewAnimationCollisionDiagnostic => {
            new_animation_collision_failure_matches(process)
        }
        ExpectedProcessFailure::MissingImagesPathDiagnostic => {
            missing_images_path_failure_matches(process)
        }
    }
}

fn new_animation_collision_failure_matches(process: &ProcessEvidence) -> bool {
    process.exit_code() == Some(0)
        && !process.assessment().passed()
        && !process.required_outputs().is_empty()
        && process.output_discovery_state() == crate::process::OutputDiscoveryState::Complete
        && process.operation() == "spine-new-animation-collision-control"
        && process.transcript_profile()
            == crate::process::TranscriptProfile::NewAnimationCollisionControl
        && process.new_animation_collision().is_some()
        && process.assessment().failures().len() == 1
        && process.assessment().failures()[0].code == ProcessFailureCode::BlockingDiagnostic
}

fn missing_images_path_failure_matches(process: &ProcessEvidence) -> bool {
    if process.exit_code() != Some(0)
        || process.assessment().passed()
        || process.required_outputs().is_empty()
        || process.output_discovery_state() != crate::process::OutputDiscoveryState::Complete
        || process.operation() != "spine-missing-images-path-control"
        || process.transcript_profile()
            != crate::process::TranscriptProfile::MissingImagesPathControl
    {
        return false;
    }

    let mut diagnostic_count = 0_u8;
    let mut missing_output_count = 0_u8;
    for failure in process.assessment().failures() {
        match failure.code {
            ProcessFailureCode::BlockingDiagnostic => {
                diagnostic_count = diagnostic_count.saturating_add(1);
            }
            ProcessFailureCode::MissingOutput => {
                missing_output_count = missing_output_count.saturating_add(1);
            }
            _ => return false,
        }
    }
    diagnostic_count == 1 && missing_output_count <= 1
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
    use crate::process::{
        ProcessCapture, ProcessStreamCapture, TranscriptPolicy, execute_and_assess,
    };

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

    fn complete_stream(bytes: &[u8]) -> ProcessStreamCapture {
        let digest = sha256_bytes(bytes);
        ProcessStreamCapture {
            retained_prefix: bytes.to_vec(),
            total_observed_bytes: bytes.len() as u64,
            bytes_seen_sha256: digest.clone(),
            full_stream_sha256: Some(digest),
            retained_prefix_truncated: false,
            complete: true,
        }
    }

    fn assessed_process(capture: ProcessCapture) -> ProcessEvidence {
        execute_and_assess(
            &FakeExecutor(capture),
            &crate::process::tests::request(),
            TranscriptPolicy::spine_4_3_23(),
        )
        .expect("fake process")
    }

    fn assessed_missing_path_process(mut capture: ProcessCapture) -> ProcessEvidence {
        let command = crate::SpineCommand::missing_images_path_control(
            "/staged/source.spine",
            &crate::JsonExportTarget::new("/staged/export", "Character").expect("export target"),
            "/staged/preset/export.json",
        )
        .expect("negative-control command");
        let request = command
            .process_request(
                "/evidence/editor",
                "/evidence/work",
                std::collections::BTreeMap::from([("LANG".to_owned(), "C".to_owned())]),
            )
            .expect("negative-control request");
        capture.stdout = complete_stream(
            concat!(
                "Spine Launcher 4.3.06 (macOS Apple Silicon)\n",
                "Esoteric Software LLC (C) 2013-2026 | http://esotericsoftware.com\n",
                "Mac OS X aarch64 26.5.2\n",
                "Starting: Spine 4.3.23 Professional\n",
                "Spine 4.3.23 Professional\n",
                "Licensed to: <hidden>\n",
                "JSON export: source\n",
                "Images path not found: ./images\n",
                "Complete.\n"
            )
            .as_bytes(),
        );
        capture.observed_outputs = request.required_outputs.clone();
        execute_and_assess(
            &FakeExecutor(capture),
            &request,
            command.transcript_policy(),
        )
        .expect("negative-control process")
    }

    fn assessed_collision_process(mut capture: ProcessCapture) -> ProcessEvidence {
        let command = crate::SpineCommand::new_animation_collision_control(
            "/staged/new-submission.spine",
            "/staged/new-collision-control.spine",
            "New Rig",
            "New Rig",
            "gesture",
        )
        .expect("collision-control command");
        let request = command
            .process_request(
                "/evidence/editor",
                "/evidence/work",
                std::collections::BTreeMap::from([("LANG".to_owned(), "C".to_owned())]),
            )
            .expect("collision-control request");
        capture.stdout = complete_stream(
            concat!(
                "Spine Launcher 4.3.06 (macOS Apple Silicon)\n",
                "Esoteric Software LLC (C) 2013-2026 | http://esotericsoftware.com\n",
                "Mac OS X aarch64 26.5.2\n",
                "Starting: Spine 4.3.23 Professional\n",
                "Spine 4.3.23 Professional\n",
                "Licensed to: <hidden>\n",
                "Animation import: new-submission into new-collision-control (New Rig)\n",
                "An animation with this name already exists: gesture -> gesture2\n",
                "Complete.\n"
            )
            .as_bytes(),
        );
        capture.observed_outputs = request.required_outputs.clone();
        execute_and_assess(
            &FakeExecutor(capture),
            &request,
            command.transcript_policy(),
        )
        .expect("collision-control process")
    }

    fn assessed_version_process(mut capture: ProcessCapture) -> ProcessEvidence {
        let command = crate::SpineCommand::version();
        let request = command
            .process_request(
                "/evidence/editor",
                "/evidence/work",
                std::collections::BTreeMap::from([("LANG".to_owned(), "C".to_owned())]),
            )
            .expect("version request");
        capture.stdout = complete_stream(
            concat!(
                "Spine Launcher 4.3.06 (macOS Apple Silicon)\n",
                "Esoteric Software LLC (C) 2013-2026 | http://esotericsoftware.com\n",
                "Mac OS X aarch64 26.5.2\n",
                "Starting: Spine 4.3.23 Professional\n",
                "Spine 4.3.23 Professional\n",
                "Licensed to: <hidden>\n",
                "Complete.\n"
            )
            .as_bytes(),
        );
        capture.observed_outputs.clear();
        execute_and_assess(
            &FakeExecutor(capture),
            &request,
            command.transcript_policy(),
        )
        .expect("version process")
    }

    fn transcript_artifacts(evidence: &ProcessEvidence) -> (ArtifactEvidence, ArtifactEvidence) {
        (
            ArtifactEvidence::from_bytes(
                "process-stdout",
                "processes/0000.stdout.txt",
                evidence.raw_stdout_retained_prefix(),
            )
            .expect("stdout artifact"),
            ArtifactEvidence::from_bytes(
                "process-stderr",
                "processes/0000.stderr.txt",
                evidence.raw_stderr_retained_prefix(),
            )
            .expect("stderr artifact"),
        )
    }

    fn required_process(evidence: ProcessEvidence) -> RecordedProcess {
        let (stdout, stderr) = transcript_artifacts(&evidence);
        RecordedProcess::required_success(evidence, stdout, stderr)
    }

    fn negative_process(
        expected_failure: ExpectedProcessFailure,
        evidence: ProcessEvidence,
    ) -> RecordedProcess {
        let (stdout, stderr) = transcript_artifacts(&evidence);
        RecordedProcess::negative_control(expected_failure, evidence, stdout, stderr)
    }

    // Test-only synthetic state for exercising report-integrity failures. No
    // production API can insert these caller-authored assertion booleans.
    fn synthetic_builder_with_process(process: RecordedProcess) -> ReportBuilder {
        let case = case();
        let provenance = crate::provenance::synthetic_complete_provenance(&case);
        let mut builder = ReportBuilder::new_with_provenance(&case, provenance);
        let assertion_artifact = ArtifactEvidence::from_bytes(
            "assertion-evidence",
            "artifacts/assertions.json",
            b"checked evidence",
        )
        .expect("assertion artifact");
        builder.push_artifact(assertion_artifact.clone());
        builder.push_artifact(process.stdout_artifact().clone());
        builder.push_artifact(process.stderr_artifact().clone());
        builder.push_process(process);
        for id in AssertionId::required() {
            builder.assertions.insert(
                *id,
                AssertionResult {
                    id: *id,
                    status: AssertionStatus::Passed,
                    summary: "verified".to_owned(),
                    evidence: vec![assertion_artifact.clone()],
                },
            );
        }
        builder
    }

    fn synthetic_builder(exit_code: i32) -> ReportBuilder {
        let mut capture = crate::process::tests::capture();
        capture.exit_code = Some(exit_code);
        synthetic_builder_with_process(required_process(assessed_process(capture)))
    }

    fn missing_path_process() -> ProcessEvidence {
        assessed_missing_path_process(crate::process::tests::capture())
    }

    #[test]
    fn missing_required_assertions_are_synthesized_as_failures() {
        let case = case();
        let report = ReportBuilder::new(&case).finish();
        assert!(!report.passed());
        assert_eq!(report.assertions().len(), AssertionId::required().len());
        assert!(
            report
                .integrity_failures()
                .iter()
                .any(|failure| { failure.code() == ReportIntegrityCode::MissingProvenance })
        );
    }

    #[test]
    fn required_success_process_passes_when_its_assessment_passes() {
        let report = synthetic_builder(0).finish();
        assert!(report.passed());
        assert_eq!(report.format_version, 4);
        assert_eq!(
            report.processes()[0].expectation(),
            ProcessExpectation::RequiredSuccess
        );
        let serialized = serde_json::to_value(&report).expect("serialized evidence report");
        assert_eq!(
            serialized["metadata"]["evidence_scope"],
            "generic_rehearsal"
        );
        assert_eq!(
            serialized["metadata"]["representative_gate_eligible"],
            false
        );
        assert_eq!(
            serialized["metadata"]["provenance"]["environment"]["build_context"]["relationship"],
            "context_only_not_binary_attestation"
        );
        assert_eq!(
            serialized["metadata"]["provenance"]["spine_launcher"]["observed_processes"],
            OperationId::ORDER.len()
        );
        let serialized_text = serde_json::to_string(&serialized).expect("report JSON");
        for forbidden in ["\"bevy\"", "\"wasm\"", "\"browser\"", "\"gpu\""] {
            assert!(!serialized_text.contains(forbidden));
        }
        assert_eq!(
            serialized["assertions"][0]["evidence"][0]["path"],
            "artifacts/assertions.json"
        );
        assert_eq!(
            serialized["processes"][0]["stdout_artifact"]["path"],
            "processes/0000.stdout.txt"
        );
        assert!(serialized["assertions"][0].get("evidence_sha256").is_none());
    }

    #[test]
    fn required_success_process_failure_forces_report_failure() {
        let report = synthetic_builder(7).finish();
        assert!(!report.passed());
        assert!(
            report
                .integrity_failures()
                .iter()
                .any(|failure| { failure.code() == ReportIntegrityCode::FailedProcess })
        );
    }

    #[test]
    fn expected_missing_path_failure_satisfies_negative_control() {
        let report = synthetic_builder_with_process(negative_process(
            ExpectedProcessFailure::MissingImagesPathDiagnostic,
            missing_path_process(),
        ))
        .finish();

        assert!(report.passed());
        assert_eq!(
            report.processes()[0].expectation(),
            ProcessExpectation::NegativeControl(
                ExpectedProcessFailure::MissingImagesPathDiagnostic
            )
        );
        let serialized = serde_json::to_value(&report).expect("serialized evidence report");
        assert_eq!(
            serialized["processes"][0]["expectation"]["kind"],
            "negative_control"
        );
        assert_eq!(
            serialized["processes"][0]["expectation"]["expected_failure"],
            "missing_images_path_diagnostic"
        );
    }

    #[test]
    fn expected_collision_failure_satisfies_negative_control() {
        let report = synthetic_builder_with_process(negative_process(
            ExpectedProcessFailure::NewAnimationCollisionDiagnostic,
            assessed_collision_process(crate::process::tests::capture()),
        ))
        .finish();

        assert!(report.passed());
        assert_eq!(
            report.processes()[0].expectation(),
            ProcessExpectation::NegativeControl(
                ExpectedProcessFailure::NewAnimationCollisionDiagnostic
            )
        );
        let serialized = serde_json::to_value(&report).expect("serialized evidence report");
        assert_eq!(
            serialized["processes"][0]["expectation"]["expected_failure"],
            "new_animation_collision_diagnostic"
        );
        assert_eq!(
            serialized["processes"][0]["evidence"]["new_animation_collision"]["renamed_animation"],
            "gesture2"
        );
    }

    #[test]
    fn success_report_maps_only_the_two_exact_negative_control_slots() {
        let mapped = OperationId::ORDER
            .iter()
            .enumerate()
            .filter_map(|(index, operation)| {
                expected_failure_for_operation(*operation)
                    .map(|expected| (index, *operation, expected))
            })
            .collect::<Vec<_>>();
        assert_eq!(
            mapped,
            vec![
                (
                    19,
                    OperationId::ImportNewCollisionControl,
                    ExpectedProcessFailure::NewAnimationCollisionDiagnostic,
                ),
                (
                    21,
                    OperationId::MissingImagesPathControl,
                    ExpectedProcessFailure::MissingImagesPathDiagnostic,
                ),
            ]
        );
    }

    #[test]
    fn unexpected_negative_control_success_forces_report_failure() {
        let process = assessed_process(crate::process::tests::capture());
        let report = synthetic_builder_with_process(negative_process(
            ExpectedProcessFailure::MissingImagesPathDiagnostic,
            process,
        ))
        .finish();

        assert!(!report.passed());
        assert!(
            report
                .integrity_failures()
                .iter()
                .any(|failure| { failure.code() == ReportIntegrityCode::UnexpectedProcessSuccess })
        );
    }

    #[test]
    fn wrong_negative_control_failure_forces_report_failure() {
        let mut capture = crate::process::tests::capture();
        capture.exit_code = Some(7);
        capture.observed_outputs.clear();
        let report = synthetic_builder_with_process(negative_process(
            ExpectedProcessFailure::MissingImagesPathDiagnostic,
            assessed_process(capture),
        ))
        .finish();

        assert!(!report.passed());
        assert!(
            report
                .integrity_failures()
                .iter()
                .any(|failure| failure.code() == ReportIntegrityCode::WrongProcessFailure)
        );
    }

    #[test]
    fn duplicate_content_and_roles_are_allowed() {
        let mut builder = synthetic_builder(0);
        let duplicate_content = ArtifactEvidence::from_bytes(
            "assertion-evidence",
            "artifacts/duplicate.txt",
            b"checked evidence",
        )
        .expect("duplicate-content artifact");
        builder.push_artifact(duplicate_content);
        assert!(builder.finish().passed());
    }

    #[test]
    fn duplicate_exact_identity_forces_report_failure() {
        let mut builder = synthetic_builder(0);
        let duplicate = builder.artifacts[0].clone();
        builder.push_artifact(duplicate);
        assert!(!builder.finish().passed());
    }

    #[test]
    fn process_streams_cite_their_exact_role_and_path_even_when_content_matches() {
        let mut builder = synthetic_builder(0);
        let stderr = builder.processes[0].stderr_artifact.clone();
        builder.processes[0].stdout_artifact = stderr;
        let report = builder.finish();

        assert!(!report.passed());
        assert!(report.integrity_failures().iter().any(|failure| {
            failure.code() == ReportIntegrityCode::MissingArtifact
                && failure.detail().contains("stdout transcript identity")
        }));
    }

    #[test]
    fn case_folded_duplicate_path_forces_report_failure() {
        let mut builder = synthetic_builder(0);
        let collision = ArtifactEvidence::from_bytes(
            "different-role",
            "ARTIFACTS/ASSERTIONS.JSON",
            b"different bytes",
        )
        .expect("case-folded path collision");
        builder.push_artifact(collision);
        assert!(!builder.finish().passed());
    }

    #[test]
    fn unrecorded_assertion_identity_forces_report_failure() {
        let mut builder = synthetic_builder(0);
        builder
            .assertions
            .get_mut(&AssertionId::CaseManifestValidated)
            .expect("recorded assertion")
            .evidence = vec![
            ArtifactEvidence::from_bytes("unrecorded", "artifacts/unrecorded.json", b"unrecorded")
                .expect("unrecorded identity"),
        ];
        let report = builder.finish();
        assert!(!report.passed());
        assert!(!report.assertions()[0].passed());
    }

    #[test]
    fn unapproved_semantic_difference_forces_report_failure() {
        let mut builder = synthetic_builder(0);
        builder.semantic_differences.push(SemanticDifference::new(
            "/bones/0/x",
            Some(serde_json::json!(0)),
            Some(serde_json::json!(1)),
        ));
        assert!(!builder.finish().passed());
    }

    #[test]
    fn malicious_non_allowlisted_approval_is_recomputed_and_rejected() {
        let mut builder = synthetic_builder(0);
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
        let mut builder = synthetic_builder(0);
        builder.semantic_differences.push(SemanticDifference::new(
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
            let mut builder = synthetic_builder(0);
            builder.semantic_differences.push(SemanticDifference::new(
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
        let mut builder = synthetic_builder(0);
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
        builder.push_process(required_process(process));
        let report = builder.finish();
        assert!(!report.passed());
        let assertion = report
            .assertions()
            .iter()
            .find(|value| value.id == AssertionId::EditorCallsSerialized)
            .expect("serialization assertion");
        assert!(!assertion.passed());
        assert!(
            report.integrity_failures().iter().any(|failure| {
                failure.code() == ReportIntegrityCode::MissingEditorLockEvidence
            })
        );
    }

    #[test]
    fn different_valid_locks_cannot_claim_global_serialization() {
        let mut builder = synthetic_builder(0);
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
        builder.push_process(required_process(process));
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
        assert!(!assertion.passed());
    }

    #[test]
    fn executable_digest_assertion_is_derived_from_process_evidence() {
        let mut builder = synthetic_builder(0);
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
        builder.push_process(required_process(process));
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
        assert!(!assertion.passed());
    }

    #[test]
    fn proof_assembler_catalog_accepts_only_the_exact_closed_layout() {
        let mut payloads = [
            ArtifactSlot::CaseManifest,
            ArtifactSlot::PackageInventories,
            ArtifactSlot::NativeValidations,
            ArtifactSlot::RoundtripComparison,
            ArtifactSlot::ExistingImportComparison,
            ArtifactSlot::NewImportComparison,
            ArtifactSlot::ProcessStdout(0),
            ArtifactSlot::ProcessStderr(0),
        ]
        .into_iter()
        .map(|slot| ArtifactPayload::new(slot, b"evidence\n".to_vec()))
        .collect::<Vec<_>>();

        let catalog = fixed_artifact_catalog(&payloads, 1).expect("exact closed catalog");
        assert_eq!(catalog.len(), 8);
        assert_eq!(
            catalog
                .get("native-validations.json")
                .expect("native evidence")
                .role(),
            "native-validations"
        );

        let omitted = payloads.pop().expect("stderr payload");
        assert!(matches!(
            fixed_artifact_catalog(&payloads, 1),
            Err(ReportAssemblyError::InternalArtifactCatalog)
        ));
        payloads.push(omitted);
        payloads.push(ArtifactPayload::new(
            ArtifactSlot::ProcessStderr(0),
            b"duplicate\n".to_vec(),
        ));
        assert!(matches!(
            fixed_artifact_catalog(&payloads, 1),
            Err(ReportAssemblyError::InternalArtifactCatalog)
        ));
        assert!(matches!(
            fixed_artifact_catalog(&payloads[..8], 2),
            Err(ReportAssemblyError::InternalArtifactCatalog)
        ));
    }

    #[test]
    fn controlled_failure_matrix_is_exact_and_always_retains_a_non_pass() {
        let case = case();
        let proofs = ControlledFailureProofs::unavailable();
        let failure =
            ArtifactEvidence::from_bytes("controlled-failure", "attempt/failure.json", b"failure")
                .expect("failure identity");
        for code in [
            ControlledFailureCode::EditorIdentity,
            ControlledFailureCode::EditorEnvironment,
            ControlledFailureCode::WorkspacePreparation,
            ControlledFailureCode::EditorOperation,
            ControlledFailureCode::WorkspaceVerification,
            ControlledFailureCode::SemanticAnalysis,
            ControlledFailureCode::RuntimeValidation,
            ControlledFailureCode::ReportAssembly,
            ControlledFailureCode::Provenance,
        ] {
            let results = controlled_failure_assertions(ControlledAssertionInputs {
                code,
                operation: Some(OperationId::Version),
                failure_identity: &failure,
                artifacts: std::slice::from_ref(&failure),
                unsafe_transcript_pairs_omitted: 0,
                case: &case,
                processes: &[],
                proofs: &proofs,
                source_status: ControlledSourceRecheckStatus::Unavailable,
            });
            assert_eq!(results.len(), AssertionId::required().len());
            assert!(
                results
                    .iter()
                    .zip(AssertionId::required())
                    .all(|(result, id)| { result.id() == *id && !result.evidence().is_empty() })
            );
            assert!(results.iter().any(|result| !result.passed()));
        }
    }

    #[test]
    fn controlled_failure_matrix_preserves_only_closed_early_proofs() {
        let case = case();
        let failure =
            ArtifactEvidence::from_bytes("controlled-failure", "attempt/failure.json", b"failure")
                .expect("failure identity");
        let version = assessed_version_process(crate::process::tests::capture());
        let processes = [version];
        let proofs = ControlledFailureProofs::unavailable();
        let results = controlled_failure_assertions(ControlledAssertionInputs {
            code: ControlledFailureCode::EditorOperation,
            operation: Some(OperationId::AdvancedHelp),
            failure_identity: &failure,
            artifacts: std::slice::from_ref(&failure),
            unsafe_transcript_pairs_omitted: 0,
            case: &case,
            processes: &processes,
            proofs: &proofs,
            source_status: ControlledSourceRecheckStatus::Unavailable,
        });
        let status = |id| {
            results
                .iter()
                .find(|result| result.id() == id)
                .expect("required assertion")
                .status()
        };
        assert_eq!(
            status(AssertionId::CaseManifestValidated),
            AssertionStatus::Passed
        );
        assert_eq!(
            status(AssertionId::ExactEditorVersion),
            AssertionStatus::Passed
        );
        assert_eq!(
            status(AssertionId::AdvancedArgumentsAccepted),
            AssertionStatus::Failed
        );
        assert_ne!(
            status(AssertionId::RoundTripDeterministic),
            AssertionStatus::Passed
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn controlled_failure_publishes_only_the_private_attempt_layout() {
        let case = case();
        let process = assessed_process(crate::process::tests::capture());
        let processes = [process];
        let provenance = crate::provenance::synthetic_controlled_provenance(&case, &processes);
        let prepared = prepare_controlled_failure_evidence(ControlledFailureInputs::new(
            &case,
            ControlledFailureCode::EditorOperation,
            Some(OperationId::Version),
            "version operation failed its closed transcript contract",
            &processes,
            ControlledFailureProofs::unavailable(),
            provenance,
        ))
        .expect("prepare controlled failure");
        let temporary = tempfile::tempdir().expect("temporary directory");
        let destination = temporary.path().join("failure-evidence");
        crate::evidence_writer::write_prepared_controlled_failure_evidence_bundle(
            &destination,
            prepared,
        )
        .expect("publish controlled failure");

        let report: serde_json::Value = serde_json::from_slice(
            &std::fs::read(destination.join("report.json")).expect("report bytes"),
        )
        .expect("report JSON");
        assert_eq!(report["passed"], false);
        assert_eq!(report["metadata"]["evidence_scope"], "generic_rehearsal");
        assert_eq!(report["metadata"]["representative_gate_eligible"], false);
        assert!(report["metadata"]["provenance"].is_object());
        assert_eq!(
            report["assertions"].as_array().expect("assertions").len(),
            AssertionId::required().len()
        );
        assert!(
            report["assertions"]
                .as_array()
                .expect("assertions")
                .iter()
                .any(|assertion| assertion["status"] != "passed")
        );
        assert!(destination.join("attempt/failure.json").is_file());
        assert!(destination.join("attempt/case.toml").is_file());
        assert!(
            destination
                .join("attempt/processes/0000.stdout.txt")
                .is_file()
        );
        assert!(
            destination
                .join("attempt/processes/0000.stderr.txt")
                .is_file()
        );
        assert!(!destination.join("native-validations.json").exists());
        assert!(!destination.join("comparisons").exists());
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn controlled_failure_omits_an_unsafe_pair_but_retains_stream_digests() {
        let case = case();
        let mut capture = crate::process::tests::capture();
        capture.stdout = complete_stream(b"Licensed to: private-person\n");
        let process = assessed_process(capture);
        let expected_stdout = process
            .assessment()
            .stdout_retained_prefix_sha256()
            .to_owned();
        let processes = [process];
        let provenance = crate::provenance::synthetic_controlled_provenance(&case, &processes);
        let prepared = prepare_controlled_failure_evidence(ControlledFailureInputs::new(
            &case,
            ControlledFailureCode::EditorOperation,
            Some(OperationId::Version),
            "Licensed to: private-diagnostic",
            &processes,
            ControlledFailureProofs::unavailable(),
            provenance,
        ))
        .expect("prepare privacy-safe controlled failure");
        let temporary = tempfile::tempdir().expect("temporary directory");
        let destination = temporary.path().join("failure-evidence");
        crate::evidence_writer::write_prepared_controlled_failure_evidence_bundle(
            &destination,
            prepared,
        )
        .expect("publish controlled failure");

        assert!(
            !destination
                .join("attempt/processes/0000.stdout.txt")
                .exists()
        );
        assert!(
            !destination
                .join("attempt/processes/0000.stderr.txt")
                .exists()
        );
        let failure_bytes =
            std::fs::read(destination.join("attempt/failure.json")).expect("failure bytes");
        let failure_text = String::from_utf8(failure_bytes.clone()).expect("UTF-8 failure");
        assert!(!failure_text.contains("private-person"));
        let failure: serde_json::Value =
            serde_json::from_slice(&failure_bytes).expect("failure JSON");
        assert_eq!(failure["failure"]["unsafe_transcript_pairs_omitted"], 1);
        assert_eq!(failure["failure"]["diagnostic_withheld"], true);
        assert_eq!(
            failure["failure"]["diagnostic"],
            "diagnostic withheld by fixed privacy and size policy"
        );
        assert_eq!(
            failure["processes"][0]["stdout_retained_prefix_sha256"],
            expected_stdout
        );
        assert!(failure["processes"][0]["transcript_artifacts"].is_null());
    }
}

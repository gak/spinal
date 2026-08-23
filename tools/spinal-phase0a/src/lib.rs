//! Fail-closed building blocks for Spinal's opt-in Phase 0A evidence harness.
//!
//! The crate now contains typed Spine 4.3.23 commands, secure package staging,
//! exact-path execution evidence, strict project-info and JSON parsing, and a
//! fail-closed report and private evidence writers. The generic runner remains
//! permanently gate-ineligible; the separate representative runner emits only
//! binding-pinned candidates for independent verification and human review.

mod case;
mod digest;
mod evidence_writer;
mod json_evidence;
mod lock;
mod native_validator;
mod operation_recipe;
mod package;
mod phase0_analysis;
mod phase0a_runner;
mod process;
mod provenance;
mod report;
mod representative;
mod representative_evidence;
mod representative_runner;
mod representative_verifier;
mod run_workspace;
mod runtime_validations;
mod spine_cli;
mod spine_run;
mod stage;
mod subprocess;
#[allow(
    dead_code,
    reason = "used by the private run-workspace mutation envelope"
)]
mod workspace_snapshot;

pub use case::{
    AnimationNames, CaseError, CaseManifest, EditorExpectation, ExportPolicy, ExportPreset,
    LoadedCase, PackageSet, PackageSpec, SkeletonNames, VolatilePolicy, load_case, parse_case,
};
pub use json_evidence::{JsonDifference, JsonEvidence, JsonEvidenceError, JsonLimits};
pub use lock::{
    EditorLockError, ExclusiveEditorLock, ExclusiveEditorLockGuard, LockedProcessExecutor,
};
pub use package::{
    CasePackageInventories, EntryKind, PackageEvidenceError, PackageInventory, TreeEntry,
};
pub use phase0a_runner::{
    GenericRehearsalRequest, Phase0aRunError, Phase0aRunErrorCode, PublishedGenericRehearsal,
    run_generic_rehearsal,
};
pub use process::{
    AdapterFailure, AdapterFailureCode, CleanupStatus, EnvironmentVariableEvidence,
    ExecutableIdentity, LockEvidence, OutputDiscoveryState, ProcessAssessment, ProcessCapture,
    ProcessEvidence, ProcessExecutionError, ProcessExecutionErrorCode, ProcessExecutor,
    ProcessFailure, ProcessFailureCode, ProcessRequest, ProcessStreamCapture, ProjectInfoError,
    ProjectInfoInventory, ProjectInfoList, ProjectInfoSection, ProjectSkeletonInventory,
    TerminationReason, TranscriptPolicy, TranscriptProfile, WorkingDirectoryIdentity,
    execute_and_assess,
};
pub use report::{
    ArtifactError, ArtifactEvidence, AssertionId, AssertionResult, AssertionStatus, EvidenceReport,
    ExpectedProcessFailure, ProcessExpectation, RecordedProcess, ReportBuilder,
    ReportIntegrityCode, ReportIntegrityFailure, ReportMetadata, RoundTripLoss, SemanticDifference,
};
pub use representative_runner::{
    PublishedRepresentativeRun, RepresentativeRunError, RepresentativeRunErrorCode,
    RepresentativeRunRequest, propose_representative_binding, run_representative_phase0a,
};
pub use representative_verifier::{
    RepresentativeVerification, RepresentativeVerificationError, VERIFICATION_LIMITATION,
    verify_representative_evidence,
};
pub use spine_cli::{
    ExpectedOutput, JsonExportTarget, OutputMode, SpineCommand, SpineCommandError,
    SpineOperationKind, approved_export_preset_bytes,
};
pub use spine_run::{
    FileIdentityObservation, OutputFileObservation, SpineInputObservation, SpineOutputObservation,
    SpineRunError, SpineRunEvidence, execute_spine_command,
};
pub use stage::{StageError, StagedPackage, secure_inventory_package, stage_package};
pub use subprocess::SubprocessExecutor;

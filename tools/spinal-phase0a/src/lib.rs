//! Fail-closed building blocks for Spinal's opt-in Phase 0A evidence harness.
//!
//! The crate now contains typed Spine 4.3.23 commands, secure package staging,
//! exact-path execution evidence, strict project-info and JSON parsing, and a
//! fail-closed report foundation. The complete linear Phase 0A orchestrator
//! and evidence writer are not implemented yet, so no production gate result
//! can be emitted from these building blocks alone.

mod case;
mod digest;
mod json_evidence;
mod lock;
mod package;
mod process;
mod report;
mod spine_cli;
mod spine_run;
mod stage;
mod subprocess;

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
    ArtifactError, ArtifactEvidence, AssertionId, AssertionResult, EvidenceReport,
    ExpectedProcessFailure, ProcessExpectation, RecordedProcess, ReportBuilder,
    ReportIntegrityCode, ReportIntegrityFailure, ReportMetadata, RoundTripLoss, SemanticDifference,
};
pub use spine_cli::{
    ExpectedOutput, OutputMode, SpineCommand, SpineCommandError, SpineOperationKind,
    approved_export_preset_bytes,
};
pub use spine_run::{
    OutputFileObservation, SpineOutputObservation, SpinePolicyInputObservation, SpineRunError,
    SpineRunEvidence, execute_spine_command,
};
pub use stage::{StageError, StagedPackage, secure_inventory_package, stage_package};
pub use subprocess::SubprocessExecutor;

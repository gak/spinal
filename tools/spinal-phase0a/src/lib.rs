//! Fail-closed building blocks for Spinal's opt-in Phase 0A evidence harness.
//!
//! This crate deliberately does not invoke Spine yet. It defines the generic
//! case contract, immutable package inventory, assertion matrix, and process
//! assessment boundary that a later real editor adapter must use.

mod case;
mod digest;
mod package;
mod process;
mod report;

pub use case::{
    AnimationNames, CaseError, CaseManifest, EditorExpectation, ExportPolicy, ExportPreset,
    LoadedCase, PackageSet, PackageSpec, SkeletonNames, VolatilePolicy, load_case, parse_case,
};
pub use package::{
    CasePackageInventories, EntryKind, PackageEvidenceError, PackageInventory, TreeEntry,
    inventory_case_packages, inventory_package,
};
pub use process::{
    ProcessAssessment, ProcessCapture, ProcessEvidence, ProcessExecutionError, ProcessExecutor,
    ProcessFailure, ProcessFailureCode, ProcessRequest, TranscriptPolicy, execute_and_assess,
};
pub use report::{
    ArtifactError, ArtifactEvidence, AssertionId, AssertionResult, EvidenceReport,
    ReportBuildError, ReportBuilder, ReportIntegrityCode, ReportIntegrityFailure, ReportMetadata,
    RoundTripLoss, SemanticDifference,
};

//! Independent, read-only verification of representative Phase 0A evidence.
//!
//! This module deliberately does not use the report builder, evidence writer,
//! representative runner, or their trusted token types. It parses the two
//! report formats through verifier-owned DTOs and manual closed-shape checks,
//! then re-observes every published byte through a private, no-follow
//! filesystem boundary. The shared pure strict-v2 case parser and immutable
//! approved-preset contract bytes are intentionally part of the syntax and
//! contract trust boundary; publisher, runner, report, and proof-token logic are
//! not.

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, Metadata, OpenOptions};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

const OUTER_FORMAT_VERSION: u32 = 5;
const INNER_FORMAT_VERSION: u32 = 4;
const BINDING_FORMAT_VERSION: u32 = 1;
const TARGET_SPINE_VERSION: &str = "4.3.23";
const REPRESENTATIVE_SCOPE: &str = "representative_phase0a";
const BINDING_EVIDENCE_CLASS: &str = "phase0a_representative";
const GENERIC_SCOPE: &str = "generic_rehearsal";
const MARKER_NAME: &str = "SPINAL_PHASE0A_REPRESENTATIVE_BINDING_SHA256";
const PACKAGE_TREE_DOMAIN: &[u8] = b"spinal-phase0a-package-tree-v1\0";
const CORE_CONTENT_TREE_DOMAIN: &[u8] = b"spinal-phase0a-representative-core-tree-v1\0";
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_BINDING_BYTES: u64 = 64 * 1024;
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
const MAX_PHYSICAL_EVIDENCE_ENTRIES: usize = 256;
const MAX_PACKAGE_INVENTORY_ENTRIES: usize = 100_000;
const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
const PRIVATE_FILE_MODE: u32 = 0o600;

/// Exact limitation of the independent verifier.
///
/// The verifier rederives filesystem, digest, provenance, fixture-role,
/// process-order, marker, assertion, and citation bindings. It shares only the
/// pure strict-v2 case parser and immutable approved-preset contract bytes as
/// syntax/contract TCB. It intentionally does not replay Spine, rerun native
/// validation, independently reclassify raw transcripts, or independently
/// recompute the semantic meaning of comparison artifacts. Those claims remain
/// bound to the reviewed, binding-pinned harness and the immutable artifacts
/// that this verifier authenticates.
pub const VERIFICATION_LIMITATION: &str = "shares only the pure strict-v2 case parser and immutable approved-preset contract bytes as syntax/contract TCB; does not replay Spine, rerun native validation, independently reclassify raw transcripts, or independently recompute comparison semantics";

/// A successfully validated representative evidence directory.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RepresentativeVerification {
    schema_version: u32,
    valid: bool,
    passed: bool,
    representative_gate_eligible: bool,
    outer_report_sha256: String,
    representative_binding_sha256: String,
    core_report_sha256: String,
    core_inventory_sha256: String,
    core_content_tree_sha256: String,
    limitation: &'static str,
}

impl RepresentativeVerification {
    /// Returns whether the verified run itself passed.
    pub fn passed(&self) -> bool {
        self.passed
    }

    /// Returns whether this verified report is eligible for maintainer gate review.
    pub fn representative_gate_eligible(&self) -> bool {
        self.representative_gate_eligible
    }

    /// Returns the SHA-256 of the exact outer report bytes.
    pub fn outer_report_sha256(&self) -> &str {
        &self.outer_report_sha256
    }
}

/// A fail-closed representative verification error.
#[derive(Debug, Error)]
pub enum RepresentativeVerificationError {
    /// The supplied evidence root was not one normalized absolute path.
    #[error("evidence path must be absolute, normalized, and name the canonical directory: `{0}`")]
    InvalidEvidencePath(PathBuf),
    /// The platform cannot enforce the required private filesystem boundary.
    #[error("representative evidence verification is supported only on macOS and Linux")]
    UnsupportedPlatform,
    /// Evidence could not be observed safely.
    #[error(
        "could not securely observe representative evidence during {operation} at `{path}`: {source}"
    )]
    Io {
        /// Short operation description.
        operation: &'static str,
        /// Path involved in the failure.
        path: PathBuf,
        /// Underlying filesystem failure.
        #[source]
        source: io::Error,
    },
    /// A filesystem entry violated the fixed private-evidence contract.
    #[error("unsafe representative evidence entry `{path}`: {reason}")]
    UnsafeFilesystem {
        /// Rejected entry.
        path: PathBuf,
        /// Stable reason for rejection.
        reason: &'static str,
    },
    /// The exact file/directory catalog did not match the applicable closed layout.
    #[error("representative evidence layout is invalid: {0}")]
    InvalidLayout(String),
    /// A fixed resource budget was exceeded.
    #[error("representative evidence exceeds the fixed {0} budget")]
    SizeLimit(&'static str),
    /// JSON bytes were malformed or did not match a closed verifier schema.
    #[error("invalid JSON in `{path}`: {source}")]
    Json {
        /// Evidence-relative file label.
        path: &'static str,
        /// Parser failure.
        #[source]
        source: serde_json::Error,
    },
    /// TOML bytes were malformed or did not match the closed binding schema.
    #[error("invalid representative binding TOML: {0}")]
    Toml(#[from] toml::de::Error),
    /// The embedded case failed the same strict v2 schema and policy as a run input.
    #[error("invalid representative core case: {0}")]
    Case(#[from] crate::case::CaseError),
    /// Authenticated content violated a cross-file representative policy.
    #[error("representative evidence policy mismatch: {0}")]
    Policy(String),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OuterReportDto {
    format_version: u32,
    metadata: OuterMetadataDto,
    passed: bool,
    core: OuterCoreDto,
    top_level_artifacts: Vec<TopLevelArtifactDto>,
    validation: OuterValidationDto,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OuterMetadataDto {
    evidence_scope: String,
    representative_gate_eligible: bool,
    binding_id: String,
    representative_binding_sha256: String,
    case_sha256: String,
    harness_executable_sha256: String,
    expected_editor_executable_sha256: String,
    package_tree_sha256: RoleDigestsDto,
    source_revision: String,
    cargo_lock_sha256: String,
    target_spine_version: String,
    tool_version: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct RoleDigestsDto {
    current: String,
    replacement_submission: String,
    new_submission: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OuterCoreDto {
    outcome: String,
    report_path: String,
    report_sha256: String,
    inventory: PackageInventoryDto,
    content_tree_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TopLevelArtifactDto {
    path: String,
    kind: String,
    sha256: String,
    #[serde(default)]
    byte_length: Option<u64>,
    #[serde(default)]
    entry_count: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OuterValidationDto {
    core_schema_validated: bool,
    clean_build_provenance_validated: bool,
    harness_identity_validated: bool,
    editor_launcher_validated: bool,
    package_bindings_validated: bool,
    workspace_source_bindings_validated: bool,
    marker_value_sha256: String,
    marker_processes_validated: usize,
    marker_evidence_complete: bool,
    process_count: usize,
    assertion_count: usize,
    passed_assertion_count: usize,
    integrity_failure_count: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingDto {
    format_version: u32,
    evidence_class: String,
    binding_id: String,
    case_sha256: String,
    harness_executable_sha256: String,
    package_tree_sha256: RoleDigestsDto,
    build: BindingBuildDto,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingBuildDto {
    source_revision: String,
    cargo_lock_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct PackageInventoryDto {
    tree_sha256: String,
    entries: Vec<TreeEntryDto>,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct TreeEntryDto {
    path: String,
    kind: String,
    size: u64,
    sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct ArtifactDto {
    role: String,
    path: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordedProcessDto {
    expectation: ProcessExpectationDto,
    evidence: ProcessEvidenceDto,
    stdout_artifact: ArtifactDto,
    stderr_artifact: ArtifactDto,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case", tag = "kind", content = "expected_failure")]
enum ProcessExpectationDto {
    RequiredSuccess,
    NegativeControl(ExpectedProcessFailureDto),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum ExpectedProcessFailureDto {
    NewAnimationCollisionDiagnostic,
    MissingImagesPathDiagnostic,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcessEvidenceDto {
    operation: String,
    requested_program: PathBuf,
    args: Vec<String>,
    requested_working_directory: PathBuf,
    environment: Vec<EnvironmentVariableDto>,
    timeout_seconds: u64,
    timeout_subsec_nanos: u32,
    cleanup_timeout_seconds: u64,
    cleanup_timeout_subsec_nanos: u32,
    max_retained_bytes_per_stream: usize,
    executable_identity: ExecutableIdentityDto,
    working_directory_identity: WorkingDirectoryIdentityDto,
    lock_evidence: Option<LockEvidenceDto>,
    exit_code: Option<i32>,
    terminating_signal: Option<i32>,
    sent_signal: Option<i32>,
    termination_reason: String,
    elapsed_seconds: u64,
    elapsed_subsec_nanos: u32,
    cleanup_status: String,
    adapter_failure: Option<Value>,
    stdout: ProcessStreamDto,
    stderr: ProcessStreamDto,
    required_outputs: Vec<String>,
    observed_outputs: Vec<String>,
    output_discovery_state: String,
    transcript_profile: String,
    #[serde(default)]
    new_animation_collision: Option<NewAnimationCollisionDto>,
    assessment: ProcessAssessmentDto,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct EnvironmentVariableDto {
    name: String,
    value_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct ExecutableIdentityDto {
    canonical_path: PathBuf,
    sha256: String,
    size: u64,
    device: u64,
    inode: u64,
    mode: u32,
    owner: u32,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    changed_seconds: i64,
    changed_nanoseconds: i64,
    local_filesystem_verified: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct WorkingDirectoryIdentityDto {
    canonical_path: PathBuf,
    device: u64,
    inode: u64,
    mode: u32,
    owner: u32,
    local_filesystem_verified: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct LockEvidenceDto {
    canonical_path: PathBuf,
    wait_seconds: u64,
    wait_subsec_nanos: u32,
    acquired: bool,
    local_filesystem_verified: bool,
    device: u64,
    inode: u64,
    filesystem_kind: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcessStreamDto {
    total_observed_bytes: u64,
    retained_bytes: usize,
    retained_prefix_sha256: String,
    bytes_seen_sha256: String,
    full_stream_sha256: Option<String>,
    retained_prefix_truncated: bool,
    complete: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NewAnimationCollisionDto {
    requested_animation: String,
    renamed_animation: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcessAssessmentDto {
    passed: bool,
    stdout_retained_prefix_sha256: String,
    stderr_retained_prefix_sha256: String,
    failures: Vec<ProcessFailureDto>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcessFailureDto {
    code: String,
    detail: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SemanticDifferenceDto {
    pointer: String,
    before: String,
    after: String,
    approved_volatile: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoundTripLossDto {
    pointer: String,
    description: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LauncherProvenanceDto {
    size: u64,
    stable_file_identity_sha256: String,
}

#[derive(Eq, PartialEq)]
struct ObservedEvidence {
    root_state: DirectoryState,
    files: BTreeMap<String, ObservedFile>,
    directories: BTreeSet<String>,
}

#[derive(Eq, PartialEq)]
struct ObservedFile {
    bytes: Vec<u8>,
    sha256: String,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Clone, Debug, Eq, PartialEq)]
struct DirectoryState {
    device: u64,
    inode: u64,
    mode: u32,
    owner: u32,
    group: u32,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    changed_seconds: i64,
    changed_nanoseconds: i64,
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
#[derive(Clone, Debug, Eq, PartialEq)]
struct DirectoryState;

/// Independently verifies one already-published representative evidence directory.
pub fn verify_representative_evidence(
    evidence_directory: impl AsRef<Path>,
) -> Result<RepresentativeVerification, RepresentativeVerificationError> {
    let root = validate_absolute_root(evidence_directory.as_ref())?;
    let observed = observe_evidence(&root)?;
    require_root_state_unchanged(&root, &observed.root_state)?;

    let outer_bytes = file_bytes(&observed, "report.json")?;
    let binding_bytes = file_bytes(&observed, "representative-binding.toml")?;
    if u64::try_from(binding_bytes.len()).unwrap_or(u64::MAX) > MAX_BINDING_BYTES {
        return Err(RepresentativeVerificationError::SizeLimit("binding file"));
    }
    let core_report_bytes = file_bytes(&observed, "core/report.json")?;

    let outer: OuterReportDto = parse_json("report.json", outer_bytes)?;
    let binding: BindingDto = toml::from_str(
        std::str::from_utf8(binding_bytes)
            .map_err(|_| policy("representative binding must be UTF-8"))?,
    )?;
    let core: Value = parse_json("core/report.json", core_report_bytes)?;

    verify_binding(&binding)?;
    verify_outer_static(&outer)?;
    verify_outer_binding_links(&outer, &binding, binding_bytes)?;

    let actual_core_inventory = observed_core_inventory(&observed)?;
    verify_inventory(
        &outer.core.inventory,
        "outer core inventory",
        MAX_PHYSICAL_EVIDENCE_ENTRIES,
    )?;
    if outer.core.inventory != actual_core_inventory {
        return Err(policy(
            "outer core inventory does not match securely observed core bytes",
        ));
    }
    let core_content_tree_sha256 = core_content_tree_sha256(&actual_core_inventory.tree_sha256);
    require_sha_match(
        "core.content_tree_sha256",
        &outer.core.content_tree_sha256,
        &core_content_tree_sha256,
    )?;
    require_sha_match(
        "core.report_sha256",
        &outer.core.report_sha256,
        &sha256(core_report_bytes),
    )?;
    verify_top_level_artifacts(
        &outer,
        binding_bytes,
        &core_content_tree_sha256,
        actual_core_inventory.entries.len(),
    )?;

    let case = verify_case_binding(
        file_bytes(&observed, "core/case.toml")?,
        &core,
        &outer,
        &binding,
    )?;
    let core_summary = verify_core_report(&core, &observed, &outer, &binding, &case)?;
    verify_package_bindings(
        file_bytes(&observed, "core/package-inventories.json")?,
        &binding,
    )?;
    verify_outer_summary(&outer, &core_summary)?;
    verify_exact_layout(&observed, &core_summary.expected_core_files)?;
    require_root_state_unchanged(&root, &observed.root_state)?;
    let final_observation = observe_evidence(&root)?;
    if final_observation != observed {
        return Err(unsafe_fs(
            &root,
            "evidence tree changed during verification",
        ));
    }

    Ok(RepresentativeVerification {
        schema_version: 1,
        valid: true,
        passed: outer.passed,
        representative_gate_eligible: outer.metadata.representative_gate_eligible,
        outer_report_sha256: sha256(outer_bytes),
        representative_binding_sha256: sha256(binding_bytes),
        core_report_sha256: sha256(core_report_bytes),
        core_inventory_sha256: actual_core_inventory.tree_sha256,
        core_content_tree_sha256,
        limitation: VERIFICATION_LIMITATION,
    })
}

struct CoreSummary {
    passed: bool,
    process_count: usize,
    passed_assertion_count: usize,
    integrity_failure_count: usize,
    expected_core_files: BTreeSet<String>,
}

fn verify_binding(binding: &BindingDto) -> Result<(), RepresentativeVerificationError> {
    require_equal(
        "binding format_version",
        binding.format_version,
        BINDING_FORMAT_VERSION,
    )?;
    require_text(
        "binding evidence_class",
        &binding.evidence_class,
        BINDING_EVIDENCE_CLASS,
    )?;
    validate_binding_id(&binding.binding_id)?;
    validate_sha("binding case_sha256", &binding.case_sha256)?;
    validate_sha(
        "binding harness_executable_sha256",
        &binding.harness_executable_sha256,
    )?;
    validate_role_digests(&binding.package_tree_sha256, "binding package trees")?;
    validate_revision(&binding.build.source_revision)?;
    validate_sha(
        "binding cargo_lock_sha256",
        &binding.build.cargo_lock_sha256,
    )
}

fn verify_outer_static(outer: &OuterReportDto) -> Result<(), RepresentativeVerificationError> {
    require_equal(
        "outer format_version",
        outer.format_version,
        OUTER_FORMAT_VERSION,
    )?;
    require_text(
        "outer evidence scope",
        &outer.metadata.evidence_scope,
        REPRESENTATIVE_SCOPE,
    )?;
    require_equal("outer passed", outer.passed, true)?;
    require_equal(
        "outer representative eligibility",
        outer.metadata.representative_gate_eligible,
        true,
    )?;
    require_text("outer core outcome", &outer.core.outcome, "passed")?;
    require_text(
        "outer core report path",
        &outer.core.report_path,
        "core/report.json",
    )?;
    if outer.metadata.tool_version.is_empty() || outer.metadata.tool_version.len() > 64 {
        return Err(policy("outer tool_version must be a short nonempty string"));
    }
    require_text(
        "outer target Spine version",
        &outer.metadata.target_spine_version,
        TARGET_SPINE_VERSION,
    )?;
    validate_binding_id(&outer.metadata.binding_id)?;
    for (label, value) in [
        (
            "outer binding SHA",
            &outer.metadata.representative_binding_sha256,
        ),
        ("outer case SHA", &outer.metadata.case_sha256),
        (
            "outer harness SHA",
            &outer.metadata.harness_executable_sha256,
        ),
        (
            "outer editor SHA",
            &outer.metadata.expected_editor_executable_sha256,
        ),
        ("outer Cargo.lock SHA", &outer.metadata.cargo_lock_sha256),
        ("outer core report SHA", &outer.core.report_sha256),
        (
            "outer core content-tree SHA",
            &outer.core.content_tree_sha256,
        ),
        (
            "outer marker value SHA",
            &outer.validation.marker_value_sha256,
        ),
    ] {
        validate_sha(label, value)?;
    }
    validate_revision(&outer.metadata.source_revision)?;
    validate_role_digests(&outer.metadata.package_tree_sha256, "outer package trees")
}

fn verify_outer_binding_links(
    outer: &OuterReportDto,
    binding: &BindingDto,
    binding_bytes: &[u8],
) -> Result<(), RepresentativeVerificationError> {
    require_text(
        "binding id",
        &outer.metadata.binding_id,
        &binding.binding_id,
    )?;
    require_sha_match(
        "representative binding bytes",
        &outer.metadata.representative_binding_sha256,
        &sha256(binding_bytes),
    )?;
    require_text(
        "case SHA",
        &outer.metadata.case_sha256,
        &binding.case_sha256,
    )?;
    require_text(
        "harness SHA",
        &outer.metadata.harness_executable_sha256,
        &binding.harness_executable_sha256,
    )?;
    require_roles_equal(
        "outer and binding package roles",
        &outer.metadata.package_tree_sha256,
        &binding.package_tree_sha256,
    )?;
    require_text(
        "source revision",
        &outer.metadata.source_revision,
        &binding.build.source_revision,
    )?;
    require_text(
        "Cargo.lock SHA",
        &outer.metadata.cargo_lock_sha256,
        &binding.build.cargo_lock_sha256,
    )
}

fn verify_top_level_artifacts(
    outer: &OuterReportDto,
    binding_bytes: &[u8],
    core_content_tree_sha256: &str,
    core_entry_count: usize,
) -> Result<(), RepresentativeVerificationError> {
    let [binding, core] = outer.top_level_artifacts.as_slice() else {
        return Err(policy(
            "top_level_artifacts must contain exactly binding then core",
        ));
    };
    require_text(
        "binding artifact path",
        &binding.path,
        "representative-binding.toml",
    )?;
    require_text("binding artifact kind", &binding.kind, "file")?;
    require_sha_match(
        "binding artifact SHA",
        &binding.sha256,
        &sha256(binding_bytes),
    )?;
    require_equal(
        "binding artifact byte_length",
        binding.byte_length,
        Some(u64::try_from(binding_bytes.len()).unwrap_or(u64::MAX)),
    )?;
    require_equal("binding artifact entry_count", binding.entry_count, None)?;

    require_text("core artifact path", &core.path, "core")?;
    require_text("core artifact kind", &core.kind, "directory")?;
    require_sha_match("core artifact SHA", &core.sha256, core_content_tree_sha256)?;
    require_equal("core artifact byte_length", core.byte_length, None)?;
    require_equal(
        "core artifact entry_count",
        core.entry_count,
        Some(core_entry_count),
    )
}

fn verify_outer_summary(
    outer: &OuterReportDto,
    core: &CoreSummary,
) -> Result<(), RepresentativeVerificationError> {
    require_text("outer core outcome", &outer.core.outcome, "passed")?;
    require_equal("outer passed", outer.passed, core.passed)?;
    require_equal(
        "outer validation process_count",
        outer.validation.process_count,
        core.process_count,
    )?;
    require_equal(
        "outer validation passed_assertion_count",
        outer.validation.passed_assertion_count,
        core.passed_assertion_count,
    )?;
    require_equal(
        "outer validation integrity_failure_count",
        outer.validation.integrity_failure_count,
        Some(core.integrity_failure_count),
    )?;
    for (label, actual) in [
        (
            "outer validation core_schema_validated",
            outer.validation.core_schema_validated,
        ),
        (
            "outer validation clean_build_provenance_validated",
            outer.validation.clean_build_provenance_validated,
        ),
        (
            "outer validation harness_identity_validated",
            outer.validation.harness_identity_validated,
        ),
    ] {
        require_equal(label, actual, true)?;
    }
    require_equal(
        "outer validation editor_launcher_validated",
        outer.validation.editor_launcher_validated,
        true,
    )?;
    require_equal(
        "outer validation package_bindings_validated",
        outer.validation.package_bindings_validated,
        true,
    )?;
    require_equal(
        "outer validation workspace_source_bindings_validated",
        outer.validation.workspace_source_bindings_validated,
        true,
    )?;
    require_equal(
        "outer validation marker_processes_validated",
        outer.validation.marker_processes_validated,
        core.process_count,
    )?;
    require_equal(
        "outer validation marker_evidence_complete",
        outer.validation.marker_evidence_complete,
        true,
    )?;
    require_equal(
        "outer validation assertion_count",
        outer.validation.assertion_count,
        EXPECTED_ASSERTIONS.len(),
    )?;
    let eligible = core.passed
        && core.process_count == EXPECTED_OPERATIONS.len()
        && core.passed_assertion_count == EXPECTED_ASSERTIONS.len()
        && core.integrity_failure_count == 0;
    require_equal(
        "outer representative_gate_eligible",
        outer.metadata.representative_gate_eligible,
        eligible,
    )
}

fn verify_core_report(
    core: &Value,
    observed: &ObservedEvidence,
    outer: &OuterReportDto,
    binding: &BindingDto,
    case: &crate::case::LoadedCase,
) -> Result<CoreSummary, RepresentativeVerificationError> {
    let report_object = object(core, "core report")?;
    let passed = bool_field(report_object, "passed", "core report")?;
    require_equal("published core passed", passed, true)?;
    let expected_keys: &[&str] = &[
        "format_version",
        "metadata",
        "passed",
        "assertions",
        "processes",
        "artifacts",
        "integrity_failures",
        "semantic_differences",
        "roundtrip_losses",
    ];
    require_keys(report_object, expected_keys, "core report")?;
    require_equal(
        "core format_version",
        u64_field(report_object, "format_version", "core report")?,
        u64::from(INNER_FORMAT_VERSION),
    )?;

    let metadata = object(
        field(report_object, "metadata", "core report")?,
        "core metadata",
    )?;
    let launcher_provenance = verify_core_metadata(metadata, outer, binding)?;

    let artifacts = parse_artifacts(field(report_object, "artifacts", "core report")?)?;
    let expected_core_files = verify_core_artifacts(&artifacts, observed)?;
    let processes = array(
        field(report_object, "processes", "core report")?,
        "core processes",
    )?;
    verify_processes(
        processes,
        &artifacts,
        observed,
        outer,
        case,
        &launcher_provenance,
    )?;
    let assertions = array(
        field(report_object, "assertions", "core report")?,
        "core assertions",
    )?;
    let passed_assertion_count = verify_assertions(assertions, &artifacts)?;

    let failures = array(
        field(report_object, "integrity_failures", "core report")?,
        "core integrity failures",
    )?;
    if !failures.is_empty() {
        return Err(policy("passing core report contains integrity failures"));
    }
    let integrity_failure_count = failures.len();
    verify_semantics(
        field(report_object, "semantic_differences", "core report")?,
        field(report_object, "roundtrip_losses", "core report")?,
    )?;

    Ok(CoreSummary {
        passed,
        process_count: processes.len(),
        passed_assertion_count,
        integrity_failure_count,
        expected_core_files,
    })
}

fn verify_semantics(
    differences: &Value,
    losses: &Value,
) -> Result<(), RepresentativeVerificationError> {
    const LOSS_DESCRIPTION: &str = "Spine regenerated the represented skeleton hash during JSON reconstruction; the complete observed string changes are retained in both round-trip comparisons.";
    let differences = array(differences, "core semantic differences")?
        .iter()
        .cloned()
        .map(|value| {
            serde_json::from_value::<SemanticDifferenceDto>(value).map_err(|source| {
                RepresentativeVerificationError::Json {
                    path: "core/report.json",
                    source,
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if differences.len() > 2 {
        return Err(policy(
            "core semantic differences exceed the two fixed round-trip observations",
        ));
    }
    for difference in &differences {
        require_text(
            "semantic difference pointer",
            &difference.pointer,
            "/skeleton/hash",
        )?;
        require_equal(
            "semantic difference approval",
            difference.approved_volatile,
            true,
        )?;
        if difference.before == difference.after {
            return Err(policy(
                "semantic skeleton-hash difference must contain distinct strings",
            ));
        }
    }

    let losses = array(losses, "core round-trip losses")?
        .iter()
        .cloned()
        .map(|value| {
            serde_json::from_value::<RoundTripLossDto>(value).map_err(|source| {
                RepresentativeVerificationError::Json {
                    path: "core/report.json",
                    source,
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if differences.is_empty() {
        if !losses.is_empty() {
            return Err(policy(
                "round-trip loss exists without an observed semantic difference",
            ));
        }
    } else {
        let [loss] = losses.as_slice() else {
            return Err(policy(
                "skeleton-hash differences require exactly one fixed loss record",
            ));
        };
        require_text("round-trip loss pointer", &loss.pointer, "/skeleton/hash")?;
        require_text(
            "round-trip loss description",
            &loss.description,
            LOSS_DESCRIPTION,
        )?;
    }
    Ok(())
}

fn verify_core_metadata(
    metadata: &Map<String, Value>,
    outer: &OuterReportDto,
    binding: &BindingDto,
) -> Result<LauncherProvenanceDto, RepresentativeVerificationError> {
    let expected_keys: &[&str] = &[
        "evidence_scope",
        "representative_gate_eligible",
        "case_id",
        "case_sha256",
        "target_spine_version",
        "expected_executable_sha256",
        "tool_version",
        "provenance",
    ];
    require_keys(metadata, expected_keys, "core metadata")?;
    require_text(
        "inner evidence scope",
        string_field(metadata, "evidence_scope", "core metadata")?,
        GENERIC_SCOPE,
    )?;
    require_equal(
        "inner representative eligibility",
        bool_field(metadata, "representative_gate_eligible", "core metadata")?,
        false,
    )?;
    require_text(
        "inner case SHA",
        string_field(metadata, "case_sha256", "core metadata")?,
        &binding.case_sha256,
    )?;
    require_text(
        "inner target Spine version",
        string_field(metadata, "target_spine_version", "core metadata")?,
        TARGET_SPINE_VERSION,
    )?;
    require_text(
        "inner tool version",
        string_field(metadata, "tool_version", "core metadata")?,
        &outer.metadata.tool_version,
    )?;
    require_text(
        "inner expected editor SHA",
        string_field(metadata, "expected_executable_sha256", "core metadata")?,
        &outer.metadata.expected_editor_executable_sha256,
    )?;
    verify_provenance(
        field(metadata, "provenance", "core metadata")?,
        outer,
        binding,
    )
}

fn verify_provenance(
    value: &Value,
    outer: &OuterReportDto,
    binding: &BindingDto,
) -> Result<LauncherProvenanceDto, RepresentativeVerificationError> {
    let provenance = object(value, "core provenance")?;
    require_keys(
        provenance,
        &["environment", "fixture", "spine_launcher"],
        "core provenance",
    )?;

    let environment = object(
        field(provenance, "environment", "core provenance")?,
        "environment",
    )?;
    require_keys(
        environment,
        &["build_context", "harness_executable", "runtime_host"],
        "provenance environment",
    )?;
    let build_context = field(environment, "build_context", "environment")?;
    let harness = field(environment, "harness_executable", "environment")?;
    verify_build_context(build_context, binding)?;
    verify_harness_identity(harness, binding)?;
    verify_runtime_host(field(environment, "runtime_host", "environment")?)?;

    let fixture = object(field(provenance, "fixture", "core provenance")?, "fixture")?;
    require_keys(
        fixture,
        &[
            "case_sha256",
            "target_spine_version",
            "packages",
            "export_preset",
        ],
        "fixture",
    )?;
    require_text(
        "fixture case SHA",
        string_field(fixture, "case_sha256", "fixture")?,
        &binding.case_sha256,
    )?;
    require_text(
        "fixture target version",
        string_field(fixture, "target_spine_version", "fixture")?,
        TARGET_SPINE_VERSION,
    )?;
    let export_preset = object(
        field(fixture, "export_preset", "fixture")?,
        "fixture export preset",
    )?;
    require_keys(
        export_preset,
        &["preset", "sha256"],
        "fixture export preset",
    )?;
    require_text(
        "fixture export preset",
        string_field(export_preset, "preset", "fixture export preset")?,
        "pretty-nonessential-json",
    )?;
    require_text(
        "fixture export preset SHA",
        string_field(export_preset, "sha256", "fixture export preset")?,
        &sha256(crate::spine_cli::approved_export_preset_bytes()),
    )?;
    let packages: RoleDigestsDto = serde_json::from_value(
        field(fixture, "packages", "fixture")?.clone(),
    )
    .map_err(|source| RepresentativeVerificationError::Json {
        path: "core/report.json",
        source,
    })?;
    require_roles_equal(
        "provenance package roles",
        &packages,
        &binding.package_tree_sha256,
    )?;

    let launcher = object(
        field(provenance, "spine_launcher", "core provenance")?,
        "Spine launcher",
    )?;
    require_keys(
        launcher,
        &[
            "expected_sha256",
            "observed",
            "target_spine_version",
            "observed_processes",
        ],
        "Spine launcher",
    )?;
    require_text(
        "launcher expected SHA",
        string_field(launcher, "expected_sha256", "Spine launcher")?,
        &outer.metadata.expected_editor_executable_sha256,
    )?;
    let observed = object(
        field(launcher, "observed", "Spine launcher")?,
        "observed editor",
    )?;
    require_keys(
        observed,
        &["sha256", "size", "stable_file_identity_sha256"],
        "observed editor",
    )?;
    require_text(
        "launcher observed SHA",
        string_field(observed, "sha256", "observed editor")?,
        &outer.metadata.expected_editor_executable_sha256,
    )?;
    let size = u64_field(observed, "size", "observed editor")?;
    if size == 0 {
        return Err(policy("observed editor size must be nonzero"));
    }
    let stable_file_identity_sha256 =
        string_field(observed, "stable_file_identity_sha256", "observed editor")?.to_owned();
    validate_sha(
        "observed editor stable identity SHA",
        &stable_file_identity_sha256,
    )?;
    require_text(
        "launcher target version",
        string_field(launcher, "target_spine_version", "Spine launcher")?,
        TARGET_SPINE_VERSION,
    )?;
    require_equal(
        "Spine launcher process count",
        usize_field(launcher, "observed_processes", "Spine launcher")?,
        EXPECTED_OPERATIONS.len(),
    )?;
    Ok(LauncherProvenanceDto {
        size,
        stable_file_identity_sha256,
    })
}

fn verify_build_context(
    value: &Value,
    binding: &BindingDto,
) -> Result<(), RepresentativeVerificationError> {
    let build = object(value, "build context")?;
    require_keys(
        build,
        &[
            "relationship",
            "checkout",
            "cargo_lock",
            "rustc",
            "build_host_triple",
            "target_triple",
        ],
        "build context",
    )?;
    require_text(
        "build context relationship",
        string_field(build, "relationship", "build context")?,
        "context_only_not_binary_attestation",
    )?;
    let checkout = object(field(build, "checkout", "build context")?, "build checkout")?;
    require_keys(
        checkout,
        &["head", "dirty", "status_sha256"],
        "build checkout",
    )?;
    require_equal(
        "build checkout dirty",
        bool_field(checkout, "dirty", "build checkout")?,
        false,
    )?;
    require_text(
        "build source revision",
        string_field(checkout, "head", "build checkout")?,
        &binding.build.source_revision,
    )?;
    validate_sha(
        "build checkout status SHA",
        string_field(checkout, "status_sha256", "build checkout")?,
    )?;
    let cargo_lock = object(field(build, "cargo_lock", "build context")?, "Cargo.lock")?;
    require_keys(cargo_lock, &["sha256", "size"], "Cargo.lock")?;
    require_text(
        "build Cargo.lock SHA",
        string_field(cargo_lock, "sha256", "Cargo.lock")?,
        &binding.build.cargo_lock_sha256,
    )?;
    if u64_field(cargo_lock, "size", "Cargo.lock")? == 0 {
        return Err(policy("build Cargo.lock size must be nonzero"));
    }
    let rustc = object(field(build, "rustc", "build context")?, "build rustc")?;
    require_keys(
        rustc,
        &[
            "verbose_version_sha256",
            "release",
            "commit_hash",
            "host_triple",
        ],
        "build rustc",
    )?;
    validate_sha(
        "rustc verbose-version SHA",
        string_field(rustc, "verbose_version_sha256", "build rustc")?,
    )?;
    let release = string_field(rustc, "release", "build rustc")?;
    let rustc_host = string_field(rustc, "host_triple", "build rustc")?;
    let build_host = string_field(build, "build_host_triple", "build context")?;
    let target = string_field(build, "target_triple", "build context")?;
    for (label, token) in [
        ("rustc release", release),
        ("rustc host triple", rustc_host),
        ("build host triple", build_host),
        ("target triple", target),
    ] {
        validate_safe_token(label, token)?;
    }
    require_text("build and rustc host triples", build_host, rustc_host)?;
    match field(rustc, "commit_hash", "build rustc")? {
        Value::Null => {}
        Value::String(value) => validate_revision(value)?,
        _ => return Err(policy("rustc commit_hash must be null or lowercase hex")),
    }
    Ok(())
}

fn verify_harness_identity(
    value: &Value,
    binding: &BindingDto,
) -> Result<(), RepresentativeVerificationError> {
    let harness = object(value, "harness executable")?;
    require_keys(
        harness,
        &["sha256", "size", "stable_file_identity_sha256"],
        "harness executable",
    )?;
    require_text(
        "provenance harness SHA",
        string_field(harness, "sha256", "harness executable")?,
        &binding.harness_executable_sha256,
    )?;
    if u64_field(harness, "size", "harness executable")? == 0 {
        return Err(policy("harness executable size must be nonzero"));
    }
    validate_sha(
        "harness stable file identity SHA",
        string_field(harness, "stable_file_identity_sha256", "harness executable")?,
    )
}

fn verify_runtime_host(value: &Value) -> Result<(), RepresentativeVerificationError> {
    let host = object(value, "runtime host")?;
    require_keys(
        host,
        &["operating_system", "process_architecture", "kernel_family"],
        "runtime host",
    )?;
    for key in ["operating_system", "process_architecture", "kernel_family"] {
        validate_safe_token(
            "runtime host token",
            string_field(host, key, "runtime host")?,
        )?;
    }
    Ok(())
}

fn parse_artifacts(value: &Value) -> Result<Vec<ArtifactDto>, RepresentativeVerificationError> {
    let values = array(value, "core artifacts")?;
    values
        .iter()
        .cloned()
        .map(|value| {
            serde_json::from_value(value).map_err(|source| RepresentativeVerificationError::Json {
                path: "core/report.json",
                source,
            })
        })
        .collect()
}

fn verify_core_artifacts(
    artifacts: &[ArtifactDto],
    observed: &ObservedEvidence,
) -> Result<BTreeSet<String>, RepresentativeVerificationError> {
    let mut expected_files = BTreeSet::from(["core/report.json".to_owned()]);
    let mut unique = BTreeSet::new();
    for artifact in artifacts {
        validate_artifact(artifact)?;
        if !unique.insert((
            artifact.role.clone(),
            artifact.path.clone(),
            artifact.sha256.clone(),
        )) {
            return Err(policy("core report contains a duplicate artifact identity"));
        }
        let physical = format!("core/{}", artifact.path);
        let bytes = file_bytes(observed, &physical)?;
        require_sha_match("core artifact bytes", &artifact.sha256, &sha256(bytes))?;
        expected_files.insert(physical);
    }
    let expected = fixed_success_artifacts();
    let actual = artifacts
        .iter()
        .map(|artifact| (artifact.path.clone(), artifact.role.clone()))
        .collect::<BTreeSet<_>>();
    if actual != expected || artifacts.len() != expected.len() {
        return Err(policy(
            "passing core artifact catalog is not the exact fixed graph",
        ));
    }
    Ok(expected_files)
}

fn verify_processes(
    processes: &[Value],
    artifacts: &[ArtifactDto],
    observed: &ObservedEvidence,
    outer: &OuterReportDto,
    case: &crate::case::LoadedCase,
    launcher_provenance: &LauncherProvenanceDto,
) -> Result<(), RepresentativeVerificationError> {
    if processes.len() != EXPECTED_OPERATIONS.len() {
        return Err(policy(
            "core process count does not match the closed operation sequence",
        ));
    }
    let marker = sha256(outer.metadata.representative_binding_sha256.as_bytes());
    require_text(
        "outer marker digest",
        &outer.validation.marker_value_sha256,
        &marker,
    )?;
    let artifact_set = artifacts.iter().cloned().collect::<BTreeSet<_>>();
    let mut prior_environment: Option<Vec<EnvironmentVariableDto>> = None;
    let mut prior_program: Option<PathBuf> = None;
    let mut prior_working_directory: Option<PathBuf> = None;
    let mut prior_executable: Option<ExecutableIdentityDto> = None;
    let mut prior_working_identity: Option<WorkingDirectoryIdentityDto> = None;
    let mut prior_lock_identity: Option<(PathBuf, u64, u64, String)> = None;
    for (index, process_value) in processes.iter().enumerate() {
        close_recorded_process_shape(index, process_value)?;
        let process: RecordedProcessDto =
            serde_json::from_value(process_value.clone()).map_err(|source| {
                RepresentativeVerificationError::Json {
                    path: "core/report.json",
                    source,
                }
            })?;
        verify_expectation_dto(index, process.expectation)?;
        let evidence = &process.evidence;
        require_text(
            "process operation",
            &evidence.operation,
            EXPECTED_OPERATIONS[index],
        )?;
        validate_process_request(index, evidence, case)?;
        validate_executable_identity(
            &evidence.executable_identity,
            &outer.metadata.expected_editor_executable_sha256,
        )?;
        verify_launcher_process_binding(&evidence.executable_identity, launcher_provenance)?;
        validate_working_directory_identity(&evidence.working_directory_identity)?;
        require_equal(
            "requested and canonical executable paths",
            evidence.requested_program.as_path(),
            evidence.executable_identity.canonical_path.as_path(),
        )?;
        require_equal(
            "requested and canonical working directories",
            evidence.requested_working_directory.as_path(),
            evidence.working_directory_identity.canonical_path.as_path(),
        )?;
        if prior_program
            .as_ref()
            .is_some_and(|prior| prior != &evidence.requested_program)
            || prior_working_directory
                .as_ref()
                .is_some_and(|prior| prior != &evidence.requested_working_directory)
            || prior_executable
                .as_ref()
                .is_some_and(|prior| prior != &evidence.executable_identity)
            || prior_working_identity
                .as_ref()
                .is_some_and(|prior| prior != &evidence.working_directory_identity)
        {
            return Err(policy(
                "process launch identities differ within one representative run",
            ));
        }
        prior_program.get_or_insert_with(|| evidence.requested_program.clone());
        prior_working_directory.get_or_insert_with(|| evidence.requested_working_directory.clone());
        prior_executable.get_or_insert_with(|| evidence.executable_identity.clone());
        prior_working_identity.get_or_insert_with(|| evidence.working_directory_identity.clone());

        let lock = evidence
            .lock_evidence
            .as_ref()
            .ok_or_else(|| policy("process lacks required editor-lock evidence"))?;
        validate_lock_evidence(lock)?;
        let lock_identity = (
            lock.canonical_path.clone(),
            lock.device,
            lock.inode,
            lock.filesystem_kind.clone(),
        );
        if prior_lock_identity
            .as_ref()
            .is_some_and(|prior| prior != &lock_identity)
        {
            return Err(policy(
                "process editor-lock identities differ within one representative run",
            ));
        }
        prior_lock_identity.get_or_insert(lock_identity);
        verify_environment(&evidence.environment, &marker)?;
        if prior_environment
            .as_ref()
            .is_some_and(|prior| prior != &evidence.environment)
        {
            return Err(policy(
                "process environments differ within one representative run",
            ));
        }
        prior_environment.get_or_insert_with(|| evidence.environment.clone());

        let stdout = &process.stdout_artifact;
        let stderr = &process.stderr_artifact;
        require_text(
            "stdout artifact path",
            &stdout.path,
            &format!("processes/{index:04}.stdout.txt"),
        )?;
        require_text(
            "stderr artifact path",
            &stderr.path,
            &format!("processes/{index:04}.stderr.txt"),
        )?;
        if !artifact_set.contains(stdout) || !artifact_set.contains(stderr) {
            return Err(policy(
                "process transcript citation is absent from artifact catalog",
            ));
        }
        verify_process_stream(
            "stdout",
            &evidence.stdout,
            file_bytes(observed, &format!("core/{}", stdout.path))?,
            &stdout.sha256,
            &evidence.assessment.stdout_retained_prefix_sha256,
        )?;
        verify_process_stream(
            "stderr",
            &evidence.stderr,
            file_bytes(observed, &format!("core/{}", stderr.path))?,
            &stderr.sha256,
            &evidence.assessment.stderr_retained_prefix_sha256,
        )?;
        verify_process_outcome(index, evidence, case)?;
    }
    Ok(())
}

fn close_recorded_process_shape(
    index: usize,
    value: &Value,
) -> Result<(), RepresentativeVerificationError> {
    let process = object(value, "recorded process")?;
    require_keys(
        process,
        &[
            "expectation",
            "evidence",
            "stdout_artifact",
            "stderr_artifact",
        ],
        "recorded process",
    )?;
    verify_expectation(
        index,
        object(
            field(process, "expectation", "recorded process")?,
            "process expectation",
        )?,
    )?;
    for key in ["stdout_artifact", "stderr_artifact"] {
        require_keys(
            object(field(process, key, "recorded process")?, "process artifact")?,
            &["role", "path", "sha256"],
            "process artifact",
        )?;
    }
    let evidence = object(
        field(process, "evidence", "recorded process")?,
        "process evidence",
    )?;
    let mut evidence_keys = vec![
        "operation",
        "requested_program",
        "args",
        "requested_working_directory",
        "environment",
        "timeout_seconds",
        "timeout_subsec_nanos",
        "cleanup_timeout_seconds",
        "cleanup_timeout_subsec_nanos",
        "max_retained_bytes_per_stream",
        "executable_identity",
        "working_directory_identity",
        "lock_evidence",
        "exit_code",
        "terminating_signal",
        "sent_signal",
        "termination_reason",
        "elapsed_seconds",
        "elapsed_subsec_nanos",
        "cleanup_status",
        "adapter_failure",
        "stdout",
        "stderr",
        "required_outputs",
        "observed_outputs",
        "output_discovery_state",
        "transcript_profile",
        "assessment",
    ];
    if index == 19 {
        evidence_keys.push("new_animation_collision");
    }
    require_keys(evidence, &evidence_keys, "process evidence")?;
    require_keys(
        object(
            field(evidence, "executable_identity", "process evidence")?,
            "process executable identity",
        )?,
        &[
            "canonical_path",
            "sha256",
            "size",
            "device",
            "inode",
            "mode",
            "owner",
            "modified_seconds",
            "modified_nanoseconds",
            "changed_seconds",
            "changed_nanoseconds",
            "local_filesystem_verified",
        ],
        "process executable identity",
    )?;
    require_keys(
        object(
            field(evidence, "working_directory_identity", "process evidence")?,
            "process working-directory identity",
        )?,
        &[
            "canonical_path",
            "device",
            "inode",
            "mode",
            "owner",
            "local_filesystem_verified",
        ],
        "process working-directory identity",
    )?;
    require_keys(
        object(
            field(evidence, "lock_evidence", "process evidence")?,
            "process lock evidence",
        )?,
        &[
            "canonical_path",
            "wait_seconds",
            "wait_subsec_nanos",
            "acquired",
            "local_filesystem_verified",
            "device",
            "inode",
            "filesystem_kind",
        ],
        "process lock evidence",
    )?;
    for key in ["stdout", "stderr"] {
        require_keys(
            object(field(evidence, key, "process evidence")?, "process stream")?,
            &[
                "total_observed_bytes",
                "retained_bytes",
                "retained_prefix_sha256",
                "bytes_seen_sha256",
                "full_stream_sha256",
                "retained_prefix_truncated",
                "complete",
            ],
            "process stream",
        )?;
    }
    for entry in array(
        field(evidence, "environment", "process evidence")?,
        "process environment",
    )? {
        require_keys(
            object(entry, "environment variable")?,
            &["name", "value_sha256"],
            "environment variable",
        )?;
    }
    let assessment = object(
        field(evidence, "assessment", "process evidence")?,
        "process assessment",
    )?;
    require_keys(
        assessment,
        &[
            "passed",
            "stdout_retained_prefix_sha256",
            "stderr_retained_prefix_sha256",
            "failures",
        ],
        "process assessment",
    )?;
    for failure in array(
        field(assessment, "failures", "process assessment")?,
        "process failures",
    )? {
        require_keys(
            object(failure, "process failure")?,
            &["code", "detail"],
            "process failure",
        )?;
    }
    if index == 19 {
        require_keys(
            object(
                field(evidence, "new_animation_collision", "process evidence")?,
                "new-animation collision evidence",
            )?,
            &["requested_animation", "renamed_animation"],
            "new-animation collision evidence",
        )?;
    }
    Ok(())
}

fn verify_expectation_dto(
    index: usize,
    expectation: ProcessExpectationDto,
) -> Result<(), RepresentativeVerificationError> {
    let expected = match index {
        19 => ProcessExpectationDto::NegativeControl(
            ExpectedProcessFailureDto::NewAnimationCollisionDiagnostic,
        ),
        21 => ProcessExpectationDto::NegativeControl(
            ExpectedProcessFailureDto::MissingImagesPathDiagnostic,
        ),
        _ => ProcessExpectationDto::RequiredSuccess,
    };
    require_equal("process expectation", expectation, expected)
}

fn verify_expectation(
    index: usize,
    expectation: &Map<String, Value>,
) -> Result<(), RepresentativeVerificationError> {
    let kind = string_field(expectation, "kind", "process expectation")?;
    match index {
        19 => {
            require_keys(
                expectation,
                &["kind", "expected_failure"],
                "process expectation",
            )?;
            require_text("process expectation", kind, "negative_control")?;
            require_text(
                "negative-control kind",
                string_field(expectation, "expected_failure", "process expectation")?,
                "new_animation_collision_diagnostic",
            )
        }
        21 => {
            require_keys(
                expectation,
                &["kind", "expected_failure"],
                "process expectation",
            )?;
            require_text("process expectation", kind, "negative_control")?;
            require_text(
                "negative-control kind",
                string_field(expectation, "expected_failure", "process expectation")?,
                "missing_images_path_diagnostic",
            )
        }
        _ => {
            require_keys(expectation, &["kind"], "process expectation")?;
            require_text("process expectation", kind, "required_success")
        }
    }
}

fn validate_process_request(
    index: usize,
    evidence: &ProcessEvidenceDto,
    case: &crate::case::LoadedCase,
) -> Result<(), RepresentativeVerificationError> {
    validate_normalized_absolute("requested process program", &evidence.requested_program)?;
    validate_normalized_absolute(
        "requested process working directory",
        &evidence.requested_working_directory,
    )?;
    require_equal(
        "process argument vector",
        &evidence.args,
        &expected_process_args(index, &evidence.requested_working_directory, case)?,
    )?;
    let expected_timeout = if index <= 1 {
        120
    } else if index <= 4 {
        300
    } else {
        1_800
    };
    require_equal(
        "process timeout seconds",
        evidence.timeout_seconds,
        expected_timeout,
    )?;
    require_equal(
        "process timeout nanoseconds",
        evidence.timeout_subsec_nanos,
        0,
    )?;
    require_equal(
        "process cleanup timeout seconds",
        evidence.cleanup_timeout_seconds,
        30,
    )?;
    require_equal(
        "process cleanup timeout nanoseconds",
        evidence.cleanup_timeout_subsec_nanos,
        0,
    )?;
    require_equal(
        "process retained-stream limit",
        evidence.max_retained_bytes_per_stream,
        4 * 1024 * 1024,
    )?;
    require_equal("process exit code", evidence.exit_code, Some(0))?;
    require_equal(
        "process terminating signal",
        evidence.terminating_signal,
        None,
    )?;
    require_equal("process sent signal", evidence.sent_signal, None)?;
    require_text(
        "process termination reason",
        &evidence.termination_reason,
        "natural_exit",
    )?;
    require_text(
        "process cleanup status",
        &evidence.cleanup_status,
        "complete",
    )?;
    require_equal(
        "process adapter failure",
        evidence.adapter_failure.as_ref(),
        None,
    )?;
    if evidence.elapsed_subsec_nanos >= 1_000_000_000
        || evidence.elapsed_seconds > evidence.timeout_seconds
        || (evidence.elapsed_seconds == evidence.timeout_seconds
            && evidence.elapsed_subsec_nanos != 0)
    {
        return Err(policy(
            "process elapsed duration exceeds its fixed request deadline",
        ));
    }
    require_text(
        "process output discovery state",
        &evidence.output_discovery_state,
        "complete",
    )?;
    require_text(
        "process transcript profile",
        &evidence.transcript_profile,
        expected_transcript_profile(index),
    )?;
    let required_outputs = expected_required_outputs(index);
    require_equal(
        "process required outputs",
        &evidence.required_outputs,
        &required_outputs,
    )?;
    validate_unique_sorted_tokens("process required outputs", &evidence.required_outputs)?;
    validate_unique_sorted_tokens("process observed outputs", &evidence.observed_outputs)
}

fn validate_executable_identity(
    executable: &ExecutableIdentityDto,
    expected_sha256: &str,
) -> Result<(), RepresentativeVerificationError> {
    validate_normalized_absolute("canonical process executable", &executable.canonical_path)?;
    require_sha_match("process editor SHA", &executable.sha256, expected_sha256)?;
    if executable.size == 0
        || executable.mode & 0o170_000 != 0o100_000
        || executable.mode & 0o111 == 0
        || !executable.local_filesystem_verified
        || !(0..1_000_000_000).contains(&executable.modified_nanoseconds)
        || !(0..1_000_000_000).contains(&executable.changed_nanoseconds)
    {
        return Err(policy(
            "process executable identity is not a complete verified executable",
        ));
    }
    Ok(())
}

fn verify_launcher_process_binding(
    executable: &ExecutableIdentityDto,
    launcher: &LauncherProvenanceDto,
) -> Result<(), RepresentativeVerificationError> {
    require_equal(
        "launcher provenance and process executable sizes",
        launcher.size,
        executable.size,
    )?;
    require_text(
        "launcher provenance and process stable file identities",
        &launcher.stable_file_identity_sha256,
        &stable_executable_identity_sha256(executable),
    )
}

fn stable_executable_identity_sha256(executable: &ExecutableIdentityDto) -> String {
    let mut framed = b"spinal-phase0a-executable-file-identity-v1\0".to_vec();
    framed.extend_from_slice(executable.sha256.as_bytes());
    framed.push(0);
    for value in [
        executable.size,
        executable.device,
        executable.inode,
        u64::from(executable.mode),
        u64::from(executable.owner),
        executable.modified_seconds as u64,
        executable.modified_nanoseconds as u64,
        executable.changed_seconds as u64,
        executable.changed_nanoseconds as u64,
    ] {
        framed.extend_from_slice(&value.to_le_bytes());
    }
    framed.push(u8::from(executable.local_filesystem_verified));
    sha256(&framed)
}

fn validate_working_directory_identity(
    working: &WorkingDirectoryIdentityDto,
) -> Result<(), RepresentativeVerificationError> {
    validate_normalized_absolute(
        "canonical process working directory",
        &working.canonical_path,
    )?;
    if working.mode & 0o170_000 != 0o040_000 || !working.local_filesystem_verified {
        return Err(policy(
            "process working-directory identity is not a verified directory",
        ));
    }
    Ok(())
}

fn validate_lock_evidence(lock: &LockEvidenceDto) -> Result<(), RepresentativeVerificationError> {
    validate_normalized_absolute("canonical editor-lock path", &lock.canonical_path)?;
    if !lock.acquired
        || !lock.local_filesystem_verified
        || lock.wait_subsec_nanos >= 1_000_000_000
        || lock.wait_seconds > 300
        || (lock.wait_seconds == 300 && lock.wait_subsec_nanos != 0)
        || lock.filesystem_kind.is_empty()
        || lock.filesystem_kind.len() > 64
        || !lock
            .filesystem_kind
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(policy(
            "process editor-lock evidence is not a complete acquired local lock",
        ));
    }
    Ok(())
}

fn verify_environment(
    environment: &[EnvironmentVariableDto],
    expected_marker: &str,
) -> Result<(), RepresentativeVerificationError> {
    let required = BTreeSet::from(["HOME", "LANG", "LC_ALL", "PATH", MARKER_NAME]);
    let allowed = BTreeSet::from(["HOME", "LANG", "LC_ALL", "PATH", "TMPDIR", MARKER_NAME]);
    if !environment
        .windows(2)
        .all(|pair| pair[0].name < pair[1].name)
    {
        return Err(policy(
            "process environment is not in producer-canonical name order",
        ));
    }
    let mut names = BTreeSet::new();
    for variable in environment {
        if !allowed.contains(variable.name.as_str()) || !names.insert(variable.name.as_str()) {
            return Err(policy(
                "process environment contains an unknown or duplicate name",
            ));
        }
        validate_sha("environment value SHA", &variable.value_sha256)?;
        if variable.name == MARKER_NAME {
            require_text(
                "representative binding marker",
                &variable.value_sha256,
                expected_marker,
            )?;
        }
    }
    if !required.is_subset(&names) || names.len() != environment.len() {
        return Err(policy(
            "process environment lacks its exact required allowlisted names",
        ));
    }
    Ok(())
}

fn verify_process_stream(
    label: &str,
    stream: &ProcessStreamDto,
    bytes: &[u8],
    artifact_sha256: &str,
    assessment_sha256: &str,
) -> Result<(), RepresentativeVerificationError> {
    let expected_sha256 = sha256(bytes);
    let expected_length = bytes.len();
    require_sha_match(label, artifact_sha256, &expected_sha256)?;
    require_equal(
        &format!("{label} retained byte count"),
        stream.retained_bytes,
        expected_length,
    )?;
    require_equal(
        &format!("{label} total byte count"),
        stream.total_observed_bytes,
        u64::try_from(expected_length).unwrap_or(u64::MAX),
    )?;
    for (digest_label, digest) in [
        ("retained prefix", stream.retained_prefix_sha256.as_str()),
        ("bytes seen", stream.bytes_seen_sha256.as_str()),
        ("assessment retained prefix", assessment_sha256),
    ] {
        require_sha_match(
            &format!("{label} {digest_label} SHA"),
            digest,
            &expected_sha256,
        )?;
    }
    require_equal(
        &format!("{label} full-stream SHA"),
        stream.full_stream_sha256.as_deref(),
        Some(expected_sha256.as_str()),
    )?;
    require_equal(
        &format!("{label} retained truncation"),
        stream.retained_prefix_truncated,
        false,
    )?;
    require_equal(&format!("{label} completion"), stream.complete, true)
}

fn verify_process_outcome(
    index: usize,
    evidence: &ProcessEvidenceDto,
    case: &crate::case::LoadedCase,
) -> Result<(), RepresentativeVerificationError> {
    match index {
        19 => {
            require_equal(
                "collision-control assessment",
                evidence.assessment.passed,
                false,
            )?;
            require_equal(
                "collision-control observed outputs",
                &evidence.observed_outputs,
                &evidence.required_outputs,
            )?;
            let [failure] = evidence.assessment.failures.as_slice() else {
                return Err(policy(
                    "collision control must have exactly one typed assessment failure",
                ));
            };
            require_text(
                "collision-control failure code",
                &failure.code,
                "blocking_diagnostic",
            )?;
            require_text(
                "collision-control failure detail",
                &failure.detail,
                "stdout contained the exact expected new-animation collision diagnostic",
            )?;
            let collision = evidence.new_animation_collision.as_ref().ok_or_else(|| {
                policy("collision control lacks typed new-animation collision evidence")
            })?;
            require_text(
                "collision requested animation",
                &collision.requested_animation,
                &case.manifest().animations.new,
            )?;
            if collision.renamed_animation == collision.requested_animation
                || !safe_argument_name(&collision.renamed_animation)
            {
                return Err(policy(
                    "collision control renamed animation is not safe and distinct",
                ));
            }
        }
        21 => {
            require_equal(
                "missing-images assessment",
                evidence.assessment.passed,
                false,
            )?;
            let failures = evidence.assessment.failures.as_slice();
            if !matches!(failures.len(), 1 | 2) {
                return Err(policy(
                    "missing-images control has the wrong typed assessment failures",
                ));
            }
            require_text(
                "missing-images failure code",
                &failures[0].code,
                "blocking_diagnostic",
            )?;
            require_text(
                "missing-images failure detail",
                &failures[0].detail,
                "stdout contained the exact expected `Images path not found: ./images/` diagnostic",
            )?;
            if let Some(failure) = failures.get(1) {
                require_text(
                    "missing-images output failure code",
                    &failure.code,
                    "missing_output",
                )?;
                require_text(
                    "missing-images output failure detail",
                    &failure.detail,
                    "required output `export-json` was not produced",
                )?;
                require_equal(
                    "missing-images observed outputs",
                    evidence.observed_outputs.as_slice(),
                    &[],
                )?;
            } else {
                require_equal(
                    "missing-images observed outputs",
                    &evidence.observed_outputs,
                    &evidence.required_outputs,
                )?;
            }
            require_equal(
                "missing-images collision evidence",
                evidence.new_animation_collision.as_ref().map(|_| ()),
                None,
            )?;
        }
        _ => {
            require_equal(
                "required-success assessment",
                evidence.assessment.passed,
                true,
            )?;
            if !evidence.assessment.failures.is_empty() {
                return Err(policy(
                    "required-success process contains assessment failures",
                ));
            }
            require_equal(
                "required-success observed outputs",
                &evidence.observed_outputs,
                &evidence.required_outputs,
            )?;
            require_equal(
                "required-success collision evidence",
                evidence.new_animation_collision.as_ref().map(|_| ()),
                None,
            )?;
        }
    }
    Ok(())
}

fn expected_transcript_profile(index: usize) -> &'static str {
    match index {
        0 => "version",
        1 => "advanced_help",
        2..=4 => "project_info",
        8 | 11 => "project_import",
        13 | 15 | 17 => "animation_import",
        19 => "new_animation_collision_control",
        21 => "missing_images_path_control",
        _ => "json_export",
    }
}

fn expected_required_outputs(index: usize) -> Vec<String> {
    let value = match index {
        0..=4 => None,
        8 | 11 => Some("reconstructed-project"),
        13 | 15 | 17 | 19 => Some("destination-project"),
        _ => Some("export-json"),
    };
    value.into_iter().map(str::to_owned).collect()
}

fn validate_unique_sorted_tokens(
    label: &str,
    values: &[String],
) -> Result<(), RepresentativeVerificationError> {
    if values.windows(2).all(|pair| pair[0] < pair[1])
        && values.iter().all(|value| {
            !value.is_empty()
                && value.len() <= 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
        })
    {
        Ok(())
    } else {
        Err(policy(format!("{label} are not unique sorted safe tokens")))
    }
}

fn validate_normalized_absolute(
    label: &str,
    path: &Path,
) -> Result<(), RepresentativeVerificationError> {
    if path.is_absolute()
        && path
            .to_str()
            .is_some_and(|text| !text.contains('\\') && !text.contains('\0'))
        && path.components().all(|component| {
            matches!(
                component,
                Component::RootDir | Component::Prefix(_) | Component::Normal(_)
            )
        })
    {
        Ok(())
    } else {
        Err(policy(format!("{label} is not absolute and normalized")))
    }
}

fn safe_argument_name(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && !value.starts_with('-')
        && !value.chars().any(char::is_control)
}

fn expected_process_args(
    index: usize,
    root: &Path,
    case: &crate::case::LoadedCase,
) -> Result<Vec<String>, RepresentativeVerificationError> {
    let manifest = case.manifest();
    let current = root
        .join("packages/current")
        .join(&manifest.packages.current.project);
    let replacement = root
        .join("packages/replacement-submission")
        .join(&manifest.packages.replacement_submission.project);
    let new_submission = root
        .join("packages/new-submission")
        .join(&manifest.packages.new_submission.project);
    let reconstructed_a = root.join("packages/current/phase0a-round-trip-a.spine");
    let reconstructed_b = root.join("packages/current/phase0a-round-trip-b.spine");
    let existing_candidate = root
        .join("packages/existing-candidate")
        .join(&manifest.packages.current.project);
    let new_candidate = root
        .join("packages/new-candidate")
        .join(&manifest.packages.current.project);
    let collision_candidate = root
        .join("packages/new-collision-control")
        .join(&manifest.packages.new_submission.project);
    let missing_images = root
        .join("packages/missing-images-control")
        .join(&manifest.packages.current.project);
    let preset = root.join("policy/pretty-nonessential.export.json");
    let output_json =
        |directory: &str, skeleton: &str| root.join(directory).join(format!("{skeleton}.json"));
    let export =
        |project: &Path, directory: &str| -> Result<Vec<String>, RepresentativeVerificationError> {
            Ok(vec![
                "--input".to_owned(),
                path_argument(project)?,
                "--output".to_owned(),
                path_argument(&root.join(directory))?,
                "--export".to_owned(),
                path_argument(&preset)?,
            ])
        };
    let reconstruct =
        |input: &Path, output: &Path| -> Result<Vec<String>, RepresentativeVerificationError> {
            Ok(vec![
                "--input".to_owned(),
                path_argument(input)?,
                "--output".to_owned(),
                path_argument(output)?,
                "--to".to_owned(),
                manifest.skeletons.current.clone(),
                "--import".to_owned(),
            ])
        };
    let import = |source: &Path,
                  destination: &Path,
                  source_skeleton: &str,
                  destination_skeleton: &str,
                  animation: &str,
                  replace: bool|
     -> Result<Vec<String>, RepresentativeVerificationError> {
        let mut values = vec![
            "--input".to_owned(),
            path_argument(source)?,
            "--output".to_owned(),
            path_argument(destination)?,
            "--from".to_owned(),
            source_skeleton.to_owned(),
            "--to".to_owned(),
            destination_skeleton.to_owned(),
            "--animation".to_owned(),
            animation.to_owned(),
        ];
        if replace {
            values.push("--replace".to_owned());
        }
        values.push("--import".to_owned());
        Ok(values)
    };
    let operation = match index {
        0 => vec!["--version".to_owned()],
        1 => vec!["--advanced".to_owned()],
        2 => vec!["--input".to_owned(), path_argument(&current)?],
        3 => vec!["--input".to_owned(), path_argument(&replacement)?],
        4 => vec!["--input".to_owned(), path_argument(&new_submission)?],
        5 => export(&current, "outputs/round-trip/a/source")?,
        6 => export(&replacement, "outputs/submissions/replacement")?,
        7 => export(&new_submission, "outputs/submissions/new")?,
        8 => reconstruct(
            &output_json("outputs/round-trip/a/source", &manifest.skeletons.current),
            &reconstructed_a,
        )?,
        9 => export(&reconstructed_a, "outputs/round-trip/a/reconstructed-json")?,
        10 => export(&current, "outputs/round-trip/b/source")?,
        11 => reconstruct(
            &output_json("outputs/round-trip/b/source", &manifest.skeletons.current),
            &reconstructed_b,
        )?,
        12 => export(&reconstructed_b, "outputs/round-trip/b/reconstructed-json")?,
        13 | 15 => import(
            &replacement,
            &existing_candidate,
            &manifest.skeletons.replacement_submission,
            &manifest.skeletons.current,
            &manifest.animations.replacement,
            true,
        )?,
        14 => export(&existing_candidate, "outputs/candidates/existing/first")?,
        16 => export(&existing_candidate, "outputs/candidates/existing/repeat")?,
        17 => import(
            &new_submission,
            &new_candidate,
            &manifest.skeletons.new_submission,
            &manifest.skeletons.current,
            &manifest.animations.new,
            false,
        )?,
        18 => export(&new_candidate, "outputs/candidates/new/first")?,
        19 => import(
            &new_submission,
            &collision_candidate,
            &manifest.skeletons.new_submission,
            &manifest.skeletons.new_submission,
            &manifest.animations.new,
            false,
        )?,
        20 => export(
            &collision_candidate,
            "outputs/candidates/new/collision-control",
        )?,
        21 => export(&missing_images, "outputs/negative-control")?,
        _ => {
            return Err(policy(
                "process index is outside the closed operation sequence",
            ));
        }
    };
    let mut args = vec![
        "--update".to_owned(),
        TARGET_SPINE_VERSION.to_owned(),
        "--hide-license".to_owned(),
        "--disable-audio".to_owned(),
    ];
    args.extend(operation);
    Ok(args)
}

fn path_argument(path: &Path) -> Result<String, RepresentativeVerificationError> {
    validate_normalized_absolute("process argument path", path)?;
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| policy("process argument path is not UTF-8"))
}

#[cfg(test)]
fn verify_marker(value: &Value, expected: &str) -> Result<(), RepresentativeVerificationError> {
    let variables = array(value, "process environment")?;
    let mut names = BTreeSet::new();
    let mut marker_count = 0_usize;
    for variable in variables {
        let variable = object(variable, "environment variable")?;
        require_keys(variable, &["name", "value_sha256"], "environment variable")?;
        let name = string_field(variable, "name", "environment variable")?;
        if !matches!(
            name,
            "HOME" | "LANG" | "LC_ALL" | "PATH" | "TMPDIR" | MARKER_NAME
        ) {
            return Err(policy(
                "process environment contains a non-allowlisted name",
            ));
        }
        if !names.insert(name.to_owned()) {
            return Err(policy("process environment contains a duplicate name"));
        }
        let value_sha = string_field(variable, "value_sha256", "environment variable")?;
        validate_sha("environment value SHA", value_sha)?;
        if name == MARKER_NAME {
            marker_count = marker_count.saturating_add(1);
            require_text("representative binding marker", value_sha, expected)?;
        }
    }
    if marker_count != 1 {
        return Err(policy(
            "every process must contain exactly one representative binding marker",
        ));
    }
    Ok(())
}

fn verify_assertions(
    assertions: &[Value],
    artifacts: &[ArtifactDto],
) -> Result<usize, RepresentativeVerificationError> {
    if assertions.len() != EXPECTED_ASSERTIONS.len() {
        return Err(policy(
            "core report must contain exactly the 25 closed assertions",
        ));
    }
    let artifact_set = artifacts.iter().cloned().collect::<BTreeSet<_>>();
    let mut passed_count = 0_usize;
    for (index, assertion_value) in assertions.iter().enumerate() {
        let assertion = object(assertion_value, "assertion")?;
        require_keys(
            assertion,
            &["id", "status", "summary", "evidence"],
            "assertion",
        )?;
        require_text(
            "assertion id",
            string_field(assertion, "id", "assertion")?,
            EXPECTED_ASSERTIONS[index],
        )?;
        require_text(
            "assertion status",
            string_field(assertion, "status", "assertion")?,
            "passed",
        )?;
        passed_count = passed_count.saturating_add(1);
        let citations = array(
            field(assertion, "evidence", "assertion")?,
            "assertion evidence",
        )?;
        if citations.is_empty() {
            return Err(policy("assertion has no artifact citation"));
        }
        let citations = citations
            .iter()
            .map(parse_artifact_value)
            .collect::<Result<BTreeSet<_>, _>>()?;
        if citations
            .iter()
            .any(|citation| !artifact_set.contains(citation))
        {
            return Err(policy(
                "assertion cites an artifact absent from the report catalog",
            ));
        }
        let expected = fixed_assertion_citations(index, artifacts)?;
        if citations != expected {
            return Err(policy(
                "assertion does not cite its exact fixed artifact graph",
            ));
        }
    }
    Ok(passed_count)
}

fn fixed_assertion_citations(
    assertion_index: usize,
    artifacts: &[ArtifactDto],
) -> Result<BTreeSet<ArtifactDto>, RepresentativeVerificationError> {
    let (base, processes) = ASSERTION_CITATIONS[assertion_index];
    let mut paths = BTreeSet::from([base.to_owned()]);
    for index in processes {
        paths.insert(format!("processes/{index:04}.stdout.txt"));
        paths.insert(format!("processes/{index:04}.stderr.txt"));
    }
    paths
        .into_iter()
        .map(|path| {
            artifacts
                .iter()
                .find(|artifact| artifact.path == path)
                .cloned()
                .ok_or_else(|| policy("fixed assertion artifact is missing"))
        })
        .collect()
}

fn verify_package_bindings(
    bytes: &[u8],
    binding: &BindingDto,
) -> Result<(), RepresentativeVerificationError> {
    let artifact: Value = parse_json("core/package-inventories.json", bytes)?;
    let root = object(&artifact, "package inventories artifact")?;
    require_keys(
        root,
        &[
            "format_version",
            "evidence_scope",
            "project_info",
            "workspace_boundary",
            "matching_non_project_sha256",
            "matching_non_project_entries",
        ],
        "package inventories artifact",
    )?;
    require_equal(
        "package inventories format_version",
        u64_field(root, "format_version", "package inventories artifact")?,
        1,
    )?;
    require_text(
        "package artifact scope",
        string_field(root, "evidence_scope", "package inventories artifact")?,
        GENERIC_SCOPE,
    )?;
    let boundary = object(
        field(root, "workspace_boundary", "package inventories artifact")?,
        "workspace boundary",
    )?;
    let sources = object(
        field(boundary, "sources", "workspace boundary")?,
        "source packages",
    )?;
    for (role, expected) in [
        ("current", &binding.package_tree_sha256.current),
        (
            "replacement_submission",
            &binding.package_tree_sha256.replacement_submission,
        ),
        (
            "new_submission",
            &binding.package_tree_sha256.new_submission,
        ),
    ] {
        let source = object(field(sources, role, "source packages")?, "source package")?;
        for moment in ["before_staging", "after_staging", "after_run"] {
            let inventory: PackageInventoryDto = serde_json::from_value(
                field(source, moment, "source package")?.clone(),
            )
            .map_err(|source| RepresentativeVerificationError::Json {
                path: "core/package-inventories.json",
                source,
            })?;
            verify_inventory(
                &inventory,
                "source package inventory",
                MAX_PACKAGE_INVENTORY_ENTRIES,
            )?;
            require_text(
                "role-tagged source tree SHA",
                &inventory.tree_sha256,
                expected,
            )?;
        }
    }
    Ok(())
}

fn verify_case_binding(
    bytes: &[u8],
    core: &Value,
    outer: &OuterReportDto,
    binding: &BindingDto,
) -> Result<crate::case::LoadedCase, RepresentativeVerificationError> {
    require_sha_match("case artifact SHA", &binding.case_sha256, &sha256(bytes))?;
    let text = std::str::from_utf8(bytes).map_err(|_| policy("core case.toml must be UTF-8"))?;
    let case = crate::case::parse_case(text)?;
    require_text(
        "strict case source SHA",
        case.source_sha256(),
        &binding.case_sha256,
    )?;
    let manifest = case.manifest();
    require_text(
        "case id",
        core.pointer("/metadata/case_id")
            .and_then(Value::as_str)
            .ok_or_else(|| policy("core metadata case_id is missing"))?,
        &manifest.case_id,
    )?;
    require_text(
        "case target version",
        &manifest.target_spine_version,
        TARGET_SPINE_VERSION,
    )?;
    require_text(
        "case expected editor SHA",
        &manifest.editor.expected_executable_sha256,
        &outer.metadata.expected_editor_executable_sha256,
    )?;
    Ok(case)
}

fn verify_exact_layout(
    observed: &ObservedEvidence,
    expected_core_files: &BTreeSet<String>,
) -> Result<(), RepresentativeVerificationError> {
    let mut expected_files = expected_core_files.clone();
    expected_files.insert("report.json".to_owned());
    expected_files.insert("representative-binding.toml".to_owned());
    let expected_directories = directories_for_files(&expected_files);
    if observed.files.keys().cloned().collect::<BTreeSet<_>>() != expected_files {
        return Err(RepresentativeVerificationError::InvalidLayout(
            "files differ from the authenticated fixed catalog".to_owned(),
        ));
    }
    if observed.directories != expected_directories {
        return Err(RepresentativeVerificationError::InvalidLayout(
            "directories differ from the authenticated fixed catalog".to_owned(),
        ));
    }
    Ok(())
}

fn directories_for_files(files: &BTreeSet<String>) -> BTreeSet<String> {
    let mut directories = BTreeSet::from([".".to_owned()]);
    for file in files {
        let path = Path::new(file);
        let mut parent = path.parent();
        while let Some(value) = parent {
            if value.as_os_str().is_empty() {
                break;
            }
            directories.insert(portable_path(value));
            parent = value.parent();
        }
    }
    directories
}

fn observed_core_inventory(
    observed: &ObservedEvidence,
) -> Result<PackageInventoryDto, RepresentativeVerificationError> {
    let mut entries = vec![TreeEntryDto {
        path: ".".to_owned(),
        kind: "directory".to_owned(),
        size: 0,
        sha256: None,
    }];
    for directory in &observed.directories {
        let Some(relative) = directory.strip_prefix("core/") else {
            continue;
        };
        entries.push(TreeEntryDto {
            path: relative.to_owned(),
            kind: "directory".to_owned(),
            size: 0,
            sha256: None,
        });
    }
    for (path, file) in &observed.files {
        let Some(relative) = path.strip_prefix("core/") else {
            continue;
        };
        entries.push(TreeEntryDto {
            path: relative.to_owned(),
            kind: "file".to_owned(),
            size: u64::try_from(file.bytes.len()).unwrap_or(u64::MAX),
            sha256: Some(file.sha256.clone()),
        });
    }
    entries.sort();
    let tree_sha256 = package_tree_sha256(&entries);
    Ok(PackageInventoryDto {
        tree_sha256,
        entries,
    })
}

fn verify_inventory(
    inventory: &PackageInventoryDto,
    label: &str,
    max_entries: usize,
) -> Result<(), RepresentativeVerificationError> {
    validate_sha("inventory tree SHA", &inventory.tree_sha256)?;
    validate_inventory_entry_count(inventory.entries.len(), max_entries, label)?;
    let mut prior: Option<&str> = None;
    for (index, entry) in inventory.entries.iter().enumerate() {
        validate_portable_inventory_path(&entry.path, index == 0)?;
        if prior.is_some_and(|prior| prior >= entry.path.as_str()) {
            return Err(policy(format!("{label} entries are not strictly sorted")));
        }
        prior = Some(&entry.path);
        match entry.kind.as_str() {
            "directory" if entry.size == 0 && entry.sha256.is_none() => {}
            "file" if entry.sha256.as_deref().is_some_and(is_sha256) => {}
            _ => return Err(policy(format!("{label} contains an invalid entry"))),
        }
    }
    require_sha_match(
        "package inventory domain digest",
        &inventory.tree_sha256,
        &package_tree_sha256(&inventory.entries),
    )
}

fn validate_inventory_entry_count(
    count: usize,
    max_entries: usize,
    label: &str,
) -> Result<(), RepresentativeVerificationError> {
    if count == 0 || count > max_entries {
        Err(policy(format!("{label} has an invalid entry count")))
    } else {
        Ok(())
    }
}

fn validate_portable_inventory_path(
    value: &str,
    root: bool,
) -> Result<(), RepresentativeVerificationError> {
    if root {
        return require_text("inventory root entry", value, ".");
    }
    if value.is_empty()
        || value == "."
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains('\\')
        || value
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err(policy("inventory contains a non-portable path"));
    }
    Ok(())
}

fn package_tree_sha256(entries: &[TreeEntryDto]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(PACKAGE_TREE_DOMAIN);
    hasher.update((entries.len() as u64).to_be_bytes());
    for entry in entries {
        hasher.update([if entry.kind == "directory" {
            b'd'
        } else {
            b'f'
        }]);
        hasher.update((entry.path.len() as u64).to_be_bytes());
        hasher.update(entry.path.as_bytes());
        hasher.update(entry.size.to_be_bytes());
        if let Some(digest) = &entry.sha256 {
            hasher.update((digest.len() as u64).to_be_bytes());
            hasher.update(digest.as_bytes());
        } else {
            hasher.update(0_u64.to_be_bytes());
        }
    }
    hex(hasher.finalize().as_slice())
}

fn core_content_tree_sha256(inventory_sha256: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(CORE_CONTENT_TREE_DOMAIN);
    hasher.update(inventory_sha256.as_bytes());
    hex(hasher.finalize().as_slice())
}

fn validate_artifact(artifact: &ArtifactDto) -> Result<(), RepresentativeVerificationError> {
    if artifact.role.is_empty()
        || !artifact
            .role
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(policy("artifact role is not a lowercase slug"));
    }
    validate_portable_inventory_path(&artifact.path, false)?;
    validate_sha("artifact SHA", &artifact.sha256)
}

fn fixed_success_artifacts() -> BTreeSet<(String, String)> {
    let mut artifacts = BTreeSet::from([
        ("case.toml".to_owned(), "case-manifest".to_owned()),
        (
            "package-inventories.json".to_owned(),
            "package-inventories".to_owned(),
        ),
        (
            "native-validations.json".to_owned(),
            "native-validations".to_owned(),
        ),
        (
            "comparisons/roundtrip.json".to_owned(),
            "roundtrip-comparison".to_owned(),
        ),
        (
            "comparisons/existing-import.json".to_owned(),
            "existing-import-comparison".to_owned(),
        ),
        (
            "comparisons/new-import.json".to_owned(),
            "new-import-comparison".to_owned(),
        ),
    ]);
    for index in 0..EXPECTED_OPERATIONS.len() {
        artifacts.insert((
            format!("processes/{index:04}.stdout.txt"),
            "process-stdout".to_owned(),
        ));
        artifacts.insert((
            format!("processes/{index:04}.stderr.txt"),
            "process-stderr".to_owned(),
        ));
    }
    artifacts
}

fn parse_artifact_value(value: &Value) -> Result<ArtifactDto, RepresentativeVerificationError> {
    serde_json::from_value(value.clone()).map_err(|source| RepresentativeVerificationError::Json {
        path: "core/report.json",
        source,
    })
}

fn file_bytes<'a>(
    observed: &'a ObservedEvidence,
    path: &str,
) -> Result<&'a [u8], RepresentativeVerificationError> {
    observed
        .files
        .get(path)
        .map(|file| file.bytes.as_slice())
        .ok_or_else(|| RepresentativeVerificationError::InvalidLayout(format!("missing `{path}`")))
}

fn parse_json<T: serde::de::DeserializeOwned>(
    path: &'static str,
    bytes: &[u8],
) -> Result<T, RepresentativeVerificationError> {
    let StrictJsonValue(value) = serde_json::from_slice(bytes)
        .map_err(|source| RepresentativeVerificationError::Json { path, source })?;
    serde_json::from_value(value)
        .map_err(|source| RepresentativeVerificationError::Json { path, source })
}

struct StrictJsonValue(Value);

impl<'de> Deserialize<'de> for StrictJsonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictJsonVisitor)
    }
}

struct StrictJsonVisitor;

impl<'de> Visitor<'de> for StrictJsonVisitor {
    type Value = StrictJsonValue;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON value without duplicate object keys")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::Number(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::Number(value.into())))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .map(StrictJsonValue)
            .ok_or_else(|| E::custom("JSON number must be finite"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_string(value.to_owned())
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        StrictJsonValue::deserialize(deserializer)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::Null))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::with_capacity(
            sequence
                .size_hint()
                .unwrap_or(0)
                .min(MAX_PHYSICAL_EVIDENCE_ENTRIES),
        );
        while let Some(StrictJsonValue(value)) = sequence.next_element()? {
            values.push(value);
        }
        Ok(StrictJsonValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut entries: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = Map::new();
        while let Some(key) = entries.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(de::Error::custom(format_args!(
                    "duplicate JSON object key `{key}`"
                )));
            }
            let StrictJsonValue(value) = entries.next_value()?;
            values.insert(key, value);
        }
        Ok(StrictJsonValue(Value::Object(values)))
    }
}

fn object<'a>(
    value: &'a Value,
    label: &str,
) -> Result<&'a Map<String, Value>, RepresentativeVerificationError> {
    value
        .as_object()
        .ok_or_else(|| policy(format!("{label} must be an object")))
}

fn array<'a>(
    value: &'a Value,
    label: &str,
) -> Result<&'a [Value], RepresentativeVerificationError> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| policy(format!("{label} must be an array")))
}

fn field<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    label: &str,
) -> Result<&'a Value, RepresentativeVerificationError> {
    object
        .get(key)
        .ok_or_else(|| policy(format!("{label} is missing `{key}`")))
}

fn string_field<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    label: &str,
) -> Result<&'a str, RepresentativeVerificationError> {
    field(object, key, label)?
        .as_str()
        .ok_or_else(|| policy(format!("{label}.{key} must be a string")))
}

fn bool_field(
    object: &Map<String, Value>,
    key: &str,
    label: &str,
) -> Result<bool, RepresentativeVerificationError> {
    field(object, key, label)?
        .as_bool()
        .ok_or_else(|| policy(format!("{label}.{key} must be a boolean")))
}

fn u64_field(
    object: &Map<String, Value>,
    key: &str,
    label: &str,
) -> Result<u64, RepresentativeVerificationError> {
    field(object, key, label)?
        .as_u64()
        .ok_or_else(|| policy(format!("{label}.{key} must be an unsigned integer")))
}

fn usize_field(
    object: &Map<String, Value>,
    key: &str,
    label: &str,
) -> Result<usize, RepresentativeVerificationError> {
    usize::try_from(u64_field(object, key, label)?)
        .map_err(|_| policy(format!("{label}.{key} is too large")))
}

fn require_keys(
    object: &Map<String, Value>,
    expected: &[&str],
    label: &str,
) -> Result<(), RepresentativeVerificationError> {
    let actual = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    if actual == expected {
        Ok(())
    } else {
        Err(policy(format!("{label} has missing or unknown fields")))
    }
}

fn require_equal<T: Eq + std::fmt::Debug>(
    label: &str,
    actual: T,
    expected: T,
) -> Result<(), RepresentativeVerificationError> {
    if actual == expected {
        Ok(())
    } else {
        Err(policy(format!("{label} does not match fixed policy")))
    }
}

fn require_text(
    label: &str,
    actual: &str,
    expected: &str,
) -> Result<(), RepresentativeVerificationError> {
    if actual == expected {
        Ok(())
    } else {
        Err(policy(format!(
            "{label} does not match its authenticated value"
        )))
    }
}

fn require_sha_match(
    label: &str,
    actual: &str,
    expected: &str,
) -> Result<(), RepresentativeVerificationError> {
    validate_sha(label, actual)?;
    require_text(label, actual, expected)
}

fn require_roles_equal(
    label: &str,
    actual: &RoleDigestsDto,
    expected: &RoleDigestsDto,
) -> Result<(), RepresentativeVerificationError> {
    if actual == expected {
        Ok(())
    } else {
        Err(policy(format!("{label} do not match by role")))
    }
}

fn validate_role_digests(
    roles: &RoleDigestsDto,
    label: &str,
) -> Result<(), RepresentativeVerificationError> {
    validate_sha(label, &roles.current)?;
    validate_sha(label, &roles.replacement_submission)?;
    validate_sha(label, &roles.new_submission)
}

fn validate_binding_id(value: &str) -> Result<(), RepresentativeVerificationError> {
    let valid = value.strip_prefix("rep-").is_some_and(|suffix| {
        suffix.len() == 32
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    });
    if valid {
        Ok(())
    } else {
        Err(policy("binding_id is not an opaque representative id"))
    }
}

fn validate_revision(value: &str) -> Result<(), RepresentativeVerificationError> {
    if matches!(value.len(), 40 | 64) && value.bytes().all(is_lower_hex) {
        Ok(())
    } else {
        Err(policy(
            "source_revision must be 40 or 64 lowercase hex digits",
        ))
    }
}

fn validate_safe_token(label: &str, value: &str) -> Result<(), RepresentativeVerificationError> {
    if !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'))
    {
        Ok(())
    } else {
        Err(policy(format!("{label} is not a safe portable token")))
    }
}

fn validate_sha(label: &str, value: &str) -> Result<(), RepresentativeVerificationError> {
    if is_sha256(value) {
        Ok(())
    } else {
        Err(policy(format!("{label} is not a lowercase SHA-256")))
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(is_lower_hex)
}

fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

fn sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex(hasher.finalize().as_slice())
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn policy(message: impl Into<String>) -> RepresentativeVerificationError {
    RepresentativeVerificationError::Policy(message.into())
}

const EXPECTED_OPERATIONS: &[&str] = &[
    "spine-version",
    "spine-advanced-help",
    "spine-project-info",
    "spine-project-info",
    "spine-project-info",
    "spine-export-json",
    "spine-export-json",
    "spine-export-json",
    "spine-reconstruct-json",
    "spine-export-json",
    "spine-export-json",
    "spine-reconstruct-json",
    "spine-export-json",
    "spine-import-existing-animation",
    "spine-export-json",
    "spine-import-existing-animation",
    "spine-export-json",
    "spine-import-new-animation",
    "spine-export-json",
    "spine-new-animation-collision-control",
    "spine-export-json",
    "spine-missing-images-path-control",
];

const EXPECTED_ASSERTIONS: &[&str] = &[
    "case_manifest_validated",
    "package_contexts_inventoried",
    "executable_identity",
    "exact_editor_version",
    "license_activated",
    "advanced_arguments_accepted",
    "target_skeletons_found",
    "native_validator_available",
    "editor_calls_serialized",
    "reconstruction_round_trip_first",
    "reconstruction_round_trip_repeat",
    "round_trip_deterministic",
    "round_trip_differences_explained",
    "round_trip_losses_recorded",
    "existing_import_matches_submission",
    "existing_import_preserves_setup",
    "existing_import_preserves_other_animations",
    "existing_import_idempotent",
    "new_import_matches_submission",
    "new_import_preserves_setup",
    "new_import_preserves_other_animations",
    "new_import_collision_hazard_detected",
    "source_packages_unchanged",
    "transcript_policy_passed",
    "missing_path_negative_control",
];

const ALL: &[usize] = &[
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21,
];
const ADVANCED: &[usize] = &[1, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20];
const ASSERTION_CITATIONS: &[(&str, &[usize])] = &[
    ("case.toml", &[]),
    ("package-inventories.json", &[2, 3, 4]),
    ("case.toml", ALL),
    ("case.toml", &[0]),
    ("case.toml", &[0, 5, 8, 13, 17]),
    ("case.toml", ADVANCED),
    ("package-inventories.json", &[2, 3, 4, 13, 15, 17, 19]),
    ("native-validations.json", &[]),
    ("case.toml", ALL),
    ("comparisons/roundtrip.json", &[5, 8, 9]),
    ("comparisons/roundtrip.json", &[10, 11, 12]),
    ("comparisons/roundtrip.json", &[5, 9, 10, 12]),
    ("comparisons/roundtrip.json", &[5, 9, 10, 12]),
    ("comparisons/roundtrip.json", &[9, 12]),
    ("comparisons/existing-import.json", &[6, 13, 14]),
    ("comparisons/existing-import.json", &[13, 14]),
    ("comparisons/existing-import.json", &[6, 13, 14]),
    ("comparisons/existing-import.json", &[15, 16]),
    ("comparisons/new-import.json", &[7, 17, 18]),
    ("comparisons/new-import.json", &[17, 18]),
    ("comparisons/new-import.json", &[7, 17, 18]),
    ("comparisons/new-import.json", &[19, 20]),
    ("package-inventories.json", ALL),
    ("case.toml", ALL),
    ("case.toml", &[21]),
];

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn validate_absolute_root(path: &Path) -> Result<PathBuf, RepresentativeVerificationError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::RootDir | Component::Normal(_)))
    {
        return Err(RepresentativeVerificationError::InvalidEvidencePath(
            path.to_path_buf(),
        ));
    }
    let metadata =
        fs::symlink_metadata(path).map_err(|source| io_error("inspect root", path, source))?;
    validate_directory(path, &metadata)?;
    let canonical =
        fs::canonicalize(path).map_err(|source| io_error("canonicalize root", path, source))?;
    if canonical != path {
        return Err(RepresentativeVerificationError::InvalidEvidencePath(
            path.to_path_buf(),
        ));
    }
    Ok(canonical)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn validate_absolute_root(_path: &Path) -> Result<PathBuf, RepresentativeVerificationError> {
    Err(RepresentativeVerificationError::UnsupportedPlatform)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn observe_evidence(root: &Path) -> Result<ObservedEvidence, RepresentativeVerificationError> {
    let root_metadata =
        fs::symlink_metadata(root).map_err(|source| io_error("inspect root", root, source))?;
    validate_directory(root, &root_metadata)?;
    let root_state = DirectoryState::from_metadata(&root_metadata);
    let mut files = BTreeMap::new();
    let mut directories = BTreeSet::from([".".to_owned()]);
    let mut pending = vec![root.to_path_buf()];
    let mut total_bytes = 0_u64;

    while let Some(directory) = pending.pop() {
        let mut children = fs::read_dir(&directory)
            .map_err(|source| io_error("read directory", &directory, source))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|source| io_error("read directory entry", &directory, source))?;
        children.sort_by_key(fs::DirEntry::file_name);
        for child in children {
            let path = child.path();
            let name = child
                .file_name()
                .into_string()
                .map_err(|_| unsafe_fs(&path, "entry name must be UTF-8"))?;
            if name.contains('\\') || name == "." || name == ".." || name.ends_with(".part") {
                return Err(unsafe_fs(&path, "entry name is not allowed"));
            }
            let relative = path
                .strip_prefix(root)
                .map(portable_path)
                .map_err(|_| unsafe_fs(&path, "entry escaped evidence root"))?;
            let metadata = fs::symlink_metadata(&path)
                .map_err(|source| io_error("inspect entry", &path, source))?;
            if metadata.file_type().is_symlink() {
                return Err(unsafe_fs(&path, "symbolic links are forbidden"));
            }
            if metadata.file_type().is_dir() {
                validate_directory(&path, &metadata)?;
                if !directories.insert(relative) {
                    return Err(unsafe_fs(&path, "duplicate directory identity"));
                }
                pending.push(path);
            } else if metadata.file_type().is_file() {
                validate_regular_file(&path, &metadata)?;
                if metadata.len() > MAX_FILE_BYTES {
                    return Err(RepresentativeVerificationError::SizeLimit("per-file"));
                }
                let bytes = secure_read_file(&path, &metadata)?;
                total_bytes = total_bytes
                    .checked_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX))
                    .ok_or(RepresentativeVerificationError::SizeLimit("total bytes"))?;
                if total_bytes > MAX_TOTAL_BYTES {
                    return Err(RepresentativeVerificationError::SizeLimit("total bytes"));
                }
                let digest = sha256(&bytes);
                if files
                    .insert(
                        relative,
                        ObservedFile {
                            bytes,
                            sha256: digest,
                        },
                    )
                    .is_some()
                {
                    return Err(unsafe_fs(&path, "duplicate file identity"));
                }
            } else {
                return Err(unsafe_fs(&path, "special files are forbidden"));
            }
            if files.len().saturating_add(directories.len()) > MAX_PHYSICAL_EVIDENCE_ENTRIES {
                return Err(RepresentativeVerificationError::SizeLimit("entry count"));
            }
        }
    }
    Ok(ObservedEvidence {
        root_state,
        files,
        directories,
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn observe_evidence(_root: &Path) -> Result<ObservedEvidence, RepresentativeVerificationError> {
    Err(RepresentativeVerificationError::UnsupportedPlatform)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn validate_directory(
    path: &Path,
    metadata: &Metadata,
) -> Result<(), RepresentativeVerificationError> {
    use std::os::unix::fs::MetadataExt as _;
    if !metadata.file_type().is_dir() {
        return Err(unsafe_fs(path, "expected a physical directory"));
    }
    if metadata.uid() != rustix::process::geteuid().as_raw() {
        return Err(unsafe_fs(
            path,
            "directory is not owned by the effective user",
        ));
    }
    if metadata.mode() & 0o7777 != PRIVATE_DIRECTORY_MODE {
        return Err(unsafe_fs(
            path,
            "directory permissions must be exactly 0700",
        ));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn validate_regular_file(
    path: &Path,
    metadata: &Metadata,
) -> Result<(), RepresentativeVerificationError> {
    use std::os::unix::fs::MetadataExt as _;
    if !metadata.file_type().is_file() {
        return Err(unsafe_fs(path, "expected a physical regular file"));
    }
    if metadata.nlink() != 1 {
        return Err(unsafe_fs(
            path,
            "regular files must have exactly one hard link",
        ));
    }
    if metadata.uid() != rustix::process::geteuid().as_raw() {
        return Err(unsafe_fs(path, "file is not owned by the effective user"));
    }
    if metadata.mode() & 0o7777 != PRIVATE_FILE_MODE {
        return Err(unsafe_fs(path, "file permissions must be exactly 0600"));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn secure_read_file(
    path: &Path,
    before: &Metadata,
) -> Result<Vec<u8>, RepresentativeVerificationError> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let mut file = options
        .open(path)
        .map_err(|source| io_error("open file", path, source))?;
    let opened = file
        .metadata()
        .map_err(|source| io_error("inspect opened file", path, source))?;
    validate_regular_file(path, &opened)?;
    if !same_regular_file(before, &opened) {
        return Err(unsafe_fs(path, "file identity changed while opening"));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(opened.len()).unwrap_or(0));
    file.by_ref()
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| io_error("read file", path, source))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_FILE_BYTES {
        return Err(RepresentativeVerificationError::SizeLimit("per-file"));
    }
    let after = file
        .metadata()
        .map_err(|source| io_error("reinspect opened file", path, source))?;
    let named_after = fs::symlink_metadata(path)
        .map_err(|source| io_error("reinspect named file", path, source))?;
    validate_regular_file(path, &after)?;
    validate_regular_file(path, &named_after)?;
    if !same_regular_file(&opened, &after)
        || !same_regular_file(&after, &named_after)
        || after.len() != u64::try_from(bytes.len()).unwrap_or(u64::MAX)
    {
        return Err(unsafe_fs(path, "file changed while it was read"));
    }
    Ok(bytes)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn same_regular_file(left: &Metadata, right: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    left.dev() == right.dev()
        && left.ino() == right.ino()
        && left.mode() == right.mode()
        && left.nlink() == right.nlink()
        && left.uid() == right.uid()
        && left.gid() == right.gid()
        && left.len() == right.len()
        && left.mtime() == right.mtime()
        && left.mtime_nsec() == right.mtime_nsec()
        && left.ctime() == right.ctime()
        && left.ctime_nsec() == right.ctime_nsec()
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl DirectoryState {
    fn from_metadata(metadata: &Metadata) -> Self {
        use std::os::unix::fs::MetadataExt as _;
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            mode: metadata.mode(),
            owner: metadata.uid(),
            group: metadata.gid(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
            changed_seconds: metadata.ctime(),
            changed_nanoseconds: metadata.ctime_nsec(),
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn require_root_state_unchanged(
    root: &Path,
    expected: &DirectoryState,
) -> Result<(), RepresentativeVerificationError> {
    let metadata =
        fs::symlink_metadata(root).map_err(|source| io_error("reinspect root", root, source))?;
    validate_directory(root, &metadata)?;
    if &DirectoryState::from_metadata(&metadata) == expected {
        Ok(())
    } else {
        Err(unsafe_fs(root, "evidence root changed during verification"))
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn require_root_state_unchanged(
    _root: &Path,
    _expected: &DirectoryState,
) -> Result<(), RepresentativeVerificationError> {
    Err(RepresentativeVerificationError::UnsupportedPlatform)
}

fn portable_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn io_error(
    operation: &'static str,
    path: &Path,
    source: io::Error,
) -> RepresentativeVerificationError {
    RepresentativeVerificationError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

fn unsafe_fs(path: &Path, reason: &'static str) -> RepresentativeVerificationError {
    RepresentativeVerificationError::UnsafeFilesystem {
        path: path.to_path_buf(),
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const CASE_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const HARNESS_SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const EDITOR_SHA: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    const CURRENT_SHA: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
    const REPLACEMENT_SHA: &str =
        "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    const NEW_SHA: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    const CARGO_LOCK_SHA: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const SOURCE_REVISION: &str = "2222222222222222222222222222222222222222";

    fn roles() -> RoleDigestsDto {
        RoleDigestsDto {
            current: CURRENT_SHA.to_owned(),
            replacement_submission: REPLACEMENT_SHA.to_owned(),
            new_submission: NEW_SHA.to_owned(),
        }
    }

    fn binding() -> BindingDto {
        BindingDto {
            format_version: 1,
            evidence_class: BINDING_EVIDENCE_CLASS.to_owned(),
            binding_id: "rep-0123456789abcdef0123456789abcdef".to_owned(),
            case_sha256: CASE_SHA.to_owned(),
            harness_executable_sha256: HARNESS_SHA.to_owned(),
            package_tree_sha256: roles(),
            build: BindingBuildDto {
                source_revision: SOURCE_REVISION.to_owned(),
                cargo_lock_sha256: CARGO_LOCK_SHA.to_owned(),
            },
        }
    }

    fn empty_inventory() -> PackageInventoryDto {
        let entries = vec![TreeEntryDto {
            path: ".".to_owned(),
            kind: "directory".to_owned(),
            size: 0,
            sha256: None,
        }];
        PackageInventoryDto {
            tree_sha256: package_tree_sha256(&entries),
            entries,
        }
    }

    fn outer() -> OuterReportDto {
        OuterReportDto {
            format_version: 5,
            metadata: OuterMetadataDto {
                evidence_scope: REPRESENTATIVE_SCOPE.to_owned(),
                representative_gate_eligible: true,
                binding_id: binding().binding_id,
                representative_binding_sha256: CASE_SHA.to_owned(),
                case_sha256: CASE_SHA.to_owned(),
                harness_executable_sha256: HARNESS_SHA.to_owned(),
                expected_editor_executable_sha256: EDITOR_SHA.to_owned(),
                package_tree_sha256: roles(),
                source_revision: SOURCE_REVISION.to_owned(),
                cargo_lock_sha256: CARGO_LOCK_SHA.to_owned(),
                target_spine_version: TARGET_SPINE_VERSION.to_owned(),
                tool_version: env!("CARGO_PKG_VERSION").to_owned(),
            },
            passed: true,
            core: OuterCoreDto {
                outcome: "passed".to_owned(),
                report_path: "core/report.json".to_owned(),
                report_sha256: CASE_SHA.to_owned(),
                inventory: empty_inventory(),
                content_tree_sha256: CASE_SHA.to_owned(),
            },
            top_level_artifacts: vec![],
            validation: OuterValidationDto {
                core_schema_validated: true,
                clean_build_provenance_validated: true,
                harness_identity_validated: true,
                editor_launcher_validated: true,
                package_bindings_validated: true,
                workspace_source_bindings_validated: true,
                marker_value_sha256: CASE_SHA.to_owned(),
                marker_processes_validated: 22,
                marker_evidence_complete: true,
                process_count: 22,
                assertion_count: 25,
                passed_assertion_count: 25,
                integrity_failure_count: Some(0),
            },
        }
    }

    fn fixture_executable_identity() -> ExecutableIdentityDto {
        ExecutableIdentityDto {
            canonical_path: PathBuf::from("/Applications/Spine.app/Contents/MacOS/Spine"),
            sha256: EDITOR_SHA.to_owned(),
            size: 1,
            device: 1,
            inode: 2,
            mode: 0o100755,
            owner: 501,
            modified_seconds: 1,
            modified_nanoseconds: 0,
            changed_seconds: 1,
            changed_nanoseconds: 0,
            local_filesystem_verified: true,
        }
    }

    fn passing_provenance() -> Value {
        let stable_file_identity_sha256 =
            stable_executable_identity_sha256(&fixture_executable_identity());
        json!({
            "environment": {
                "build_context": {
                    "relationship": "context_only_not_binary_attestation",
                    "checkout": {
                        "head": SOURCE_REVISION,
                        "dirty": false,
                        "status_sha256": CASE_SHA
                    },
                    "cargo_lock": {"sha256": CARGO_LOCK_SHA, "size": 1},
                    "rustc": {
                        "verbose_version_sha256": CASE_SHA,
                        "release": "1.95.0",
                        "commit_hash": SOURCE_REVISION,
                        "host_triple": "aarch64-apple-darwin"
                    },
                    "build_host_triple": "aarch64-apple-darwin",
                    "target_triple": "aarch64-apple-darwin"
                },
                "harness_executable": {
                    "sha256": HARNESS_SHA,
                    "size": 1,
                    "stable_file_identity_sha256": CASE_SHA
                },
                "runtime_host": {
                    "operating_system": "macos",
                    "process_architecture": "aarch64",
                    "kernel_family": "unix"
                }
            },
            "fixture": {
                "case_sha256": CASE_SHA,
                "target_spine_version": TARGET_SPINE_VERSION,
                "packages": {
                    "current": CURRENT_SHA,
                    "replacement_submission": REPLACEMENT_SHA,
                    "new_submission": NEW_SHA
                },
                "export_preset": {
                    "preset": "pretty-nonessential-json",
                    "sha256": sha256(crate::spine_cli::approved_export_preset_bytes())
                }
            },
            "spine_launcher": {
                "expected_sha256": EDITOR_SHA,
                "observed": {
                    "sha256": EDITOR_SHA,
                    "size": 1,
                    "stable_file_identity_sha256": stable_file_identity_sha256
                },
                "target_spine_version": TARGET_SPINE_VERSION,
                "observed_processes": 22
            }
        })
    }

    fn valid_case_toml() -> String {
        format!(
            r#"format_version = 2
case_id = "representative-case"
target_spine_version = "4.3.23"
runtime_atlas = "character.atlas"

[editor]
expected_executable_sha256 = "{EDITOR_SHA}"

[packages.current]
root = "/private/current"
project = "character.spine"
required_directories = ["images"]
asset_roots = ["images"]

[packages.replacement_submission]
root = "/private/replacement"
project = "character.spine"
required_directories = ["images"]
asset_roots = ["images"]

[packages.new_submission]
root = "/private/new"
project = "character.spine"
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
"#
        )
    }

    fn producer_shaped_process(
        index: usize,
        artifacts_by_path: &BTreeMap<String, ArtifactDto>,
        marker_value_sha256: &str,
        parsed_case: &crate::case::LoadedCase,
    ) -> Value {
        let stdout = artifacts_by_path
            .get(&format!("processes/{index:04}.stdout.txt"))
            .expect("stdout artifact")
            .clone();
        let stderr = artifacts_by_path
            .get(&format!("processes/{index:04}.stderr.txt"))
            .expect("stderr artifact")
            .clone();
        let expectation = match index {
            19 => json!({
                "kind": "negative_control",
                "expected_failure": "new_animation_collision_diagnostic"
            }),
            21 => json!({
                "kind": "negative_control",
                "expected_failure": "missing_images_path_diagnostic"
            }),
            _ => json!({"kind": "required_success"}),
        };
        let stdout_bytes = format!("synthetic processes/{index:04}.stdout.txt\n").into_bytes();
        let stderr_bytes = format!("synthetic processes/{index:04}.stderr.txt\n").into_bytes();
        let stdout_sha256 = sha256(&stdout_bytes);
        let stderr_sha256 = sha256(&stderr_bytes);
        let required_outputs = expected_required_outputs(index);
        let failures = match index {
            19 => vec![json!({
                "code": "blocking_diagnostic",
                "detail": "stdout contained the exact expected new-animation collision diagnostic"
            })],
            21 => vec![json!({
                "code": "blocking_diagnostic",
                "detail": "stdout contained the exact expected `Images path not found: ./images/` diagnostic"
            })],
            _ => vec![],
        };
        let workspace = Path::new("/private/phase0a-workspace");
        let mut evidence = json!({
            "operation": EXPECTED_OPERATIONS[index],
            "requested_program": "/Applications/Spine.app/Contents/MacOS/Spine",
            "args": expected_process_args(index, workspace, parsed_case)
                .expect("fixed process arguments"),
            "requested_working_directory": workspace,
            "environment": [
                {"name": "HOME", "value_sha256": CASE_SHA},
                {"name": "LANG", "value_sha256": CASE_SHA},
                {"name": "LC_ALL", "value_sha256": CASE_SHA},
                {"name": "PATH", "value_sha256": CASE_SHA},
                {"name": MARKER_NAME, "value_sha256": marker_value_sha256}
            ],
            "timeout_seconds": if index <= 1 { 120 } else if index <= 4 { 300 } else { 1_800 },
            "timeout_subsec_nanos": 0,
            "cleanup_timeout_seconds": 30,
            "cleanup_timeout_subsec_nanos": 0,
            "max_retained_bytes_per_stream": 4 * 1024 * 1024,
            "executable_identity": {
                "canonical_path": "/Applications/Spine.app/Contents/MacOS/Spine",
                "sha256": EDITOR_SHA,
                "size": 1,
                "device": 1,
                "inode": 2,
                "mode": 0o100755,
                "owner": 501,
                "modified_seconds": 1,
                "modified_nanoseconds": 0,
                "changed_seconds": 1,
                "changed_nanoseconds": 0,
                "local_filesystem_verified": true
            },
            "working_directory_identity": {
                "canonical_path": workspace,
                "device": 1,
                "inode": 3,
                "mode": 0o040700,
                "owner": 501,
                "local_filesystem_verified": true
            },
            "lock_evidence": {
                "canonical_path": "/private/spine-editor.lock",
                "wait_seconds": 0,
                "wait_subsec_nanos": 0,
                "acquired": true,
                "local_filesystem_verified": true,
                "device": 1,
                "inode": 4,
                "filesystem_kind": "apfs"
            },
            "exit_code": 0,
            "terminating_signal": null,
            "sent_signal": null,
            "termination_reason": "natural_exit",
            "elapsed_seconds": 1,
            "elapsed_subsec_nanos": 0,
            "cleanup_status": "complete",
            "adapter_failure": null,
            "stdout": {
                "total_observed_bytes": stdout_bytes.len(),
                "retained_bytes": stdout_bytes.len(),
                "retained_prefix_sha256": stdout_sha256,
                "bytes_seen_sha256": stdout_sha256,
                "full_stream_sha256": stdout_sha256,
                "retained_prefix_truncated": false,
                "complete": true
            },
            "stderr": {
                "total_observed_bytes": stderr_bytes.len(),
                "retained_bytes": stderr_bytes.len(),
                "retained_prefix_sha256": stderr_sha256,
                "bytes_seen_sha256": stderr_sha256,
                "full_stream_sha256": stderr_sha256,
                "retained_prefix_truncated": false,
                "complete": true
            },
            "required_outputs": required_outputs,
            "observed_outputs": required_outputs,
            "output_discovery_state": "complete",
            "transcript_profile": expected_transcript_profile(index),
            "assessment": {
                "passed": !matches!(index, 19 | 21),
                "stdout_retained_prefix_sha256": stdout_sha256,
                "stderr_retained_prefix_sha256": stderr_sha256,
                "failures": failures
            }
        });
        if index == 19 {
            evidence["new_animation_collision"] = json!({
                "requested_animation": "gesture",
                "renamed_animation": "gesture2"
            });
        }
        json!({
            "expectation": expectation,
            "evidence": evidence,
            "stdout_artifact": stdout,
            "stderr_artifact": stderr
        })
    }

    fn standalone_producer_shaped_process(index: usize) -> (Value, crate::case::LoadedCase) {
        let mut artifacts_by_path = BTreeMap::new();
        for (suffix, role) in [("stdout", "process-stdout"), ("stderr", "process-stderr")] {
            let path = format!("processes/{index:04}.{suffix}.txt");
            let bytes = format!("synthetic {path}\n").into_bytes();
            artifacts_by_path.insert(
                path.clone(),
                ArtifactDto {
                    role: role.to_owned(),
                    path,
                    sha256: sha256(&bytes),
                },
            );
        }
        let parsed_case = crate::case::parse_case(&valid_case_toml()).expect("strict case");
        let marker_value_sha256 = sha256(CASE_SHA.as_bytes());
        let process = producer_shaped_process(
            index,
            &artifacts_by_path,
            &marker_value_sha256,
            &parsed_case,
        );
        (process, parsed_case)
    }

    #[test]
    fn process_schema_rejects_deleted_nested_fields() {
        let (mut process, _) = standalone_producer_shaped_process(0);
        process["evidence"]
            .as_object_mut()
            .expect("process evidence")
            .remove("args");
        assert!(close_recorded_process_shape(0, &process).is_err());
    }

    #[test]
    fn process_schema_rejects_extra_nested_fields() {
        let (mut process, _) = standalone_producer_shaped_process(0);
        process["evidence"]["stderr"]
            .as_object_mut()
            .expect("stderr stream")
            .insert("unexpected".to_owned(), json!(true));
        assert!(close_recorded_process_shape(0, &process).is_err());
    }

    #[test]
    fn launcher_provenance_tampering_is_rejected_against_process_identity() {
        let executable = fixture_executable_identity();
        let matching = LauncherProvenanceDto {
            size: executable.size,
            stable_file_identity_sha256: stable_executable_identity_sha256(&executable),
        };
        verify_launcher_process_binding(&executable, &matching)
            .expect("matching launcher identity");

        let wrong_size = LauncherProvenanceDto {
            size: executable.size + 1,
            stable_file_identity_sha256: matching.stable_file_identity_sha256.clone(),
        };
        assert!(verify_launcher_process_binding(&executable, &wrong_size).is_err());

        let wrong_stable_identity = LauncherProvenanceDto {
            size: executable.size,
            stable_file_identity_sha256: CASE_SHA.to_owned(),
        };
        assert!(verify_launcher_process_binding(&executable, &wrong_stable_identity).is_err());
    }

    #[test]
    fn negative_controls_reject_wrong_typed_failure_reasons() {
        for (index, wrong_code, wrong_detail) in [
            (
                19,
                "missing_output",
                "required output `export-json` was not produced",
            ),
            (
                21,
                "blocking_diagnostic",
                "stdout contained the exact expected new-animation collision diagnostic",
            ),
        ] {
            let (mut process, parsed_case) = standalone_producer_shaped_process(index);
            process["evidence"]["assessment"]["failures"][0]["code"] = json!(wrong_code);
            process["evidence"]["assessment"]["failures"][0]["detail"] = json!(wrong_detail);
            close_recorded_process_shape(index, &process).expect("producer-shaped process");
            let process: RecordedProcessDto =
                serde_json::from_value(process).expect("typed process evidence");
            assert!(verify_process_outcome(index, &process.evidence, &parsed_case).is_err());
        }
    }

    fn approved_semantic_evidence() -> (Value, Value) {
        (
            json!([{
                "pointer": "/skeleton/hash",
                "before": "old-hash",
                "after": "new-hash",
                "approved_volatile": true
            }]),
            json!([{
                "pointer": "/skeleton/hash",
                "description": "Spine regenerated the represented skeleton hash during JSON reconstruction; the complete observed string changes are retained in both round-trip comparisons."
            }]),
        )
    }

    #[test]
    fn semantic_difference_tampering_is_rejected() {
        let (mut differences, losses) = approved_semantic_evidence();
        verify_semantics(&differences, &losses).expect("approved semantic evidence");
        differences[0]["pointer"] = json!("/bones/0/name");
        assert!(verify_semantics(&differences, &losses).is_err());
    }

    #[test]
    fn roundtrip_loss_tampering_is_rejected() {
        let (differences, mut losses) = approved_semantic_evidence();
        losses[0]["description"] = json!("hash changed");
        assert!(verify_semantics(&differences, &losses).is_err());
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn complete_on_disk_eligible_v5_bundle_verifies() {
        let temporary = tempfile::tempdir().expect("temporary evidence root");
        make_private_directory(temporary.path());
        for relative in ["core", "core/comparisons", "core/processes"] {
            let directory = temporary.path().join(relative);
            fs::create_dir(&directory).expect("create evidence directory");
            make_private_directory(&directory);
        }

        let case_bytes = valid_case_toml().into_bytes();
        let case_sha256 = sha256(&case_bytes);
        let source_inventory = empty_inventory();
        let source_tree_sha256 = source_inventory.tree_sha256.clone();
        let binding_id = "rep-0123456789abcdef0123456789abcdef";
        let binding_bytes = format!(
            r#"format_version = 1
evidence_class = "{BINDING_EVIDENCE_CLASS}"
binding_id = "{binding_id}"
case_sha256 = "{case_sha256}"
harness_executable_sha256 = "{HARNESS_SHA}"

[package_tree_sha256]
current = "{source_tree_sha256}"
replacement_submission = "{source_tree_sha256}"
new_submission = "{source_tree_sha256}"

[build]
source_revision = "{SOURCE_REVISION}"
cargo_lock_sha256 = "{CARGO_LOCK_SHA}"
"#
        )
        .into_bytes();
        write_private_file(
            &temporary.path().join("representative-binding.toml"),
            &binding_bytes,
        );
        let binding_sha256 = sha256(&binding_bytes);
        let marker_value_sha256 = sha256(binding_sha256.as_bytes());

        let source = json!({
            "before_staging": source_inventory,
            "after_staging": source_inventory,
            "after_run": source_inventory
        });
        let package_inventory_bytes = json_bytes(&json!({
            "format_version": 1,
            "evidence_scope": GENERIC_SCOPE,
            "project_info": {},
            "workspace_boundary": {
                "sources": {
                    "current": source,
                    "replacement_submission": source,
                    "new_submission": source
                }
            },
            "matching_non_project_sha256": CASE_SHA,
            "matching_non_project_entries": []
        }));

        let mut artifacts_by_path = BTreeMap::new();
        for (path, role) in fixed_success_artifacts() {
            let bytes = match path.as_str() {
                "case.toml" => case_bytes.clone(),
                "package-inventories.json" => package_inventory_bytes.clone(),
                path if path.ends_with(".txt") => format!("synthetic {path}\n").into_bytes(),
                path => json_bytes(&json!({"synthetic_artifact": path})),
            };
            write_private_file(&temporary.path().join("core").join(&path), &bytes);
            artifacts_by_path.insert(
                path.clone(),
                ArtifactDto {
                    role,
                    path,
                    sha256: sha256(&bytes),
                },
            );
        }
        let artifacts = artifacts_by_path.values().cloned().collect::<Vec<_>>();

        let parsed_case =
            crate::case::parse_case(std::str::from_utf8(&case_bytes).expect("UTF-8 case"))
                .expect("strict case");
        let processes = EXPECTED_OPERATIONS
            .iter()
            .enumerate()
            .map(|(index, _)| {
                producer_shaped_process(
                    index,
                    &artifacts_by_path,
                    &marker_value_sha256,
                    &parsed_case,
                )
            })
            .collect::<Vec<_>>();

        let assertions = EXPECTED_ASSERTIONS
            .iter()
            .enumerate()
            .map(|(index, id)| {
                let evidence = fixed_assertion_citations(index, &artifacts)
                    .expect("fixed citation graph")
                    .into_iter()
                    .collect::<Vec<_>>();
                json!({
                    "id": id,
                    "status": "passed",
                    "summary": "synthetic verifier fixture",
                    "evidence": evidence
                })
            })
            .collect::<Vec<_>>();

        let mut provenance = passing_provenance();
        provenance["fixture"]["case_sha256"] = json!(case_sha256);
        provenance["fixture"]["packages"] = json!({
            "current": source_tree_sha256,
            "replacement_submission": source_tree_sha256,
            "new_submission": source_tree_sha256
        });
        let core_report_bytes = json_bytes(&json!({
            "format_version": INNER_FORMAT_VERSION,
            "metadata": {
                "evidence_scope": GENERIC_SCOPE,
                "representative_gate_eligible": false,
                "case_id": "representative-case",
                "case_sha256": case_sha256,
                "target_spine_version": TARGET_SPINE_VERSION,
                "expected_executable_sha256": EDITOR_SHA,
                "tool_version": env!("CARGO_PKG_VERSION"),
                "provenance": provenance
            },
            "passed": true,
            "assertions": assertions,
            "processes": processes,
            "artifacts": artifacts,
            "integrity_failures": [],
            "semantic_differences": [],
            "roundtrip_losses": []
        }));
        write_private_file(
            &temporary.path().join("core/report.json"),
            &core_report_bytes,
        );

        let observed = observe_evidence(temporary.path()).expect("observe pre-outer tree");
        let core_inventory = observed_core_inventory(&observed).expect("core inventory");
        let core_content_tree_sha256 = core_content_tree_sha256(&core_inventory.tree_sha256);
        let core_report_sha256 = sha256(&core_report_bytes);
        let outer_report_bytes = json_bytes(&json!({
            "format_version": OUTER_FORMAT_VERSION,
            "metadata": {
                "evidence_scope": REPRESENTATIVE_SCOPE,
                "representative_gate_eligible": true,
                "binding_id": binding_id,
                "representative_binding_sha256": binding_sha256,
                "case_sha256": case_sha256,
                "harness_executable_sha256": HARNESS_SHA,
                "expected_editor_executable_sha256": EDITOR_SHA,
                "package_tree_sha256": {
                    "current": source_tree_sha256,
                    "replacement_submission": source_tree_sha256,
                    "new_submission": source_tree_sha256
                },
                "source_revision": SOURCE_REVISION,
                "cargo_lock_sha256": CARGO_LOCK_SHA,
                "target_spine_version": TARGET_SPINE_VERSION,
                "tool_version": env!("CARGO_PKG_VERSION")
            },
            "passed": true,
            "core": {
                "outcome": "passed",
                "report_path": "core/report.json",
                "report_sha256": core_report_sha256,
                "inventory": core_inventory,
                "content_tree_sha256": core_content_tree_sha256
            },
            "top_level_artifacts": [
                {
                    "path": "representative-binding.toml",
                    "kind": "file",
                    "sha256": binding_sha256,
                    "byte_length": binding_bytes.len()
                },
                {
                    "path": "core",
                    "kind": "directory",
                    "sha256": core_content_tree_sha256,
                    "entry_count": core_inventory.entries.len()
                }
            ],
            "validation": {
                "core_schema_validated": true,
                "clean_build_provenance_validated": true,
                "harness_identity_validated": true,
                "editor_launcher_validated": true,
                "package_bindings_validated": true,
                "workspace_source_bindings_validated": true,
                "marker_value_sha256": marker_value_sha256,
                "marker_processes_validated": EXPECTED_OPERATIONS.len(),
                "marker_evidence_complete": true,
                "process_count": EXPECTED_OPERATIONS.len(),
                "assertion_count": EXPECTED_ASSERTIONS.len(),
                "passed_assertion_count": EXPECTED_ASSERTIONS.len(),
                "integrity_failure_count": 0
            }
        }));
        write_private_file(&temporary.path().join("report.json"), &outer_report_bytes);

        let canonical = fs::canonicalize(temporary.path()).expect("canonical evidence root");
        let verification =
            verify_representative_evidence(&canonical).expect("complete bundle verifies");
        assert!(verification.passed());
        assert!(verification.representative_gate_eligible());
        assert_eq!(
            verification.outer_report_sha256(),
            sha256(&outer_report_bytes)
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn json_bytes(value: &Value) -> Vec<u8> {
        let mut bytes = serde_json::to_vec_pretty(value).expect("serialize test JSON");
        bytes.push(b'\n');
        bytes
    }

    #[test]
    fn relabelled_v4_core_is_rejected() {
        let metadata = json!({
            "evidence_scope": REPRESENTATIVE_SCOPE,
            "representative_gate_eligible": false,
            "case_id": "case",
            "case_sha256": CASE_SHA,
            "target_spine_version": TARGET_SPINE_VERSION,
            "expected_executable_sha256": EDITOR_SHA,
            "tool_version": env!("CARGO_PKG_VERSION"),
            "provenance": passing_provenance()
        });
        let error = verify_core_metadata(
            metadata.as_object().expect("metadata object"),
            &outer(),
            &binding(),
        )
        .expect_err("relabelled generic core must fail");
        assert!(error.to_string().contains("evidence scope"));
    }

    #[test]
    fn duplicate_json_keys_are_rejected_at_every_depth() {
        for bytes in [
            br#"{"format_version": 5, "format_version": 4}"#.as_slice(),
            br#"{"metadata": {"passed": true, "passed": false}}"#.as_slice(),
        ] {
            let error = parse_json::<Value>("core/report.json", bytes)
                .expect_err("duplicate object key must fail");
            assert!(error.to_string().contains("duplicate JSON object key"));
        }
    }

    #[test]
    fn embedded_case_uses_the_full_strict_v2_schema_and_policy() {
        let valid = valid_case_toml();
        let mut expected_binding = binding();
        expected_binding.case_sha256 = sha256(valid.as_bytes());
        let core = json!({"metadata": {"case_id": "representative-case"}});
        verify_case_binding(valid.as_bytes(), &core, &outer(), &expected_binding)
            .expect("strict valid case");

        for invalid in [
            format!("{valid}\nunknown = true\n"),
            valid.replace(
                "approved_json_pointers = [\"/skeleton/hash\"]",
                "approved_json_pointers = []",
            ),
        ] {
            expected_binding.case_sha256 = sha256(invalid.as_bytes());
            assert!(
                verify_case_binding(invalid.as_bytes(), &core, &outer(), &expected_binding,)
                    .is_err()
            );
        }
    }

    #[test]
    fn missing_and_wrong_process_markers_are_rejected() {
        let expected = sha256(CASE_SHA.as_bytes());
        let missing = json!([{"name": "HOME", "value_sha256": CASE_SHA}]);
        assert!(verify_marker(&missing, &expected).is_err());
        let wrong = json!([{"name": MARKER_NAME, "value_sha256": CASE_SHA}]);
        assert!(verify_marker(&wrong, &expected).is_err());
    }

    #[test]
    fn role_swaps_are_rejected_even_when_all_digests_are_valid() {
        let swapped = RoleDigestsDto {
            current: CURRENT_SHA.to_owned(),
            replacement_submission: NEW_SHA.to_owned(),
            new_submission: REPLACEMENT_SHA.to_owned(),
        };
        assert!(require_roles_equal("roles", &swapped, &roles()).is_err());
    }

    #[test]
    fn source_inventory_budget_does_not_inherit_the_small_evidence_layout_limit() {
        assert!(
            validate_inventory_entry_count(257, MAX_PACKAGE_INVENTORY_ENTRIES, "source inventory",)
                .is_ok()
        );
        assert!(validate_inventory_entry_count(
            257,
            MAX_PHYSICAL_EVIDENCE_ENTRIES,
            "physical evidence",
        )
        .is_err());
        assert!(
            validate_inventory_entry_count(
                MAX_PACKAGE_INVENTORY_ENTRIES,
                MAX_PACKAGE_INVENTORY_ENTRIES,
                "source inventory",
            )
            .is_ok()
        );
        assert!(
            validate_inventory_entry_count(
                MAX_PACKAGE_INVENTORY_ENTRIES + 1,
                MAX_PACKAGE_INVENTORY_ENTRIES,
                "source inventory",
            )
            .is_err()
        );
    }

    #[test]
    fn dirty_or_wrong_harness_provenance_is_rejected() {
        let mut dirty = passing_provenance();
        dirty["environment"]["build_context"]["checkout"]["dirty"] = json!(true);
        assert!(verify_provenance(&dirty, &outer(), &binding()).is_err());

        let mut wrong_harness = passing_provenance();
        wrong_harness["environment"]["harness_executable"]["sha256"] = json!(CASE_SHA);
        assert!(verify_provenance(&wrong_harness, &outer(), &binding()).is_err());

        let mut wrong_relationship = passing_provenance();
        wrong_relationship["environment"]["build_context"]["relationship"] =
            json!("binary_attestation");
        assert!(verify_provenance(&wrong_relationship, &outer(), &binding()).is_err());

        let mut extra_build_field = passing_provenance();
        extra_build_field["environment"]["build_context"]["unexpected"] = json!(true);
        assert!(verify_provenance(&extra_build_field, &outer(), &binding()).is_err());

        let mut wrong_export_preset = passing_provenance();
        wrong_export_preset["fixture"]["export_preset"]["sha256"] = json!(CASE_SHA);
        assert!(verify_provenance(&wrong_export_preset, &outer(), &binding()).is_err());

        let mut extra_fixture_field = passing_provenance();
        extra_fixture_field["fixture"]["unexpected"] = json!(true);
        assert!(verify_provenance(&extra_fixture_field, &outer(), &binding()).is_err());

        let mut extra_launcher_field = passing_provenance();
        extra_launcher_field["spine_launcher"]["unexpected"] = json!(true);
        assert!(verify_provenance(&extra_launcher_field, &outer(), &binding()).is_err());
    }

    #[test]
    fn nonpassing_or_ineligible_outer_reports_are_unpublished_inputs() {
        let mut report = outer();
        report.passed = false;
        assert!(verify_outer_static(&report).is_err());

        let mut report = outer();
        report.metadata.representative_gate_eligible = false;
        assert!(verify_outer_static(&report).is_err());

        let mut report = outer();
        report.core.outcome = "controlled_failure".to_owned();
        assert!(verify_outer_static(&report).is_err());
    }

    #[test]
    fn artifact_byte_tampering_is_rejected() {
        let observed = ObservedEvidence {
            root_state: test_directory_state(),
            files: BTreeMap::from([(
                "core/case.toml".to_owned(),
                ObservedFile {
                    bytes: b"tampered".to_vec(),
                    sha256: sha256(b"tampered"),
                },
            )]),
            directories: BTreeSet::from([".".to_owned(), "core".to_owned()]),
        };
        let artifact = ArtifactDto {
            role: "case-manifest".to_owned(),
            path: "case.toml".to_owned(),
            sha256: CASE_SHA.to_owned(),
        };
        assert!(verify_core_artifacts(&[artifact], &observed).is_err());
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn extra_entries_symlinks_and_hardlinks_fail_closed() {
        use std::os::unix::fs::{PermissionsExt as _, symlink};

        let extra = tempfile::tempdir().expect("temporary root");
        make_private_directory(extra.path());
        let core = extra.path().join("core");
        fs::create_dir(&core).expect("core");
        make_private_directory(&core);
        for relative in [
            "report.json",
            "representative-binding.toml",
            "core/report.json",
            "extra",
        ] {
            write_private_file(&extra.path().join(relative), b"bytes");
        }
        let observed = observe_evidence(extra.path()).expect("safe physical tree");
        assert!(
            verify_exact_layout(&observed, &BTreeSet::from(["core/report.json".to_owned()]),)
                .is_err()
        );

        let symlink_root = tempfile::tempdir().expect("symlink root");
        make_private_directory(symlink_root.path());
        symlink("missing", symlink_root.path().join("link")).expect("symlink");
        assert!(observe_evidence(symlink_root.path()).is_err());

        let hardlink_root = tempfile::tempdir().expect("hardlink root");
        make_private_directory(hardlink_root.path());
        let first = hardlink_root.path().join("first");
        write_private_file(&first, b"same inode");
        fs::hard_link(&first, hardlink_root.path().join("second")).expect("hard link");
        fs::set_permissions(&first, fs::Permissions::from_mode(PRIVATE_FILE_MODE))
            .expect("private mode");
        assert!(observe_evidence(hardlink_root.path()).is_err());
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn make_private_directory(path: &Path) {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(PRIVATE_DIRECTORY_MODE))
            .expect("private directory mode");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn write_private_file(path: &Path, bytes: &[u8]) {
        use std::os::unix::fs::PermissionsExt as _;
        fs::write(path, bytes).expect("write test file");
        fs::set_permissions(path, fs::Permissions::from_mode(PRIVATE_FILE_MODE))
            .expect("private file mode");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn test_directory_state() -> DirectoryState {
        DirectoryState {
            device: 1,
            inode: 1,
            mode: PRIVATE_DIRECTORY_MODE,
            owner: 1,
            group: 1,
            modified_seconds: 0,
            modified_nanoseconds: 0,
            changed_seconds: 0,
            changed_nanoseconds: 0,
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn test_directory_state() -> DirectoryState {
        DirectoryState
    }
}

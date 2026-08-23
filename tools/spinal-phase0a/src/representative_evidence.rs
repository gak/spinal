//! Format-v5 representative evidence composed around an unchanged format-v4 core.
//!
//! The generic runner remains the only component that executes Spine and its
//! report remains permanently labeled `generic_rehearsal` and ineligible. This
//! module verifies that immutable core, binds it to an owner-private
//! representative envelope, and prepares a separate outer report. It never
//! edits or relabels the inner report.

use crate::case::LoadedCase;
use crate::digest::{is_sha256, sha256_bytes};
use crate::package::{EntryKind, PackageInventory, TreeEntry};
use crate::phase0a_runner::PublishedGenericRehearsal;
use crate::representative::{
    OwnerPrivateExactFile, RepresentativeEnvelopeError, VerifiedRepresentativeEnvelope,
};
use crate::stage::{StageError, secure_inventory_tree};
use serde::de::{Error as _, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use thiserror::Error;

const OUTER_FORMAT_VERSION: u32 = 5;
const INNER_FORMAT_VERSION: u64 = 4;
const TARGET_SPINE_VERSION: &str = "4.3.23";
const REPRESENTATIVE_SCOPE: &str = "representative_phase0a";
const GENERIC_SCOPE: &str = "generic_rehearsal";
const BINDING_PATH: &str = "representative-binding.toml";
const CORE_PATH: &str = "core";
const CORE_REPORT_PATH: &str = "core/report.json";
const CORE_TREE_DIGEST_DOMAIN: &[u8] = b"spinal-phase0a-representative-core-tree-v1\0";
const REPRESENTATIVE_BINDING_ENVIRONMENT_NAME: &str =
    "SPINAL_PHASE0A_REPRESENTATIVE_BINDING_SHA256";
const REQUIRED_PROCESS_COUNT: usize = 22;
const REQUIRED_ASSERTION_COUNT: usize = 25;
const MAX_INNER_JSON_BYTES: u64 = 64 * 1024 * 1024;
const MAX_OUTER_REPORT_BYTES: usize = 64 * 1024 * 1024;

const REQUIRED_ASSERTION_IDS: [&str; REQUIRED_ASSERTION_COUNT] = [
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

/// A fully preflighted outer report and the immutable inputs its orchestrator
/// must reobserve before publishing `report.json` last.
///
/// The representative runner creates the generic core directly beneath one
/// fresh owner-private destination, so no recursive evidence copy is needed.
/// This preparer remains separate from publication to keep report construction
/// pure and make the final create-only boundary explicit.
pub(crate) struct PreparedRepresentativeEvidence {
    report_bytes: Vec<u8>,
    report_sha256: String,
    binding_bytes: Vec<u8>,
    core_source: PathBuf,
    core_inventory: PackageInventory,
}

impl PreparedRepresentativeEvidence {
    pub(crate) fn report_bytes(&self) -> &[u8] {
        &self.report_bytes
    }

    pub(crate) fn report_sha256(&self) -> &str {
        &self.report_sha256
    }

    pub(crate) fn binding_bytes(&self) -> &[u8] {
        &self.binding_bytes
    }

    pub(crate) fn core_source(&self) -> &Path {
        &self.core_source
    }

    pub(crate) fn core_inventory(&self) -> &PackageInventory {
        &self.core_inventory
    }

    #[cfg(test)]
    pub(crate) fn test_only(
        report_bytes: Vec<u8>,
        binding_bytes: Vec<u8>,
        core_source: PathBuf,
        core_inventory: PackageInventory,
    ) -> Self {
        let report_sha256 = sha256_bytes(&report_bytes);
        Self {
            report_bytes,
            report_sha256,
            binding_bytes,
            core_source,
            core_inventory,
        }
    }
}

/// Fail-closed preparation errors. No error path produces outer evidence.
#[derive(Debug, Error)]
pub(crate) enum RepresentativeEvidenceError {
    #[error(transparent)]
    Admission(#[from] RepresentativeEnvelopeError),
    #[error(transparent)]
    Inventory(#[from] StageError),
    #[error("could not read representative core file `{path}`: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("representative core file `{path}` exceeds the fixed byte budget")]
    CoreFileTooLarge { path: PathBuf },
    #[error("representative core JSON `{path}` is invalid: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("representative cross-binding failed: {0}")]
    CrossBinding(String),
    #[error("could not serialize the representative outer report: {0}")]
    Serialization(serde_json::Error),
    #[error("representative outer report exceeds the fixed byte budget")]
    OuterReportTooLarge,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct RepresentativeReport {
    format_version: u32,
    metadata: RepresentativeMetadata,
    passed: bool,
    core: RepresentativeCore,
    top_level_artifacts: Vec<TopLevelArtifact>,
    validation: RepresentativeValidation,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct RepresentativeMetadata {
    evidence_scope: &'static str,
    representative_gate_eligible: bool,
    binding_id: String,
    representative_binding_sha256: String,
    case_sha256: String,
    harness_executable_sha256: String,
    expected_editor_executable_sha256: String,
    source_revision: String,
    cargo_lock_sha256: String,
    package_tree_sha256: RepresentativePackageHashes,
    target_spine_version: &'static str,
    tool_version: &'static str,
}

#[derive(Clone, Serialize)]
#[serde(deny_unknown_fields)]
struct RepresentativePackageHashes {
    current: String,
    replacement_submission: String,
    new_submission: String,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct RepresentativeCore {
    outcome: CoreOutcome,
    report_path: &'static str,
    report_sha256: String,
    content_tree_sha256: String,
    inventory: PackageInventory,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum CoreOutcome {
    Passed,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum TopLevelArtifact {
    File {
        path: &'static str,
        sha256: String,
        byte_length: u64,
    },
    Directory {
        path: &'static str,
        sha256: String,
        entry_count: usize,
    },
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct RepresentativeValidation {
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

struct ExpectedBindings<'a> {
    case_id: &'a str,
    case_sha256: &'a str,
    binding_sha256: &'a str,
    harness_sha256: &'a str,
    expected_editor_sha256: &'a str,
    source_revision: &'a str,
    cargo_lock_sha256: &'a str,
    packages: RepresentativePackageHashes,
}

/// Verifies an immutable generic-v4 core and prepares a distinct representative-v5 report.
///
/// A passing inner report is eligible only after every strict check succeeds.
/// A failed inner rehearsal remains generic-v4 diagnostics and is never wrapped
/// or published as representative-v5 evidence.
pub(crate) fn prepare_representative_evidence(
    binding: &VerifiedRepresentativeEnvelope,
    loaded_case: &LoadedCase,
    admitted_case: &OwnerPrivateExactFile,
    published_core: &PublishedGenericRehearsal,
    core_inventory: PackageInventory,
) -> Result<PreparedRepresentativeEvidence, RepresentativeEvidenceError> {
    binding.reobserve()?;
    admitted_case.reobserve()?;

    if !published_core.passed() || published_core.failure_code().is_some() {
        return cross(
            "only a successful generic-v4 core can be composed into representative-v5 evidence",
        );
    }

    if loaded_case.source_bytes() != admitted_case.source_bytes() {
        return cross("loaded case bytes do not match the admitted private case");
    }
    require_equal(
        "admitted case digest",
        loaded_case.source_sha256(),
        admitted_case.source_sha256(),
    )?;
    require_equal(
        "binding case digest",
        binding.case_sha256(),
        loaded_case.source_sha256(),
    )?;
    require_equal(
        "published core case digest",
        published_core.case_sha256(),
        loaded_case.source_sha256(),
    )?;

    let observed_before = secure_inventory_tree(published_core.destination())?;
    require_inventory_equal(&core_inventory, &observed_before)?;

    let report_path = published_core.destination().join("report.json");
    let report_bytes = read_limited(&report_path, MAX_INNER_JSON_BYTES)?;
    require_inventory_file(&core_inventory, "report.json", &report_bytes)?;
    let report_sha256 = sha256_bytes(&report_bytes);
    require_equal(
        "published core report digest",
        published_core.report_sha256(),
        &report_sha256,
    )?;
    let report = parse_json(&report_path, &report_bytes)?;

    let case_relative_path = "case.toml";
    let core_case_path = published_core.destination().join(case_relative_path);
    let core_case_bytes = read_limited(&core_case_path, MAX_INNER_JSON_BYTES)?;
    require_inventory_file(&core_inventory, case_relative_path, &core_case_bytes)?;
    if core_case_bytes != admitted_case.source_bytes() {
        return cross("core case bytes do not match the admitted private case");
    }
    require_report_artifact(
        &report,
        case_relative_path,
        &core_case_bytes,
        "case-manifest",
    )?;

    let package_hashes = RepresentativePackageHashes {
        current: binding.current_package_tree_sha256().to_owned(),
        replacement_submission: binding
            .replacement_submission_package_tree_sha256()
            .to_owned(),
        new_submission: binding.new_submission_package_tree_sha256().to_owned(),
    };
    let expected = ExpectedBindings {
        case_id: &loaded_case.manifest().case_id,
        case_sha256: loaded_case.source_sha256(),
        binding_sha256: binding.source_sha256(),
        harness_sha256: binding.harness_executable_sha256(),
        expected_editor_sha256: &loaded_case.manifest().editor.expected_executable_sha256,
        source_revision: binding.source_revision(),
        cargo_lock_sha256: binding.cargo_lock_sha256(),
        packages: package_hashes.clone(),
    };

    let package_path = published_core
        .destination()
        .join("package-inventories.json");
    let package_bytes = read_limited(&package_path, MAX_INNER_JSON_BYTES)?;
    require_inventory_file(&core_inventory, "package-inventories.json", &package_bytes)?;
    require_report_artifact(
        &report,
        "package-inventories.json",
        &package_bytes,
        "package-inventories",
    )?;
    let package_artifact = parse_json(&package_path, &package_bytes)?;

    let validation = validate_core_value(&report, &package_artifact, &expected)?;
    if !bool_at(&report, "/passed")? {
        return cross("representative-v5 composition requires a passing inner report");
    }

    let observed_after = secure_inventory_tree(published_core.destination())?;
    require_inventory_equal(&core_inventory, &observed_after)?;
    binding.reobserve()?;
    admitted_case.reobserve()?;

    let core_tree_sha256 = representative_core_tree_sha256(&core_inventory.tree_sha256)?;
    let binding_byte_length = u64::try_from(binding.source_bytes().len())
        .map_err(|_| RepresentativeEvidenceError::OuterReportTooLarge)?;
    let report = RepresentativeReport {
        format_version: OUTER_FORMAT_VERSION,
        metadata: RepresentativeMetadata {
            evidence_scope: REPRESENTATIVE_SCOPE,
            representative_gate_eligible: true,
            binding_id: binding.binding_id().to_owned(),
            representative_binding_sha256: binding.source_sha256().to_owned(),
            case_sha256: loaded_case.source_sha256().to_owned(),
            harness_executable_sha256: binding.harness_executable_sha256().to_owned(),
            expected_editor_executable_sha256: loaded_case
                .manifest()
                .editor
                .expected_executable_sha256
                .clone(),
            source_revision: binding.source_revision().to_owned(),
            cargo_lock_sha256: binding.cargo_lock_sha256().to_owned(),
            package_tree_sha256: package_hashes,
            target_spine_version: TARGET_SPINE_VERSION,
            tool_version: env!("CARGO_PKG_VERSION"),
        },
        passed: true,
        core: RepresentativeCore {
            outcome: CoreOutcome::Passed,
            report_path: CORE_REPORT_PATH,
            report_sha256,
            content_tree_sha256: core_tree_sha256.clone(),
            inventory: core_inventory.clone(),
        },
        top_level_artifacts: vec![
            TopLevelArtifact::File {
                path: BINDING_PATH,
                sha256: binding.source_sha256().to_owned(),
                byte_length: binding_byte_length,
            },
            TopLevelArtifact::Directory {
                path: CORE_PATH,
                sha256: core_tree_sha256,
                entry_count: core_inventory.entries.len(),
            },
        ],
        validation,
    };
    let mut outer_bytes =
        serde_json::to_vec_pretty(&report).map_err(RepresentativeEvidenceError::Serialization)?;
    outer_bytes.push(b'\n');
    if outer_bytes.len() > MAX_OUTER_REPORT_BYTES {
        return Err(RepresentativeEvidenceError::OuterReportTooLarge);
    }
    let outer_sha256 = sha256_bytes(&outer_bytes);
    Ok(PreparedRepresentativeEvidence {
        report_bytes: outer_bytes,
        report_sha256: outer_sha256,
        binding_bytes: binding.source_bytes().to_vec(),
        core_source: published_core.destination().to_path_buf(),
        core_inventory,
    })
}

fn validate_core_value(
    report: &Value,
    package_artifact: &Value,
    expected: &ExpectedBindings<'_>,
) -> Result<RepresentativeValidation, RepresentativeEvidenceError> {
    if u64_at(report, "/format_version")? != INNER_FORMAT_VERSION {
        return cross("inner report format_version must be 4");
    }
    require_json_string(report, "/metadata/evidence_scope", GENERIC_SCOPE)?;
    if bool_at(report, "/metadata/representative_gate_eligible")? {
        return cross("inner generic report must remain representative-ineligible");
    }
    require_json_string(report, "/metadata/case_id", expected.case_id)?;
    require_json_string(report, "/metadata/case_sha256", expected.case_sha256)?;
    require_json_string(
        report,
        "/metadata/target_spine_version",
        TARGET_SPINE_VERSION,
    )?;
    require_json_string(report, "/metadata/tool_version", env!("CARGO_PKG_VERSION"))?;
    require_json_string(
        report,
        "/metadata/expected_executable_sha256",
        expected.expected_editor_sha256,
    )?;
    if !bool_at(report, "/passed")? {
        return cross("representative-v5 composition requires a passing inner report");
    }

    validate_build_and_harness(report, expected)?;
    validate_editor_launcher(report, expected.expected_editor_sha256)?;
    validate_provenance_packages(report, &expected.packages)?;

    let assertions = array_at(report, "/assertions")?;
    if assertions.len() != REQUIRED_ASSERTION_COUNT {
        return cross("inner report must contain the exact 25-assertion catalog");
    }
    let mut assertion_ids = BTreeSet::new();
    let mut passed_assertion_count = 0_usize;
    for assertion in assertions {
        let id = string_at(assertion, "/id")?;
        if !assertion_ids.insert(id) {
            return cross("inner report contains duplicate assertion identifiers");
        }
        if string_at(assertion, "/status")? == "passed" {
            passed_assertion_count = passed_assertion_count.saturating_add(1);
        }
    }
    let required_ids = REQUIRED_ASSERTION_IDS.into_iter().collect::<BTreeSet<_>>();
    if assertion_ids != required_ids {
        return cross("inner report assertion identifiers do not match the fixed catalog");
    }

    let processes = array_at(report, "/processes")?;
    let marker_value_sha256 = sha256_bytes(expected.binding_sha256.as_bytes());
    if processes.len() != REQUIRED_PROCESS_COUNT {
        return cross("passing inner report must contain exactly 22 processes");
    }
    for process in processes {
        validate_process_marker(process, &marker_value_sha256)?;
    }
    if passed_assertion_count != REQUIRED_ASSERTION_COUNT {
        return cross("passing inner report must contain exactly 25 passed assertions");
    }
    let integrity_failures = array_at(report, "/integrity_failures")?;
    if !integrity_failures.is_empty() {
        return cross("passing inner report must contain no integrity failures");
    }
    validate_workspace_sources(package_artifact, &expected.packages)?;

    Ok(RepresentativeValidation {
        core_schema_validated: true,
        clean_build_provenance_validated: true,
        harness_identity_validated: true,
        editor_launcher_validated: true,
        package_bindings_validated: true,
        workspace_source_bindings_validated: true,
        marker_value_sha256,
        marker_processes_validated: REQUIRED_PROCESS_COUNT,
        marker_evidence_complete: true,
        process_count: processes.len(),
        assertion_count: assertions.len(),
        passed_assertion_count,
        integrity_failure_count: Some(integrity_failures.len()),
    })
}

fn validate_build_and_harness(
    report: &Value,
    expected: &ExpectedBindings<'_>,
) -> Result<(), RepresentativeEvidenceError> {
    let prefix = "/metadata/provenance/environment";
    let build_context = value_at(report, &format!("{prefix}/build_context"))?;
    let harness = value_at(report, &format!("{prefix}/harness_executable"))?;
    if bool_at(build_context, "/checkout/dirty")? {
        return cross("representative core build provenance must be clean");
    }
    require_json_string(build_context, "/checkout/head", expected.source_revision)?;
    require_json_string(
        build_context,
        "/cargo_lock/sha256",
        expected.cargo_lock_sha256,
    )?;
    require_json_string(harness, "/sha256", expected.harness_sha256)?;
    Ok(())
}

fn validate_editor_launcher(
    report: &Value,
    expected_editor_sha256: &str,
) -> Result<(), RepresentativeEvidenceError> {
    let launcher = value_at(report, "/metadata/provenance/spine_launcher")?;
    validate_available_launcher(launcher, expected_editor_sha256)?;
    if usize_at(launcher, "/observed_processes")? != REQUIRED_PROCESS_COUNT {
        return cross("passing launcher provenance must bind all 22 processes");
    }
    Ok(())
}

fn validate_available_launcher(
    launcher: &Value,
    expected_editor_sha256: &str,
) -> Result<(), RepresentativeEvidenceError> {
    require_json_string(launcher, "/expected_sha256", expected_editor_sha256)?;
    require_json_string(launcher, "/observed/sha256", expected_editor_sha256)?;
    require_json_string(launcher, "/target_spine_version", TARGET_SPINE_VERSION)?;
    Ok(())
}

fn validate_provenance_packages(
    report: &Value,
    expected: &RepresentativePackageHashes,
) -> Result<(), RepresentativeEvidenceError> {
    let packages = value_at(report, "/metadata/provenance/fixture/packages")?;
    validate_role_hashes(packages, expected)
}

fn validate_workspace_sources(
    artifact: &Value,
    expected: &RepresentativePackageHashes,
) -> Result<(), RepresentativeEvidenceError> {
    if u64_at(artifact, "/format_version")? != 1 {
        return cross("package inventory artifact format_version must be 1");
    }
    require_json_string(artifact, "/evidence_scope", GENERIC_SCOPE)?;
    for (role, digest) in [
        ("current", expected.current.as_str()),
        (
            "replacement_submission",
            expected.replacement_submission.as_str(),
        ),
        ("new_submission", expected.new_submission.as_str()),
    ] {
        for observation in ["before_staging", "after_staging", "after_run"] {
            require_json_string(
                artifact,
                &format!("/workspace_boundary/sources/{role}/{observation}/tree_sha256"),
                digest,
            )?;
        }
    }
    Ok(())
}

fn validate_role_hashes(
    value: &Value,
    expected: &RepresentativePackageHashes,
) -> Result<(), RepresentativeEvidenceError> {
    require_json_string(value, "/current", &expected.current)?;
    require_json_string(
        value,
        "/replacement_submission",
        &expected.replacement_submission,
    )?;
    require_json_string(value, "/new_submission", &expected.new_submission)?;
    Ok(())
}

fn validate_process_marker(
    process: &Value,
    expected_value_sha256: &str,
) -> Result<(), RepresentativeEvidenceError> {
    let environment = array_at(process, "/evidence/environment")?;
    let markers = environment
        .iter()
        .filter(|entry| {
            entry.pointer("/name").and_then(Value::as_str)
                == Some(REPRESENTATIVE_BINDING_ENVIRONMENT_NAME)
        })
        .collect::<Vec<_>>();
    if markers.len() != 1 {
        return cross("each passing process must contain exactly one binding marker");
    }
    require_json_string(markers[0], "/value_sha256", expected_value_sha256)
}

fn require_report_artifact(
    report: &Value,
    path: &str,
    bytes: &[u8],
    role: &str,
) -> Result<(), RepresentativeEvidenceError> {
    let artifacts = array_at(report, "/artifacts")?;
    let matches = artifacts
        .iter()
        .filter(|artifact| artifact.pointer("/path").and_then(Value::as_str) == Some(path))
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return cross(format!(
            "inner report must cite exactly one `{path}` artifact"
        ));
    }
    let artifact = matches[0];
    require_json_string(artifact, "/role", role)?;
    require_json_string(artifact, "/sha256", &sha256_bytes(bytes))?;
    Ok(())
}

fn require_inventory_file(
    inventory: &PackageInventory,
    path: &str,
    bytes: &[u8],
) -> Result<(), RepresentativeEvidenceError> {
    let matches = inventory
        .entries
        .iter()
        .filter(|entry| entry.path == path)
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return cross(format!(
            "core inventory must contain exactly one `{path}` file"
        ));
    }
    let TreeEntry {
        kind, size, sha256, ..
    } = matches[0];
    let expected_size = u64::try_from(bytes.len()).map_err(|_| {
        RepresentativeEvidenceError::CrossBinding(
            "inventoried file byte length is not representable".to_owned(),
        )
    })?;
    if *kind != EntryKind::File
        || *size != expected_size
        || sha256.as_deref() != Some(sha256_bytes(bytes).as_str())
    {
        return cross(format!("core inventory identity for `{path}` is incorrect"));
    }
    Ok(())
}

fn require_inventory_equal(
    expected: &PackageInventory,
    observed: &PackageInventory,
) -> Result<(), RepresentativeEvidenceError> {
    if expected == observed {
        Ok(())
    } else {
        cross("representative core inventory changed or was not independently observed")
    }
}

fn representative_core_tree_sha256(
    inventory_tree_sha256: &str,
) -> Result<String, RepresentativeEvidenceError> {
    if !is_sha256(inventory_tree_sha256) {
        return cross("core package-tree digest is not a lowercase SHA-256");
    }
    let mut framed = Vec::with_capacity(CORE_TREE_DIGEST_DOMAIN.len() + 64);
    framed.extend_from_slice(CORE_TREE_DIGEST_DOMAIN);
    framed.extend_from_slice(inventory_tree_sha256.as_bytes());
    Ok(sha256_bytes(&framed))
}

fn read_limited(path: &Path, max_bytes: u64) -> Result<Vec<u8>, RepresentativeEvidenceError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|source| RepresentativeEvidenceError::Read {
            path: path.to_path_buf(),
            source,
        })?;
    if !metadata.file_type().is_file() {
        return cross(format!(
            "core path `{}` is not a regular file",
            path.display()
        ));
    }
    if metadata.len() > max_bytes {
        return Err(RepresentativeEvidenceError::CoreFileTooLarge {
            path: path.to_path_buf(),
        });
    }
    let mut file = File::open(path).map_err(|source| RepresentativeEvidenceError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).map_err(|_| {
        RepresentativeEvidenceError::CoreFileTooLarge {
            path: path.to_path_buf(),
        }
    })?);
    file.by_ref()
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|source| RepresentativeEvidenceError::Read {
            path: path.to_path_buf(),
            source,
        })?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > max_bytes {
        return Err(RepresentativeEvidenceError::CoreFileTooLarge {
            path: path.to_path_buf(),
        });
    }
    Ok(bytes)
}

fn parse_json(path: &Path, bytes: &[u8]) -> Result<Value, RepresentativeEvidenceError> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let StrictJsonValue(value) = StrictJsonValue::deserialize(&mut deserializer)
        .map_err(|source| json_error(path, source))?;
    deserializer
        .end()
        .map_err(|source| json_error(path, source))?;
    Ok(value)
}

fn json_error(path: &Path, source: serde_json::Error) -> RepresentativeEvidenceError {
    RepresentativeEvidenceError::Json {
        path: path.to_path_buf(),
        source,
    }
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
        formatter.write_str("JSON without duplicate object keys")
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
        E: serde::de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .map(StrictJsonValue)
            .ok_or_else(|| E::custom("JSON number is not finite"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::Null))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(StrictJsonValue(value)) = sequence.next_element()? {
            values.push(value);
        }
        Ok(StrictJsonValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut object: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = serde_json::Map::new();
        while let Some(key) = object.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(A::Error::custom(format!(
                    "duplicate JSON object key `{key}`"
                )));
            }
            let StrictJsonValue(value) = object.next_value()?;
            values.insert(key, value);
        }
        Ok(StrictJsonValue(Value::Object(values)))
    }
}

fn value_at<'a>(value: &'a Value, pointer: &str) -> Result<&'a Value, RepresentativeEvidenceError> {
    value
        .pointer(pointer)
        .ok_or_else(|| RepresentativeEvidenceError::CrossBinding(format!("missing `{pointer}`")))
}

fn string_at<'a>(value: &'a Value, pointer: &str) -> Result<&'a str, RepresentativeEvidenceError> {
    value_at(value, pointer)?.as_str().ok_or_else(|| {
        RepresentativeEvidenceError::CrossBinding(format!("`{pointer}` must be a string"))
    })
}

fn bool_at(value: &Value, pointer: &str) -> Result<bool, RepresentativeEvidenceError> {
    value_at(value, pointer)?.as_bool().ok_or_else(|| {
        RepresentativeEvidenceError::CrossBinding(format!("`{pointer}` must be a boolean"))
    })
}

fn u64_at(value: &Value, pointer: &str) -> Result<u64, RepresentativeEvidenceError> {
    value_at(value, pointer)?.as_u64().ok_or_else(|| {
        RepresentativeEvidenceError::CrossBinding(format!(
            "`{pointer}` must be an unsigned integer"
        ))
    })
}

fn usize_at(value: &Value, pointer: &str) -> Result<usize, RepresentativeEvidenceError> {
    usize::try_from(u64_at(value, pointer)?).map_err(|_| {
        RepresentativeEvidenceError::CrossBinding(format!("`{pointer}` does not fit usize"))
    })
}

fn array_at<'a>(
    value: &'a Value,
    pointer: &str,
) -> Result<&'a [Value], RepresentativeEvidenceError> {
    value_at(value, pointer)?
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| {
            RepresentativeEvidenceError::CrossBinding(format!("`{pointer}` must be an array"))
        })
}

fn require_json_string(
    value: &Value,
    pointer: &str,
    expected: &str,
) -> Result<(), RepresentativeEvidenceError> {
    if string_at(value, pointer)? == expected {
        Ok(())
    } else {
        cross(format!(
            "`{pointer}` does not match its representative binding"
        ))
    }
}

fn require_equal(
    label: &str,
    expected: &str,
    observed: &str,
) -> Result<(), RepresentativeEvidenceError> {
    if expected == observed {
        Ok(())
    } else {
        cross(format!("{label} mismatch"))
    }
}

fn cross<T>(message: impl Into<String>) -> Result<T, RepresentativeEvidenceError> {
    Err(RepresentativeEvidenceError::CrossBinding(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const SHA_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    const SHA_D: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
    const SHA_E: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    const SHA_F: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    const SHA_1: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const SHA_2: &str = "2222222222222222222222222222222222222222222222222222222222222222";

    fn expected() -> ExpectedBindings<'static> {
        ExpectedBindings {
            case_id: "representative-case",
            case_sha256: SHA_A,
            binding_sha256: SHA_B,
            harness_sha256: SHA_C,
            expected_editor_sha256: SHA_D,
            source_revision: SHA_E,
            cargo_lock_sha256: SHA_F,
            packages: RepresentativePackageHashes {
                current: SHA_1.to_owned(),
                replacement_submission: SHA_2.to_owned(),
                new_submission: SHA_A.to_owned(),
            },
        }
    }

    fn package_artifact() -> Value {
        let expected = expected();
        let source = |digest: &str| {
            json!({
                "before_staging": {"tree_sha256": digest},
                "after_staging": {"tree_sha256": digest},
                "after_run": {"tree_sha256": digest}
            })
        };
        json!({
            "format_version": 1,
            "evidence_scope": "generic_rehearsal",
            "workspace_boundary": {
                "sources": {
                    "current": source(&expected.packages.current),
                    "replacement_submission": source(&expected.packages.replacement_submission),
                    "new_submission": source(&expected.packages.new_submission)
                }
            }
        })
    }

    fn passing_report() -> Value {
        let expected = expected();
        let marker = sha256_bytes(expected.binding_sha256.as_bytes());
        let processes = (0..REQUIRED_PROCESS_COUNT)
            .map(|index| {
                json!({
                    "expectation": {"kind": "required_success"},
                    "evidence": {
                        "operation": format!("operation-{index}"),
                        "environment": [{
                            "name": REPRESENTATIVE_BINDING_ENVIRONMENT_NAME,
                            "value_sha256": marker.clone()
                        }]
                    }
                })
            })
            .collect::<Vec<_>>();
        let assertions = REQUIRED_ASSERTION_IDS
            .iter()
            .map(|id| json!({"id": id, "status": "passed"}))
            .collect::<Vec<_>>();
        json!({
            "format_version": 4,
            "metadata": {
                "evidence_scope": "generic_rehearsal",
                "representative_gate_eligible": false,
                "case_id": expected.case_id,
                "case_sha256": expected.case_sha256,
                "target_spine_version": "4.3.23",
                "expected_executable_sha256": expected.expected_editor_sha256,
                "tool_version": env!("CARGO_PKG_VERSION"),
                "provenance": {
                    "environment": {
                        "build_context": {
                            "checkout": {"head": expected.source_revision, "dirty": false},
                            "cargo_lock": {"sha256": expected.cargo_lock_sha256}
                        },
                        "harness_executable": {"sha256": expected.harness_sha256}
                    },
                    "fixture": {
                        "packages": {
                            "current": expected.packages.current,
                            "replacement_submission": expected.packages.replacement_submission,
                            "new_submission": expected.packages.new_submission
                        }
                    },
                    "spine_launcher": {
                        "expected_sha256": expected.expected_editor_sha256,
                        "observed": {"sha256": expected.expected_editor_sha256},
                        "target_spine_version": "4.3.23",
                        "observed_processes": REQUIRED_PROCESS_COUNT
                    }
                }
            },
            "passed": true,
            "assertions": assertions,
            "processes": processes,
            "integrity_failures": []
        })
    }

    fn assert_cross_binding(error: RepresentativeEvidenceError) {
        assert!(matches!(
            error,
            RepresentativeEvidenceError::CrossBinding(_)
        ));
    }

    #[test]
    fn synthetic_passing_core_satisfies_every_gate_check() {
        let validation = validate_core_value(&passing_report(), &package_artifact(), &expected())
            .expect("valid representative core");

        assert!(validation.marker_evidence_complete);
        assert_eq!(validation.marker_processes_validated, 22);
        assert_eq!(validation.passed_assertion_count, 25);
        assert!(validation.workspace_source_bindings_validated);
    }

    #[test]
    fn generic_core_cannot_be_relabelled_representative() {
        let mut report = passing_report();
        report["metadata"]["evidence_scope"] = json!("representative_phase0a");

        assert_cross_binding(
            validate_core_value(&report, &package_artifact(), &expected())
                .expect_err("relabelled core"),
        );
    }

    #[test]
    fn every_passing_process_requires_the_exact_hashed_marker() {
        let mut report = passing_report();
        report["processes"][7]["evidence"]["environment"][0]["value_sha256"] = json!(SHA_A);

        assert_cross_binding(
            validate_core_value(&report, &package_artifact(), &expected())
                .expect_err("wrong marker"),
        );
    }

    #[test]
    fn role_swapped_or_changed_package_hashes_fail_closed() {
        let mut artifact = package_artifact();
        artifact["workspace_boundary"]["sources"]["current"]["after_run"]["tree_sha256"] =
            json!(SHA_2);

        assert_cross_binding(
            validate_core_value(&passing_report(), &artifact, &expected())
                .expect_err("changed package role"),
        );
    }

    #[test]
    fn representative_harness_must_match_the_binding() {
        let mut report = passing_report();
        report["metadata"]["provenance"]["environment"]["harness_executable"]["sha256"] =
            json!(SHA_A);

        assert_cross_binding(
            validate_core_value(&report, &package_artifact(), &expected())
                .expect_err("wrong harness"),
        );
    }

    #[test]
    fn dirty_build_context_can_never_mint_representative_evidence() {
        let mut report = passing_report();
        report["metadata"]["provenance"]["environment"]["build_context"]["checkout"]["dirty"] =
            json!(true);

        assert_cross_binding(
            validate_core_value(&report, &package_artifact(), &expected())
                .expect_err("dirty build"),
        );
    }

    #[test]
    fn core_tree_digest_uses_a_distinct_outer_domain() {
        let digest = representative_core_tree_sha256(SHA_A).expect("domain digest");
        assert!(is_sha256(&digest));
        assert_ne!(digest, SHA_A);
        assert_eq!(digest, representative_core_tree_sha256(SHA_A).unwrap());
    }

    #[test]
    fn duplicate_json_keys_are_rejected_at_every_depth() {
        let path = Path::new("core/report.json");
        for bytes in [
            br#"{"passed":true,"passed":false}"#.as_slice(),
            br#"{"metadata":{"case_id":"first","case_id":"second"}}"#.as_slice(),
        ] {
            assert!(matches!(
                parse_json(path, bytes),
                Err(RepresentativeEvidenceError::Json { .. })
            ));
        }
    }
}

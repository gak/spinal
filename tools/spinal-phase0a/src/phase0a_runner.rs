//! One closed, generic-only Phase 0A rehearsal orchestrator.

use crate::case::{CaseError, LoadedCase, load_case};
use crate::evidence_writer::{
    EvidenceWriterError, PreparedControlledFailureEvidenceBundle, PreparedEvidenceBundle,
    write_prepared_controlled_failure_evidence_bundle, write_prepared_evidence_bundle,
};
use crate::lock::LockedProcessExecutor;
use crate::operation_recipe::OperationId;
use crate::phase0_analysis::{Phase0AnalysisError, analyze_phase0};
use crate::process::{ProcessEvidence, ProcessExecutionError, ProcessExecutor};
use crate::provenance::{ProvenanceSession, controlled_phase0a_provenance};
use crate::report::{
    ControlledFailureCode, ControlledFailureInputs, ControlledFailureProofs, Phase0aReportInputs,
    ReportAssemblyError, prepare_controlled_failure_evidence, prepare_phase0a_evidence,
};
use crate::run_workspace::{CompletedWorkspaceRunParts, RunWorkspaceError, WorkspacePreparation};
use crate::runtime_validations::{RuntimeValidationsError, complete_runtime_validations};
use crate::subprocess::{SubprocessExecutor, inspect_executable_identity};
use std::collections::BTreeMap;
use std::env;
use std::error::Error as StdError;
use std::fmt;
use std::fs::{self, File};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

const EDITOR_LOCK_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Paths required by the one production generic-rehearsal entry point.
///
/// There is intentionally no scope flag: this request can never mint
/// representative downstream-project gate evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenericRehearsalRequest {
    case_path: PathBuf,
    editor_executable: PathBuf,
    workspace_root: PathBuf,
    editor_lock: PathBuf,
    evidence_destination: PathBuf,
}

impl GenericRehearsalRequest {
    /// Creates a request. Every path is checked again before any editor launch.
    pub fn new(
        case_path: impl Into<PathBuf>,
        editor_executable: impl Into<PathBuf>,
        workspace_root: impl Into<PathBuf>,
        editor_lock: impl Into<PathBuf>,
        evidence_destination: impl Into<PathBuf>,
    ) -> Self {
        Self {
            case_path: case_path.into(),
            editor_executable: editor_executable.into(),
            workspace_root: workspace_root.into(),
            editor_lock: editor_lock.into(),
            evidence_destination: evidence_destination.into(),
        }
    }

    /// Returns the exact case TOML path.
    pub fn case_path(&self) -> &Path {
        &self.case_path
    }

    /// Returns the selected editor executable path.
    pub fn editor_executable(&self) -> &Path {
        &self.editor_executable
    }

    /// Returns the fresh private workspace destination.
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Returns the persistent cross-process editor lock path.
    pub fn editor_lock(&self) -> &Path {
        &self.editor_lock
    }

    /// Returns the fresh private evidence destination.
    pub fn evidence_destination(&self) -> &Path {
        &self.evidence_destination
    }
}

struct PreparedSuccessfulRehearsal {
    case_id: String,
    case_sha256: String,
    workspace_root: PathBuf,
    prepared: PreparedEvidenceBundle,
}

/// Identity and outcome of one published generic-only rehearsal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishedGenericRehearsal {
    case_id: String,
    case_sha256: String,
    destination: PathBuf,
    report_sha256: String,
    workspace_root: Option<PathBuf>,
    passed: bool,
    failure_code: Option<Phase0aRunErrorCode>,
}

impl PublishedGenericRehearsal {
    /// Returns the generic case identifier.
    pub fn case_id(&self) -> &str {
        &self.case_id
    }

    /// Returns the exact validated case TOML digest.
    pub fn case_sha256(&self) -> &str {
        &self.case_sha256
    }

    /// Returns the new evidence directory containing `report.json`.
    pub fn destination(&self) -> &Path {
        &self.destination
    }

    /// Returns the digest of the published report bytes.
    pub fn report_sha256(&self) -> &str {
        &self.report_sha256
    }

    /// Returns the retained workspace when preparation created one.
    pub fn workspace_root(&self) -> Option<&Path> {
        self.workspace_root.as_deref()
    }

    /// Returns true only for the completion-token-derived passing report.
    pub const fn passed(&self) -> bool {
        self.passed
    }

    /// Returns the stable controlled-failure category for a published failure.
    pub const fn failure_code(&self) -> Option<Phase0aRunErrorCode> {
        self.failure_code
    }
}

/// Stable category for a generic-rehearsal failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Phase0aRunErrorCode {
    /// A caller-supplied path violated the absolute normalized-path contract.
    InvalidPath,
    /// The selected executable did not match the case-pinned editor bytes.
    EditorIdentity,
    /// The process environment could not be reduced to the fixed safe subset.
    EditorEnvironment,
    /// An evidence destination overlapped an immutable input or private workspace.
    EvidenceDestinationOverlap,
    /// A mutable run path overlapped an immutable input or another run boundary.
    RunPathOverlap,
    /// A case manifest was unreadable or invalid.
    Case,
    /// The fixed subprocess adapter could not establish trustworthy evidence.
    Process,
    /// The sealed run workspace or its closed operation recipe failed.
    Workspace,
    /// Semantic JSON analysis failed closed.
    Analysis,
    /// Shared native/runtime-bundle validation failed closed.
    RuntimeValidation,
    /// The proof-derived report could not be assembled.
    ReportAssembly,
    /// Mandatory harness, build, host, fixture, or launcher provenance failed closed.
    Provenance,
    /// The preflighted evidence bundle could not be published atomically.
    EvidencePublication,
    /// A required filesystem identity operation failed.
    Filesystem,
}

/// Opaque fail-closed failure from request validation through publication.
///
/// Internal proof-token types deliberately remain private. Callers receive a
/// stable category, a diagnostic, and the original error chain where one
/// exists, without gaining a way to manufacture proof-stage values.
#[derive(Debug)]
pub struct Phase0aRunError {
    code: Phase0aRunErrorCode,
    detail: String,
    source: Option<Box<dyn StdError + Send + Sync>>,
}

impl Phase0aRunError {
    /// Returns the stable failure category.
    pub const fn code(&self) -> Phase0aRunErrorCode {
        self.code
    }

    /// Returns the human-readable failure diagnostic.
    pub fn detail(&self) -> &str {
        &self.detail
    }

    fn policy(code: Phase0aRunErrorCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
            source: None,
        }
    }

    fn caused<E>(code: Phase0aRunErrorCode, context: impl Into<String>, source: E) -> Self
    where
        E: StdError + Send + Sync + 'static,
    {
        let context = context.into();
        Self {
            code,
            detail: format!("{context}: {source}"),
            source: Some(Box::new(source)),
        }
    }
}

impl fmt::Display for Phase0aRunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl StdError for Phase0aRunError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn StdError + 'static))
    }
}

macro_rules! phase0a_source_conversion {
    ($source:ty, $code:expr, $context:literal) => {
        impl From<$source> for Phase0aRunError {
            fn from(source: $source) -> Self {
                Self::caused($code, $context, source)
            }
        }
    };
}

phase0a_source_conversion!(
    CaseError,
    Phase0aRunErrorCode::Case,
    "case validation failed"
);
phase0a_source_conversion!(
    ProcessExecutionError,
    Phase0aRunErrorCode::Process,
    "editor executable preflight failed"
);
phase0a_source_conversion!(
    RunWorkspaceError,
    Phase0aRunErrorCode::Workspace,
    "closed rehearsal workspace failed"
);
phase0a_source_conversion!(
    Phase0AnalysisError,
    Phase0aRunErrorCode::Analysis,
    "semantic analysis failed"
);
phase0a_source_conversion!(
    RuntimeValidationsError,
    Phase0aRunErrorCode::RuntimeValidation,
    "runtime validation failed"
);
phase0a_source_conversion!(
    ReportAssemblyError,
    Phase0aRunErrorCode::ReportAssembly,
    "evidence assembly failed"
);
phase0a_source_conversion!(
    EvidenceWriterError,
    Phase0aRunErrorCode::EvidencePublication,
    "evidence publication failed"
);

/// Runs the exact generic rehearsal and returns only preflighted evidence.
///
/// This is the sole production entry point. It uses the concrete bounded
/// subprocess adapter and persistent OS lock; injected executors are available
/// only to this module's tests.
pub fn run_generic_rehearsal(
    request: GenericRehearsalRequest,
) -> Result<PublishedGenericRehearsal, Phase0aRunError> {
    let provenance = ProvenanceSession::begin();
    let admitted = admit_rehearsal(&request)?;
    let executor = LockedProcessExecutor::new(
        SubprocessExecutor,
        admitted.editor_lock.clone(),
        EDITOR_LOCK_TIMEOUT,
    );
    run_admitted_rehearsal(admitted, &executor, &provenance)
}

struct AdmittedRehearsal {
    case: LoadedCase,
    requested_editor_executable: PathBuf,
    workspace_root: PathBuf,
    editor_lock: PathBuf,
    evidence_destination: PathBuf,
}

fn admit_rehearsal(
    request: &GenericRehearsalRequest,
) -> Result<AdmittedRehearsal, Phase0aRunError> {
    validate_request_paths(request)?;
    let case = load_case(&request.case_path)?;
    let source_roots = canonical_source_roots(&case)?;
    let workspace_root = resolve_target(
        "workspace root",
        &request.workspace_root,
        TargetState::MustBeAbsent,
        ParentPolicy::ExistingDirectory,
    )?;
    let editor_lock = resolve_target(
        "editor lock",
        &request.editor_lock,
        TargetState::MayExistRegularFile,
        ParentPolicy::TrustedLocal,
    )?;
    reject_run_path_overlap(&workspace_root, &editor_lock, &source_roots)?;
    let evidence_destination = resolve_target(
        "evidence destination",
        &request.evidence_destination,
        TargetState::MustBeAbsent,
        ParentPolicy::TrustedLocal,
    )?;
    let mut forbidden_evidence_roots = source_roots;
    forbidden_evidence_roots.push(workspace_root.clone());
    forbidden_evidence_roots.push(editor_lock.clone());
    reject_evidence_overlap(&evidence_destination, &forbidden_evidence_roots)?;

    Ok(AdmittedRehearsal {
        case,
        requested_editor_executable: request.editor_executable.clone(),
        workspace_root,
        editor_lock,
        evidence_destination,
    })
}

fn run_admitted_rehearsal<E: ProcessExecutor + ?Sized>(
    admitted: AdmittedRehearsal,
    executor: &E,
    provenance: &ProvenanceSession,
) -> Result<PublishedGenericRehearsal, Phase0aRunError> {
    let AdmittedRehearsal {
        case,
        requested_editor_executable,
        workspace_root,
        editor_lock: _,
        evidence_destination,
    } = admitted;
    if !provenance.initially_complete() {
        return publish_controlled_failure(
            &case,
            &evidence_destination,
            provenance,
            None,
            ControlledFailureCode::Provenance,
            None,
            "mandatory build or harness provenance was unavailable before editor execution",
            Vec::new(),
            ControlledFailureProofs::unavailable(),
            Phase0aRunErrorCode::Provenance,
        );
    }
    let environment = match minimal_editor_environment() {
        Ok(environment) => environment,
        Err(error) => {
            return publish_controlled_failure(
                &case,
                &evidence_destination,
                provenance,
                None,
                ControlledFailureCode::EditorEnvironment,
                None,
                &error.to_string(),
                Vec::new(),
                ControlledFailureProofs::unavailable(),
                Phase0aRunErrorCode::EditorEnvironment,
            );
        }
    };
    let executable = match inspect_executable_identity(&requested_editor_executable) {
        Ok(executable)
            if executable.sha256() == case.manifest().editor.expected_executable_sha256 =>
        {
            executable
        }
        Ok(_) => {
            return publish_controlled_failure(
                &case,
                &evidence_destination,
                provenance,
                None,
                ControlledFailureCode::EditorIdentity,
                None,
                "the selected editor executable does not match the case-pinned SHA-256",
                Vec::new(),
                ControlledFailureProofs::unavailable(),
                Phase0aRunErrorCode::EditorIdentity,
            );
        }
        Err(error) => {
            return publish_controlled_failure(
                &case,
                &evidence_destination,
                provenance,
                None,
                ControlledFailureCode::EditorIdentity,
                None,
                &error.to_string(),
                Vec::new(),
                ControlledFailureProofs::unavailable(),
                Phase0aRunErrorCode::EditorIdentity,
            );
        }
    };
    let preparation = match WorkspacePreparation::create(&workspace_root) {
        Ok(preparation) => preparation,
        Err(error) => {
            return publish_controlled_failure(
                &case,
                &evidence_destination,
                provenance,
                workspace_root.is_dir().then_some(workspace_root),
                ControlledFailureCode::WorkspacePreparation,
                None,
                &error.to_string(),
                Vec::new(),
                ControlledFailureProofs::unavailable(),
                Phase0aRunErrorCode::Workspace,
            );
        }
    };
    let mut workspace = match preparation.seal(&case) {
        Ok(workspace) => workspace,
        Err(error) => {
            return publish_controlled_failure(
                &case,
                &evidence_destination,
                provenance,
                Some(workspace_root),
                ControlledFailureCode::WorkspacePreparation,
                None,
                &error.to_string(),
                Vec::new(),
                ControlledFailureProofs::unavailable(),
                Phase0aRunErrorCode::Workspace,
            );
        }
    };
    let canonical_workspace = workspace.root().to_path_buf();
    for operation in OperationId::ORDER {
        if let Err(error) = workspace.execute(
            executor,
            operation,
            executable.canonical_path(),
            environment.clone(),
        ) {
            let (processes, source_rechecks) = workspace.into_failure_evidence();
            return publish_controlled_failure(
                &case,
                &evidence_destination,
                provenance,
                Some(canonical_workspace),
                ControlledFailureCode::EditorOperation,
                Some(operation),
                &error.to_string(),
                processes,
                ControlledFailureProofs::from_rechecks(&source_rechecks),
                Phase0aRunErrorCode::Workspace,
            );
        }
    }

    let completed = match workspace.finish_with_diagnostics() {
        Ok(completed) => completed,
        Err(failure) => {
            let (error, processes, source_rechecks) = (*failure).into_parts();
            return publish_controlled_failure(
                &case,
                &evidence_destination,
                provenance,
                Some(canonical_workspace),
                ControlledFailureCode::WorkspaceVerification,
                None,
                &error.to_string(),
                processes,
                ControlledFailureProofs::from_rechecks(&source_rechecks),
                Phase0aRunErrorCode::Workspace,
            );
        }
    };
    let CompletedWorkspaceRunParts {
        case_sha256,
        operation_inventory,
        runs,
        json_sources,
        runtime_inputs,
        project_inventories,
        boundary_evidence,
    } = completed.into_parts();
    if case_sha256 != case.source_sha256() {
        return publish_controlled_failure(
            &case,
            &evidence_destination,
            provenance,
            Some(canonical_workspace),
            ControlledFailureCode::ReportAssembly,
            None,
            "completion proofs were not bound to the same validated case",
            processes_from_runs(runs),
            ControlledFailureProofs::unavailable()
                .with_workspace(&project_inventories, &boundary_evidence),
            Phase0aRunErrorCode::ReportAssembly,
        );
    }
    let package_inventories = boundary_evidence.case_package_inventories();
    let analysis = match analyze_phase0(&case, json_sources, &package_inventories) {
        Ok(analysis) => analysis,
        Err(error) => {
            return publish_controlled_failure(
                &case,
                &evidence_destination,
                provenance,
                Some(canonical_workspace),
                ControlledFailureCode::SemanticAnalysis,
                None,
                &error.to_string(),
                processes_from_runs(runs),
                ControlledFailureProofs::unavailable()
                    .with_workspace(&project_inventories, &boundary_evidence),
                Phase0aRunErrorCode::Analysis,
            );
        }
    };
    let runtime_validations = match complete_runtime_validations(&analysis, runtime_inputs) {
        Ok(validations) => validations,
        Err(error) => {
            return publish_controlled_failure(
                &case,
                &evidence_destination,
                provenance,
                Some(canonical_workspace),
                ControlledFailureCode::RuntimeValidation,
                None,
                &error.to_string(),
                processes_from_runs(runs),
                ControlledFailureProofs::unavailable()
                    .with_workspace(&project_inventories, &boundary_evidence)
                    .with_analysis(&analysis),
                Phase0aRunErrorCode::RuntimeValidation,
            );
        }
    };
    let runtime_provenance = match provenance.snapshot().require_complete() {
        Ok(provenance) => provenance,
        Err(error) => {
            return publish_controlled_failure(
                &case,
                &evidence_destination,
                provenance,
                Some(canonical_workspace),
                ControlledFailureCode::Provenance,
                None,
                &error.to_string(),
                processes_from_runs(runs),
                ControlledFailureProofs::unavailable()
                    .with_workspace(&project_inventories, &boundary_evidence)
                    .with_analysis(&analysis)
                    .with_runtime(&runtime_validations),
                Phase0aRunErrorCode::Provenance,
            );
        }
    };
    let report_inputs = Phase0aReportInputs::new(
        &case,
        &case_sha256,
        &operation_inventory,
        &runs,
        &project_inventories,
        &boundary_evidence,
        &analysis,
        &runtime_validations,
        runtime_provenance,
    );
    let prepared = match prepare_phase0a_evidence(report_inputs) {
        Ok(prepared) => prepared,
        Err(error) => {
            return publish_controlled_failure(
                &case,
                &evidence_destination,
                provenance,
                Some(canonical_workspace),
                ControlledFailureCode::ReportAssembly,
                None,
                &error.to_string(),
                processes_from_runs(runs),
                ControlledFailureProofs::unavailable()
                    .with_workspace(&project_inventories, &boundary_evidence)
                    .with_analysis(&analysis)
                    .with_runtime(&runtime_validations),
                Phase0aRunErrorCode::ReportAssembly,
            );
        }
    };
    publish_success(
        &evidence_destination,
        PreparedSuccessfulRehearsal {
            case_id: case.manifest().case_id.clone(),
            case_sha256,
            workspace_root: canonical_workspace,
            prepared,
        },
    )
}

fn processes_from_runs(
    runs: Vec<crate::run_workspace::WorkspaceRunEvidence>,
) -> Vec<ProcessEvidence> {
    runs.into_iter()
        .map(crate::run_workspace::WorkspaceRunEvidence::into_process)
        .collect()
}

fn publish_success(
    destination: &Path,
    success: PreparedSuccessfulRehearsal,
) -> Result<PublishedGenericRehearsal, Phase0aRunError> {
    let report_sha256 = success.prepared.report_sha256().to_owned();
    let persisted = write_prepared_evidence_bundle(destination, success.prepared)?;
    debug_assert_eq!(persisted.report_sha256(), report_sha256);
    Ok(PublishedGenericRehearsal {
        case_id: success.case_id,
        case_sha256: success.case_sha256,
        destination: persisted.destination().to_path_buf(),
        report_sha256,
        workspace_root: Some(success.workspace_root),
        passed: true,
        failure_code: None,
    })
}

#[allow(clippy::too_many_arguments)]
fn publish_controlled_failure(
    case: &LoadedCase,
    destination: &Path,
    provenance: &ProvenanceSession,
    workspace_root: Option<PathBuf>,
    code: ControlledFailureCode,
    operation: Option<OperationId>,
    diagnostic: &str,
    processes: Vec<ProcessEvidence>,
    proofs: ControlledFailureProofs<'_>,
    public_code: Phase0aRunErrorCode,
) -> Result<PublishedGenericRehearsal, Phase0aRunError> {
    let packages = proofs.package_inventories();
    let provenance =
        controlled_phase0a_provenance(provenance.snapshot(), case, packages.as_ref(), &processes);
    let inputs = ControlledFailureInputs::new(
        case, code, operation, diagnostic, &processes, proofs, provenance,
    );
    let prepared: PreparedControlledFailureEvidenceBundle =
        prepare_controlled_failure_evidence(inputs)?;
    let report_sha256 = prepared.report_sha256().to_owned();
    let persisted = write_prepared_controlled_failure_evidence_bundle(destination, prepared)?;
    debug_assert_eq!(persisted.report_sha256(), report_sha256);
    Ok(PublishedGenericRehearsal {
        case_id: case.manifest().case_id.clone(),
        case_sha256: case.source_sha256().to_owned(),
        destination: persisted.destination().to_path_buf(),
        report_sha256,
        workspace_root,
        passed: false,
        failure_code: Some(public_code),
    })
}

fn canonical_source_roots(case: &LoadedCase) -> Result<Vec<PathBuf>, Phase0aRunError> {
    let packages = &case.manifest().packages;
    [
        &packages.current.root,
        &packages.replacement_submission.root,
        &packages.new_submission.root,
    ]
    .into_iter()
    .map(|root| {
        fs::canonicalize(root).map_err(|source| {
            Phase0aRunError::caused(
                Phase0aRunErrorCode::Filesystem,
                format!(
                    "could not canonicalize source package root `{}`",
                    root.display()
                ),
                source,
            )
        })
    })
    .collect()
}

fn validate_request_paths(request: &GenericRehearsalRequest) -> Result<(), Phase0aRunError> {
    normalized_absolute("case path", &request.case_path)?;
    normalized_absolute("editor executable", &request.editor_executable)?;
    normalized_absolute("workspace root", &request.workspace_root)?;
    normalized_absolute("editor lock", &request.editor_lock)?;
    normalized_absolute("evidence destination", &request.evidence_destination)?;
    Ok(())
}

fn normalized_absolute(role: &'static str, path: &Path) -> Result<PathBuf, Phase0aRunError> {
    let valid = path.is_absolute()
        && path
            .to_str()
            .is_some_and(|text| !text.contains('\\') && !text.contains('\0'))
        && path.components().all(|component| {
            matches!(
                component,
                Component::RootDir | Component::Prefix(_) | Component::Normal(_)
            )
        });
    if valid {
        Ok(path.to_path_buf())
    } else {
        Err(Phase0aRunError::policy(
            Phase0aRunErrorCode::InvalidPath,
            format!(
                "invalid {role} `{}`: must be an absolute normalized path",
                path.display()
            ),
        ))
    }
}

#[derive(Clone, Copy)]
enum TargetState {
    MustBeAbsent,
    MayExistRegularFile,
}

#[derive(Clone, Copy)]
enum ParentPolicy {
    ExistingDirectory,
    TrustedLocal,
}

fn resolve_target(
    role: &'static str,
    path: &Path,
    state: TargetState,
    parent_policy: ParentPolicy,
) -> Result<PathBuf, Phase0aRunError> {
    let path = normalized_absolute(role, path)?;
    let parent = path
        .parent()
        .ok_or_else(|| invalid_target(role, &path, "must have a parent"))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| invalid_target(role, &path, "must name one path component"))?;
    let canonical_parent = fs::canonicalize(parent).map_err(|source| {
        Phase0aRunError::caused(
            Phase0aRunErrorCode::Filesystem,
            format!(
                "could not canonicalize {role} parent `{}`",
                parent.display()
            ),
            source,
        )
    })?;
    let parent_metadata = fs::symlink_metadata(&canonical_parent).map_err(|source| {
        Phase0aRunError::caused(
            Phase0aRunErrorCode::Filesystem,
            format!(
                "could not inspect {role} parent `{}`",
                canonical_parent.display()
            ),
            source,
        )
    })?;
    if parent_metadata.file_type().is_symlink() || !parent_metadata.is_dir() {
        return Err(invalid_target(
            role,
            &path,
            "parent must be a physical directory",
        ));
    }
    if matches!(parent_policy, ParentPolicy::TrustedLocal) {
        require_trusted_local_parent(role, &canonical_parent)?;
    }

    let resolved = canonical_parent.join(file_name);
    match fs::symlink_metadata(&resolved) {
        Ok(metadata) => match state {
            TargetState::MustBeAbsent => {
                return Err(invalid_target(role, &path, "target must not already exist"));
            }
            TargetState::MayExistRegularFile
                if metadata.file_type().is_symlink() || !metadata.is_file() =>
            {
                return Err(invalid_target(
                    role,
                    &path,
                    "existing target must be a physical regular file",
                ));
            }
            TargetState::MayExistRegularFile => {}
        },
        Err(source) if source.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(Phase0aRunError::caused(
                Phase0aRunErrorCode::Filesystem,
                format!("could not inspect {role} `{}`", resolved.display()),
                source,
            ));
        }
    }
    Ok(resolved)
}

fn invalid_target(role: &'static str, path: &Path, reason: &'static str) -> Phase0aRunError {
    Phase0aRunError::policy(
        Phase0aRunErrorCode::InvalidPath,
        format!("invalid {role} `{}`: {reason}", path.display()),
    )
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn require_trusted_local_parent(role: &'static str, parent: &Path) -> Result<(), Phase0aRunError> {
    use std::os::unix::fs::MetadataExt as _;

    let directory = File::open(parent).map_err(|source| {
        Phase0aRunError::caused(
            Phase0aRunErrorCode::Filesystem,
            format!(
                "could not open trusted {role} parent `{}`",
                parent.display()
            ),
            source,
        )
    })?;
    let opened = directory.metadata().map_err(|source| {
        Phase0aRunError::caused(
            Phase0aRunErrorCode::Filesystem,
            format!(
                "could not inspect opened {role} parent `{}`",
                parent.display()
            ),
            source,
        )
    })?;
    let named = fs::metadata(parent).map_err(|source| {
        Phase0aRunError::caused(
            Phase0aRunErrorCode::Filesystem,
            format!("could not recheck {role} parent `{}`", parent.display()),
            source,
        )
    })?;
    let effective_uid = rustix::process::geteuid().as_raw();
    if !opened.is_dir()
        || opened.uid() != effective_uid
        || opened.mode() & 0o022 != 0
        || opened.dev() != named.dev()
        || opened.ino() != named.ino()
        || opened.mode() != named.mode()
        || opened.uid() != named.uid()
    {
        return Err(invalid_target(
            role,
            parent,
            "parent must be a stable user-owned directory that is not group/world-writable",
        ));
    }
    require_local_filesystem(role, &directory)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn require_trusted_local_parent(role: &'static str, parent: &Path) -> Result<(), Phase0aRunError> {
    Err(invalid_target(
        role,
        parent,
        "trusted local publication is supported only on macOS and Linux",
    ))
}

#[cfg(target_os = "macos")]
fn require_local_filesystem(role: &'static str, directory: &File) -> Result<(), Phase0aRunError> {
    let status = rustix::fs::fstatfs(directory).map_err(|source| {
        Phase0aRunError::caused(
            Phase0aRunErrorCode::Filesystem,
            format!("could not inspect {role} parent filesystem"),
            source,
        )
    })?;
    if status.f_flags & u32::try_from(libc::MNT_LOCAL).unwrap_or(0) == 0 {
        return Err(Phase0aRunError::policy(
            Phase0aRunErrorCode::InvalidPath,
            format!("{role} parent must be hosted on a verified local filesystem"),
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn require_local_filesystem(role: &'static str, directory: &File) -> Result<(), Phase0aRunError> {
    let status = rustix::fs::fstatfs(directory).map_err(|source| {
        Phase0aRunError::caused(
            Phase0aRunErrorCode::Filesystem,
            format!("could not inspect {role} parent filesystem"),
            source,
        )
    })?;
    if !matches!(
        i128::from(status.f_type),
        0xEF53
            | 0x5846_5342
            | 0x9123_683E
            | 0x0102_1994
            | 0x794C_7630
            | 0x2FC1_2FC1
            | 0x8584_58F6
            | 0xF2F5_2010
    ) {
        return Err(Phase0aRunError::policy(
            Phase0aRunErrorCode::InvalidPath,
            format!("{role} parent filesystem is not in the local-filesystem allowlist"),
        ));
    }
    Ok(())
}

fn reject_run_path_overlap(
    workspace_root: &Path,
    editor_lock: &Path,
    source_roots: &[PathBuf],
) -> Result<(), Phase0aRunError> {
    let source_overlap = source_roots
        .iter()
        .any(|source| paths_overlap(workspace_root, source) || paths_overlap(editor_lock, source));
    if source_overlap || paths_overlap(workspace_root, editor_lock) {
        Err(Phase0aRunError::policy(
            Phase0aRunErrorCode::RunPathOverlap,
            "the workspace or editor lock overlaps a source package or each other",
        ))
    } else {
        Ok(())
    }
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}

fn minimal_editor_environment() -> Result<BTreeMap<String, String>, Phase0aRunError> {
    let home = env::var("HOME").map_err(|_error| {
        Phase0aRunError::policy(
            Phase0aRunErrorCode::EditorEnvironment,
            "the minimal editor environment is missing a valid absolute UTF-8 HOME",
        )
    })?;
    if !Path::new(&home).is_absolute() || home.contains('\0') {
        return Err(Phase0aRunError::policy(
            Phase0aRunErrorCode::EditorEnvironment,
            "the minimal editor environment is missing a valid absolute UTF-8 HOME",
        ));
    }
    let mut environment = BTreeMap::from([
        ("HOME".to_owned(), home),
        ("LANG".to_owned(), "C".to_owned()),
        ("LC_ALL".to_owned(), "C".to_owned()),
        (
            "PATH".to_owned(),
            "/usr/bin:/bin:/usr/sbin:/sbin".to_owned(),
        ),
    ]);
    if let Ok(temporary) = env::var("TMPDIR")
        && Path::new(&temporary).is_absolute()
        && !temporary.contains('\0')
    {
        environment.insert("TMPDIR".to_owned(), temporary);
    }
    Ok(environment)
}

fn reject_evidence_overlap(
    destination: &Path,
    forbidden_roots: &[PathBuf],
) -> Result<(), Phase0aRunError> {
    if forbidden_roots
        .iter()
        .any(|root| paths_overlap(destination, root))
    {
        Err(Phase0aRunError::policy(
            Phase0aRunErrorCode::EvidenceDestinationOverlap,
            "the evidence destination overlaps a source package or rehearsal workspace",
        ))
    } else {
        Ok(())
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt as _, symlink};

    struct CaseFixture {
        case_path: PathBuf,
        current: PathBuf,
        replacement: PathBuf,
        new_submission: PathBuf,
    }

    fn case_fixture(root: &Path, executable_sha256: &str) -> CaseFixture {
        let current = root.join("current");
        let replacement = root.join("replacement");
        let new_submission = root.join("new-submission");
        for package in [&current, &replacement, &new_submission] {
            fs::create_dir(package).expect("package root");
        }
        let quote = |path: &Path| {
            serde_json::to_string(path.to_str().expect("portable temporary path"))
                .expect("TOML-compatible quoted path")
        };
        let case = format!(
            r#"format_version = 2
case_id = "generic-runner-test"
target_spine_version = "4.3.23"
runtime_atlas = "character.atlas"

[editor]
expected_executable_sha256 = "{executable_sha256}"

[packages.current]
root = {}
project = "character.spine"
required_directories = ["images"]
asset_roots = ["images"]

[packages.replacement_submission]
root = {}
project = "character.spine"
required_directories = ["images"]
asset_roots = ["images"]

[packages.new_submission]
root = {}
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
"#,
            quote(&current),
            quote(&replacement),
            quote(&new_submission),
        );
        let case_path = root.join("case.toml");
        fs::write(&case_path, case).expect("case manifest");
        CaseFixture {
            case_path,
            current,
            replacement,
            new_submission,
        }
    }

    fn request_for(
        root: &Path,
        fixture: &CaseFixture,
        executable: &Path,
    ) -> GenericRehearsalRequest {
        let lock_parent = root.join("locks");
        fs::create_dir(&lock_parent).expect("lock parent");
        GenericRehearsalRequest::new(
            &fixture.case_path,
            executable,
            root.join("workspace"),
            lock_parent.join("spine.lock"),
            root.join("evidence"),
        )
    }

    #[test]
    fn resolves_fresh_targets_through_their_physical_parent() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let physical = temporary.path().join("physical");
        let alias = temporary.path().join("alias");
        fs::create_dir(&physical).expect("physical parent");
        symlink(&physical, &alias).expect("parent alias");

        let resolved = resolve_target(
            "workspace root",
            &alias.join("workspace"),
            TargetState::MustBeAbsent,
            ParentPolicy::ExistingDirectory,
        )
        .expect("resolved target");

        assert_eq!(
            resolved,
            fs::canonicalize(&physical)
                .expect("canonical physical parent")
                .join("workspace")
        );
        assert!(!resolved.exists());
    }

    #[test]
    fn rejects_workspace_and_lock_paths_that_can_mutate_a_source() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let executable = env::current_exe().expect("test executable");
        let fixture = case_fixture(temporary.path(), &"0".repeat(64));
        let mut request = request_for(temporary.path(), &fixture, &executable);
        request.workspace_root = fixture.current.join("workspace");

        let error = admit_rehearsal(&request)
            .err()
            .expect("source-contained workspace");
        assert_eq!(error.code(), Phase0aRunErrorCode::RunPathOverlap);
        assert!(!request.workspace_root.exists());

        request.workspace_root = temporary.path().join("workspace");
        request.editor_lock = fixture.current.join("spine.lock");
        let error = admit_rehearsal(&request)
            .err()
            .expect("source-contained lock");
        assert_eq!(error.code(), Phase0aRunErrorCode::RunPathOverlap);
        assert!(!request.editor_lock.exists());
    }

    #[test]
    fn a_wrong_editor_digest_cannot_create_the_workspace_or_lock() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let executable = env::current_exe().expect("test executable");
        let fixture = case_fixture(temporary.path(), &"0".repeat(64));
        let request = request_for(temporary.path(), &fixture, &executable);

        let published = run_generic_rehearsal(request.clone()).expect("published failure");

        assert!(!published.passed());
        assert_eq!(
            published.failure_code(),
            Some(Phase0aRunErrorCode::EditorIdentity)
        );
        assert!(published.destination().join("report.json").is_file());
        assert!(!request.workspace_root.exists());
        assert!(!request.editor_lock.exists());
    }

    #[test]
    fn executable_symlink_is_reduced_to_the_preflighted_canonical_path() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let executable = env::current_exe().expect("test executable");
        let identity = inspect_executable_identity(&executable).expect("editor identity");
        let executable_alias = temporary.path().join("editor-alias");
        symlink(&executable, &executable_alias).expect("executable alias");
        let fixture = case_fixture(temporary.path(), identity.sha256());
        let request = request_for(temporary.path(), &fixture, &executable_alias);

        let admitted = admit_rehearsal(&request).expect("admission");

        assert_eq!(admitted.requested_editor_executable, executable_alias);
        assert!(!admitted.workspace_root.exists());
        assert!(!admitted.editor_lock.exists());
    }

    #[test]
    fn evidence_aliases_are_compared_in_the_physical_namespace() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let source = temporary.path().join("source");
        let alias = temporary.path().join("source-alias");
        fs::create_dir(&source).expect("source root");
        symlink(&source, &alias).expect("source alias");
        let destination = resolve_target(
            "evidence destination",
            &alias.join("evidence"),
            TargetState::MustBeAbsent,
            ParentPolicy::TrustedLocal,
        )
        .expect("physical evidence destination");

        let source = fs::canonicalize(source).expect("canonical source root");
        let error =
            reject_evidence_overlap(&destination, &[source]).expect_err("physical source overlap");

        assert_eq!(
            error.code(),
            Phase0aRunErrorCode::EvidenceDestinationOverlap
        );
        assert!(!destination.exists());
    }

    #[test]
    fn publication_parent_must_be_owned_and_not_group_or_world_writable() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let unsafe_parent = temporary.path().join("unsafe");
        fs::create_dir(&unsafe_parent).expect("unsafe parent");
        fs::set_permissions(&unsafe_parent, fs::Permissions::from_mode(0o777))
            .expect("unsafe permissions");

        let error = resolve_target(
            "evidence destination",
            &unsafe_parent.join("evidence"),
            TargetState::MustBeAbsent,
            ParentPolicy::TrustedLocal,
        )
        .expect_err("unsafe parent");

        assert_eq!(error.code(), Phase0aRunErrorCode::InvalidPath);
        assert!(!unsafe_parent.join("evidence").exists());
    }

    #[test]
    fn normalized_paths_reject_parent_components() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("one/../two");

        let error = normalized_absolute("test path", &path).expect_err("parent component");

        assert_eq!(error.code(), Phase0aRunErrorCode::InvalidPath);
    }

    #[test]
    fn editor_environment_has_the_fixed_macos_system_path() {
        let environment = minimal_editor_environment().expect("minimal editor environment");

        assert_eq!(
            environment.get("PATH").map(String::as_str),
            Some("/usr/bin:/bin:/usr/sbin:/sbin")
        );
    }

    #[test]
    fn source_roots_are_canonicalized_once_before_workspace_creation() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let executable = env::current_exe().expect("test executable");
        let identity = inspect_executable_identity(&executable).expect("editor identity");
        let fixture = case_fixture(temporary.path(), identity.sha256());
        let request = request_for(temporary.path(), &fixture, &executable);

        let admitted = admit_rehearsal(&request).expect("admission");

        let expected = [fixture.current, fixture.replacement, fixture.new_submission]
            .into_iter()
            .map(fs::canonicalize)
            .collect::<Result<Vec<_>, _>>()
            .expect("canonical fixture roots");
        assert_eq!(
            canonical_source_roots(&admitted.case).expect("source roots"),
            expected
        );
        assert!(!admitted.workspace_root.exists());
        assert!(!admitted.evidence_destination.exists());
    }
}

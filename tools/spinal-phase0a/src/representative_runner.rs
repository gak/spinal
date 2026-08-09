//! Closed representative Phase 0A orchestration around the generic-v4 core.

use crate::case::{CaseError, LoadedCase, parse_case};
use crate::phase0a_runner::{GenericRehearsalRequest, Phase0aRunError, run_representative_core};
use crate::provenance::{ProvenanceSession, RepresentativeBuildObservation};
use crate::representative::{
    ObservedRepresentativePackageTrees, RepresentativeEnvelopeError, RepresentativeObservations,
    load_owner_private_exact_file, load_representative_envelope,
};
use crate::representative_evidence::{
    PreparedRepresentativeEvidence, RepresentativeEvidenceError, prepare_representative_evidence,
};
use crate::stage::{
    SecurePackageObservation, StageError, observe_package_identity, secure_inventory_tree,
};
use std::collections::BTreeSet;
use std::env;
use std::error::Error as StdError;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Component, Path, PathBuf};

const BINDING_PATH: &str = "representative-binding.toml";
const CORE_PATH: &str = "core";
const REPORT_PATH: &str = "report.json";
const REPORT_PART_PATH: &str = "report.json.part";

/// Exact paths for one representative gate candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepresentativeRunRequest {
    binding_path: PathBuf,
    case_path: PathBuf,
    editor_executable: PathBuf,
    workspace_root: PathBuf,
    editor_lock: PathBuf,
    evidence_destination: PathBuf,
}

impl RepresentativeRunRequest {
    /// Creates a request. All six paths are revalidated before any mutation or
    /// editor process.
    pub fn new(
        binding_path: impl Into<PathBuf>,
        case_path: impl Into<PathBuf>,
        editor_executable: impl Into<PathBuf>,
        workspace_root: impl Into<PathBuf>,
        editor_lock: impl Into<PathBuf>,
        evidence_destination: impl Into<PathBuf>,
    ) -> Self {
        Self {
            binding_path: binding_path.into(),
            case_path: case_path.into(),
            editor_executable: editor_executable.into(),
            workspace_root: workspace_root.into(),
            editor_lock: editor_lock.into(),
            evidence_destination: evidence_destination.into(),
        }
    }

    /// Returns the exact representative-binding file path.
    pub fn binding_path(&self) -> &Path {
        &self.binding_path
    }

    /// Returns the exact representative-case file path.
    pub fn case_path(&self) -> &Path {
        &self.case_path
    }

    /// Returns the exact Spine editor executable path.
    pub fn editor_executable(&self) -> &Path {
        &self.editor_executable
    }

    /// Returns the fresh workspace path reserved for the run.
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Returns the editor coordination-lock path.
    pub fn editor_lock(&self) -> &Path {
        &self.editor_lock
    }

    /// Returns the fresh evidence destination path.
    pub fn evidence_destination(&self) -> &Path {
        &self.evidence_destination
    }
}

/// Published identity of one successful representative-v5 candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishedRepresentativeRun {
    destination: PathBuf,
    report_sha256: String,
    workspace_root: Option<PathBuf>,
}

impl PublishedRepresentativeRun {
    /// Returns the published representative evidence directory.
    pub fn destination(&self) -> &Path {
        &self.destination
    }

    /// Returns the SHA-256 digest of the published outer report.
    pub fn report_sha256(&self) -> &str {
        &self.report_sha256
    }

    /// Returns the retained core workspace, when the core retained one.
    pub fn workspace_root(&self) -> Option<&Path> {
        self.workspace_root.as_deref()
    }
}

/// Stable failure category for representative orchestration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RepresentativeRunErrorCode {
    /// A request path was not absolute and lexically normalized.
    InvalidPath,
    /// A private admission file could not be loaded or matched.
    AdmissionFile,
    /// The representative case was invalid or changed identity while parsing.
    Case,
    /// Clean, pinned build provenance was unavailable or mismatched.
    BuildProvenance,
    /// A source package or evidence tree could not be inventoried securely.
    PackageInventory,
    /// Representative project roles resolved to aliased project files.
    ProjectAlias,
    /// Mutable and immutable request paths overlapped.
    PathOverlap,
    /// A requested fresh private destination was invalid or unavailable.
    Destination,
    /// The generic Phase 0A core did not publish complete inner evidence.
    Core,
    /// The outer representative evidence graph could not be assembled.
    EvidenceAssembly,
    /// An admitted input or produced artifact changed before publication.
    Reobservation,
    /// The final evidence filesystem publication failed.
    Publication,
}

/// Fail-closed representative error. An error never claims a published report.
#[derive(Debug)]
pub struct RepresentativeRunError {
    code: RepresentativeRunErrorCode,
    detail: String,
    source: Option<Box<dyn StdError + Send + Sync>>,
}

impl RepresentativeRunError {
    /// Returns the stable failure category.
    pub const fn code(&self) -> RepresentativeRunErrorCode {
        self.code
    }

    /// Returns the diagnostic failure detail.
    pub fn detail(&self) -> &str {
        &self.detail
    }

    fn policy(code: RepresentativeRunErrorCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
            source: None,
        }
    }

    fn caused<E>(code: RepresentativeRunErrorCode, context: impl Into<String>, source: E) -> Self
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

impl fmt::Display for RepresentativeRunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl StdError for RepresentativeRunError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn StdError + 'static))
    }
}

/// Runs one binding-pinned candidate. A report is written only after the
/// complete inner core and outer graph have both passed their closed checks.
pub fn run_representative_phase0a(
    request: RepresentativeRunRequest,
) -> Result<PublishedRepresentativeRun, RepresentativeRunError> {
    validate_request_paths(&request)?;
    let provenance = ProvenanceSession::begin();
    let build = provenance
        .representative_build_observation()
        .ok_or_else(|| {
            RepresentativeRunError::policy(
                RepresentativeRunErrorCode::BuildProvenance,
                "representative admission requires complete clean-checkout build provenance",
            )
        })?;

    let (admitted_case, loaded_case) = admit_representative_case(&request.case_path)?;

    let loaded_binding = load_representative_envelope(&request.binding_path).map_err(|source| {
        RepresentativeRunError::caused(
            RepresentativeRunErrorCode::AdmissionFile,
            "representative binding admission failed",
            source,
        )
    })?;
    let source_observations = source_package_observations(&loaded_case)?;
    reject_aliased_projects(&loaded_case)?;
    let observations = representative_observations(&loaded_case, &build, &source_observations)?;
    let binding = loaded_binding
        .verify(&observations)
        .map_err(admission_error)?;
    binding.reobserve().map_err(reobservation_error)?;
    admitted_case.reobserve().map_err(reobservation_error)?;

    let destination = resolve_fresh_private_root(&request.evidence_destination)?;
    let workspace = resolve_fresh_private_root(&request.workspace_root)?;
    validate_path_separation(
        &request,
        &loaded_case,
        &destination,
        &workspace,
        admitted_case.path(),
    )?;
    create_private_directory(&destination)?;
    write_private_new_file(&destination.join(BINDING_PATH), binding.source_bytes())?;

    let core_destination = destination.join(CORE_PATH);
    let core_request = GenericRehearsalRequest::new(
        &request.case_path,
        &request.editor_executable,
        &workspace,
        &request.editor_lock,
        &core_destination,
    );
    let published_core =
        run_representative_core(core_request, &binding, &provenance).map_err(core_error)?;

    source_observations.reobserve()?;
    binding.reobserve().map_err(reobservation_error)?;
    admitted_case.reobserve().map_err(reobservation_error)?;
    if !provenance.representative_reobservation_ready(
        binding.harness_executable_sha256(),
        binding.source_revision(),
        binding.cargo_lock_sha256(),
    ) {
        return Err(RepresentativeRunError::policy(
            RepresentativeRunErrorCode::Reobservation,
            "representative harness or clean build provenance changed before publication",
        ));
    }
    if !published_core.passed() {
        return Err(RepresentativeRunError::policy(
            RepresentativeRunErrorCode::Core,
            format!(
                "representative inner core failed ({:?}); its generic-v4 diagnostics are retained at `{}`, but the outer destination is UNPUBLISHED and no format-v5 report was created",
                published_core.failure_code(),
                core_destination.display()
            ),
        ));
    }

    let core_inventory = secure_inventory_tree(&core_destination).map_err(stage_error)?;
    let prepared = prepare_representative_evidence(
        &binding,
        &loaded_case,
        &admitted_case,
        &published_core,
        core_inventory,
    )
    .map_err(evidence_error)?;
    validate_prepared_sources(&destination, &prepared)?;
    source_observations.reobserve()?;
    binding.reobserve().map_err(reobservation_error)?;
    admitted_case.reobserve().map_err(reobservation_error)?;
    let report_sha256 = prepared.report_sha256().to_owned();
    publish_report_last(&destination, &prepared)?;

    Ok(PublishedRepresentativeRun {
        destination,
        report_sha256,
        workspace_root: published_core.workspace_root().map(Path::to_path_buf),
    })
}

/// Produces strict proposal-only binding TOML from the exact prebuilt
/// representative runner and one owner-private representative case.
///
/// The function creates no files, workspace, editor lock, or evidence. The
/// returned proposal still requires owner review and a later independent run.
pub fn propose_representative_binding(
    case_path: impl AsRef<Path>,
) -> Result<String, RepresentativeRunError> {
    let case_path = normalized_absolute("case", case_path.as_ref())?;
    let provenance = ProvenanceSession::begin();
    let build = provenance
        .representative_build_observation()
        .ok_or_else(|| {
            RepresentativeRunError::policy(
                RepresentativeRunErrorCode::BuildProvenance,
                "binding proposals require an exact prebuilt runner with complete clean-checkout build provenance",
            )
        })?;
    let (admitted_case, loaded_case) = admit_representative_case(&case_path)?;
    let source_observations = source_package_observations(&loaded_case)?;
    reject_aliased_projects(&loaded_case)?;
    validate_proposal_path_separation(&loaded_case, admitted_case.path())?;
    let observations = representative_observations(&loaded_case, &build, &source_observations)?;
    let proposal = observations
        .binding_proposal_toml()
        .map_err(admission_error)?;

    source_observations.reobserve()?;
    admitted_case.reobserve().map_err(reobservation_error)?;
    if !provenance.representative_reobservation_ready(
        build.harness_executable_sha256(),
        build.source_revision(),
        build.cargo_lock_sha256(),
    ) {
        return Err(RepresentativeRunError::policy(
            RepresentativeRunErrorCode::Reobservation,
            "representative harness or clean build provenance changed before proposal output",
        ));
    }
    Ok(proposal)
}

fn admit_representative_case(
    case_path: &Path,
) -> Result<(crate::representative::OwnerPrivateExactFile, LoadedCase), RepresentativeRunError> {
    let admitted_case = load_owner_private_exact_file(case_path).map_err(|source| {
        RepresentativeRunError::caused(
            RepresentativeRunErrorCode::AdmissionFile,
            "representative case admission failed",
            source,
        )
    })?;
    require_representative_case_privacy_safe(admitted_case.source_bytes())?;
    let case_text = std::str::from_utf8(admitted_case.source_bytes()).map_err(|error| {
        RepresentativeRunError::policy(
            RepresentativeRunErrorCode::Case,
            format!("representative case is not UTF-8: {error}"),
        )
    })?;
    let loaded_case = parse_case(case_text).map_err(case_error)?;
    if loaded_case.source_sha256() != admitted_case.source_sha256() {
        return Err(RepresentativeRunError::policy(
            RepresentativeRunErrorCode::Case,
            "parsed case bytes did not retain their admitted digest",
        ));
    }
    Ok((admitted_case, loaded_case))
}

fn representative_observations(
    case: &LoadedCase,
    build: &RepresentativeBuildObservation,
    sources: &RepresentativeSourceObservations,
) -> Result<RepresentativeObservations, RepresentativeRunError> {
    RepresentativeObservations::new(
        case.source_sha256(),
        build.harness_executable_sha256(),
        build.source_revision(),
        build.cargo_lock_sha256(),
        ObservedRepresentativePackageTrees::new(
            sources.current.inventory().tree_sha256.clone(),
            sources
                .replacement_submission
                .inventory()
                .tree_sha256
                .clone(),
            sources.new_submission.inventory().tree_sha256.clone(),
        )
        .map_err(admission_error)?,
    )
    .map_err(admission_error)
}

fn require_representative_case_privacy_safe(bytes: &[u8]) -> Result<(), RepresentativeRunError> {
    if crate::evidence_writer::evidence_bytes_are_privacy_safe(bytes) {
        Ok(())
    } else {
        Err(RepresentativeRunError::policy(
            RepresentativeRunErrorCode::Case,
            "representative case bytes contain privacy-sensitive license text",
        ))
    }
}

fn case_error(source: CaseError) -> RepresentativeRunError {
    RepresentativeRunError::caused(
        RepresentativeRunErrorCode::Case,
        "representative case validation failed",
        source,
    )
}

fn admission_error(source: RepresentativeEnvelopeError) -> RepresentativeRunError {
    RepresentativeRunError::caused(
        RepresentativeRunErrorCode::AdmissionFile,
        "representative binding did not match admitted observations",
        source,
    )
}

fn reobservation_error(source: RepresentativeEnvelopeError) -> RepresentativeRunError {
    RepresentativeRunError::caused(
        RepresentativeRunErrorCode::Reobservation,
        "representative admission file changed",
        source,
    )
}

fn stage_error(source: StageError) -> RepresentativeRunError {
    RepresentativeRunError::caused(
        RepresentativeRunErrorCode::PackageInventory,
        "representative secure inventory failed",
        source,
    )
}

fn source_reobservation_error(source: StageError) -> RepresentativeRunError {
    RepresentativeRunError::caused(
        RepresentativeRunErrorCode::Reobservation,
        "representative source package identity changed after admission",
        source,
    )
}

fn core_error(source: Phase0aRunError) -> RepresentativeRunError {
    RepresentativeRunError::caused(
        RepresentativeRunErrorCode::Core,
        "representative inner core did not publish a complete report; the retained destination is UNPUBLISHED",
        source,
    )
}

fn evidence_error(source: RepresentativeEvidenceError) -> RepresentativeRunError {
    RepresentativeRunError::caused(
        RepresentativeRunErrorCode::EvidenceAssembly,
        "representative outer evidence could not be assembled",
        source,
    )
}

struct RepresentativeSourceObservations {
    current: SecurePackageObservation,
    replacement_submission: SecurePackageObservation,
    new_submission: SecurePackageObservation,
}

impl RepresentativeSourceObservations {
    fn reobserve(&self) -> Result<(), RepresentativeRunError> {
        self.current
            .reobserve()
            .map_err(source_reobservation_error)?;
        self.replacement_submission
            .reobserve()
            .map_err(source_reobservation_error)?;
        self.new_submission
            .reobserve()
            .map_err(source_reobservation_error)
    }
}

fn source_package_observations(
    case: &LoadedCase,
) -> Result<RepresentativeSourceObservations, RepresentativeRunError> {
    let packages = &case.manifest().packages;
    Ok(RepresentativeSourceObservations {
        current: observe_package_identity(&packages.current).map_err(stage_error)?,
        replacement_submission: observe_package_identity(&packages.replacement_submission)
            .map_err(stage_error)?,
        new_submission: observe_package_identity(&packages.new_submission).map_err(stage_error)?,
    })
}

fn validate_request_paths(
    request: &RepresentativeRunRequest,
) -> Result<(), RepresentativeRunError> {
    for (role, path) in [
        ("representative binding", request.binding_path()),
        ("case", request.case_path()),
        ("editor executable", request.editor_executable()),
        ("workspace root", request.workspace_root()),
        ("editor lock", request.editor_lock()),
        ("evidence destination", request.evidence_destination()),
    ] {
        normalized_absolute(role, path)?;
    }
    Ok(())
}

fn normalized_absolute(role: &'static str, path: &Path) -> Result<PathBuf, RepresentativeRunError> {
    let valid = path.is_absolute()
        && path
            .to_str()
            .is_some_and(|text| !text.contains(['\\', '\0']))
        && path.components().all(|component| {
            matches!(
                component,
                Component::RootDir | Component::Prefix(_) | Component::Normal(_)
            )
        });
    if valid {
        Ok(path.to_path_buf())
    } else {
        Err(RepresentativeRunError::policy(
            RepresentativeRunErrorCode::InvalidPath,
            format!(
                "invalid {role} `{}`: must be an absolute normalized path",
                path.display()
            ),
        ))
    }
}

fn resolve_fresh_private_root(path: &Path) -> Result<PathBuf, RepresentativeRunError> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::fs::MetadataExt as _;

        let path = normalized_absolute("fresh private directory", path)?;
        let parent = path.parent().ok_or_else(|| {
            RepresentativeRunError::policy(
                RepresentativeRunErrorCode::InvalidPath,
                "fresh private directory must have a parent",
            )
        })?;
        let file_name = path.file_name().ok_or_else(|| {
            RepresentativeRunError::policy(
                RepresentativeRunErrorCode::InvalidPath,
                "fresh private directory must name one path component",
            )
        })?;
        let canonical_parent = fs::canonicalize(parent).map_err(|source| {
            RepresentativeRunError::caused(
                RepresentativeRunErrorCode::Destination,
                "could not canonicalize fresh private directory parent",
                source,
            )
        })?;
        let metadata = fs::symlink_metadata(&canonical_parent).map_err(|source| {
            RepresentativeRunError::caused(
                RepresentativeRunErrorCode::Destination,
                "could not inspect fresh private directory parent",
                source,
            )
        })?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.mode() & 0o077 != 0
        {
            return Err(RepresentativeRunError::policy(
                RepresentativeRunErrorCode::Destination,
                "fresh representative directories require an owner-private physical parent",
            ));
        }
        let resolved = canonical_parent.join(file_name);
        match fs::symlink_metadata(&resolved) {
            Ok(_) => Err(RepresentativeRunError::policy(
                RepresentativeRunErrorCode::Destination,
                format!("fresh directory already exists: `{}`", resolved.display()),
            )),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(resolved),
            Err(source) => Err(RepresentativeRunError::caused(
                RepresentativeRunErrorCode::Destination,
                "could not inspect fresh representative directory",
                source,
            )),
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = path;
        Err(RepresentativeRunError::policy(
            RepresentativeRunErrorCode::Destination,
            "representative evidence is supported only on macOS and Linux",
        ))
    }
}

fn validate_path_separation(
    request: &RepresentativeRunRequest,
    case: &LoadedCase,
    destination: &Path,
    workspace: &Path,
    admitted_case: &Path,
) -> Result<(), RepresentativeRunError> {
    let manifest = case.manifest();
    let sources = [
        fs::canonicalize(&manifest.packages.current.root),
        fs::canonicalize(&manifest.packages.replacement_submission.root),
        fs::canonicalize(&manifest.packages.new_submission.root),
    ]
    .into_iter()
    .collect::<Result<Vec<_>, _>>()
    .map_err(|source| {
        RepresentativeRunError::caused(
            RepresentativeRunErrorCode::PathOverlap,
            "could not canonicalize a representative source root",
            source,
        )
    })?;
    let binding = fs::canonicalize(request.binding_path()).map_err(|source| {
        RepresentativeRunError::caused(
            RepresentativeRunErrorCode::PathOverlap,
            "could not canonicalize representative binding",
            source,
        )
    })?;
    let case_path = fs::canonicalize(admitted_case).map_err(|source| {
        RepresentativeRunError::caused(
            RepresentativeRunErrorCode::PathOverlap,
            "could not canonicalize representative case",
            source,
        )
    })?;
    let editor = fs::canonicalize(request.editor_executable()).map_err(|source| {
        RepresentativeRunError::caused(
            RepresentativeRunErrorCode::PathOverlap,
            "could not canonicalize editor executable",
            source,
        )
    })?;
    let harness = env::current_exe()
        .and_then(fs::canonicalize)
        .map_err(|source| {
            RepresentativeRunError::caused(
                RepresentativeRunErrorCode::PathOverlap,
                "could not canonicalize representative harness",
                source,
            )
        })?;
    let lock = resolve_may_exist_file(request.editor_lock())?;
    let mutable = [destination.to_path_buf(), workspace.to_path_buf(), lock];
    let immutable_files = [binding, case_path, editor, harness];
    if mutable.iter().enumerate().any(|(index, left)| {
        mutable
            .iter()
            .skip(index + 1)
            .any(|right| paths_overlap(left, right))
    }) || mutable.iter().any(|candidate| {
        sources
            .iter()
            .any(|source| paths_overlap(candidate, source))
            || immutable_files
                .iter()
                .any(|file| paths_overlap(candidate, file))
    }) || immutable_files
        .iter()
        .any(|file| sources.iter().any(|source| paths_overlap(file, source)))
    {
        return Err(RepresentativeRunError::policy(
            RepresentativeRunErrorCode::PathOverlap,
            "representative mutable paths and immutable inputs must not overlap",
        ));
    }
    if same_physical_file(&immutable_files[0], &immutable_files[1])? {
        return Err(RepresentativeRunError::policy(
            RepresentativeRunErrorCode::PathOverlap,
            "representative binding and case must be distinct physical files",
        ));
    }
    Ok(())
}

fn validate_proposal_path_separation(
    case: &LoadedCase,
    admitted_case: &Path,
) -> Result<(), RepresentativeRunError> {
    let manifest = case.manifest();
    let sources = [
        fs::canonicalize(&manifest.packages.current.root),
        fs::canonicalize(&manifest.packages.replacement_submission.root),
        fs::canonicalize(&manifest.packages.new_submission.root),
    ]
    .into_iter()
    .collect::<Result<Vec<_>, _>>()
    .map_err(|source| {
        RepresentativeRunError::caused(
            RepresentativeRunErrorCode::PathOverlap,
            "could not canonicalize a representative source root",
            source,
        )
    })?;
    let case_path = fs::canonicalize(admitted_case).map_err(|source| {
        RepresentativeRunError::caused(
            RepresentativeRunErrorCode::PathOverlap,
            "could not canonicalize representative case",
            source,
        )
    })?;
    let harness = env::current_exe()
        .and_then(fs::canonicalize)
        .map_err(|source| {
            RepresentativeRunError::caused(
                RepresentativeRunErrorCode::PathOverlap,
                "could not canonicalize representative harness",
                source,
            )
        })?;
    if [case_path, harness]
        .iter()
        .any(|file| sources.iter().any(|source| paths_overlap(file, source)))
    {
        return Err(RepresentativeRunError::policy(
            RepresentativeRunErrorCode::PathOverlap,
            "representative case and harness must not overlap a source package",
        ));
    }
    Ok(())
}

fn resolve_may_exist_file(path: &Path) -> Result<PathBuf, RepresentativeRunError> {
    let path = normalized_absolute("editor lock", path)?;
    let parent = path.parent().ok_or_else(|| {
        RepresentativeRunError::policy(
            RepresentativeRunErrorCode::InvalidPath,
            "editor lock must have a parent",
        )
    })?;
    let file_name = path.file_name().ok_or_else(|| {
        RepresentativeRunError::policy(
            RepresentativeRunErrorCode::InvalidPath,
            "editor lock must name one path component",
        )
    })?;
    let resolved = fs::canonicalize(parent)
        .map_err(|source| {
            RepresentativeRunError::caused(
                RepresentativeRunErrorCode::PathOverlap,
                "could not canonicalize editor-lock parent",
                source,
            )
        })?
        .join(file_name);
    match fs::symlink_metadata(&resolved) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(RepresentativeRunError::policy(
                RepresentativeRunErrorCode::InvalidPath,
                "existing editor lock must be a physical regular file",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(RepresentativeRunError::caused(
                RepresentativeRunErrorCode::InvalidPath,
                "could not inspect editor lock",
                source,
            ));
        }
    }
    Ok(resolved)
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}

fn reject_aliased_projects(case: &LoadedCase) -> Result<(), RepresentativeRunError> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::fs::MetadataExt as _;

        let packages = &case.manifest().packages;
        let projects = [
            packages.current.root.join(&packages.current.project),
            packages
                .replacement_submission
                .root
                .join(&packages.replacement_submission.project),
            packages
                .new_submission
                .root
                .join(&packages.new_submission.project),
        ];
        let mut physical = BTreeSet::new();
        let mut folded = BTreeSet::new();
        for project in projects {
            let canonical = fs::canonicalize(&project).map_err(|source| {
                RepresentativeRunError::caused(
                    RepresentativeRunErrorCode::ProjectAlias,
                    "could not canonicalize a declared representative project",
                    source,
                )
            })?;
            let metadata = fs::symlink_metadata(&canonical).map_err(|source| {
                RepresentativeRunError::caused(
                    RepresentativeRunErrorCode::ProjectAlias,
                    "could not inspect a declared representative project",
                    source,
                )
            })?;
            if !metadata.is_file()
                || !physical.insert((metadata.dev(), metadata.ino()))
                || !folded.insert(canonical.to_string_lossy().to_lowercase())
            {
                return Err(RepresentativeRunError::policy(
                    RepresentativeRunErrorCode::ProjectAlias,
                    "the three representative roles must name distinct physical project files",
                ));
            }
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = case;
        Err(RepresentativeRunError::policy(
            RepresentativeRunErrorCode::ProjectAlias,
            "representative project identity requires macOS or Linux",
        ))
    }
}

fn same_physical_file(left: &Path, right: &Path) -> Result<bool, RepresentativeRunError> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::fs::MetadataExt as _;

        let left = fs::metadata(left).map_err(publication_io)?;
        let right = fs::metadata(right).map_err(publication_io)?;
        Ok(left.dev() == right.dev() && left.ino() == right.ino())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (left, right);
        Ok(false)
    }
}

fn create_private_directory(path: &Path) -> Result<(), RepresentativeRunError> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, PermissionsExt as _};

        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        builder.create(path).map_err(publication_io)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(publication_io)?;
        let metadata = fs::symlink_metadata(path).map_err(publication_io)?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.mode() & 0o777 != 0o700
        {
            return Err(RepresentativeRunError::policy(
                RepresentativeRunErrorCode::Destination,
                "created representative destination is not owner-private",
            ));
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = path;
        Err(RepresentativeRunError::policy(
            RepresentativeRunErrorCode::Destination,
            "representative publication requires macOS or Linux",
        ))
    }
}

fn write_private_new_file(path: &Path, bytes: &[u8]) -> Result<(), RepresentativeRunError> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};

        let mut options = OpenOptions::new();
        options.write(true).create_new(true).mode(0o600);
        let mut file = options.open(path).map_err(publication_io)?;
        file.write_all(bytes).map_err(publication_io)?;
        file.flush().map_err(publication_io)?;
        file.sync_all().map_err(publication_io)?;
        let metadata = file.metadata().map_err(publication_io)?;
        if !metadata.is_file()
            || metadata.nlink() != 1
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.mode() & 0o777 != 0o600
            || metadata.len() != u64::try_from(bytes.len()).unwrap_or(u64::MAX)
        {
            return Err(RepresentativeRunError::policy(
                RepresentativeRunErrorCode::Publication,
                "created representative evidence file is not a private exact copy",
            ));
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (path, bytes);
        Err(RepresentativeRunError::policy(
            RepresentativeRunErrorCode::Publication,
            "representative publication requires macOS or Linux",
        ))
    }
}

fn validate_prepared_sources(
    destination: &Path,
    prepared: &PreparedRepresentativeEvidence,
) -> Result<(), RepresentativeRunError> {
    if prepared.core_source() != destination.join(CORE_PATH) {
        return Err(RepresentativeRunError::policy(
            RepresentativeRunErrorCode::EvidenceAssembly,
            "prepared core source was not the fixed representative core directory",
        ));
    }
    let observed = secure_inventory_tree(prepared.core_source()).map_err(stage_error)?;
    if &observed != prepared.core_inventory() {
        return Err(RepresentativeRunError::policy(
            RepresentativeRunErrorCode::Reobservation,
            "representative core changed after outer report preparation",
        ));
    }
    let binding = load_owner_private_exact_file(destination.join(BINDING_PATH))
        .map_err(reobservation_error)?;
    if binding.source_bytes() != prepared.binding_bytes() {
        return Err(RepresentativeRunError::policy(
            RepresentativeRunErrorCode::Reobservation,
            "published representative binding copy does not match admitted bytes",
        ));
    }
    Ok(())
}

fn publish_report_last(
    destination: &Path,
    prepared: &PreparedRepresentativeEvidence,
) -> Result<(), RepresentativeRunError> {
    publish_report_last_inner(destination, prepared, false)
}

fn publish_report_last_inner(
    destination: &Path,
    prepared: &PreparedRepresentativeEvidence,
    fail_before_rename: bool,
) -> Result<(), RepresentativeRunError> {
    validate_prepared_sources(destination, prepared)?;
    let part = destination.join(REPORT_PART_PATH);
    let report = destination.join(REPORT_PATH);
    write_private_new_file(&part, prepared.report_bytes())?;
    let exact = load_owner_private_exact_file(&part).map_err(reobservation_error)?;
    if exact.source_sha256() != prepared.report_sha256()
        || exact.source_bytes() != prepared.report_bytes()
    {
        return Err(RepresentativeRunError::policy(
            RepresentativeRunErrorCode::Publication,
            "representative report part bytes did not match the prepared report",
        ));
    }
    validate_prepared_sources(destination, prepared)?;
    let names = fs::read_dir(destination)
        .map_err(publication_io)?
        .map(|entry| {
            entry
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .map_err(publication_io)
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    let expected = [BINDING_PATH, CORE_PATH, REPORT_PART_PATH]
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    if names != expected {
        return Err(RepresentativeRunError::policy(
            RepresentativeRunErrorCode::Publication,
            "representative destination did not retain the exact fixed layout",
        ));
    }
    File::open(destination)
        .and_then(|directory| directory.sync_all())
        .map_err(publication_io)?;
    if fail_before_rename {
        return Err(RepresentativeRunError::policy(
            RepresentativeRunErrorCode::Publication,
            "injected representative failure before final report rename",
        ));
    }
    // The part file has already been securely re-read and the complete fixed
    // layout has been preflighted. Keep this same-directory rename as the final
    // fallible operation: once `report.json` is visible this function cannot
    // return `Err` and the CLI cannot mislabel it UNPUBLISHED.
    fs::rename(&part, &report).map_err(publication_io)?;
    Ok(())
}

fn publication_io(source: io::Error) -> RepresentativeRunError {
    RepresentativeRunError::caused(
        RepresentativeRunErrorCode::Publication,
        "representative evidence filesystem operation failed",
        source,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_retains_the_exact_six_path_contract() {
        let request = RepresentativeRunRequest::new(
            "/binding",
            "/case",
            "/editor",
            "/workspace",
            "/lock",
            "/evidence",
        );
        assert_eq!(request.binding_path(), Path::new("/binding"));
        assert_eq!(request.case_path(), Path::new("/case"));
        assert_eq!(request.editor_executable(), Path::new("/editor"));
        assert_eq!(request.workspace_root(), Path::new("/workspace"));
        assert_eq!(request.editor_lock(), Path::new("/lock"));
        assert_eq!(request.evidence_destination(), Path::new("/evidence"));
    }

    #[test]
    fn request_rejects_relative_parent_and_backslash_paths() {
        for path in ["relative", "/tmp/../escape", "/tmp/back\\slash"] {
            assert!(normalized_absolute("test", Path::new(path)).is_err());
        }
    }

    #[test]
    fn representative_case_privacy_failure_precedes_publication() {
        assert!(require_representative_case_privacy_safe(b"case bytes").is_ok());
        let error = require_representative_case_privacy_safe(b"Licensed to: Private Owner")
            .expect_err("private license text");
        assert_eq!(error.code(), RepresentativeRunErrorCode::Case);
    }

    #[test]
    fn normalized_absolute_accepts_an_absolute_normal_path() {
        let path = Path::new("/tmp/spinal-phase0a/editor.lock");
        assert_eq!(
            normalized_absolute("test", path).expect("absolute normalized path"),
            path
        );
    }

    #[test]
    fn editor_lock_resolution_accepts_absent_and_regular_files() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let parent = fs::canonicalize(temporary.path()).expect("canonical parent");
        let lock = parent.join("editor.lock");

        assert_eq!(
            resolve_may_exist_file(&lock).expect("absent lock path"),
            lock
        );
        fs::write(&lock, b"held").expect("regular lock file");
        assert_eq!(
            resolve_may_exist_file(&lock).expect("regular lock path"),
            lock
        );
    }

    #[test]
    fn editor_lock_resolution_rejects_a_directory() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let lock = fs::canonicalize(temporary.path()).expect("canonical directory");
        let error = resolve_may_exist_file(&lock).expect_err("directory is not a lock file");
        assert_eq!(error.code(), RepresentativeRunErrorCode::InvalidPath);
    }

    #[cfg(unix)]
    #[test]
    fn editor_lock_resolution_rejects_a_symlink() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().expect("temporary directory");
        let parent = fs::canonicalize(temporary.path()).expect("canonical parent");
        let target = parent.join("target.lock");
        fs::write(&target, b"held").expect("target lock file");
        let lock = parent.join("editor.lock");
        symlink(&target, &lock).expect("lock symlink");

        let error = resolve_may_exist_file(&lock).expect_err("symlink is not a lock file");
        assert_eq!(error.code(), RepresentativeRunErrorCode::InvalidPath);
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn pre_rename_failure_never_exposes_report_json() {
        use std::os::unix::fs::PermissionsExt as _;

        let temporary = tempfile::tempdir().expect("temporary parent");
        fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700))
            .expect("private parent");
        let destination = temporary.path().join("evidence");
        create_private_directory(&destination).expect("private evidence root");
        let binding_bytes = b"binding bytes\n".to_vec();
        write_private_new_file(&destination.join(BINDING_PATH), &binding_bytes)
            .expect("binding copy");
        let core = destination.join(CORE_PATH);
        create_private_directory(&core).expect("core directory");
        let core_inventory = secure_inventory_tree(&core).expect("core inventory");
        let prepared = PreparedRepresentativeEvidence::test_only(
            b"{\"format_version\":5}\n".to_vec(),
            binding_bytes,
            core,
            core_inventory,
        );

        let error = publish_report_last_inner(&destination, &prepared, true)
            .expect_err("injected pre-rename failure");
        assert_eq!(error.code(), RepresentativeRunErrorCode::Publication);
        assert!(!destination.join(REPORT_PATH).exists());
        assert!(destination.join(REPORT_PART_PATH).is_file());
    }
}

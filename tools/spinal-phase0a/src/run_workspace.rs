//! Sealed private workspace and per-operation mutation boundary for Phase 0A.

use crate::case::{LoadedCase, PackageSpec, SkeletonNames};
use crate::digest::sha256_bytes;
use crate::operation_recipe::{
    CompletedOperationInventory, OperationId, OperationRecipe, OperationRecord, RecipeError,
};
use crate::package::{CasePackageInventories, EntryKind, PackageInventory};
use crate::phase0_analysis::Phase0JsonSources;
use crate::process::{
    NewAnimationCollisionEvidence, ProcessEvidence, ProcessExecutor, ProcessFailureCode,
    ProjectInfoError, ProjectInfoInventory, TranscriptProfile,
};
use crate::spine_cli::{OutputMode, SpineCommand, approved_export_preset_bytes};
use crate::spine_run::{SpineRunError, SpineRunEvidence, execute_spine_command_attempt};
use crate::stage::{
    ControlledSourceRecheck, ControlledSourceRecheckStatus, StageError, StagedPackage,
    stage_package,
};
use crate::workspace_snapshot::{WorkspaceSnapshot, WorkspaceSnapshotError, snapshot_workspace};
use serde::Serialize;
use spinal::{RuntimeBundleError, RuntimeBundleManifest};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use rustix::fs::{Mode, OFlags, openat};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::ffi::CString;

const PRESET_PATH: &str = "policy/pretty-nonessential.export.json";
const CURRENT_PATH: &str = "packages/current";
const REPLACEMENT_PATH: &str = "packages/replacement-submission";
const NEW_SUBMISSION_PATH: &str = "packages/new-submission";
const EXISTING_CANDIDATE_PATH: &str = "packages/existing-candidate";
const NEW_CANDIDATE_PATH: &str = "packages/new-candidate";
const NEW_COLLISION_CONTROL_PATH: &str = "packages/new-collision-control";
const MISSING_IMAGES_PATH: &str = "packages/missing-images-control";
const IMAGES_PATH: &str = "images";
const RUNTIME_VIRTUAL_ROOT: &str = "review";
const RUNTIME_JSON_PATH: &str = "review/rig.json";
const MAX_EXTRACTED_JSON_BYTES: u64 = 16 * 1024 * 1024;
const MAX_EXTRACTED_ATLAS_BYTES: u64 = 2 * 1024 * 1024;
const MAX_EXTRACTED_PAGE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TOTAL_EXTRACTED_BYTES: u64 = 384 * 1024 * 1024;

const JSON_OUTPUT_OPERATIONS: [OperationId; 10] = [
    OperationId::ExportCurrentA,
    OperationId::ExportReplacementSubmission,
    OperationId::ExportNewSubmission,
    OperationId::ExportReconstructedA,
    OperationId::ExportCurrentB,
    OperationId::ExportReconstructedB,
    OperationId::ExportExistingFirst,
    OperationId::ExportExistingRepeat,
    OperationId::ExportNewFirst,
    OperationId::ExportNewCollisionControl,
];

const FIXED_DIRECTORIES: &[&str] = &[
    "policy",
    "packages",
    "outputs/round-trip/a/source",
    "outputs/round-trip/a/reconstructed-json",
    "outputs/round-trip/b/source",
    "outputs/round-trip/b/reconstructed-json",
    "outputs/submissions/replacement",
    "outputs/submissions/new",
    "outputs/candidates/existing/first",
    "outputs/candidates/existing/repeat",
    "outputs/candidates/new/first",
    "outputs/candidates/new/collision-control",
    "outputs/negative-control",
];

/// One typed editor run bound to complete before-and-after tree evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkspaceRunEvidence {
    before: PackageInventory,
    before_physical_sha256: String,
    run: SpineRunEvidence,
    after: PackageInventory,
    after_physical_sha256: String,
}

impl WorkspaceRunEvidence {
    /// Returns exact-path editor evidence for this operation.
    pub(crate) fn run(&self) -> &SpineRunEvidence {
        &self.run
    }

    pub(crate) fn into_process(self) -> ProcessEvidence {
        self.run.into_process()
    }

    /// Returns the complete physical snapshot digest before execution.
    #[cfg(test)]
    pub(crate) fn before_physical_sha256(&self) -> &str {
        &self.before_physical_sha256
    }

    /// Returns the complete physical snapshot digest after execution.
    #[cfg(test)]
    pub(crate) fn after_physical_sha256(&self) -> &str {
        &self.after_physical_sha256
    }
}

/// Physical identity retained for one immutable external source root.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceRootIdentityEvidence {
    device: u64,
    inode: u64,
}

/// Portable evidence for one source package across staging and completion.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourcePackageCompletionEvidence {
    root_identity: SourceRootIdentityEvidence,
    before_staging: PackageInventory,
    after_staging: PackageInventory,
    after_run: PackageInventory,
}

/// Source evidence for exactly the three manifest package roles.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourcePackageSetCompletionEvidence {
    current: SourcePackageCompletionEvidence,
    replacement_submission: SourcePackageCompletionEvidence,
    new_submission: SourcePackageCompletionEvidence,
}

/// Sealed and final portable evidence for one staged package role.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StagedPackageCompletionEvidence {
    sealed: PackageInventory,
    final_state: PackageInventory,
}

/// Portable staged-package evidence for every fixed workspace role.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StagedPackageSetCompletionEvidence {
    current: StagedPackageCompletionEvidence,
    replacement_submission: StagedPackageCompletionEvidence,
    new_submission: StagedPackageCompletionEvidence,
    existing_candidate: StagedPackageCompletionEvidence,
    new_candidate: StagedPackageCompletionEvidence,
    new_collision_control: StagedPackageCompletionEvidence,
    missing_images_control: StagedPackageCompletionEvidence,
}

/// Complete serializable physical and portable filesystem evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkspaceBoundaryEvidence {
    sealed_physical: WorkspaceSnapshot,
    sealed_portable: PackageInventory,
    final_physical: WorkspaceSnapshot,
    final_portable: PackageInventory,
    sources: SourcePackageSetCompletionEvidence,
    staged: StagedPackageSetCompletionEvidence,
}

impl WorkspaceBoundaryEvidence {
    #[cfg(test)]
    pub(crate) fn sealed_portable(&self) -> &PackageInventory {
        &self.sealed_portable
    }

    #[cfg(test)]
    pub(crate) fn final_portable(&self) -> &PackageInventory {
        &self.final_portable
    }

    /// Derives the three semantic-analysis inputs without caller-selected roles.
    pub(crate) fn case_package_inventories(&self) -> CasePackageInventories {
        CasePackageInventories {
            current: self.sources.current.before_staging.clone(),
            replacement_submission: self.sources.replacement_submission.before_staging.clone(),
            new_submission: self.sources.new_submission.before_staging.clone(),
        }
    }
}

/// One extracted runtime file bound to both workspace and virtual paths.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeFileBinding {
    workspace_path: String,
    virtual_path: PathBuf,
    size: u64,
    sha256: String,
}

/// Exact bytes for one independently validated runtime target.
pub(crate) struct RuntimeTargetInput {
    json_path: PathBuf,
    atlas_path: PathBuf,
    files: BTreeMap<PathBuf, Vec<u8>>,
    bindings: Vec<RuntimeFileBinding>,
}

impl RuntimeTargetInput {
    #[cfg(test)]
    pub(crate) fn json_path(&self) -> &Path {
        &self.json_path
    }

    #[cfg(test)]
    pub(crate) fn atlas_path(&self) -> &Path {
        &self.atlas_path
    }

    pub(crate) fn files(&self) -> &BTreeMap<PathBuf, Vec<u8>> {
        &self.files
    }

    pub(crate) fn bindings(&self) -> &[RuntimeFileBinding] {
        &self.bindings
    }

    pub(crate) fn into_bundle_parts(self) -> (PathBuf, PathBuf, BTreeMap<PathBuf, Vec<u8>>) {
        (self.json_path, self.atlas_path, self.files)
    }
}

impl RuntimeFileBinding {
    pub(crate) fn workspace_path(&self) -> &str {
        &self.workspace_path
    }

    pub(crate) fn virtual_path(&self) -> &Path {
        &self.virtual_path
    }

    pub(crate) fn size(&self) -> u64 {
        self.size
    }

    pub(crate) fn sha256(&self) -> &str {
        &self.sha256
    }
}

/// Runtime inputs for exactly CurrentB, ExistingRepeat, and NewFirst.
pub(crate) struct RuntimeInputs {
    current: RuntimeTargetInput,
    existing: RuntimeTargetInput,
    new: RuntimeTargetInput,
}

impl RuntimeInputs {
    #[cfg(test)]
    pub(crate) fn current(&self) -> &RuntimeTargetInput {
        &self.current
    }

    #[cfg(test)]
    pub(crate) fn existing(&self) -> &RuntimeTargetInput {
        &self.existing
    }

    #[cfg(test)]
    pub(crate) fn new_target(&self) -> &RuntimeTargetInput {
        &self.new
    }

    pub(crate) fn into_targets(
        self,
    ) -> (RuntimeTargetInput, RuntimeTargetInput, RuntimeTargetInput) {
        (self.current, self.existing, self.new)
    }
}

/// Strict project-info inventories for the three manifest package roles.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkspaceProjectInventories {
    current: ProjectInfoInventory,
    replacement_submission: ProjectInfoInventory,
    new_submission: ProjectInfoInventory,
}

/// Unforgeable result of the one closed workspace recipe and extraction pass.
pub(crate) struct CompletedWorkspaceRun {
    case_sha256: String,
    operation_inventory: CompletedOperationInventory,
    runs: Vec<WorkspaceRunEvidence>,
    json_sources: Phase0JsonSources,
    runtime_inputs: RuntimeInputs,
    project_inventories: WorkspaceProjectInventories,
    boundary_evidence: WorkspaceBoundaryEvidence,
}

impl CompletedWorkspaceRun {
    #[cfg(test)]
    pub(crate) fn case_sha256(&self) -> &str {
        &self.case_sha256
    }

    #[cfg(test)]
    pub(crate) fn operation_inventory(&self) -> &CompletedOperationInventory {
        &self.operation_inventory
    }

    #[cfg(test)]
    pub(crate) fn runs(&self) -> &[WorkspaceRunEvidence] {
        &self.runs
    }

    #[cfg(test)]
    pub(crate) fn json_sources(&self) -> &Phase0JsonSources {
        &self.json_sources
    }

    #[cfg(test)]
    pub(crate) fn runtime_inputs(&self) -> &RuntimeInputs {
        &self.runtime_inputs
    }

    #[cfg(test)]
    pub(crate) fn boundary_evidence(&self) -> &WorkspaceBoundaryEvidence {
        &self.boundary_evidence
    }

    pub(crate) fn into_parts(self) -> CompletedWorkspaceRunParts {
        CompletedWorkspaceRunParts {
            case_sha256: self.case_sha256,
            operation_inventory: self.operation_inventory,
            runs: self.runs,
            json_sources: self.json_sources,
            runtime_inputs: self.runtime_inputs,
            project_inventories: self.project_inventories,
            boundary_evidence: self.boundary_evidence,
        }
    }
}

/// Owned components produced only by consuming a completed workspace token.
pub(crate) struct CompletedWorkspaceRunParts {
    pub(crate) case_sha256: String,
    pub(crate) operation_inventory: CompletedOperationInventory,
    pub(crate) runs: Vec<WorkspaceRunEvidence>,
    pub(crate) json_sources: Phase0JsonSources,
    pub(crate) runtime_inputs: RuntimeInputs,
    pub(crate) project_inventories: WorkspaceProjectInventories,
    pub(crate) boundary_evidence: WorkspaceBoundaryEvidence,
}

/// A controlled finish failure paired with every retained process capture.
pub(crate) struct FailedWorkspaceFinish {
    error: RunWorkspaceError,
    processes: Vec<ProcessEvidence>,
    source_rechecks: ControlledSourceRechecks,
}

impl FailedWorkspaceFinish {
    pub(crate) fn into_parts(
        self,
    ) -> (
        RunWorkspaceError,
        Vec<ProcessEvidence>,
        ControlledSourceRechecks,
    ) {
        (self.error, self.processes, self.source_rechecks)
    }
}

/// Best-effort re-inventories of all three immutable source package roles.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ControlledSourceRechecks {
    current: ControlledSourceRecheck,
    replacement_submission: ControlledSourceRecheck,
    new_submission: ControlledSourceRecheck,
}

impl ControlledSourceRechecks {
    pub(crate) fn status(&self) -> ControlledSourceRecheckStatus {
        let statuses = [
            self.current.status(),
            self.replacement_submission.status(),
            self.new_submission.status(),
        ];
        if statuses.contains(&ControlledSourceRecheckStatus::Changed) {
            ControlledSourceRecheckStatus::Changed
        } else if statuses
            .iter()
            .all(|status| *status == ControlledSourceRecheckStatus::Unchanged)
        {
            ControlledSourceRecheckStatus::Unchanged
        } else {
            ControlledSourceRecheckStatus::Unavailable
        }
    }
}

struct WorkspaceFinishProducts {
    operation_inventory: CompletedOperationInventory,
    json_sources: Phase0JsonSources,
    runtime_inputs: RuntimeInputs,
    project_inventories: WorkspaceProjectInventories,
    boundary_evidence: WorkspaceBoundaryEvidence,
}

/// A fresh private root in which only the fixed Phase 0A layout may be built.
///
/// Consuming `seal` removes all generic staging and file-creation capability
/// before any editor process can run.
pub(crate) struct WorkspacePreparation {
    boundary: WorkspaceBoundary,
}

/// A sealed, linear editor workspace with no generic mutation API.
pub(crate) struct RunWorkspace {
    boundary: WorkspaceBoundary,
    recipe: OperationRecipe,
    case_sha256: String,
    skeletons: SkeletonNames,
    runtime_atlas: PathBuf,
    current: StagedPackage,
    replacement_submission: StagedPackage,
    new_submission: StagedPackage,
    existing_candidate: StagedPackage,
    new_candidate: StagedPackage,
    readable: BTreeMap<String, ReadableCapability>,
    created_slots: BTreeSet<String>,
    update_slots: BTreeSet<String>,
    completed_operations: BTreeSet<OperationId>,
    runs: Vec<WorkspaceRunEvidence>,
    failed_process: Option<ProcessEvidence>,
    sealed_snapshot: WorkspaceSnapshot,
    last_snapshot: WorkspaceSnapshot,
    poisoned: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PackageRole {
    CurrentBaseline,
    ReplacementSubmission,
    NewSubmission,
    ExistingCandidate,
    NewCandidate,
    NewCollisionControl,
    MissingImagesControl,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReadableCapability {
    Package(PackageRole),
    ApprovedPreset,
    CreatedOutput(OperationId),
}

/// Failures that prevent the workspace from remaining a trustworthy boundary.
#[derive(Debug, Error)]
pub(crate) enum RunWorkspaceError {
    /// The host cannot provide the reviewed Unix filesystem contract.
    #[cfg_attr(
        any(target_os = "linux", target_os = "macos"),
        allow(
            dead_code,
            reason = "constructed only by fail-closed non-Unix workspace branches"
        )
    )]
    #[error("private Phase 0A run workspaces are supported only on macOS and Linux")]
    UnsupportedPlatform,
    /// A root or workspace-relative path was unsafe.
    #[error("invalid private run path `{path}`: {reason}")]
    InvalidPath { path: PathBuf, reason: &'static str },
    /// A fresh run root or create-only preparation file already existed.
    #[error("private run path already exists: `{0}`")]
    AlreadyExists(PathBuf),
    /// An expected private directory was missing or insecure.
    #[error("private run directory is missing or insecure: `{0}`")]
    InsecureDirectory(PathBuf),
    /// The retained run root changed physical identity or permissions.
    #[error("private run root identity changed: `{0}`")]
    RootIdentityChanged(PathBuf),
    /// The fixed package layout did not satisfy its role contract.
    #[error("invalid fixed workspace layout: {0}")]
    InvalidLayout(String),
    /// A typed input or output escaped the private run root.
    #[error("typed command path escaped the private run tree: `{0}`")]
    CommandEscaped(PathBuf),
    /// One exact recipe operation was submitted more than once.
    #[error("Phase 0A operation {0:?} has already completed")]
    OperationAlreadyCompleted(OperationId),
    /// Operations must run in the one closed recipe order.
    #[error("Phase 0A operation must be {expected:?}, not {actual:?}")]
    OutOfOrder {
        expected: OperationId,
        actual: OperationId,
    },
    /// A command attempted to read a path that had not been staged or created.
    #[error("typed command input lacks a readable capability: `{0}`")]
    UnreadableInput(PathBuf),
    /// A command output did not match its fixed create/update capability.
    #[error("typed command output lacks its exact capability: `{0}`")]
    InvalidOutputCapability(PathBuf),
    /// The tree changed after the preceding successful operation returned.
    #[error("private run tree changed between operations at `{0}`")]
    BetweenOperationMutation(PathBuf),
    /// A command changed something outside its exact declared output envelope.
    #[error("typed command violated its mutation envelope at `{0}`")]
    MutationEnvelope(PathBuf),
    /// A fixed extraction path did not match final snapshot and operation evidence.
    #[error("fixed workspace extraction was not bound at `{0}`")]
    ExtractionBinding(PathBuf),
    /// One extracted file exceeded its fixed role limit.
    #[error("fixed workspace extraction exceeded its {limit}-byte file limit at `{path}`")]
    ExtractionByteLimit { path: PathBuf, limit: u64 },
    /// The complete fixed extraction exceeded its aggregate byte limit.
    #[error("fixed workspace extraction exceeded its aggregate byte limit")]
    ExtractionTotalByteLimit,
    /// A captured operation did not satisfy its typed process policy.
    #[error("Phase 0A operation {0:?} did not pass its process policy")]
    UnexpectedProcessFailure(OperationId),
    /// A launched process did not use the case-pinned editor bytes.
    #[error("Phase 0A operation {0:?} did not use the case-pinned editor executable")]
    UntrustedEditorExecutable(OperationId),
    /// The workspace was finished before every fixed recipe operation ran.
    #[error("private run workspace completed {completed} of {expected} required operations")]
    IncompleteOperations { completed: usize, expected: usize },
    /// A prior trust-boundary error permanently disabled the workspace.
    #[error("private run workspace is poisoned after an earlier boundary failure")]
    Poisoned,
    /// Secure descriptor-relative staging failed.
    #[error(transparent)]
    Stage(#[from] StageError),
    /// Secure descriptor-relative tree capture failed.
    #[error(transparent)]
    Snapshot(#[from] WorkspaceSnapshotError),
    /// The closed operation recipe could not be constructed.
    #[error(transparent)]
    Recipe(#[from] RecipeError),
    /// Strict project-info extraction or manifest skeleton validation failed.
    #[error(transparent)]
    ProjectInfo(#[from] ProjectInfoError),
    /// Shared runtime dependency discovery rejected the extracted bytes.
    #[error(transparent)]
    RuntimeBundle(#[from] RuntimeBundleError),
    /// Typed Spine execution or exact-file discovery failed.
    #[error(transparent)]
    Run(#[from] SpineRunError),
    /// A private preparation filesystem operation failed.
    #[error("could not {operation} `{path}`: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

impl WorkspacePreparation {
    /// Creates and retains one fresh owner-private run root.
    pub(crate) fn create(destination: impl AsRef<Path>) -> Result<Self, RunWorkspaceError> {
        Ok(Self {
            boundary: WorkspaceBoundary::create(destination.as_ref())?,
        })
    }

    /// Builds the one fixed package/output layout, validates it, and seals it.
    pub(crate) fn seal(mut self, case: &LoadedCase) -> Result<RunWorkspace, RunWorkspaceError> {
        self.boundary.verify()?;
        for relative in FIXED_DIRECTORIES {
            self.create_directory(Path::new(relative))?;
        }
        let preset =
            self.write_private_file(Path::new(PRESET_PATH), approved_export_preset_bytes())?;

        let manifest = case.manifest();
        require_exact_images_contract(&manifest.packages.current)?;
        let current = self.stage_role(&manifest.packages.current, CURRENT_PATH)?;
        require_runtime_atlas(current.staged(), &manifest.runtime_atlas)?;
        let replacement_submission =
            self.stage_role(&manifest.packages.replacement_submission, REPLACEMENT_PATH)?;
        let new_submission =
            self.stage_role(&manifest.packages.new_submission, NEW_SUBMISSION_PATH)?;
        let existing_candidate =
            self.stage_role(&manifest.packages.current, EXISTING_CANDIDATE_PATH)?;
        let new_candidate = self.stage_role(&manifest.packages.current, NEW_CANDIDATE_PATH)?;
        let new_collision_control = self.stage_role(
            &manifest.packages.new_submission,
            NEW_COLLISION_CONTROL_PATH,
        )?;
        let missing_images = self.stage_role(&manifest.packages.current, MISSING_IMAGES_PATH)?;

        require_identical_package(
            current.staged(),
            existing_candidate.staged(),
            "existing-animation candidate",
        )?;
        require_identical_package(
            current.staged(),
            new_candidate.staged(),
            "new-animation candidate",
        )?;
        require_identical_package(
            new_submission.staged(),
            new_collision_control.staged(),
            "new-animation collision control",
        )?;

        let removed_images = missing_images.root().join(IMAGES_PATH);
        fs::remove_dir_all(&removed_images).map_err(|source| {
            workspace_io_error(
                "remove exact missing-images control tree",
                &removed_images,
                source,
            )
        })?;
        let missing_inventory = snapshot_workspace(missing_images.root())?.evidence();
        require_only_images_removed(current.staged(), &missing_inventory)?;
        if !missing_images.project().is_file() {
            return Err(RunWorkspaceError::InvalidLayout(
                "removing `images` also removed the negative-control project".to_owned(),
            ));
        }

        let recipe = OperationRecipe::new(case, &self.boundary.root)?;
        let mut readable = BTreeMap::new();
        insert_readable(
            &self.boundary.root,
            current.project(),
            ReadableCapability::Package(PackageRole::CurrentBaseline),
            &mut readable,
        )?;
        insert_readable(
            &self.boundary.root,
            replacement_submission.project(),
            ReadableCapability::Package(PackageRole::ReplacementSubmission),
            &mut readable,
        )?;
        insert_readable(
            &self.boundary.root,
            new_submission.project(),
            ReadableCapability::Package(PackageRole::NewSubmission),
            &mut readable,
        )?;
        insert_readable(
            &self.boundary.root,
            existing_candidate.project(),
            ReadableCapability::Package(PackageRole::ExistingCandidate),
            &mut readable,
        )?;
        insert_readable(
            &self.boundary.root,
            new_candidate.project(),
            ReadableCapability::Package(PackageRole::NewCandidate),
            &mut readable,
        )?;
        insert_readable(
            &self.boundary.root,
            new_collision_control.project(),
            ReadableCapability::Package(PackageRole::NewCollisionControl),
            &mut readable,
        )?;
        insert_readable(
            &self.boundary.root,
            missing_images.project(),
            ReadableCapability::Package(PackageRole::MissingImagesControl),
            &mut readable,
        )?;
        insert_readable(
            &self.boundary.root,
            &preset,
            ReadableCapability::ApprovedPreset,
            &mut readable,
        )?;

        let (created_slots, update_slots) =
            build_output_capabilities(&self.boundary.root, &recipe, &readable)?;
        validate_recipe_inputs(&self.boundary.root, &recipe, &readable, &created_slots)?;
        self.boundary.verify()?;
        let sealed_snapshot = snapshot_workspace(&self.boundary.root)?;

        Ok(RunWorkspace {
            boundary: self.boundary,
            recipe,
            case_sha256: case.source_sha256().to_owned(),
            skeletons: manifest.skeletons.clone(),
            runtime_atlas: manifest.runtime_atlas.clone(),
            current,
            replacement_submission,
            new_submission,
            existing_candidate,
            new_candidate,
            readable,
            created_slots,
            update_slots,
            completed_operations: BTreeSet::new(),
            runs: Vec::with_capacity(OperationId::ORDER.len()),
            failed_process: None,
            last_snapshot: sealed_snapshot.clone(),
            sealed_snapshot,
            poisoned: false,
        })
    }

    fn create_directory(&mut self, relative: &Path) -> Result<PathBuf, RunWorkspaceError> {
        self.boundary.verify()?;
        let relative = safe_relative(relative)?;
        let mut path = self.boundary.root.clone();
        for component in Path::new(&relative).components() {
            let Component::Normal(name) = component else {
                unreachable!("safe_relative accepts only normal components");
            };
            path.push(name);
            match fs::symlink_metadata(&path) {
                Ok(metadata) => require_private_directory_metadata(&path, &metadata)?,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    fs::create_dir(&path).map_err(|source| {
                        workspace_io_error("create fixed private directory", &path, source)
                    })?;
                    make_directory_private(&path)?;
                }
                Err(source) => {
                    return workspace_io("inspect fixed private directory", &path, source);
                }
            }
        }
        self.boundary.verify()?;
        Ok(path)
    }

    fn write_private_file(
        &mut self,
        relative: &Path,
        bytes: &[u8],
    ) -> Result<PathBuf, RunWorkspaceError> {
        self.boundary.verify()?;
        let relative = safe_relative(relative)?;
        let path = self.boundary.root.join(relative);
        let parent = path.parent().expect("a relative file has a parent");
        require_private_directory(parent)?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options
                .mode(0o600)
                .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
        }
        let mut file = options.open(&path).map_err(|source| match source.kind() {
            io::ErrorKind::AlreadyExists => RunWorkspaceError::AlreadyExists(path.clone()),
            _ => workspace_io_error("create fixed private file", &path, source),
        })?;
        file.write_all(bytes)
            .map_err(|source| workspace_io_error("write fixed private file", &path, source))?;
        file.flush()
            .map_err(|source| workspace_io_error("flush fixed private file", &path, source))?;
        let metadata = file
            .metadata()
            .map_err(|source| workspace_io_error("inspect fixed private file", &path, source))?;
        require_private_file_metadata(&path, &metadata)?;
        self.boundary.verify()?;
        Ok(path)
    }

    fn stage_role(
        &mut self,
        package: &PackageSpec,
        relative: &str,
    ) -> Result<StagedPackage, RunWorkspaceError> {
        self.boundary.verify()?;
        let destination = self.boundary.root.join(safe_relative(Path::new(relative))?);
        let parent = destination
            .parent()
            .expect("a fixed package destination has a parent");
        require_private_directory(parent)?;
        let staged = stage_package(package, &destination)?;
        self.boundary.verify()?;
        Ok(staged)
    }
}

impl RunWorkspace {
    /// Returns the sealed canonical root.
    pub(crate) fn root(&self) -> &Path {
        &self.boundary.root
    }

    /// Consumes an aborted workspace and returns every safely assessed editor
    /// process in recipe order, including the failed operation when available.
    pub(crate) fn into_failure_evidence(self) -> (Vec<ProcessEvidence>, ControlledSourceRechecks) {
        let source_rechecks = self.controlled_source_rechecks();
        let mut processes = self
            .runs
            .into_iter()
            .map(|run| run.run.into_process())
            .collect::<Vec<_>>();
        if let Some(process) = self.failed_process {
            processes.push(process);
        }
        (processes, source_rechecks)
    }

    fn controlled_source_rechecks(&self) -> ControlledSourceRechecks {
        ControlledSourceRechecks {
            current: self.current.controlled_source_recheck(),
            replacement_submission: self.replacement_submission.controlled_source_recheck(),
            new_submission: self.new_submission.controlled_source_recheck(),
        }
    }

    /// Executes one approved operation and permanently poisons on any failure.
    pub(crate) fn execute<E: ProcessExecutor + ?Sized>(
        &mut self,
        executor: &E,
        operation: OperationId,
        program: impl AsRef<Path>,
        environment: BTreeMap<String, String>,
    ) -> Result<WorkspaceRunEvidence, RunWorkspaceError> {
        if self.poisoned {
            return Err(RunWorkspaceError::Poisoned);
        }
        let command = self.recipe.command(operation).clone();
        let result =
            self.execute_inner(executor, operation, &command, program.as_ref(), environment);
        match result {
            Ok(evidence) => {
                self.runs.push(evidence.clone());
                Ok(evidence)
            }
            Err(error) => {
                self.poisoned = true;
                Err(error)
            }
        }
    }

    /// Consumes the workspace and returns the only closed extraction token.
    #[cfg(test)]
    pub(crate) fn finish(self) -> Result<CompletedWorkspaceRun, RunWorkspaceError> {
        self.finish_inner(ExtractionLimits::production(), |_| {})
    }

    /// Finishes the fixed recipe while preserving all process diagnostics when
    /// final extraction or boundary validation fails.
    pub(crate) fn finish_with_diagnostics(
        self,
    ) -> Result<CompletedWorkspaceRun, Box<FailedWorkspaceFinish>> {
        match self.finish_products(ExtractionLimits::production(), |_| {}) {
            Ok(products) => Ok(self.complete(products)),
            Err(error) => {
                let (processes, source_rechecks) = self.into_failure_evidence();
                Err(Box::new(FailedWorkspaceFinish {
                    error,
                    processes,
                    source_rechecks,
                }))
            }
        }
    }

    #[cfg(test)]
    fn finish_inner(
        self,
        limits: ExtractionLimits,
        after_final_snapshot: impl FnOnce(&Path),
    ) -> Result<CompletedWorkspaceRun, RunWorkspaceError> {
        let products = self.finish_products(limits, after_final_snapshot)?;
        Ok(self.complete(products))
    }

    fn complete(self, products: WorkspaceFinishProducts) -> CompletedWorkspaceRun {
        CompletedWorkspaceRun {
            case_sha256: self.case_sha256,
            operation_inventory: products.operation_inventory,
            runs: self.runs,
            json_sources: products.json_sources,
            runtime_inputs: products.runtime_inputs,
            project_inventories: products.project_inventories,
            boundary_evidence: products.boundary_evidence,
        }
    }

    fn finish_products(
        &self,
        limits: ExtractionLimits,
        after_final_snapshot: impl FnOnce(&Path),
    ) -> Result<WorkspaceFinishProducts, RunWorkspaceError> {
        if self.poisoned {
            return Err(RunWorkspaceError::Poisoned);
        }
        if self.completed_operations.len() != OperationId::ORDER.len()
            || self.runs.len() != OperationId::ORDER.len()
        {
            return Err(RunWorkspaceError::IncompleteOperations {
                completed: self.runs.len(),
                expected: OperationId::ORDER.len(),
            });
        }
        self.boundary.verify()?;
        let final_snapshot = snapshot_workspace(&self.boundary.root)?;
        if let Some(path) = first_snapshot_difference(&self.last_snapshot, &final_snapshot) {
            return Err(RunWorkspaceError::BetweenOperationMutation(path));
        }
        after_final_snapshot(&self.boundary.root);

        // Evaluate all three checks before propagating any one failure.
        let current_final = self.current.verify_source_unchanged();
        let replacement_final = self.replacement_submission.verify_source_unchanged();
        let new_final = self.new_submission.verify_source_unchanged();
        let current_final = current_final?;
        let replacement_final = replacement_final?;
        let new_final = new_final?;

        let records = OperationId::ORDER
            .into_iter()
            .zip(&self.runs)
            .map(|(id, evidence)| OperationRecord::from_run(id, evidence.run()))
            .collect();
        let operation_inventory = CompletedOperationInventory::validate(&self.recipe, records)?;
        let project_inventories = self.extract_project_inventories()?;
        let mut budget = ExtractionBudget::new(limits);
        let json_documents = self.extract_json_documents(&final_snapshot, &mut budget)?;
        let runtime_inputs =
            self.extract_runtime_inputs(&final_snapshot, &json_documents, &mut budget)?;
        let new_animation_collision = self
            .run_evidence(OperationId::ImportNewCollisionControl)?
            .run()
            .process()
            .new_animation_collision()
            .cloned()
            .ok_or_else(|| {
                RunWorkspaceError::ExtractionBinding(PathBuf::from(
                    "operation/ImportNewCollisionControl/transcript",
                ))
            })?;
        let json_sources = phase0_json_sources(json_documents, new_animation_collision)?;

        self.boundary.verify()?;
        let confirmed_final_snapshot = snapshot_workspace(&self.boundary.root)?;
        if let Some(path) = first_snapshot_difference(&final_snapshot, &confirmed_final_snapshot) {
            return Err(RunWorkspaceError::ExtractionBinding(path));
        }

        let sources = SourcePackageSetCompletionEvidence {
            current: source_completion(&self.current, current_final),
            replacement_submission: source_completion(
                &self.replacement_submission,
                replacement_final,
            ),
            new_submission: source_completion(&self.new_submission, new_final),
        };
        let staged = staged_completion(&self.sealed_snapshot, &final_snapshot)?;
        self.boundary.verify()?;
        Ok(WorkspaceFinishProducts {
            operation_inventory,
            json_sources,
            runtime_inputs,
            project_inventories,
            boundary_evidence: WorkspaceBoundaryEvidence {
                sealed_portable: self.sealed_snapshot.evidence(),
                sealed_physical: self.sealed_snapshot.clone(),
                final_portable: final_snapshot.evidence(),
                final_physical: final_snapshot,
                sources,
                staged,
            },
        })
    }

    fn extract_project_inventories(
        &self,
    ) -> Result<WorkspaceProjectInventories, RunWorkspaceError> {
        let current = self
            .run_evidence(OperationId::InfoCurrent)?
            .run()
            .process()
            .project_info_inventory()?;
        current.require_exact_skeleton(&self.skeletons.current)?;
        let replacement_submission = self
            .run_evidence(OperationId::InfoReplacement)?
            .run()
            .process()
            .project_info_inventory()?;
        replacement_submission.require_exact_skeleton(&self.skeletons.replacement_submission)?;
        let new_submission = self
            .run_evidence(OperationId::InfoNew)?
            .run()
            .process()
            .project_info_inventory()?;
        new_submission.require_exact_skeleton(&self.skeletons.new_submission)?;
        Ok(WorkspaceProjectInventories {
            current,
            replacement_submission,
            new_submission,
        })
    }

    fn extract_json_documents(
        &self,
        final_snapshot: &WorkspaceSnapshot,
        budget: &mut ExtractionBudget,
    ) -> Result<BTreeMap<OperationId, BoundWorkspaceFile>, RunWorkspaceError> {
        JSON_OUTPUT_OPERATIONS
            .into_iter()
            .map(|id| {
                self.extract_operation_output(id, final_snapshot, budget)
                    .map(|file| (id, file))
            })
            .collect()
    }

    fn extract_operation_output(
        &self,
        id: OperationId,
        final_snapshot: &WorkspaceSnapshot,
        budget: &mut ExtractionBudget,
    ) -> Result<BoundWorkspaceFile, RunWorkspaceError> {
        let command = self.recipe.command(id);
        let [expected] = command.expected_outputs() else {
            return Err(RunWorkspaceError::ExtractionBinding(PathBuf::from(
                format!("operation/{id:?}"),
            )));
        };
        if expected.mode() != OutputMode::CreatedFile {
            return Err(RunWorkspaceError::ExtractionBinding(
                expected.path().to_path_buf(),
            ));
        }
        let run = self.run_evidence(id)?.run();
        let [observed] = run.outputs() else {
            return Err(RunWorkspaceError::ExtractionBinding(
                expected.path().to_path_buf(),
            ));
        };
        let Some(after) = observed.after() else {
            return Err(RunWorkspaceError::ExtractionBinding(
                expected.path().to_path_buf(),
            ));
        };
        if observed.id() != expected.id()
            || observed.path() != expected.path()
            || observed.mode() != expected.mode()
        {
            return Err(RunWorkspaceError::ExtractionBinding(
                expected.path().to_path_buf(),
            ));
        }
        let relative = relative_to_root(&self.boundary.root, expected.path())?;
        let file_limit = budget.limits.json_bytes;
        read_bound_workspace_file(
            &self.boundary,
            final_snapshot,
            &relative,
            file_limit,
            Some(after.sha256()),
            budget,
        )
    }

    fn extract_runtime_inputs(
        &self,
        final_snapshot: &WorkspaceSnapshot,
        json_documents: &BTreeMap<OperationId, BoundWorkspaceFile>,
        budget: &mut ExtractionBudget,
    ) -> Result<RuntimeInputs, RunWorkspaceError> {
        Ok(RuntimeInputs {
            current: self.extract_runtime_target(
                OperationId::ExportCurrentB,
                self.current.root(),
                final_snapshot,
                json_documents,
                budget,
            )?,
            existing: self.extract_runtime_target(
                OperationId::ExportExistingRepeat,
                self.existing_candidate.root(),
                final_snapshot,
                json_documents,
                budget,
            )?,
            new: self.extract_runtime_target(
                OperationId::ExportNewFirst,
                self.new_candidate.root(),
                final_snapshot,
                json_documents,
                budget,
            )?,
        })
    }

    fn extract_runtime_target(
        &self,
        json_operation: OperationId,
        package_root: &Path,
        final_snapshot: &WorkspaceSnapshot,
        json_documents: &BTreeMap<OperationId, BoundWorkspaceFile>,
        budget: &mut ExtractionBudget,
    ) -> Result<RuntimeTargetInput, RunWorkspaceError> {
        let json = json_documents.get(&json_operation).ok_or_else(|| {
            RunWorkspaceError::ExtractionBinding(PathBuf::from(format!(
                "operation/{json_operation:?}"
            )))
        })?;
        let json_path = PathBuf::from(RUNTIME_JSON_PATH);
        let atlas_path = Path::new(RUNTIME_VIRTUAL_ROOT).join(&self.runtime_atlas);
        let atlas_workspace = package_root.join(&self.runtime_atlas);
        let atlas_relative = relative_to_root(&self.boundary.root, &atlas_workspace)?;
        let atlas_limit = budget.limits.atlas_bytes;
        let atlas = read_bound_workspace_file(
            &self.boundary,
            final_snapshot,
            &atlas_relative,
            atlas_limit,
            None,
            budget,
        )?;
        let page_paths = RuntimeBundleManifest::required_page_paths(
            &json_path,
            &atlas_path,
            &json.bytes,
            &atlas.bytes,
        )?;

        let mut files = BTreeMap::from([
            (json_path.clone(), json.bytes.clone()),
            (atlas_path.clone(), atlas.bytes.clone()),
        ]);
        let mut bindings = vec![
            json.binding(json_path.clone()),
            atlas.binding(atlas_path.clone()),
        ];
        for virtual_path in page_paths {
            let package_relative = virtual_path
                .strip_prefix(RUNTIME_VIRTUAL_ROOT)
                .map_err(|_| RunWorkspaceError::ExtractionBinding(virtual_path.clone()))?;
            let workspace_path = package_root.join(package_relative);
            let workspace_relative = relative_to_root(&self.boundary.root, &workspace_path)?;
            let page_limit = budget.limits.page_bytes;
            let page = read_bound_workspace_file(
                &self.boundary,
                final_snapshot,
                &workspace_relative,
                page_limit,
                None,
                budget,
            )?;
            if files
                .insert(virtual_path.clone(), page.bytes.clone())
                .is_some()
            {
                return Err(RunWorkspaceError::ExtractionBinding(virtual_path));
            }
            bindings.push(page.binding(virtual_path));
        }
        bindings.sort_by(|left, right| left.virtual_path.cmp(&right.virtual_path));
        Ok(RuntimeTargetInput {
            json_path,
            atlas_path,
            files,
            bindings,
        })
    }

    fn run_evidence(&self, id: OperationId) -> Result<&WorkspaceRunEvidence, RunWorkspaceError> {
        let index = OperationId::ORDER
            .iter()
            .position(|candidate| *candidate == id)
            .ok_or_else(|| {
                RunWorkspaceError::ExtractionBinding(PathBuf::from(format!("operation/{id:?}")))
            })?;
        self.runs.get(index).ok_or_else(|| {
            RunWorkspaceError::ExtractionBinding(PathBuf::from(format!("operation/{id:?}")))
        })
    }

    fn execute_inner<E: ProcessExecutor + ?Sized>(
        &mut self,
        executor: &E,
        operation: OperationId,
        command: &SpineCommand,
        program: &Path,
        environment: BTreeMap<String, String>,
    ) -> Result<WorkspaceRunEvidence, RunWorkspaceError> {
        self.boundary.verify()?;
        let before = snapshot_workspace(&self.boundary.root)?;
        if let Some(path) = first_snapshot_difference(&self.last_snapshot, &before) {
            return Err(RunWorkspaceError::BetweenOperationMutation(path));
        }
        if self.completed_operations.contains(&operation) {
            return Err(RunWorkspaceError::OperationAlreadyCompleted(operation));
        }
        let expected = OperationId::ORDER
            .get(self.completed_operations.len())
            .copied()
            .ok_or(RunWorkspaceError::OperationAlreadyCompleted(operation))?;
        if operation != expected {
            return Err(RunWorkspaceError::OutOfOrder {
                expected,
                actual: operation,
            });
        }
        self.validate_capabilities(command, operation, &before)?;

        let run_result = execute_spine_command_attempt(
            executor,
            command,
            program,
            &self.boundary.root,
            environment,
        );
        self.failed_process = match &run_result {
            Ok(run) => Some(run.process().clone()),
            Err(error) => error.process().cloned(),
        };

        // Both post-run checks are attempted even when the process boundary
        // failed, so a runner error cannot conceal collateral filesystem work.
        let after_result = snapshot_workspace(&self.boundary.root);
        let root_result = self.boundary.verify();
        let after = after_result?;
        root_result?;
        verify_mutation_envelope(&self.boundary.root, &before, &after, command)?;
        let run = run_result.map_err(|error| RunWorkspaceError::Run(error.into_error()))?;
        if run.process().executable_identity().sha256() != self.recipe.expected_executable_sha256()
        {
            return Err(RunWorkspaceError::UntrustedEditorExecutable(operation));
        }
        if !operation_succeeded(operation, &run) {
            return Err(RunWorkspaceError::UnexpectedProcessFailure(operation));
        }

        for output in command.expected_outputs() {
            let relative = relative_to_root(&self.boundary.root, output.path())?;
            if output.mode() == OutputMode::CreatedFile
                && after
                    .entry(&relative)
                    .is_some_and(|entry| entry.kind() == EntryKind::File)
            {
                self.readable
                    .insert(relative, ReadableCapability::CreatedOutput(operation));
            }
        }
        self.completed_operations.insert(operation);
        self.last_snapshot = after.clone();
        self.failed_process = None;
        Ok(WorkspaceRunEvidence {
            before: before.evidence(),
            before_physical_sha256: before.physical_sha256(),
            run,
            after: after.evidence(),
            after_physical_sha256: after.physical_sha256(),
        })
    }

    fn validate_capabilities(
        &self,
        command: &SpineCommand,
        operation: OperationId,
        before: &WorkspaceSnapshot,
    ) -> Result<(), RunWorkspaceError> {
        for input in command.expected_inputs() {
            let relative = relative_to_root(&self.boundary.root, input.path())?;
            if !self.readable.contains_key(&relative)
                || before
                    .entry(&relative)
                    .is_none_or(|entry| entry.kind() != EntryKind::File)
            {
                return Err(RunWorkspaceError::UnreadableInput(
                    input.path().to_path_buf(),
                ));
            }
        }

        for output in command.expected_outputs() {
            let relative = relative_to_root(&self.boundary.root, output.path())?;
            let state = before.entry(&relative);
            match output.mode() {
                OutputMode::CreatedFile
                    if !self.created_slots.contains(&relative) || state.is_some() =>
                {
                    return Err(RunWorkspaceError::InvalidOutputCapability(
                        output.path().to_path_buf(),
                    ));
                }
                OutputMode::UpdatedFile
                    if !self.update_slots.contains(&relative)
                        || state.is_none_or(|entry| entry.kind() != EntryKind::File) =>
                {
                    return Err(RunWorkspaceError::InvalidOutputCapability(
                        output.path().to_path_buf(),
                    ));
                }
                OutputMode::CreatedFile | OutputMode::UpdatedFile => {}
            }
        }
        debug_assert_eq!(self.recipe.command(operation), command);
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct ExtractionLimits {
    json_bytes: u64,
    atlas_bytes: u64,
    page_bytes: u64,
    total_bytes: u64,
}

impl ExtractionLimits {
    const fn production() -> Self {
        Self {
            json_bytes: MAX_EXTRACTED_JSON_BYTES,
            atlas_bytes: MAX_EXTRACTED_ATLAS_BYTES,
            page_bytes: MAX_EXTRACTED_PAGE_BYTES,
            total_bytes: MAX_TOTAL_EXTRACTED_BYTES,
        }
    }
}

struct ExtractionBudget {
    limits: ExtractionLimits,
    used: u64,
}

impl ExtractionBudget {
    const fn new(limits: ExtractionLimits) -> Self {
        Self { limits, used: 0 }
    }

    fn reserve(&mut self, path: &str, size: u64, file_limit: u64) -> Result<(), RunWorkspaceError> {
        if size > file_limit {
            return Err(RunWorkspaceError::ExtractionByteLimit {
                path: PathBuf::from(path),
                limit: file_limit,
            });
        }
        self.used = self
            .used
            .checked_add(size)
            .filter(|total| *total <= self.limits.total_bytes)
            .ok_or(RunWorkspaceError::ExtractionTotalByteLimit)?;
        Ok(())
    }
}

struct BoundWorkspaceFile {
    workspace_path: String,
    bytes: Vec<u8>,
    sha256: String,
}

impl BoundWorkspaceFile {
    fn binding(&self, virtual_path: PathBuf) -> RuntimeFileBinding {
        RuntimeFileBinding {
            workspace_path: self.workspace_path.clone(),
            virtual_path,
            size: self.bytes.len() as u64,
            sha256: self.sha256.clone(),
        }
    }
}

fn phase0_json_sources(
    mut documents: BTreeMap<OperationId, BoundWorkspaceFile>,
    new_animation_collision: NewAnimationCollisionEvidence,
) -> Result<Phase0JsonSources, RunWorkspaceError> {
    fn take(
        documents: &mut BTreeMap<OperationId, BoundWorkspaceFile>,
        id: OperationId,
    ) -> Result<Vec<u8>, RunWorkspaceError> {
        documents.remove(&id).map(|file| file.bytes).ok_or_else(|| {
            RunWorkspaceError::ExtractionBinding(PathBuf::from(format!("operation/{id:?}")))
        })
    }

    let sources = Phase0JsonSources {
        current_a: take(&mut documents, OperationId::ExportCurrentA)?,
        replacement_submission: take(&mut documents, OperationId::ExportReplacementSubmission)?,
        new_submission: take(&mut documents, OperationId::ExportNewSubmission)?,
        reconstructed_a: take(&mut documents, OperationId::ExportReconstructedA)?,
        current_b: take(&mut documents, OperationId::ExportCurrentB)?,
        reconstructed_b: take(&mut documents, OperationId::ExportReconstructedB)?,
        existing_first: take(&mut documents, OperationId::ExportExistingFirst)?,
        existing_repeat: take(&mut documents, OperationId::ExportExistingRepeat)?,
        new_first: take(&mut documents, OperationId::ExportNewFirst)?,
        new_collision_control: take(&mut documents, OperationId::ExportNewCollisionControl)?,
        new_animation_collision,
    };
    if !documents.is_empty() {
        return Err(RunWorkspaceError::ExtractionBinding(PathBuf::from(
            "operation/unexpected-json-output",
        )));
    }
    Ok(sources)
}

fn source_completion(
    package: &StagedPackage,
    after_run: PackageInventory,
) -> SourcePackageCompletionEvidence {
    let (device, inode) = package.source_root_identity();
    SourcePackageCompletionEvidence {
        root_identity: SourceRootIdentityEvidence { device, inode },
        before_staging: package.source_before().clone(),
        after_staging: package.source_after().clone(),
        after_run,
    }
}

fn staged_completion(
    sealed: &WorkspaceSnapshot,
    final_state: &WorkspaceSnapshot,
) -> Result<StagedPackageSetCompletionEvidence, RunWorkspaceError> {
    Ok(StagedPackageSetCompletionEvidence {
        current: staged_role_completion(sealed, final_state, CURRENT_PATH)?,
        replacement_submission: staged_role_completion(sealed, final_state, REPLACEMENT_PATH)?,
        new_submission: staged_role_completion(sealed, final_state, NEW_SUBMISSION_PATH)?,
        existing_candidate: staged_role_completion(sealed, final_state, EXISTING_CANDIDATE_PATH)?,
        new_candidate: staged_role_completion(sealed, final_state, NEW_CANDIDATE_PATH)?,
        new_collision_control: staged_role_completion(
            sealed,
            final_state,
            NEW_COLLISION_CONTROL_PATH,
        )?,
        missing_images_control: staged_role_completion(sealed, final_state, MISSING_IMAGES_PATH)?,
    })
}

fn staged_role_completion(
    sealed: &WorkspaceSnapshot,
    final_state: &WorkspaceSnapshot,
    root: &str,
) -> Result<StagedPackageCompletionEvidence, RunWorkspaceError> {
    Ok(StagedPackageCompletionEvidence {
        sealed: sealed
            .subtree_evidence(root)
            .ok_or_else(|| RunWorkspaceError::ExtractionBinding(PathBuf::from(root)))?,
        final_state: final_state
            .subtree_evidence(root)
            .ok_or_else(|| RunWorkspaceError::ExtractionBinding(PathBuf::from(root)))?,
    })
}

fn read_bound_workspace_file(
    boundary: &WorkspaceBoundary,
    snapshot: &WorkspaceSnapshot,
    relative: &str,
    file_limit: u64,
    expected_operation_sha256: Option<&str>,
    budget: &mut ExtractionBudget,
) -> Result<BoundWorkspaceFile, RunWorkspaceError> {
    let state = snapshot
        .entry(relative)
        .filter(|state| state.kind() == EntryKind::File)
        .ok_or_else(|| RunWorkspaceError::ExtractionBinding(PathBuf::from(relative)))?;
    let snapshot_sha256 = state
        .sha256()
        .ok_or_else(|| RunWorkspaceError::ExtractionBinding(PathBuf::from(relative)))?;
    if expected_operation_sha256.is_some_and(|expected| expected != snapshot_sha256) {
        return Err(RunWorkspaceError::ExtractionBinding(PathBuf::from(
            relative,
        )));
    }
    budget.reserve(relative, state.size(), file_limit)?;
    let mut file = open_workspace_file(boundary, relative)?;
    if !state.matches_open_file(&file).map_err(|source| {
        workspace_io_error(
            "match opened extraction file to final snapshot",
            &boundary.root.join(relative),
            source,
        )
    })? {
        return Err(RunWorkspaceError::ExtractionBinding(PathBuf::from(
            relative,
        )));
    }

    let capacity =
        usize::try_from(state.size()).map_err(|_| RunWorkspaceError::ExtractionByteLimit {
            path: PathBuf::from(relative),
            limit: file_limit,
        })?;
    let mut bytes = Vec::with_capacity(capacity);
    Read::by_ref(&mut file)
        .take(file_limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|source| {
            workspace_io_error(
                "read fixed extraction file",
                &boundary.root.join(relative),
                source,
            )
        })?;
    if bytes.len() as u64 > file_limit {
        return Err(RunWorkspaceError::ExtractionByteLimit {
            path: PathBuf::from(relative),
            limit: file_limit,
        });
    }
    if bytes.len() as u64 != state.size()
        || !state.matches_open_file(&file).map_err(|source| {
            workspace_io_error(
                "recheck fixed extraction file",
                &boundary.root.join(relative),
                source,
            )
        })?
    {
        return Err(RunWorkspaceError::ExtractionBinding(PathBuf::from(
            relative,
        )));
    }
    let sha256 = sha256_bytes(&bytes);
    if sha256 != snapshot_sha256
        || expected_operation_sha256.is_some_and(|expected| expected != sha256)
    {
        return Err(RunWorkspaceError::ExtractionBinding(PathBuf::from(
            relative,
        )));
    }
    Ok(BoundWorkspaceFile {
        workspace_path: relative.to_owned(),
        bytes,
        sha256,
    })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn open_workspace_file(
    boundary: &WorkspaceBoundary,
    relative: &str,
) -> Result<File, RunWorkspaceError> {
    let relative = safe_relative(Path::new(relative))?;
    let mut components = relative.split('/').collect::<Vec<_>>();
    let file_name = components
        .pop()
        .ok_or_else(|| RunWorkspaceError::ExtractionBinding(PathBuf::from(&relative)))?;
    let mut directory = boundary.root_file.try_clone().map_err(|source| {
        workspace_io_error("clone retained workspace root", &boundary.root, source)
    })?;
    for component in components {
        let name = CString::new(component).map_err(|_| RunWorkspaceError::InvalidPath {
            path: PathBuf::from(&relative),
            reason: "must not contain NUL",
        })?;
        directory = openat(
            &directory,
            name.as_c_str(),
            OFlags::RDONLY
                | OFlags::DIRECTORY
                | OFlags::NOFOLLOW
                | OFlags::CLOEXEC
                | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map(File::from)
        .map_err(|error| {
            workspace_io_error(
                "open fixed extraction directory",
                &boundary.root.join(&relative),
                error.into(),
            )
        })?;
    }
    let name = CString::new(file_name).map_err(|_| RunWorkspaceError::InvalidPath {
        path: PathBuf::from(&relative),
        reason: "must not contain NUL",
    })?;
    openat(
        &directory,
        name.as_c_str(),
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(|error| {
        workspace_io_error(
            "open fixed extraction file",
            &boundary.root.join(relative),
            error.into(),
        )
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn open_workspace_file(
    _boundary: &WorkspaceBoundary,
    _relative: &str,
) -> Result<File, RunWorkspaceError> {
    Err(RunWorkspaceError::UnsupportedPlatform)
}

struct WorkspaceBoundary {
    root: PathBuf,
    root_file: File,
    root_identity: RootIdentity,
}

impl WorkspaceBoundary {
    fn create(destination: &Path) -> Result<Self, RunWorkspaceError> {
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = destination;
            return Err(RunWorkspaceError::UnsupportedPlatform);
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            validate_absolute_destination(destination)?;
            match fs::symlink_metadata(destination) {
                Ok(_) => return Err(RunWorkspaceError::AlreadyExists(destination.to_path_buf())),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(source) => return workspace_io("inspect fresh run root", destination, source),
            }
            fs::create_dir(destination).map_err(|source| {
                workspace_io_error("create private run root", destination, source)
            })?;
            make_directory_private(destination)?;
            let root = fs::canonicalize(destination).map_err(|source| {
                workspace_io_error("canonicalize private run root", destination, source)
            })?;
            let root_file = open_directory_no_follow(&root)
                .map_err(|source| workspace_io_error("open private run root", &root, source))?;
            let root_identity = root_identity(&root_file, &root)?;
            require_private_root(&root, &root_identity)?;
            Ok(Self {
                root,
                root_file,
                root_identity,
            })
        }
    }

    fn verify(&self) -> Result<(), RunWorkspaceError> {
        let held = root_identity(&self.root_file, &self.root)?;
        if held != self.root_identity {
            return Err(RunWorkspaceError::RootIdentityChanged(self.root.clone()));
        }
        let metadata = fs::symlink_metadata(&self.root).map_err(|source| {
            workspace_io_error("reinspect private run root", &self.root, source)
        })?;
        if metadata.file_type().is_symlink()
            || RootIdentity::from_metadata(&metadata) != self.root_identity
        {
            return Err(RunWorkspaceError::RootIdentityChanged(self.root.clone()));
        }
        require_private_root(&self.root, &held)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RootIdentity {
    device: u64,
    inode: u64,
    mode: u32,
    owner: u32,
}

impl RootIdentity {
    #[cfg(unix)]
    fn from_metadata(metadata: &Metadata) -> Self {
        use std::os::unix::fs::MetadataExt as _;
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            mode: metadata.mode(),
            owner: metadata.uid(),
        }
    }

    #[cfg(not(unix))]
    fn from_metadata(_metadata: &Metadata) -> Self {
        Self {
            device: 0,
            inode: 0,
            mode: 0,
            owner: 0,
        }
    }
}

fn build_output_capabilities(
    root: &Path,
    recipe: &OperationRecipe,
    readable: &BTreeMap<String, ReadableCapability>,
) -> Result<(BTreeSet<String>, BTreeSet<String>), RunWorkspaceError> {
    let mut created = BTreeSet::new();
    let mut updated = BTreeSet::new();
    for id in OperationId::ORDER {
        for output in recipe.command(id).expected_outputs() {
            let relative = relative_to_root(root, output.path())?;
            match output.mode() {
                OutputMode::CreatedFile => {
                    let output_is_absent = match fs::symlink_metadata(output.path()) {
                        Err(error) if error.kind() == io::ErrorKind::NotFound => true,
                        Ok(_) => false,
                        Err(source) => {
                            return workspace_io(
                                "inspect create-only output slot",
                                output.path(),
                                source,
                            );
                        }
                    };
                    if !output_is_absent
                        || !created.insert(relative.clone())
                        || readable.contains_key(&relative)
                    {
                        return Err(RunWorkspaceError::InvalidLayout(format!(
                            "create-only output slot `{relative}` is not unique and absent"
                        )));
                    }
                }
                OutputMode::UpdatedFile => {
                    let role = readable.get(&relative);
                    if !matches!(
                        role,
                        Some(ReadableCapability::Package(
                            PackageRole::ExistingCandidate
                                | PackageRole::NewCandidate
                                | PackageRole::NewCollisionControl
                        ))
                    ) {
                        return Err(RunWorkspaceError::InvalidLayout(format!(
                            "update-only output slot `{relative}` is not a candidate project"
                        )));
                    }
                    updated.insert(relative);
                }
            }
        }
    }
    Ok((created, updated))
}

fn validate_recipe_inputs(
    root: &Path,
    recipe: &OperationRecipe,
    readable: &BTreeMap<String, ReadableCapability>,
    created: &BTreeSet<String>,
) -> Result<(), RunWorkspaceError> {
    for id in OperationId::ORDER {
        for input in recipe.command(id).expected_inputs() {
            let relative = relative_to_root(root, input.path())?;
            if !readable.contains_key(&relative) && !created.contains(&relative) {
                return Err(RunWorkspaceError::InvalidLayout(format!(
                    "recipe input `{relative}` is neither staged nor produced"
                )));
            }
        }
    }
    Ok(())
}

fn insert_readable(
    root: &Path,
    path: &Path,
    capability: ReadableCapability,
    readable: &mut BTreeMap<String, ReadableCapability>,
) -> Result<(), RunWorkspaceError> {
    let relative = relative_to_root(root, path)?;
    if readable.insert(relative.clone(), capability).is_some() {
        return Err(RunWorkspaceError::InvalidLayout(format!(
            "readable path `{relative}` has multiple roles"
        )));
    }
    Ok(())
}

fn require_exact_images_contract(package: &PackageSpec) -> Result<(), RunWorkspaceError> {
    let exact = Path::new(IMAGES_PATH);
    if !package
        .required_directories
        .iter()
        .any(|path| path == exact)
        || !package.asset_roots.iter().any(|path| path == exact)
    {
        return Err(RunWorkspaceError::InvalidLayout(
            "current package must declare exact root-level `images` required and asset paths"
                .to_owned(),
        ));
    }
    Ok(())
}

fn require_runtime_atlas(
    current: &PackageInventory,
    runtime_atlas: &Path,
) -> Result<(), RunWorkspaceError> {
    let path = safe_relative(runtime_atlas)?;
    if !current
        .entries
        .iter()
        .any(|entry| entry.path == path && entry.kind == EntryKind::File)
    {
        return Err(RunWorkspaceError::InvalidLayout(format!(
            "current package is missing runtime atlas `{path}`"
        )));
    }
    Ok(())
}

fn operation_succeeded(operation: OperationId, run: &SpineRunEvidence) -> bool {
    let process = run.process();
    if operation == OperationId::ImportNewCollisionControl {
        if run.operation_kind()
            != crate::spine_cli::SpineOperationKind::NewAnimationCollisionControl
            || process.transcript_profile() != TranscriptProfile::NewAnimationCollisionControl
            || process.exit_code() != Some(0)
            || process.new_animation_collision().is_none()
        {
            return false;
        }
        let failures = process.assessment().failures();
        let exact_failure =
            failures.len() == 1 && failures[0].code == ProcessFailureCode::BlockingDiagnostic;
        let exact_mutation = run.outputs().first().is_some_and(|output| {
            run.outputs().len() == 1
                && output.mode() == OutputMode::UpdatedFile
                && output
                    .before()
                    .zip(output.after())
                    .is_some_and(|(before, after)| before.sha256() != after.sha256())
        });
        return !process.assessment().passed() && exact_failure && exact_mutation;
    }
    if operation != OperationId::MissingImagesPathControl {
        return process.assessment().passed();
    }
    if run.operation_kind() != crate::spine_cli::SpineOperationKind::MissingImagesPathControl
        || process.transcript_profile() != TranscriptProfile::MissingImagesPathControl
        || process.exit_code() != Some(0)
    {
        return false;
    }
    let failures = process.assessment().failures();
    let blocking = failures
        .iter()
        .filter(|failure| failure.code == ProcessFailureCode::BlockingDiagnostic)
        .count();
    let missing = failures
        .iter()
        .filter(|failure| failure.code == ProcessFailureCode::MissingOutput)
        .count();
    blocking == 1 && missing <= 1 && failures.len() == blocking + missing
}

fn require_identical_package(
    expected: &PackageInventory,
    actual: &PackageInventory,
    role: &str,
) -> Result<(), RunWorkspaceError> {
    if expected != actual {
        return Err(RunWorkspaceError::InvalidLayout(format!(
            "{role} must be a byte-identical complete copy of current"
        )));
    }
    Ok(())
}

fn require_only_images_removed(
    current: &PackageInventory,
    missing: &PackageInventory,
) -> Result<(), RunWorkspaceError> {
    let expected = current
        .entries
        .iter()
        .filter(|entry| entry.path != IMAGES_PATH && !entry.path.starts_with("images/"))
        .cloned()
        .collect::<Vec<_>>();
    if expected.len() == current.entries.len()
        || expected != missing.entries
        || missing
            .entries
            .iter()
            .any(|entry| entry.path == IMAGES_PATH || entry.path.starts_with("images/"))
    {
        return Err(RunWorkspaceError::InvalidLayout(
            "missing-images control must differ only by removal of the exact `images` tree"
                .to_owned(),
        ));
    }
    Ok(())
}

fn verify_mutation_envelope(
    root: &Path,
    before: &WorkspaceSnapshot,
    after: &WorkspaceSnapshot,
    command: &SpineCommand,
) -> Result<(), RunWorkspaceError> {
    let outputs = command
        .expected_outputs()
        .iter()
        .map(|output| (output.path(), output.mode()))
        .collect::<Vec<_>>();
    let mut output_paths = BTreeMap::new();
    for (path, mode) in outputs {
        let relative = relative_to_root(root, path)?;
        output_paths.insert(relative, mode);
    }

    let paths = before
        .entries()
        .keys()
        .chain(after.entries().keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for path in paths {
        let before_entry = before.entry(&path);
        let after_entry = after.entry(&path);
        match output_paths.get(&path) {
            Some(OutputMode::CreatedFile)
                if before_entry.is_some()
                    || after_entry.is_some_and(|entry| entry.kind() != EntryKind::File) =>
            {
                return Err(RunWorkspaceError::MutationEnvelope(PathBuf::from(path)));
            }
            Some(OutputMode::UpdatedFile)
                if before_entry.is_none_or(|entry| entry.kind() != EntryKind::File)
                    || after_entry.is_none_or(|entry| entry.kind() != EntryKind::File) =>
            {
                return Err(RunWorkspaceError::MutationEnvelope(PathBuf::from(path)));
            }
            Some(OutputMode::CreatedFile | OutputMode::UpdatedFile) => {}
            None if before_entry != after_entry => {
                return Err(RunWorkspaceError::MutationEnvelope(PathBuf::from(path)));
            }
            None => {}
        }
    }
    Ok(())
}

fn first_snapshot_difference(
    expected: &WorkspaceSnapshot,
    actual: &WorkspaceSnapshot,
) -> Option<PathBuf> {
    expected
        .entries()
        .keys()
        .chain(actual.entries().keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .find(|path| expected.entry(path) != actual.entry(path))
        .map(PathBuf::from)
}

fn relative_to_root(root: &Path, path: &Path) -> Result<String, RunWorkspaceError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| RunWorkspaceError::CommandEscaped(path.to_path_buf()))?;
    safe_relative(relative)
}

fn safe_relative(path: &Path) -> Result<String, RunWorkspaceError> {
    let text = path
        .to_str()
        .ok_or_else(|| RunWorkspaceError::InvalidPath {
            path: path.to_path_buf(),
            reason: "must be UTF-8",
        })?;
    if text.is_empty()
        || path.is_absolute()
        || text.contains('\\')
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(RunWorkspaceError::InvalidPath {
            path: path.to_path_buf(),
            reason: "must be a normalized portable relative path",
        });
    }
    Ok(text.to_owned())
}

fn validate_absolute_destination(path: &Path) -> Result<(), RunWorkspaceError> {
    if !path.is_absolute()
        || path.components().any(|component| {
            !matches!(
                component,
                Component::RootDir | Component::Prefix(_) | Component::Normal(_)
            )
        })
    {
        return Err(RunWorkspaceError::InvalidPath {
            path: path.to_path_buf(),
            reason: "must be an absolute normalized path",
        });
    }
    Ok(())
}

fn require_private_directory(path: &Path) -> Result<(), RunWorkspaceError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|source| workspace_io_error("inspect private directory", path, source))?;
    require_private_directory_metadata(path, &metadata)
}

fn require_private_directory_metadata(
    path: &Path,
    metadata: &Metadata,
) -> Result<(), RunWorkspaceError> {
    let identity = RootIdentity::from_metadata(metadata);
    if metadata.file_type().is_symlink()
        || !metadata.file_type().is_dir()
        || identity.owner != effective_user_id()
        || identity.mode & 0o077 != 0
        || identity.mode & 0o700 != 0o700
    {
        return Err(RunWorkspaceError::InsecureDirectory(path.to_path_buf()));
    }
    Ok(())
}

fn require_private_file_metadata(
    path: &Path,
    metadata: &Metadata,
) -> Result<(), RunWorkspaceError> {
    let identity = RootIdentity::from_metadata(metadata);
    if !metadata.file_type().is_file()
        || identity.owner != effective_user_id()
        || identity.mode & 0o077 != 0
        || identity.mode & 0o600 != 0o600
    {
        return Err(RunWorkspaceError::InsecureDirectory(path.to_path_buf()));
    }
    Ok(())
}

fn require_private_root(path: &Path, identity: &RootIdentity) -> Result<(), RunWorkspaceError> {
    if identity.owner != effective_user_id()
        || identity.mode & 0o077 != 0
        || identity.mode & 0o700 != 0o700
    {
        return Err(RunWorkspaceError::InsecureDirectory(path.to_path_buf()));
    }
    Ok(())
}

#[cfg(unix)]
fn effective_user_id() -> u32 {
    rustix::process::geteuid().as_raw()
}

#[cfg(not(unix))]
fn effective_user_id() -> u32 {
    0
}

#[cfg(unix)]
fn make_directory_private(path: &Path) -> Result<(), RunWorkspaceError> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|source| workspace_io_error("set private directory permissions", path, source))?;
    require_private_directory(path)
}

#[cfg(not(unix))]
fn make_directory_private(_path: &Path) -> Result<(), RunWorkspaceError> {
    Err(RunWorkspaceError::UnsupportedPlatform)
}

#[cfg(unix)]
fn open_directory_no_follow(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt as _;
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_DIRECTORY)
        .open(path)
}

#[cfg(not(unix))]
fn open_directory_no_follow(_path: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "private run workspaces require Unix no-follow directory opens",
    ))
}

fn root_identity(file: &File, path: &Path) -> Result<RootIdentity, RunWorkspaceError> {
    let metadata = file
        .metadata()
        .map_err(|source| workspace_io_error("inspect opened private run root", path, source))?;
    if !metadata.file_type().is_dir() {
        return Err(RunWorkspaceError::InsecureDirectory(path.to_path_buf()));
    }
    Ok(RootIdentity::from_metadata(&metadata))
}

fn workspace_io<T>(
    operation: &'static str,
    path: &Path,
    source: io::Error,
) -> Result<T, RunWorkspaceError> {
    Err(workspace_io_error(operation, path, source))
}

fn workspace_io_error(
    operation: &'static str,
    path: &Path,
    source: io::Error,
) -> RunWorkspaceError {
    RunWorkspaceError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::case::parse_case;
    use crate::digest::sha256_bytes;
    use crate::process::{
        ExecutableIdentity, ProcessCapture, ProcessExecutionError, ProcessRequest,
        ProcessStreamCapture, tests::capture,
    };
    use serde_json::{Map, Value, json};
    use std::cell::Cell;
    use std::fs;
    use std::os::unix::fs::symlink;

    const ADVANCED_HELP: &str = include_str!("../policy/spine-4.3.23-advanced-help.txt");
    const ATLAS_BYTES: &[u8] = b"images/page.png\n\tsize: 1, 1\n\tformat: RGBA8888\n\tfilter: Linear, Linear\n\trepeat: none\n\tpma: false\nbody\n\tbounds: 0, 0, 1, 1\n";
    const PNG_BYTES: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 96, 96, 248, 255, 31,
        0, 3, 2, 1, 255, 230, 119, 11, 174, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];

    struct Fixture {
        _temporary: tempfile::TempDir,
        case: LoadedCase,
        current_source: PathBuf,
        run_root: PathBuf,
    }

    impl Fixture {
        fn new(current_atlas: bool) -> Self {
            let temporary = tempfile::tempdir().expect("temporary fixture");
            let current_source = write_package(
                temporary.path(),
                "current-source",
                b"current project",
                current_atlas,
            );
            let replacement = write_package(
                temporary.path(),
                "replacement-source",
                b"replacement project",
                true,
            );
            let new_submission =
                write_package(temporary.path(), "new-source", b"new project", true);
            let manifest = format!(
                r#"
format_version = 2
case_id = "workspace-contract"
target_spine_version = "4.3.23"
runtime_atlas = "character.atlas"

[editor]
expected_executable_sha256 = "0000000000000000000000000000000000000000000000000000000000000000"

[packages.current]
root = "{}"
project = "character.spine"
required_directories = ["images"]
asset_roots = ["images"]

[packages.replacement_submission]
root = "{}"
project = "character.spine"
required_directories = ["images"]
asset_roots = ["images"]

[packages.new_submission]
root = "{}"
project = "character.spine"
required_directories = ["images"]
asset_roots = ["images"]

[skeletons]
current = "Current Rig"
replacement_submission = "Replacement Rig"
new_submission = "New Rig"

[animations]
replacement = "idle"
new = "gesture"

[export]
preset = "pretty-nonessential-json"

[volatile]
approved_json_pointers = ["/skeleton/hash"]
"#,
                current_source.display(),
                replacement.display(),
                new_submission.display(),
            );
            let case = parse_case(&manifest).expect("valid test case");
            let run_root = temporary.path().join("run");
            Self {
                _temporary: temporary,
                case,
                current_source,
                run_root,
            }
        }

        fn seal(&self) -> Result<RunWorkspace, RunWorkspaceError> {
            WorkspacePreparation::create(&self.run_root)?.seal(&self.case)
        }
    }

    fn write_package(parent: &Path, name: &str, project: &[u8], atlas: bool) -> PathBuf {
        let root = parent.join(name);
        fs::create_dir(&root).expect("package root");
        fs::create_dir(root.join("images")).expect("images root");
        fs::write(root.join("images/page.png"), PNG_BYTES).expect("image");
        fs::write(root.join("character.spine"), project).expect("project");
        if atlas {
            fs::write(root.join("character.atlas"), ATLAS_BYTES).expect("atlas");
        }
        root
    }

    struct FakeExecutor {
        writes: Vec<(PathBuf, Vec<u8>)>,
        outcome: FakeOutcome,
        calls: Cell<usize>,
    }

    enum FakeOutcome {
        Passing,
        InvalidTranscript,
        RunnerError,
    }

    impl FakeExecutor {
        fn passing(writes: Vec<(PathBuf, Vec<u8>)>) -> Self {
            Self {
                writes,
                outcome: FakeOutcome::Passing,
                calls: Cell::new(0),
            }
        }

        fn invalid_transcript(writes: Vec<(PathBuf, Vec<u8>)>) -> Self {
            Self {
                writes,
                outcome: FakeOutcome::InvalidTranscript,
                calls: Cell::new(0),
            }
        }

        fn runner_error(writes: Vec<(PathBuf, Vec<u8>)>) -> Self {
            Self {
                writes,
                outcome: FakeOutcome::RunnerError,
                calls: Cell::new(0),
            }
        }
    }

    impl ProcessExecutor for FakeExecutor {
        fn execute(
            &self,
            request: &ProcessRequest,
        ) -> Result<ProcessCapture, ProcessExecutionError> {
            self.calls.set(self.calls.get() + 1);
            for (path, bytes) in &self.writes {
                fs::write(path, bytes).expect("fake executor output");
            }
            match self.outcome {
                FakeOutcome::Passing => Ok(passing_capture(request)),
                FakeOutcome::InvalidTranscript => {
                    let mut value = capture();
                    value.stdout = complete_stream(b"unreviewed transcript\n");
                    Ok(value)
                }
                FakeOutcome::RunnerError => Err(ProcessExecutionError::new("runner failed")),
            }
        }
    }

    struct WrongExecutableExecutor {
        calls: Cell<usize>,
    }

    impl ProcessExecutor for WrongExecutableExecutor {
        fn execute(
            &self,
            request: &ProcessRequest,
        ) -> Result<ProcessCapture, ProcessExecutionError> {
            self.calls.set(self.calls.get() + 1);
            let mut value = passing_capture(request);
            value.executable_identity = ExecutableIdentity::new(
                PathBuf::from("/evidence/different-editor"),
                "1".repeat(64),
                6,
                1,
                2,
                0o100700,
                0,
                0,
                0,
                0,
                0,
            );
            Ok(value)
        }
    }

    struct CompleteExecutor {
        calls: Cell<usize>,
        wrong_skeleton: Option<OperationId>,
    }

    impl CompleteExecutor {
        fn passing() -> Self {
            Self {
                calls: Cell::new(0),
                wrong_skeleton: None,
            }
        }

        fn with_wrong_skeleton(id: OperationId) -> Self {
            Self {
                calls: Cell::new(0),
                wrong_skeleton: Some(id),
            }
        }
    }

    impl ProcessExecutor for CompleteExecutor {
        fn execute(
            &self,
            request: &ProcessRequest,
        ) -> Result<ProcessCapture, ProcessExecutionError> {
            let index = self.calls.get();
            let id = *OperationId::ORDER.get(index).expect("fixed operation call");
            self.calls.set(index + 1);
            if let Some(output) = complete_operation_output_path(request, id) {
                fs::write(output, operation_output_bytes(id)).expect("complete output");
            }
            Ok(complete_capture(
                request,
                id,
                self.wrong_skeleton == Some(id),
            ))
        }
    }

    fn complete_operation_output_path(
        request: &ProcessRequest,
        id: OperationId,
    ) -> Option<PathBuf> {
        match id {
            OperationId::ExportCurrentA
            | OperationId::ExportReplacementSubmission
            | OperationId::ExportNewSubmission
            | OperationId::ExportReconstructedA
            | OperationId::ExportCurrentB
            | OperationId::ExportReconstructedB
            | OperationId::ExportExistingFirst
            | OperationId::ExportExistingRepeat
            | OperationId::ExportNewFirst
            | OperationId::ExportNewCollisionControl => {
                let directory = argument_value(request, "--output")?;
                let skeleton = match id {
                    OperationId::ExportReplacementSubmission => "Replacement Rig",
                    OperationId::ExportNewSubmission => "New Rig",
                    OperationId::ExportCurrentA
                    | OperationId::ExportReconstructedA
                    | OperationId::ExportCurrentB
                    | OperationId::ExportReconstructedB
                    | OperationId::ExportExistingFirst
                    | OperationId::ExportExistingRepeat
                    | OperationId::ExportNewFirst => "Current Rig",
                    OperationId::ExportNewCollisionControl => "New Rig",
                    _ => unreachable!("matched only JSON export operations"),
                };
                Some(Path::new(directory).join(format!("{skeleton}.json")))
            }
            OperationId::ReconstructA
            | OperationId::ReconstructB
            | OperationId::ImportExistingFirst
            | OperationId::ImportExistingRepeat
            | OperationId::ImportNewFirst
            | OperationId::ImportNewCollisionControl => {
                argument_value(request, "--output").map(PathBuf::from)
            }
            OperationId::Version
            | OperationId::AdvancedHelp
            | OperationId::InfoCurrent
            | OperationId::InfoReplacement
            | OperationId::InfoNew
            | OperationId::MissingImagesPathControl => None,
        }
    }

    fn operation_output_bytes(id: OperationId) -> Vec<u8> {
        match id {
            OperationId::ExportCurrentA | OperationId::ExportCurrentB => {
                project_json("current-hash", [("idle", 0)])
            }
            OperationId::ExportReplacementSubmission => {
                project_json("replacement-hash", [("idle", 10)])
            }
            OperationId::ExportNewSubmission => {
                project_json("new-submission-hash", [("gesture", 20), ("idle", 0)])
            }
            OperationId::ExportReconstructedA | OperationId::ExportReconstructedB => {
                project_json("reconstructed-hash", [("idle", 0)])
            }
            OperationId::ExportExistingFirst | OperationId::ExportExistingRepeat => {
                project_json("existing-hash", [("idle", 10)])
            }
            OperationId::ExportNewFirst => {
                project_json("new-candidate-hash", [("gesture", 20), ("idle", 0)])
            }
            OperationId::ExportNewCollisionControl => project_json(
                "new-collision-control-hash",
                [("gesture", 20), ("gesture2", 20), ("idle", 0)],
            ),
            OperationId::ReconstructA | OperationId::ReconstructB => {
                b"reconstructed project".to_vec()
            }
            OperationId::ImportExistingFirst | OperationId::ImportExistingRepeat => {
                b"existing candidate".to_vec()
            }
            OperationId::ImportNewFirst => b"new candidate".to_vec(),
            OperationId::ImportNewCollisionControl => b"new collision candidate".to_vec(),
            OperationId::Version
            | OperationId::AdvancedHelp
            | OperationId::InfoCurrent
            | OperationId::InfoReplacement
            | OperationId::InfoNew
            | OperationId::MissingImagesPathControl => Vec::new(),
        }
    }

    fn project_json<const N: usize>(hash: &str, values: [(&str, i64); N]) -> Vec<u8> {
        let animations = values
            .into_iter()
            .map(|(name, value)| {
                (
                    name.to_owned(),
                    json!({"bones": {"root": {"rotate": [{"time": 0, "value": value}]}}}),
                )
            })
            .collect::<Map<String, Value>>();
        serde_json::to_vec_pretty(&json!({
            "skeleton": {"hash": hash, "spine": "4.3.23", "x": 0, "y": 0},
            "bones": [{"name": "root"}],
            "slots": [{"name": "body", "bone": "root"}],
            "skins": [{"name": "default", "attachments": {}}],
            "animations": animations,
        }))
        .expect("runtime JSON")
    }

    fn argument_value<'a>(request: &'a ProcessRequest, name: &str) -> Option<&'a str> {
        request
            .args
            .windows(2)
            .find(|pair| pair[0] == name)
            .map(|pair| pair[1].as_str())
    }

    fn complete_capture(
        request: &ProcessRequest,
        id: OperationId,
        wrong_skeleton: bool,
    ) -> ProcessCapture {
        let body = match id {
            OperationId::Version => reviewed_session("Complete.\n"),
            OperationId::AdvancedHelp => reviewed_header(&format!("\n{ADVANCED_HELP}")),
            OperationId::InfoCurrent | OperationId::InfoReplacement | OperationId::InfoNew => {
                let input = argument_value(request, "--input").expect("project info input");
                let skeleton = if wrong_skeleton {
                    "Wrong"
                } else {
                    match id {
                        OperationId::InfoCurrent => "Current Rig",
                        OperationId::InfoReplacement => "Replacement Rig",
                        OperationId::InfoNew => "New Rig",
                        _ => unreachable!("matched only project-info operations"),
                    }
                };
                reviewed_session(&format!(
                    concat!(
                        "Project info: {}\n",
                        "  Spine version: 4.3.23\n",
                        "  Dopesheet FPS: 30\n",
                        "  Skeleton: {}\n",
                        "    Size: <unknown>\n",
                        "    Bones (1): root\n",
                        "    Slots (1): body\n",
                        "    Animations (1): idle\n",
                        "Complete.\n"
                    ),
                    input, skeleton
                ))
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
            | OperationId::ExportNewCollisionControl => reviewed_session(&format!(
                "JSON export: {}\nComplete.\n",
                input_stem(request)
            )),
            OperationId::ReconstructA | OperationId::ReconstructB => {
                let input = argument_value(request, "--input").expect("reconstruct input");
                let output = argument_value(request, "--output").expect("reconstruct output");
                reviewed_session(&format!(
                    "Project import: {} into {}\nComplete.\n",
                    Path::new(input)
                        .file_name()
                        .and_then(|value| value.to_str())
                        .expect("input filename"),
                    Path::new(output)
                        .file_stem()
                        .and_then(|value| value.to_str())
                        .expect("output stem")
                ))
            }
            OperationId::ImportExistingFirst
            | OperationId::ImportExistingRepeat
            | OperationId::ImportNewFirst => {
                let input = argument_value(request, "--input").expect("import input");
                let output = argument_value(request, "--output").expect("import output");
                let destination = argument_value(request, "--to").expect("destination skeleton");
                let animation = argument_value(request, "--animation").expect("animation");
                reviewed_session(&format!(
                    concat!(
                        "Animation import: {} into {} ({})\n",
                        "Imported animation: {}\n",
                        "Complete.\n"
                    ),
                    Path::new(input)
                        .file_stem()
                        .and_then(|value| value.to_str())
                        .expect("input stem"),
                    Path::new(output)
                        .file_stem()
                        .and_then(|value| value.to_str())
                        .expect("output stem"),
                    destination,
                    animation
                ))
            }
            OperationId::ImportNewCollisionControl => {
                let input = argument_value(request, "--input").expect("import input");
                let output = argument_value(request, "--output").expect("import output");
                let destination = argument_value(request, "--to").expect("destination skeleton");
                let animation = argument_value(request, "--animation").expect("animation");
                reviewed_session(&format!(
                    concat!(
                        "Animation import: {} into {} ({})\n",
                        "Imported animation: {}\n",
                        "An animation with this name already exists: {} -> {}2\n",
                        "Complete.\n"
                    ),
                    Path::new(input)
                        .file_stem()
                        .and_then(|value| value.to_str())
                        .expect("input stem"),
                    Path::new(output)
                        .file_stem()
                        .and_then(|value| value.to_str())
                        .expect("output stem"),
                    destination,
                    animation,
                    animation,
                    animation,
                ))
            }
            OperationId::MissingImagesPathControl => reviewed_session(&format!(
                "JSON export: {}\nImages path not found: ./images\nComplete.\n",
                input_stem(request)
            )),
        };
        let mut value = capture();
        value.stdout = complete_stream(body.as_bytes());
        value
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

    fn reviewed_header(body: &str) -> String {
        format!(
            concat!(
                "Spine Launcher 4.3.06 (macOS Apple Silicon)\n",
                "Esoteric Software LLC (C) 2013-2026 | http://esotericsoftware.com\n",
                "Mac OS X aarch64 26.5.2\n",
                "{}"
            ),
            body
        )
    }

    fn reviewed_session(body: &str) -> String {
        reviewed_header(&format!(
            concat!(
                "Starting: Spine 4.3.23 Professional\n",
                "Spine 4.3.23 Professional\n",
                "Licensed to: <hidden>\n",
                "{}"
            ),
            body
        ))
    }

    fn input_stem(request: &ProcessRequest) -> &str {
        request
            .args
            .windows(2)
            .find(|pair| pair[0] == "--input")
            .and_then(|pair| Path::new(&pair[1]).file_stem())
            .and_then(|stem| stem.to_str())
            .expect("input stem")
    }

    fn passing_capture(request: &ProcessRequest) -> ProcessCapture {
        let body = match request.operation.as_str() {
            "spine-version" => "Complete.\n".to_owned(),
            "spine-export-json" => format!("JSON export: {}\nComplete.\n", input_stem(request)),
            "spine-missing-images-path-control" => format!(
                "JSON export: {}\nImages path not found: ./images\nComplete.\n",
                input_stem(request)
            ),
            operation => panic!("test capture does not support {operation}"),
        };
        let mut value = capture();
        value.stdout = complete_stream(reviewed_session(&body).as_bytes());
        value
    }

    fn program(workspace: &RunWorkspace) -> PathBuf {
        workspace.root().join("Spine")
    }

    fn complete_workspace(fixture: &Fixture, executor: &CompleteExecutor) -> RunWorkspace {
        let mut workspace = fixture.seal().expect("sealed workspace");
        let selected_program = program(&workspace);
        for id in OperationId::ORDER {
            workspace
                .execute(executor, id, &selected_program, BTreeMap::new())
                .unwrap_or_else(|error| panic!("{id:?} failed: {error}"));
        }
        assert_eq!(executor.calls.get(), OperationId::ORDER.len());
        workspace
    }

    fn mark_completed_before(workspace: &mut RunWorkspace, operation: OperationId) {
        let index = OperationId::ORDER
            .iter()
            .position(|candidate| *candidate == operation)
            .expect("operation in fixed order");
        workspace
            .completed_operations
            .extend(OperationId::ORDER[..index].iter().copied());
    }

    fn first_output(workspace: &RunWorkspace, operation: OperationId) -> PathBuf {
        workspace.recipe.command(operation).expected_outputs()[0]
            .path()
            .to_path_buf()
    }

    #[test]
    fn seal_builds_only_the_fixed_roles_and_exact_negative_control() {
        let fixture = Fixture::new(true);
        let workspace = fixture.seal().expect("sealed workspace");
        let root = workspace.root();
        assert_eq!(
            fs::read(root.join("packages/current/character.spine")).expect("current"),
            fs::read(root.join("packages/existing-candidate/character.spine"))
                .expect("existing candidate")
        );
        assert_eq!(
            fs::read(root.join("packages/current/character.spine")).expect("current"),
            fs::read(root.join("packages/new-candidate/character.spine")).expect("new candidate")
        );
        assert_eq!(
            fs::read(root.join("packages/new-submission/character.spine")).expect("new submission"),
            fs::read(root.join("packages/new-collision-control/character.spine"))
                .expect("collision control")
        );
        assert!(!root.join("packages/missing-images-control/images").exists());
        assert_eq!(
            fs::read(root.join("packages/missing-images-control/character.atlas"))
                .expect("negative control atlas"),
            ATLAS_BYTES
        );
        assert_eq!(workspace.readable.len(), 8);
        assert_eq!(workspace.created_slots.len(), 13);
        assert_eq!(workspace.update_slots.len(), 3);
        assert!(
            workspace
                .update_slots
                .contains("packages/new-collision-control/character.spine")
        );
        for operation in [OperationId::ReconstructA, OperationId::ReconstructB] {
            let output = first_output(&workspace, operation);
            assert_eq!(
                output.parent(),
                Some(root.join("packages/current").as_path())
            );
            let relative = relative_to_root(root, &output).expect("relative reconstruction");
            assert!(workspace.created_slots.contains(&relative));
            assert!(!output.exists());
        }
        assert!(
            !workspace
                .update_slots
                .contains("packages/current/character.spine")
        );
    }

    #[test]
    fn seal_rejects_a_missing_runtime_atlas() {
        let fixture = Fixture::new(false);
        assert!(matches!(
            fixture.seal(),
            Err(RunWorkspaceError::InvalidLayout(message)) if message.contains("runtime atlas")
        ));
    }

    #[test]
    fn seal_rejects_a_reconstruction_slot_collision_in_the_current_package() {
        let fixture = Fixture::new(true);
        fs::write(
            fixture.current_source.join("phase0a-round-trip-a.spine"),
            b"preexisting project",
        )
        .expect("reserved path collision");
        assert!(matches!(
            fixture.seal(),
            Err(RunWorkspaceError::InvalidLayout(message))
                if message.contains("create-only output slot")
        ));
    }

    #[test]
    fn exact_created_output_is_registered_only_after_success() {
        let fixture = Fixture::new(true);
        let mut workspace = fixture.seal().expect("sealed workspace");
        mark_completed_before(&mut workspace, OperationId::ExportCurrentA);
        let output = first_output(&workspace, OperationId::ExportCurrentA);
        let executor = FakeExecutor::passing(vec![(output.clone(), b"json bytes".to_vec())]);
        let selected_program = program(&workspace);
        let evidence = workspace
            .execute(
                &executor,
                OperationId::ExportCurrentA,
                selected_program,
                BTreeMap::new(),
            )
            .expect("successful fixed export");
        assert!(evidence.run().process().assessment().passed());
        let relative = relative_to_root(workspace.root(), &output).expect("relative output");
        assert!(matches!(
            workspace.readable.get(&relative),
            Some(ReadableCapability::CreatedOutput(
                OperationId::ExportCurrentA
            ))
        ));
        assert_eq!(executor.calls.get(), 1);
    }

    #[test]
    fn out_of_order_submission_poisoned_without_launching() {
        let fixture = Fixture::new(true);
        let mut workspace = fixture.seal().expect("sealed workspace");
        let executor = FakeExecutor::passing(Vec::new());
        let selected_program = program(&workspace);
        assert!(matches!(
            workspace.execute(
                &executor,
                OperationId::InfoCurrent,
                &selected_program,
                BTreeMap::new(),
            ),
            Err(RunWorkspaceError::OutOfOrder {
                expected: OperationId::Version,
                actual: OperationId::InfoCurrent,
            })
        ));
        assert_eq!(executor.calls.get(), 0);
        assert!(matches!(
            workspace.execute(
                &executor,
                OperationId::Version,
                selected_program,
                BTreeMap::new(),
            ),
            Err(RunWorkspaceError::Poisoned)
        ));
    }

    #[test]
    fn between_operation_mutation_and_hardlinks_fail_before_launch() {
        let fixture = Fixture::new(true);
        let mut workspace = fixture.seal().expect("sealed workspace");
        fs::write(
            workspace.root().join("packages/current/character.spine"),
            b"tampered",
        )
        .expect("tamper staged input");
        let executor = FakeExecutor::passing(Vec::new());
        let selected_program = program(&workspace);
        assert!(matches!(
            workspace.execute(
                &executor,
                OperationId::Version,
                selected_program,
                BTreeMap::new(),
            ),
            Err(RunWorkspaceError::BetweenOperationMutation(path))
                if path == Path::new("packages/current/character.spine")
        ));
        assert_eq!(executor.calls.get(), 0);

        let second = Fixture::new(true);
        let mut workspace = second.seal().expect("second sealed workspace");
        fs::hard_link(
            workspace.root().join("packages/current/character.spine"),
            workspace.root().join("packages/current/alias.spine"),
        )
        .expect("hard link injection");
        let selected_program = program(&workspace);
        assert!(matches!(
            workspace.execute(
                &executor,
                OperationId::Version,
                selected_program,
                BTreeMap::new(),
            ),
            Err(RunWorkspaceError::Snapshot(
                WorkspaceSnapshotError::MultipleLinks(_)
            ))
        ));
    }

    #[test]
    fn collateral_mutation_is_rejected_and_poisons() {
        let fixture = Fixture::new(true);
        let mut workspace = fixture.seal().expect("sealed workspace");
        mark_completed_before(&mut workspace, OperationId::ExportCurrentA);
        let output = first_output(&workspace, OperationId::ExportCurrentA);
        let extra = workspace
            .root()
            .join("outputs/round-trip/a/source/extra.txt");
        let executor = FakeExecutor::passing(vec![
            (output.clone(), b"json".to_vec()),
            (extra.clone(), b"collateral".to_vec()),
        ]);
        let selected_program = program(&workspace);
        assert!(matches!(
            workspace.execute(
                &executor,
                OperationId::ExportCurrentA,
                &selected_program,
                BTreeMap::new(),
            ),
            Err(RunWorkspaceError::MutationEnvelope(path))
                if path == Path::new("outputs/round-trip/a/source/extra.txt")
        ));
        assert!(matches!(
            workspace.execute(
                &executor,
                OperationId::ExportCurrentA,
                selected_program,
                BTreeMap::new(),
            ),
            Err(RunWorkspaceError::Poisoned)
        ));
    }

    #[test]
    fn post_snapshot_exposes_collateral_work_even_when_runner_fails() {
        let fixture = Fixture::new(true);
        let mut workspace = fixture.seal().expect("sealed workspace");
        mark_completed_before(&mut workspace, OperationId::ExportCurrentA);
        let extra = workspace
            .root()
            .join("outputs/round-trip/a/source/extra.txt");
        let executor = FakeExecutor::runner_error(vec![(extra, b"collateral".to_vec())]);
        let selected_program = program(&workspace);
        assert!(matches!(
            workspace.execute(
                &executor,
                OperationId::ExportCurrentA,
                selected_program,
                BTreeMap::new(),
            ),
            Err(RunWorkspaceError::MutationEnvelope(path))
                if path == Path::new("outputs/round-trip/a/source/extra.txt")
        ));
        assert_eq!(executor.calls.get(), 1);
    }

    #[test]
    fn executable_digest_mismatch_stops_and_poisons_after_one_launch() {
        let fixture = Fixture::new(true);
        let mut workspace = fixture.seal().expect("sealed workspace");
        let executor = WrongExecutableExecutor {
            calls: Cell::new(0),
        };
        let selected_program = program(&workspace);

        assert!(matches!(
            workspace.execute(
                &executor,
                OperationId::Version,
                &selected_program,
                BTreeMap::new(),
            ),
            Err(RunWorkspaceError::UntrustedEditorExecutable(
                OperationId::Version
            ))
        ));
        assert!(matches!(
            workspace.execute(
                &executor,
                OperationId::Version,
                selected_program,
                BTreeMap::new(),
            ),
            Err(RunWorkspaceError::Poisoned)
        ));
        assert_eq!(executor.calls.get(), 1);
        assert!(workspace.runs.is_empty());
        assert!(workspace.completed_operations.is_empty());
    }

    #[test]
    fn failed_process_does_not_register_its_created_output() {
        let fixture = Fixture::new(true);
        let mut workspace = fixture.seal().expect("sealed workspace");
        mark_completed_before(&mut workspace, OperationId::ExportCurrentA);
        let output = first_output(&workspace, OperationId::ExportCurrentA);
        let executor = FakeExecutor::invalid_transcript(vec![(output.clone(), b"json".to_vec())]);
        let selected_program = program(&workspace);
        assert!(matches!(
            workspace.execute(
                &executor,
                OperationId::ExportCurrentA,
                selected_program,
                BTreeMap::new(),
            ),
            Err(RunWorkspaceError::UnexpectedProcessFailure(
                OperationId::ExportCurrentA
            ))
        ));
        let relative = relative_to_root(workspace.root(), &output).expect("relative output");
        assert!(!workspace.readable.contains_key(&relative));
        assert!(
            !workspace
                .completed_operations
                .contains(&OperationId::ExportCurrentA)
        );
    }

    #[test]
    fn isolated_new_animation_collision_is_an_expected_negative_control() {
        let fixture = Fixture::new(true);
        let mut workspace = fixture.seal().expect("sealed workspace");
        let executor = CompleteExecutor::passing();
        let selected_program = program(&workspace);

        for operation in OperationId::ORDER {
            let evidence = workspace
                .execute(&executor, operation, &selected_program, BTreeMap::new())
                .expect("operation accepted by its exact contract");
            if operation == OperationId::ImportNewCollisionControl {
                assert!(!evidence.run().process().assessment().passed());
                let collision = evidence
                    .run()
                    .process()
                    .new_animation_collision()
                    .expect("typed collision evidence");
                assert_eq!(collision.requested_animation(), "gesture");
                assert_eq!(collision.renamed_animation(), "gesture2");
                assert_eq!(
                    fs::read(
                        workspace
                            .root()
                            .join(NEW_CANDIDATE_PATH)
                            .join("character.spine")
                    )
                    .expect("positive candidate"),
                    b"new candidate"
                );
                assert_eq!(
                    fs::read(
                        workspace
                            .root()
                            .join(NEW_COLLISION_CONTROL_PATH)
                            .join("character.spine")
                    )
                    .expect("collision candidate"),
                    b"new collision candidate"
                );
            }
        }

        let (processes, source_rechecks) = workspace.into_failure_evidence();
        assert_eq!(processes.len(), OperationId::ORDER.len());
        let collision_index = OperationId::ORDER
            .iter()
            .position(|id| *id == OperationId::ImportNewCollisionControl)
            .expect("collision slot");
        let failed = &processes[collision_index];
        assert!(!failed.assessment().passed());
        assert!(
            std::str::from_utf8(failed.raw_stdout_retained_prefix())
                .expect("UTF-8 diagnostic")
                .contains("An animation with this name already exists: gesture -> gesture2")
        );
        assert_eq!(
            source_rechecks.status(),
            ControlledSourceRecheckStatus::Unchanged
        );
    }

    #[test]
    fn exact_missing_images_failure_completes_without_output_or_poison() {
        let fixture = Fixture::new(true);
        let mut workspace = fixture.seal().expect("sealed workspace");
        mark_completed_before(&mut workspace, OperationId::MissingImagesPathControl);
        let output = first_output(&workspace, OperationId::MissingImagesPathControl);
        let executor = FakeExecutor::passing(Vec::new());
        let selected_program = program(&workspace);
        let evidence = workspace
            .execute(
                &executor,
                OperationId::MissingImagesPathControl,
                selected_program,
                BTreeMap::new(),
            )
            .expect("exact negative control");
        assert!(!evidence.run().process().assessment().passed());
        assert_eq!(evidence.run().process().assessment().failures().len(), 2);
        assert!(!output.exists());
        let relative = relative_to_root(workspace.root(), &output).expect("relative output");
        assert!(!workspace.readable.contains_key(&relative));
        assert!(matches!(
            workspace.finish(),
            Err(RunWorkspaceError::IncompleteOperations {
                completed: 1,
                expected: 22,
            })
        ));
    }

    #[test]
    fn finish_rejects_incomplete_runs_and_mutated_original_sources() {
        let fixture = Fixture::new(true);
        assert!(matches!(
            fixture.seal().expect("sealed workspace").finish(),
            Err(RunWorkspaceError::IncompleteOperations {
                completed: 0,
                expected: 22,
            })
        ));

        let second = Fixture::new(true);
        let executor = CompleteExecutor::passing();
        let workspace = complete_workspace(&second, &executor);
        fs::write(
            second.current_source.join("character.spine"),
            b"external source changed",
        )
        .expect("mutate original source");
        assert!(matches!(
            workspace.finish(),
            Err(RunWorkspaceError::Stage(StageError::SourceChanged))
        ));
    }

    #[test]
    fn completed_workspace_owns_every_closed_input_and_evidence_layer() {
        let fixture = Fixture::new(true);
        let executor = CompleteExecutor::passing();
        let completed = complete_workspace(&fixture, &executor)
            .finish()
            .expect("closed extraction");

        assert_eq!(completed.case_sha256(), fixture.case.source_sha256());
        assert_eq!(completed.runs().len(), OperationId::ORDER.len());
        assert_eq!(
            completed.operation_inventory().records().len(),
            OperationId::ORDER.len()
        );
        assert!(
            completed
                .runs()
                .iter()
                .all(|run| run.before_physical_sha256().len() == 64
                    && run.after_physical_sha256().len() == 64)
        );
        assert_eq!(
            completed.json_sources().current_b,
            operation_output_bytes(OperationId::ExportCurrentB)
        );
        assert_eq!(
            completed.json_sources().existing_repeat,
            operation_output_bytes(OperationId::ExportExistingRepeat)
        );
        assert_eq!(
            completed.json_sources().new_collision_control,
            operation_output_bytes(OperationId::ExportNewCollisionControl)
        );

        let runtime = completed.runtime_inputs();
        for target in [runtime.current(), runtime.existing(), runtime.new_target()] {
            assert_eq!(target.json_path(), Path::new(RUNTIME_JSON_PATH));
            assert_eq!(target.atlas_path(), Path::new("review/character.atlas"));
            assert_eq!(
                target.files().keys().collect::<Vec<_>>(),
                [
                    &PathBuf::from("review/character.atlas"),
                    &PathBuf::from("review/images/page.png"),
                    &PathBuf::from(RUNTIME_JSON_PATH),
                ]
            );
            assert_eq!(target.bindings().len(), 3);
        }
        assert_eq!(
            runtime.current().files()[Path::new("review/character.atlas")],
            ATLAS_BYTES
        );
        assert_eq!(
            runtime.current().files()[Path::new("review/images/page.png")],
            PNG_BYTES
        );

        let boundary = completed.boundary_evidence();
        assert!(!boundary.sealed_portable().entries.is_empty());
        assert!(!boundary.final_portable().entries.is_empty());
        assert_eq!(
            boundary.case_package_inventories().current,
            boundary.sources.current.before_staging
        );
        let serialized = serde_json::to_value(boundary).expect("serializable rich evidence");
        assert!(serialized["sealed_physical"]["entries"]["."]["device"].is_number());
    }

    #[test]
    fn finish_rejects_wrong_manifest_skeleton() {
        let fixture = Fixture::new(true);
        let executor = CompleteExecutor::with_wrong_skeleton(OperationId::InfoReplacement);
        let workspace = complete_workspace(&fixture, &executor);
        assert!(matches!(
            workspace.finish(),
            Err(RunWorkspaceError::ProjectInfo(
                ProjectInfoError::WrongTargetSkeleton { .. }
            ))
        ));
    }

    #[test]
    fn extraction_rejects_swapped_output_bytes_after_final_snapshot() {
        let fixture = Fixture::new(true);
        let executor = CompleteExecutor::passing();
        let workspace = complete_workspace(&fixture, &executor);
        let current = first_output(&workspace, OperationId::ExportCurrentA);
        let replacement = first_output(&workspace, OperationId::ExportReplacementSubmission);
        assert!(matches!(
            workspace.finish_inner(ExtractionLimits::production(), move |_| {
                let current_bytes = fs::read(&current).expect("current JSON");
                let replacement_bytes = fs::read(&replacement).expect("replacement JSON");
                fs::write(&current, replacement_bytes).expect("swap current");
                fs::write(&replacement, current_bytes).expect("swap replacement");
            }),
            Err(RunWorkspaceError::ExtractionBinding(_))
        ));
    }

    #[test]
    fn finish_rejects_after_run_tamper_before_extraction() {
        let fixture = Fixture::new(true);
        let executor = CompleteExecutor::passing();
        let workspace = complete_workspace(&fixture, &executor);
        let output = first_output(&workspace, OperationId::ExportCurrentA);
        fs::write(&output, b"tampered after run").expect("tamper output");
        assert!(matches!(
            workspace.finish(),
            Err(RunWorkspaceError::BetweenOperationMutation(path))
                if path == Path::new("outputs/round-trip/a/source/Current Rig.json")
        ));
    }

    #[test]
    fn extraction_enforces_role_byte_limits_before_reading() {
        let fixture = Fixture::new(true);
        let executor = CompleteExecutor::passing();
        let workspace = complete_workspace(&fixture, &executor);
        let mut limits = ExtractionLimits::production();
        limits.json_bytes = 1;
        assert!(matches!(
            workspace.finish_inner(limits, |_| {}),
            Err(RunWorkspaceError::ExtractionByteLimit { limit: 1, .. })
        ));
    }

    #[test]
    fn extraction_rejects_symlink_and_same_byte_replacement() {
        let fixture = Fixture::new(true);
        let executor = CompleteExecutor::passing();
        let workspace = complete_workspace(&fixture, &executor);
        let target = first_output(&workspace, OperationId::ExportCurrentA);
        let alias_target = first_output(&workspace, OperationId::ExportReplacementSubmission);
        assert!(matches!(
            workspace.finish_inner(ExtractionLimits::production(), move |_| {
                fs::remove_file(&target).expect("remove JSON");
                symlink(&alias_target, &target).expect("inject symlink");
            }),
            Err(RunWorkspaceError::Io { .. })
                | Err(RunWorkspaceError::ExtractionBinding(_))
                | Err(RunWorkspaceError::Snapshot(
                    WorkspaceSnapshotError::Symlink(_)
                ))
        ));

        let second = Fixture::new(true);
        let executor = CompleteExecutor::passing();
        let workspace = complete_workspace(&second, &executor);
        let target = first_output(&workspace, OperationId::ExportCurrentA);
        let held = second._temporary.path().join("held-current.json");
        assert!(matches!(
            workspace.finish_inner(ExtractionLimits::production(), move |_| {
                let bytes = fs::read(&target).expect("original bytes");
                fs::rename(&target, &held).expect("retain original inode");
                fs::write(&target, bytes).expect("same-byte replacement");
            }),
            Err(RunWorkspaceError::ExtractionBinding(_))
        ));
    }
}

use crate::digest::{is_sha256, sha256_bytes};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

const ALLOWED_ENVIRONMENT_NAMES: &[&str] = &["HOME", "LANG", "LC_ALL", "PATH", "TMPDIR"];
const MAX_PROCESS_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const MAX_CLEANUP_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RETAINED_BYTES_PER_STREAM: usize = 4 * 1024 * 1024;
const BLOCKING_TERMS: &[&str] = &[
    "warning",
    "error",
    "failed",
    "failure",
    "not found",
    "unknown",
    "ignored",
    "unsupported",
    "exception",
    "unable",
    "missing",
    "invalid",
    "denied",
];

// Populate only from captured, reviewed Spine 4.3.23 operation evidence. Case
// files may not add to or weaken this list. Version and advanced-help probes
// use their own exact structural profiles below.
const SPINE_4_3_23_INFORMATIONAL_LINES: &[&str] = &[];
const SPINE_4_3_23_ADVANCED_HELP: &str = include_str!("../policy/spine-4.3.23-advanced-help.txt");
const SPINE_LAUNCHER_HEADER: &str = "Spine Launcher 4.3.06 (macOS Apple Silicon)";
const SPINE_COPYRIGHT_HEADER: &str =
    "Esoteric Software LLC (C) 2013-2026 | http://esotericsoftware.com";

/// One editor process request at the injectable process boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessRequest {
    /// Logical operation name used in evidence.
    pub operation: String,
    /// Absolute executable path selected by the caller.
    pub program: String,
    /// Exact argument vector, without shell interpretation.
    pub args: Vec<String>,
    /// Absolute working directory for the child process.
    pub working_directory: PathBuf,
    /// Complete minimal child environment. Ambient values are cleared.
    pub environment: BTreeMap<String, String>,
    /// Maximum wall-clock time for process and pipe completion.
    pub timeout: Duration,
    /// Maximum cleanup time after forced termination.
    pub cleanup_timeout: Duration,
    /// Maximum raw prefix bytes retained from each output stream.
    pub max_retained_bytes_per_stream: usize,
    /// Output identifiers that must be present after a successful process.
    pub required_outputs: BTreeSet<String>,
}

/// Stable reason the adapter stopped observing one process.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminationReason {
    /// The child exited and both output streams reached EOF before the deadline.
    NaturalExit,
    /// The process or inherited output streams exceeded the execution deadline.
    DeadlineExceeded,
    /// A fatal adapter, status, signal, poll, or pipe error forced termination.
    CaptureFailure,
}

/// Stable status of forced-termination cleanup.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupStatus {
    /// The direct child was reaped and both output pipes reached EOF.
    Complete,
    /// The cleanup deadline expired and the child was delegated to the reaper.
    ReaperDelegated,
    /// The cleanup deadline expired and bounded reaper handoff failed.
    ReaperUnavailable,
    /// The direct child was reaped, but inherited pipes missed the cleanup deadline.
    DeadlineExceeded,
}

/// Stable adapter failure categories recorded after a child has started.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterFailureCode {
    /// A child pipe was absent or could not be made nonblocking.
    PipeSetup,
    /// Reading a child pipe failed.
    PipeRead,
    /// Polling child pipes failed or returned an invalid descriptor.
    PipePoll,
    /// The direct child exited while an inherited output writer remained open.
    InheritedPipeAfterExit,
    /// Querying the direct child status failed.
    StatusQuery,
    /// A stream byte count exceeded the representable range.
    ByteCountOverflow,
    /// Sending the required termination signal failed.
    SignalDelivery,
    /// The executable or working-directory identity changed during launch.
    LaunchIdentityChanged,
    /// Cleanup exceeded its separate deadline.
    CleanupDeadlineExceeded,
    /// The bounded reaper was unavailable after cleanup expiry.
    ReaperUnavailable,
}

/// One stable adapter failure with diagnostic context.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterFailure {
    code: AdapterFailureCode,
    detail: String,
}

impl AdapterFailure {
    /// Creates one adapter failure.
    pub(crate) fn new(code: AdapterFailureCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }

    /// Returns the stable failure category.
    pub fn code(&self) -> AdapterFailureCode {
        self.code
    }

    /// Returns diagnostic context for the failure.
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

/// Canonical executable identity captured immediately before launch.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutableIdentity {
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

impl ExecutableIdentity {
    /// Creates a resolved executable identity at the subprocess boundary.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
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
    ) -> Self {
        Self {
            canonical_path,
            sha256,
            size,
            device,
            inode,
            mode,
            owner,
            modified_seconds,
            modified_nanoseconds,
            changed_seconds,
            changed_nanoseconds,
            local_filesystem_verified: true,
        }
    }

    /// Returns the canonical executable path.
    pub fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }

    /// Returns the executable SHA-256.
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// Returns the executable size observed while hashing its exact bytes.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Returns a path-free digest of the complete stable file identity used
    /// to detect replacement even when executable bytes are unchanged.
    pub fn stable_file_identity_sha256(&self) -> String {
        let mut framed = b"spinal-phase0a-executable-file-identity-v1\0".to_vec();
        framed.extend_from_slice(self.sha256.as_bytes());
        framed.push(0);
        for value in [
            self.size,
            self.device,
            self.inode,
            u64::from(self.mode),
            u64::from(self.owner),
            self.modified_seconds as u64,
            self.modified_nanoseconds as u64,
            self.changed_seconds as u64,
            self.changed_nanoseconds as u64,
        ] {
            framed.extend_from_slice(&value.to_le_bytes());
        }
        framed.push(u8::from(self.local_filesystem_verified));
        sha256_bytes(&framed)
    }

    /// Returns true when stable file identity fields match another observation.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn same_file(
        &self,
        device: u64,
        inode: u64,
        size: u64,
        mode: u32,
        owner: u32,
        modified_seconds: i64,
        modified_nanoseconds: i64,
        changed_seconds: i64,
        changed_nanoseconds: i64,
    ) -> bool {
        self.device == device
            && self.inode == inode
            && self.size == size
            && self.mode == mode
            && self.owner == owner
            && self.modified_seconds == modified_seconds
            && self.modified_nanoseconds == modified_nanoseconds
            && self.changed_seconds == changed_seconds
            && self.changed_nanoseconds == changed_nanoseconds
    }
}

/// Canonical working-directory identity captured immediately before launch.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkingDirectoryIdentity {
    canonical_path: PathBuf,
    device: u64,
    inode: u64,
    mode: u32,
    owner: u32,
    local_filesystem_verified: bool,
}

impl WorkingDirectoryIdentity {
    /// Creates a resolved working-directory identity at the subprocess boundary.
    pub(crate) fn new(
        canonical_path: PathBuf,
        device: u64,
        inode: u64,
        mode: u32,
        owner: u32,
    ) -> Self {
        Self {
            canonical_path,
            device,
            inode,
            mode,
            owner,
            local_filesystem_verified: true,
        }
    }

    /// Returns the canonical working-directory path.
    pub fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }

    /// Returns true when stable directory identity fields match an observation.
    pub(crate) fn same_file(&self, device: u64, inode: u64, mode: u32, owner: u32) -> bool {
        self.device == device && self.inode == inode && self.mode == mode && self.owner == owner
    }
}

/// Hashed representation of one allowlisted environment value.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentVariableEvidence {
    name: String,
    value_sha256: String,
}

impl EnvironmentVariableEvidence {
    fn from_pair(name: &str, value: &str) -> Self {
        Self {
            name: name.to_owned(),
            value_sha256: sha256_bytes(value.as_bytes()),
        }
    }

    /// Returns the allowlisted variable name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the SHA-256 of the unrecorded raw value.
    pub fn value_sha256(&self) -> &str {
        &self.value_sha256
    }
}

/// Evidence that the process call was protected by the trusted editor lock.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LockEvidence {
    canonical_path: PathBuf,
    wait_seconds: u64,
    wait_subsec_nanos: u32,
    acquired: bool,
    local_filesystem_verified: bool,
    device: u64,
    inode: u64,
    filesystem_kind: String,
}

impl LockEvidence {
    /// Creates acquired lock evidence from the hardened lock boundary.
    pub(crate) fn new_acquired(
        canonical_path: PathBuf,
        wait: Duration,
        device: u64,
        inode: u64,
        filesystem_kind: String,
    ) -> Self {
        Self {
            canonical_path,
            wait_seconds: wait.as_secs(),
            wait_subsec_nanos: wait.subsec_nanos(),
            acquired: true,
            local_filesystem_verified: true,
            device,
            inode,
            filesystem_kind,
        }
    }

    /// Returns whether acquisition and local-filesystem verification succeeded.
    pub fn acquired(&self) -> bool {
        self.acquired && self.local_filesystem_verified
    }

    /// Returns the canonical persistent lock-file path.
    pub fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }

    /// Returns the measured acquisition wait.
    pub fn wait(&self) -> Duration {
        Duration::new(self.wait_seconds, self.wait_subsec_nanos)
    }

    /// Returns true when two acquisitions used the same persistent lock file.
    pub fn same_identity(&self, other: &Self) -> bool {
        self.acquired()
            && other.acquired()
            && self.canonical_path == other.canonical_path
            && self.device == other.device
            && self.inode == other.inode
    }
}

/// Bounded retained prefix and streaming identity for one output stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessStreamCapture {
    /// Exact raw prefix retained for later artifact writing.
    pub retained_prefix: Vec<u8>,
    /// Total bytes observed and streamed through the digest.
    pub total_observed_bytes: u64,
    /// SHA-256 of every byte observed, even when the prefix was truncated.
    pub bytes_seen_sha256: String,
    /// Full-stream SHA-256, present only after a proven EOF.
    pub full_stream_sha256: Option<String>,
    /// Whether more bytes were observed than retained.
    pub retained_prefix_truncated: bool,
    /// Whether EOF was observed for the stream.
    pub complete: bool,
}

/// Whether exact-path output discovery ran after process execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputDiscoveryState {
    /// The process adapter did not inspect typed expected output paths.
    NotPerformed,
    /// The typed command wrapper completed exact-path output discovery.
    Complete,
}

/// Captured result returned by a process executor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessCapture {
    /// Normal exit code, when one was produced.
    pub exit_code: Option<i32>,
    /// Unix signal that terminated the direct child, when applicable.
    pub terminating_signal: Option<i32>,
    /// Signal the adapter sent to the child process group, when applicable.
    pub sent_signal: Option<i32>,
    /// Stable reason observation ended.
    pub termination_reason: TerminationReason,
    /// Total monotonic execution-and-capture duration.
    pub elapsed: Duration,
    /// Bounded cleanup result.
    pub cleanup_status: CleanupStatus,
    /// Stable adapter failure, if one occurred after launch.
    pub adapter_failure: Option<AdapterFailure>,
    /// Standard output prefix and streaming identity.
    pub stdout: ProcessStreamCapture,
    /// Standard error prefix and streaming identity.
    pub stderr: ProcessStreamCapture,
    /// Output identifiers observed after execution.
    pub observed_outputs: BTreeSet<String>,
    /// Whether trusted exact-path output discovery completed.
    pub(crate) output_discovery_state: OutputDiscoveryState,
    /// Canonical executable identity used at launch.
    pub executable_identity: ExecutableIdentity,
    /// Canonical working-directory identity used at launch.
    pub working_directory_identity: WorkingDirectoryIdentity,
    /// Acquired editor-lock evidence, absent for an unlocked executor.
    pub lock_evidence: Option<LockEvidence>,
}

/// Injectable boundary used to test process policy without invoking Spine.
pub trait ProcessExecutor {
    /// Executes one request and returns its bounded capture.
    fn execute(&self, request: &ProcessRequest) -> Result<ProcessCapture, ProcessExecutionError>;
}

/// Stable failure categories before a child process can be safely observed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessExecutionErrorCode {
    /// The request violated the fixed subprocess contract.
    InvalidRequest,
    /// The current platform cannot provide the required safety properties.
    UnsupportedPlatform,
    /// The executable could not be resolved, validated, or hashed.
    ExecutableIdentity,
    /// The working directory could not be resolved or validated.
    WorkingDirectoryIdentity,
    /// The bounded child reaper could not be initialized.
    ReaperUnavailable,
    /// Identity resolution and hashing consumed the execution deadline.
    PreflightDeadline,
    /// The operating system refused to start the child.
    Spawn,
    /// The editor lock could not be safely acquired.
    Lock,
    /// A custom executor reported an execution boundary failure.
    Executor,
}

/// Failure before a safe process capture could be produced.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("process execution failed ({code:?}): {message}")]
pub struct ProcessExecutionError {
    code: ProcessExecutionErrorCode,
    message: String,
}

impl ProcessExecutionError {
    /// Creates a custom-executor failure.
    pub fn new(message: impl Into<String>) -> Self {
        Self::with_code(ProcessExecutionErrorCode::Executor, message)
    }

    /// Creates a failure with a stable category.
    pub(crate) fn with_code(code: ProcessExecutionErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Returns the stable execution failure category.
    pub fn code(&self) -> ProcessExecutionErrorCode {
        self.code
    }

    /// Returns diagnostic context for the failure.
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Checked-in transcript rules for one closed Spine 4.3.23 operation class.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TranscriptPolicy {
    profile: TranscriptProfile,
}

/// Stable identifier for one checked-in transcript policy profile.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptProfile {
    /// Deny-first generic operation transcript policy.
    #[default]
    Operation,
    /// Exact activated-version transcript policy.
    Version,
    /// Exact reviewed advanced-help transcript policy.
    AdvancedHelp,
    /// Reviewed project-import transcript policy.
    ProjectImport,
    /// Reviewed JSON-export transcript policy.
    JsonExport,
    /// Exact missing-images-path negative-control transcript policy.
    MissingImagesPathControl,
    /// Reviewed animation-import transcript policy.
    AnimationImport,
    /// Exact repeat-import animation-name collision control.
    NewAnimationCollisionControl,
    /// Reviewed project-information transcript policy.
    ProjectInfo,
}

/// Typed proof of the rename selected by Spine for a new-animation collision.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NewAnimationCollisionEvidence {
    requested_animation: String,
    renamed_animation: String,
}

impl NewAnimationCollisionEvidence {
    #[cfg(test)]
    pub(crate) fn for_test(requested_animation: &str, renamed_animation: &str) -> Self {
        assert!(safe_animation_name(requested_animation));
        assert!(safe_animation_name(renamed_animation));
        assert_ne!(requested_animation, renamed_animation);
        Self {
            requested_animation: requested_animation.to_owned(),
            renamed_animation: renamed_animation.to_owned(),
        }
    }

    /// Returns the exact animation name bound by the typed CLI request.
    pub fn requested_animation(&self) -> &str {
        &self.requested_animation
    }

    /// Returns the safe, distinct animation name parsed from Spine's diagnostic.
    pub fn renamed_animation(&self) -> &str {
        &self.renamed_animation
    }
}

/// One typed category reported by Spine's project-info command.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectInfoSection {
    /// Bone names.
    Bones,
    /// Slot names.
    Slots,
    /// Skin names.
    Skins,
    /// Event names.
    Events,
    /// IK constraint names.
    IkConstraints,
    /// Transform constraint names.
    TransformConstraints,
    /// Path constraint names.
    PathConstraints,
    /// Physics constraint names.
    PhysicsConstraints,
    /// Animation names. Exported JSON remains the exact animation-name oracle.
    Animations,
}

/// Counted names from one project-info inventory section.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectInfoList {
    reported_count: usize,
    values: Vec<String>,
}

impl ProjectInfoList {
    /// Returns the exact count printed by Spine.
    pub fn reported_count(&self) -> usize {
        self.reported_count
    }

    /// Returns the unambiguous comma-delimited names printed by Spine.
    pub fn values(&self) -> &[String] {
        &self.values
    }
}

/// Typed inventory for one skeleton in a Spine project.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectSkeletonInventory {
    name: String,
    size: String,
    sections: BTreeMap<ProjectInfoSection, ProjectInfoList>,
}

impl ProjectSkeletonInventory {
    /// Returns the exact skeleton name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the exact size text reported by Spine.
    pub fn size(&self) -> &str {
        &self.size
    }

    /// Returns typed inventory sections keyed by their closed category.
    pub fn sections(&self) -> &BTreeMap<ProjectInfoSection, ProjectInfoList> {
        &self.sections
    }
}

/// Strictly parsed project-info output from one assessed Spine 4.3.23 call.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectInfoInventory {
    project: PathBuf,
    spine_version: String,
    dopesheet_fps: String,
    skeletons: Vec<ProjectSkeletonInventory>,
}

impl ProjectInfoInventory {
    /// Returns the exact project path bound by the typed request and transcript.
    pub fn project(&self) -> &Path {
        &self.project
    }

    /// Returns the exact version reported by the project itself.
    pub fn spine_version(&self) -> &str {
        &self.spine_version
    }

    /// Returns the validated positive finite FPS lexeme.
    pub fn dopesheet_fps(&self) -> &str {
        &self.dopesheet_fps
    }

    /// Returns skeletons in the order printed by Spine.
    pub fn skeletons(&self) -> &[ProjectSkeletonInventory] {
        &self.skeletons
    }

    /// Requires the v1 one-skeleton contract and its exact manifest name.
    pub fn require_exact_skeleton(
        &self,
        expected: &str,
    ) -> Result<&ProjectSkeletonInventory, ProjectInfoError> {
        match self.skeletons.as_slice() {
            [skeleton] if skeleton.name() == expected => Ok(skeleton),
            skeletons => Err(ProjectInfoError::WrongTargetSkeleton {
                expected: expected.to_owned(),
                observed: skeletons
                    .iter()
                    .map(|skeleton| skeleton.name.clone())
                    .collect(),
            }),
        }
    }
}

/// Failures while extracting typed project-info evidence.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ProjectInfoError {
    /// Evidence did not come from a successful typed project-info operation.
    #[error("process evidence is not a passing typed project-info operation")]
    WrongProcessEvidence,
    /// The reviewed transcript grammar was not satisfied.
    #[error("project-info transcript was malformed or internally inconsistent")]
    MalformedTranscript,
    /// The v1 project did not contain exactly the one expected skeleton.
    #[error("expected exactly skeleton `{expected}`, observed {observed:?}")]
    WrongTargetSkeleton {
        /// Manifest-selected skeleton name.
        expected: String,
        /// Skeleton names actually reported by Spine.
        observed: Vec<String>,
    },
}

impl TranscriptPolicy {
    /// Returns the stable profile identifier bound to this policy.
    pub const fn profile(&self) -> TranscriptProfile {
        self.profile
    }

    /// Returns the deny-first policy for normal Spine 4.3.23 operations.
    ///
    /// Until reviewed operation transcripts populate the checked-in exact-line
    /// list, this profile accepts only blank output.
    pub const fn spine_4_3_23() -> Self {
        Self {
            profile: TranscriptProfile::Operation,
        }
    }

    /// Returns the exact activated-version transcript profile.
    pub const fn spine_4_3_23_version() -> Self {
        Self {
            profile: TranscriptProfile::Version,
        }
    }

    /// Returns the exact reviewed advanced-help transcript profile.
    pub const fn spine_4_3_23_advanced_help() -> Self {
        Self {
            profile: TranscriptProfile::AdvancedHelp,
        }
    }

    pub(crate) const fn spine_4_3_23_project_import() -> Self {
        Self {
            profile: TranscriptProfile::ProjectImport,
        }
    }

    pub(crate) const fn spine_4_3_23_json_export() -> Self {
        Self {
            profile: TranscriptProfile::JsonExport,
        }
    }

    pub(crate) const fn spine_4_3_23_missing_images_path_control() -> Self {
        Self {
            profile: TranscriptProfile::MissingImagesPathControl,
        }
    }

    pub(crate) const fn spine_4_3_23_animation_import() -> Self {
        Self {
            profile: TranscriptProfile::AnimationImport,
        }
    }

    pub(crate) const fn spine_4_3_23_new_animation_collision_control() -> Self {
        Self {
            profile: TranscriptProfile::NewAnimationCollisionControl,
        }
    }

    pub(crate) const fn spine_4_3_23_project_info() -> Self {
        Self {
            profile: TranscriptProfile::ProjectInfo,
        }
    }
}

/// Stable failure codes emitted by process assessment.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessFailureCode {
    /// The process or inherited pipes exceeded the deadline.
    TimedOut,
    /// The executor did not produce a normal exit status.
    MissingExitStatus,
    /// The process returned a nonzero exit code.
    NonzeroExit,
    /// The process terminated from a signal without an adapter deadline.
    TerminatedBySignal,
    /// A post-launch adapter failure occurred.
    AdapterFailure,
    /// Forced termination did not complete within its cleanup deadline.
    CleanupIncomplete,
    /// Canonical executable or working-directory evidence was invalid.
    InvalidLaunchIdentity,
    /// Acquired trusted editor-lock evidence was absent.
    MissingLockEvidence,
    /// Stream counts, flags, or digests were internally inconsistent.
    InvalidCaptureEvidence,
    /// Standard output or error was not UTF-8.
    NonUtf8Transcript,
    /// More output bytes were observed than the retained-prefix limit.
    OutputLimitExceeded,
    /// A deny-listed diagnostic appeared in the transcript.
    BlockingDiagnostic,
    /// A nonblank line was not an exact checked-in informational line.
    UnknownTranscriptLine,
    /// A structured version or capability transcript did not match policy.
    TranscriptContractMismatch,
    /// A required output was absent after execution.
    MissingOutput,
    /// Trusted exact-path output discovery did not run.
    OutputDiscoveryNotPerformed,
}

/// One actionable process-policy failure.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessFailure {
    /// Stable machine-readable failure code.
    pub code: ProcessFailureCode,
    /// Context suitable for the evidence report.
    pub detail: String,
}

/// Fail-closed assessment of one captured editor process.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessAssessment {
    passed: bool,
    stdout_retained_prefix_sha256: String,
    stderr_retained_prefix_sha256: String,
    failures: Vec<ProcessFailure>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ProcessStreamEvidence {
    total_observed_bytes: u64,
    retained_bytes: usize,
    retained_prefix_sha256: String,
    bytes_seen_sha256: String,
    full_stream_sha256: Option<String>,
    retained_prefix_truncated: bool,
    complete: bool,
}

/// Serialized evidence atomically binding a request, capture, and assessment.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessEvidence {
    operation: String,
    requested_program: String,
    args: Vec<String>,
    requested_working_directory: PathBuf,
    environment: Vec<EnvironmentVariableEvidence>,
    timeout_seconds: u64,
    timeout_subsec_nanos: u32,
    cleanup_timeout_seconds: u64,
    cleanup_timeout_subsec_nanos: u32,
    max_retained_bytes_per_stream: usize,
    executable_identity: ExecutableIdentity,
    working_directory_identity: WorkingDirectoryIdentity,
    lock_evidence: Option<LockEvidence>,
    exit_code: Option<i32>,
    terminating_signal: Option<i32>,
    sent_signal: Option<i32>,
    termination_reason: TerminationReason,
    elapsed_seconds: u64,
    elapsed_subsec_nanos: u32,
    cleanup_status: CleanupStatus,
    adapter_failure: Option<AdapterFailure>,
    stdout: ProcessStreamEvidence,
    stderr: ProcessStreamEvidence,
    required_outputs: BTreeSet<String>,
    observed_outputs: BTreeSet<String>,
    output_discovery_state: OutputDiscoveryState,
    transcript_profile: TranscriptProfile,
    #[serde(skip_serializing_if = "Option::is_none")]
    new_animation_collision: Option<NewAnimationCollisionEvidence>,
    assessment: ProcessAssessment,
    #[serde(skip_serializing)]
    raw_stdout_retained_prefix: Vec<u8>,
    #[serde(skip_serializing)]
    raw_stderr_retained_prefix: Vec<u8>,
}

/// Executes one request through an injected adapter and applies strict policy.
pub fn execute_and_assess(
    executor: &impl ProcessExecutor,
    request: &ProcessRequest,
    policy: TranscriptPolicy,
) -> Result<ProcessEvidence, ProcessExecutionError> {
    validate_request(request)?;
    let capture = executor.execute(request)?;
    evidence_from_capture(request, capture, policy)
}

pub(crate) fn evidence_from_capture(
    request: &ProcessRequest,
    capture: ProcessCapture,
    policy: TranscriptPolicy,
) -> Result<ProcessEvidence, ProcessExecutionError> {
    validate_request(request)?;
    let environment = request
        .environment
        .iter()
        .map(|(name, value)| EnvironmentVariableEvidence::from_pair(name, value))
        .collect();
    let assessment = assess_capture(request, &capture, policy);
    let new_animation_collision = derive_new_animation_collision(
        request,
        &capture.stdout.retained_prefix,
        policy,
        &assessment,
    );
    let stdout = stream_evidence(&capture.stdout);
    let stderr = stream_evidence(&capture.stderr);
    Ok(ProcessEvidence {
        operation: request.operation.clone(),
        requested_program: request.program.clone(),
        args: request.args.clone(),
        requested_working_directory: request.working_directory.clone(),
        environment,
        timeout_seconds: request.timeout.as_secs(),
        timeout_subsec_nanos: request.timeout.subsec_nanos(),
        cleanup_timeout_seconds: request.cleanup_timeout.as_secs(),
        cleanup_timeout_subsec_nanos: request.cleanup_timeout.subsec_nanos(),
        max_retained_bytes_per_stream: request.max_retained_bytes_per_stream,
        executable_identity: capture.executable_identity,
        working_directory_identity: capture.working_directory_identity,
        lock_evidence: capture.lock_evidence,
        exit_code: capture.exit_code,
        terminating_signal: capture.terminating_signal,
        sent_signal: capture.sent_signal,
        termination_reason: capture.termination_reason,
        elapsed_seconds: capture.elapsed.as_secs(),
        elapsed_subsec_nanos: capture.elapsed.subsec_nanos(),
        cleanup_status: capture.cleanup_status,
        adapter_failure: capture.adapter_failure,
        stdout,
        stderr,
        required_outputs: request.required_outputs.clone(),
        observed_outputs: capture.observed_outputs,
        output_discovery_state: capture.output_discovery_state,
        transcript_profile: policy.profile(),
        new_animation_collision,
        assessment,
        raw_stdout_retained_prefix: capture.stdout.retained_prefix,
        raw_stderr_retained_prefix: capture.stderr.retained_prefix,
    })
}

impl ProcessAssessment {
    /// Returns true only when process, transcript, lock, cleanup, and outputs passed.
    pub fn passed(&self) -> bool {
        self.passed
    }

    /// Returns the SHA-256 of the exact retained stdout prefix artifact.
    pub fn stdout_retained_prefix_sha256(&self) -> &str {
        &self.stdout_retained_prefix_sha256
    }

    /// Returns the SHA-256 of the exact retained stderr prefix artifact.
    pub fn stderr_retained_prefix_sha256(&self) -> &str {
        &self.stderr_retained_prefix_sha256
    }

    /// Returns every detected policy failure.
    pub fn failures(&self) -> &[ProcessFailure] {
        &self.failures
    }
}

impl ProcessEvidence {
    /// Returns the exact logical operation name.
    pub fn operation(&self) -> &str {
        &self.operation
    }

    /// Returns the exact executable string supplied in the request.
    pub fn program(&self) -> &str {
        &self.requested_program
    }

    /// Returns the exact argument vector supplied in the request.
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// Returns the exact requested working directory.
    pub fn working_directory(&self) -> &Path {
        &self.requested_working_directory
    }

    /// Returns hashed values for only the fixed allowlisted environment names.
    pub fn environment(&self) -> &[EnvironmentVariableEvidence] {
        &self.environment
    }

    /// Returns the exact process-and-capture deadline supplied in the request.
    pub fn timeout(&self) -> Duration {
        Duration::new(self.timeout_seconds, self.timeout_subsec_nanos)
    }

    /// Returns the canonical executable identity bound to this launch.
    pub fn executable_identity(&self) -> &ExecutableIdentity {
        &self.executable_identity
    }

    /// Returns trusted lock evidence bound to this process, when present.
    pub fn lock_evidence(&self) -> Option<&LockEvidence> {
        self.lock_evidence.as_ref()
    }

    /// Returns the captured exit code, when a normal status existed.
    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    /// Returns whether the execution deadline forced termination.
    pub fn timed_out(&self) -> bool {
        self.termination_reason == TerminationReason::DeadlineExceeded
    }

    /// Returns the exact retained stdout prefix inside the privacy boundary.
    pub(crate) fn raw_stdout_retained_prefix(&self) -> &[u8] {
        &self.raw_stdout_retained_prefix
    }

    /// Returns the exact retained stderr prefix inside the privacy boundary.
    pub(crate) fn raw_stderr_retained_prefix(&self) -> &[u8] {
        &self.raw_stderr_retained_prefix
    }

    /// Extracts the strict typed inventory from a passing project-info call.
    pub fn project_info_inventory(&self) -> Result<ProjectInfoInventory, ProjectInfoError> {
        if self.transcript_profile != TranscriptProfile::ProjectInfo || !self.assessment.passed() {
            return Err(ProjectInfoError::WrongProcessEvidence);
        }
        let input =
            argument_value(&self.args, "--input").ok_or(ProjectInfoError::MalformedTranscript)?;
        let text = std::str::from_utf8(&self.raw_stdout_retained_prefix)
            .map_err(|_| ProjectInfoError::MalformedTranscript)?;
        let body = editor_session_body(text).map_err(|_| ProjectInfoError::MalformedTranscript)?;
        parse_project_info_body(body, input)
    }

    /// Returns the request-bound rename parsed by the collision-control profile.
    ///
    /// Evidence is absent for every other profile, a malformed diagnostic, or
    /// any failure beyond the one expected blocking collision diagnostic.
    pub fn new_animation_collision(&self) -> Option<&NewAnimationCollisionEvidence> {
        self.new_animation_collision.as_ref()
    }

    /// Returns whether trusted editor-lock acquisition was bound to this call.
    pub fn trusted_lock_acquired(&self) -> bool {
        self.lock_evidence
            .as_ref()
            .is_some_and(LockEvidence::acquired)
    }

    /// Returns the exact required output identifiers.
    pub fn required_outputs(&self) -> &BTreeSet<String> {
        &self.required_outputs
    }

    /// Returns the exact observed output identifiers.
    pub fn observed_outputs(&self) -> &BTreeSet<String> {
        &self.observed_outputs
    }

    /// Returns whether trusted exact-path output discovery completed.
    pub fn output_discovery_state(&self) -> OutputDiscoveryState {
        self.output_discovery_state
    }

    /// Returns the transcript profile bound during assessment.
    pub fn transcript_profile(&self) -> TranscriptProfile {
        self.transcript_profile
    }

    /// Returns the assessment derived from this bound request and capture.
    pub fn assessment(&self) -> &ProcessAssessment {
        &self.assessment
    }
}

pub(crate) fn validate_request(request: &ProcessRequest) -> Result<(), ProcessExecutionError> {
    if request.operation.is_empty()
        || request.operation.trim() != request.operation
        || request.operation.chars().any(char::is_control)
    {
        return invalid_request("process operation must be a nonempty trimmed name");
    }
    if request.program.is_empty() {
        return invalid_request("process program must not be empty");
    }
    if request.program.contains('\0') || request.args.iter().any(|argument| argument.contains('\0'))
    {
        return invalid_request("process program and arguments must not contain NUL bytes");
    }
    if !Path::new(&request.program).is_absolute() {
        return invalid_request("process program must be an absolute path");
    }
    if !request.working_directory.is_absolute() {
        return invalid_request("process working directory must be an absolute path");
    }
    if request.timeout < Duration::from_millis(1) {
        return invalid_request("process timeout must be at least one millisecond");
    }
    if request.timeout > MAX_PROCESS_TIMEOUT {
        return invalid_request("process timeout must not exceed 30 minutes");
    }
    if request.cleanup_timeout < Duration::from_millis(1) {
        return invalid_request("process cleanup timeout must be at least one millisecond");
    }
    if request.cleanup_timeout > MAX_CLEANUP_TIMEOUT {
        return invalid_request("process cleanup timeout must not exceed 30 seconds");
    }
    if request.max_retained_bytes_per_stream == 0 {
        return invalid_request("process retained output limit must be nonzero");
    }
    if request.max_retained_bytes_per_stream > MAX_RETAINED_BYTES_PER_STREAM {
        return invalid_request("process retained output limit must not exceed four MiB");
    }
    if request.environment.iter().any(|(name, value)| {
        !ALLOWED_ENVIRONMENT_NAMES.contains(&name.as_str())
            || name.contains('\0')
            || value.contains('\0')
    }) {
        return invalid_request("process environment contains a non-allowlisted name or NUL byte");
    }
    Ok(())
}

fn invalid_request<T>(message: impl Into<String>) -> Result<T, ProcessExecutionError> {
    Err(ProcessExecutionError::with_code(
        ProcessExecutionErrorCode::InvalidRequest,
        message,
    ))
}

fn stream_evidence(capture: &ProcessStreamCapture) -> ProcessStreamEvidence {
    ProcessStreamEvidence {
        total_observed_bytes: capture.total_observed_bytes,
        retained_bytes: capture.retained_prefix.len(),
        retained_prefix_sha256: sha256_bytes(&capture.retained_prefix),
        bytes_seen_sha256: capture.bytes_seen_sha256.clone(),
        full_stream_sha256: capture.full_stream_sha256.clone(),
        retained_prefix_truncated: capture.retained_prefix_truncated,
        complete: capture.complete,
    }
}

fn assess_capture(
    request: &ProcessRequest,
    capture: &ProcessCapture,
    policy: TranscriptPolicy,
) -> ProcessAssessment {
    let mut failures = Vec::new();
    if capture.output_discovery_state != OutputDiscoveryState::Complete {
        failures.push(failure(
            ProcessFailureCode::OutputDiscoveryNotPerformed,
            "trusted exact-path output discovery did not run",
        ));
    }
    match capture.termination_reason {
        TerminationReason::NaturalExit => match (capture.exit_code, capture.terminating_signal) {
            (Some(0), None) => {}
            (Some(code), None) => failures.push(failure(
                ProcessFailureCode::NonzeroExit,
                format!("process exited with code {code}"),
            )),
            (None, Some(signal)) => failures.push(failure(
                ProcessFailureCode::TerminatedBySignal,
                format!("process terminated from Unix signal {signal}"),
            )),
            _ => failures.push(failure(
                ProcessFailureCode::MissingExitStatus,
                "process produced no coherent final status",
            )),
        },
        TerminationReason::DeadlineExceeded => failures.push(failure(
            ProcessFailureCode::TimedOut,
            "process or inherited output pipes exceeded the deadline",
        )),
        TerminationReason::CaptureFailure => failures.push(failure(
            ProcessFailureCode::AdapterFailure,
            "adapter failure forced process termination",
        )),
    }
    if let Some(adapter_failure) = &capture.adapter_failure {
        failures.push(failure(
            ProcessFailureCode::AdapterFailure,
            format!("{:?}: {}", adapter_failure.code(), adapter_failure.detail()),
        ));
    }
    if capture.cleanup_status != CleanupStatus::Complete {
        failures.push(failure(
            ProcessFailureCode::CleanupIncomplete,
            "process cleanup exceeded its deadline and required reaper delegation",
        ));
    }
    if !capture_fields_coherent(request, capture) {
        failures.push(failure(
            ProcessFailureCode::InvalidCaptureEvidence,
            "termination reason, status, signal, cleanup, or elapsed fields were inconsistent",
        ));
    }
    if !valid_identity(capture) {
        failures.push(failure(
            ProcessFailureCode::InvalidLaunchIdentity,
            "canonical executable or working-directory identity was invalid",
        ));
    }
    if !capture
        .lock_evidence
        .as_ref()
        .is_some_and(LockEvidence::acquired)
    {
        failures.push(failure(
            ProcessFailureCode::MissingLockEvidence,
            "trusted editor-lock acquisition was not bound to this process",
        ));
    }
    validate_stream(
        "stdout",
        &capture.stdout,
        request.max_retained_bytes_per_stream,
        &mut failures,
    );
    validate_stream(
        "stderr",
        &capture.stderr,
        request.max_retained_bytes_per_stream,
        &mut failures,
    );
    classify_transcript(
        "stdout",
        &capture.stdout.retained_prefix,
        request,
        policy,
        &mut failures,
    );
    classify_transcript(
        "stderr",
        &capture.stderr.retained_prefix,
        request,
        policy,
        &mut failures,
    );
    for output in request
        .required_outputs
        .difference(&capture.observed_outputs)
    {
        failures.push(failure(
            ProcessFailureCode::MissingOutput,
            format!("required output `{output}` was not produced"),
        ));
    }

    ProcessAssessment {
        passed: failures.is_empty(),
        stdout_retained_prefix_sha256: sha256_bytes(&capture.stdout.retained_prefix),
        stderr_retained_prefix_sha256: sha256_bytes(&capture.stderr.retained_prefix),
        failures,
    }
}

fn capture_fields_coherent(request: &ProcessRequest, capture: &ProcessCapture) -> bool {
    let mutually_exclusive_status =
        !(capture.exit_code.is_some() && capture.terminating_signal.is_some());
    let sent_signal_valid = capture.sent_signal.is_none_or(|signal| signal == 9);
    let cleanup_reason_valid = capture.cleanup_status == CleanupStatus::Complete
        || capture.termination_reason != TerminationReason::NaturalExit;
    let absolute_elapsed_bound = request
        .timeout
        .saturating_add(request.cleanup_timeout)
        .saturating_add(Duration::from_secs(1));
    let reason_fields_valid = match capture.termination_reason {
        TerminationReason::NaturalExit => {
            capture.sent_signal.is_none()
                && capture.adapter_failure.is_none()
                && capture.cleanup_status == CleanupStatus::Complete
                && capture.elapsed <= request.timeout
        }
        TerminationReason::DeadlineExceeded => capture.elapsed >= request.timeout,
        TerminationReason::CaptureFailure => capture.adapter_failure.is_some(),
    };
    mutually_exclusive_status
        && sent_signal_valid
        && cleanup_reason_valid
        && capture.elapsed <= absolute_elapsed_bound
        && reason_fields_valid
}

fn valid_identity(capture: &ProcessCapture) -> bool {
    capture.executable_identity.canonical_path().is_absolute()
        && is_sha256(capture.executable_identity.sha256())
        && capture.executable_identity.mode & 0o170_000 == 0o100_000
        && capture.executable_identity.mode & 0o111 != 0
        && capture.executable_identity.local_filesystem_verified
        && capture
            .working_directory_identity
            .canonical_path()
            .is_absolute()
        && capture.working_directory_identity.mode & 0o170_000 == 0o040_000
        && capture.working_directory_identity.local_filesystem_verified
}

fn validate_stream(
    name: &str,
    stream: &ProcessStreamCapture,
    retained_limit: usize,
    failures: &mut Vec<ProcessFailure>,
) {
    let retained = stream.retained_prefix.len();
    let coherent_count =
        u64::try_from(retained).is_ok_and(|retained| retained <= stream.total_observed_bytes);
    let expected_truncation =
        u64::try_from(retained).is_ok_and(|retained| stream.total_observed_bytes > retained);
    let complete_digest_valid = if stream.complete {
        stream
            .full_stream_sha256
            .as_ref()
            .is_some_and(|digest| digest == &stream.bytes_seen_sha256 && is_sha256(digest))
    } else {
        stream.full_stream_sha256.is_none()
    };
    let complete_prefix_digest_valid = if stream.complete && !stream.retained_prefix_truncated {
        stream.bytes_seen_sha256 == sha256_bytes(&stream.retained_prefix)
    } else {
        is_sha256(&stream.bytes_seen_sha256)
    };
    if retained > retained_limit
        || !coherent_count
        || stream.retained_prefix_truncated != expected_truncation
        || !complete_digest_valid
        || !complete_prefix_digest_valid
    {
        failures.push(failure(
            ProcessFailureCode::InvalidCaptureEvidence,
            format!("{name} counts, completion flags, or digests were inconsistent"),
        ));
    }
    if !stream.complete {
        failures.push(failure(
            ProcessFailureCode::InvalidCaptureEvidence,
            format!("{name} did not reach EOF"),
        ));
    }
    if stream.retained_prefix_truncated {
        failures.push(failure(
            ProcessFailureCode::OutputLimitExceeded,
            format!("{name} exceeded the {retained_limit}-byte retained-prefix limit"),
        ));
    }
}

fn classify_transcript(
    stream: &str,
    bytes: &[u8],
    request: &ProcessRequest,
    policy: TranscriptPolicy,
    failures: &mut Vec<ProcessFailure>,
) {
    let Ok(text) = std::str::from_utf8(bytes) else {
        failures.push(failure(
            ProcessFailureCode::NonUtf8Transcript,
            format!("{stream} retained prefix was not UTF-8"),
        ));
        return;
    };
    match policy.profile {
        TranscriptProfile::Operation => classify_operation_transcript(stream, text, failures),
        TranscriptProfile::Version => classify_version_transcript(stream, text, failures),
        TranscriptProfile::AdvancedHelp => {
            classify_advanced_help_transcript(stream, text, failures);
        }
        TranscriptProfile::ProjectImport => {
            classify_project_import_transcript(stream, text, request, failures);
        }
        TranscriptProfile::JsonExport => {
            classify_json_export_transcript(stream, text, request, failures);
        }
        TranscriptProfile::MissingImagesPathControl => {
            classify_missing_images_path_transcript(stream, text, request, failures);
        }
        TranscriptProfile::AnimationImport => {
            classify_animation_import_transcript(stream, text, request, failures);
        }
        TranscriptProfile::NewAnimationCollisionControl => {
            classify_new_animation_collision_transcript(stream, text, request, failures);
        }
        TranscriptProfile::ProjectInfo => {
            classify_project_info_transcript(stream, text, request, failures);
        }
    }
}

fn classify_operation_transcript(stream: &str, text: &str, failures: &mut Vec<ProcessFailure>) {
    for (index, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let lowercase = line.to_ascii_lowercase();
        if BLOCKING_TERMS.iter().any(|term| lowercase.contains(term)) {
            failures.push(failure(
                ProcessFailureCode::BlockingDiagnostic,
                format!(
                    "{stream} line {}: {}",
                    index + 1,
                    sanitized_transcript_line(line)
                ),
            ));
        } else if !SPINE_4_3_23_INFORMATIONAL_LINES.contains(&line) {
            failures.push(failure(
                ProcessFailureCode::UnknownTranscriptLine,
                format!(
                    "{stream} line {}: {}",
                    index + 1,
                    sanitized_transcript_line(line)
                ),
            ));
        }
    }
}

fn sanitized_transcript_line(line: &str) -> &str {
    if line.to_ascii_lowercase().starts_with("licensed to:") {
        "Licensed to: <redacted>"
    } else {
        line
    }
}

fn classify_version_transcript(stream: &str, text: &str, failures: &mut Vec<ProcessFailure>) {
    if stream == "stderr" {
        require_empty_profile_stream("version", stream, text, failures);
        return;
    }
    let Some(body) = checked_editor_session_header("version", text, failures) else {
        return;
    };
    if body != "Complete.\n" {
        transcript_contract_failure(
            failures,
            "version stdout did not prove exact Spine 4.3.23 Professional activation",
        );
    }
}

fn classify_advanced_help_transcript(stream: &str, text: &str, failures: &mut Vec<ProcessFailure>) {
    if stream == "stderr" {
        require_empty_profile_stream("advanced-help", stream, text, failures);
        return;
    }
    let Some(body) = checked_spine_header("advanced-help", text, failures) else {
        return;
    };
    let Some(body) = body.strip_prefix('\n') else {
        transcript_contract_failure(
            failures,
            "advanced-help stdout lacked the reviewed header separator",
        );
        return;
    };
    if body != SPINE_4_3_23_ADVANCED_HELP {
        transcript_contract_failure(
            failures,
            "advanced-help stdout differed from the checked-in 4.3.23 capability contract",
        );
    }
}

fn classify_project_import_transcript(
    stream: &str,
    text: &str,
    request: &ProcessRequest,
    failures: &mut Vec<ProcessFailure>,
) {
    if stream == "stderr" {
        require_empty_profile_stream("project-import", stream, text, failures);
        return;
    }
    let Some(body) = checked_editor_session_header("project-import", text, failures) else {
        return;
    };
    let Some(input) = request_argument(request, "--input") else {
        transcript_contract_failure(failures, "project-import request lacked --input");
        return;
    };
    let Some(output) = request_argument(request, "--output") else {
        transcript_contract_failure(failures, "project-import request lacked --output");
        return;
    };
    let (Some(input_name), Some(output_stem)) = (path_file_name(input), path_file_stem(output))
    else {
        transcript_contract_failure(
            failures,
            "project-import input/output lacked a UTF-8 file identity",
        );
        return;
    };
    let expected = format!("Project import: {input_name} into {output_stem}\nComplete.\n");
    require_exact_operation_body("project-import", body, &expected, failures);
}

fn classify_json_export_transcript(
    stream: &str,
    text: &str,
    request: &ProcessRequest,
    failures: &mut Vec<ProcessFailure>,
) {
    if stream == "stderr" {
        require_empty_profile_stream("json-export", stream, text, failures);
        return;
    }
    let Some(body) = checked_editor_session_header("json-export", text, failures) else {
        return;
    };
    let Some(input_stem) = request_argument(request, "--input").and_then(path_file_stem) else {
        transcript_contract_failure(failures, "json-export input lacked a UTF-8 file stem");
        return;
    };
    let expected = format!("JSON export: {input_stem}\nComplete.\n");
    require_exact_operation_body("json-export", body, &expected, failures);
}

fn classify_missing_images_path_transcript(
    stream: &str,
    text: &str,
    request: &ProcessRequest,
    failures: &mut Vec<ProcessFailure>,
) {
    if stream == "stderr" {
        require_empty_profile_stream("missing-images-path-control", stream, text, failures);
        return;
    }
    let Some(body) = checked_editor_session_header("missing-images-path-control", text, failures)
    else {
        return;
    };
    let Some(input_stem) = request_argument(request, "--input").and_then(path_file_stem) else {
        transcript_contract_failure(
            failures,
            "missing-images-path-control input lacked a UTF-8 file stem",
        );
        return;
    };
    let expected =
        format!("JSON export: {input_stem}\nImages path not found: ./images/\nComplete.\n");
    if body == expected {
        failures.push(failure(
            ProcessFailureCode::BlockingDiagnostic,
            "stdout contained the exact expected `Images path not found: ./images/` diagnostic",
        ));
    } else {
        transcript_contract_failure(
            failures,
            "missing-images-path-control stdout differed from its exact diagnostic contract",
        );
    }
}

fn classify_animation_import_transcript(
    stream: &str,
    text: &str,
    request: &ProcessRequest,
    failures: &mut Vec<ProcessFailure>,
) {
    if stream == "stderr" {
        require_empty_profile_stream("animation-import", stream, text, failures);
        return;
    }
    let Some(body) = checked_editor_session_header("animation-import", text, failures) else {
        return;
    };
    let values =
        ["--input", "--output", "--to", "--animation"].map(|flag| request_argument(request, flag));
    let [
        Some(input),
        Some(output),
        Some(destination_skeleton),
        Some(animation),
    ] = values
    else {
        transcript_contract_failure(
            failures,
            "animation-import request lacked a required scoped argument",
        );
        return;
    };
    let (Some(input_stem), Some(output_stem)) = (path_file_stem(input), path_file_stem(output))
    else {
        transcript_contract_failure(
            failures,
            "animation-import input/output lacked a UTF-8 file stem",
        );
        return;
    };
    let expected = format!(
        "Animation import: {input_stem} into {output_stem} ({destination_skeleton})\nImported animation: {animation}\nComplete.\n"
    );
    require_exact_operation_body("animation-import", body, &expected, failures);
}

fn classify_new_animation_collision_transcript(
    stream: &str,
    text: &str,
    request: &ProcessRequest,
    failures: &mut Vec<ProcessFailure>,
) {
    if stream == "stderr" {
        require_empty_profile_stream("new-animation-collision-control", stream, text, failures);
        return;
    }
    let Some(body) =
        checked_editor_session_header("new-animation-collision-control", text, failures)
    else {
        return;
    };
    if parse_new_animation_collision_body(body, request).is_some() {
        failures.push(failure(
            ProcessFailureCode::BlockingDiagnostic,
            "stdout contained the exact expected new-animation collision diagnostic",
        ));
    } else {
        transcript_contract_failure(
            failures,
            "new-animation-collision-control stdout differed from its exact request-bound contract",
        );
    }
}

fn derive_new_animation_collision(
    request: &ProcessRequest,
    stdout: &[u8],
    policy: TranscriptPolicy,
    assessment: &ProcessAssessment,
) -> Option<NewAnimationCollisionEvidence> {
    let expected_collision_failure = matches!(
        assessment.failures(),
        [ProcessFailure {
            code: ProcessFailureCode::BlockingDiagnostic,
            ..
        }]
    );
    if policy.profile() != TranscriptProfile::NewAnimationCollisionControl
        || !expected_collision_failure
    {
        return None;
    }
    let text = std::str::from_utf8(stdout).ok()?;
    let body = editor_session_body(text).ok()?;
    parse_new_animation_collision_body(body, request)
}

fn parse_new_animation_collision_body(
    body: &str,
    request: &ProcessRequest,
) -> Option<NewAnimationCollisionEvidence> {
    let input = request_argument(request, "--input")?;
    let output = request_argument(request, "--output")?;
    let destination_skeleton = request_argument(request, "--to")?;
    let requested_animation = request_argument(request, "--animation")?;
    if !safe_animation_name(requested_animation) {
        return None;
    }
    let input_stem = path_file_stem(input)?;
    let output_stem = path_file_stem(output)?;

    let ordinary_line =
        format!("Animation import: {input_stem} into {output_stem} ({destination_skeleton})\n");
    let imported_line = format!("Imported animation: {requested_animation}\n");
    let diagnostic_prefix =
        format!("An animation with this name already exists: {requested_animation} -> ");
    let renamed_animation = body
        .strip_prefix(&ordinary_line)?
        .strip_prefix(&imported_line)?
        .strip_prefix(&diagnostic_prefix)?
        .strip_suffix("\nComplete.\n")?;
    if !safe_animation_name(renamed_animation) || renamed_animation == requested_animation {
        return None;
    }
    Some(NewAnimationCollisionEvidence {
        requested_animation: requested_animation.to_owned(),
        renamed_animation: renamed_animation.to_owned(),
    })
}

fn safe_animation_name(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && !value.starts_with('-')
        && !value.chars().any(char::is_control)
}

fn classify_project_info_transcript(
    stream: &str,
    text: &str,
    request: &ProcessRequest,
    failures: &mut Vec<ProcessFailure>,
) {
    if stream == "stderr" {
        require_empty_profile_stream("project-info", stream, text, failures);
        return;
    }
    let Some(body) = checked_editor_session_header("project-info", text, failures) else {
        return;
    };
    let Some(input) = request_argument(request, "--input") else {
        transcript_contract_failure(failures, "project-info request lacked --input");
        return;
    };
    if parse_project_info_body(body, input).is_err() {
        transcript_contract_failure(
            failures,
            "project-info stdout contained missing, malformed, or unreviewed inventory lines",
        );
    }
}

const PROJECT_INFO_SECTIONS: &[(ProjectInfoSection, &str)] = &[
    (ProjectInfoSection::Bones, "    Bones ("),
    (ProjectInfoSection::Slots, "    Slots ("),
    (ProjectInfoSection::Skins, "    Skins ("),
    (ProjectInfoSection::Events, "    Events ("),
    (ProjectInfoSection::IkConstraints, "    IK constraints ("),
    (
        ProjectInfoSection::TransformConstraints,
        "    Transform constraints (",
    ),
    (
        ProjectInfoSection::PathConstraints,
        "    Path constraints (",
    ),
    (
        ProjectInfoSection::PhysicsConstraints,
        "    Physics constraints (",
    ),
    (ProjectInfoSection::Animations, "    Animations ("),
];

fn parse_project_info_body(
    body: &str,
    expected_project: &str,
) -> Result<ProjectInfoInventory, ProjectInfoError> {
    let body = body
        .strip_suffix('\n')
        .ok_or(ProjectInfoError::MalformedTranscript)?;
    let mut lines = body.split('\n');
    let expected_project_line = format!("Project info: {expected_project}");
    if lines.next() != Some(expected_project_line.as_str())
        || lines.next() != Some("  Spine version: 4.3.23")
    {
        return Err(ProjectInfoError::MalformedTranscript);
    }
    let fps = lines
        .next()
        .and_then(|line| line.strip_prefix("  Dopesheet FPS: "))
        .ok_or(ProjectInfoError::MalformedTranscript)?;
    if !fps
        .parse::<f64>()
        .ok()
        .is_some_and(|value| value.is_finite() && value > 0.0)
    {
        return Err(ProjectInfoError::MalformedTranscript);
    }

    let mut detail_lines = lines.collect::<Vec<_>>();
    if detail_lines.pop() != Some("Complete.") || detail_lines.is_empty() {
        return Err(ProjectInfoError::MalformedTranscript);
    }
    let mut skeletons = Vec::<ProjectSkeletonInventory>::new();
    let mut skeleton_names = BTreeSet::new();
    for line in detail_lines {
        if let Some(name) = line.strip_prefix("  Skeleton: ") {
            if !valid_info_name(name) || !skeleton_names.insert(name.to_owned()) {
                return Err(ProjectInfoError::MalformedTranscript);
            }
            skeletons.push(ProjectSkeletonInventory {
                name: name.to_owned(),
                size: String::new(),
                sections: BTreeMap::new(),
            });
            continue;
        }
        let skeleton = skeletons
            .last_mut()
            .ok_or(ProjectInfoError::MalformedTranscript)?;
        if let Some(size) = line.strip_prefix("    Size: ") {
            if !skeleton.size.is_empty() || !valid_info_name(size) {
                return Err(ProjectInfoError::MalformedTranscript);
            }
            skeleton.size = size.to_owned();
            continue;
        }
        let (section, list) = parse_project_info_list(line)?;
        if skeleton.sections.insert(section, list).is_some() {
            return Err(ProjectInfoError::MalformedTranscript);
        }
    }
    if skeletons.is_empty()
        || skeletons.iter().any(|skeleton| {
            skeleton.size.is_empty()
                || !skeleton.sections.contains_key(&ProjectInfoSection::Bones)
                || !skeleton
                    .sections
                    .contains_key(&ProjectInfoSection::Animations)
        })
    {
        return Err(ProjectInfoError::MalformedTranscript);
    }
    Ok(ProjectInfoInventory {
        project: PathBuf::from(expected_project),
        spine_version: TARGET_SPINE_VERSION.to_owned(),
        dopesheet_fps: fps.to_owned(),
        skeletons,
    })
}

const TARGET_SPINE_VERSION: &str = "4.3.23";

fn parse_project_info_list(
    line: &str,
) -> Result<(ProjectInfoSection, ProjectInfoList), ProjectInfoError> {
    let (section, rest) = PROJECT_INFO_SECTIONS
        .iter()
        .find_map(|(section, prefix)| line.strip_prefix(prefix).map(|rest| (*section, rest)))
        .ok_or(ProjectInfoError::MalformedTranscript)?;
    let (count, raw_values) = rest
        .split_once("): ")
        .ok_or(ProjectInfoError::MalformedTranscript)?;
    let reported_count = count
        .parse::<usize>()
        .map_err(|_| ProjectInfoError::MalformedTranscript)?;
    let values = if reported_count == 0 {
        if raw_values != "<none>" {
            return Err(ProjectInfoError::MalformedTranscript);
        }
        Vec::new()
    } else {
        let values = raw_values
            .split(", ")
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let unique = values.iter().collect::<BTreeSet<_>>();
        if values.len() != reported_count
            || unique.len() != values.len()
            || values.iter().any(|value| !valid_info_name(value))
        {
            return Err(ProjectInfoError::MalformedTranscript);
        }
        values
    };
    Ok((
        section,
        ProjectInfoList {
            reported_count,
            values,
        },
    ))
}

fn valid_info_name(value: &str) -> bool {
    !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control)
}

fn request_argument<'a>(request: &'a ProcessRequest, flag: &str) -> Option<&'a str> {
    argument_value(&request.args, flag)
}

fn argument_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

fn path_file_name(path: &str) -> Option<&str> {
    Path::new(path).file_name().and_then(|value| value.to_str())
}

fn path_file_stem(path: &str) -> Option<&str> {
    Path::new(path).file_stem().and_then(|value| value.to_str())
}

fn require_exact_operation_body(
    profile: &str,
    body: &str,
    expected: &str,
    failures: &mut Vec<ProcessFailure>,
) {
    if body != expected {
        transcript_contract_failure(
            failures,
            format!("{profile} stdout differed from its exact typed operation contract"),
        );
    }
}

fn checked_editor_session_header<'a>(
    profile: &str,
    text: &'a str,
    failures: &mut Vec<ProcessFailure>,
) -> Option<&'a str> {
    match editor_session_body(text) {
        Ok(body) => Some(body),
        Err(()) => {
            transcript_contract_failure(
                failures,
                format!(
                    "{profile} stdout lacked exact hidden-license Spine 4.3.23 session headers"
                ),
            );
            None
        }
    }
}

fn editor_session_body(text: &str) -> Result<&str, ()> {
    let body = spine_header_body(text)?;
    let prefix = concat!(
        "Starting: Spine 4.3.23 Professional\n",
        "Spine 4.3.23 Professional\n",
        "Licensed to: <hidden>\n"
    );
    body.strip_prefix(prefix).ok_or(())
}

fn checked_spine_header<'a>(
    profile: &str,
    text: &'a str,
    failures: &mut Vec<ProcessFailure>,
) -> Option<&'a str> {
    match spine_header_body(text) {
        Ok(body) => Some(body),
        Err(()) => {
            transcript_contract_failure(
                failures,
                format!("{profile} stdout had an unreviewed launcher or platform header"),
            );
            None
        }
    }
}

fn spine_header_body(text: &str) -> Result<&str, ()> {
    let Some((launcher, rest)) = text.split_once('\n') else {
        return Err(());
    };
    let Some((copyright, rest)) = rest.split_once('\n') else {
        return Err(());
    };
    let Some((platform, body)) = rest.split_once('\n') else {
        return Err(());
    };
    let platform_version = platform.strip_prefix("Mac OS X aarch64 ");
    let platform_valid = platform_version.is_some_and(|version| {
        !version.is_empty()
            && version
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.')
            && version.split('.').all(|part| !part.is_empty())
    });
    if launcher != SPINE_LAUNCHER_HEADER || copyright != SPINE_COPYRIGHT_HEADER || !platform_valid {
        return Err(());
    }
    Ok(body)
}

fn require_empty_profile_stream(
    profile: &str,
    stream: &str,
    text: &str,
    failures: &mut Vec<ProcessFailure>,
) {
    if !text.is_empty() {
        transcript_contract_failure(
            failures,
            format!("{profile} {stream} was not empty under the checked-in contract"),
        );
    }
}

fn transcript_contract_failure(failures: &mut Vec<ProcessFailure>, detail: impl Into<String>) {
    failures.push(failure(
        ProcessFailureCode::TranscriptContractMismatch,
        detail,
    ));
}

fn failure(code: ProcessFailureCode, detail: impl Into<String>) -> ProcessFailure {
    ProcessFailure {
        code,
        detail: detail.into(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[derive(Clone)]
    struct FakeExecutor(ProcessCapture);

    impl ProcessExecutor for FakeExecutor {
        fn execute(
            &self,
            _request: &ProcessRequest,
        ) -> Result<ProcessCapture, ProcessExecutionError> {
            Ok(self.0.clone())
        }
    }

    pub(crate) fn request() -> ProcessRequest {
        ProcessRequest {
            operation: "generic-export".to_owned(),
            program: "/evidence/editor".to_owned(),
            args: vec!["--fixed-version".to_owned()],
            working_directory: PathBuf::from("/evidence/work"),
            environment: BTreeMap::from([("LANG".to_owned(), "C".to_owned())]),
            timeout: Duration::from_secs(30),
            cleanup_timeout: Duration::from_secs(2),
            max_retained_bytes_per_stream: 1024,
            required_outputs: BTreeSet::from(["skeleton.json".to_owned()]),
        }
    }

    fn stream(bytes: &[u8]) -> ProcessStreamCapture {
        let sha256 = sha256_bytes(bytes);
        ProcessStreamCapture {
            retained_prefix: bytes.to_vec(),
            total_observed_bytes: bytes.len() as u64,
            bytes_seen_sha256: sha256.clone(),
            full_stream_sha256: Some(sha256),
            retained_prefix_truncated: false,
            complete: true,
        }
    }

    pub(crate) fn capture() -> ProcessCapture {
        ProcessCapture {
            exit_code: Some(0),
            terminating_signal: None,
            sent_signal: None,
            termination_reason: TerminationReason::NaturalExit,
            elapsed: Duration::from_millis(10),
            cleanup_status: CleanupStatus::Complete,
            adapter_failure: None,
            stdout: stream(b""),
            stderr: stream(b""),
            observed_outputs: BTreeSet::from(["skeleton.json".to_owned()]),
            output_discovery_state: OutputDiscoveryState::Complete,
            executable_identity: ExecutableIdentity::new(
                PathBuf::from("/evidence/editor"),
                "0".repeat(64),
                6,
                1,
                2,
                0o100700,
                0,
                0,
                0,
                0,
                0,
            ),
            working_directory_identity: WorkingDirectoryIdentity::new(
                PathBuf::from("/evidence/work"),
                1,
                3,
                0o040700,
                0,
            ),
            lock_evidence: Some(LockEvidence::new_acquired(
                PathBuf::from("/evidence/lock/editor.lock"),
                Duration::from_millis(1),
                1,
                4,
                "test-local".to_owned(),
            )),
        }
    }

    fn assess(capture: ProcessCapture) -> ProcessEvidence {
        execute_and_assess(
            &FakeExecutor(capture),
            &request(),
            TranscriptPolicy::spine_4_3_23(),
        )
        .expect("fake execution")
    }

    fn assess_with_policy(capture: ProcessCapture, policy: TranscriptPolicy) -> ProcessEvidence {
        let mut request = request();
        request.max_retained_bytes_per_stream = MAX_RETAINED_BYTES_PER_STREAM;
        assess_request_with_policy(request, capture, policy)
    }

    fn assess_request_with_policy(
        mut request: ProcessRequest,
        mut capture: ProcessCapture,
        policy: TranscriptPolicy,
    ) -> ProcessEvidence {
        request.max_retained_bytes_per_stream = MAX_RETAINED_BYTES_PER_STREAM;
        capture.observed_outputs = request.required_outputs.clone();
        capture.output_discovery_state = OutputDiscoveryState::Complete;
        execute_and_assess(&FakeExecutor(capture), &request, policy).expect("fake execution")
    }

    fn reviewed_header(body: &str) -> String {
        format!(
            "{SPINE_LAUNCHER_HEADER}\n{SPINE_COPYRIGHT_HEADER}\nMac OS X aarch64 26.5.2\n{body}"
        )
    }

    fn reviewed_session(body: &str) -> String {
        reviewed_header(&format!(
            "Starting: Spine 4.3.23 Professional\nSpine 4.3.23 Professional\nLicensed to: <hidden>\n{body}"
        ))
    }

    #[test]
    fn accepts_only_a_complete_quiet_locked_success() {
        assert!(assess(capture()).assessment().passed());
    }

    #[test]
    fn rejects_executor_output_claims_without_trusted_discovery() {
        let mut value = capture();
        value.output_discovery_state = OutputDiscoveryState::NotPerformed;
        assert_has_failure(
            assess(value),
            ProcessFailureCode::OutputDiscoveryNotPerformed,
        );
    }

    #[test]
    fn rejects_nonzero_exit_timeout_missing_lock_and_cleanup_failure() {
        let mut value = capture();
        value.exit_code = Some(7);
        assert_has_failure(assess(value), ProcessFailureCode::NonzeroExit);

        let mut value = capture();
        value.exit_code = None;
        value.terminating_signal = Some(9);
        value.sent_signal = Some(9);
        value.termination_reason = TerminationReason::DeadlineExceeded;
        assert_has_failure(assess(value), ProcessFailureCode::TimedOut);

        let mut value = capture();
        value.lock_evidence = None;
        assert_has_failure(assess(value), ProcessFailureCode::MissingLockEvidence);

        let mut value = capture();
        value.cleanup_status = CleanupStatus::ReaperDelegated;
        assert_has_failure(assess(value), ProcessFailureCode::CleanupIncomplete);
    }

    #[test]
    fn rejects_diagnostics_unknown_text_and_missing_output() {
        let mut value = capture();
        value.stdout = stream(b"Images path not found: ./images\n");
        assert_has_failure(assess(value), ProcessFailureCode::BlockingDiagnostic);

        let mut value = capture();
        value.stderr = stream(b"unreviewed status text\n");
        assert_has_failure(assess(value), ProcessFailureCode::UnknownTranscriptLine);

        let mut value = capture();
        value.observed_outputs.clear();
        assert_has_failure(assess(value), ProcessFailureCode::MissingOutput);
    }

    #[test]
    fn exact_version_profile_proves_hidden_professional_activation() {
        let mut value = capture();
        value.stdout = stream(
            reviewed_header(concat!(
                "Starting: Spine 4.3.23 Professional\n",
                "Spine 4.3.23 Professional\n",
                "Licensed to: <hidden>\n",
                "Complete.\n"
            ))
            .as_bytes(),
        );
        let evidence = assess_with_policy(value, TranscriptPolicy::spine_4_3_23_version());
        assert!(evidence.assessment().passed());
    }

    #[test]
    fn version_profile_rejects_wrong_version_license_disclosure_and_stderr() {
        for body in [
            concat!(
                "Starting: Spine 4.3.22 Professional\n",
                "Spine 4.3.22 Professional\n",
                "Licensed to: <hidden>\n",
                "Complete.\n"
            ),
            concat!(
                "Starting: Spine 4.3.23 Professional\n",
                "Spine 4.3.23 Professional\n",
                "Licensed to: Example Person\n",
                "Complete.\n"
            ),
        ] {
            let mut value = capture();
            value.stdout = stream(reviewed_header(body).as_bytes());
            assert_has_failure(
                assess_with_policy(value, TranscriptPolicy::spine_4_3_23_version()),
                ProcessFailureCode::TranscriptContractMismatch,
            );
        }

        let mut value = capture();
        value.stdout = stream(
            reviewed_header(concat!(
                "Starting: Spine 4.3.23 Professional\n",
                "Spine 4.3.23 Professional\n",
                "Licensed to: <hidden>\n",
                "Complete.\n"
            ))
            .as_bytes(),
        );
        value.stderr = stream(b"unexpected\n");
        assert_has_failure(
            assess_with_policy(value, TranscriptPolicy::spine_4_3_23_version()),
            ProcessFailureCode::TranscriptContractMismatch,
        );
    }

    #[test]
    fn advanced_help_profile_is_bound_to_the_reviewed_capability_contract() {
        let mut value = capture();
        value.stdout =
            stream(reviewed_header(&format!("\n{SPINE_4_3_23_ADVANCED_HELP}")).as_bytes());
        let evidence = assess_with_policy(
            value.clone(),
            TranscriptPolicy::spine_4_3_23_advanced_help(),
        );
        assert!(evidence.assessment().passed());

        let changed = reviewed_header(&format!(
            "\n{}",
            SPINE_4_3_23_ADVANCED_HELP.replacen("--animation", "--animations", 1)
        ));
        value.stdout = stream(changed.as_bytes());
        assert_has_failure(
            assess_with_policy(value, TranscriptPolicy::spine_4_3_23_advanced_help()),
            ProcessFailureCode::TranscriptContractMismatch,
        );
    }

    #[test]
    fn typed_operation_profiles_match_reviewed_spine_transcripts() {
        let environment = BTreeMap::from([("LANG".to_owned(), "C".to_owned())]);
        let commands_and_bodies = [
            (
                crate::SpineCommand::reconstruct_json(
                    "/staged/export/Character.json",
                    "/staged/reconstructed.spine",
                    "Character",
                )
                .expect("reconstruct"),
                "Project import: Character.json into reconstructed\nComplete.\n".to_owned(),
            ),
            (
                crate::SpineCommand::export_json(
                    "/staged/current.spine",
                    &crate::JsonExportTarget::new("/staged/export", "Character")
                        .expect("export target"),
                    "/staged/preset/export.json",
                )
                .expect("export"),
                "JSON export: current\nComplete.\n".to_owned(),
            ),
            (
                crate::SpineCommand::import_existing_animation(
                    "/staged/submission.spine",
                    "/staged/candidate.spine",
                    "Character",
                    "Character",
                    "idle",
                )
                .expect("animation import"),
                concat!(
                    "Animation import: submission into candidate (Character)\n",
                    "Imported animation: idle\n",
                    "Complete.\n"
                )
                .to_owned(),
            ),
            (
                crate::SpineCommand::project_info("/staged/current.spine").expect("project info"),
                concat!(
                    "Project info: /staged/current.spine\n",
                    "  Spine version: 4.3.23\n",
                    "  Dopesheet FPS: 30\n",
                    "  Skeleton: Character\n",
                    "    Size: <unknown>\n",
                    "    Bones (1): root\n",
                    "    Slots (0): <none>\n",
                    "    Animations (1): idle\n",
                    "Complete.\n"
                )
                .to_owned(),
            ),
        ];

        for (command, body) in commands_and_bodies {
            let request = command
                .process_request("/evidence/editor", "/evidence/work", environment.clone())
                .expect("typed request");
            let mut value = capture();
            value.stdout = stream(reviewed_session(&body).as_bytes());
            let evidence = assess_request_with_policy(request, value, command.transcript_policy());
            assert!(
                evidence.assessment().passed(),
                "operation failed: {:?}: {:?}",
                command.kind(),
                evidence.assessment().failures()
            );
        }
    }

    fn collision_control() -> (crate::SpineCommand, ProcessRequest) {
        let command = crate::SpineCommand::new_animation_collision_control(
            "/staged/new-submission.spine",
            "/staged/character.spine",
            "Submission Rig",
            "Current Rig",
            "gesture",
        )
        .expect("collision command");
        let request = command
            .process_request("/evidence/editor", "/evidence/work", BTreeMap::new())
            .expect("collision request");
        (command, request)
    }

    fn collision_body(renamed: &str) -> String {
        format!(
            concat!(
                "Animation import: new-submission into character (Current Rig)\n",
                "Imported animation: gesture\n",
                "An animation with this name already exists: gesture -> {}\n",
                "Complete.\n"
            ),
            renamed
        )
    }

    #[test]
    fn collision_control_records_one_expected_failure_and_a_typed_safe_rename() {
        let (command, request) = collision_control();
        let mut value = capture();
        value.stdout = stream(reviewed_session(&collision_body("gesture2")).as_bytes());
        let evidence = assess_request_with_policy(request, value, command.transcript_policy());

        assert!(!evidence.assessment().passed());
        assert_eq!(evidence.assessment().failures().len(), 1);
        assert_eq!(
            evidence.assessment().failures()[0].code,
            ProcessFailureCode::BlockingDiagnostic
        );
        assert_eq!(
            evidence.transcript_profile(),
            TranscriptProfile::NewAnimationCollisionControl
        );
        let collision = evidence
            .new_animation_collision()
            .expect("typed collision evidence");
        assert_eq!(collision.requested_animation(), "gesture");
        assert_eq!(collision.renamed_animation(), "gesture2");
        let serialized = serde_json::to_value(&evidence).expect("serialized evidence");
        assert_eq!(
            serialized["new_animation_collision"]["renamed_animation"],
            "gesture2"
        );
    }

    #[test]
    fn ordinary_animation_import_rejects_the_collision_diagnostic() {
        let command = crate::SpineCommand::import_new_animation(
            "/staged/new-submission.spine",
            "/staged/character.spine",
            "Submission Rig",
            "Current Rig",
            "gesture",
        )
        .expect("ordinary import");
        let request = command
            .process_request("/evidence/editor", "/evidence/work", BTreeMap::new())
            .expect("ordinary request");
        let mut value = capture();
        value.stdout = stream(reviewed_session(&collision_body("gesture2")).as_bytes());
        let evidence = assess_request_with_policy(request, value, command.transcript_policy());

        assert_has_failure_ref(&evidence, ProcessFailureCode::TranscriptContractMismatch);
        assert!(evidence.new_animation_collision().is_none());
    }

    #[test]
    fn collision_control_rejects_wrong_names_unsafe_renames_and_extra_text() {
        let (command, request) = collision_control();
        let invalid_bodies = [
            "Animation import: other into character (Current Rig)\nImported animation: gesture\nAn animation with this name already exists: gesture -> gesture2\nComplete.\n".to_owned(),
            "Animation import: new-submission into other (Current Rig)\nImported animation: gesture\nAn animation with this name already exists: gesture -> gesture2\nComplete.\n".to_owned(),
            "Animation import: new-submission into character (Other Rig)\nImported animation: gesture\nAn animation with this name already exists: gesture -> gesture2\nComplete.\n".to_owned(),
            "Animation import: new-submission into character (Current Rig)\nImported animation: other\nAn animation with this name already exists: gesture -> gesture2\nComplete.\n".to_owned(),
            "Animation import: new-submission into character (Current Rig)\nImported animation: gesture\nAn animation with this name already exists: other -> gesture2\nComplete.\n".to_owned(),
            collision_body(""),
            collision_body(" gesture2"),
            collision_body("gesture2 "),
            collision_body("-gesture2"),
            collision_body("gesture\t2"),
            collision_body("gesture"),
            format!("{}Extra.\n", collision_body("gesture2")),
            collision_body("gesture2\nUnexpected line"),
            concat!(
                "Animation import: new-submission into character (Current Rig)\n",
                "An animation with this name already exists: gesture -> gesture2\n",
                "Complete.\n"
            )
            .to_owned(),
            concat!(
                "Animation import: new-submission into character (Current Rig)\n",
                "Imported animation: gesture\n",
                "An animation with this name already exists: gesture -> gesture2\n",
                "An animation with this name already exists: gesture -> gesture3\n",
                "Complete.\n"
            )
            .to_owned(),
        ];

        for body in invalid_bodies {
            let mut value = capture();
            value.stdout = stream(reviewed_session(&body).as_bytes());
            let evidence =
                assess_request_with_policy(request.clone(), value, command.transcript_policy());
            assert_has_failure_ref(&evidence, ProcessFailureCode::TranscriptContractMismatch);
            assert!(
                evidence.new_animation_collision().is_none(),
                "body: {body:?}"
            );
        }
    }

    #[test]
    fn collision_evidence_requires_zero_exit_and_clean_stderr() {
        let (command, request) = collision_control();

        let mut nonzero = capture();
        nonzero.exit_code = Some(7);
        nonzero.stdout = stream(reviewed_session(&collision_body("gesture2")).as_bytes());
        let evidence =
            assess_request_with_policy(request.clone(), nonzero, command.transcript_policy());
        assert_has_failure_ref(&evidence, ProcessFailureCode::NonzeroExit);
        assert!(evidence.new_animation_collision().is_none());

        let mut dirty_stderr = capture();
        dirty_stderr.stdout = stream(reviewed_session(&collision_body("gesture2")).as_bytes());
        dirty_stderr.stderr = stream(b"unexpected\n");
        let evidence =
            assess_request_with_policy(request, dirty_stderr, command.transcript_policy());
        assert_has_failure_ref(&evidence, ProcessFailureCode::TranscriptContractMismatch);
        assert!(evidence.new_animation_collision().is_none());
    }

    #[test]
    fn typed_operation_profiles_reject_cross_operation_and_extra_lines() {
        let command = crate::SpineCommand::export_json(
            "/staged/current.spine",
            &crate::JsonExportTarget::new("/staged/export", "Character").expect("export target"),
            "/staged/preset/export.json",
        )
        .expect("export");
        let request = command
            .process_request("/evidence/editor", "/evidence/work", BTreeMap::new())
            .expect("typed request");
        for body in [
            "JSON export: another-project\nComplete.\n",
            "JSON export: current\nImages path not found: ./images\nComplete.\n",
            "Project import: Character.json into current\nComplete.\n",
        ] {
            let mut value = capture();
            value.stdout = stream(reviewed_session(body).as_bytes());
            assert_has_failure(
                assess_request_with_policy(request.clone(), value, command.transcript_policy()),
                ProcessFailureCode::TranscriptContractMismatch,
            );
        }
    }

    #[test]
    fn missing_images_control_accepts_only_the_exact_reviewed_diagnostic() {
        let command = crate::SpineCommand::missing_images_path_control(
            "/staged/negative/current.spine",
            &crate::JsonExportTarget::new("/staged/export", "Character").expect("export target"),
            "/staged/preset/export.json",
        )
        .expect("negative control");
        let request = command
            .process_request("/evidence/editor", "/evidence/work", BTreeMap::new())
            .expect("typed request");

        let mut exact = capture();
        exact.stdout = stream(
            reviewed_session(concat!(
                "JSON export: current\n",
                "Images path not found: ./images/\n",
                "Complete.\n"
            ))
            .as_bytes(),
        );
        let evidence =
            assess_request_with_policy(request.clone(), exact, command.transcript_policy());
        assert_eq!(evidence.assessment().failures().len(), 1);
        assert_eq!(
            evidence.assessment().failures()[0].code,
            ProcessFailureCode::BlockingDiagnostic
        );

        for body in [
            "JSON export: current\nImages path not found: /images\nComplete.\n",
            "JSON export: current\nImages path not found: ./images\nComplete.\n",
            "JSON export: current\nImages path not found: ./images/\nWarning: extra\nComplete.\n",
            "JSON export: current\nImages path not found: ./images/\nComplete.\nExtra\n",
        ] {
            let mut value = capture();
            value.stdout = stream(reviewed_session(body).as_bytes());
            assert_has_failure(
                assess_request_with_policy(request.clone(), value, command.transcript_policy()),
                ProcessFailureCode::TranscriptContractMismatch,
            );
        }
    }

    #[test]
    fn project_info_requires_one_final_completion_marker() {
        let command =
            crate::SpineCommand::project_info("/staged/current.spine").expect("project info");
        let request = command
            .process_request("/evidence/editor", "/evidence/work", BTreeMap::new())
            .expect("typed request");
        let mut value = capture();
        value.stdout = stream(
            reviewed_session(concat!(
                "Project info: /staged/current.spine\n",
                "  Spine version: 4.3.23\n",
                "  Dopesheet FPS: 30\n",
                "  Skeleton: Character\n",
                "Complete.\n",
                "    Animations (1): idle\n",
                "Complete.\n"
            ))
            .as_bytes(),
        );
        assert_has_failure(
            assess_request_with_policy(request, value, command.transcript_policy()),
            ProcessFailureCode::TranscriptContractMismatch,
        );
    }

    #[test]
    fn project_info_extraction_binds_counts_grouping_and_exact_target() {
        let command =
            crate::SpineCommand::project_info("/staged/current.spine").expect("project info");
        let request = command
            .process_request("/evidence/editor", "/evidence/work", BTreeMap::new())
            .expect("typed request");
        let body = concat!(
            "Project info: /staged/current.spine\n",
            "  Spine version: 4.3.23\n",
            "  Dopesheet FPS: 30\n",
            "  Skeleton: Character\n",
            "    Size: <unknown>\n",
            "    Bones (4): root, upper, lower, target\n",
            "    Slots (1): body-slot\n",
            "    Events (2): land, step\n",
            "    IK constraints (2): aim-paw, aim-upper\n",
            "    Animations (3): interrupt, source, target\n",
            "Complete.\n"
        );
        let mut value = capture();
        value.stdout = stream(reviewed_session(body).as_bytes());
        let evidence =
            assess_request_with_policy(request.clone(), value, command.transcript_policy());
        assert!(evidence.assessment().passed());
        let inventory = evidence.project_info_inventory().expect("typed inventory");
        let skeleton = inventory
            .require_exact_skeleton("Character")
            .expect("exact skeleton");
        assert_eq!(inventory.project(), Path::new("/staged/current.spine"));
        assert_eq!(inventory.spine_version(), "4.3.23");
        assert_eq!(inventory.dopesheet_fps(), "30");
        assert_eq!(
            skeleton.sections()[&ProjectInfoSection::Animations].values(),
            ["interrupt", "source", "target"]
        );
        assert!(matches!(
            inventory.require_exact_skeleton("Another"),
            Err(ProjectInfoError::WrongTargetSkeleton { .. })
        ));

        for invalid_body in [
            body.replace("Bones (4)", "Bones (3)"),
            body.replace(
                "    Slots (1): body-slot\n",
                "    Slots (1): body-slot\n    Slots (1): body-slot\n",
            ),
            body.replace(
                "  Skeleton: Character\n",
                "    Bones (1): root\n  Skeleton: Character\n",
            ),
        ] {
            let mut value = capture();
            value.stdout = stream(reviewed_session(&invalid_body).as_bytes());
            assert_has_failure(
                assess_request_with_policy(request.clone(), value, command.transcript_policy()),
                ProcessFailureCode::TranscriptContractMismatch,
            );
        }
    }

    #[test]
    fn serialized_failures_never_disclose_an_unhidden_license_name() {
        let secret = "Sensitive License Owner";
        let mut value = capture();
        value.stdout = stream(format!("Licensed to: {secret}\n").as_bytes());
        let evidence = assess(value);
        assert!(!evidence.assessment().passed());
        let serialized = serde_json::to_string(&evidence).expect("serialize evidence");
        assert!(!serialized.contains(secret));
        assert!(serialized.contains("Licensed to: <redacted>"));
        assert!(
            std::str::from_utf8(evidence.raw_stdout_retained_prefix())
                .expect("raw transcript")
                .contains(secret)
        );
    }

    #[test]
    fn truncated_stream_binds_prefix_and_full_stream_without_lying() {
        let full = b"retained prefix plus complete tail";
        let prefix = &full[..15];
        let mut value = capture();
        value.stdout = ProcessStreamCapture {
            retained_prefix: prefix.to_vec(),
            total_observed_bytes: full.len() as u64,
            bytes_seen_sha256: sha256_bytes(full),
            full_stream_sha256: Some(sha256_bytes(full)),
            retained_prefix_truncated: true,
            complete: true,
        };
        let evidence = assess(value);
        assert_has_failure_ref(&evidence, ProcessFailureCode::OutputLimitExceeded);
        assert_eq!(evidence.raw_stdout_retained_prefix(), prefix);
        assert_eq!(
            evidence.assessment().stdout_retained_prefix_sha256(),
            sha256_bytes(prefix)
        );
        let serialized = serde_json::to_value(&evidence).expect("serialize evidence");
        assert_eq!(
            serialized["stdout"]["bytes_seen_sha256"],
            sha256_bytes(full)
        );
        assert_eq!(
            serialized["stdout"]["full_stream_sha256"],
            sha256_bytes(full)
        );
        assert!(serialized.get("raw_stdout_retained_prefix").is_none());
    }

    #[test]
    fn rejects_incoherent_stream_evidence() {
        let mut value = capture();
        value.stdout.total_observed_bytes = 1;
        value.stdout.bytes_seen_sha256 = sha256_bytes(b"different");
        assert_has_failure(assess(value), ProcessFailureCode::InvalidCaptureEvidence);
    }

    #[test]
    fn rejects_incoherent_termination_and_elapsed_evidence() {
        let mut value = capture();
        value.sent_signal = Some(9);
        assert_has_failure(assess(value), ProcessFailureCode::InvalidCaptureEvidence);

        let mut value = capture();
        value.elapsed = request().timeout + Duration::from_millis(1);
        assert_has_failure(assess(value), ProcessFailureCode::InvalidCaptureEvidence);
    }

    #[test]
    fn environment_values_are_hashed_and_never_serialized() {
        let mut request = request();
        request.environment.insert(
            "PATH".to_owned(),
            "/usr/bin:/bin:/usr/sbin:/sbin".to_owned(),
        );
        let evidence = execute_and_assess(
            &FakeExecutor(capture()),
            &request,
            TranscriptPolicy::spine_4_3_23(),
        )
        .expect("fixed launcher PATH is accepted");
        let serialized = serde_json::to_string(&evidence).expect("serialize evidence");
        assert!(!serialized.contains("\"C\""));
        assert!(!serialized.contains("/usr/bin:/bin:/usr/sbin:/sbin"));
        assert!(serialized.contains(&sha256_bytes(b"C")));
        assert!(serialized.contains(&sha256_bytes(b"/usr/bin:/bin:/usr/sbin:/sbin")));
        assert_eq!(evidence.environment()[0].name(), "LANG");
        assert_eq!(evidence.environment()[1].name(), "PATH");
    }

    #[test]
    fn rejects_non_allowlisted_environment_before_executor_runs() {
        let mut request = request();
        request
            .environment
            .insert("SECRET_TOKEN".to_owned(), "secret".to_owned());
        let error = execute_and_assess(
            &FakeExecutor(capture()),
            &request,
            TranscriptPolicy::spine_4_3_23(),
        )
        .expect_err("non-allowlisted environment");
        assert_eq!(error.code(), ProcessExecutionErrorCode::InvalidRequest);
    }

    #[test]
    fn rejects_resource_limits_that_are_not_meaningfully_bounded() {
        for request in [
            ProcessRequest {
                timeout: Duration::from_secs(30 * 60 + 1),
                ..request()
            },
            ProcessRequest {
                cleanup_timeout: Duration::from_secs(31),
                ..request()
            },
            ProcessRequest {
                max_retained_bytes_per_stream: 4 * 1024 * 1024 + 1,
                ..request()
            },
        ] {
            let error = execute_and_assess(
                &FakeExecutor(capture()),
                &request,
                TranscriptPolicy::spine_4_3_23(),
            )
            .expect_err("unbounded resource request");
            assert_eq!(error.code(), ProcessExecutionErrorCode::InvalidRequest);
        }
    }

    #[test]
    fn serialized_evidence_binds_request_capture_and_assessment() {
        let evidence = assess(capture());
        let value = serde_json::to_value(evidence).expect("serialize process evidence");
        assert_eq!(value["operation"], "generic-export");
        assert_eq!(value["requested_program"], "/evidence/editor");
        assert_eq!(value["args"][0], "--fixed-version");
        assert_eq!(value["requested_working_directory"], "/evidence/work");
        assert_eq!(value["timeout_seconds"], 30);
        assert_eq!(value["cleanup_timeout_seconds"], 2);
        assert_eq!(value["max_retained_bytes_per_stream"], 1024);
        assert_eq!(value["termination_reason"], "natural_exit");
        assert_eq!(value["cleanup_status"], "complete");
        assert_eq!(value["output_discovery_state"], "complete");
        assert_eq!(value["transcript_profile"], "operation");
        assert_eq!(value["lock_evidence"]["acquired"], true);
        assert_eq!(value["assessment"]["passed"], true);
    }

    fn assert_has_failure(evidence: ProcessEvidence, code: ProcessFailureCode) {
        assert_has_failure_ref(&evidence, code);
    }

    fn assert_has_failure_ref(evidence: &ProcessEvidence, code: ProcessFailureCode) {
        let assessment = evidence.assessment();
        assert!(!assessment.passed());
        assert!(
            assessment
                .failures()
                .iter()
                .any(|failure| failure.code == code)
        );
    }
}

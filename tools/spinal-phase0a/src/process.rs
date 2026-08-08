use crate::digest::{is_sha256, sha256_bytes};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

const ALLOWED_ENVIRONMENT_NAMES: &[&str] = &["HOME", "LANG", "LC_ALL", "TMPDIR"];
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

// Populate only from captured, reviewed Spine 4.3.23 evidence. Case files may
// not add to or weaken this list.
const SPINE_4_3_23_INFORMATIONAL_LINES: &[&str] = &[];

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

/// Checked-in transcript rules for a specific editor version.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TranscriptPolicy;

impl TranscriptPolicy {
    /// Returns the deny-first policy pinned to Spine 4.3.23.
    pub const fn spine_4_3_23() -> Self {
        Self
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
    /// A required output was absent after execution.
    MissingOutput,
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
    let environment = request
        .environment
        .iter()
        .map(|(name, value)| EnvironmentVariableEvidence::from_pair(name, value))
        .collect();
    let capture = executor.execute(request)?;
    let assessment = assess_capture(request, &capture, policy);
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

    /// Returns the exact retained stdout prefix bytes.
    pub fn raw_stdout_retained_prefix(&self) -> &[u8] {
        &self.raw_stdout_retained_prefix
    }

    /// Returns the exact retained stderr prefix bytes.
    pub fn raw_stderr_retained_prefix(&self) -> &[u8] {
        &self.raw_stderr_retained_prefix
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
        policy,
        &mut failures,
    );
    classify_transcript(
        "stderr",
        &capture.stderr.retained_prefix,
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
    _policy: TranscriptPolicy,
    failures: &mut Vec<ProcessFailure>,
) {
    let Ok(text) = std::str::from_utf8(bytes) else {
        failures.push(failure(
            ProcessFailureCode::NonUtf8Transcript,
            format!("{stream} retained prefix was not UTF-8"),
        ));
        return;
    };
    for (index, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let lowercase = line.to_ascii_lowercase();
        if BLOCKING_TERMS.iter().any(|term| lowercase.contains(term)) {
            failures.push(failure(
                ProcessFailureCode::BlockingDiagnostic,
                format!("{stream} line {}: {line}", index + 1),
            ));
        } else if !SPINE_4_3_23_INFORMATIONAL_LINES.contains(&line) {
            failures.push(failure(
                ProcessFailureCode::UnknownTranscriptLine,
                format!("{stream} line {}: {line}", index + 1),
            ));
        }
    }
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

    #[test]
    fn accepts_only_a_complete_quiet_locked_success() {
        assert!(assess(capture()).assessment().passed());
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
        let evidence = assess(capture());
        let serialized = serde_json::to_string(&evidence).expect("serialize evidence");
        assert!(!serialized.contains("\"C\""));
        assert!(serialized.contains(&sha256_bytes(b"C")));
        assert_eq!(evidence.environment()[0].name(), "LANG");
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

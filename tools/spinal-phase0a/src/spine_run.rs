//! Exact-path execution and output discovery for typed Spine commands.

use crate::digest::hex_digest;
use crate::process::{
    OutputDiscoveryState, ProcessEvidence, ProcessExecutionError, ProcessExecutor,
    evidence_from_capture, validate_request,
};
use crate::spine_cli::{
    ExpectedInput, ExpectedOutput, OutputMode, SpineCommand, SpineCommandError, SpineOperationKind,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Stable content evidence for one regular output file.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OutputFileObservation {
    size: u64,
    sha256: String,
    identity: FileIdentityObservation,
}

/// Stable identity for one opened regular file.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileIdentityObservation {
    device: u64,
    inode: u64,
    mode: u32,
    owner: u32,
    strong_identity_available: bool,
}

impl FileIdentityObservation {
    /// Returns the filesystem device number.
    pub fn device(&self) -> u64 {
        self.device
    }

    /// Returns the filesystem inode number.
    pub fn inode(&self) -> u64 {
        self.inode
    }

    fn aliases(&self, other: &Self) -> bool {
        self.strong_identity_available
            && other.strong_identity_available
            && self.device == other.device
            && self.inode == other.inode
    }
}

impl OutputFileObservation {
    /// Returns the number of bytes read while calculating the digest.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Returns the lowercase SHA-256 of the complete file bytes.
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// Returns the opened file identity bound to this digest.
    pub fn identity(&self) -> &FileIdentityObservation {
        &self.identity
    }
}

/// Before-and-after evidence for one exact typed command output.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpineOutputObservation {
    id: String,
    path: PathBuf,
    mode: OutputMode,
    before: Option<OutputFileObservation>,
    after: Option<OutputFileObservation>,
}

/// Before-and-after evidence for one immutable command input.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpineInputObservation {
    id: String,
    path: PathBuf,
    expected_sha256: Option<String>,
    before: OutputFileObservation,
    after: OutputFileObservation,
}

impl SpineInputObservation {
    /// Returns the stable typed input identifier.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the exact immutable file path passed to Spine.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns a digest fixed by policy, when this input has one.
    pub fn expected_sha256(&self) -> Option<&str> {
        self.expected_sha256.as_deref()
    }

    /// Returns the content observation immediately before execution.
    pub fn before(&self) -> &OutputFileObservation {
        &self.before
    }

    /// Returns the content observation immediately after execution.
    pub fn after(&self) -> &OutputFileObservation {
        &self.after
    }
}

impl SpineOutputObservation {
    /// Returns the stable output identifier from the typed command.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the exact absolute path from the typed command.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the required before-and-after filesystem mode.
    pub fn mode(&self) -> OutputMode {
        self.mode
    }

    /// Returns content evidence captured before execution, when required.
    pub fn before(&self) -> Option<&OutputFileObservation> {
        self.before.as_ref()
    }

    /// Returns content evidence captured after execution, when present.
    ///
    /// A missing value remains explicit and causes process assessment to report
    /// the command's required output as absent.
    pub fn after(&self) -> Option<&OutputFileObservation> {
        self.after.as_ref()
    }
}

/// Process evidence paired with exact-path output observations.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpineRunEvidence {
    operation_kind: SpineOperationKind,
    process: ProcessEvidence,
    inputs: Vec<SpineInputObservation>,
    outputs: Vec<SpineOutputObservation>,
}

/// Internal failed-attempt result that preserves an assessed process capture
/// whenever a child was launched successfully enough to produce one.
///
/// The public typed-command API remains fail-closed and returns only the
/// underlying error. The closed Phase 0A workspace uses this richer value so
/// a later output or immutable-input check cannot erase bounded diagnostics.
pub(crate) struct SpineRunAttemptError {
    error: SpineRunError,
    process: Option<Box<ProcessEvidence>>,
}

impl SpineRunAttemptError {
    pub(crate) fn process(&self) -> Option<&ProcessEvidence> {
        self.process.as_deref()
    }

    pub(crate) fn into_error(self) -> SpineRunError {
        self.error
    }

    fn before_launch(error: SpineRunError) -> Self {
        Self {
            error,
            process: None,
        }
    }

    fn after_launch(error: SpineRunError, process: Option<ProcessEvidence>) -> Self {
        Self {
            error,
            process: process.map(Box::new),
        }
    }
}

impl SpineRunEvidence {
    /// Returns the closed typed operation bound to this evidence.
    pub fn operation_kind(&self) -> SpineOperationKind {
        self.operation_kind
    }

    /// Returns the assessed process evidence.
    pub fn process(&self) -> &ProcessEvidence {
        &self.process
    }

    pub(crate) fn into_process(self) -> ProcessEvidence {
        self.process
    }

    /// Returns every immutable input observed before and after launch.
    pub fn inputs(&self) -> &[SpineInputObservation] {
        &self.inputs
    }

    /// Returns output observations in typed-command order.
    pub fn outputs(&self) -> &[SpineOutputObservation] {
        &self.outputs
    }
}

/// Failures that prevent safe typed-command execution or output discovery.
#[derive(Debug, Error)]
pub enum SpineRunError {
    /// The program or working-directory path could not form a process request.
    #[error(transparent)]
    Command(#[from] SpineCommandError),
    /// The injected process boundary could not produce trustworthy capture.
    #[error(transparent)]
    Process(#[from] ProcessExecutionError),
    /// An immutable input was absent before command execution.
    #[error("required immutable input `{id}` does not exist at `{path}`")]
    InputMissing {
        /// Stable typed input identifier.
        id: String,
        /// Exact missing input path.
        path: PathBuf,
    },
    /// A policy-bound input did not match its approved bytes.
    #[error("immutable input `{id}` did not match its approved digest at `{path}")]
    InputDigestMismatch {
        /// Stable typed input identifier.
        id: String,
        /// Exact mismatching input path.
        path: PathBuf,
    },
    /// An immutable input changed across the editor operation.
    #[error("immutable input `{id}` changed during execution at `{path}")]
    InputChanged {
        /// Stable typed input identifier.
        id: String,
        /// Exact unstable input path.
        path: PathBuf,
    },
    /// Two supposedly distinct typed file roles physically aliased one file.
    #[error(
        "typed files `{first_id}` at `{first_path}` and `{second_id}` at `{second_path}` physically alias"
    )]
    PhysicalAlias {
        /// First typed role.
        first_id: String,
        /// First absolute path.
        first_path: PathBuf,
        /// Second typed role.
        second_id: String,
        /// Second absolute path.
        second_path: PathBuf,
    },
    /// A create-only output already existed as a regular file before execution.
    #[error("created output `{id}` already exists at `{path}`")]
    CreatedOutputAlreadyExists {
        /// Stable typed output identifier.
        id: String,
        /// Exact conflicting output path.
        path: PathBuf,
    },
    /// An update-only output was absent before execution.
    #[error("updated output `{id}` does not exist at `{path}`")]
    UpdatedOutputMissing {
        /// Stable typed output identifier.
        id: String,
        /// Exact missing output path.
        path: PathBuf,
    },
    /// A typed output path resolved to a symbolic link.
    #[error("output `{id}` must not be a symbolic link: `{path}`")]
    Symlink {
        /// Stable typed output identifier.
        id: String,
        /// Exact rejected output path.
        path: PathBuf,
    },
    /// A typed output path contained a directory, socket, device, or pipe.
    #[error("output `{id}` is not a regular file: `{path}`")]
    UnsupportedFileType {
        /// Stable typed output identifier.
        id: String,
        /// Exact rejected output path.
        path: PathBuf,
    },
    /// File identity or metadata changed while content evidence was read.
    #[error("output `{id}` changed while it was being read: `{path}`")]
    ChangedDuringRead {
        /// Stable typed output identifier.
        id: String,
        /// Exact unstable output path.
        path: PathBuf,
    },
    /// A filesystem operation failed.
    #[error("could not {operation} output `{id}` at `{path}`: {source}")]
    Io {
        /// Short filesystem operation description.
        operation: &'static str,
        /// Stable typed output identifier.
        id: String,
        /// Exact affected output path.
        path: PathBuf,
        /// Underlying filesystem failure.
        #[source]
        source: io::Error,
    },
}

/// Executes one typed command and binds its process evidence to exact outputs.
///
/// The executor cannot claim output identifiers: its set is cleared and rebuilt
/// only from regular files discovered at the command's exact expected paths.
/// Missing post-execution files remain explicit observations and fail process
/// assessment. Filesystem type or change-during-read violations fail closed.
pub fn execute_spine_command<E: ProcessExecutor + ?Sized>(
    executor: &E,
    command: &SpineCommand,
    program: impl AsRef<Path>,
    working_directory: impl AsRef<Path>,
    environment: BTreeMap<String, String>,
) -> Result<SpineRunEvidence, SpineRunError> {
    execute_spine_command_attempt(executor, command, program, working_directory, environment)
        .map_err(SpineRunAttemptError::into_error)
}

pub(crate) fn execute_spine_command_attempt<E: ProcessExecutor + ?Sized>(
    executor: &E,
    command: &SpineCommand,
    program: impl AsRef<Path>,
    working_directory: impl AsRef<Path>,
    environment: BTreeMap<String, String>,
) -> Result<SpineRunEvidence, SpineRunAttemptError> {
    let request = command
        .process_request(program, working_directory, environment)
        .map_err(|error| SpineRunAttemptError::before_launch(error.into()))?;
    validate_request(&request)
        .map_err(|error| SpineRunAttemptError::before_launch(error.into()))?;
    let prepared_inputs =
        prepare_inputs(command.expected_inputs()).map_err(SpineRunAttemptError::before_launch)?;
    let prepared =
        prepare_outputs(command.expected_outputs()).map_err(SpineRunAttemptError::before_launch)?;
    reject_physical_aliases(&prepared_inputs, &prepared)
        .map_err(SpineRunAttemptError::before_launch)?;
    let mut capture = executor
        .execute(&request)
        .map_err(|error| SpineRunAttemptError::before_launch(error.into()))?;
    capture.observed_outputs.clear();
    capture.output_discovery_state = OutputDiscoveryState::NotPerformed;
    let (outputs, observed) = match discover_outputs(prepared) {
        Ok(value) => value,
        Err(error) => {
            let process =
                evidence_from_capture(&request, capture, command.transcript_policy()).ok();
            return Err(SpineRunAttemptError::after_launch(error, process));
        }
    };
    capture.observed_outputs = observed;
    capture.output_discovery_state = OutputDiscoveryState::Complete;
    let inputs = match verify_inputs(prepared_inputs) {
        Ok(value) => value,
        Err(error) => {
            let process =
                evidence_from_capture(&request, capture, command.transcript_policy()).ok();
            return Err(SpineRunAttemptError::after_launch(error, process));
        }
    };
    let process = evidence_from_capture(&request, capture, command.transcript_policy())
        .map_err(|error| SpineRunAttemptError::after_launch(error.into(), None))?;
    Ok(SpineRunEvidence {
        operation_kind: command.kind(),
        process,
        inputs,
        outputs,
    })
}

struct PreparedInput {
    id: String,
    path: PathBuf,
    expected_sha256: Option<String>,
    before: OutputFileObservation,
}

fn prepare_inputs(expected: &[ExpectedInput]) -> Result<Vec<PreparedInput>, SpineRunError> {
    expected
        .iter()
        .map(|input| {
            let id = input.id().to_owned();
            let path = input.path().to_path_buf();
            let Some(metadata) = optional_metadata(&id, &path)? else {
                return Err(SpineRunError::InputMissing { id, path });
            };
            reject_file_type(&id, &path, &metadata)?;
            let before = observe_regular_file(&id, &path, metadata)?;
            if input
                .expected_sha256()
                .is_some_and(|expected| before.sha256() != expected)
            {
                return Err(SpineRunError::InputDigestMismatch { id, path });
            }
            Ok(PreparedInput {
                id,
                path,
                expected_sha256: input.expected_sha256().map(str::to_owned),
                before,
            })
        })
        .collect()
}

fn verify_inputs(
    prepared: Vec<PreparedInput>,
) -> Result<Vec<SpineInputObservation>, SpineRunError> {
    prepared
        .into_iter()
        .map(|input| {
            let Some(metadata) = optional_metadata(&input.id, &input.path)? else {
                return Err(SpineRunError::InputChanged {
                    id: input.id,
                    path: input.path,
                });
            };
            reject_file_type(&input.id, &input.path, &metadata)?;
            let after = observe_regular_file(&input.id, &input.path, metadata)?;
            if input
                .expected_sha256
                .as_deref()
                .is_some_and(|expected| after.sha256() != expected)
                || after != input.before
            {
                return Err(SpineRunError::InputChanged {
                    id: input.id,
                    path: input.path,
                });
            }
            Ok(SpineInputObservation {
                id: input.id,
                path: input.path,
                expected_sha256: input.expected_sha256,
                before: input.before,
                after,
            })
        })
        .collect()
}

fn reject_physical_aliases(
    inputs: &[PreparedInput],
    outputs: &[PreparedOutput],
) -> Result<(), SpineRunError> {
    for (index, first) in inputs.iter().enumerate() {
        for second in &inputs[index + 1..] {
            if first.before.identity().aliases(second.before.identity()) {
                return physical_alias(first, &second.id, &second.path);
            }
        }
        for output in outputs {
            if output
                .before
                .as_ref()
                .is_some_and(|before| first.before.identity().aliases(before.identity()))
            {
                return physical_alias(first, &output.id, &output.path);
            }
        }
    }
    Ok(())
}

fn physical_alias<T>(
    first: &PreparedInput,
    second_id: &str,
    second_path: &Path,
) -> Result<T, SpineRunError> {
    Err(SpineRunError::PhysicalAlias {
        first_id: first.id.clone(),
        first_path: first.path.clone(),
        second_id: second_id.to_owned(),
        second_path: second_path.to_path_buf(),
    })
}

struct PreparedOutput {
    id: String,
    path: PathBuf,
    mode: OutputMode,
    before: Option<OutputFileObservation>,
}

fn prepare_outputs(expected: &[ExpectedOutput]) -> Result<Vec<PreparedOutput>, SpineRunError> {
    expected
        .iter()
        .map(|output| {
            let id = output.id().to_owned();
            let path = output.path().to_path_buf();
            let metadata = optional_metadata(&id, &path)?;
            let before = match (output.mode(), metadata) {
                (OutputMode::CreatedFile, None) => None,
                (OutputMode::CreatedFile, Some(metadata)) => {
                    reject_file_type(&id, &path, &metadata)?;
                    return Err(SpineRunError::CreatedOutputAlreadyExists { id, path });
                }
                (OutputMode::UpdatedFile, None) => {
                    return Err(SpineRunError::UpdatedOutputMissing { id, path });
                }
                (OutputMode::UpdatedFile, Some(metadata)) => {
                    reject_file_type(&id, &path, &metadata)?;
                    Some(observe_regular_file(&id, &path, metadata)?)
                }
            };
            Ok(PreparedOutput {
                id,
                path,
                mode: output.mode(),
                before,
            })
        })
        .collect()
}

fn discover_outputs(
    prepared: Vec<PreparedOutput>,
) -> Result<(Vec<SpineOutputObservation>, BTreeSet<String>), SpineRunError> {
    let mut outputs = Vec::with_capacity(prepared.len());
    let mut observed = BTreeSet::new();
    for output in prepared {
        let after = match optional_metadata(&output.id, &output.path)? {
            None => None,
            Some(metadata) => {
                reject_file_type(&output.id, &output.path, &metadata)?;
                let observation = observe_regular_file(&output.id, &output.path, metadata)?;
                observed.insert(output.id.clone());
                Some(observation)
            }
        };
        outputs.push(SpineOutputObservation {
            id: output.id,
            path: output.path,
            mode: output.mode,
            before: output.before,
            after,
        });
    }
    Ok((outputs, observed))
}

fn optional_metadata(id: &str, path: &Path) -> Result<Option<Metadata>, SpineRunError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(SpineRunError::Io {
            operation: "read metadata for",
            id: id.to_owned(),
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn reject_file_type(id: &str, path: &Path, metadata: &Metadata) -> Result<(), SpineRunError> {
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return Err(SpineRunError::Symlink {
            id: id.to_owned(),
            path: path.to_path_buf(),
        });
    }
    if !file_type.is_file() {
        return Err(SpineRunError::UnsupportedFileType {
            id: id.to_owned(),
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

fn observe_regular_file(
    id: &str,
    path: &Path,
    path_before: Metadata,
) -> Result<OutputFileObservation, SpineRunError> {
    let mut file = open_without_following(path).map_err(|source| SpineRunError::Io {
        operation: "open",
        id: id.to_owned(),
        path: path.to_path_buf(),
        source,
    })?;
    let opened_before = file.metadata().map_err(|source| SpineRunError::Io {
        operation: "read opened-file metadata for",
        id: id.to_owned(),
        path: path.to_path_buf(),
        source,
    })?;
    if !same_file(&path_before, &opened_before) {
        return changed_during_read(id, path);
    }

    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|source| SpineRunError::Io {
            operation: "read",
            id: id.to_owned(),
            path: path.to_path_buf(),
            source,
        })?;
        if count == 0 {
            break;
        }
        size = size
            .checked_add(u64::try_from(count).expect("read size fits within u64"))
            .ok_or_else(|| SpineRunError::ChangedDuringRead {
                id: id.to_owned(),
                path: path.to_path_buf(),
            })?;
        hasher.update(&buffer[..count]);
    }

    let opened_after = file.metadata().map_err(|source| SpineRunError::Io {
        operation: "reread opened-file metadata for",
        id: id.to_owned(),
        path: path.to_path_buf(),
        source,
    })?;
    let Some(path_after) = optional_metadata(id, path)? else {
        return changed_during_read(id, path);
    };
    reject_file_type(id, path, &path_after)?;
    if size != opened_before.len()
        || !same_file(&opened_before, &opened_after)
        || !same_file(&opened_after, &path_after)
    {
        return changed_during_read(id, path);
    }

    Ok(OutputFileObservation {
        size,
        sha256: hex_digest(hasher.finalize().as_slice()),
        identity: file_identity_observation(&opened_after),
    })
}

#[cfg(unix)]
fn file_identity_observation(metadata: &Metadata) -> FileIdentityObservation {
    use std::os::unix::fs::MetadataExt as _;

    FileIdentityObservation {
        device: metadata.dev(),
        inode: metadata.ino(),
        mode: metadata.mode(),
        owner: metadata.uid(),
        strong_identity_available: true,
    }
}

#[cfg(not(unix))]
fn file_identity_observation(_metadata: &Metadata) -> FileIdentityObservation {
    FileIdentityObservation {
        device: 0,
        inode: 0,
        mode: 0,
        owner: 0,
        strong_identity_available: false,
    }
}

fn changed_during_read<T>(id: &str, path: &Path) -> Result<T, SpineRunError> {
    Err(SpineRunError::ChangedDuringRead {
        id: id.to_owned(),
        path: path.to_path_buf(),
    })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn open_without_following(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt as _;

    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn open_without_following(path: &Path) -> io::Result<File> {
    OpenOptions::new().read(true).open(path)
}

#[cfg(unix)]
fn same_file(left: &Metadata, right: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;

    left.file_type().is_file()
        && right.file_type().is_file()
        && left.dev() == right.dev()
        && left.ino() == right.ino()
        && left.mode() == right.mode()
        && left.len() == right.len()
        && left.mtime() == right.mtime()
        && left.mtime_nsec() == right.mtime_nsec()
        && left.ctime() == right.ctime()
        && left.ctime_nsec() == right.ctime_nsec()
}

#[cfg(not(unix))]
fn same_file(left: &Metadata, right: &Metadata) -> bool {
    left.file_type().is_file()
        && right.file_type().is_file()
        && left.len() == right.len()
        && left.modified().ok() == right.modified().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{
        ProcessCapture, ProcessFailureCode, ProcessStreamCapture, TranscriptProfile, tests::capture,
    };
    use std::cell::Cell;

    struct FakeExecutor<Action> {
        action: Action,
        capture: ProcessCapture,
        calls: Cell<usize>,
    }

    impl<Action> FakeExecutor<Action> {
        fn new(action: Action) -> Self {
            Self {
                action,
                capture: capture(),
                calls: Cell::new(0),
            }
        }
    }

    impl<Action: Fn()> ProcessExecutor for FakeExecutor<Action> {
        fn execute(
            &self,
            request: &crate::ProcessRequest,
        ) -> Result<ProcessCapture, ProcessExecutionError> {
            self.calls.set(self.calls.get() + 1);
            (self.action)();
            let mut capture = self.capture.clone();
            capture.stdout = complete_stream(operation_transcript(request).as_bytes());
            Ok(capture)
        }
    }

    fn complete_stream(bytes: &[u8]) -> ProcessStreamCapture {
        let digest = crate::digest::sha256_bytes(bytes);
        ProcessStreamCapture {
            retained_prefix: bytes.to_vec(),
            total_observed_bytes: bytes.len() as u64,
            bytes_seen_sha256: digest.clone(),
            full_stream_sha256: Some(digest),
            retained_prefix_truncated: false,
            complete: true,
        }
    }

    fn operation_transcript(request: &crate::ProcessRequest) -> String {
        let body = match request.operation.as_str() {
            "spine-export-json" => "JSON export: source\nComplete.\n",
            "spine-import-existing-animation" => concat!(
                "Animation import: submission into candidate (Destination)\n",
                "Imported animation: idle\n",
                "Complete.\n"
            ),
            operation => panic!("unexpected fake operation: {operation}"),
        };
        format!(
            concat!(
                "Spine Launcher 4.3.06 (macOS Apple Silicon)\n",
                "Esoteric Software LLC (C) 2013-2026 | http://esotericsoftware.com\n",
                "Mac OS X aarch64 26.5.2\n",
                "Starting: Spine 4.3.23 Professional\n",
                "Spine 4.3.23 Professional\n",
                "Licensed to: <hidden>\n",
                "{}"
            ),
            body
        )
    }

    fn export_command(output: &Path) -> SpineCommand {
        let root = output.parent().expect("output parent");
        let skeleton_name = output
            .file_stem()
            .and_then(|value| value.to_str())
            .expect("output stem");
        let target = crate::JsonExportTarget::new(root, skeleton_name).expect("export target");
        assert_eq!(target.output_json(), output);
        fs::write(root.join("source.spine"), b"immutable source").expect("write source project");
        fs::write(
            root.join("settings.json"),
            crate::approved_export_preset_bytes(),
        )
        .expect("write approved settings");
        SpineCommand::export_json(
            root.join("source.spine"),
            &target,
            root.join("settings.json"),
        )
        .expect("export command")
    }

    fn updated_command(output: &Path) -> SpineCommand {
        let root = output.parent().expect("output parent");
        fs::write(root.join("submission.spine"), b"immutable submission")
            .expect("write submission project");
        SpineCommand::import_existing_animation(
            root.join("submission.spine"),
            output,
            "Source",
            "Destination",
            "idle",
        )
        .expect("updated command")
    }

    fn run(
        executor: &impl ProcessExecutor,
        command: &SpineCommand,
        working_directory: &Path,
    ) -> Result<SpineRunEvidence, SpineRunError> {
        execute_spine_command(
            executor,
            command,
            working_directory.join("Spine"),
            working_directory,
            BTreeMap::new(),
        )
    }

    fn run_attempt(
        executor: &impl ProcessExecutor,
        command: &SpineCommand,
        working_directory: &Path,
    ) -> Result<SpineRunEvidence, SpineRunAttemptError> {
        execute_spine_command_attempt(
            executor,
            command,
            working_directory.join("Spine"),
            working_directory,
            BTreeMap::new(),
        )
    }

    #[test]
    fn created_output_is_hashed_and_bound_to_process_evidence() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let output = directory.path().join("export.json");
        let written_output = output.clone();
        let executor = FakeExecutor::new(move || {
            fs::write(&written_output, b"created").expect("write created output");
        });

        let evidence = run(&executor, &export_command(&output), directory.path()).expect("run");
        assert!(evidence.process().assessment().passed());
        assert_eq!(evidence.operation_kind(), SpineOperationKind::ExportJson);
        assert_eq!(
            evidence.process().output_discovery_state(),
            OutputDiscoveryState::Complete
        );
        assert_eq!(
            evidence.process().transcript_profile(),
            TranscriptProfile::JsonExport
        );
        assert_eq!(
            evidence.process().observed_outputs(),
            &BTreeSet::from(["export-json".to_owned()])
        );
        assert_eq!(evidence.outputs().len(), 1);
        assert!(evidence.outputs()[0].before().is_none());
        let after = evidence.outputs()[0].after().expect("created output");
        assert_eq!(after.size(), 7);
        assert_eq!(after.sha256(), crate::digest::sha256_bytes(b"created"));
        let serialized = serde_json::to_value(&evidence).expect("serialize run evidence");
        assert_eq!(serialized["operation_kind"], "export_json");
        assert_eq!(serialized["inputs"][0]["id"], "project");
        assert!(serialized["inputs"][0]["expected_sha256"].is_null());
        assert_eq!(
            serialized["inputs"][1]["expected_sha256"],
            crate::digest::sha256_bytes(crate::approved_export_preset_bytes())
        );
        assert_eq!(serialized["process"]["transcript_profile"], "json_export");
        assert_eq!(serialized["process"]["output_discovery_state"], "complete");
        assert_eq!(serialized["outputs"][0]["mode"], "created_file");
    }

    #[test]
    fn unapproved_or_changed_export_preset_is_rejected() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let output = directory.path().join("export.json");
        let command = export_command(&output);
        fs::write(directory.path().join("settings.json"), b"{}").expect("replace settings");
        let executor = FakeExecutor::new(|| {});
        assert!(matches!(
            run(&executor, &command, directory.path()),
            Err(SpineRunError::InputDigestMismatch { .. })
        ));
        assert_eq!(executor.calls.get(), 0);

        fs::write(
            directory.path().join("settings.json"),
            crate::approved_export_preset_bytes(),
        )
        .expect("restore settings");
        let changed_settings = directory.path().join("settings.json");
        let written_output = output.clone();
        let executor = FakeExecutor::new(move || {
            fs::write(&changed_settings, b"{}").expect("mutate settings");
            fs::write(&written_output, b"created").expect("write output");
        });
        assert!(matches!(
            run(&executor, &command, directory.path()),
            Err(SpineRunError::InputChanged { .. })
        ));
    }

    #[test]
    fn every_immutable_input_must_exist_and_remain_unchanged() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let output = directory.path().join("export.json");
        let command = export_command(&output);
        fs::remove_file(directory.path().join("source.spine")).expect("remove source");
        let executor = FakeExecutor::new(|| {});
        assert!(matches!(
            run(&executor, &command, directory.path()),
            Err(SpineRunError::InputMissing { .. })
        ));
        assert_eq!(executor.calls.get(), 0);

        fs::write(directory.path().join("source.spine"), b"immutable source")
            .expect("restore source");
        let source = directory.path().join("source.spine");
        let written_output = output.clone();
        let executor = FakeExecutor::new(move || {
            fs::write(&source, b"mutated source").expect("mutate source");
            fs::write(&written_output, b"created").expect("write output");
        });
        assert!(matches!(
            run(&executor, &command, directory.path()),
            Err(SpineRunError::InputChanged { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn physically_aliased_source_and_destination_are_rejected_before_execution() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let output = directory.path().join("candidate.spine");
        let command = updated_command(&output);
        fs::hard_link(directory.path().join("submission.spine"), &output)
            .expect("hard-link destination");
        let executor = FakeExecutor::new(|| {});
        assert!(matches!(
            run(&executor, &command, directory.path()),
            Err(SpineRunError::PhysicalAlias { .. })
        ));
        assert_eq!(executor.calls.get(), 0);
    }

    #[test]
    fn missing_output_is_not_claimed_and_fails_assessment() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let output = directory.path().join("export.json");
        let executor = FakeExecutor::new(|| {});

        let evidence = run(&executor, &export_command(&output), directory.path()).expect("run");
        assert!(!evidence.process().assessment().passed());
        assert!(evidence.outputs()[0].after().is_none());
        assert!(evidence.process().observed_outputs().is_empty());
        assert!(
            evidence
                .process()
                .assessment()
                .failures()
                .iter()
                .any(|failure| { failure.code == ProcessFailureCode::MissingOutput })
        );
    }

    #[test]
    fn preexisting_created_output_is_rejected_without_execution() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let output = directory.path().join("export.json");
        fs::write(&output, b"stale").expect("write stale output");
        let executor = FakeExecutor::new(|| {});

        assert!(matches!(
            run(&executor, &export_command(&output), directory.path()),
            Err(SpineRunError::CreatedOutputAlreadyExists { .. })
        ));
        assert_eq!(executor.calls.get(), 0);
    }

    #[test]
    fn updated_output_records_changed_and_idempotent_bytes() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let output = directory.path().join("candidate.spine");
        fs::write(&output, b"before").expect("write initial candidate");
        let changed_output = output.clone();
        let changed_executor = FakeExecutor::new(move || {
            fs::write(&changed_output, b"after").expect("update candidate");
        });

        let changed = run(
            &changed_executor,
            &updated_command(&output),
            directory.path(),
        )
        .expect("changed update");
        let observation = &changed.outputs()[0];
        assert_eq!(observation.mode(), OutputMode::UpdatedFile);
        assert_ne!(
            observation.before().expect("before").sha256(),
            observation.after().expect("after").sha256()
        );

        let idempotent_output = output.clone();
        let idempotent_executor = FakeExecutor::new(move || {
            fs::write(&idempotent_output, b"after").expect("repeat candidate update");
        });
        let idempotent = run(
            &idempotent_executor,
            &updated_command(&output),
            directory.path(),
        )
        .expect("idempotent update");
        let observation = &idempotent.outputs()[0];
        assert_eq!(
            observation.before().expect("before").sha256(),
            observation.after().expect("after").sha256()
        );
        assert!(idempotent.process().assessment().passed());
    }

    #[test]
    fn updated_output_must_exist_before_execution() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let output = directory.path().join("candidate.spine");
        let executor = FakeExecutor::new(|| {});

        assert!(matches!(
            run(&executor, &updated_command(&output), directory.path()),
            Err(SpineRunError::UpdatedOutputMissing { .. })
        ));
        assert_eq!(executor.calls.get(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_link_output_is_rejected() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().expect("temporary directory");
        let output = directory.path().join("export.json");
        let target = directory.path().join("target.json");
        fs::write(&target, b"target").expect("write target");
        let linked_output = output.clone();
        let linked_target = target.clone();
        let executor = FakeExecutor::new(move || {
            symlink(&linked_target, &linked_output).expect("create output symlink");
        });

        assert!(matches!(
            run(&executor, &export_command(&output), directory.path()),
            Err(SpineRunError::Symlink { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn post_launch_output_discovery_failure_retains_assessed_process() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().expect("temporary directory");
        let output = directory.path().join("export.json");
        let target = directory.path().join("target.json");
        fs::write(&target, b"target").expect("write target");
        let linked_output = output.clone();
        let linked_target = target.clone();
        let executor = FakeExecutor::new(move || {
            symlink(&linked_target, &linked_output).expect("create output symlink");
        });

        let error = run_attempt(&executor, &export_command(&output), directory.path())
            .expect_err("unsafe post-launch output");
        assert!(matches!(error.error, SpineRunError::Symlink { .. }));
        let process = error.process().expect("retained process evidence");
        assert!(!process.assessment().passed());
        assert_eq!(
            process.output_discovery_state(),
            OutputDiscoveryState::NotPerformed
        );
        assert!(
            process
                .assessment()
                .failures()
                .iter()
                .any(|failure| failure.code == ProcessFailureCode::OutputDiscoveryNotPerformed)
        );
    }

    #[cfg(unix)]
    #[test]
    fn special_file_output_is_rejected() {
        use std::os::unix::net::UnixListener;

        let directory = tempfile::tempdir().expect("temporary directory");
        let output = directory.path().join("export.json");
        let _listener = match UnixListener::bind(&output) {
            Ok(listener) => listener,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("create output socket: {error}"),
        };
        let executor = FakeExecutor::new(|| {});

        assert!(matches!(
            run(&executor, &export_command(&output), directory.path()),
            Err(SpineRunError::UnsupportedFileType { .. })
        ));
        assert_eq!(executor.calls.get(), 0);
    }

    #[test]
    fn nonzero_exit_is_preserved_as_failed_process_evidence() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let output = directory.path().join("export.json");
        let written_output = output.clone();
        let mut executor = FakeExecutor::new(move || {
            fs::write(&written_output, b"partial").expect("write output");
        });
        executor.capture.exit_code = Some(7);

        let evidence = run(&executor, &export_command(&output), directory.path()).expect("run");
        assert!(!evidence.process().assessment().passed());
        assert!(
            evidence
                .process()
                .assessment()
                .failures()
                .iter()
                .any(|failure| { failure.code == ProcessFailureCode::NonzeroExit })
        );
        assert!(evidence.outputs()[0].after().is_some());
    }
}

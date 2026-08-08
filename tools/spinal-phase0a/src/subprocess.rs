//! Bounded shell-free subprocess execution for the Phase 0A editor boundary.

use crate::process::{
    AdapterFailure, AdapterFailureCode, CleanupStatus, ExecutableIdentity, ProcessCapture,
    ProcessExecutionError, ProcessExecutionErrorCode, ProcessExecutor, ProcessRequest,
    ProcessStreamCapture, TerminationReason, WorkingDirectoryIdentity, validate_request,
};

/// Shell-free subprocess executor with bounded capture and cleanup deadlines.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SubprocessExecutor;

impl ProcessExecutor for SubprocessExecutor {
    fn execute(&self, request: &ProcessRequest) -> Result<ProcessCapture, ProcessExecutionError> {
        execute(request)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn execute(_request: &ProcessRequest) -> Result<ProcessCapture, ProcessExecutionError> {
    Err(ProcessExecutionError::with_code(
        ProcessExecutionErrorCode::UnsupportedPlatform,
        "bounded subprocess execution is supported only on macOS and Linux",
    ))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix {
    use super::*;
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
    use rustix::io::Errno;
    use rustix::process::{Pid, Signal, kill_process_group};
    use sha2::{Digest, Sha256};
    use std::fs::{File, Metadata};
    use std::io::{self, Read};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    use std::path::Path;
    use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
    use std::sync::{Arc, OnceLock};
    use std::thread;
    use std::time::{Duration, Instant};

    const READ_BUFFER_BYTES: usize = 8 * 1024;
    const DRAIN_QUANTUM_BYTES: usize = 64 * 1024;
    const HEARTBEAT: Duration = Duration::from_millis(10);
    const SIGKILL_NUMBER: i32 = 9;
    const MAX_REAPER_CHILDREN: usize = 32;

    pub(super) fn execute(
        request: &ProcessRequest,
    ) -> Result<ProcessCapture, ProcessExecutionError> {
        validate_request(request)?;
        let started = Instant::now();
        let execution_deadline = started.checked_add(request.timeout).ok_or_else(|| {
            ProcessExecutionError::with_code(
                ProcessExecutionErrorCode::InvalidRequest,
                "process timeout exceeded the monotonic clock range",
            )
        })?;
        let reaper = reaper_handle()?;
        let mut reaper_reservation = Some(reaper.reserve()?);
        let executable = resolve_executable(&request.program, execution_deadline)?;
        let working_directory = resolve_working_directory(&request.working_directory)?;
        if Instant::now() >= execution_deadline {
            return Err(ProcessExecutionError::with_code(
                ProcessExecutionErrorCode::PreflightDeadline,
                "canonical launch-identity preflight exceeded the execution deadline",
            ));
        }

        let mut command = Command::new(executable.canonical_path());
        command
            .args(&request.args)
            .current_dir(working_directory.canonical_path())
            .env_clear()
            .envs(&request.environment)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let mut child = command.spawn().map_err(|error| {
            ProcessExecutionError::with_code(
                ProcessExecutionErrorCode::Spawn,
                format!(
                    "could not start canonical executable `{}`: {error}",
                    executable.canonical_path().display()
                ),
            )
        })?;
        let child_pid = Pid::from_child(&child);

        let mut stdout = child.stdout.take();
        let mut stderr = child.stderr.take();
        let mut stdout_capture = StreamAccumulator::default();
        let mut stderr_capture = StreamAccumulator::default();
        let mut status = None;
        let mut sent_signal = None;
        let mut termination_reason = TerminationReason::NaturalExit;
        let mut adapter_failure = None;
        let mut cleanup_deadline = None;

        if stdout.is_none() || stderr.is_none() {
            begin_capture_failure(
                &mut child,
                child_pid,
                request.cleanup_timeout,
                &mut termination_reason,
                &mut adapter_failure,
                &mut cleanup_deadline,
                &mut sent_signal,
                AdapterFailure::new(
                    AdapterFailureCode::PipeSetup,
                    "spawned child did not expose both configured output pipes",
                ),
            );
            drop(stdout.take());
            drop(stderr.take());
        } else if let Err(error) = set_nonblocking(stdout.as_ref(), stderr.as_ref()) {
            begin_capture_failure(
                &mut child,
                child_pid,
                request.cleanup_timeout,
                &mut termination_reason,
                &mut adapter_failure,
                &mut cleanup_deadline,
                &mut sent_signal,
                error,
            );
            drop(stdout.take());
            drop(stderr.take());
        } else if !identity_still_matches(&executable, &working_directory) {
            begin_capture_failure(
                &mut child,
                child_pid,
                request.cleanup_timeout,
                &mut termination_reason,
                &mut adapter_failure,
                &mut cleanup_deadline,
                &mut sent_signal,
                AdapterFailure::new(
                    AdapterFailureCode::LaunchIdentityChanged,
                    "canonical executable or working directory changed during launch",
                ),
            );
        }

        let cleanup_status = loop {
            if let Err(error) = drain_stream(
                &mut stdout,
                &mut stdout_capture,
                request.max_retained_bytes_per_stream,
            ) {
                drop(stdout.take());
                begin_capture_failure(
                    &mut child,
                    child_pid,
                    request.cleanup_timeout,
                    &mut termination_reason,
                    &mut adapter_failure,
                    &mut cleanup_deadline,
                    &mut sent_signal,
                    error,
                );
            }
            if let Err(error) = drain_stream(
                &mut stderr,
                &mut stderr_capture,
                request.max_retained_bytes_per_stream,
            ) {
                drop(stderr.take());
                begin_capture_failure(
                    &mut child,
                    child_pid,
                    request.cleanup_timeout,
                    &mut termination_reason,
                    &mut adapter_failure,
                    &mut cleanup_deadline,
                    &mut sent_signal,
                    error,
                );
            }

            if status.is_none() {
                match child.try_wait() {
                    Ok(observed) => status = observed,
                    Err(error) => begin_capture_failure(
                        &mut child,
                        child_pid,
                        request.cleanup_timeout,
                        &mut termination_reason,
                        &mut adapter_failure,
                        &mut cleanup_deadline,
                        &mut sent_signal,
                        AdapterFailure::new(
                            AdapterFailureCode::StatusQuery,
                            format!("could not query child status: {error}"),
                        ),
                    ),
                }
            }

            if status.is_some()
                && cleanup_deadline.is_none()
                && (!stdout_capture.eof || !stderr_capture.eof)
            {
                let mut post_exit_failure = None;
                for _ in 0..16 {
                    let before = (
                        stdout_capture.total_observed_bytes,
                        stderr_capture.total_observed_bytes,
                    );
                    if let Err(error) = drain_stream(
                        &mut stdout,
                        &mut stdout_capture,
                        request.max_retained_bytes_per_stream,
                    ) {
                        drop(stdout.take());
                        post_exit_failure = Some(error);
                        break;
                    }
                    if let Err(error) = drain_stream(
                        &mut stderr,
                        &mut stderr_capture,
                        request.max_retained_bytes_per_stream,
                    ) {
                        drop(stderr.take());
                        post_exit_failure = Some(error);
                        break;
                    }
                    if stdout_capture.eof && stderr_capture.eof {
                        break;
                    }
                    let after = (
                        stdout_capture.total_observed_bytes,
                        stderr_capture.total_observed_bytes,
                    );
                    if after == before || Instant::now() >= execution_deadline {
                        break;
                    }
                }
                if let Some(error) = post_exit_failure {
                    begin_capture_failure(
                        &mut child,
                        child_pid,
                        request.cleanup_timeout,
                        &mut termination_reason,
                        &mut adapter_failure,
                        &mut cleanup_deadline,
                        &mut sent_signal,
                        error,
                    );
                } else if !stdout_capture.eof || !stderr_capture.eof {
                    begin_capture_failure(
                        &mut child,
                        child_pid,
                        request.cleanup_timeout,
                        &mut termination_reason,
                        &mut adapter_failure,
                        &mut cleanup_deadline,
                        &mut sent_signal,
                        AdapterFailure::new(
                            AdapterFailureCode::InheritedPipeAfterExit,
                            "direct child exited while an inherited output writer remained open",
                        ),
                    );
                }
            }

            let now = Instant::now();
            if cleanup_deadline.is_none() && now >= execution_deadline {
                termination_reason = TerminationReason::DeadlineExceeded;
                cleanup_deadline = Some(deadline_after(now, request.cleanup_timeout));
                if let Err(error) = terminate_group(&mut child, child_pid, &mut sent_signal) {
                    adapter_failure.get_or_insert(error);
                }
            }

            if let Some(deadline) = cleanup_deadline
                && now >= deadline
            {
                drop(stdout.take());
                drop(stderr.take());
                adapter_failure.get_or_insert_with(|| {
                    AdapterFailure::new(
                        AdapterFailureCode::CleanupDeadlineExceeded,
                        "forced process cleanup exceeded its separate deadline",
                    )
                });
                if status.is_none() {
                    let handoff = ReaperHandoff {
                        child,
                        _reservation: reaper_reservation
                            .take()
                            .expect("reaper capacity was reserved before spawn"),
                    };
                    match reaper.sender.try_send(handoff) {
                        Ok(()) => break CleanupStatus::ReaperDelegated,
                        Err(
                            TrySendError::Full(mut handoff)
                            | TrySendError::Disconnected(mut handoff),
                        ) => {
                            let _ = handoff.child.kill();
                            let reaped = matches!(handoff.child.try_wait(), Ok(Some(_)));
                            adapter_failure = Some(AdapterFailure::new(
                                AdapterFailureCode::ReaperUnavailable,
                                "bounded child reaper could not accept cleanup handoff",
                            ));
                            break if reaped {
                                CleanupStatus::DeadlineExceeded
                            } else {
                                CleanupStatus::ReaperUnavailable
                            };
                        }
                    }
                }
                break CleanupStatus::DeadlineExceeded;
            }

            if status.is_some() && stdout_capture.eof && stderr_capture.eof {
                break CleanupStatus::Complete;
            }

            let deadline = cleanup_deadline.unwrap_or(execution_deadline);
            let wait = deadline
                .saturating_duration_since(Instant::now())
                .min(HEARTBEAT);
            if let Err(error) = poll_pipes(stdout.as_ref(), stderr.as_ref(), wait) {
                begin_capture_failure(
                    &mut child,
                    child_pid,
                    request.cleanup_timeout,
                    &mut termination_reason,
                    &mut adapter_failure,
                    &mut cleanup_deadline,
                    &mut sent_signal,
                    error,
                );
            }
        };

        let exit_code = status.as_ref().and_then(ExitStatus::code);
        let terminating_signal = status.as_ref().and_then(ExitStatusExt::signal);
        Ok(ProcessCapture {
            exit_code,
            terminating_signal,
            sent_signal,
            termination_reason,
            elapsed: started.elapsed(),
            cleanup_status,
            adapter_failure,
            stdout: stdout_capture.finish(),
            stderr: stderr_capture.finish(),
            observed_outputs: Default::default(),
            output_discovery_state: crate::process::OutputDiscoveryState::NotPerformed,
            executable_identity: executable,
            working_directory_identity: working_directory,
            lock_evidence: None,
        })
    }

    fn deadline_after(now: Instant, timeout: Duration) -> Instant {
        now.checked_add(timeout).unwrap_or(now)
    }

    #[allow(clippy::too_many_arguments)]
    fn begin_capture_failure(
        child: &mut Child,
        child_pid: Pid,
        cleanup_timeout: Duration,
        termination_reason: &mut TerminationReason,
        adapter_failure: &mut Option<AdapterFailure>,
        cleanup_deadline: &mut Option<Instant>,
        sent_signal: &mut Option<i32>,
        failure: AdapterFailure,
    ) {
        if cleanup_deadline.is_none() {
            *termination_reason = TerminationReason::CaptureFailure;
            *cleanup_deadline = Some(deadline_after(Instant::now(), cleanup_timeout));
            *adapter_failure = Some(match terminate_group(child, child_pid, sent_signal) {
                Ok(()) => failure,
                Err(signal_failure) => AdapterFailure::new(
                    failure.code(),
                    format!(
                        "{}; secondary {:?}: {}",
                        failure.detail(),
                        signal_failure.code(),
                        signal_failure.detail()
                    ),
                ),
            });
        }
    }

    fn terminate_group(
        child: &mut Child,
        child_pid: Pid,
        sent_signal: &mut Option<i32>,
    ) -> Result<(), AdapterFailure> {
        match kill_process_group(child_pid, Signal::KILL) {
            Ok(()) => {
                *sent_signal = Some(SIGKILL_NUMBER);
                Ok(())
            }
            Err(group_error) => match child.kill() {
                Ok(()) => {
                    *sent_signal = Some(SIGKILL_NUMBER);
                    Err(AdapterFailure::new(
                        AdapterFailureCode::SignalDelivery,
                        format!(
                            "process-group SIGKILL failed ({group_error}); direct-child fallback was sent"
                        ),
                    ))
                }
                Err(error)
                    if group_error == Errno::SRCH
                        && error.kind() == io::ErrorKind::InvalidInput =>
                {
                    Ok(())
                }
                Err(error) => Err(AdapterFailure::new(
                    AdapterFailureCode::SignalDelivery,
                    format!(
                        "process-group SIGKILL failed ({group_error}); direct-child fallback failed: {error}"
                    ),
                )),
            },
        }
    }

    fn set_nonblocking(
        stdout: Option<&ChildStdout>,
        stderr: Option<&ChildStderr>,
    ) -> Result<(), AdapterFailure> {
        let stdout = stdout.ok_or_else(|| {
            AdapterFailure::new(AdapterFailureCode::PipeSetup, "stdout pipe was absent")
        })?;
        let stderr = stderr.ok_or_else(|| {
            AdapterFailure::new(AdapterFailureCode::PipeSetup, "stderr pipe was absent")
        })?;
        set_pipe_nonblocking(stdout, "stdout")?;
        set_pipe_nonblocking(stderr, "stderr")
    }

    fn set_pipe_nonblocking(
        pipe: &impl std::os::fd::AsFd,
        name: &str,
    ) -> Result<(), AdapterFailure> {
        let flags = fcntl_getfl(pipe).map_err(|error| {
            AdapterFailure::new(
                AdapterFailureCode::PipeSetup,
                format!("could not read {name} pipe flags: {error}"),
            )
        })?;
        fcntl_setfl(pipe, flags | OFlags::NONBLOCK).map_err(|error| {
            AdapterFailure::new(
                AdapterFailureCode::PipeSetup,
                format!("could not make {name} pipe nonblocking: {error}"),
            )
        })
    }

    fn poll_pipes(
        stdout: Option<&ChildStdout>,
        stderr: Option<&ChildStderr>,
        timeout: Duration,
    ) -> Result<(), AdapterFailure> {
        if stdout.is_none() && stderr.is_none() {
            thread::sleep(timeout);
            return Ok(());
        }
        let interest = PollFlags::IN | PollFlags::HUP | PollFlags::ERR;
        let mut descriptors = Vec::with_capacity(2);
        if let Some(pipe) = stdout {
            descriptors.push(PollFd::new(pipe, interest));
        }
        if let Some(pipe) = stderr {
            descriptors.push(PollFd::new(pipe, interest));
        }
        let timeout = Timespec::try_from(timeout).map_err(|error| {
            AdapterFailure::new(
                AdapterFailureCode::PipePoll,
                format!("poll timeout was not representable: {error}"),
            )
        })?;
        match poll(&mut descriptors, Some(&timeout)) {
            Ok(_) => {
                if descriptors
                    .iter()
                    .any(|descriptor| descriptor.revents().contains(PollFlags::NVAL))
                {
                    Err(AdapterFailure::new(
                        AdapterFailureCode::PipePoll,
                        "poll reported an invalid output descriptor",
                    ))
                } else {
                    Ok(())
                }
            }
            Err(Errno::INTR) => Ok(()),
            Err(error) => Err(AdapterFailure::new(
                AdapterFailureCode::PipePoll,
                format!("polling child output failed: {error}"),
            )),
        }
    }

    fn drain_stream<R: Read>(
        pipe: &mut Option<R>,
        capture: &mut StreamAccumulator,
        retained_limit: usize,
    ) -> Result<(), AdapterFailure> {
        if pipe.is_none() || capture.eof {
            return Ok(());
        }
        let mut drained = 0;
        let mut buffer = [0_u8; READ_BUFFER_BYTES];
        while drained < DRAIN_QUANTUM_BYTES {
            let request = buffer.len().min(DRAIN_QUANTUM_BYTES - drained);
            let result = pipe
                .as_mut()
                .expect("pipe presence checked at loop entry")
                .read(&mut buffer[..request]);
            match result {
                Ok(0) => {
                    capture.eof = true;
                    *pipe = None;
                    break;
                }
                Ok(bytes) => {
                    capture.observe(&buffer[..bytes], retained_limit)?;
                    drained += bytes;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => break,
                Err(error) => {
                    return Err(AdapterFailure::new(
                        AdapterFailureCode::PipeRead,
                        format!("reading a child output pipe failed: {error}"),
                    ));
                }
            }
        }
        Ok(())
    }

    #[derive(Default)]
    struct StreamAccumulator {
        retained_prefix: Vec<u8>,
        total_observed_bytes: u64,
        hasher: Sha256,
        eof: bool,
    }

    impl StreamAccumulator {
        fn observe(&mut self, bytes: &[u8], retained_limit: usize) -> Result<(), AdapterFailure> {
            self.total_observed_bytes = self
                .total_observed_bytes
                .checked_add(u64::try_from(bytes.len()).map_err(|error| {
                    AdapterFailure::new(
                        AdapterFailureCode::ByteCountOverflow,
                        format!("read length was not representable: {error}"),
                    )
                })?)
                .ok_or_else(|| {
                    AdapterFailure::new(
                        AdapterFailureCode::ByteCountOverflow,
                        "child output byte count overflowed u64",
                    )
                })?;
            self.hasher.update(bytes);
            let available = retained_limit.saturating_sub(self.retained_prefix.len());
            self.retained_prefix
                .extend_from_slice(&bytes[..bytes.len().min(available)]);
            Ok(())
        }

        fn finish(self) -> ProcessStreamCapture {
            let digest = format!("{:x}", self.hasher.finalize());
            let retained_prefix_truncated = self.total_observed_bytes
                > u64::try_from(self.retained_prefix.len()).unwrap_or(u64::MAX);
            ProcessStreamCapture {
                retained_prefix: self.retained_prefix,
                total_observed_bytes: self.total_observed_bytes,
                bytes_seen_sha256: digest.clone(),
                full_stream_sha256: self.eof.then_some(digest),
                retained_prefix_truncated,
                complete: self.eof,
            }
        }
    }

    fn resolve_executable(
        program: &str,
        deadline: Instant,
    ) -> Result<ExecutableIdentity, ProcessExecutionError> {
        let canonical_path = std::fs::canonicalize(program).map_err(|error| {
            identity_error(
                ProcessExecutionErrorCode::ExecutableIdentity,
                format!("could not canonicalize executable `{program}`: {error}"),
            )
        })?;
        let mut file = File::open(&canonical_path).map_err(|error| {
            identity_error(
                ProcessExecutionErrorCode::ExecutableIdentity,
                format!(
                    "could not open canonical executable `{}`: {error}",
                    canonical_path.display()
                ),
            )
        })?;
        verify_local_filesystem(
            &file,
            ProcessExecutionErrorCode::ExecutableIdentity,
            "canonical executable",
        )?;
        let before = file.metadata().map_err(|error| {
            identity_error(
                ProcessExecutionErrorCode::ExecutableIdentity,
                format!("could not stat canonical executable: {error}"),
            )
        })?;
        validate_executable_metadata(&before)?;
        validate_trusted_executable_parent(&canonical_path)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            if Instant::now() >= deadline {
                return Err(identity_error(
                    ProcessExecutionErrorCode::PreflightDeadline,
                    "hashing the canonical executable exceeded the execution deadline",
                ));
            }
            let bytes = file.read(&mut buffer).map_err(|error| {
                identity_error(
                    ProcessExecutionErrorCode::ExecutableIdentity,
                    format!("could not hash canonical executable: {error}"),
                )
            })?;
            if bytes == 0 {
                break;
            }
            hasher.update(&buffer[..bytes]);
        }
        let after = file.metadata().map_err(|error| {
            identity_error(
                ProcessExecutionErrorCode::ExecutableIdentity,
                format!("could not restat canonical executable: {error}"),
            )
        })?;
        if !same_metadata(&before, &after) {
            return Err(identity_error(
                ProcessExecutionErrorCode::ExecutableIdentity,
                "canonical executable changed while it was hashed",
            ));
        }
        let path_metadata = std::fs::metadata(&canonical_path).map_err(|error| {
            identity_error(
                ProcessExecutionErrorCode::ExecutableIdentity,
                format!("could not verify canonical executable path: {error}"),
            )
        })?;
        if !same_metadata(&before, &path_metadata) {
            return Err(identity_error(
                ProcessExecutionErrorCode::ExecutableIdentity,
                "canonical executable path changed before launch",
            ));
        }
        Ok(ExecutableIdentity::new(
            canonical_path,
            format!("{:x}", hasher.finalize()),
            before.len(),
            before.dev(),
            before.ino(),
            before.mode(),
            before.uid(),
            before.mtime(),
            before.mtime_nsec(),
            before.ctime(),
            before.ctime_nsec(),
        ))
    }

    fn resolve_working_directory(
        path: &Path,
    ) -> Result<WorkingDirectoryIdentity, ProcessExecutionError> {
        let canonical_path = std::fs::canonicalize(path).map_err(|error| {
            identity_error(
                ProcessExecutionErrorCode::WorkingDirectoryIdentity,
                format!(
                    "could not canonicalize working directory `{}`: {error}",
                    path.display()
                ),
            )
        })?;
        let directory = File::open(&canonical_path).map_err(|error| {
            identity_error(
                ProcessExecutionErrorCode::WorkingDirectoryIdentity,
                format!("could not open canonical working directory: {error}"),
            )
        })?;
        verify_local_filesystem(
            &directory,
            ProcessExecutionErrorCode::WorkingDirectoryIdentity,
            "canonical working directory",
        )?;
        let metadata = directory.metadata().map_err(|error| {
            identity_error(
                ProcessExecutionErrorCode::WorkingDirectoryIdentity,
                format!("could not stat opened canonical working directory: {error}"),
            )
        })?;
        if !metadata.is_dir() {
            return Err(identity_error(
                ProcessExecutionErrorCode::WorkingDirectoryIdentity,
                "canonical working directory was not a directory",
            ));
        }
        let effective_uid = rustix::process::geteuid().as_raw();
        if metadata.uid() != effective_uid || metadata.mode() & 0o022 != 0 {
            return Err(identity_error(
                ProcessExecutionErrorCode::WorkingDirectoryIdentity,
                "canonical working directory must be user-owned and not group/world-writable",
            ));
        }
        let path_metadata = std::fs::metadata(&canonical_path).map_err(|error| {
            identity_error(
                ProcessExecutionErrorCode::WorkingDirectoryIdentity,
                format!("could not verify canonical working-directory path: {error}"),
            )
        })?;
        if path_metadata.dev() != metadata.dev()
            || path_metadata.ino() != metadata.ino()
            || path_metadata.mode() != metadata.mode()
            || path_metadata.uid() != metadata.uid()
        {
            return Err(identity_error(
                ProcessExecutionErrorCode::WorkingDirectoryIdentity,
                "canonical working-directory path changed during secure open",
            ));
        }
        Ok(WorkingDirectoryIdentity::new(
            canonical_path,
            metadata.dev(),
            metadata.ino(),
            metadata.mode(),
            metadata.uid(),
        ))
    }

    fn validate_executable_metadata(metadata: &Metadata) -> Result<(), ProcessExecutionError> {
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(identity_error(
                ProcessExecutionErrorCode::ExecutableIdentity,
                "canonical executable was not a regular file",
            ));
        }
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(identity_error(
                ProcessExecutionErrorCode::ExecutableIdentity,
                "canonical executable had no executable permission bit",
            ));
        }
        let mode = metadata.mode();
        let effective_uid = rustix::process::geteuid().as_raw();
        if mode & 0o022 != 0
            || mode & 0o6000 != 0
            || (metadata.uid() != 0 && metadata.uid() != effective_uid)
        {
            return Err(identity_error(
                ProcessExecutionErrorCode::ExecutableIdentity,
                "canonical executable must be trusted-user/root-owned, non-setid, and not group/world-writable",
            ));
        }
        Ok(())
    }

    fn validate_trusted_executable_parent(path: &Path) -> Result<(), ProcessExecutionError> {
        let parent = path.parent().ok_or_else(|| {
            identity_error(
                ProcessExecutionErrorCode::ExecutableIdentity,
                "canonical executable had no parent directory",
            )
        })?;
        let metadata = std::fs::metadata(parent).map_err(|error| {
            identity_error(
                ProcessExecutionErrorCode::ExecutableIdentity,
                format!("could not stat canonical executable parent: {error}"),
            )
        })?;
        let effective_uid = rustix::process::geteuid().as_raw();
        if !metadata.is_dir()
            || metadata.mode() & 0o022 != 0
            || (metadata.uid() != 0 && metadata.uid() != effective_uid)
        {
            return Err(identity_error(
                ProcessExecutionErrorCode::ExecutableIdentity,
                "canonical executable parent was not a trusted non-writable directory",
            ));
        }
        Ok(())
    }

    fn identity_still_matches(
        executable: &ExecutableIdentity,
        working_directory: &WorkingDirectoryIdentity,
    ) -> bool {
        let executable_matches =
            std::fs::metadata(executable.canonical_path()).is_ok_and(|value| {
                executable.same_file(
                    value.dev(),
                    value.ino(),
                    value.len(),
                    value.mode(),
                    value.uid(),
                    value.mtime(),
                    value.mtime_nsec(),
                    value.ctime(),
                    value.ctime_nsec(),
                )
            });
        let directory_matches =
            std::fs::metadata(working_directory.canonical_path()).is_ok_and(|value| {
                value.is_dir()
                    && working_directory.same_file(
                        value.dev(),
                        value.ino(),
                        value.mode(),
                        value.uid(),
                    )
            });
        executable_matches && directory_matches
    }

    fn same_metadata(left: &Metadata, right: &Metadata) -> bool {
        left.dev() == right.dev()
            && left.ino() == right.ino()
            && left.len() == right.len()
            && left.mode() == right.mode()
            && left.uid() == right.uid()
            && left.mtime() == right.mtime()
            && left.mtime_nsec() == right.mtime_nsec()
            && left.ctime() == right.ctime()
            && left.ctime_nsec() == right.ctime_nsec()
    }

    #[cfg(target_os = "macos")]
    fn verify_local_filesystem(
        file: &File,
        code: ProcessExecutionErrorCode,
        description: &str,
    ) -> Result<(), ProcessExecutionError> {
        let status = rustix::fs::fstatfs(file).map_err(|error| {
            identity_error(
                code,
                format!("could not inspect {description} filesystem: {error}"),
            )
        })?;
        if status.f_flags & u32::try_from(libc::MNT_LOCAL).unwrap_or(0) == 0 {
            return Err(identity_error(
                code,
                format!("{description} must be hosted on a verified local filesystem"),
            ));
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn verify_local_filesystem(
        file: &File,
        code: ProcessExecutionErrorCode,
        description: &str,
    ) -> Result<(), ProcessExecutionError> {
        let status = rustix::fs::fstatfs(file).map_err(|error| {
            identity_error(
                code,
                format!("could not inspect {description} filesystem: {error}"),
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
            return Err(identity_error(
                code,
                format!("{description} filesystem is not in the local-filesystem allowlist"),
            ));
        }
        Ok(())
    }

    fn identity_error(
        code: ProcessExecutionErrorCode,
        message: impl Into<String>,
    ) -> ProcessExecutionError {
        ProcessExecutionError::with_code(code, message)
    }

    #[derive(Clone)]
    struct ReaperHandle {
        sender: SyncSender<ReaperHandoff>,
        available: Arc<AtomicUsize>,
    }

    impl ReaperHandle {
        fn reserve(&self) -> Result<ReaperReservation, ProcessExecutionError> {
            self.available
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |available| {
                    available.checked_sub(1)
                })
                .map_err(|_| {
                    ProcessExecutionError::with_code(
                        ProcessExecutionErrorCode::ReaperUnavailable,
                        "bounded cleanup capacity was exhausted before spawn",
                    )
                })?;
            Ok(ReaperReservation {
                available: Arc::clone(&self.available),
            })
        }
    }

    struct ReaperReservation {
        available: Arc<AtomicUsize>,
    }

    impl Drop for ReaperReservation {
        fn drop(&mut self) {
            self.available.fetch_add(1, Ordering::Release);
        }
    }

    struct ReaperHandoff {
        child: Child,
        _reservation: ReaperReservation,
    }

    fn reaper_handle() -> Result<ReaperHandle, ProcessExecutionError> {
        static REAPER: OnceLock<Result<ReaperHandle, String>> = OnceLock::new();
        match REAPER.get_or_init(start_reaper) {
            Ok(handle) => Ok(handle.clone()),
            Err(message) => Err(ProcessExecutionError::with_code(
                ProcessExecutionErrorCode::ReaperUnavailable,
                message.clone(),
            )),
        }
    }

    fn start_reaper() -> Result<ReaperHandle, String> {
        let (sender, receiver) = mpsc::sync_channel(MAX_REAPER_CHILDREN);
        let available = Arc::new(AtomicUsize::new(MAX_REAPER_CHILDREN));
        thread::Builder::new()
            .name("spinal-phase0a-reaper".to_owned())
            .spawn(move || reaper_loop(&receiver))
            .map_err(|error| format!("could not start bounded child reaper: {error}"))?;
        Ok(ReaperHandle { sender, available })
    }

    fn reaper_loop(receiver: &Receiver<ReaperHandoff>) {
        let mut children = Vec::with_capacity(MAX_REAPER_CHILDREN);
        loop {
            if children.len() < MAX_REAPER_CHILDREN {
                match receiver.recv_timeout(HEARTBEAT) {
                    Ok(child) => children.push(child),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) if children.is_empty() => break,
                    Err(mpsc::RecvTimeoutError::Disconnected) => {}
                }
            } else {
                thread::sleep(HEARTBEAT);
            }
            while children.len() < MAX_REAPER_CHILDREN
                && let Ok(child) = receiver.try_recv()
            {
                children.push(child);
            }
            for handoff in &mut children {
                let pid = Pid::from_child(&handoff.child);
                let _ = kill_process_group(pid, Signal::KILL);
                let _ = handoff.child.kill();
            }
            children.retain_mut(|handoff| match handoff.child.try_wait() {
                Ok(Some(_)) => false,
                Ok(None) | Err(_) => true,
            });
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::digest::sha256_bytes;
        use std::collections::{BTreeMap, BTreeSet};

        fn system_program(candidates: &[&str]) -> String {
            candidates
                .iter()
                .find(|path| Path::new(path).is_file())
                .expect("required system test program")
                .to_string()
        }

        fn request(program: String, args: Vec<String>) -> ProcessRequest {
            ProcessRequest {
                operation: "subprocess-test".to_owned(),
                program,
                args,
                working_directory: std::fs::canonicalize(
                    std::env::current_dir().expect("current test directory"),
                )
                .expect("canonical current test directory"),
                environment: BTreeMap::from([("LANG".to_owned(), "C".to_owned())]),
                timeout: Duration::from_secs(2),
                cleanup_timeout: Duration::from_secs(1),
                max_retained_bytes_per_stream: 4 * 1024 * 1024,
                required_outputs: BTreeSet::new(),
            }
        }

        #[test]
        fn captures_a_quiet_direct_success_without_blocking_wait() {
            let value = SubprocessExecutor
                .execute(&request(
                    system_program(&["/usr/bin/true", "/bin/true"]),
                    Vec::new(),
                ))
                .expect("direct process");
            assert_eq!(value.exit_code, Some(0));
            assert_eq!(value.termination_reason, TerminationReason::NaturalExit);
            assert_eq!(value.cleanup_status, CleanupStatus::Complete);
            assert!(value.stdout.complete && value.stderr.complete);
            assert!(value.adapter_failure.is_none());
        }

        #[test]
        fn retains_a_prefix_but_hashes_the_complete_stream() {
            let mut request = request(
                system_program(&["/usr/bin/printf", "/bin/printf"]),
                vec!["abcdefghij".to_owned()],
            );
            request.max_retained_bytes_per_stream = 4;
            let value = SubprocessExecutor
                .execute(&request)
                .expect("direct process");
            assert_eq!(value.stdout.retained_prefix, b"abcd");
            assert_eq!(value.stdout.total_observed_bytes, 10);
            assert!(value.stdout.retained_prefix_truncated);
            assert_eq!(value.stdout.bytes_seen_sha256, sha256_bytes(b"abcdefghij"));
            assert_eq!(
                value.stdout.full_stream_sha256.as_deref(),
                Some(sha256_bytes(b"abcdefghij").as_str())
            );
        }

        #[test]
        fn clears_ambient_environment_and_uses_the_canonical_working_directory() {
            let environment = SubprocessExecutor
                .execute(&request(
                    system_program(&["/usr/bin/env", "/bin/env"]),
                    Vec::new(),
                ))
                .expect("environment process");
            assert_eq!(environment.stdout.retained_prefix, b"LANG=C\n");

            let working_directory = SubprocessExecutor
                .execute(&request(
                    system_program(&["/bin/pwd", "/usr/bin/pwd"]),
                    Vec::new(),
                ))
                .expect("working-directory process");
            let mut expected = working_directory
                .working_directory_identity
                .canonical_path()
                .as_os_str()
                .as_encoded_bytes()
                .to_vec();
            expected.push(b'\n');
            assert_eq!(working_directory.stdout.retained_prefix, expected);
        }

        #[test]
        fn continuously_writing_process_is_killed_at_the_execution_deadline() {
            let mut request = request(system_program(&["/usr/bin/yes", "/bin/yes"]), Vec::new());
            request.timeout = Duration::from_millis(100);
            request.cleanup_timeout = Duration::from_secs(1);
            request.max_retained_bytes_per_stream = 1024;
            let value = SubprocessExecutor
                .execute(&request)
                .expect("bounded process");
            assert_eq!(
                value.termination_reason,
                TerminationReason::DeadlineExceeded
            );
            assert_eq!(value.sent_signal, Some(SIGKILL_NUMBER));
            assert!(value.stdout.total_observed_bytes > 1024);
            assert!(value.elapsed < Duration::from_secs(2));
        }

        #[test]
        fn direct_exit_with_descendant_held_pipes_still_has_a_deadline() {
            let mut request = request(
                system_program(&["/bin/sh"]),
                vec!["-c".to_owned(), "/bin/sleep 30 &".to_owned()],
            );
            request.timeout = Duration::from_millis(150);
            request.cleanup_timeout = Duration::from_secs(1);
            let value = SubprocessExecutor
                .execute(&request)
                .expect("bounded process");
            assert_eq!(value.termination_reason, TerminationReason::CaptureFailure);
            assert_eq!(value.sent_signal, Some(SIGKILL_NUMBER));
            assert_eq!(value.cleanup_status, CleanupStatus::Complete);
            assert_eq!(
                value.adapter_failure.as_ref().map(AdapterFailure::code),
                Some(AdapterFailureCode::InheritedPipeAfterExit)
            );
            assert!(value.elapsed < Duration::from_secs(1));
        }

        #[test]
        fn drains_large_simultaneous_stdout_and_stderr_without_deadlock() {
            let mut request = request(
                system_program(&["/bin/sh"]),
                vec![
                    "-c".to_owned(),
                    "i=0; while [ \"$i\" -lt 20000 ]; do printf 'stdout-output\\n'; printf 'stderr-output\\n' >&2; i=$((i+1)); done".to_owned(),
                ],
            );
            request.timeout = Duration::from_secs(5);
            let value = SubprocessExecutor
                .execute(&request)
                .expect("simultaneous output process");
            assert_eq!(value.exit_code, Some(0));
            assert_eq!(value.stdout.total_observed_bytes, 280_000);
            assert_eq!(value.stderr.total_observed_bytes, 280_000);
            assert!(value.stdout.complete && value.stderr.complete);
        }

        #[test]
        fn implementation_contains_no_blocking_child_wait_call() {
            let blocking_wait_spelling = [".wa", "it("].concat();
            assert!(!include_str!("subprocess.rs").contains(&blocking_wait_spelling));
        }

        #[test]
        fn invalid_executable_is_rejected_before_spawn() {
            let directory = tempfile::tempdir().expect("temporary directory");
            let mut request = request(
                directory.path().join("missing").display().to_string(),
                Vec::new(),
            );
            request.working_directory = directory.path().to_owned();
            let error = SubprocessExecutor
                .execute(&request)
                .expect_err("missing executable");
            assert_eq!(error.code(), ProcessExecutionErrorCode::ExecutableIdentity);
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
use unix::execute;

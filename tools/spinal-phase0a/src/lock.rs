use crate::process::{
    CleanupStatus, LockEvidence, ProcessCapture, ProcessExecutionError, ProcessExecutionErrorCode,
    ProcessExecutor, ProcessRequest,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use thiserror::Error;

static EDITOR_EXECUTION_POISONED: AtomicBool = AtomicBool::new(false);

/// Failure to acquire the cross-process editor lock.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("editor lock failed: {message}")]
pub struct EditorLockError {
    message: String,
}

impl EditorLockError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Namespace for acquiring the one-at-a-time editor lock.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExclusiveEditorLock;

/// Held operating-system lock. Dropping it releases the lock.
#[derive(Debug)]
pub struct ExclusiveEditorLockGuard {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    file: std::fs::File,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    _parent: std::fs::File,
    evidence: LockEvidence,
}

impl ExclusiveEditorLockGuard {
    /// Returns evidence bound to this held lock acquisition.
    pub fn evidence(&self) -> &LockEvidence {
        &self.evidence
    }
}

impl ExclusiveEditorLock {
    /// Acquires an exclusive advisory lock, waiting no longer than `timeout`.
    ///
    /// The absolute lock path and its parent must already be canonical. The
    /// trusted parent and persistent regular lock file must be owned by the
    /// effective user, not group/world-writable, and hosted on a verified
    /// local filesystem.
    pub fn acquire(
        path: impl AsRef<Path>,
        timeout: Duration,
    ) -> Result<ExclusiveEditorLockGuard, EditorLockError> {
        if timeout.is_zero() {
            return Err(EditorLockError::new("acquisition timeout must be nonzero"));
        }
        acquire(path.as_ref(), timeout)
    }
}

/// Process executor that holds the OS editor lock for each complete call.
#[derive(Clone, Debug)]
pub struct LockedProcessExecutor<E> {
    inner: E,
    lock_path: PathBuf,
    lock_timeout: Duration,
}

impl<E> LockedProcessExecutor<E> {
    /// Wraps an executor with one explicit lock path and acquisition deadline.
    pub fn new(inner: E, lock_path: impl Into<PathBuf>, lock_timeout: Duration) -> Self {
        Self {
            inner,
            lock_path: lock_path.into(),
            lock_timeout,
        }
    }

    /// Returns the wrapped executor.
    pub fn inner(&self) -> &E {
        &self.inner
    }
}

impl<E: ProcessExecutor> ProcessExecutor for LockedProcessExecutor<E> {
    fn execute(&self, request: &ProcessRequest) -> Result<ProcessCapture, ProcessExecutionError> {
        if EDITOR_EXECUTION_POISONED.load(Ordering::SeqCst) {
            return Err(ProcessExecutionError::with_code(
                ProcessExecutionErrorCode::Lock,
                "editor execution is poisoned by incomplete prior cleanup; restart the coordinator",
            ));
        }
        let guard =
            ExclusiveEditorLock::acquire(&self.lock_path, self.lock_timeout).map_err(|error| {
                ProcessExecutionError::with_code(ProcessExecutionErrorCode::Lock, error.to_string())
            })?;
        let mut capture = self.inner.execute(request)?;
        capture.lock_evidence = Some(guard.evidence().clone());
        if capture.cleanup_status != CleanupStatus::Complete {
            EDITOR_EXECUTION_POISONED.store(true, Ordering::SeqCst);
            std::mem::forget(guard);
        }
        Ok(capture)
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn acquire(path: &Path, timeout: Duration) -> Result<ExclusiveEditorLockGuard, EditorLockError> {
    use rustix::fs::{FlockOperation, flock};
    use std::fs::File;
    use std::os::unix::fs::MetadataExt;
    use std::time::Instant;

    if !path.is_absolute() {
        return Err(EditorLockError::new("lock path must be absolute"));
    }
    let parent = path
        .parent()
        .ok_or_else(|| EditorLockError::new("lock path must have a trusted parent"))?;
    let file_name = path
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| EditorLockError::new("lock path must name a file"))?;
    let canonical_parent = std::fs::canonicalize(parent).map_err(|error| {
        EditorLockError::new(format!(
            "could not canonicalize lock parent `{}`: {error}",
            parent.display()
        ))
    })?;
    if canonical_parent != parent {
        return Err(EditorLockError::new(
            "lock parent must be supplied in canonical form",
        ));
    }
    let parent_metadata = std::fs::metadata(&canonical_parent).map_err(|error| {
        EditorLockError::new(format!("could not stat trusted lock parent: {error}"))
    })?;
    validate_owned_private_directory(&parent_metadata)?;
    let parent_file = File::open(&canonical_parent).map_err(|error| {
        EditorLockError::new(format!("could not open trusted lock parent: {error}"))
    })?;
    let opened_parent_metadata = parent_file.metadata().map_err(|error| {
        EditorLockError::new(format!("could not stat opened lock parent: {error}"))
    })?;
    validate_owned_private_directory(&opened_parent_metadata)?;
    if opened_parent_metadata.dev() != parent_metadata.dev()
        || opened_parent_metadata.ino() != parent_metadata.ino()
        || opened_parent_metadata.mode() != parent_metadata.mode()
    {
        return Err(EditorLockError::new(
            "trusted lock parent changed during secure open",
        ));
    }

    let file = securely_open_lock(&parent_file, file_name, path)?;
    let before = file.metadata().map_err(|error| {
        EditorLockError::new(format!("could not stat persistent lock file: {error}"))
    })?;
    validate_owned_private_file(&before)?;
    verify_lock_name_at_parent(&parent_file, file_name, &before)?;
    let filesystem_kind = verified_local_filesystem(&file)?;

    let canonical_path = std::fs::canonicalize(path).map_err(|error| {
        EditorLockError::new(format!(
            "could not canonicalize persistent lock file: {error}"
        ))
    })?;
    let intended_path = canonical_parent.join(file_name);
    if canonical_path != intended_path {
        return Err(EditorLockError::new(
            "persistent lock file did not resolve to the intended canonical path",
        ));
    }
    let path_metadata = std::fs::metadata(&canonical_path).map_err(|error| {
        EditorLockError::new(format!("could not verify persistent lock path: {error}"))
    })?;
    if path_metadata.dev() != before.dev() || path_metadata.ino() != before.ino() {
        return Err(EditorLockError::new(
            "persistent lock path changed during secure open",
        ));
    }

    let started = Instant::now();
    loop {
        match flock(&file, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => {
                let after = file.metadata().map_err(|error| {
                    EditorLockError::new(format!(
                        "could not restat acquired persistent lock: {error}"
                    ))
                })?;
                validate_owned_private_file(&after)?;
                if after.dev() != before.dev() || after.ino() != before.ino() {
                    return Err(EditorLockError::new(
                        "persistent lock identity changed during acquisition",
                    ));
                }
                verify_lock_name_at_parent(&parent_file, file_name, &after)?;
                let acquired_path_metadata =
                    std::fs::metadata(&canonical_path).map_err(|error| {
                        EditorLockError::new(format!(
                            "could not verify acquired persistent lock path: {error}"
                        ))
                    })?;
                if acquired_path_metadata.dev() != after.dev()
                    || acquired_path_metadata.ino() != after.ino()
                {
                    return Err(EditorLockError::new(
                        "persistent lock path changed during acquisition",
                    ));
                }
                return Ok(ExclusiveEditorLockGuard {
                    file,
                    _parent: parent_file,
                    evidence: LockEvidence::new_acquired(
                        canonical_path,
                        started.elapsed(),
                        after.dev(),
                        after.ino(),
                        filesystem_kind,
                    ),
                });
            }
            Err(error) if error == rustix::io::Errno::WOULDBLOCK => {
                let elapsed = started.elapsed();
                if elapsed >= timeout {
                    return Err(EditorLockError::new(format!(
                        "timed out acquiring `{}`",
                        path.display()
                    )));
                }
                std::thread::sleep((timeout - elapsed).min(Duration::from_millis(10)));
            }
            Err(error) => {
                return Err(EditorLockError::new(format!(
                    "could not lock `{}`: {error}",
                    path.display()
                )));
            }
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn verify_lock_name_at_parent(
    parent: &std::fs::File,
    file_name: &std::ffi::OsStr,
    expected: &std::fs::Metadata,
) -> Result<(), EditorLockError> {
    use rustix::fs::{AtFlags, statat};
    use std::os::unix::fs::MetadataExt;

    let path_stat = statat(parent, file_name, AtFlags::SYMLINK_NOFOLLOW).map_err(|error| {
        EditorLockError::new(format!(
            "could not verify persistent lock name relative to its parent: {error}"
        ))
    })?;
    #[cfg(target_os = "macos")]
    let device = u64::try_from(path_stat.st_dev).map_err(|_| {
        EditorLockError::new("persistent lock path reported an invalid device identity")
    })?;
    #[cfg(target_os = "linux")]
    let device = path_stat.st_dev;
    if device != expected.dev() || path_stat.st_ino != expected.ino() {
        return Err(EditorLockError::new(
            "persistent lock name changed relative to its trusted parent",
        ));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn securely_open_lock(
    parent: &std::fs::File,
    file_name: &std::ffi::OsStr,
    display_path: &Path,
) -> Result<std::fs::File, EditorLockError> {
    use rustix::fs::{Mode, OFlags, openat};

    let existing_flags = OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW;
    let opened = match openat(parent, file_name, existing_flags, Mode::empty()) {
        Ok(file) => Ok(file),
        Err(rustix::io::Errno::NOENT) => match openat(
            parent,
            file_name,
            existing_flags | OFlags::CREATE | OFlags::EXCL,
            Mode::RUSR | Mode::WUSR,
        ) {
            Ok(file) => Ok(file),
            Err(rustix::io::Errno::EXIST) => {
                openat(parent, file_name, existing_flags, Mode::empty())
            }
            Err(error) => Err(error),
        },
        Err(error) => Err(error),
    }
    .map_err(|error| {
        EditorLockError::new(format!(
            "could not securely open `{}`: {error}",
            display_path.display()
        ))
    })?;
    Ok(std::fs::File::from(opened))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn validate_owned_private_directory(metadata: &std::fs::Metadata) -> Result<(), EditorLockError> {
    if !metadata.is_dir() {
        return Err(EditorLockError::new("lock parent was not a directory"));
    }
    validate_owner_and_mode(metadata, "lock parent")
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn validate_owned_private_file(metadata: &std::fs::Metadata) -> Result<(), EditorLockError> {
    if !metadata.is_file() {
        return Err(EditorLockError::new(
            "persistent lock was not a regular file",
        ));
    }
    validate_owner_and_mode(metadata, "persistent lock file")
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn validate_owner_and_mode(
    metadata: &std::fs::Metadata,
    description: &str,
) -> Result<(), EditorLockError> {
    use std::os::unix::fs::MetadataExt;

    let effective_uid = rustix::process::geteuid().as_raw();
    if metadata.uid() != effective_uid {
        return Err(EditorLockError::new(format!(
            "{description} was not owned by the effective user"
        )));
    }
    if metadata.mode() & 0o022 != 0 {
        return Err(EditorLockError::new(format!(
            "{description} was group- or world-writable"
        )));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn verified_local_filesystem(file: &std::fs::File) -> Result<String, EditorLockError> {
    let status = rustix::fs::fstatfs(file).map_err(|error| {
        EditorLockError::new(format!("could not inspect lock filesystem: {error}"))
    })?;
    if status.f_flags & u32::try_from(libc::MNT_LOCAL).unwrap_or(0) == 0 {
        return Err(EditorLockError::new(
            "lock file must be hosted on a verified local filesystem",
        ));
    }
    Ok("macos-local".to_owned())
}

#[cfg(target_os = "linux")]
fn verified_local_filesystem(file: &std::fs::File) -> Result<String, EditorLockError> {
    let status = rustix::fs::fstatfs(file).map_err(|error| {
        EditorLockError::new(format!("could not inspect lock filesystem: {error}"))
    })?;
    let kind = match i128::from(status.f_type) {
        0xEF53 => "ext",
        0x5846_5342 => "xfs",
        0x9123_683E => "btrfs",
        0x0102_1994 => "tmpfs",
        0x794C_7630 => "overlay",
        0x2FC1_2FC1 => "zfs",
        0x8584_58F6 => "ramfs",
        0xF2F5_2010 => "f2fs",
        _ => {
            return Err(EditorLockError::new(format!(
                "lock filesystem type 0x{:x} is not in the local-filesystem allowlist",
                status.f_type
            )));
        }
    };
    Ok(kind.to_owned())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn acquire(_path: &Path, _timeout: Duration) -> Result<ExclusiveEditorLockGuard, EditorLockError> {
    Err(EditorLockError::new(
        "exclusive editor locking is supported only on macOS and Linux",
    ))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl Drop for ExclusiveEditorLockGuard {
    fn drop(&mut self) {
        let _ = rustix::fs::flock(&self.file, rustix::fs::FlockOperation::Unlock);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[derive(Clone)]
    struct ConcurrencyProbe {
        active: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        maximum: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    impl ProcessExecutor for ConcurrencyProbe {
        fn execute(
            &self,
            _request: &ProcessRequest,
        ) -> Result<ProcessCapture, ProcessExecutionError> {
            use std::sync::atomic::Ordering;

            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.maximum.fetch_max(active, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(60));
            self.active.fetch_sub(1, Ordering::SeqCst);
            let mut capture = crate::process::tests::capture();
            capture.lock_evidence = None;
            Ok(capture)
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn canonical_lock_path(directory: &tempfile::TempDir) -> PathBuf {
        std::fs::canonicalize(directory.path())
            .expect("canonical temporary directory")
            .join("editor.lock")
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn exclusive_lock_times_out_under_contention_and_can_be_reacquired() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = canonical_lock_path(&directory);
        std::fs::write(&path, b"persistent lock identity").expect("seed lock file");
        let first = ExclusiveEditorLock::acquire(&path, Duration::from_secs(1))
            .expect("first lock acquisition");
        assert!(first.evidence().acquired());

        let second_path = path.clone();
        let contender = std::thread::spawn(move || {
            ExclusiveEditorLock::acquire(second_path, Duration::from_millis(80))
        });
        let error = contender
            .join()
            .expect("contender thread")
            .expect_err("exclusive lock contention must time out");
        assert!(error.to_string().contains("timed out acquiring"));

        drop(first);
        ExclusiveEditorLock::acquire(&path, Duration::from_secs(1))
            .expect("lock can be reacquired after release");
        assert_eq!(
            std::fs::read(&path).expect("read persistent lock file"),
            b"persistent lock identity"
        );
    }

    #[test]
    fn zero_lock_timeout_is_never_treated_as_success() {
        let error = ExclusiveEditorLock::acquire("relative.lock", Duration::ZERO)
            .expect_err("zero timeout");
        assert!(error.to_string().contains("timeout"));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn relative_symlink_and_untrusted_parent_paths_are_rejected() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let relative = ExclusiveEditorLock::acquire("editor.lock", Duration::from_millis(10))
            .expect_err("relative path");
        assert!(relative.to_string().contains("absolute"));

        let directory = tempfile::tempdir().expect("temporary directory");
        let canonical = std::fs::canonicalize(directory.path()).expect("canonical directory");
        let target = canonical.join("target.lock");
        std::fs::write(&target, b"").expect("target lock");
        let link = canonical.join("link.lock");
        symlink(&target, &link).expect("lock symlink");
        assert!(ExclusiveEditorLock::acquire(&link, Duration::from_millis(10)).is_err());

        let permissions = std::fs::Permissions::from_mode(0o777);
        std::fs::set_permissions(&canonical, permissions).expect("untrusted permissions");
        let untrusted = ExclusiveEditorLock::acquire(
            canonical.join("untrusted.lock"),
            Duration::from_millis(10),
        )
        .expect_err("untrusted parent");
        assert!(untrusted.to_string().contains("group- or world-writable"));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn locked_executor_serializes_calls_and_binds_lock_evidence() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, Barrier};

        let directory = tempfile::tempdir().expect("temporary directory");
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let probe = ConcurrencyProbe {
            active: Arc::clone(&active),
            maximum: Arc::clone(&maximum),
        };
        let executor = LockedProcessExecutor::new(
            probe,
            canonical_lock_path(&directory),
            Duration::from_secs(1),
        );
        let request = crate::process::tests::request();
        let barrier = Arc::new(Barrier::new(3));
        let mut workers = Vec::new();
        for _ in 0..2 {
            let executor = executor.clone();
            let request = request.clone();
            let barrier = Arc::clone(&barrier);
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                executor.execute(&request).expect("locked execution")
            }));
        }
        barrier.wait();
        for worker in workers {
            let capture = worker.join().expect("worker thread");
            assert!(capture.lock_evidence.is_some_and(|value| value.acquired()));
        }
        assert_eq!(maximum.load(Ordering::SeqCst), 1);
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }
}

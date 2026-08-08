//! Descriptor-relative snapshots for one private Phase 0A run tree.
//!
//! The implementation is intentionally limited to macOS and Linux. Every
//! descendant is inspected and opened relative to an already-open directory,
//! with symbolic-link following disabled. Other platforms fail closed rather
//! than substituting a weaker path-based traversal.

use crate::digest::hex_digest;
use crate::package::{EntryKind, PackageInventory, TreeEntry};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use rustix::fs::{AtFlags, CWD, Dir, FileType, Mode, OFlags, fstat, open, openat, statat};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::ffi::CString;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::fs::File;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::io::Read;

const TREE_DIGEST_DOMAIN: &[u8] = b"spinal-phase0a-package-tree-v1\0";
const PHYSICAL_DIGEST_DOMAIN: &[u8] = b"spinal-phase0a-physical-workspace-v1\0";
const MAX_TREE_DEPTH: usize = 128;
const MAX_TREE_ENTRIES: usize = 100_000;
const MAX_PORTABLE_PATH_BYTES: usize = 4_096;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_TOTAL_FILE_BYTES: u64 = 16 * 1024 * 1024 * 1024;

/// A complete, physically bound view of one private run tree.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkspaceSnapshot {
    entries: BTreeMap<String, WorkspaceEntryState>,
    total_file_bytes: u64,
}

impl WorkspaceSnapshot {
    /// Returns the physical state for one portable relative path.
    pub(crate) fn entry(&self, path: &str) -> Option<&WorkspaceEntryState> {
        self.entries.get(path)
    }

    /// Returns all states in portable lexical path order.
    pub(crate) fn entries(&self) -> &BTreeMap<String, WorkspaceEntryState> {
        &self.entries
    }

    /// Projects the snapshot into the compact, report-safe package evidence.
    ///
    /// The projection preserves the complete directory and content inventory.
    /// Physical identities remain in this snapshot for mutation checks and do
    /// not enter the portable package artifact.
    pub(crate) fn evidence(&self) -> PackageInventory {
        let entries = self
            .entries
            .iter()
            .map(|(path, state)| TreeEntry {
                path: path.clone(),
                kind: state.kind,
                size: if state.kind == EntryKind::File {
                    state.size
                } else {
                    0
                },
                sha256: state.sha256.clone(),
            })
            .collect::<Vec<_>>();
        PackageInventory {
            tree_sha256: digest_tree(&entries),
            entries,
        }
    }

    /// Hashes the complete deterministic physical and content entry view.
    pub(crate) fn physical_sha256(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(PHYSICAL_DIGEST_DOMAIN);
        hasher.update((self.entries.len() as u64).to_be_bytes());
        hasher.update(self.total_file_bytes.to_be_bytes());
        for (path, state) in &self.entries {
            hasher.update((path.len() as u64).to_be_bytes());
            hasher.update(path.as_bytes());
            hasher.update([match state.kind {
                EntryKind::Directory => b'd',
                EntryKind::File => b'f',
            }]);
            for value in [
                state.device,
                state.inode,
                u64::from(state.mode),
                u64::from(state.owner),
                state.links,
                state.size,
            ] {
                hasher.update(value.to_be_bytes());
            }
            for value in [
                state.modified_seconds,
                state.modified_nanoseconds,
                state.changed_seconds,
                state.changed_nanoseconds,
            ] {
                hasher.update(value.to_be_bytes());
            }
            if let Some(sha256) = &state.sha256 {
                hasher.update((sha256.len() as u64).to_be_bytes());
                hasher.update(sha256.as_bytes());
            } else {
                hasher.update(0_u64.to_be_bytes());
            }
        }
        hex_digest(hasher.finalize().as_slice())
    }

    /// Projects one exact directory subtree into package-relative evidence.
    pub(crate) fn subtree_evidence(&self, root: &str) -> Option<PackageInventory> {
        if self
            .entry(root)
            .is_none_or(|entry| entry.kind != EntryKind::Directory)
        {
            return None;
        }
        let prefix = format!("{root}/");
        let entries = self
            .entries
            .iter()
            .filter_map(|(path, state)| {
                let relative = if path == root {
                    "."
                } else {
                    path.strip_prefix(&prefix)?
                };
                Some(TreeEntry {
                    path: relative.to_owned(),
                    kind: state.kind,
                    size: if state.kind == EntryKind::File {
                        state.size
                    } else {
                        0
                    },
                    sha256: state.sha256.clone(),
                })
            })
            .collect::<Vec<_>>();
        Some(PackageInventory {
            tree_sha256: digest_tree(&entries),
            entries,
        })
    }
}

/// Physical and content state for one opened workspace entry.
#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkspaceEntryState {
    kind: EntryKind,
    device: u64,
    inode: u64,
    mode: u32,
    owner: u32,
    links: u64,
    size: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    changed_seconds: i64,
    changed_nanoseconds: i64,
    sha256: Option<String>,
}

impl WorkspaceEntryState {
    /// Returns whether this entry is a regular file or directory.
    pub(crate) fn kind(&self) -> EntryKind {
        self.kind
    }

    /// Returns the exact regular-file length bound by this snapshot.
    pub(crate) fn size(&self) -> u64 {
        self.size
    }

    /// Returns the exact regular-file digest bound by this snapshot.
    pub(crate) fn sha256(&self) -> Option<&str> {
        self.sha256.as_deref()
    }

    /// Checks an already-opened file against all snapshot metadata.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(crate) fn matches_open_file(&self, file: &File) -> io::Result<bool> {
        let stat = fstat(file)?;
        let observed = state_from_stat(&stat, Path::new("<opened-workspace-file>"), None)
            .map_err(io::Error::other)?;
        Ok(self.exact_identity_eq(&observed))
    }

    fn exact_identity_eq(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.device == other.device
            && self.inode == other.inode
            && self.mode == other.mode
            && self.owner == other.owner
            && self.links == other.links
            && self.size == other.size
            && self.modified_seconds == other.modified_seconds
            && self.modified_nanoseconds == other.modified_nanoseconds
            && self.changed_seconds == other.changed_seconds
            && self.changed_nanoseconds == other.changed_nanoseconds
    }

    fn physical_identity(&self) -> (u64, u64) {
        (self.device, self.inode)
    }
}

impl PartialEq for WorkspaceEntryState {
    fn eq(&self, other: &Self) -> bool {
        let physical_metadata_equal = self.kind == other.kind
            && self.device == other.device
            && self.inode == other.inode
            && self.mode == other.mode
            && self.owner == other.owner;
        physical_metadata_equal
            && (self.kind == EntryKind::Directory
                || (self.links == other.links
                    && self.size == other.size
                    && self.sha256 == other.sha256
                    && self.modified_seconds == other.modified_seconds
                    && self.modified_nanoseconds == other.modified_nanoseconds
                    && self.changed_seconds == other.changed_seconds
                    && self.changed_nanoseconds == other.changed_nanoseconds))
    }
}

impl Eq for WorkspaceEntryState {}

/// Failures that make a run-tree snapshot untrustworthy.
#[derive(Debug, Error)]
pub(crate) enum WorkspaceSnapshotError {
    /// The host cannot provide the reviewed no-follow traversal contract.
    #[error("secure workspace snapshots are supported only on macOS and Linux")]
    UnsupportedPlatform,
    /// The root was not an absolute normalized path.
    #[error("workspace snapshot root must be an absolute normalized path: `{0}`")]
    InvalidRoot(PathBuf),
    /// The root or one of its descendants was a symbolic link.
    #[error("symbolic links are forbidden in a workspace snapshot: `{0}`")]
    Symlink(PathBuf),
    /// A socket, device, pipe, or another special entry was encountered.
    #[error("unsupported filesystem entry in workspace snapshot: `{0}`")]
    UnsupportedFileType(PathBuf),
    /// An entry name cannot be represented as one portable relative path.
    #[error("workspace contains a non-portable entry beneath `{0}`")]
    NonPortableName(PathBuf),
    /// A mount or another filesystem boundary was crossed below the root.
    #[error("workspace crosses a filesystem boundary at `{0}`")]
    NestedFilesystem(PathBuf),
    /// A regular file had aliases outside or within the run tree.
    #[error("workspace file must have exactly one physical link: `{0}`")]
    MultipleLinks(PathBuf),
    /// Two different tree paths resolved to one physical entry.
    #[error("workspace paths `{first}` and `{second}` share one physical identity")]
    DuplicateIdentity { first: PathBuf, second: PathBuf },
    /// An entry moved or changed while its state or bytes were observed.
    #[error("workspace entry changed while being snapshotted: `{0}`")]
    EntryChanged(PathBuf),
    /// A directory was not private to the effective user.
    #[error("workspace directory is not owner-private: `{0}`")]
    InsecureDirectory(PathBuf),
    /// The fixed recursion-depth bound was exceeded.
    #[error("workspace snapshot exceeds its depth limit at `{0}`")]
    DepthLimit(PathBuf),
    /// The fixed total-entry bound was exceeded.
    #[error("workspace snapshot exceeds its entry limit at `{0}`")]
    EntryLimit(PathBuf),
    /// One regular file exceeded the fixed byte bound.
    #[error("workspace file exceeds its byte limit: `{0}`")]
    FileByteLimit(PathBuf),
    /// All regular files together exceeded the fixed byte bound.
    #[error("workspace snapshot exceeds its total byte limit at `{0}`")]
    TotalByteLimit(PathBuf),
    /// A filesystem operation failed.
    #[error("could not {operation} `{path}`: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// Captures one quiescent private run tree without following symbolic links.
pub(crate) fn snapshot_workspace(root: &Path) -> Result<WorkspaceSnapshot, WorkspaceSnapshotError> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        snapshot_workspace_with(root, SnapshotLimits::production(), |_| {})
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = root;
        Err(WorkspaceSnapshotError::UnsupportedPlatform)
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Clone, Copy)]
struct SnapshotLimits {
    max_depth: usize,
    max_entries: usize,
    max_portable_path_bytes: usize,
    max_file_bytes: u64,
    max_total_file_bytes: u64,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl SnapshotLimits {
    const fn production() -> Self {
        Self {
            max_depth: MAX_TREE_DEPTH,
            max_entries: MAX_TREE_ENTRIES,
            max_portable_path_bytes: MAX_PORTABLE_PATH_BYTES,
            max_file_bytes: MAX_FILE_BYTES,
            max_total_file_bytes: MAX_TOTAL_FILE_BYTES,
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn snapshot_workspace_with<F>(
    root: &Path,
    limits: SnapshotLimits,
    mut before_file_read: F,
) -> Result<WorkspaceSnapshot, WorkspaceSnapshotError>
where
    F: FnMut(&Path),
{
    validate_root_path(root)?;
    if limits.max_entries == 0 {
        return Err(WorkspaceSnapshotError::EntryLimit(root.to_path_buf()));
    }

    let observed = statat(CWD, root, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|error| descriptor_io("inspect workspace root", root, error))?;
    reject_observed_type(&observed, root, EntryKind::Directory)?;
    let root_file = open(root, directory_flags(), Mode::empty())
        .map(File::from)
        .map_err(|error| descriptor_io("open workspace root", root, error))?;
    let root_state = state_from_stat(
        &fstat(&root_file)
            .map_err(|error| descriptor_io("inspect opened workspace root", root, error))?,
        root,
        None,
    )?;
    let observed_state = state_from_stat(&observed, root, None)?;
    if !root_state.exact_identity_eq(&observed_state) {
        return Err(WorkspaceSnapshotError::EntryChanged(root.to_path_buf()));
    }
    require_private_directory(&root_state, root)?;

    let mut traversal = TraversalState::new(limits);
    traversal.register_identity(".", &root_state, root)?;
    traversal.entries.insert(".".to_owned(), root_state.clone());
    walk_directory(
        &root_file,
        root,
        "",
        root_state.device,
        0,
        &mut traversal,
        &mut before_file_read,
    )?;
    let root_after = state_from_stat(
        &fstat(&root_file)
            .map_err(|error| descriptor_io("reinspect opened workspace root", root, error))?,
        root,
        None,
    )?;
    if !root_state.exact_identity_eq(&root_after) {
        return Err(WorkspaceSnapshotError::EntryChanged(root.to_path_buf()));
    }

    Ok(WorkspaceSnapshot {
        entries: traversal.entries,
        total_file_bytes: traversal.total_file_bytes,
    })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
struct TraversalState {
    limits: SnapshotLimits,
    entries: BTreeMap<String, WorkspaceEntryState>,
    physical_paths: BTreeMap<(u64, u64), String>,
    total_file_bytes: u64,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl TraversalState {
    fn new(limits: SnapshotLimits) -> Self {
        Self {
            limits,
            entries: BTreeMap::new(),
            physical_paths: BTreeMap::new(),
            total_file_bytes: 0,
        }
    }

    fn register_identity(
        &mut self,
        relative: &str,
        state: &WorkspaceEntryState,
        display_path: &Path,
    ) -> Result<(), WorkspaceSnapshotError> {
        if let Some(first) = self
            .physical_paths
            .insert(state.physical_identity(), relative.to_owned())
        {
            return Err(WorkspaceSnapshotError::DuplicateIdentity {
                first: PathBuf::from(first),
                second: display_path.to_path_buf(),
            });
        }
        Ok(())
    }

    fn reserve_file_bytes(&mut self, size: u64, path: &Path) -> Result<(), WorkspaceSnapshotError> {
        if size > self.limits.max_file_bytes {
            return Err(WorkspaceSnapshotError::FileByteLimit(path.to_path_buf()));
        }
        let total = self
            .total_file_bytes
            .checked_add(size)
            .ok_or_else(|| WorkspaceSnapshotError::TotalByteLimit(path.to_path_buf()))?;
        if total > self.limits.max_total_file_bytes {
            return Err(WorkspaceSnapshotError::TotalByteLimit(path.to_path_buf()));
        }
        self.total_file_bytes = total;
        Ok(())
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
fn walk_directory<F>(
    directory: &File,
    root: &Path,
    relative_parent: &str,
    root_device: u64,
    depth: usize,
    traversal: &mut TraversalState,
    before_file_read: &mut F,
) -> Result<(), WorkspaceSnapshotError>
where
    F: FnMut(&Path),
{
    let directory_path = display_path(root, relative_parent);
    let before = opened_state(directory, &directory_path, None)?;
    require_kind(&before, EntryKind::Directory, &directory_path)?;
    require_root_device(root_device, before.device, &directory_path)?;
    require_private_directory(&before, &directory_path)?;

    let remaining = traversal
        .limits
        .max_entries
        .saturating_sub(traversal.entries.len());
    let children = read_names(directory, &directory_path, remaining)?;
    for child in children {
        let child_depth = depth
            .checked_add(1)
            .ok_or_else(|| WorkspaceSnapshotError::DepthLimit(directory_path.clone()))?;
        let relative = portable_child_path(
            relative_parent,
            &child.text,
            traversal.limits.max_portable_path_bytes,
            &directory_path,
        )?;
        let path = root.join(&relative);
        if child_depth > traversal.limits.max_depth {
            return Err(WorkspaceSnapshotError::DepthLimit(path));
        }
        if traversal.entries.len() >= traversal.limits.max_entries {
            return Err(WorkspaceSnapshotError::EntryLimit(path));
        }

        let observed = statat(directory, child.raw.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|error| descriptor_io("inspect workspace entry", &path, error))?;
        let file_type = FileType::from_raw_mode(observed.st_mode);
        match file_type {
            FileType::Symlink => return Err(WorkspaceSnapshotError::Symlink(path)),
            FileType::Directory => {
                let child_file = openat(
                    directory,
                    child.raw.as_c_str(),
                    directory_flags(),
                    Mode::empty(),
                )
                .map(File::from)
                .map_err(|error| descriptor_io("open workspace directory", &path, error))?;
                let state = require_same_open_entry(&observed, &child_file, &path, None)?;
                require_root_device(root_device, state.device, &path)?;
                require_private_directory(&state, &path)?;
                traversal.register_identity(&relative, &state, &path)?;
                traversal.entries.insert(relative.clone(), state.clone());
                walk_directory(
                    &child_file,
                    root,
                    &relative,
                    root_device,
                    child_depth,
                    traversal,
                    before_file_read,
                )?;
                let after = opened_state(&child_file, &path, None)?;
                if !state.exact_identity_eq(&after) {
                    return Err(WorkspaceSnapshotError::EntryChanged(path));
                }
            }
            FileType::RegularFile => {
                let child_file = openat(
                    directory,
                    child.raw.as_c_str(),
                    file_read_flags(),
                    Mode::empty(),
                )
                .map(File::from)
                .map_err(|error| descriptor_io("open workspace file", &path, error))?;
                let mut state =
                    require_same_open_entry(&observed, &child_file, &path, Some(String::new()))?;
                require_root_device(root_device, state.device, &path)?;
                if state.links != 1 {
                    return Err(WorkspaceSnapshotError::MultipleLinks(path));
                }
                traversal.register_identity(&relative, &state, &path)?;
                traversal.reserve_file_bytes(state.size, &path)?;
                before_file_read(&path);
                state.sha256 = Some(hash_file(
                    child_file,
                    &state,
                    &path,
                    traversal.limits,
                    traversal.total_file_bytes.saturating_sub(state.size),
                )?);
                traversal.entries.insert(relative, state);
            }
            _ => return Err(WorkspaceSnapshotError::UnsupportedFileType(path)),
        }
    }

    let after = opened_state(directory, &directory_path, None)?;
    if !before.exact_identity_eq(&after) {
        return Err(WorkspaceSnapshotError::EntryChanged(directory_path));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
struct ChildName {
    raw: CString,
    text: String,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn read_names(
    directory: &File,
    path: &Path,
    remaining_entries: usize,
) -> Result<Vec<ChildName>, WorkspaceSnapshotError> {
    let mut reader = Dir::read_from(directory)
        .map_err(|error| descriptor_io("read workspace directory", path, error))?;
    let mut children = Vec::new();
    let mut folded = BTreeSet::new();
    while let Some(entry) = reader.read() {
        let entry =
            entry.map_err(|error| descriptor_io("read workspace directory", path, error))?;
        let raw = entry.file_name();
        if raw.to_bytes() == b"." || raw.to_bytes() == b".." {
            continue;
        }
        if children.len() >= remaining_entries {
            return Err(WorkspaceSnapshotError::EntryLimit(path.to_path_buf()));
        }
        let text = raw
            .to_str()
            .map_err(|_| WorkspaceSnapshotError::NonPortableName(path.to_path_buf()))?;
        validate_portable_component(text, path)?;
        register_folded_name(&mut folded, text, path)?;
        children.push(ChildName {
            raw: raw.to_owned(),
            text: text.to_owned(),
        });
    }
    children.sort_by(|left, right| left.text.cmp(&right.text));
    Ok(children)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn register_folded_name(
    folded: &mut BTreeSet<String>,
    text: &str,
    parent: &Path,
) -> Result<(), WorkspaceSnapshotError> {
    if !folded.insert(text.to_ascii_lowercase()) {
        return Err(WorkspaceSnapshotError::NonPortableName(
            parent.to_path_buf(),
        ));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn validate_portable_component(text: &str, parent: &Path) -> Result<(), WorkspaceSnapshotError> {
    let forbidden = text.is_empty()
        || text == "."
        || text == ".."
        || text.ends_with(['.', ' '])
        || text.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
        });
    let basename = text.split('.').next().unwrap_or(text).to_ascii_uppercase();
    let windows_reserved = matches!(basename.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || basename.strip_prefix("COM").is_some_and(|number| {
            matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
        })
        || basename.strip_prefix("LPT").is_some_and(|number| {
            matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
        });
    if forbidden || windows_reserved {
        return Err(WorkspaceSnapshotError::NonPortableName(
            parent.to_path_buf(),
        ));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn portable_child_path(
    parent: &str,
    child: &str,
    max_bytes: usize,
    display_parent: &Path,
) -> Result<String, WorkspaceSnapshotError> {
    let relative = if parent.is_empty() {
        child.to_owned()
    } else {
        format!("{parent}/{child}")
    };
    if relative.len() > max_bytes {
        return Err(WorkspaceSnapshotError::NonPortableName(
            display_parent.to_path_buf(),
        ));
    }
    Ok(relative)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn hash_file(
    mut file: File,
    before: &WorkspaceEntryState,
    path: &Path,
    limits: SnapshotLimits,
    prior_total: u64,
) -> Result<String, WorkspaceSnapshotError> {
    let mut hasher = Sha256::new();
    let mut observed_bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|source| WorkspaceSnapshotError::Io {
                operation: "read workspace file",
                path: path.to_path_buf(),
                source,
            })?;
        if count == 0 {
            break;
        }
        observed_bytes = observed_bytes
            .checked_add(count as u64)
            .ok_or_else(|| WorkspaceSnapshotError::FileByteLimit(path.to_path_buf()))?;
        if observed_bytes > limits.max_file_bytes {
            return Err(WorkspaceSnapshotError::FileByteLimit(path.to_path_buf()));
        }
        if prior_total
            .checked_add(observed_bytes)
            .is_none_or(|total| total > limits.max_total_file_bytes)
        {
            return Err(WorkspaceSnapshotError::TotalByteLimit(path.to_path_buf()));
        }
        hasher.update(&buffer[..count]);
    }
    let after = opened_state(&file, path, Some(String::new()))?;
    if !before.exact_identity_eq(&after) || observed_bytes != before.size {
        return Err(WorkspaceSnapshotError::EntryChanged(path.to_path_buf()));
    }
    Ok(hex_digest(hasher.finalize().as_slice()))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn require_same_open_entry(
    observed: &rustix::fs::Stat,
    opened: &File,
    path: &Path,
    sha256: Option<String>,
) -> Result<WorkspaceEntryState, WorkspaceSnapshotError> {
    let observed = state_from_stat(observed, path, sha256.clone())?;
    let opened = opened_state(opened, path, sha256)?;
    if !observed.exact_identity_eq(&opened) {
        return Err(WorkspaceSnapshotError::EntryChanged(path.to_path_buf()));
    }
    Ok(opened)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn opened_state(
    file: &File,
    path: &Path,
    sha256: Option<String>,
) -> Result<WorkspaceEntryState, WorkspaceSnapshotError> {
    let stat = fstat(file)
        .map_err(|error| descriptor_io("inspect opened workspace entry", path, error))?;
    state_from_stat(&stat, path, sha256)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[allow(
    clippy::unnecessary_cast,
    reason = "rustix Stat field widths differ between supported Unix targets"
)]
fn state_from_stat(
    stat: &rustix::fs::Stat,
    path: &Path,
    sha256: Option<String>,
) -> Result<WorkspaceEntryState, WorkspaceSnapshotError> {
    let kind = match FileType::from_raw_mode(stat.st_mode) {
        FileType::Directory => EntryKind::Directory,
        FileType::RegularFile => EntryKind::File,
        FileType::Symlink => return Err(WorkspaceSnapshotError::Symlink(path.to_path_buf())),
        _ => {
            return Err(WorkspaceSnapshotError::UnsupportedFileType(
                path.to_path_buf(),
            ));
        }
    };
    let size = u64::try_from(stat.st_size)
        .map_err(|_| WorkspaceSnapshotError::EntryChanged(path.to_path_buf()))?;
    Ok(WorkspaceEntryState {
        kind,
        device: stat.st_dev as u64,
        inode: stat.st_ino as u64,
        mode: stat.st_mode as u32,
        owner: stat.st_uid as u32,
        links: stat.st_nlink as u64,
        size,
        modified_seconds: stat.st_mtime as i64,
        modified_nanoseconds: stat.st_mtime_nsec as i64,
        changed_seconds: stat.st_ctime as i64,
        changed_nanoseconds: stat.st_ctime_nsec as i64,
        sha256,
    })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn reject_observed_type(
    stat: &rustix::fs::Stat,
    path: &Path,
    required: EntryKind,
) -> Result<(), WorkspaceSnapshotError> {
    let observed = match FileType::from_raw_mode(stat.st_mode) {
        FileType::Directory => EntryKind::Directory,
        FileType::RegularFile => EntryKind::File,
        FileType::Symlink => return Err(WorkspaceSnapshotError::Symlink(path.to_path_buf())),
        _ => {
            return Err(WorkspaceSnapshotError::UnsupportedFileType(
                path.to_path_buf(),
            ));
        }
    };
    if observed != required {
        return Err(WorkspaceSnapshotError::UnsupportedFileType(
            path.to_path_buf(),
        ));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn require_kind(
    state: &WorkspaceEntryState,
    required: EntryKind,
    path: &Path,
) -> Result<(), WorkspaceSnapshotError> {
    if state.kind != required {
        return Err(WorkspaceSnapshotError::EntryChanged(path.to_path_buf()));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn require_root_device(
    root_device: u64,
    device: u64,
    path: &Path,
) -> Result<(), WorkspaceSnapshotError> {
    if device != root_device {
        return Err(WorkspaceSnapshotError::NestedFilesystem(path.to_path_buf()));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn require_private_directory(
    state: &WorkspaceEntryState,
    path: &Path,
) -> Result<(), WorkspaceSnapshotError> {
    let owner = rustix::process::geteuid().as_raw();
    if state.kind != EntryKind::Directory
        || state.owner != owner
        || state.mode & 0o077 != 0
        || state.mode & 0o700 != 0o700
    {
        return Err(WorkspaceSnapshotError::InsecureDirectory(
            path.to_path_buf(),
        ));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn validate_root_path(root: &Path) -> Result<(), WorkspaceSnapshotError> {
    if !root.is_absolute()
        || root.components().any(|component| {
            !matches!(
                component,
                Component::RootDir | Component::Prefix(_) | Component::Normal(_)
            )
        })
    {
        return Err(WorkspaceSnapshotError::InvalidRoot(root.to_path_buf()));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn directory_flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn file_read_flags() -> OFlags {
    OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn display_path(root: &Path, relative: &str) -> PathBuf {
    if relative.is_empty() {
        root.to_path_buf()
    } else {
        root.join(relative)
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn descriptor_io(
    operation: &'static str,
    path: &Path,
    error: rustix::io::Errno,
) -> WorkspaceSnapshotError {
    WorkspaceSnapshotError::Io {
        operation,
        path: path.to_path_buf(),
        source: error.into(),
    }
}

fn digest_tree(entries: &[TreeEntry]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(TREE_DIGEST_DOMAIN);
    hasher.update((entries.len() as u64).to_be_bytes());
    for entry in entries {
        hasher.update([match entry.kind {
            EntryKind::Directory => b'd',
            EntryKind::File => b'f',
        }]);
        hasher.update((entry.path.len() as u64).to_be_bytes());
        hasher.update(entry.path.as_bytes());
        hasher.update(entry.size.to_be_bytes());
        if let Some(content) = &entry.sha256 {
            hasher.update((content.len() as u64).to_be_bytes());
            hasher.update(content.as_bytes());
        } else {
            hasher.update(0_u64.to_be_bytes());
        }
    }
    hex_digest(hasher.finalize().as_slice())
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;
    use std::fs::{self, hard_link};
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn private_root() -> (tempfile::TempDir, PathBuf) {
        let parent = tempfile::tempdir().expect("temporary parent");
        let root = parent.path().join("run");
        fs::create_dir(&root).expect("private root");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).expect("private mode");
        (parent, root)
    }

    fn test_limits() -> SnapshotLimits {
        SnapshotLimits {
            max_depth: 8,
            max_entries: 32,
            max_portable_path_bytes: 128,
            max_file_bytes: 64,
            max_total_file_bytes: 128,
        }
    }

    #[test]
    fn captures_sorted_physical_state_and_portable_evidence() {
        let (_parent, root) = private_root();
        fs::create_dir(root.join("assets")).expect("assets");
        fs::set_permissions(root.join("assets"), fs::Permissions::from_mode(0o700))
            .expect("assets private");
        fs::write(root.join("z.spine"), b"project").expect("project");
        fs::write(root.join("assets/a.png"), b"image").expect("image");

        let snapshot = snapshot_workspace(&root).expect("snapshot");
        assert_eq!(
            snapshot.entries().keys().cloned().collect::<Vec<_>>(),
            [".", "assets", "assets/a.png", "z.spine"]
        );
        assert_eq!(
            snapshot.entry("assets").map(WorkspaceEntryState::kind),
            Some(EntryKind::Directory)
        );
        let evidence = snapshot.evidence();
        assert_eq!(evidence.entries.len(), 4);
        assert_eq!(evidence.entries[0].path, ".");
        assert!(evidence.entries[2].sha256.is_some());
        assert_eq!(snapshot.total_file_bytes, 12);
    }

    #[test]
    fn snapshot_equality_ignores_volatile_directory_counters() {
        let directory = WorkspaceEntryState {
            kind: EntryKind::Directory,
            device: 1,
            inode: 2,
            mode: 0o40700,
            owner: 3,
            links: 2,
            size: 64,
            modified_seconds: 10,
            modified_nanoseconds: 11,
            changed_seconds: 12,
            changed_nanoseconds: 13,
            sha256: None,
        };
        let mut timestamp_changed = directory.clone();
        timestamp_changed.modified_seconds += 1;
        timestamp_changed.changed_nanoseconds += 1;
        timestamp_changed.links += 1;
        timestamp_changed.size += 32;
        assert_eq!(directory, timestamp_changed);

        let mut mode_changed = directory.clone();
        mode_changed.mode = 0o40750;
        assert_ne!(directory, mode_changed);

        let mut file = directory;
        file.kind = EntryKind::File;
        file.links = 1;
        file.sha256 = Some("00".repeat(32));
        let mut file_timestamp_changed = file.clone();
        file_timestamp_changed.modified_seconds += 1;
        assert_ne!(file, file_timestamp_changed);
    }

    #[test]
    fn physical_digest_detects_same_byte_file_replacement() {
        let (parent, root) = private_root();
        let project = root.join("character.spine");
        fs::write(&project, b"same bytes").expect("project");
        let before = snapshot_workspace(&root).expect("before snapshot");

        fs::rename(&project, parent.path().join("held-original")).expect("retain old inode");
        fs::write(&project, b"same bytes").expect("replacement project");
        let after = snapshot_workspace(&root).expect("after snapshot");

        assert_eq!(before.evidence(), after.evidence());
        assert_ne!(before.physical_sha256(), after.physical_sha256());
    }

    #[test]
    fn rejects_root_and_nested_symbolic_links() {
        let (_parent, root) = private_root();
        fs::write(root.join("target"), b"bytes").expect("target");
        symlink(root.join("target"), root.join("alias")).expect("nested symlink");
        assert!(matches!(
            snapshot_workspace(&root),
            Err(WorkspaceSnapshotError::Symlink(path)) if path.ends_with("alias")
        ));

        let alias = root.with_file_name("run-alias");
        symlink(&root, &alias).expect("root alias");
        assert!(matches!(
            snapshot_workspace(&alias),
            Err(WorkspaceSnapshotError::Symlink(path)) if path == alias
        ));
    }

    #[test]
    fn rejects_special_files() {
        use std::os::unix::net::UnixListener;

        let (_parent, root) = private_root();
        let _listener = match UnixListener::bind(root.join("socket")) {
            Ok(listener) => listener,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("unix socket: {error}"),
        };
        assert!(matches!(
            snapshot_workspace(&root),
            Err(WorkspaceSnapshotError::UnsupportedFileType(path)) if path.ends_with("socket")
        ));
    }

    #[test]
    fn rejects_hardlinked_files() {
        let (_parent, root) = private_root();
        fs::write(root.join("first"), b"bytes").expect("first");
        hard_link(root.join("first"), root.join("second")).expect("hard link");
        assert!(matches!(
            snapshot_workspace(&root),
            Err(WorkspaceSnapshotError::MultipleLinks(_))
        ));
    }

    #[test]
    fn rejects_nonportable_and_case_colliding_names() {
        let (_parent, root) = private_root();
        fs::write(root.join("bad\\name"), b"bytes").expect("nonportable Unix name");
        assert!(matches!(
            snapshot_workspace(&root),
            Err(WorkspaceSnapshotError::NonPortableName(_))
        ));

        let mut folded = BTreeSet::new();
        register_folded_name(&mut folded, "Cat", &root).expect("first spelling");
        assert!(matches!(
            register_folded_name(&mut folded, "cat", &root),
            Err(WorkspaceSnapshotError::NonPortableName(_))
        ));
    }

    #[test]
    fn detects_a_file_changed_after_it_was_opened() {
        let (_parent, root) = private_root();
        let project = root.join("project.spine");
        fs::write(&project, b"before").expect("project");
        let mut mutated = false;
        let error = snapshot_workspace_with(&root, test_limits(), |path| {
            if !mutated && path == project {
                mutated = true;
                fs::write(path, b"after with a different size").expect("mutate opened file");
            }
        })
        .expect_err("concurrent mutation must fail");
        assert!(matches!(error, WorkspaceSnapshotError::EntryChanged(path) if path == project));
    }

    #[test]
    fn enforces_per_file_and_total_byte_limits_before_unbounded_reads() {
        let (_parent, root) = private_root();
        fs::write(root.join("one"), b"1234").expect("one");
        let mut limits = test_limits();
        limits.max_file_bytes = 3;
        assert!(matches!(
            snapshot_workspace_with(&root, limits, |_| {}),
            Err(WorkspaceSnapshotError::FileByteLimit(path)) if path.ends_with("one")
        ));

        fs::write(root.join("two"), b"5678").expect("two");
        limits.max_file_bytes = 4;
        limits.max_total_file_bytes = 7;
        assert!(matches!(
            snapshot_workspace_with(&root, limits, |_| {}),
            Err(WorkspaceSnapshotError::TotalByteLimit(_))
        ));
    }

    #[test]
    fn enforces_entry_depth_and_portable_path_limits() {
        let (_parent, root) = private_root();
        fs::create_dir(root.join("nested")).expect("nested");
        fs::set_permissions(root.join("nested"), fs::Permissions::from_mode(0o700))
            .expect("nested private");
        fs::write(root.join("nested/file"), b"x").expect("file");

        let mut limits = test_limits();
        limits.max_entries = 2;
        assert!(matches!(
            snapshot_workspace_with(&root, limits, |_| {}),
            Err(WorkspaceSnapshotError::EntryLimit(_))
        ));

        limits = test_limits();
        limits.max_depth = 1;
        assert!(matches!(
            snapshot_workspace_with(&root, limits, |_| {}),
            Err(WorkspaceSnapshotError::DepthLimit(path)) if path.ends_with("nested/file")
        ));

        limits = test_limits();
        limits.max_portable_path_bytes = 6;
        assert!(matches!(
            snapshot_workspace_with(&root, limits, |_| {}),
            Err(WorkspaceSnapshotError::NonPortableName(_))
        ));
    }

    #[test]
    fn rejects_insecure_directories_and_nested_filesystems() {
        let (_parent, root) = private_root();
        fs::create_dir(root.join("shared")).expect("shared");
        fs::set_permissions(root.join("shared"), fs::Permissions::from_mode(0o755))
            .expect("shared mode");
        assert!(matches!(
            snapshot_workspace(&root),
            Err(WorkspaceSnapshotError::InsecureDirectory(path)) if path.ends_with("shared")
        ));

        assert!(matches!(
            require_root_device(1, 2, Path::new("mounted")),
            Err(WorkspaceSnapshotError::NestedFilesystem(path)) if path == Path::new("mounted")
        ));
    }

    #[test]
    fn duplicate_physical_identity_registration_fails_closed() {
        let state = WorkspaceEntryState {
            kind: EntryKind::File,
            device: 1,
            inode: 2,
            mode: 0o100600,
            owner: rustix::process::geteuid().as_raw(),
            links: 1,
            size: 0,
            modified_seconds: 0,
            modified_nanoseconds: 0,
            changed_seconds: 0,
            changed_nanoseconds: 0,
            sha256: Some("00".repeat(32)),
        };
        let mut traversal = TraversalState::new(test_limits());
        traversal
            .register_identity("first", &state, Path::new("first"))
            .expect("first identity");
        assert!(matches!(
            traversal.register_identity("second", &state, Path::new("second")),
            Err(WorkspaceSnapshotError::DuplicateIdentity { .. })
        ));
    }
}

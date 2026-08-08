//! Descriptor-relative staging of immutable Phase 0A package trees.
//!
//! The strong staging implementation is deliberately limited to macOS and
//! Linux, the two platforms where the process boundary already provides its
//! reviewed guarantees. Other platforms fail closed instead of falling back
//! to path-based copying that could follow a replaced symbolic link.

use crate::case::PackageSpec;
use crate::package::PackageInventory;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::package::{EntryKind, TreeEntry};
use serde::Serialize;
use std::collections::BTreeSet;
use std::io;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::digest::hex_digest;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use rustix::fs::{
    AtFlags, Dir, FileType, Mode, OFlags, fchmod, fstat, mkdirat, open, openat, statat,
};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use sha2::{Digest, Sha256};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::ffi::CString;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::fs::{self, File};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::io::{Read, Write};

#[cfg(any(target_os = "linux", target_os = "macos"))]
const TREE_DIGEST_DOMAIN: &[u8] = b"spinal-phase0a-package-tree-v1\0";
#[cfg(any(target_os = "linux", target_os = "macos"))]
const MAX_TREE_DEPTH: usize = 128;
#[cfg(any(target_os = "linux", target_os = "macos"))]
const MAX_TREE_ENTRIES: usize = 100_000;
#[cfg(any(target_os = "linux", target_os = "macos"))]
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
#[cfg(any(target_os = "linux", target_os = "macos"))]
const MAX_TREE_BYTES: u64 = 16 * 1024 * 1024 * 1024;

/// A complete package staged into a fresh private directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StagedPackage {
    source_root: PathBuf,
    source_root_device: u64,
    source_root_inode: u64,
    root: PathBuf,
    project: PathBuf,
    source_before: PackageInventory,
    staged: PackageInventory,
    source_after: PackageInventory,
}

/// Result of a best-effort post-attempt source-package re-inventory.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ControlledSourceRecheckStatus {
    Unchanged,
    Changed,
    Unavailable,
}

/// Exact before/after portable inventories retained for a controlled failure.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ControlledSourceRecheck {
    status: ControlledSourceRecheckStatus,
    before: PackageInventory,
    after: Option<PackageInventory>,
}

impl ControlledSourceRecheck {
    pub(crate) const fn status(&self) -> ControlledSourceRecheckStatus {
        self.status
    }
}

impl StagedPackage {
    /// Returns the canonical source package root that was observed.
    pub fn source_root(&self) -> &Path {
        &self.source_root
    }

    /// Returns the staging-time device and inode of the retained source root.
    pub(crate) fn source_root_identity(&self) -> (u64, u64) {
        (self.source_root_device, self.source_root_inode)
    }

    /// Returns the fresh private staged package root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the writable staged `.spine` project path.
    pub fn project(&self) -> &Path {
        &self.project
    }

    /// Returns the secure source inventory captured before copying.
    pub fn source_before(&self) -> &PackageInventory {
        &self.source_before
    }

    /// Returns the secure inventory of the completed staged package.
    pub fn staged(&self) -> &PackageInventory {
        &self.staged
    }

    /// Returns the secure source inventory captured after copying.
    pub fn source_after(&self) -> &PackageInventory {
        &self.source_after
    }

    /// Securely re-inventories the original package after all editor work.
    ///
    /// This is deliberately separate from staging-time checks: the Phase 0A
    /// orchestrator must call it only after the final Spine operation.
    pub fn verify_source_unchanged(&self) -> Result<PackageInventory, StageError> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let current = snapshot(&self.source_root)?;
            if current.root_identity.device != self.source_root_device
                || current.root_identity.inode != self.source_root_inode
                || current.inventory != self.source_before
            {
                return Err(StageError::SourceChanged);
            }
            Ok(current.inventory)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Err(StageError::UnsupportedPlatform)
        }
    }

    /// Re-inventories the immutable source for failure reporting without
    /// converting an unavailable scan into a false changed/unchanged claim.
    pub(crate) fn controlled_source_recheck(&self) -> ControlledSourceRecheck {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            match snapshot(&self.source_root) {
                Ok(current) => {
                    let unchanged = current.root_identity.device == self.source_root_device
                        && current.root_identity.inode == self.source_root_inode
                        && current.inventory == self.source_before;
                    ControlledSourceRecheck {
                        status: if unchanged {
                            ControlledSourceRecheckStatus::Unchanged
                        } else {
                            ControlledSourceRecheckStatus::Changed
                        },
                        before: self.source_before.clone(),
                        after: Some(current.inventory),
                    }
                }
                Err(_) => ControlledSourceRecheck {
                    status: ControlledSourceRecheckStatus::Unavailable,
                    before: self.source_before.clone(),
                    after: None,
                },
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            ControlledSourceRecheck {
                status: ControlledSourceRecheckStatus::Unavailable,
                before: self.source_before.clone(),
                after: None,
            }
        }
    }
}

/// Securely inventories one complete declared package without copying it.
pub fn secure_inventory_package(package: &PackageSpec) -> Result<PackageInventory, StageError> {
    validate_package_spec(package)?;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        reject_root_symlink_or_special(&package.root)?;
        let root = fs::canonicalize(&package.root).map_err(|source| StageError::Io {
            operation: "canonicalize package root",
            path: package.root.clone(),
            source,
        })?;
        let snapshot = snapshot(&root)?;
        validate_declared_paths(package, &snapshot.inventory)?;
        Ok(snapshot.inventory)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Err(StageError::UnsupportedPlatform)
    }
}

/// Failures that prevent a source package from becoming a trustworthy stage.
#[derive(Debug, Error)]
pub enum StageError {
    /// The host cannot provide the reviewed no-follow staging implementation.
    #[error("secure package staging is supported only on macOS and Linux")]
    UnsupportedPlatform,
    /// A root or declared package path was not absolute or safely relative.
    #[error("invalid {role} path `{path}`: {reason}")]
    InvalidPath {
        /// Stable description of the path's purpose.
        role: &'static str,
        /// Rejected path.
        path: PathBuf,
        /// Concise fixed-policy explanation.
        reason: &'static str,
    },
    /// The destination already existed and therefore was not a fresh stage.
    #[error("staging destination already exists: `{0}`")]
    DestinationExists(PathBuf),
    /// The destination aliases, contains, or is contained by the source.
    #[error("staging destination aliases the source package: `{0}`")]
    DestinationAliasesSource(PathBuf),
    /// The destination parent is on a different filesystem from the source.
    #[error("staging destination must be on the same filesystem as the source package")]
    DifferentFilesystem,
    /// A symbolic link was encountered at or beneath the source root.
    #[error("symbolic links are forbidden in staged packages: `{0}`")]
    Symlink(PathBuf),
    /// A socket, device, pipe, or another special entry was encountered.
    #[error("unsupported filesystem entry in staged package: `{0}`")]
    UnsupportedFileType(PathBuf),
    /// A package entry name was not portable UTF-8.
    #[error("package contains a non-portable entry beneath `{0}`")]
    NonPortableName(PathBuf),
    /// A package entry moved or changed while its bytes were observed.
    #[error("source package changed while staging `{0}`")]
    EntryChanged(PathBuf),
    /// A mount or another filesystem boundary was encountered inside a package.
    #[error("package crosses a filesystem boundary at `{0}`")]
    NestedFilesystem(PathBuf),
    /// The package exceeded the fixed recursion or entry bound.
    #[error("package tree exceeds the staging safety bound at `{0}`")]
    TreeLimit(PathBuf),
    /// A declared project or directory was missing or had the wrong kind.
    #[error("package is missing declared {expected} `{path}`")]
    MissingDeclaredPath {
        /// Expected stable entry kind.
        expected: &'static str,
        /// Portable package-relative path.
        path: String,
    },
    /// The source inventory changed before or during the copy.
    #[error("source package changed before or during staging")]
    SourceChangedDuringCopy,
    /// The source inventory changed after the copy completed.
    #[error("source package changed during the staging operation")]
    SourceChanged,
    /// The staged tree did not exactly reproduce the source bytes and directories.
    #[error("staged package inventory does not match the source inventory")]
    StagedInventoryMismatch,
    /// The created stage did not retain private, writable ownership.
    #[error("created staging entry is not private and writable: `{0}`")]
    InsecureDestination(PathBuf),
    /// A filesystem operation failed.
    #[error("could not {operation} `{path}`: {source}")]
    Io {
        /// Stable operation description.
        operation: &'static str,
        /// Path involved in the failure.
        path: PathBuf,
        /// Underlying operating-system failure.
        #[source]
        source: io::Error,
    },
}

/// Copies one complete package into a fresh private directory.
///
/// `destination` must be an absolute, nonexistent path on the source
/// filesystem. The source is securely inventoried before and after copying,
/// and the staged tree is independently inventoried. All traversal beneath
/// both roots is descriptor-relative with no-follow opens. Files in the stage
/// are owner-readable and owner-writable; directories are owner-private.
///
/// On failure after destination creation, the incomplete private directory is
/// deliberately retained for inspection and must never be reused.
pub fn stage_package(
    package: &PackageSpec,
    destination: impl AsRef<Path>,
) -> Result<StagedPackage, StageError> {
    validate_package_spec(package)?;
    let destination = destination.as_ref();
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        stage_package_with_hook(package, destination, || {})
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = destination;
        Err(StageError::UnsupportedPlatform)
    }
}

fn validate_package_spec(package: &PackageSpec) -> Result<(), StageError> {
    if !package.root.is_absolute() {
        return Err(invalid_path(
            "source root",
            &package.root,
            "must be absolute",
        ));
    }
    let project = portable_relative("project", &package.project)?;
    if !project.ends_with(".spine") {
        return Err(invalid_path(
            "project",
            &package.project,
            "must end in `.spine`",
        ));
    }
    if package.required_directories.is_empty() {
        return Err(invalid_path(
            "required directory list",
            Path::new("."),
            "must not be empty",
        ));
    }
    if package.asset_roots.is_empty() {
        return Err(invalid_path(
            "asset root list",
            Path::new("."),
            "must not be empty",
        ));
    }

    let required = package
        .required_directories
        .iter()
        .map(|path| portable_relative("required directory", path))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let assets = package
        .asset_roots
        .iter()
        .map(|path| portable_relative("asset root", path))
        .collect::<Result<BTreeSet<_>, _>>()?;
    if required.len() != package.required_directories.len()
        || assets.len() != package.asset_roots.len()
    {
        return Err(invalid_path(
            "declared package path",
            Path::new("."),
            "entries must be unique",
        ));
    }
    if !assets.is_subset(&required) {
        return Err(invalid_path(
            "asset root",
            Path::new("."),
            "must also be a required directory",
        ));
    }
    Ok(())
}

fn portable_relative(role: &'static str, path: &Path) -> Result<String, StageError> {
    let text = path
        .to_str()
        .ok_or_else(|| invalid_path(role, path, "must be UTF-8"))?;
    if text.is_empty()
        || text.starts_with('/')
        || text.ends_with('/')
        || text.contains('\\')
        || text.split('/').any(|part| part.is_empty())
        || path.components().any(|component| {
            !matches!(component, Component::Normal(_))
                || matches!(component, Component::Normal(value) if value == "." || value == "..")
        })
    {
        return Err(invalid_path(
            role,
            path,
            "must be a normalized portable relative path",
        ));
    }
    Ok(text.to_owned())
}

fn invalid_path(role: &'static str, path: &Path, reason: &'static str) -> StageError {
    StageError::InvalidPath {
        role,
        path: path.to_path_buf(),
        reason,
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn stage_package_with_hook(
    package: &PackageSpec,
    destination: &Path,
    after_initial_inventory: impl FnOnce(),
) -> Result<StagedPackage, StageError> {
    reject_root_symlink_or_special(&package.root)?;
    let source_root = fs::canonicalize(&package.root).map_err(|source| StageError::Io {
        operation: "canonicalize source package root",
        path: package.root.clone(),
        source,
    })?;
    let (destination_root, destination_parent, destination_name) =
        resolve_destination(destination, &source_root)?;

    let source_before = snapshot(&source_root)?;
    validate_declared_paths(package, &source_before.inventory)?;

    let parent = open_directory(&destination_parent, "open staging parent")?;
    let parent_identity = identity(&parent, &destination_parent)?;
    if parent_identity.file_type != FileType::Directory {
        return Err(StageError::UnsupportedFileType(destination_parent));
    }
    if parent_identity.device != source_before.root_identity.device {
        return Err(StageError::DifferentFilesystem);
    }

    mkdirat(
        &parent,
        destination_name.as_c_str(),
        Mode::from_bits_retain(0o700),
    )
    .map_err(|error| {
        if error == rustix::io::Errno::EXIST {
            StageError::DestinationExists(destination_root.clone())
        } else {
            descriptor_io("create private staging directory", &destination_root, error)
        }
    })?;
    let staged_root_file = openat(
        &parent,
        destination_name.as_c_str(),
        directory_flags(),
        Mode::empty(),
    )
    .map(File::from)
    .map_err(|error| descriptor_io("open private staging directory", &destination_root, error))?;
    make_private(&staged_root_file, &destination_root, 0o700)?;

    after_initial_inventory();

    let source_for_copy = open_directory(&source_root, "reopen source package for copy")?;
    let copy_root_identity = identity(&source_for_copy, &source_root)?;
    let copied_source = inventory_open_tree(
        &source_for_copy,
        Some(&staged_root_file),
        &source_root,
        Some(&destination_root),
    )?;
    if copy_root_identity != source_before.root_identity || copied_source != source_before.inventory
    {
        return Err(StageError::SourceChangedDuringCopy);
    }

    drop(staged_root_file);
    drop(parent);

    let staged = snapshot(&destination_root)?;
    let source_after = snapshot(&source_root)?;
    if source_after.root_identity != source_before.root_identity
        || source_after.inventory != source_before.inventory
    {
        return Err(StageError::SourceChanged);
    }
    if staged.inventory != source_before.inventory {
        return Err(StageError::StagedInventoryMismatch);
    }
    validate_declared_paths(package, &staged.inventory)?;

    let project = destination_root.join(&package.project);
    let project_metadata = fs::metadata(&project).map_err(|source| StageError::Io {
        operation: "inspect staged project permissions",
        path: project.clone(),
        source,
    })?;
    use std::os::unix::fs::MetadataExt;
    if project_metadata.mode() & 0o200 == 0 || project_metadata.mode() & 0o077 != 0 {
        return Err(StageError::InsecureDestination(project));
    }

    Ok(StagedPackage {
        source_root,
        source_root_device: source_before.root_identity.device,
        source_root_inode: source_before.root_identity.inode,
        root: destination_root,
        project,
        source_before: source_before.inventory,
        staged: staged.inventory,
        source_after: source_after.inventory,
    })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn reject_root_symlink_or_special(path: &Path) -> Result<(), StageError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| StageError::Io {
        operation: "inspect source package root",
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() {
        return Err(StageError::Symlink(path.to_path_buf()));
    }
    if !metadata.file_type().is_dir() {
        return Err(StageError::UnsupportedFileType(path.to_path_buf()));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn resolve_destination(
    destination: &Path,
    source_root: &Path,
) -> Result<(PathBuf, PathBuf, CString), StageError> {
    if !destination.is_absolute() {
        return Err(invalid_path(
            "staging destination",
            destination,
            "must be absolute",
        ));
    }
    let parent = destination.parent().ok_or_else(|| {
        invalid_path(
            "staging destination",
            destination,
            "must have an existing parent",
        )
    })?;
    let name = destination.file_name().ok_or_else(|| {
        invalid_path(
            "staging destination",
            destination,
            "must name one fresh directory",
        )
    })?;
    let name_text = name.to_str().ok_or_else(|| {
        invalid_path(
            "staging destination",
            destination,
            "directory name must be UTF-8",
        )
    })?;
    if name_text.is_empty()
        || name_text == "."
        || name_text == ".."
        || name_text.contains(['/', '\\'])
    {
        return Err(invalid_path(
            "staging destination",
            destination,
            "must name one normalized directory component",
        ));
    }
    let parent = fs::canonicalize(parent).map_err(|source| StageError::Io {
        operation: "canonicalize staging parent",
        path: parent.to_path_buf(),
        source,
    })?;
    let resolved = parent.join(name);
    if resolved == source_root
        || resolved.starts_with(source_root)
        || source_root.starts_with(&resolved)
    {
        return Err(StageError::DestinationAliasesSource(resolved));
    }
    match fs::symlink_metadata(&resolved) {
        Ok(_) => return Err(StageError::DestinationExists(resolved)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(StageError::Io {
                operation: "inspect staging destination",
                path: resolved,
                source,
            });
        }
    }
    let name = CString::new(name_text).map_err(|_| {
        invalid_path(
            "staging destination",
            destination,
            "directory name must not contain NUL",
        )
    })?;
    Ok((resolved, parent, name))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Debug)]
struct Snapshot {
    inventory: PackageInventory,
    root_identity: FileIdentity,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn snapshot(root: &Path) -> Result<Snapshot, StageError> {
    let root_file = open_directory(root, "open package root for inventory")?;
    let root_identity = identity(&root_file, root)?;
    let inventory = inventory_open_tree(&root_file, None, root, None)?;
    let after = identity(&root_file, root)?;
    if after != root_identity {
        return Err(StageError::EntryChanged(root.to_path_buf()));
    }
    Ok(Snapshot {
        inventory,
        root_identity,
    })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn inventory_open_tree(
    source_root: &File,
    destination_root: Option<&File>,
    source_path: &Path,
    destination_path: Option<&Path>,
) -> Result<PackageInventory, StageError> {
    let root_identity = identity(source_root, source_path)?;
    if root_identity.file_type != FileType::Directory {
        return Err(StageError::UnsupportedFileType(source_path.to_path_buf()));
    }
    let mut entries = vec![TreeEntry {
        path: ".".to_owned(),
        kind: EntryKind::Directory,
        size: 0,
        sha256: None,
    }];
    let mut total_bytes = 0_u64;
    walk_directory(
        source_root,
        destination_root,
        source_path,
        destination_path,
        "",
        root_identity.device,
        0,
        &mut entries,
        &mut total_bytes,
    )?;
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(PackageInventory {
        tree_sha256: digest_tree(&entries),
        entries,
    })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
fn walk_directory(
    source: &File,
    destination: Option<&File>,
    source_root: &Path,
    destination_root: Option<&Path>,
    relative_parent: &str,
    root_device: u64,
    depth: usize,
    entries: &mut Vec<TreeEntry>,
    total_bytes: &mut u64,
) -> Result<(), StageError> {
    if depth > MAX_TREE_DEPTH {
        return Err(StageError::TreeLimit(source_root.join(relative_parent)));
    }
    let directory_path = source_root.join(relative_parent);
    let before = identity(source, &directory_path)?;
    if before.file_type != FileType::Directory {
        return Err(StageError::EntryChanged(directory_path));
    }
    if before.device != root_device {
        return Err(StageError::NestedFilesystem(directory_path));
    }

    let children = read_names(source, &directory_path)?;
    for child in children {
        if entries.len() >= MAX_TREE_ENTRIES {
            return Err(StageError::TreeLimit(directory_path));
        }
        let relative = if relative_parent.is_empty() {
            child.text.clone()
        } else {
            format!("{relative_parent}/{}", child.text)
        };
        let source_child_path = source_root.join(&relative);
        let observed = statat(source, child.raw.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|error| descriptor_io("inspect package entry", &source_child_path, error))?;
        let observed_type = FileType::from_raw_mode(observed.st_mode);
        match observed_type {
            FileType::Symlink => return Err(StageError::Symlink(source_child_path)),
            FileType::Directory => {
                let source_child = openat(
                    source,
                    child.raw.as_c_str(),
                    directory_flags(),
                    Mode::empty(),
                )
                .map(File::from)
                .map_err(|error| {
                    descriptor_io("open package directory", &source_child_path, error)
                })?;
                require_same_open_entry(&observed, &source_child, &source_child_path)?;
                let destination_child = if let (Some(parent), Some(root)) =
                    (destination, destination_root)
                {
                    let path = root.join(&relative);
                    mkdirat(parent, child.raw.as_c_str(), Mode::from_bits_retain(0o700))
                        .map_err(|error| descriptor_io("create staged directory", &path, error))?;
                    let file = openat(
                        parent,
                        child.raw.as_c_str(),
                        directory_flags(),
                        Mode::empty(),
                    )
                    .map(File::from)
                    .map_err(|error| descriptor_io("open staged directory", &path, error))?;
                    make_private(&file, &path, 0o700)?;
                    Some(file)
                } else {
                    None
                };
                entries.push(TreeEntry {
                    path: relative.clone(),
                    kind: EntryKind::Directory,
                    size: 0,
                    sha256: None,
                });
                walk_directory(
                    &source_child,
                    destination_child.as_ref(),
                    source_root,
                    destination_root,
                    &relative,
                    root_device,
                    depth + 1,
                    entries,
                    total_bytes,
                )?;
            }
            FileType::RegularFile => {
                let observed_identity = identity_from_stat(&observed, &source_child_path)?;
                let next_total = total_bytes
                    .checked_add(observed_identity.size)
                    .ok_or_else(|| StageError::TreeLimit(source_child_path.clone()))?;
                if observed_identity.size > MAX_FILE_BYTES || next_total > MAX_TREE_BYTES {
                    return Err(StageError::TreeLimit(source_child_path));
                }
                let source_child = openat(
                    source,
                    child.raw.as_c_str(),
                    file_read_flags(),
                    Mode::empty(),
                )
                .map(File::from)
                .map_err(|error| descriptor_io("open package file", &source_child_path, error))?;
                require_same_open_entry(&observed, &source_child, &source_child_path)?;
                let destination_child =
                    if let (Some(parent), Some(root)) = (destination, destination_root) {
                        let path = root.join(&relative);
                        let file = openat(
                            parent,
                            child.raw.as_c_str(),
                            OFlags::WRONLY
                                | OFlags::CREATE
                                | OFlags::EXCL
                                | OFlags::NOFOLLOW
                                | OFlags::CLOEXEC,
                            Mode::from_bits_retain(0o600),
                        )
                        .map(File::from)
                        .map_err(|error| descriptor_io("create staged file", &path, error))?;
                        make_private(&file, &path, 0o600)?;
                        Some((file, path))
                    } else {
                        None
                    };
                let (size, sha256) = copy_regular_file(
                    source_child,
                    destination_child,
                    &source_child_path,
                    root_device,
                )?;
                entries.push(TreeEntry {
                    path: relative,
                    kind: EntryKind::File,
                    size,
                    sha256: Some(sha256),
                });
                *total_bytes = next_total;
            }
            _ => return Err(StageError::UnsupportedFileType(source_child_path)),
        }
    }
    if identity(source, &directory_path)? != before {
        return Err(StageError::EntryChanged(directory_path));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
struct ChildName {
    raw: CString,
    text: String,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn read_names(directory: &File, path: &Path) -> Result<Vec<ChildName>, StageError> {
    let mut reader = Dir::read_from(directory)
        .map_err(|error| descriptor_io("read package directory", path, error))?;
    let mut names = Vec::new();
    while let Some(entry) = reader.read() {
        let entry = entry.map_err(|error| descriptor_io("read package directory", path, error))?;
        let raw = entry.file_name();
        if raw.to_bytes() == b"." || raw.to_bytes() == b".." {
            continue;
        }
        let text = raw
            .to_str()
            .map_err(|_| StageError::NonPortableName(path.to_path_buf()))?;
        if text.is_empty() || text.contains(['/', '\\']) {
            return Err(StageError::NonPortableName(path.to_path_buf()));
        }
        names.push(ChildName {
            raw: raw.to_owned(),
            text: text.to_owned(),
        });
    }
    names.sort_by(|left, right| left.text.cmp(&right.text));
    Ok(names)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn copy_regular_file(
    mut source: File,
    destination: Option<(File, PathBuf)>,
    source_path: &Path,
    root_device: u64,
) -> Result<(u64, String), StageError> {
    let before = identity(&source, source_path)?;
    if before.file_type != FileType::RegularFile {
        return Err(StageError::EntryChanged(source_path.to_path_buf()));
    }
    if before.device != root_device {
        return Err(StageError::NestedFilesystem(source_path.to_path_buf()));
    }
    let mut destination = destination;
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = source.read(&mut buffer).map_err(|source| StageError::Io {
            operation: "read source package file",
            path: source_path.to_path_buf(),
            source,
        })?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .ok_or_else(|| StageError::EntryChanged(source_path.to_path_buf()))?;
        hasher.update(&buffer[..count]);
        if let Some((file, path)) = destination.as_mut() {
            file.write_all(&buffer[..count])
                .map_err(|source| StageError::Io {
                    operation: "write staged package file",
                    path: path.clone(),
                    source,
                })?;
        }
    }
    if let Some((file, path)) = destination.as_mut() {
        file.flush().map_err(|source| StageError::Io {
            operation: "flush staged package file",
            path: path.clone(),
            source,
        })?;
        let staged = identity(file, path)?;
        if staged.file_type != FileType::RegularFile
            || staged.size != total
            || staged.mode & 0o777 != 0o600
        {
            return Err(StageError::InsecureDestination(path.clone()));
        }
    }
    let after = identity(&source, source_path)?;
    if after != before || total != before.size {
        return Err(StageError::EntryChanged(source_path.to_path_buf()));
    }
    Ok((total, hex_digest(hasher.finalize().as_slice())))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn validate_declared_paths(
    package: &PackageSpec,
    inventory: &PackageInventory,
) -> Result<(), StageError> {
    let project = portable_relative("project", &package.project)?;
    require_entry(inventory, &project, EntryKind::File, "project file")?;
    for directory in &package.required_directories {
        let path = portable_relative("required directory", directory)?;
        require_entry(inventory, &path, EntryKind::Directory, "directory")?;
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn require_entry(
    inventory: &PackageInventory,
    path: &str,
    kind: EntryKind,
    expected: &'static str,
) -> Result<(), StageError> {
    if inventory
        .entries
        .iter()
        .any(|entry| entry.path == path && entry.kind == kind)
    {
        Ok(())
    } else {
        Err(StageError::MissingDeclaredPath {
            expected,
            path: path.to_owned(),
        })
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Clone, Debug, Eq, PartialEq)]
struct FileIdentity {
    file_type: FileType,
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
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn identity(file: &File, path: &Path) -> Result<FileIdentity, StageError> {
    let stat =
        fstat(file).map_err(|error| descriptor_io("inspect opened package entry", path, error))?;
    identity_from_stat(&stat, path)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[allow(
    clippy::unnecessary_cast,
    reason = "rustix Stat field widths differ between supported Unix targets"
)]
fn identity_from_stat(stat: &rustix::fs::Stat, path: &Path) -> Result<FileIdentity, StageError> {
    let size =
        u64::try_from(stat.st_size).map_err(|_| StageError::EntryChanged(path.to_path_buf()))?;
    Ok(FileIdentity {
        file_type: FileType::from_raw_mode(stat.st_mode),
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
    })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn require_same_open_entry(
    observed: &rustix::fs::Stat,
    opened: &File,
    path: &Path,
) -> Result<(), StageError> {
    if identity_from_stat(observed, path)? != identity(opened, path)? {
        return Err(StageError::EntryChanged(path.to_path_buf()));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn open_directory(path: &Path, operation: &'static str) -> Result<File, StageError> {
    open(path, directory_flags(), Mode::empty())
        .map(File::from)
        .map_err(|error| descriptor_io(operation, path, error))
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
fn make_private(file: &File, path: &Path, permissions: u16) -> Result<(), StageError> {
    fchmod(file, Mode::from_bits_retain(permissions))
        .map_err(|error| descriptor_io("set private staging permissions", path, error))?;
    let identity = identity(file, path)?;
    if identity.owner != rustix::process::geteuid().as_raw()
        || identity.mode & 0o777 != u32::from(permissions)
    {
        return Err(StageError::InsecureDestination(path.to_path_buf()));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn descriptor_io(operation: &'static str, path: &Path, error: rustix::io::Errno) -> StageError {
    StageError::Io {
        operation,
        path: path.to_path_buf(),
        source: error.into(),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
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
    use crate::package::inventory_package;
    use std::fs;
    use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};

    fn package(root: &Path) -> PackageSpec {
        PackageSpec {
            root: root.to_path_buf(),
            project: PathBuf::from("character.spine"),
            required_directories: vec![PathBuf::from("images"), PathBuf::from("audio")],
            asset_roots: vec![PathBuf::from("images"), PathBuf::from("audio")],
        }
    }

    fn fixture() -> tempfile::TempDir {
        let directory = tempfile::tempdir().expect("fixture root");
        let source = directory.path().join("source");
        fs::create_dir(&source).expect("source");
        fs::create_dir(source.join("images")).expect("images");
        fs::create_dir(source.join("audio")).expect("empty audio");
        fs::create_dir(source.join("images/nested")).expect("nested images");
        fs::write(source.join("character.spine"), b"immutable project").expect("project");
        fs::write(source.join("images/nested/page.png"), b"page bytes").expect("page");
        directory
    }

    #[test]
    fn stages_a_deterministic_private_writable_copy_without_touching_source() {
        let fixture = fixture();
        let source = fixture.path().join("source");
        let source_project = source.join("character.spine");
        fs::set_permissions(&source_project, fs::Permissions::from_mode(0o400))
            .expect("read-only source project");
        let destination = fixture.path().join("stage");

        let staged = stage_package(&package(&source), &destination).expect("secure stage");
        let expected_root = destination
            .parent()
            .expect("destination parent")
            .canonicalize()
            .expect("canonical destination parent")
            .join("stage");
        assert_eq!(staged.root(), expected_root);
        assert_eq!(staged.project(), expected_root.join("character.spine"));
        assert_eq!(staged.source_before(), staged.staged());
        assert_eq!(staged.source_before(), staged.source_after());
        assert_eq!(
            staged.source_before(),
            &inventory_package(&source).expect("legacy inventory agrees")
        );
        assert!(destination.join("audio").is_dir());
        assert_eq!(
            fs::read(destination.join("images/nested/page.png")).expect("staged page"),
            b"page bytes"
        );
        assert_eq!(
            fs::metadata(&destination).expect("stage metadata").mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(staged.project())
                .expect("project metadata")
                .mode()
                & 0o777,
            0o600
        );

        fs::write(staged.project(), b"candidate mutation").expect("stage is writable");
        assert_eq!(
            fs::read(&source_project).expect("source remains readable"),
            b"immutable project"
        );
        assert_eq!(
            fs::metadata(source_project)
                .expect("source metadata")
                .mode()
                & 0o777,
            0o400
        );
    }

    #[test]
    fn final_secure_inventory_detects_source_mutation_after_staging() {
        let fixture = fixture();
        let source = fixture.path().join("source");
        let staged =
            stage_package(&package(&source), fixture.path().join("stage")).expect("stage package");
        assert_eq!(
            staged
                .verify_source_unchanged()
                .expect("unchanged final inventory"),
            *staged.source_before()
        );
        fs::write(source.join("character.spine"), b"mutated source")
            .expect("mutate source after staging");
        assert!(matches!(
            staged.verify_source_unchanged(),
            Err(StageError::SourceChanged)
        ));
    }

    #[test]
    fn public_secure_inventory_uses_the_descriptor_relative_contract() {
        let fixture = fixture();
        let source = fixture.path().join("source");
        let inventory =
            secure_inventory_package(&package(&source)).expect("secure package inventory");
        assert!(
            inventory
                .entries
                .iter()
                .any(|entry| entry.path == "character.spine")
        );
    }

    #[test]
    fn independent_stages_have_identical_evidence() {
        let fixture = fixture();
        let source = fixture.path().join("source");
        let first = stage_package(&package(&source), fixture.path().join("first")).expect("first");
        let second =
            stage_package(&package(&source), fixture.path().join("second")).expect("second");
        assert_eq!(first.staged(), second.staged());
        assert_eq!(first.source_before(), second.source_before());
    }

    #[test]
    fn rejects_existing_or_source_alias_destinations() {
        let fixture = fixture();
        let source = fixture.path().join("source");
        fs::create_dir(fixture.path().join("existing")).expect("existing destination");
        assert!(matches!(
            stage_package(&package(&source), fixture.path().join("existing")),
            Err(StageError::DestinationExists(_))
        ));
        assert!(matches!(
            stage_package(&package(&source), source.join("nested-stage")),
            Err(StageError::DestinationAliasesSource(_))
        ));
        assert!(matches!(
            stage_package(&package(&source), &source),
            Err(StageError::DestinationAliasesSource(_))
        ));
    }

    #[test]
    fn resolves_destination_parent_aliases_before_alias_checks() {
        let fixture = fixture();
        let source = fixture.path().join("source");
        symlink(&source, fixture.path().join("source-alias")).expect("source alias");
        assert!(matches!(
            stage_package(&package(&source), fixture.path().join("source-alias/stage")),
            Err(StageError::DestinationAliasesSource(_))
        ));
    }

    #[test]
    fn rejects_root_and_nested_symbolic_links() {
        let fixture = fixture();
        let source = fixture.path().join("source");
        symlink(
            source.join("character.spine"),
            source.join("images/project-link"),
        )
        .expect("nested symlink");
        assert!(matches!(
            stage_package(&package(&source), fixture.path().join("stage")),
            Err(StageError::Symlink(_))
        ));

        let other = fixture.path().join("other");
        symlink(&source, &other).expect("root symlink");
        assert!(matches!(
            stage_package(&package(&other), fixture.path().join("other-stage")),
            Err(StageError::Symlink(_))
        ));
    }

    #[test]
    fn rejects_special_files() {
        use std::os::unix::net::UnixListener;

        let fixture = fixture();
        let source = fixture.path().join("source");
        let _listener = match UnixListener::bind(source.join("socket")) {
            Ok(listener) => listener,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("unix socket: {error}"),
        };
        assert!(matches!(
            stage_package(&package(&source), fixture.path().join("stage")),
            Err(StageError::UnsupportedFileType(_))
        ));
    }

    #[test]
    fn rejects_an_oversized_file_before_reading_or_copying_it() {
        let fixture = fixture();
        let source = fixture.path().join("source");
        let oversized = File::create(source.join("images/oversized.bin"))
            .expect("create sparse oversized fixture");
        oversized
            .set_len(MAX_FILE_BYTES + 1)
            .expect("size sparse oversized fixture");
        assert!(matches!(
            stage_package(&package(&source), fixture.path().join("stage")),
            Err(StageError::TreeLimit(path)) if path.ends_with("images/oversized.bin")
        ));
        assert!(!fixture.path().join("stage/images/oversized.bin").exists());
    }

    #[test]
    fn rejects_unsafe_declared_paths() {
        let fixture = fixture();
        let source = fixture.path().join("source");
        for (index, project) in [
            PathBuf::from("../character.spine"),
            PathBuf::from("/character.spine"),
            PathBuf::from("folder\\character.spine"),
            PathBuf::from("folder//character.spine"),
            PathBuf::from("./character.spine"),
        ]
        .into_iter()
        .enumerate()
        {
            let mut package = package(&source);
            package.project = project;
            assert!(matches!(
                stage_package(&package, fixture.path().join(format!("stage-{index}"))),
                Err(StageError::InvalidPath { .. })
            ));
        }
    }

    #[test]
    fn detects_mutation_between_initial_inventory_and_copy() {
        let fixture = fixture();
        let source = fixture.path().join("source");
        let project = source.join("character.spine");
        let error =
            stage_package_with_hook(&package(&source), &fixture.path().join("stage"), || {
                fs::write(&project, b"changed project").expect("mutate source fixture")
            })
            .expect_err("source mutation must fail");
        assert!(matches!(error, StageError::SourceChangedDuringCopy));
    }

    #[test]
    fn rejects_missing_declared_project_or_empty_directory() {
        let fixture = fixture();
        let source = fixture.path().join("source");
        fs::remove_file(source.join("character.spine")).expect("remove project");
        assert!(matches!(
            stage_package(&package(&source), fixture.path().join("missing-project")),
            Err(StageError::MissingDeclaredPath {
                expected: "project file",
                ..
            })
        ));

        fs::write(source.join("character.spine"), b"project").expect("restore project");
        fs::remove_dir(source.join("audio")).expect("remove empty audio directory");
        assert!(matches!(
            stage_package(&package(&source), fixture.path().join("missing-directory")),
            Err(StageError::MissingDeclaredPath {
                expected: "directory",
                ..
            })
        ));
    }
}

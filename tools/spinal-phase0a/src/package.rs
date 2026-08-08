#[cfg(test)]
use crate::digest::hex_digest;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::fs::{self, File, Metadata};
use std::io;
#[cfg(test)]
use std::io::Read;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use thiserror::Error;

#[cfg(test)]
const TREE_DIGEST_DOMAIN: &[u8] = b"spinal-phase0a-package-tree-v1\0";

/// The filesystem kind of an inventoried package entry.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    /// A directory, including an empty directory.
    Directory,
    /// A regular file whose bytes were hashed.
    File,
}

/// One deterministic entry in a package inventory.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TreeEntry {
    /// UTF-8 package-relative path using `/`, or `.` for the package root.
    pub path: String,
    /// Whether this entry is a regular file or directory.
    pub kind: EntryKind,
    /// File size in bytes. Directories use zero.
    pub size: u64,
    /// Lowercase content SHA-256 for files. Directories have no content digest.
    pub sha256: Option<String>,
}

/// Directory-aware, content-addressed inventory of one complete package.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageInventory {
    /// Lowercase digest of the framed, sorted entry sequence.
    pub tree_sha256: String,
    /// Sorted entries, including the root and every empty directory.
    pub entries: Vec<TreeEntry>,
}

/// Inventories for all package roles declared by a case.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CasePackageInventories {
    /// Current full project package.
    pub current: PackageInventory,
    /// Existing-animation submission package.
    pub replacement_submission: PackageInventory,
    /// New-animation submission package.
    pub new_submission: PackageInventory,
}

/// Failures that prevent a package from becoming trustworthy evidence.
#[derive(Debug, Error)]
pub enum PackageEvidenceError {
    /// A filesystem operation failed.
    #[error("could not {operation} `{path}`: {source}")]
    Io {
        /// Short operation description.
        operation: &'static str,
        /// Path involved in the failure.
        path: PathBuf,
        /// Underlying filesystem failure.
        #[source]
        source: io::Error,
    },
    /// The requested package root was not a directory.
    #[error("package root is not a directory: `{0}`")]
    RootNotDirectory(PathBuf),
    /// Symbolic links are forbidden anywhere in an evidence package.
    #[error("symbolic links are forbidden in evidence packages: `{0}`")]
    Symlink(PathBuf),
    /// A socket, device, pipe, or other non-portable entry was present.
    #[error("unsupported filesystem entry in evidence package: `{0}`")]
    UnsupportedFileType(PathBuf),
    /// Package entry names must be portable UTF-8.
    #[error("package contains a non-UTF-8 entry beneath `{0}`")]
    NonUtf8Name(PathBuf),
    /// A file changed while its evidence digest was being calculated.
    #[error("package file changed while it was being inventoried: `{0}`")]
    ChangedDuringRead(PathBuf),
    /// A path required by the case manifest was absent.
    #[error("{package} package is missing required {expected} `{path}`")]
    MissingDeclaredPath {
        /// Package role from the case schema.
        package: &'static str,
        /// Expected entry kind.
        expected: &'static str,
        /// Package-relative path.
        path: String,
    },
    /// A declared path existed with the wrong filesystem kind.
    #[error("{package} package path `{path}` must be a {expected}")]
    WrongDeclaredKind {
        /// Package role from the case schema.
        package: &'static str,
        /// Expected entry kind.
        expected: &'static str,
        /// Package-relative path.
        path: String,
    },
    /// A validated package-relative path could not be represented portably.
    #[error("{package} package contains an invalid declared path `{path}`")]
    InvalidDeclaredPath {
        /// Package role from the case schema.
        package: &'static str,
        /// Path that could not be represented without loss.
        path: PathBuf,
    },
}

/// Inventories one quiescent package and rejects encountered symbolic links.
///
/// The tree digest includes explicit directory records, so adding or removing
/// an empty asset root changes the result.
#[cfg(test)]
pub(crate) fn inventory_package(
    root: impl AsRef<Path>,
) -> Result<PackageInventory, PackageEvidenceError> {
    let root = root.as_ref();
    let root_metadata = metadata(root)?;
    reject_disallowed_root(root, &root_metadata)?;

    let mut entries = vec![TreeEntry {
        path: ".".to_owned(),
        kind: EntryKind::Directory,
        size: 0,
        sha256: None,
    }];
    let mut pending = vec![(root.to_path_buf(), String::new())];

    while let Some((directory, relative_parent)) = pending.pop() {
        let read_dir = fs::read_dir(&directory).map_err(|source| PackageEvidenceError::Io {
            operation: "read directory",
            path: directory.clone(),
            source,
        })?;
        let mut children = Vec::new();
        for child in read_dir {
            let child = child.map_err(|source| PackageEvidenceError::Io {
                operation: "read directory entry",
                path: directory.clone(),
                source,
            })?;
            let name = child
                .file_name()
                .into_string()
                .map_err(|_| PackageEvidenceError::NonUtf8Name(directory.clone()))?;
            if name.contains('\\') {
                return Err(PackageEvidenceError::NonUtf8Name(directory.clone()));
            }
            children.push((name, child.path()));
        }
        children.sort_by(|left, right| left.0.cmp(&right.0));

        for (name, path) in children {
            let relative = if relative_parent.is_empty() {
                name
            } else {
                format!("{relative_parent}/{name}")
            };
            let metadata = metadata(&path)?;
            let file_type = metadata.file_type();
            if file_type.is_symlink() {
                return Err(PackageEvidenceError::Symlink(path));
            }
            if file_type.is_dir() {
                entries.push(TreeEntry {
                    path: relative.clone(),
                    kind: EntryKind::Directory,
                    size: 0,
                    sha256: None,
                });
                pending.push((path, relative));
            } else if file_type.is_file() {
                let (size, sha256) = hash_file(&path, &metadata)?;
                entries.push(TreeEntry {
                    path: relative,
                    kind: EntryKind::File,
                    size,
                    sha256: Some(sha256),
                });
            } else {
                return Err(PackageEvidenceError::UnsupportedFileType(path));
            }
        }
    }

    entries.sort_by(|left, right| left.path.cmp(&right.path));
    let tree_sha256 = digest_tree(&entries);
    Ok(PackageInventory {
        tree_sha256,
        entries,
    })
}

#[cfg(test)]
fn metadata(path: &Path) -> Result<Metadata, PackageEvidenceError> {
    fs::symlink_metadata(path).map_err(|source| PackageEvidenceError::Io {
        operation: "read metadata for",
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
fn reject_disallowed_root(root: &Path, metadata: &Metadata) -> Result<(), PackageEvidenceError> {
    if metadata.file_type().is_symlink() {
        return Err(PackageEvidenceError::Symlink(root.to_path_buf()));
    }
    if !metadata.file_type().is_dir() {
        return Err(PackageEvidenceError::RootNotDirectory(root.to_path_buf()));
    }
    Ok(())
}

#[cfg(test)]
fn hash_file(path: &Path, before: &Metadata) -> Result<(u64, String), PackageEvidenceError> {
    let mut file = File::open(path).map_err(|source| PackageEvidenceError::Io {
        operation: "open file",
        path: path.to_path_buf(),
        source,
    })?;
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|source| PackageEvidenceError::Io {
                operation: "read file",
                path: path.to_path_buf(),
                source,
            })?;
        if count == 0 {
            break;
        }
        size = size
            .checked_add(count as u64)
            .ok_or_else(|| PackageEvidenceError::ChangedDuringRead(path.to_path_buf()))?;
        hasher.update(&buffer[..count]);
    }

    let after = metadata(path)?;
    if !after.file_type().is_file() || before.len() != after.len() || after.len() != size {
        return Err(PackageEvidenceError::ChangedDuringRead(path.to_path_buf()));
    }
    Ok((size, hex_digest(hasher.finalize().as_slice())))
}

#[cfg(test)]
fn digest_tree(entries: &[TreeEntry]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(TREE_DIGEST_DOMAIN);
    hasher.update((entries.len() as u64).to_be_bytes());
    for entry in entries {
        let kind = match entry.kind {
            EntryKind::Directory => b'd',
            EntryKind::File => b'f',
        };
        hasher.update([kind]);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn inventory_is_deterministic_and_directory_aware() {
        let first = tempfile::tempdir().expect("first package");
        fs::create_dir(first.path().join("images")).expect("empty asset root");
        fs::write(first.path().join("character.spine"), b"project").expect("project");

        let second = tempfile::tempdir().expect("second package");
        fs::write(second.path().join("character.spine"), b"project").expect("project");
        fs::create_dir(second.path().join("images")).expect("empty asset root");

        let first_inventory = inventory_package(first.path()).expect("first inventory");
        let second_inventory = inventory_package(second.path()).expect("second inventory");
        assert_eq!(first_inventory, second_inventory);
        assert!(
            first_inventory
                .entries
                .iter()
                .any(|entry| { entry.path == "images" && entry.kind == EntryKind::Directory })
        );

        let without_empty_directory = tempfile::tempdir().expect("third package");
        fs::write(
            without_empty_directory.path().join("character.spine"),
            b"project",
        )
        .expect("project");
        let third_inventory =
            inventory_package(without_empty_directory.path()).expect("third inventory");
        assert_ne!(first_inventory.tree_sha256, third_inventory.tree_sha256);
    }

    #[test]
    fn file_bytes_change_the_tree_digest() {
        let package = tempfile::tempdir().expect("package");
        let project = package.path().join("character.spine");
        fs::write(&project, b"first").expect("first project");
        let first = inventory_package(package.path()).expect("first inventory");

        fs::write(&project, b"second").expect("second project");
        let second = inventory_package(package.path()).expect("second inventory");
        assert_ne!(first.tree_sha256, second.tree_sha256);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symbolic_links() {
        use std::os::unix::fs::symlink;

        let package = tempfile::tempdir().expect("package");
        fs::write(package.path().join("target"), b"target").expect("target");
        symlink("target", package.path().join("alias")).expect("symlink");

        assert!(matches!(
            inventory_package(package.path()),
            Err(PackageEvidenceError::Symlink(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_special_files() {
        use std::os::unix::net::UnixListener;

        let package = tempfile::tempdir().expect("package");
        let _listener = match UnixListener::bind(package.path().join("socket")) {
            Ok(listener) => listener,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("unix socket: {error}"),
        };
        assert!(matches!(
            inventory_package(package.path()),
            Err(PackageEvidenceError::UnsupportedFileType(_))
        ));
    }
}

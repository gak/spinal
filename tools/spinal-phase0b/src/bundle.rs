//! Case-relative acquisition of immutable Current and Proposed runtime bundles.
//!
//! This loader consumes the runtime-manifest bytes already authenticated by
//! [`crate::load_case`]. It never reopens those manifest files. Runtime file
//! locations are resolved beneath the directory containing each manifest, and
//! exact bytes are retained before the shared Spinal validator runs.
//!
//! Like the surrounding Phase 0B case loader, this is a local-trusted boundary:
//! the owner-private case tree must remain quiescent while acquisition runs. It
//! detects ordinary changes, links, aliases, and unsafe paths, but deliberately
//! does not claim descriptor-relative resistance to a hostile concurrent writer.

use std::{
    collections::BTreeMap,
    fs::{self, File, Metadata},
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

use sha2::{Digest, Sha256};
use spinal::{
    MAX_RUNTIME_BUNDLE_BYTES, RuntimeBundleError, RuntimeBundleManifest, ValidatedRuntimeBundle,
};
use thiserror::Error;

use crate::spec::{LoadedCase, RuntimeManifestInput};

/// Identifies one side of the fixed Phase 0B comparison.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaseBundleSide {
    /// The authenticated Current runtime export.
    Current,
    /// The authenticated Proposed runtime export.
    Proposed,
}

impl std::fmt::Display for CaseBundleSide {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Current => "Current",
            Self::Proposed => "Proposed",
        })
    }
}

/// Two independently acquired and validated immutable runtime bundles.
#[derive(Clone, Debug)]
pub struct LoadedCaseRuntimeBundles {
    current: ValidatedRuntimeBundle,
    proposed: ValidatedRuntimeBundle,
}

impl LoadedCaseRuntimeBundles {
    /// Returns the validated Current bundle.
    #[must_use]
    pub const fn current(&self) -> &ValidatedRuntimeBundle {
        &self.current
    }

    /// Returns the validated Proposed bundle.
    #[must_use]
    pub const fn proposed(&self) -> &ValidatedRuntimeBundle {
        &self.proposed
    }

    /// Consumes the pair without mixing either side's virtual file map.
    #[must_use]
    pub fn into_parts(self) -> (ValidatedRuntimeBundle, ValidatedRuntimeBundle) {
        (self.current, self.proposed)
    }
}

/// Failure while acquiring runtime files from an authenticated Phase 0B case.
#[derive(Debug, Error)]
pub enum CaseBundleLoadError {
    /// One or both runtime-manifest evidence slots were absent.
    #[error("the case has no authenticated Current and Proposed runtime manifests")]
    MissingAuthenticatedManifests,
    /// The owner-private case directory was missing, linked, or not a directory.
    #[error("the Phase 0B case directory is not a regular non-symlink directory")]
    InvalidCaseDirectory,
    /// An authenticated runtime manifest failed the shared strict parser.
    #[error("invalid {bundle} runtime manifest: {source}")]
    InvalidManifest {
        /// Current or Proposed.
        bundle: CaseBundleSide,
        /// Shared manifest failure.
        #[source]
        source: RuntimeBundleError,
    },
    /// A declared runtime location did not stay beneath its manifest directory.
    #[error("unsafe {bundle} runtime location `{location}`: {reason}")]
    UnsafeLocation {
        /// Current or Proposed.
        bundle: CaseBundleSide,
        /// Manifest-declared location, never an absolute host path.
        location: Box<str>,
        /// Stable rejection reason.
        reason: &'static str,
    },
    /// A declared runtime file could not be inspected, opened, or read.
    #[error("could not {operation} {bundle} runtime location `{location}`: {source}")]
    FileIo {
        /// Current or Proposed.
        bundle: CaseBundleSide,
        /// Manifest-declared location, never an absolute host path.
        location: Box<str>,
        /// Stable filesystem operation.
        operation: &'static str,
        /// Host I/O failure.
        #[source]
        source: io::Error,
    },
    /// A location resolved to a directory, link, socket, device, or another special entry.
    #[error("{bundle} runtime location `{location}` is not a regular non-symlink file")]
    UnsupportedFileType {
        /// Current or Proposed.
        bundle: CaseBundleSide,
        /// Manifest-declared location.
        location: Box<str>,
    },
    /// A runtime file exceeded the role-specific encoded-byte bound.
    #[error("{bundle} runtime file `{path}` has {actual} bytes; limit is {limit}")]
    FileByteLimit {
        /// Current or Proposed.
        bundle: CaseBundleSide,
        /// Normalized virtual bundle path.
        path: PathBuf,
        /// Observed encoded length.
        actual: usize,
        /// Fixed role-specific bound.
        limit: usize,
    },
    /// A runtime file differed from its manifest-declared encoded length.
    #[error("{bundle} runtime file `{path}` has {actual} bytes; expected {expected}")]
    FileLengthMismatch {
        /// Current or Proposed.
        bundle: CaseBundleSide,
        /// Normalized virtual bundle path.
        path: PathBuf,
        /// Manifest-declared length.
        expected: usize,
        /// Observed length.
        actual: usize,
    },
    /// The observed runtime-file total exceeded the fixed per-bundle bound.
    #[error("{bundle} runtime files exceed the {MAX_RUNTIME_BUNDLE_BYTES}-byte aggregate limit")]
    AggregateByteLimit {
        /// Current or Proposed.
        bundle: CaseBundleSide,
    },
    /// A runtime file changed identity between inspection and its single open.
    #[error("{bundle} runtime location `{location}` changed while it was opened")]
    FileChanged {
        /// Current or Proposed.
        bundle: CaseBundleSide,
        /// Manifest-declared location.
        location: Box<str>,
    },
    /// A runtime file did not match its manifest-declared SHA-256.
    #[error("{bundle} runtime file `{path}` failed its SHA-256 check")]
    FileDigestMismatch {
        /// Current or Proposed.
        bundle: CaseBundleSide,
        /// Normalized virtual bundle path.
        path: PathBuf,
    },
    /// Two declared locations physically or lexically aliased one file.
    #[error("{bundle} runtime locations `{first}` and `{second}` alias the same file")]
    PhysicalAlias {
        /// Current or Proposed. Cross-side aliases are attributed to Proposed.
        bundle: CaseBundleSide,
        /// First case-relative location.
        first: PathBuf,
        /// Second case-relative location.
        second: PathBuf,
    },
    /// Exact acquired bytes failed shared runtime, texture, or file-set validation.
    #[error("invalid {bundle} runtime bundle: {source}")]
    InvalidBundle {
        /// Current or Proposed.
        bundle: CaseBundleSide,
        /// Shared validation failure.
        #[source]
        source: RuntimeBundleError,
    },
}

/// Acquires and validates both runtime bundles referenced by a loaded case.
///
/// Manifest bytes come only from [`LoadedCase`]'s authenticated immutable
/// snapshot. Runtime files are opened once, read through exact bounded reads,
/// digest-checked, and then passed to [`RuntimeBundleManifest::validate`]. This
/// function produces inputs for later rehearsal machinery only; it records no
/// run, result, report, or gate state.
pub fn load_case_runtime_bundles(
    case: &LoadedCase,
) -> Result<LoadedCaseRuntimeBundles, CaseBundleLoadError> {
    let inputs = case
        .runtime_manifest_inputs()
        .ok_or(CaseBundleLoadError::MissingAuthenticatedManifests)?;
    validate_case_directory(inputs.current.case_directory())?;
    if inputs.current.case_directory() != inputs.proposed.case_directory() {
        return Err(CaseBundleLoadError::InvalidCaseDirectory);
    }

    let protected_artifacts = case.authenticated_artifact_paths().collect::<Vec<_>>();
    let current = acquire_bundle(
        CaseBundleSide::Current,
        inputs.current,
        &protected_artifacts,
        None,
    )?;
    let proposed = acquire_bundle(
        CaseBundleSide::Proposed,
        inputs.proposed,
        &protected_artifacts,
        Some(&current.identities),
    )?;

    Ok(LoadedCaseRuntimeBundles {
        current: current.validated,
        proposed: proposed.validated,
    })
}

struct AcquiredBundle {
    validated: ValidatedRuntimeBundle,
    identities: BTreeMap<PhysicalIdentity, PathBuf>,
}

fn acquire_bundle(
    bundle: CaseBundleSide,
    input: RuntimeManifestInput<'_>,
    protected_artifacts: &[&Path],
    other_bundle_identities: Option<&BTreeMap<PhysicalIdentity, PathBuf>>,
) -> Result<AcquiredBundle, CaseBundleLoadError> {
    let manifest = RuntimeBundleManifest::parse(input.manifest_bytes())
        .map_err(|source| CaseBundleLoadError::InvalidManifest { bundle, source })?;
    let mut files = BTreeMap::<PathBuf, Vec<u8>>::new();
    let mut identities = BTreeMap::<PhysicalIdentity, PathBuf>::new();
    let mut total_bytes = 0_usize;

    for declaration in manifest.files() {
        let case_relative =
            case_relative_location(bundle, input, declaration.location_reference())?;
        if protected_artifacts.contains(&case_relative.as_path()) {
            return Err(CaseBundleLoadError::UnsafeLocation {
                bundle,
                location: declaration.location_reference().into(),
                reason: "runtime files cannot alias an authenticated case artifact",
            });
        }
        let opened = open_regular_file(
            bundle,
            input.case_directory(),
            &case_relative,
            declaration.location_reference(),
            other_bundle_identities,
        )?;
        if opened.length > declaration.max_bytes() {
            return Err(CaseBundleLoadError::FileByteLimit {
                bundle,
                path: declaration.virtual_path().to_path_buf(),
                actual: opened.length,
                limit: declaration.max_bytes(),
            });
        }
        if opened.length != declaration.expected_bytes() {
            return Err(CaseBundleLoadError::FileLengthMismatch {
                bundle,
                path: declaration.virtual_path().to_path_buf(),
                expected: declaration.expected_bytes(),
                actual: opened.length,
            });
        }
        total_bytes = total_bytes
            .checked_add(opened.length)
            .ok_or(CaseBundleLoadError::AggregateByteLimit { bundle })?;
        if total_bytes > MAX_RUNTIME_BUNDLE_BYTES {
            return Err(CaseBundleLoadError::AggregateByteLimit { bundle });
        }
        if let Some(first) = identities.insert(opened.identity, case_relative.clone()) {
            return Err(CaseBundleLoadError::PhysicalAlias {
                bundle,
                first,
                second: case_relative,
            });
        }

        let bytes = read_exact_bounded(
            bundle,
            declaration.location_reference(),
            declaration.virtual_path(),
            opened.file,
            declaration.expected_bytes(),
        )?;
        if sha256_hex(&bytes) != declaration.expected_sha256() {
            return Err(CaseBundleLoadError::FileDigestMismatch {
                bundle,
                path: declaration.virtual_path().to_path_buf(),
            });
        }
        if files
            .insert(declaration.virtual_path().to_path_buf(), bytes)
            .is_some()
        {
            return Err(CaseBundleLoadError::InvalidBundle {
                bundle,
                source: RuntimeBundleError::FileSetMismatch,
            });
        }
    }
    if total_bytes != manifest.encoded_bytes() {
        return Err(CaseBundleLoadError::AggregateByteLimit { bundle });
    }

    let validated = manifest
        .validate(files)
        .map_err(|source| CaseBundleLoadError::InvalidBundle { bundle, source })?;
    Ok(AcquiredBundle {
        validated,
        identities,
    })
}

fn validate_case_directory(case_directory: &Path) -> Result<(), CaseBundleLoadError> {
    let metadata = fs::symlink_metadata(case_directory)
        .map_err(|_source| CaseBundleLoadError::InvalidCaseDirectory)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(CaseBundleLoadError::InvalidCaseDirectory);
    }
    Ok(())
}

fn case_relative_location(
    bundle: CaseBundleSide,
    input: RuntimeManifestInput<'_>,
    location: &str,
) -> Result<PathBuf, CaseBundleLoadError> {
    let parent = input
        .manifest_relative_path()
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let result = parent.join(location);
    if result
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
    {
        Ok(result)
    } else {
        Err(CaseBundleLoadError::UnsafeLocation {
            bundle,
            location: location.into(),
            reason: "the resolved case-relative path is not normalized",
        })
    }
}

struct OpenedFile {
    file: File,
    length: usize,
    identity: PhysicalIdentity,
}

fn open_regular_file(
    bundle: CaseBundleSide,
    root: &Path,
    case_relative: &Path,
    location: &str,
    other_bundle_identities: Option<&BTreeMap<PhysicalIdentity, PathBuf>>,
) -> Result<OpenedFile, CaseBundleLoadError> {
    let mut resolved = root.to_path_buf();
    let components = case_relative.components().collect::<Vec<_>>();
    if components.is_empty() {
        return Err(CaseBundleLoadError::UnsafeLocation {
            bundle,
            location: location.into(),
            reason: "the resolved location does not name a file",
        });
    }
    let mut inspected = None;
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(component) = component else {
            return Err(CaseBundleLoadError::UnsafeLocation {
                bundle,
                location: location.into(),
                reason: "the resolved location is not a normalized relative path",
            });
        };
        resolved.push(component);
        let metadata =
            fs::symlink_metadata(&resolved).map_err(|source| CaseBundleLoadError::FileIo {
                bundle,
                location: location.into(),
                operation: "inspect",
                source,
            })?;
        if metadata.file_type().is_symlink() {
            return Err(CaseBundleLoadError::UnsupportedFileType {
                bundle,
                location: location.into(),
            });
        }
        let final_component = index + 1 == components.len();
        if (!final_component && !metadata.is_dir()) || (final_component && !metadata.is_file()) {
            return Err(CaseBundleLoadError::UnsupportedFileType {
                bundle,
                location: location.into(),
            });
        }
        if final_component {
            inspected = Some(metadata);
        }
    }

    let inspected = inspected.expect("a nonempty path has one final component");
    let inspected_identity = physical_identity(&inspected, &resolved);
    reject_multiple_links(bundle, location, &inspected)?;
    if let Some(first) =
        other_bundle_identities.and_then(|identities| identities.get(&inspected_identity))
    {
        return Err(CaseBundleLoadError::PhysicalAlias {
            bundle,
            first: first.clone(),
            second: case_relative.to_path_buf(),
        });
    }
    let file = File::open(&resolved).map_err(|source| CaseBundleLoadError::FileIo {
        bundle,
        location: location.into(),
        operation: "open",
        source,
    })?;
    let opened = file
        .metadata()
        .map_err(|source| CaseBundleLoadError::FileIo {
            bundle,
            location: location.into(),
            operation: "inspect opened file",
            source,
        })?;
    if !opened.is_file() {
        return Err(CaseBundleLoadError::UnsupportedFileType {
            bundle,
            location: location.into(),
        });
    }
    reject_multiple_links(bundle, location, &opened)?;
    let identity = physical_identity(&opened, &resolved);
    if identity != inspected_identity {
        return Err(CaseBundleLoadError::FileChanged {
            bundle,
            location: location.into(),
        });
    }
    let length =
        usize::try_from(opened.len()).map_err(|_error| CaseBundleLoadError::FileByteLimit {
            bundle,
            path: case_relative.to_path_buf(),
            actual: usize::MAX,
            limit: MAX_RUNTIME_BUNDLE_BYTES,
        })?;
    Ok(OpenedFile {
        file,
        length,
        identity,
    })
}

fn read_exact_bounded(
    bundle: CaseBundleSide,
    location: &str,
    virtual_path: &Path,
    file: File,
    expected: usize,
) -> Result<Vec<u8>, CaseBundleLoadError> {
    let limit = expected
        .checked_add(1)
        .and_then(|value| u64::try_from(value).ok())
        .ok_or(CaseBundleLoadError::FileByteLimit {
            bundle,
            path: virtual_path.to_path_buf(),
            actual: expected,
            limit: expected,
        })?;
    let mut bytes = Vec::with_capacity(expected);
    file.take(limit)
        .read_to_end(&mut bytes)
        .map_err(|source| CaseBundleLoadError::FileIo {
            bundle,
            location: location.into(),
            operation: "read",
            source,
        })?;
    if bytes.len() != expected {
        return Err(CaseBundleLoadError::FileLengthMismatch {
            bundle,
            path: virtual_path.to_path_buf(),
            expected,
            actual: bytes.len(),
        });
    }
    Ok(bytes)
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum PhysicalIdentity {
    #[cfg(unix)]
    Unix { device: u64, inode: u64 },
    #[cfg(not(unix))]
    ResolvedPath(PathBuf),
}

#[cfg(unix)]
fn physical_identity(metadata: &Metadata, _resolved: &Path) -> PhysicalIdentity {
    use std::os::unix::fs::MetadataExt;

    PhysicalIdentity::Unix {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

#[cfg(not(unix))]
fn physical_identity(_metadata: &Metadata, resolved: &Path) -> PhysicalIdentity {
    PhysicalIdentity::ResolvedPath(resolved.to_path_buf())
}

#[cfg(unix)]
fn reject_multiple_links(
    bundle: CaseBundleSide,
    location: &str,
    metadata: &Metadata,
) -> Result<(), CaseBundleLoadError> {
    use std::os::unix::fs::MetadataExt;

    if metadata.nlink() == 1 {
        Ok(())
    } else {
        Err(CaseBundleLoadError::PhysicalAlias {
            bundle,
            first: PathBuf::from(location),
            second: PathBuf::from(location),
        })
    }
}

#[cfg(not(unix))]
fn reject_multiple_links(
    _bundle: CaseBundleSide,
    _location: &str,
    _metadata: &Metadata,
) -> Result<(), CaseBundleLoadError> {
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load_case;

    const CASE: &str = include_str!("../cases/generic-bevy-0.18.1.toml");
    const CURRENT_JSON: &[u8] = br#"{"skeleton":{"spine":"4.3.23"},"bones":[{"name":"root"}]}"#;
    const PROPOSED_JSON: &[u8] = br#"{"skeleton":{"spine":"4.3.23"},"bones":[{"name":"root","x":1}],"animations":{"sway":{}}}"#;
    const ATLAS: &[u8] = b"textures/page.png\n\tsize: 1, 1\n\tformat: RGBA8888\n\tfilter: Linear, Linear\n\trepeat: none\n\tpma: false\n";
    const PNG: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 96, 96, 248, 255, 31,
        0, 3, 2, 1, 255, 230, 119, 11, 174, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];

    struct Fixture {
        _directory: tempfile::TempDir,
        root: PathBuf,
        current_manifest: PathBuf,
        proposed_manifest: PathBuf,
        current_json: PathBuf,
        proposed_json: PathBuf,
        case_path: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = tempfile::tempdir().expect("temporary case directory");
            let root = directory.path().to_path_buf();
            let (current_manifest, current_json) = write_bundle(
                &root,
                "sources/current/manifests/runtime.json",
                "Current fixture",
                CURRENT_JSON,
            );
            let (proposed_manifest, proposed_json) = write_bundle(
                &root,
                "sources/proposed/manifests/runtime.json",
                "Proposed fixture",
                PROPOSED_JSON,
            );
            let case_path = root.join("case.toml");
            write_case(&case_path, &root, &current_manifest, &proposed_manifest);
            Self {
                _directory: directory,
                root,
                current_manifest,
                proposed_manifest,
                current_json,
                proposed_json,
                case_path,
            }
        }

        fn reload_case_manifest_authentication(&self) {
            write_case(
                &self.case_path,
                &self.root,
                &self.current_manifest,
                &self.proposed_manifest,
            );
        }
    }

    fn write_bundle(
        root: &Path,
        manifest_relative: &str,
        label: &str,
        json: &[u8],
    ) -> (PathBuf, PathBuf) {
        let manifest_path = root.join(manifest_relative);
        let manifest_directory = manifest_path
            .parent()
            .expect("manifest parent")
            .to_path_buf();
        let files = BTreeMap::from([
            (PathBuf::from("rig/fixture.json"), json.to_vec()),
            (PathBuf::from("rig/fixture.atlas"), ATLAS.to_vec()),
            (PathBuf::from("rig/textures/page.png"), PNG.to_vec()),
        ]);
        let manifest = RuntimeBundleManifest::build(
            label,
            Path::new("rig/fixture.json"),
            Path::new("rig/fixture.atlas"),
            files.clone(),
        )
        .expect("strict fixture bundle")
        .0;
        fs::create_dir_all(&manifest_directory).expect("manifest directory");
        fs::write(&manifest_path, manifest).expect("runtime manifest");
        for (relative, bytes) in files {
            let path = manifest_directory.join(relative);
            fs::create_dir_all(path.parent().expect("runtime parent")).expect("runtime directory");
            fs::write(path, bytes).expect("runtime source");
        }
        (manifest_path, manifest_directory.join("rig/fixture.json"))
    }

    fn write_case(case_path: &Path, root: &Path, current: &Path, proposed: &Path) {
        let current = current.strip_prefix(root).expect("current relative");
        let proposed = proposed.strip_prefix(root).expect("proposed relative");
        let mut case = CASE.to_owned();
        case = replace_slot(
            case,
            "runtime_manifest = { required = true }",
            current,
            &fs::read(root.join(current)).expect("current manifest bytes"),
        );
        case = replace_slot(
            case,
            "runtime_manifest = {required = true}",
            proposed,
            &fs::read(root.join(proposed)).expect("proposed manifest bytes"),
        );
        fs::write(case_path, case).expect("case manifest");
    }

    fn replace_slot(mut case: String, placeholder: &str, path: &Path, bytes: &[u8]) -> String {
        assert_eq!(case.matches(placeholder).count(), 1);
        let field = placeholder.split_once('=').expect("slot field").0;
        let path = path.to_str().expect("portable fixture path");
        let replacement = format!(
            "{field}= {{ required = true, path = \"{path}\", byte_length = {}, sha256 = \"{}\" }}",
            bytes.len(),
            sha256_hex(bytes)
        );
        case = case.replacen(placeholder, &replacement, 1);
        case
    }

    fn rewrite_manifest_url(path: &Path, from: &str, to: &str) {
        let bytes = fs::read(path).expect("manifest bytes");
        let text = String::from_utf8(bytes).expect("manifest UTF-8");
        assert_eq!(text.matches(from).count(), 1, "unique URL replacement");
        fs::write(path, text.replacen(from, to, 1)).expect("rewrite manifest");
    }

    #[test]
    fn loads_valid_nested_current_and_proposed_bundles_in_isolation() {
        let fixture = Fixture::new();
        let case = load_case(&fixture.case_path).expect("authenticated case");
        let bundles = load_case_runtime_bundles(&case).expect("validated runtime bundles");

        assert_eq!(bundles.current().label(), "Current fixture");
        assert_eq!(bundles.proposed().label(), "Proposed fixture");
        assert_eq!(bundles.current().json_bytes(), CURRENT_JSON);
        assert_eq!(bundles.proposed().json_bytes(), PROPOSED_JSON);
        assert_ne!(
            bundles.current().content_sha256(),
            bundles.proposed().content_sha256()
        );
    }

    #[test]
    fn retained_manifest_and_source_bytes_ignore_later_mutation() {
        let fixture = Fixture::new();
        let case = load_case(&fixture.case_path).expect("authenticated case");
        fs::write(
            &fixture.current_manifest,
            b"mutated manifest after load_case",
        )
        .expect("mutate authenticated manifest file");

        let bundles = load_case_runtime_bundles(&case).expect("retained manifest bytes");
        fs::write(&fixture.current_json, b"mutated source after bundle load")
            .expect("mutate runtime source");
        fs::write(&fixture.proposed_json, b"mutated proposed source")
            .expect("mutate proposed runtime source");
        fs::write(&fixture.proposed_manifest, b"mutated proposed manifest")
            .expect("mutate manifest source");

        assert_eq!(bundles.current().json_bytes(), CURRENT_JSON);
        assert_eq!(bundles.proposed().json_bytes(), PROPOSED_JSON);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_runtime_files_fail_closed() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        let case = load_case(&fixture.case_path).expect("authenticated case");
        let target = fixture.current_json.with_file_name("target.json");
        fs::rename(&fixture.current_json, &target).expect("move runtime source");
        symlink(&target, &fixture.current_json).expect("runtime symlink");

        assert!(matches!(
            load_case_runtime_bundles(&case),
            Err(CaseBundleLoadError::UnsupportedFileType {
                bundle: CaseBundleSide::Current,
                ..
            })
        ));
    }

    #[test]
    fn special_runtime_entries_fail_closed() {
        let fixture = Fixture::new();
        let case = load_case(&fixture.case_path).expect("authenticated case");
        fs::remove_file(&fixture.current_json).expect("remove runtime source");
        fs::create_dir(&fixture.current_json).expect("replace source with directory");

        assert!(matches!(
            load_case_runtime_bundles(&case),
            Err(CaseBundleLoadError::UnsupportedFileType {
                bundle: CaseBundleSide::Current,
                ..
            })
        ));
    }

    #[test]
    fn unsafe_escape_absolute_backslash_and_url_locations_fail_closed() {
        for unsafe_location in [
            "../fixture.json",
            "/fixture.json",
            r"rig\fixture.json",
            "https://example.invalid/fixture.json",
        ] {
            let fixture = Fixture::new();
            rewrite_manifest_url(
                &fixture.current_manifest,
                "\"url\":\"rig/fixture.json\"",
                &format!("\"url\":\"{unsafe_location}\""),
            );
            fixture.reload_case_manifest_authentication();
            let case = load_case(&fixture.case_path).expect("authenticated unsafe manifest");
            assert!(matches!(
                load_case_runtime_bundles(&case),
                Err(CaseBundleLoadError::InvalidManifest {
                    bundle: CaseBundleSide::Current,
                    ..
                })
            ));
        }
    }

    #[test]
    fn missing_length_and_digest_changes_fail_closed() {
        let missing = Fixture::new();
        let missing_case = load_case(&missing.case_path).expect("authenticated case");
        fs::remove_file(&missing.current_json).expect("remove source");
        assert!(load_case_runtime_bundles(&missing_case).is_err());

        let length = Fixture::new();
        let length_case = load_case(&length.case_path).expect("authenticated case");
        fs::write(&length.current_json, b"different length").expect("change source length");
        assert!(matches!(
            load_case_runtime_bundles(&length_case),
            Err(CaseBundleLoadError::FileLengthMismatch {
                bundle: CaseBundleSide::Current,
                ..
            })
        ));

        let digest = Fixture::new();
        let digest_case = load_case(&digest.case_path).expect("authenticated case");
        let replacement = vec![b'x'; CURRENT_JSON.len()];
        fs::write(&digest.current_json, replacement).expect("change source digest");
        assert!(matches!(
            load_case_runtime_bundles(&digest_case),
            Err(CaseBundleLoadError::FileDigestMismatch {
                bundle: CaseBundleSide::Current,
                ..
            })
        ));
    }

    #[test]
    fn actual_per_file_limit_is_checked_before_reading() {
        let fixture = Fixture::new();
        let case = load_case(&fixture.case_path).expect("authenticated case");
        let manifest = RuntimeBundleManifest::parse(
            &fs::read(&fixture.current_manifest).expect("runtime manifest"),
        )
        .expect("strict manifest");
        let json_limit = manifest
            .files()
            .iter()
            .find(|file| file.virtual_path() == Path::new("rig/fixture.json"))
            .expect("JSON declaration")
            .max_bytes();
        File::options()
            .write(true)
            .open(&fixture.current_json)
            .expect("open source for sparse resize")
            .set_len(u64::try_from(json_limit + 1).expect("limit fits u64"))
            .expect("oversize sparse source");

        assert!(matches!(
            load_case_runtime_bundles(&case),
            Err(CaseBundleLoadError::FileByteLimit {
                bundle: CaseBundleSide::Current,
                ..
            })
        ));
    }

    #[test]
    fn declared_aggregate_limit_fails_before_file_acquisition() {
        let fixture = Fixture::new();
        let digest = "0".repeat(64);
        let manifest = format!(
            "{{\"format_version\":1,\"source\":{{\"label\":\"Oversized fixture\",\"json\":\"rig/fixture.json\",\"atlas\":\"rig/fixture.atlas\",\"files\":[{{\"path\":\"rig/fixture.json\",\"url\":\"rig/fixture.json\",\"byte_length\":16777216,\"sha256\":\"{digest}\"}},{{\"path\":\"rig/fixture.atlas\",\"url\":\"rig/fixture.atlas\",\"byte_length\":2097152,\"sha256\":\"{digest}\"}},{{\"path\":\"rig/page-a.png\",\"url\":\"rig/page-a.png\",\"byte_length\":16777216,\"sha256\":\"{digest}\"}},{{\"path\":\"rig/page-b.png\",\"url\":\"rig/page-b.png\",\"byte_length\":16777216,\"sha256\":\"{digest}\"}},{{\"path\":\"rig/page-c.png\",\"url\":\"rig/page-c.png\",\"byte_length\":16777216,\"sha256\":\"{digest}\"}}]}}}}"
        );
        fs::write(&fixture.current_manifest, manifest).expect("oversized manifest");
        fixture.reload_case_manifest_authentication();
        let case = load_case(&fixture.case_path).expect("authenticated oversized manifest");

        assert!(matches!(
            load_case_runtime_bundles(&case),
            Err(CaseBundleLoadError::InvalidManifest {
                bundle: CaseBundleSide::Current,
                ..
            })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn physical_aliases_fail_closed() {
        let fixture = Fixture::new();
        let current_atlas = fixture
            .current_json
            .parent()
            .expect("rig directory")
            .join("fixture.atlas");
        fs::remove_file(&current_atlas).expect("remove atlas");
        fs::hard_link(&fixture.current_json, &current_atlas).expect("hard-link source alias");
        let case = load_case(&fixture.case_path).expect("authenticated case");

        assert!(matches!(
            load_case_runtime_bundles(&case),
            Err(CaseBundleLoadError::PhysicalAlias {
                bundle: CaseBundleSide::Current,
                ..
            })
        ));
    }

    #[test]
    fn runtime_files_cannot_alias_authenticated_oracle_artifacts() {
        let fixture = Fixture::new();
        let relative_json = fixture
            .current_json
            .strip_prefix(&fixture.root)
            .expect("case-relative runtime JSON");
        let case = fs::read_to_string(&fixture.case_path).expect("case source");
        let case = replace_slot(
            case,
            "method_document = { required=true }",
            relative_json,
            CURRENT_JSON,
        );
        fs::write(&fixture.case_path, case).expect("case with aliased oracle");
        let case = load_case(&fixture.case_path).expect("authenticated aliased artifact");

        assert!(matches!(
            load_case_runtime_bundles(&case),
            Err(CaseBundleLoadError::UnsafeLocation {
                bundle: CaseBundleSide::Current,
                reason: "runtime files cannot alias an authenticated case artifact",
                ..
            })
        ));
    }

    #[test]
    fn current_and_proposed_cannot_share_physical_runtime_sources() {
        let fixture = Fixture::new();
        let shared_proposed_manifest = fixture
            .current_manifest
            .with_file_name("proposed-runtime.json");
        fs::copy(&fixture.current_manifest, &shared_proposed_manifest)
            .expect("second authenticated manifest beside Current");
        write_case(
            &fixture.case_path,
            &fixture.root,
            &fixture.current_manifest,
            &shared_proposed_manifest,
        );
        let case = load_case(&fixture.case_path).expect("authenticated shared-source case");

        assert!(matches!(
            load_case_runtime_bundles(&case),
            Err(CaseBundleLoadError::PhysicalAlias {
                bundle: CaseBundleSide::Proposed,
                ..
            })
        ));
    }

    #[test]
    fn file_set_mismatch_fails_in_shared_validation() {
        let fixture = Fixture::new();
        let mut manifest =
            String::from_utf8(fs::read(&fixture.current_manifest).expect("current manifest bytes"))
                .expect("manifest UTF-8");
        let insertion = format!(
            ",{{\"path\":\"rig/extra.png\",\"url\":\"rig/extra.png\",\"byte_length\":{},\"sha256\":\"{}\"}}",
            PNG.len(),
            sha256_hex(PNG)
        );
        let boundary = manifest.rfind("]}}").expect("files array ending");
        manifest.insert_str(boundary, &insertion);
        fs::write(&fixture.current_manifest, manifest).expect("expanded manifest");
        let extra = fixture
            .current_json
            .parent()
            .expect("rig directory")
            .join("extra.png");
        fs::write(extra, PNG).expect("extra runtime file");
        fixture.reload_case_manifest_authentication();
        let case = load_case(&fixture.case_path).expect("authenticated case");

        assert!(matches!(
            load_case_runtime_bundles(&case),
            Err(CaseBundleLoadError::InvalidBundle {
                bundle: CaseBundleSide::Current,
                source: RuntimeBundleError::RuntimeFileSetMismatch,
            })
        ));
    }
}

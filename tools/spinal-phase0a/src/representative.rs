//! Strict admission binding for a representative Phase 0A evidence run.
//!
//! This module intentionally does not share the generic rehearsal case schema. A
//! representative binding is a small, owner-private declaration of the exact
//! case, harness executable, and three role-tagged package trees that an owner
//! selected for one evidence run. Loading validates the exact file bytes and
//! filesystem identity; matching those bytes to independently observed inputs
//! is the only way to obtain [`VerifiedRepresentativeEnvelope`].

use crate::digest::{is_sha256, sha256_bytes};
use serde::Deserialize;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::fs::{self, File, Metadata, OpenOptions};
use std::io;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::{Path, PathBuf};
use thiserror::Error;

const REPRESENTATIVE_FORMAT_VERSION: u32 = 1;
const REPRESENTATIVE_EVIDENCE_CLASS: &str = "phase0a_representative";
const BINDING_ID_PREFIX: &str = "rep-";
const BINDING_ID_HEX_LENGTH: usize = 32;
const BINDING_ID_DOMAIN: &[u8] = b"spinal-phase0a-representative-binding-id-v1\0";
const MAX_ENVELOPE_BYTES: u64 = 64 * 1024;

/// Strict, immutable contents of one representative evidence binding.
#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RepresentativeEnvelope {
    format_version: u32,
    evidence_class: String,
    binding_id: String,
    case_sha256: String,
    harness_executable_sha256: String,
    build: RepresentativeBuildIdentity,
    package_tree_sha256: RepresentativePackageTreeSha256,
}

/// Clean reviewed build context pinned by the representative owner.
#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct RepresentativeBuildIdentity {
    source_revision: String,
    cargo_lock_sha256: String,
}

/// Expected package-tree identities, labeled by their Phase 0A roles.
#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct RepresentativePackageTreeSha256 {
    current: String,
    replacement_submission: String,
    new_submission: String,
}

/// A validated representative envelope read through the secure file boundary.
///
/// This type cannot be constructed outside this module. It is deliberately not
/// cloneable and does not expose the binding until all independently observed
/// digests have been matched.
#[derive(Debug)]
pub(crate) struct LoadedRepresentativeEnvelope {
    envelope: RepresentativeEnvelope,
    source: OwnerPrivateExactFile,
}

/// Exact bytes and stable physical identity of one owner-private admission file.
///
/// The representative runner uses the same boundary for the binding and case
/// files. Calling [`Self::reobserve`] immediately before evidence publication
/// proves that the named file, its permissions, and its exact bytes have not
/// changed since admission.
#[derive(Debug)]
pub(crate) struct OwnerPrivateExactFile {
    path: PathBuf,
    source_bytes: Vec<u8>,
    source_sha256: String,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    state: PrivateFileState,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Debug, Eq, PartialEq)]
struct PrivateFileState {
    device: u64,
    inode: u64,
    mode: u32,
    links: u64,
    owner: u32,
    group: u32,
    size: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    changed_seconds: i64,
    changed_nanoseconds: i64,
}

/// Independently observed package-tree identities supplied by the runner.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct ObservedRepresentativePackageTrees {
    current: String,
    replacement_submission: String,
    new_submission: String,
}

/// Independently observed identities that must match a representative binding.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct RepresentativeObservations {
    case_sha256: String,
    harness_executable_sha256: String,
    source_revision: String,
    cargo_lock_sha256: String,
    package_tree_sha256: ObservedRepresentativePackageTrees,
}

/// Proof that a securely loaded representative binding matched every observed
/// identity required for admission.
///
/// The private fields and absence of public constructors make this a narrow
/// capability token for the representative runner. It is deliberately not
/// cloneable.
#[derive(Debug)]
pub(crate) struct VerifiedRepresentativeEnvelope {
    loaded: LoadedRepresentativeEnvelope,
}

/// Errors produced while securely loading or matching a representative binding.
#[derive(Debug, Error)]
pub(crate) enum RepresentativeEnvelopeError {
    /// An admission file could not be inspected, opened, or read.
    #[error("failed to securely read representative admission file `{path}`: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// A path did not identify one stable owner-private regular file.
    #[error("unsafe representative admission file `{path}`: {reason}")]
    UnsafeFile { path: PathBuf, reason: &'static str },
    /// An admission file exceeded the fixed resource limit.
    #[error(
        "representative admission file `{path}` exceeds the {MAX_ENVELOPE_BYTES}-byte size limit"
    )]
    TooLarge { path: PathBuf },
    /// The exact binding bytes were not UTF-8.
    #[error("representative binding is not UTF-8: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    /// TOML did not match the closed representative schema.
    #[error("failed to parse representative binding: {0}")]
    Parse(#[from] toml::de::Error),
    /// A parsed value violated fixed representative evidence policy.
    #[error("invalid representative binding: {0}")]
    Invalid(String),
    /// An independently observed identity did not match its bound role.
    #[error("representative binding did not match observed `{field}`")]
    ObservationMismatch { field: &'static str },
    /// An admitted private file no longer has the same identity and exact bytes.
    #[error("representative admission file `{path}` changed after it was admitted")]
    ReobservationMismatch { path: PathBuf },
    /// This platform cannot enforce the representative file boundary.
    #[error("secure representative binding loading is supported only on macOS and Linux")]
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    UnsupportedPlatform,
}

/// Securely reads and validates one owner-private representative binding.
///
/// On macOS and Linux the final path component is opened with `O_NOFOLLOW`, and
/// the named and opened identities must remain the same owner-private regular
/// file with exactly one hard link before and after two identical reads.
pub(crate) fn load_representative_envelope(
    path: impl AsRef<Path>,
) -> Result<LoadedRepresentativeEnvelope, RepresentativeEnvelopeError> {
    let source = load_owner_private_exact_file(path)?;
    let envelope = parse_representative_envelope(source.source_bytes())?;
    Ok(LoadedRepresentativeEnvelope { envelope, source })
}

/// Securely loads exact bytes that the representative runner can reobserve.
pub(crate) fn load_owner_private_exact_file(
    path: impl AsRef<Path>,
) -> Result<OwnerPrivateExactFile, RepresentativeEnvelopeError> {
    let path = path.as_ref();
    validate_private_parent(path)?;
    secure_read_owner_private(path)
}

impl OwnerPrivateExactFile {
    /// Returns the original path used for secure admission.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the exact securely loaded bytes.
    pub(crate) fn source_bytes(&self) -> &[u8] {
        &self.source_bytes
    }

    /// Returns the lowercase SHA-256 of the exact securely loaded bytes.
    pub(crate) fn source_sha256(&self) -> &str {
        &self.source_sha256
    }

    /// Reopens the named file through the same secure boundary and requires its
    /// physical identity, permissions, and exact bytes to remain unchanged.
    pub(crate) fn reobserve(&self) -> Result<(), RepresentativeEnvelopeError> {
        let observed = secure_read_owner_private(&self.path)?;
        let matches = observed.source_bytes == self.source_bytes
            && observed.source_sha256 == self.source_sha256;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let matches = matches && observed.state == self.state;
        if matches {
            Ok(())
        } else {
            Err(RepresentativeEnvelopeError::ReobservationMismatch {
                path: self.path.clone(),
            })
        }
    }
}

impl LoadedRepresentativeEnvelope {
    /// Matches every role-bound digest and returns the non-forgeable admission
    /// token on success.
    pub(crate) fn verify(
        self,
        observed: &RepresentativeObservations,
    ) -> Result<VerifiedRepresentativeEnvelope, RepresentativeEnvelopeError> {
        require_match(
            "case_sha256",
            &self.envelope.case_sha256,
            &observed.case_sha256,
        )?;
        require_match(
            "harness_executable_sha256",
            &self.envelope.harness_executable_sha256,
            &observed.harness_executable_sha256,
        )?;
        require_match(
            "build.source_revision",
            &self.envelope.build.source_revision,
            &observed.source_revision,
        )?;
        require_match(
            "build.cargo_lock_sha256",
            &self.envelope.build.cargo_lock_sha256,
            &observed.cargo_lock_sha256,
        )?;
        require_match(
            "package_tree_sha256.current",
            &self.envelope.package_tree_sha256.current,
            &observed.package_tree_sha256.current,
        )?;
        require_match(
            "package_tree_sha256.replacement_submission",
            &self.envelope.package_tree_sha256.replacement_submission,
            &observed.package_tree_sha256.replacement_submission,
        )?;
        require_match(
            "package_tree_sha256.new_submission",
            &self.envelope.package_tree_sha256.new_submission,
            &observed.package_tree_sha256.new_submission,
        )?;
        Ok(VerifiedRepresentativeEnvelope { loaded: self })
    }
}

impl ObservedRepresentativePackageTrees {
    /// Creates role-labeled observed package identities.
    pub(crate) fn new(
        current: impl Into<String>,
        replacement_submission: impl Into<String>,
        new_submission: impl Into<String>,
    ) -> Result<Self, RepresentativeEnvelopeError> {
        let value = Self {
            current: current.into(),
            replacement_submission: replacement_submission.into(),
            new_submission: new_submission.into(),
        };
        validate_sha256("observed package_tree_sha256.current", &value.current)?;
        validate_sha256(
            "observed package_tree_sha256.replacement_submission",
            &value.replacement_submission,
        )?;
        validate_sha256(
            "observed package_tree_sha256.new_submission",
            &value.new_submission,
        )?;
        Ok(value)
    }
}

impl RepresentativeObservations {
    /// Creates a complete set of independently observed admission identities.
    pub(crate) fn new(
        case_sha256: impl Into<String>,
        harness_executable_sha256: impl Into<String>,
        source_revision: impl Into<String>,
        cargo_lock_sha256: impl Into<String>,
        package_tree_sha256: ObservedRepresentativePackageTrees,
    ) -> Result<Self, RepresentativeEnvelopeError> {
        let value = Self {
            case_sha256: case_sha256.into(),
            harness_executable_sha256: harness_executable_sha256.into(),
            source_revision: source_revision.into(),
            cargo_lock_sha256: cargo_lock_sha256.into(),
            package_tree_sha256,
        };
        validate_sha256("observed case_sha256", &value.case_sha256)?;
        validate_sha256(
            "observed harness_executable_sha256",
            &value.harness_executable_sha256,
        )?;
        validate_source_revision("observed source_revision", &value.source_revision)?;
        validate_sha256("observed cargo_lock_sha256", &value.cargo_lock_sha256)?;
        Ok(value)
    }

    /// Formats a deterministic strict binding proposal from independently
    /// observed identities. The result is parsed back through the admission
    /// schema before it can leave the exact representative runner.
    pub(crate) fn binding_proposal_toml(&self) -> Result<String, RepresentativeEnvelopeError> {
        let binding_id = derive_binding_id(self)?;
        let proposal = format!(
            "format_version = {REPRESENTATIVE_FORMAT_VERSION}\n\
evidence_class = \"{REPRESENTATIVE_EVIDENCE_CLASS}\"\n\
binding_id = \"{binding_id}\"\n\
case_sha256 = \"{}\"\n\
harness_executable_sha256 = \"{}\"\n\
\n\
[build]\n\
source_revision = \"{}\"\n\
cargo_lock_sha256 = \"{}\"\n\
\n\
[package_tree_sha256]\n\
current = \"{}\"\n\
replacement_submission = \"{}\"\n\
new_submission = \"{}\"\n",
            self.case_sha256,
            self.harness_executable_sha256,
            self.source_revision,
            self.cargo_lock_sha256,
            self.package_tree_sha256.current,
            self.package_tree_sha256.replacement_submission,
            self.package_tree_sha256.new_submission,
        );
        parse_representative_envelope(proposal.as_bytes())?;
        Ok(proposal)
    }
}

impl VerifiedRepresentativeEnvelope {
    /// Returns the opaque binding identifier.
    pub(crate) fn binding_id(&self) -> &str {
        &self.loaded.envelope.binding_id
    }

    /// Returns the fixed representative evidence class.
    #[cfg(test)]
    pub(crate) fn evidence_class(&self) -> &str {
        &self.loaded.envelope.evidence_class
    }

    /// Returns the exact securely loaded TOML bytes.
    pub(crate) fn source_bytes(&self) -> &[u8] {
        self.loaded.source.source_bytes()
    }

    /// Returns the lowercase SHA-256 of the exact securely loaded TOML bytes.
    pub(crate) fn source_sha256(&self) -> &str {
        self.loaded.source.source_sha256()
    }

    /// Reobserves the binding file immediately before evidence publication.
    pub(crate) fn reobserve(&self) -> Result<(), RepresentativeEnvelopeError> {
        self.loaded.source.reobserve()
    }

    /// Returns the bound case digest.
    pub(crate) fn case_sha256(&self) -> &str {
        &self.loaded.envelope.case_sha256
    }

    /// Returns the bound harness executable digest.
    pub(crate) fn harness_executable_sha256(&self) -> &str {
        &self.loaded.envelope.harness_executable_sha256
    }

    /// Returns the bound clean source revision.
    pub(crate) fn source_revision(&self) -> &str {
        &self.loaded.envelope.build.source_revision
    }

    /// Returns the bound workspace Cargo.lock digest.
    pub(crate) fn cargo_lock_sha256(&self) -> &str {
        &self.loaded.envelope.build.cargo_lock_sha256
    }

    /// Returns the bound current-package tree digest.
    pub(crate) fn current_package_tree_sha256(&self) -> &str {
        &self.loaded.envelope.package_tree_sha256.current
    }

    /// Returns the bound replacement-submission package tree digest.
    pub(crate) fn replacement_submission_package_tree_sha256(&self) -> &str {
        &self
            .loaded
            .envelope
            .package_tree_sha256
            .replacement_submission
    }

    /// Returns the bound new-submission package tree digest.
    pub(crate) fn new_submission_package_tree_sha256(&self) -> &str {
        &self.loaded.envelope.package_tree_sha256.new_submission
    }
}

fn parse_representative_envelope(
    source_bytes: &[u8],
) -> Result<RepresentativeEnvelope, RepresentativeEnvelopeError> {
    let text = std::str::from_utf8(source_bytes)?;
    let envelope: RepresentativeEnvelope = toml::from_str(text)?;
    envelope.validate()?;
    Ok(envelope)
}

impl RepresentativeEnvelope {
    fn validate(&self) -> Result<(), RepresentativeEnvelopeError> {
        if self.format_version != REPRESENTATIVE_FORMAT_VERSION {
            return invalid(format!(
                "format_version must be {REPRESENTATIVE_FORMAT_VERSION}, got {}",
                self.format_version
            ));
        }
        if self.evidence_class != REPRESENTATIVE_EVIDENCE_CLASS {
            return invalid(format!(
                "evidence_class must be `{REPRESENTATIVE_EVIDENCE_CLASS}`"
            ));
        }
        validate_binding_id(&self.binding_id)?;
        validate_sha256("case_sha256", &self.case_sha256)?;
        validate_sha256("harness_executable_sha256", &self.harness_executable_sha256)?;
        validate_source_revision("build.source_revision", &self.build.source_revision)?;
        validate_sha256("build.cargo_lock_sha256", &self.build.cargo_lock_sha256)?;
        validate_sha256(
            "package_tree_sha256.current",
            &self.package_tree_sha256.current,
        )?;
        validate_sha256(
            "package_tree_sha256.replacement_submission",
            &self.package_tree_sha256.replacement_submission,
        )?;
        validate_sha256(
            "package_tree_sha256.new_submission",
            &self.package_tree_sha256.new_submission,
        )?;
        Ok(())
    }
}

fn validate_binding_id(value: &str) -> Result<(), RepresentativeEnvelopeError> {
    let Some(suffix) = value.strip_prefix(BINDING_ID_PREFIX) else {
        return invalid(
            "binding_id must be an opaque `rep-` identifier followed by 32 lowercase hex digits",
        );
    };
    if suffix.len() != BINDING_ID_HEX_LENGTH
        || !suffix
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return invalid(
            "binding_id must be an opaque `rep-` identifier followed by 32 lowercase hex digits",
        );
    }
    Ok(())
}

fn derive_binding_id(
    observations: &RepresentativeObservations,
) -> Result<String, RepresentativeEnvelopeError> {
    let mut framed = BINDING_ID_DOMAIN.to_vec();
    for (role, value) in [
        ("case_sha256", observations.case_sha256.as_str()),
        (
            "harness_executable_sha256",
            observations.harness_executable_sha256.as_str(),
        ),
        (
            "build.source_revision",
            observations.source_revision.as_str(),
        ),
        (
            "build.cargo_lock_sha256",
            observations.cargo_lock_sha256.as_str(),
        ),
        (
            "package_tree_sha256.current",
            observations.package_tree_sha256.current.as_str(),
        ),
        (
            "package_tree_sha256.replacement_submission",
            observations
                .package_tree_sha256
                .replacement_submission
                .as_str(),
        ),
        (
            "package_tree_sha256.new_submission",
            observations.package_tree_sha256.new_submission.as_str(),
        ),
    ] {
        append_framed(&mut framed, role.as_bytes())?;
        append_framed(&mut framed, value.as_bytes())?;
    }
    let digest = sha256_bytes(&framed);
    Ok(format!(
        "{BINDING_ID_PREFIX}{}",
        &digest[..BINDING_ID_HEX_LENGTH]
    ))
}

fn append_framed(
    destination: &mut Vec<u8>,
    value: &[u8],
) -> Result<(), RepresentativeEnvelopeError> {
    let length = u64::try_from(value.len())
        .map_err(|_| RepresentativeEnvelopeError::Invalid("binding input is too large".into()))?;
    destination.extend_from_slice(&length.to_be_bytes());
    destination.extend_from_slice(value);
    Ok(())
}

fn validate_sha256(field: &'static str, value: &str) -> Result<(), RepresentativeEnvelopeError> {
    if is_sha256(value) {
        Ok(())
    } else {
        invalid(format!("{field} must be 64 lowercase hex digits"))
    }
}

fn validate_source_revision(
    field: &'static str,
    value: &str,
) -> Result<(), RepresentativeEnvelopeError> {
    if matches!(value.len(), 40 | 64)
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        Ok(())
    } else {
        invalid(format!(
            "{field} must be a 40- or 64-character lowercase source revision"
        ))
    }
}

fn require_match(
    field: &'static str,
    expected: &str,
    observed: &str,
) -> Result<(), RepresentativeEnvelopeError> {
    if expected == observed {
        Ok(())
    } else {
        Err(RepresentativeEnvelopeError::ObservationMismatch { field })
    }
}

fn invalid<T>(message: impl Into<String>) -> Result<T, RepresentativeEnvelopeError> {
    Err(RepresentativeEnvelopeError::Invalid(message.into()))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn secure_read_owner_private(
    path: &Path,
) -> Result<OwnerPrivateExactFile, RepresentativeEnvelopeError> {
    use std::os::unix::fs::OpenOptionsExt as _;

    let named_before = fs::symlink_metadata(path).map_err(|source| read_error(path, source))?;
    validate_private_file(path, &named_before)?;

    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let mut file = options
        .open(path)
        .map_err(|source| read_error(path, source))?;
    let opened_before = file.metadata().map_err(|source| read_error(path, source))?;
    validate_private_file(path, &opened_before)?;
    if !same_file_state(&named_before, &opened_before) {
        return unsafe_file(path, "named file changed while it was opened");
    }

    let first = read_limited(path, &mut file)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|source| read_error(path, source))?;
    let second = read_limited(path, &mut file)?;
    if first != second {
        return unsafe_file(path, "file contents changed while they were read");
    }

    let opened_after = file.metadata().map_err(|source| read_error(path, source))?;
    let named_after = fs::symlink_metadata(path).map_err(|source| read_error(path, source))?;
    validate_private_file(path, &opened_after)?;
    validate_private_file(path, &named_after)?;
    if !same_file_state(&opened_before, &opened_after)
        || !same_file_state(&opened_after, &named_after)
    {
        return unsafe_file(path, "file identity changed while it was read");
    }

    let state = PrivateFileState::from_metadata(&opened_after);
    Ok(OwnerPrivateExactFile {
        path: path.to_path_buf(),
        source_sha256: sha256_bytes(&first),
        source_bytes: first,
        state,
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn secure_read_owner_private(
    _path: &Path,
) -> Result<OwnerPrivateExactFile, RepresentativeEnvelopeError> {
    Err(RepresentativeEnvelopeError::UnsupportedPlatform)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn validate_private_file(
    path: &Path,
    metadata: &Metadata,
) -> Result<(), RepresentativeEnvelopeError> {
    use std::os::unix::fs::MetadataExt as _;

    if !metadata.file_type().is_file() {
        return unsafe_file(path, "path must be a physical regular file");
    }
    if metadata.nlink() != 1 {
        return unsafe_file(path, "file must have exactly one hard link");
    }
    if metadata.uid() != rustix::process::geteuid().as_raw() {
        return unsafe_file(path, "file must be owned by the effective user");
    }
    let permissions = metadata.mode() & 0o7777;
    if permissions != 0o400 && permissions != 0o600 {
        return unsafe_file(path, "file permissions must be exactly 0400 or 0600");
    }
    if metadata.len() > MAX_ENVELOPE_BYTES {
        return Err(RepresentativeEnvelopeError::TooLarge {
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn validate_private_parent(path: &Path) -> Result<(), RepresentativeEnvelopeError> {
    use std::os::unix::fs::MetadataExt as _;

    let parent = path
        .parent()
        .ok_or_else(|| RepresentativeEnvelopeError::UnsafeFile {
            path: path.to_path_buf(),
            reason: "path must have an owner-private parent directory",
        })?;
    let metadata = fs::symlink_metadata(parent).map_err(|source| read_error(parent, source))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return unsafe_file(path, "parent must be a physical directory");
    }
    if metadata.uid() != rustix::process::geteuid().as_raw() || metadata.mode() & 0o077 != 0 {
        return unsafe_file(
            path,
            "parent directory must be owned and private to the effective user",
        );
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn validate_private_parent(_path: &Path) -> Result<(), RepresentativeEnvelopeError> {
    Err(RepresentativeEnvelopeError::UnsupportedPlatform)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn same_file_state(left: &Metadata, right: &Metadata) -> bool {
    PrivateFileState::from_metadata(left) == PrivateFileState::from_metadata(right)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl PrivateFileState {
    fn from_metadata(metadata: &Metadata) -> Self {
        use std::os::unix::fs::MetadataExt as _;

        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            mode: metadata.mode(),
            links: metadata.nlink(),
            owner: metadata.uid(),
            group: metadata.gid(),
            size: metadata.size(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
            changed_seconds: metadata.ctime(),
            changed_nanoseconds: metadata.ctime_nsec(),
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn read_limited(path: &Path, file: &mut File) -> Result<Vec<u8>, RepresentativeEnvelopeError> {
    let mut bytes = Vec::new();
    file.take(MAX_ENVELOPE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| read_error(path, source))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_ENVELOPE_BYTES {
        return Err(RepresentativeEnvelopeError::TooLarge {
            path: path.to_path_buf(),
        });
    }
    Ok(bytes)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn read_error(path: &Path, source: io::Error) -> RepresentativeEnvelopeError {
    RepresentativeEnvelopeError::Read {
        path: path.to_path_buf(),
        source,
    }
}

fn unsafe_file<T>(path: &Path, reason: &'static str) -> Result<T, RepresentativeEnvelopeError> {
    Err(RepresentativeEnvelopeError::UnsafeFile {
        path: path.to_path_buf(),
        reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const SHA_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    const SHA_D: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
    const SHA_E: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    const SHA_F: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    const SOURCE_REVISION: &str = "0123456789abcdef0123456789abcdef01234567";

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn private_tempdir() -> tempfile::TempDir {
        use std::os::unix::fs::PermissionsExt as _;

        let temporary = tempfile::tempdir().expect("temporary directory");
        fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700))
            .expect("private temporary directory");
        temporary
    }

    fn valid_toml() -> String {
        format!(
            r#"format_version = 1
evidence_class = "phase0a_representative"
binding_id = "rep-0123456789abcdef0123456789abcdef"
case_sha256 = "{SHA_A}"
harness_executable_sha256 = "{SHA_B}"

[build]
source_revision = "{SOURCE_REVISION}"
cargo_lock_sha256 = "{SHA_F}"

[package_tree_sha256]
current = "{SHA_C}"
replacement_submission = "{SHA_D}"
new_submission = "{SHA_E}"
"#
        )
    }

    fn parse(
        text: impl AsRef<[u8]>,
    ) -> Result<RepresentativeEnvelope, RepresentativeEnvelopeError> {
        parse_representative_envelope(text.as_ref())
    }

    fn observations() -> RepresentativeObservations {
        RepresentativeObservations::new(
            SHA_A,
            SHA_B,
            SOURCE_REVISION,
            SHA_F,
            ObservedRepresentativePackageTrees::new(SHA_C, SHA_D, SHA_E)
                .expect("valid package observations"),
        )
        .expect("valid observations")
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn strict_schema_retains_exact_source_bytes_and_digest() {
        let temporary = private_tempdir();
        let path = temporary.path().join("representative.toml");
        write_private_binding(&path);
        let text = valid_toml();
        let loaded = load_representative_envelope(&path).expect("valid binding");
        let verified = loaded
            .verify(&observations())
            .expect("matching observations");

        assert_eq!(
            verified.binding_id(),
            "rep-0123456789abcdef0123456789abcdef"
        );
        assert_eq!(verified.evidence_class(), REPRESENTATIVE_EVIDENCE_CLASS);
        assert_eq!(verified.source_bytes(), text.as_bytes());
        assert_eq!(verified.source_sha256(), sha256_bytes(text.as_bytes()));
        assert_eq!(verified.case_sha256(), SHA_A);
        assert_eq!(verified.harness_executable_sha256(), SHA_B);
        assert_eq!(verified.source_revision(), SOURCE_REVISION);
        assert_eq!(verified.cargo_lock_sha256(), SHA_F);
        assert_eq!(verified.current_package_tree_sha256(), SHA_C);
        assert_eq!(verified.replacement_submission_package_tree_sha256(), SHA_D);
        assert_eq!(verified.new_submission_package_tree_sha256(), SHA_E);
        verified.reobserve().expect("stable binding");
    }

    #[test]
    fn malformed_toml_is_rejected() {
        let error = parse(b"format_version = [").expect_err("must reject malformed TOML");
        assert!(matches!(error, RepresentativeEnvelopeError::Parse(_)));
    }

    #[test]
    fn binding_proposal_is_deterministic_and_accepted_by_the_strict_loader() {
        let observations = observations();
        let first = observations
            .binding_proposal_toml()
            .expect("valid proposal");
        let second = observations
            .binding_proposal_toml()
            .expect("deterministic proposal");
        assert_eq!(first, second);

        let parsed = parse(first).expect("proposal parses as a strict binding");
        assert!(parsed.binding_id.starts_with(BINDING_ID_PREFIX));
        assert_eq!(parsed.case_sha256, SHA_A);
        assert_eq!(parsed.harness_executable_sha256, SHA_B);
        assert_eq!(parsed.build.source_revision, SOURCE_REVISION);
        assert_eq!(parsed.build.cargo_lock_sha256, SHA_F);
        assert_eq!(parsed.package_tree_sha256.current, SHA_C);
        assert_eq!(parsed.package_tree_sha256.replacement_submission, SHA_D);
        assert_eq!(parsed.package_tree_sha256.new_submission, SHA_E);
    }

    #[test]
    fn unknown_top_level_and_nested_keys_are_rejected() {
        let top_level = valid_toml().replace(
            "\n[package_tree_sha256]",
            "\nunknown = true\n\n[package_tree_sha256]",
        );
        assert!(matches!(
            parse(top_level).expect_err("must reject top-level key"),
            RepresentativeEnvelopeError::Parse(_)
        ));

        let nested = valid_toml().replace(
            &format!("new_submission = \"{SHA_E}\""),
            &format!("new_submission = \"{SHA_E}\"\nunknown = true"),
        );
        assert!(matches!(
            parse(nested).expect_err("must reject nested key"),
            RepresentativeEnvelopeError::Parse(_)
        ));
    }

    #[test]
    fn wrong_evidence_class_is_rejected() {
        let text = valid_toml().replace("phase0a_representative", "generic_rehearsal");
        let error = parse(text).expect_err("must reject another class");
        assert!(matches!(error, RepresentativeEnvelopeError::Invalid(_)));
        assert!(error.to_string().contains("evidence_class"));
    }

    #[test]
    fn binding_id_must_be_opaque_and_portable() {
        for invalid_id in [
            "",
            "named-project-production",
            "rep-not-a-random-opaque-identifier",
            "rep-0123456789ABCDEF0123456789ABCDEF",
            "rep-0123456789abcdef",
        ] {
            let text = valid_toml().replace("rep-0123456789abcdef0123456789abcdef", invalid_id);
            let error = parse(text).expect_err("must reject binding id");
            assert!(error.to_string().contains("binding_id"));
        }
    }

    #[test]
    fn every_bound_sha_must_be_lowercase_sha256() {
        for valid in [SHA_A, SHA_B, SHA_C, SHA_D, SHA_E, SHA_F] {
            let uppercase = valid.to_ascii_uppercase();
            for invalid_sha in ["abc", uppercase.as_str()] {
                let text = valid_toml().replacen(valid, invalid_sha, 1);
                let error = parse(text).expect_err("must reject malformed SHA");
                assert!(error.to_string().contains("lowercase hex digits"));
            }
        }
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn role_mismatch_cannot_produce_verified_token() {
        let temporary = private_tempdir();
        let path = temporary.path().join("representative.toml");
        write_private_binding(&path);
        let loaded = load_representative_envelope(&path).expect("valid binding");
        let swapped = RepresentativeObservations::new(
            SHA_A,
            SHA_B,
            SOURCE_REVISION,
            SHA_F,
            ObservedRepresentativePackageTrees::new(SHA_C, SHA_E, SHA_D)
                .expect("valid package observations"),
        )
        .expect("valid observations");
        let error = loaded.verify(&swapped).expect_err("must reject role swap");
        assert!(matches!(
            error,
            RepresentativeEnvelopeError::ObservationMismatch {
                field: "package_tree_sha256.replacement_submission"
            }
        ));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn write_private_binding(path: &Path) {
        use std::os::unix::fs::PermissionsExt as _;

        fs::write(path, valid_toml()).expect("write binding");
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .expect("set private permissions");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn secure_loader_accepts_one_owner_private_regular_file() {
        let temporary = private_tempdir();
        let path = temporary.path().join("representative.toml");
        write_private_binding(&path);

        let loaded = load_representative_envelope(&path).expect("secure binding");
        assert!(loaded.verify(&observations()).is_ok());
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn exact_file_reobserver_detects_case_file_change() {
        let temporary = private_tempdir();
        let path = temporary.path().join("representative-case.toml");
        write_private_binding(&path);
        let admitted = load_owner_private_exact_file(&path).expect("secure case file");

        assert_eq!(admitted.path(), path);
        assert_eq!(admitted.source_bytes(), valid_toml().as_bytes());
        assert_eq!(
            admitted.source_sha256(),
            sha256_bytes(admitted.source_bytes())
        );
        admitted.reobserve().expect("unchanged case file");

        fs::write(&path, format!("{}\n", valid_toml())).expect("change case file");
        let error = admitted
            .reobserve()
            .expect_err("must detect changed case file");
        assert!(matches!(
            error,
            RepresentativeEnvelopeError::ReobservationMismatch { .. }
        ));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn secure_loader_rejects_non_private_permissions() {
        use std::os::unix::fs::PermissionsExt as _;

        let temporary = private_tempdir();
        let path = temporary.path().join("representative.toml");
        write_private_binding(&path);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644))
            .expect("set unsafe permissions");

        let error = load_representative_envelope(&path).expect_err("must reject permissions");
        assert!(matches!(
            error,
            RepresentativeEnvelopeError::UnsafeFile { .. }
        ));
        assert!(error.to_string().contains("0400 or 0600"));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn secure_loader_rejects_a_non_private_parent() {
        use std::os::unix::fs::PermissionsExt as _;

        let temporary = private_tempdir();
        let parent = temporary.path().join("admission");
        fs::create_dir(&parent).expect("admission parent");
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o755))
            .expect("set non-private parent");
        let path = parent.join("representative.toml");
        write_private_binding(&path);

        let error = load_representative_envelope(&path).expect_err("must reject parent");
        assert!(matches!(
            error,
            RepresentativeEnvelopeError::UnsafeFile { .. }
        ));
        assert!(error.to_string().contains("parent directory"));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn secure_loader_rejects_symlink() {
        use std::os::unix::fs::symlink;

        let temporary = private_tempdir();
        let target = temporary.path().join("representative.toml");
        let link = temporary.path().join("representative-link.toml");
        write_private_binding(&target);
        symlink(&target, &link).expect("create symlink");

        let error = load_representative_envelope(&link).expect_err("must reject symlink");
        assert!(matches!(
            error,
            RepresentativeEnvelopeError::UnsafeFile { .. }
        ));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn secure_loader_rejects_hardlink() {
        let temporary = private_tempdir();
        let original = temporary.path().join("representative.toml");
        let alias = temporary.path().join("representative-alias.toml");
        write_private_binding(&original);
        fs::hard_link(&original, &alias).expect("create hardlink");

        let error = load_representative_envelope(&original).expect_err("must reject hardlink");
        assert!(matches!(
            error,
            RepresentativeEnvelopeError::UnsafeFile { .. }
        ));
        assert!(error.to_string().contains("exactly one hard link"));
    }
}

//! Host-neutral validation for one immutable browser/native runtime bundle.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::Cursor,
    path::{Path, PathBuf},
    sync::Arc,
};

use png::{BitDepth, ColorType, DecodeOptions, Decoder, Limits};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{SkeletonAsset, TARGET_SPINE_VERSION, load_json};

/// Maximum encoded size of a runtime-bundle manifest.
pub const MAX_RUNTIME_MANIFEST_BYTES: usize = 64 * 1024;
/// Maximum encoded size of all files in one runtime bundle.
pub const MAX_RUNTIME_BUNDLE_BYTES: usize = 64 * 1024 * 1024;
/// Maximum number of files across one runtime review load.
pub const MAX_RUNTIME_FILE_COUNT: usize = 128;
/// Maximum encoded size of one skeleton JSON file.
pub const MAX_RUNTIME_JSON_BYTES: usize = 16 * 1024 * 1024;
/// Maximum encoded size of one text-atlas file.
pub const MAX_RUNTIME_ATLAS_BYTES: usize = 2 * 1024 * 1024;
/// Maximum encoded size of one atlas-page PNG file.
pub const MAX_RUNTIME_PAGE_BYTES: usize = 16 * 1024 * 1024;
const MAX_PAGE_DIMENSION: u32 = 4_096;
const MAX_PAGE_DECODED_BYTES: usize = 64 * 1024 * 1024;
/// Maximum decoded RGBA texture bytes across one runtime review load.
pub const MAX_RUNTIME_DECODED_TEXTURE_BYTES: usize = 192 * 1024 * 1024;
const MANIFEST_FORMAT_VERSION: u32 = 1;

/// One immutable file declared by a runtime-bundle manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeBundleFile {
    virtual_path: PathBuf,
    location_reference: Box<str>,
    max_bytes: usize,
    expected_bytes: usize,
    expected_sha256: [u8; 32],
}

impl RuntimeBundleFile {
    /// Returns the normalized path used inside the virtual bundle.
    #[must_use]
    pub fn virtual_path(&self) -> &Path {
        &self.virtual_path
    }

    /// Returns the safe relative location from which a host may acquire the file.
    #[must_use]
    pub fn location_reference(&self) -> &str {
        &self.location_reference
    }

    /// Returns the fixed encoded-byte limit for this file's role.
    #[must_use]
    pub const fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    /// Returns the exact encoded length declared by the manifest.
    #[must_use]
    pub const fn expected_bytes(&self) -> usize {
        self.expected_bytes
    }

    /// Returns the exact lowercase SHA-256 declared by the manifest.
    #[must_use]
    pub fn expected_sha256(&self) -> String {
        hex_digest(&self.expected_sha256)
    }
}

/// Parsed, bounded description of one complete immutable runtime export.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeBundleManifest {
    manifest_bytes: Vec<u8>,
    manifest_sha256: String,
    label: Box<str>,
    json_path: PathBuf,
    atlas_path: PathBuf,
    files: Box<[RuntimeBundleFile]>,
    encoded_bytes: usize,
}

impl RuntimeBundleManifest {
    /// Returns the exact normalized atlas-page paths required by JSON and atlas
    /// bytes before a host reads any page files.
    ///
    /// This discovery applies the same core load, exact-version, and path
    /// resolution checks used by final bundle validation.
    pub fn required_page_paths(
        json_path: &Path,
        atlas_path: &Path,
        json: &[u8],
        atlas: &[u8],
    ) -> Result<Vec<PathBuf>, RuntimeBundleError> {
        let json_path = validate_path_argument(json_path)?;
        let atlas_path = validate_path_argument(atlas_path)?;
        if json_path == atlas_path {
            return Err(RuntimeBundleError::DuplicatePath(json_path));
        }
        require_encoded_length(json.len(), MAX_RUNTIME_JSON_BYTES, "JSON")?;
        require_encoded_length(atlas.len(), MAX_RUNTIME_ATLAS_BYTES, "atlas")?;
        let (_asset, mut required) =
            load_and_resolve_dependencies(&json_path, &atlas_path, json, atlas)?;
        required.remove(&json_path);
        required.remove(&atlas_path);
        Ok(required.into_iter().collect())
    }

    /// Builds the canonical manifest for an exact file map and validates the
    /// resulting bundle before returning either value.
    ///
    /// File acquisition locations equal their normalized virtual paths. Hosts
    /// that need a different published layout must parse supplied manifest bytes.
    pub fn build(
        label: &str,
        json_path: &Path,
        atlas_path: &Path,
        files: BTreeMap<PathBuf, Vec<u8>>,
    ) -> Result<(Vec<u8>, ValidatedRuntimeBundle), RuntimeBundleError> {
        let json = path_text(json_path)?;
        let atlas = path_text(atlas_path)?;
        let entries = files
            .iter()
            .map(|(path, bytes)| {
                let path = path_text(path)?;
                Ok(FileDocument {
                    url: path.clone(),
                    path,
                    byte_length: u64::try_from(bytes.len()).map_err(|_error| {
                        RuntimeBundleError::InvalidManifest(
                            "runtime file length does not fit the manifest".into(),
                        )
                    })?,
                    sha256: sha256_hex(bytes),
                })
            })
            .collect::<Result<Vec<_>, RuntimeBundleError>>()?;
        let document = ManifestDocument {
            format_version: MANIFEST_FORMAT_VERSION,
            source: SourceDocument {
                label: label.to_owned(),
                json,
                atlas,
                files: entries,
            },
        };
        let manifest_bytes = serde_json::to_vec(&document).map_err(|error| {
            RuntimeBundleError::InvalidManifest(
                format!("could not encode canonical runtime manifest: {error}").into(),
            )
        })?;
        let validated = Self::parse(&manifest_bytes)?.validate(files)?;
        Ok((manifest_bytes, validated))
    }

    /// Strictly parses the exact manifest bytes using a deny-unknown-fields schema.
    pub fn parse(bytes: &[u8]) -> Result<Self, RuntimeBundleError> {
        if bytes.is_empty() {
            return Err(RuntimeBundleError::InvalidManifest(
                "the runtime manifest is empty".into(),
            ));
        }
        if bytes.len() > MAX_RUNTIME_MANIFEST_BYTES {
            return Err(RuntimeBundleError::InvalidManifest(
                format!("the runtime manifest exceeds the {MAX_RUNTIME_MANIFEST_BYTES}-byte limit")
                    .into(),
            ));
        }
        let document: ManifestDocument = serde_json::from_slice(bytes)
            .map_err(|error| RuntimeBundleError::InvalidManifest(error.to_string().into()))?;
        if document.format_version != MANIFEST_FORMAT_VERSION {
            return Err(RuntimeBundleError::InvalidManifest(
                format!(
                    "unsupported runtime manifest version {}; expected {MANIFEST_FORMAT_VERSION}",
                    document.format_version
                )
                .into(),
            ));
        }
        validate_label(&document.source.label)?;
        if document.source.files.len() < 2 {
            return Err(RuntimeBundleError::InvalidManifest(
                "the runtime manifest must declare JSON, atlas, and all referenced pages".into(),
            ));
        }
        if document.source.files.len() > MAX_RUNTIME_FILE_COUNT {
            return Err(RuntimeBundleError::InvalidManifest(
                format!("the runtime manifest exceeds the {MAX_RUNTIME_FILE_COUNT}-file limit")
                    .into(),
            ));
        }

        let json_path = validate_virtual_path(&document.source.json)?;
        let atlas_path = validate_virtual_path(&document.source.atlas)?;
        if json_path == atlas_path {
            return Err(RuntimeBundleError::DuplicatePath(json_path));
        }

        let mut seen_paths = BTreeSet::new();
        let mut seen_locations = BTreeSet::new();
        let mut files = Vec::with_capacity(document.source.files.len());
        let mut declared_total = 0_usize;
        for entry in document.source.files {
            let virtual_path = validate_virtual_path(&entry.path)?;
            if !seen_paths.insert(virtual_path.clone()) {
                return Err(RuntimeBundleError::DuplicatePath(virtual_path));
            }
            validate_runtime_bundle_location_reference(&entry.url)?;
            if !seen_locations.insert(entry.url.clone()) {
                return Err(RuntimeBundleError::DuplicateLocation(entry.url.into()));
            }
            let max_bytes = if virtual_path == json_path {
                MAX_RUNTIME_JSON_BYTES
            } else if virtual_path == atlas_path {
                MAX_RUNTIME_ATLAS_BYTES
            } else {
                MAX_RUNTIME_PAGE_BYTES
            };
            let expected_bytes = usize::try_from(entry.byte_length).map_err(|_error| {
                RuntimeBundleError::InvalidManifest(
                    format!(
                        "declared byte length for `{}` does not fit this host",
                        virtual_path.display()
                    )
                    .into(),
                )
            })?;
            if expected_bytes == 0 || expected_bytes > max_bytes {
                return Err(RuntimeBundleError::InvalidManifest(
                    format!(
                        "declared byte length for `{}` must be 1-{max_bytes}",
                        virtual_path.display()
                    )
                    .into(),
                ));
            }
            declared_total = declared_total.checked_add(expected_bytes).ok_or_else(|| {
                RuntimeBundleError::InvalidManifest(
                    "declared runtime bundle size overflowed".into(),
                )
            })?;
            if declared_total > MAX_RUNTIME_BUNDLE_BYTES {
                return Err(RuntimeBundleError::InvalidManifest(
                    format!(
                        "declared runtime bundle exceeds the {MAX_RUNTIME_BUNDLE_BYTES}-byte limit"
                    )
                    .into(),
                ));
            }
            files.push(RuntimeBundleFile {
                virtual_path,
                location_reference: entry.url.into(),
                max_bytes,
                expected_bytes,
                expected_sha256: parse_runtime_bundle_sha256(&entry.sha256)?,
            });
        }
        for required in [&json_path, &atlas_path] {
            if !seen_paths.contains(required) {
                return Err(RuntimeBundleError::MissingDeclaredFile(
                    required.to_path_buf(),
                ));
            }
        }

        Ok(Self {
            manifest_bytes: bytes.to_vec(),
            manifest_sha256: sha256_hex(bytes),
            label: document.source.label.into(),
            json_path,
            atlas_path,
            files: files.into_boxed_slice(),
            encoded_bytes: declared_total,
        })
    }

    /// Returns the human-readable, validated source label.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns the SHA-256 of the exact manifest bytes.
    #[must_use]
    pub fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
    }

    /// Returns the normalized virtual path of the skeleton JSON.
    #[must_use]
    pub fn json_path(&self) -> &Path {
        &self.json_path
    }

    /// Returns the normalized virtual path of the text atlas.
    #[must_use]
    pub fn atlas_path(&self) -> &Path {
        &self.atlas_path
    }

    /// Returns every declared file in manifest order.
    #[must_use]
    pub fn files(&self) -> &[RuntimeBundleFile] {
        &self.files
    }

    /// Returns the exact number of files declared by this manifest.
    #[must_use]
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Returns the exact sum of encoded file lengths declared by this manifest.
    #[must_use]
    pub const fn encoded_bytes(&self) -> usize {
        self.encoded_bytes
    }

    /// Checks the exact file map, strict PNG profile, and core Spinal load.
    pub fn validate(
        self,
        files: BTreeMap<PathBuf, Vec<u8>>,
    ) -> Result<ValidatedRuntimeBundle, RuntimeBundleError> {
        for path in files.keys() {
            let value = path
                .to_str()
                .ok_or_else(|| RuntimeBundleError::UnsafeInputPath(path.clone()))?;
            match validate_virtual_path(value) {
                Ok(validated) if &validated == path => {}
                _other => {
                    return Err(RuntimeBundleError::UnsafeInputPath(path.clone()));
                }
            }
        }
        let expected = self
            .files
            .iter()
            .map(|file| file.virtual_path.clone())
            .collect::<BTreeSet<_>>();
        let actual = files.keys().cloned().collect::<BTreeSet<_>>();
        if expected != actual {
            return Err(RuntimeBundleError::FileSetMismatch);
        }

        let mut decoded_total = 0_usize;
        for file in &self.files {
            let bytes = files
                .get(&file.virtual_path)
                .expect("the exact file set was checked above");
            if bytes.len() != file.expected_bytes {
                return Err(RuntimeBundleError::FileLengthMismatch {
                    path: file.virtual_path.clone(),
                    expected: file.expected_bytes,
                    actual: bytes.len(),
                });
            }
            let actual_digest: [u8; 32] = Sha256::digest(bytes).into();
            if actual_digest != file.expected_sha256 {
                return Err(RuntimeBundleError::FileDigestMismatch(
                    file.virtual_path.clone(),
                ));
            }
            if file.virtual_path != self.json_path && file.virtual_path != self.atlas_path {
                let decoded = validate_png(&file.virtual_path, bytes)?;
                decoded_total = decoded_total.checked_add(decoded).ok_or_else(|| {
                    RuntimeBundleError::InvalidTexture {
                        path: file.virtual_path.clone(),
                        detail: "decoded texture size overflowed".into(),
                    }
                })?;
                if decoded_total > MAX_RUNTIME_DECODED_TEXTURE_BYTES {
                    return Err(RuntimeBundleError::DecodedTextureBudgetExceeded);
                }
            }
        }

        let json = files
            .get(&self.json_path)
            .expect("the exact file set was checked above");
        let atlas = files
            .get(&self.atlas_path)
            .expect("the exact file set was checked above");
        let (asset, referenced) =
            load_and_resolve_dependencies(&self.json_path, &self.atlas_path, json, atlas)?;
        if referenced != expected {
            return Err(RuntimeBundleError::RuntimeFileSetMismatch);
        }

        Ok(ValidatedRuntimeBundle {
            manifest_bytes: self.manifest_bytes,
            manifest_sha256: self.manifest_sha256,
            content_sha256: content_sha256(&files),
            label: self.label,
            json_path: self.json_path,
            atlas_path: self.atlas_path,
            file_count: self.files.len(),
            encoded_bytes: self.encoded_bytes,
            decoded_texture_bytes: decoded_total,
            files,
            asset,
        })
    }
}

/// Exact immutable runtime bytes after all shared native/browser checks.
#[derive(Clone, Debug)]
pub struct ValidatedRuntimeBundle {
    manifest_bytes: Vec<u8>,
    manifest_sha256: String,
    content_sha256: String,
    label: Box<str>,
    json_path: PathBuf,
    atlas_path: PathBuf,
    file_count: usize,
    encoded_bytes: usize,
    decoded_texture_bytes: usize,
    files: BTreeMap<PathBuf, Vec<u8>>,
    asset: Arc<SkeletonAsset>,
}

impl ValidatedRuntimeBundle {
    /// Returns the exact manifest bytes used for validation.
    #[must_use]
    pub fn manifest_bytes(&self) -> &[u8] {
        &self.manifest_bytes
    }

    /// Returns the SHA-256 of the exact manifest bytes used for validation.
    #[must_use]
    pub fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
    }

    /// Returns a deterministic identity for the normalized file paths and exact
    /// bytes, independent of manifest labels and acquisition locations.
    #[must_use]
    pub fn content_sha256(&self) -> &str {
        &self.content_sha256
    }

    /// Returns the human-readable source label.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns the normalized virtual path of the skeleton JSON.
    #[must_use]
    pub fn json_path(&self) -> &Path {
        &self.json_path
    }

    /// Returns the exact skeleton JSON bytes.
    #[must_use]
    pub fn json_bytes(&self) -> &[u8] {
        self.files
            .get(&self.json_path)
            .expect("validated bundle retains its JSON")
    }

    /// Returns the normalized virtual path of the text atlas.
    #[must_use]
    pub fn atlas_path(&self) -> &Path {
        &self.atlas_path
    }

    /// Returns the exact text-atlas bytes.
    #[must_use]
    pub fn atlas_bytes(&self) -> &[u8] {
        self.files
            .get(&self.atlas_path)
            .expect("validated bundle retains its atlas")
    }

    /// Returns every exact file in normalized virtual-path order.
    pub fn files(&self) -> impl ExactSizeIterator<Item = (&Path, &[u8])> {
        self.files
            .iter()
            .map(|(path, bytes)| (path.as_path(), bytes.as_slice()))
    }

    /// Returns the exact number of validated files retained by this bundle.
    #[must_use]
    pub const fn file_count(&self) -> usize {
        self.file_count
    }

    /// Returns the exact sum of validated encoded file bytes.
    #[must_use]
    pub const fn encoded_bytes(&self) -> usize {
        self.encoded_bytes
    }

    /// Returns the exact decoded RGBA byte cost of all validated texture pages.
    #[must_use]
    pub const fn decoded_texture_bytes(&self) -> usize {
        self.decoded_texture_bytes
    }

    /// Consumes the validated bundle and moves out its exact file map.
    ///
    /// Hosts can retain cloned paths and the shared asset before calling this
    /// method, avoiding a second full-bundle byte allocation.
    #[must_use]
    pub fn into_files(self) -> BTreeMap<PathBuf, Vec<u8>> {
        self.files
    }

    /// Returns the core Spinal asset loaded from these exact bytes.
    #[must_use]
    pub fn asset(&self) -> &Arc<SkeletonAsset> {
        &self.asset
    }
}

/// Failure at the shared immutable runtime-bundle boundary.
#[derive(Debug, Error)]
pub enum RuntimeBundleError {
    /// The manifest schema, version, value, or fixed limit is invalid.
    #[error("invalid runtime manifest: {0}")]
    InvalidManifest(Box<str>),
    /// Two manifest entries use the same virtual path.
    #[error("duplicate runtime bundle path `{}`", .0.display())]
    DuplicatePath(PathBuf),
    /// Two manifest entries use the same acquisition location.
    #[error("duplicate runtime bundle location `{0}`")]
    DuplicateLocation(Box<str>),
    /// A required JSON or atlas path has no file declaration.
    #[error("runtime manifest has no file entry for `{}`", .0.display())]
    MissingDeclaredFile(PathBuf),
    /// An input-map key is not one normalized portable relative path.
    #[error("unsafe runtime bundle input path `{}`", .0.display())]
    UnsafeInputPath(PathBuf),
    /// The supplied file names do not exactly equal the declared names.
    #[error("runtime bundle files do not match the manifest")]
    FileSetMismatch,
    /// One file's actual length differs from the manifest.
    #[error(
        "runtime bundle file `{}` has {actual} bytes; expected {expected}",
        path.display()
    )]
    FileLengthMismatch {
        /// Normalized virtual file path.
        path: PathBuf,
        /// Exact manifest length.
        expected: usize,
        /// Actual supplied length.
        actual: usize,
    },
    /// One file's digest differs from the manifest.
    #[error("runtime bundle file `{}` failed its SHA-256 check", .0.display())]
    FileDigestMismatch(PathBuf),
    /// A texture is not a fully decodable PNG in the fixed profile.
    #[error("invalid runtime texture `{}`: {detail}", path.display())]
    InvalidTexture {
        /// Normalized virtual texture path.
        path: PathBuf,
        /// Non-sensitive validation detail.
        detail: Box<str>,
    },
    /// The sum of decoded RGBA page sizes exceeds the fixed budget.
    #[error("runtime textures exceed the {MAX_RUNTIME_DECODED_TEXTURE_BYTES}-byte decoded limit")]
    DecodedTextureBudgetExceeded,
    /// Core Spinal rejected the JSON or atlas.
    #[error("invalid Spine runtime export: {0}")]
    InvalidExport(#[source] Box<crate::LoadError>),
    /// The export does not target the one supported editor patch version.
    #[error("expected Spine {expected}, but the export declares {actual}")]
    WrongSpineVersion {
        /// Required exact editor version.
        expected: &'static str,
        /// Version declared by the export.
        actual: Box<str>,
    },
    /// An atlas page reference cannot resolve inside the virtual root.
    #[error("invalid atlas page reference `{page}`: {reason}")]
    InvalidPageReference {
        /// Raw page name from the atlas.
        page: Box<str>,
        /// Fixed reason for rejection.
        reason: &'static str,
    },
    /// Two atlas page names normalize to the same virtual file.
    #[error("duplicate resolved runtime dependency `{}`", .0.display())]
    DuplicateDependencyPath(PathBuf),
    /// Declared runtime files differ from the dependencies loaded by Spinal.
    #[error("runtime manifest file set does not exactly match JSON, atlas, and atlas pages")]
    RuntimeFileSetMismatch,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ManifestDocument {
    format_version: u32,
    source: SourceDocument,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SourceDocument {
    label: String,
    json: String,
    atlas: String,
    files: Vec<FileDocument>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FileDocument {
    path: String,
    url: String,
    byte_length: u64,
    sha256: String,
}

fn validate_label(label: &str) -> Result<(), RuntimeBundleError> {
    if label.is_empty()
        || label.len() > 128
        || label.chars().any(|character| character.is_control())
    {
        return Err(RuntimeBundleError::InvalidManifest(
            "source label must be 1-128 non-control UTF-8 bytes".into(),
        ));
    }
    Ok(())
}

fn path_text(path: &Path) -> Result<String, RuntimeBundleError> {
    let validated = validate_path_argument(path)?;
    let value = validated.to_str().expect("validated virtual path is UTF-8");
    Ok(value.to_owned())
}

fn validate_path_argument(path: &Path) -> Result<PathBuf, RuntimeBundleError> {
    let value = path
        .to_str()
        .ok_or_else(|| RuntimeBundleError::UnsafeInputPath(path.to_path_buf()))?;
    let validated = validate_virtual_path(value)?;
    if validated != path {
        return Err(RuntimeBundleError::UnsafeInputPath(path.to_path_buf()));
    }
    Ok(validated)
}

fn require_encoded_length(
    actual: usize,
    maximum: usize,
    role: &'static str,
) -> Result<(), RuntimeBundleError> {
    if actual == 0 || actual > maximum {
        Err(RuntimeBundleError::InvalidManifest(
            format!("{role} bytes must have length 1-{maximum}").into(),
        ))
    } else {
        Ok(())
    }
}

fn load_and_resolve_dependencies(
    json_path: &Path,
    atlas_path: &Path,
    json: &[u8],
    atlas: &[u8],
) -> Result<(Arc<SkeletonAsset>, BTreeSet<PathBuf>), RuntimeBundleError> {
    let loaded = load_json(json, atlas)
        .map_err(|source| RuntimeBundleError::InvalidExport(Box::new(source)))?;
    let asset = Arc::clone(loaded.asset());
    if asset.spine_version() != TARGET_SPINE_VERSION {
        return Err(RuntimeBundleError::WrongSpineVersion {
            expected: TARGET_SPINE_VERSION,
            actual: asset.spine_version().into(),
        });
    }
    let mut referenced = BTreeSet::from([json_path.to_path_buf(), atlas_path.to_path_buf()]);
    for page in asset.atlas_pages() {
        let path = resolve_page_path(atlas_path, page.name()).map_err(|reason| {
            RuntimeBundleError::InvalidPageReference {
                page: page.name().into(),
                reason,
            }
        })?;
        if !referenced.insert(path.clone()) {
            return Err(RuntimeBundleError::DuplicateDependencyPath(path));
        }
    }
    Ok((asset, referenced))
}

fn validate_virtual_path(value: &str) -> Result<PathBuf, RuntimeBundleError> {
    let invalid = value.is_empty()
        || value.len() > 2_048
        || value.starts_with('/')
        || value.starts_with('\\')
        || looks_like_windows_drive(value)
        || value.contains(['\\', ':', '#', '?', '%'])
        || value.chars().any(|character| character.is_control())
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..");
    if invalid {
        return Err(RuntimeBundleError::InvalidManifest(
            format!("unsafe virtual runtime bundle path `{value}`").into(),
        ));
    }
    Ok(PathBuf::from(value))
}

/// Validates one safe relative acquisition location used by a runtime bundle.
///
/// Browser and native hosts should use this exact grammar for any outer
/// manifest that points at a [`RuntimeBundleManifest`].
pub fn validate_runtime_bundle_location_reference(value: &str) -> Result<(), RuntimeBundleError> {
    let invalid = value.is_empty()
        || value.len() > 2_048
        || value.starts_with('/')
        || value.starts_with('\\')
        || looks_like_windows_drive(value)
        || value.contains(['\\', ':', '?', '#', '%'])
        || value.chars().any(|character| character.is_control())
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..");
    if invalid {
        return Err(RuntimeBundleError::InvalidManifest(
            "runtime file locations must be safe relative paths".into(),
        ));
    }
    Ok(())
}

/// Parses one canonical lowercase SHA-256 value used by a runtime bundle.
pub fn parse_runtime_bundle_sha256(value: &str) -> Result<[u8; 32], RuntimeBundleError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(RuntimeBundleError::InvalidManifest(
            "file SHA-256 values must be 64 lowercase hexadecimal characters".into(),
        ));
    }
    let mut digest = [0_u8; 32];
    for (index, pair) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        digest[index] = (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]);
    }
    Ok(digest)
}

const fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _other => 0,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex_digest(&Sha256::digest(bytes).into())
}

fn content_sha256(files: &BTreeMap<PathBuf, Vec<u8>>) -> String {
    let mut digest = Sha256::new();
    digest.update(b"spinal-runtime-bundle-content-v1\0");
    digest.update(
        u64::try_from(files.len())
            .expect("runtime file-count limit fits u64")
            .to_be_bytes(),
    );
    for (path, bytes) in files {
        let path = path
            .to_str()
            .expect("validated runtime bundle paths are UTF-8")
            .as_bytes();
        digest.update(
            u64::try_from(path.len())
                .expect("runtime path limit fits u64")
                .to_be_bytes(),
        );
        digest.update(path);
        digest.update(
            u64::try_from(bytes.len())
                .expect("runtime file-size limit fits u64")
                .to_be_bytes(),
        );
        digest.update(bytes);
    }
    hex_digest(&digest.finalize().into())
}

fn hex_digest(digest: &[u8; 32]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn validate_png(path: &Path, bytes: &[u8]) -> Result<usize, RuntimeBundleError> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    let invalid_profile = || {
        RuntimeBundleError::InvalidTexture {
        path: path.to_path_buf(),
        detail: "expected a bounded non-animated, non-interlaced 8-bit RGBA PNG using only approved fixed-size metadata chunks".into(),
    }
    };
    if bytes.len() < 33
        || &bytes[..8] != PNG_SIGNATURE
        || u32::from_be_bytes(bytes[8..12].try_into().expect("four-byte PNG field")) != 13
        || &bytes[12..16] != b"IHDR"
    {
        return Err(invalid_profile());
    }
    let profile = (bytes[24], bytes[25], bytes[26], bytes[27], bytes[28]);
    if profile != (8, 6, 0, 0, 0) {
        return Err(invalid_profile());
    }
    validate_png_chunk_profile(path, bytes)?;

    let width = u32::from_be_bytes(bytes[16..20].try_into().expect("four-byte PNG width"));
    let height = u32::from_be_bytes(bytes[20..24].try_into().expect("four-byte PNG height"));
    if width == 0 || height == 0 || width > MAX_PAGE_DIMENSION || height > MAX_PAGE_DIMENSION {
        return Err(RuntimeBundleError::InvalidTexture {
            path: path.to_path_buf(),
            detail: format!(
                "PNG dimensions {width}x{height} exceed the 1-{MAX_PAGE_DIMENSION} limit"
            )
            .into(),
        });
    }
    let decoded = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| RuntimeBundleError::InvalidTexture {
            path: path.to_path_buf(),
            detail: "decoded PNG size overflowed".into(),
        })?;
    if decoded > MAX_PAGE_DECODED_BYTES {
        return Err(RuntimeBundleError::InvalidTexture {
            path: path.to_path_buf(),
            detail: format!(
                "decoded PNG requires {decoded} bytes; limit is {MAX_PAGE_DECODED_BYTES}"
            )
            .into(),
        });
    }

    let mut options = DecodeOptions::default();
    options.set_ignore_checksums(false);
    options.set_skip_ancillary_crc_failures(false);
    let mut decoder = Decoder::new_with_options(Cursor::new(bytes), options);
    decoder.set_limits(Limits {
        bytes: MAX_PAGE_DECODED_BYTES,
    });
    let mut reader = decoder
        .read_info()
        .map_err(|_error| RuntimeBundleError::InvalidTexture {
            path: path.to_path_buf(),
            detail: "PNG metadata or checksums are invalid".into(),
        })?;
    let info = reader.info();
    if info.width != width
        || info.height != height
        || info.bit_depth != BitDepth::Eight
        || info.color_type != ColorType::Rgba
        || info.interlaced
        || info.animation_control.is_some()
    {
        return Err(invalid_profile());
    }
    let output_bytes =
        reader
            .output_buffer_size()
            .ok_or_else(|| RuntimeBundleError::InvalidTexture {
                path: path.to_path_buf(),
                detail: "decoded PNG size overflowed".into(),
            })?;
    if output_bytes != decoded {
        return Err(invalid_profile());
    }
    let mut pixels = vec![0_u8; output_bytes];
    let output =
        reader
            .next_frame(&mut pixels)
            .map_err(|_error| RuntimeBundleError::InvalidTexture {
                path: path.to_path_buf(),
                detail: "PNG pixels or checksums are invalid".into(),
            })?;
    if output.buffer_size() != decoded {
        return Err(invalid_profile());
    }
    reader
        .finish()
        .map_err(|_error| RuntimeBundleError::InvalidTexture {
            path: path.to_path_buf(),
            detail: "PNG ending or checksums are invalid".into(),
        })?;
    Ok(decoded)
}

fn validate_png_chunk_profile(path: &Path, bytes: &[u8]) -> Result<(), RuntimeBundleError> {
    #[derive(Clone, Copy, Eq, PartialEq)]
    enum Stage {
        Header,
        Metadata,
        ImageData,
    }

    let invalid = |detail: &'static str| RuntimeBundleError::InvalidTexture {
        path: path.to_path_buf(),
        detail: detail.into(),
    };
    let mut cursor = 8_usize;
    let mut stage = Stage::Header;
    let mut saw_image_data = false;
    loop {
        let header_end = cursor
            .checked_add(8)
            .ok_or_else(|| invalid("PNG chunk offset overflowed"))?;
        if header_end > bytes.len() {
            return Err(invalid("PNG chunk header is truncated"));
        }
        let length = usize::try_from(u32::from_be_bytes(
            bytes[cursor..cursor + 4]
                .try_into()
                .expect("four-byte PNG chunk length"),
        ))
        .map_err(|_error| invalid("PNG chunk length does not fit this host"))?;
        let kind = &bytes[cursor + 4..header_end];
        let chunk_end = header_end
            .checked_add(length)
            .and_then(|end| end.checked_add(4))
            .ok_or_else(|| invalid("PNG chunk length overflowed"))?;
        if chunk_end > bytes.len() {
            return Err(invalid("PNG chunk payload is truncated"));
        }

        match kind {
            b"IHDR" if stage == Stage::Header && cursor == 8 && length == 13 => {
                stage = Stage::Metadata;
            }
            b"cHRM" if stage == Stage::Metadata && length == 32 => {}
            b"gAMA" if stage == Stage::Metadata && length == 4 => {}
            b"sBIT" if stage == Stage::Metadata && length == 4 => {}
            b"sRGB" if stage == Stage::Metadata && length == 1 => {}
            b"bKGD" if stage == Stage::Metadata && length == 6 => {}
            b"pHYs" if stage == Stage::Metadata && length == 9 => {}
            b"tIME" if stage == Stage::Metadata && length == 7 => {}
            b"IDAT" if matches!(stage, Stage::Metadata | Stage::ImageData) && length > 0 => {
                stage = Stage::ImageData;
                saw_image_data = true;
            }
            b"IEND" if stage == Stage::ImageData && saw_image_data && length == 0 => {
                if chunk_end != bytes.len() {
                    return Err(invalid("PNG contains bytes after its IEND chunk"));
                }
                return Ok(());
            }
            _other => {
                return Err(invalid(
                    "PNG contains an unsupported, compressed-metadata, animated, misplaced, or malformed chunk",
                ));
            }
        }
        cursor = chunk_end;
        if cursor == bytes.len() {
            return Err(invalid("PNG has no final IEND chunk"));
        }
    }
}

fn resolve_page_path(atlas_path: &Path, reference: &str) -> Result<PathBuf, &'static str> {
    if reference.is_empty() {
        return Err("the page name is empty");
    }
    if reference.starts_with('/')
        || reference.starts_with('\\')
        || looks_like_windows_drive(reference)
    {
        return Err("absolute page paths are not allowed");
    }
    if reference.contains(['\\', ':', '#', '?', '%']) {
        return Err("unsafe page-path syntax is not allowed");
    }
    if reference.chars().any(|character| character.is_control()) {
        return Err("control characters are not allowed in page paths");
    }

    let mut resolved = atlas_path
        .to_str()
        .expect("validated virtual paths are UTF-8")
        .split('/')
        .collect::<Vec<_>>();
    resolved.pop();
    for component in reference.split('/') {
        match component {
            "" => return Err("empty page-path components are not allowed"),
            "." => return Err("dot page-path components are not allowed"),
            ".." => {
                if resolved.pop().is_none() {
                    return Err("the page path escapes the virtual bundle root");
                }
            }
            value => resolved.push(value),
        }
    }
    if resolved.is_empty() {
        return Err("the page path does not name a file");
    }
    let result = resolved.join("/");
    validate_virtual_path(&result)
        .map_err(|_error| "the resolved page path is not normalized and portable")
}

fn looks_like_windows_drive(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &[u8] = br#"{"skeleton":{"spine":"4.3.23"},"bones":[{"name":"root"}]}"#;
    const ATLAS: &[u8] = b"textures/page.png\n\tsize: 1, 1\n\tformat: RGBA8888\n\tfilter: Linear, Linear\n\trepeat: none\n\tpma: false\n";
    const PNG: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 96, 96, 248, 255, 31,
        0, 3, 2, 1, 255, 230, 119, 11, 174, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];

    fn manifest(json_path: &str, page: &[u8]) -> Vec<u8> {
        format!(
            r#"{{
  "format_version": 1,
  "source": {{
    "label": "Generic fixture",
    "json": "{json_path}",
    "atlas": "rig/fixture.atlas",
    "files": [
      {{"path":"{json_path}","url":"fixture.json","byte_length":{},"sha256":"{}"}},
      {{"path":"rig/fixture.atlas","url":"fixture.atlas","byte_length":{},"sha256":"{}"}},
      {{"path":"rig/textures/page.png","url":"textures/page.png","byte_length":{},"sha256":"{}"}}
    ]
  }}
}}"#,
            JSON.len(),
            sha256_hex(JSON),
            ATLAS.len(),
            sha256_hex(ATLAS),
            page.len(),
            sha256_hex(page),
        )
        .into_bytes()
    }

    fn exact_files() -> BTreeMap<PathBuf, Vec<u8>> {
        BTreeMap::from([
            (PathBuf::from("rig/fixture.json"), JSON.to_vec()),
            (PathBuf::from("rig/fixture.atlas"), ATLAS.to_vec()),
            (PathBuf::from("rig/textures/page.png"), PNG.to_vec()),
        ])
    }

    #[test]
    fn exact_manifest_and_file_map_load_once_for_every_host() {
        let manifest_bytes = manifest("rig/fixture.json", PNG);
        let parsed = RuntimeBundleManifest::parse(&manifest_bytes).expect("valid manifest");
        assert_eq!(parsed.manifest_sha256(), sha256_hex(&manifest_bytes));
        assert_eq!(parsed.files()[0].expected_bytes(), JSON.len());
        assert_eq!(parsed.file_count(), 3);
        assert_eq!(parsed.encoded_bytes(), JSON.len() + ATLAS.len() + PNG.len());
        let bundle = parsed.validate(exact_files()).expect("valid bundle");
        assert_eq!(bundle.json_path(), Path::new("rig/fixture.json"));
        assert_eq!(bundle.json_bytes(), JSON);
        assert_eq!(bundle.atlas_bytes(), ATLAS);
        assert_eq!(bundle.asset().spine_version(), TARGET_SPINE_VERSION);
        assert_eq!(bundle.files().len(), 3);
        assert_eq!(bundle.file_count(), 3);
        assert_eq!(bundle.encoded_bytes(), JSON.len() + ATLAS.len() + PNG.len());
        assert_eq!(bundle.decoded_texture_bytes(), 4);
    }

    #[test]
    fn canonical_builder_and_required_page_discovery_share_resolution() {
        let required = RuntimeBundleManifest::required_page_paths(
            Path::new("rig/fixture.json"),
            Path::new("rig/fixture.atlas"),
            JSON,
            ATLAS,
        )
        .expect("discover pages");
        assert_eq!(required, [PathBuf::from("rig/textures/page.png")]);

        let (first_manifest, validated) = RuntimeBundleManifest::build(
            "Generic fixture",
            Path::new("rig/fixture.json"),
            Path::new("rig/fixture.atlas"),
            exact_files(),
        )
        .expect("canonical bundle");
        let (second_manifest, _second) = RuntimeBundleManifest::build(
            "Generic fixture",
            Path::new("rig/fixture.json"),
            Path::new("rig/fixture.atlas"),
            exact_files(),
        )
        .expect("same canonical bundle");
        assert_eq!(first_manifest, second_manifest);
        assert_eq!(validated.manifest_bytes(), first_manifest);
        assert_eq!(validated.manifest_sha256(), sha256_hex(&first_manifest));
        assert_eq!(validated.content_sha256().len(), 64);
    }

    #[test]
    fn content_identity_ignores_manifest_metadata_but_changes_with_exact_bytes() {
        let (first_manifest, first) = RuntimeBundleManifest::build(
            "First acquisition label",
            Path::new("rig/fixture.json"),
            Path::new("rig/fixture.atlas"),
            exact_files(),
        )
        .expect("first bundle");
        let (second_manifest, second) = RuntimeBundleManifest::build(
            "Second acquisition label",
            Path::new("rig/fixture.json"),
            Path::new("rig/fixture.atlas"),
            exact_files(),
        )
        .expect("same exact content");
        assert_ne!(first_manifest, second_manifest);
        assert_ne!(first.manifest_sha256(), second.manifest_sha256());
        assert_eq!(first.content_sha256(), second.content_sha256());

        let mut changed_files = exact_files();
        changed_files
            .get_mut(Path::new("rig/fixture.json"))
            .expect("JSON")
            .push(b' ');
        let (_manifest, changed) = RuntimeBundleManifest::build(
            "First acquisition label",
            Path::new("rig/fixture.json"),
            Path::new("rig/fixture.atlas"),
            changed_files,
        )
        .expect("semantically equivalent changed bytes remain valid");
        assert_ne!(first.content_sha256(), changed.content_sha256());
    }

    #[test]
    fn consuming_files_moves_the_original_page_allocation() {
        let (_manifest, validated) = RuntimeBundleManifest::build(
            "Generic fixture",
            Path::new("rig/fixture.json"),
            Path::new("rig/fixture.atlas"),
            exact_files(),
        )
        .expect("canonical bundle");
        let page_pointer = validated
            .files()
            .find(|(path, _bytes)| *path == Path::new("rig/textures/page.png"))
            .expect("page")
            .1
            .as_ptr();
        let files = validated.into_files();
        assert_eq!(
            files
                .get(Path::new("rig/textures/page.png"))
                .expect("moved page")
                .as_ptr(),
            page_pointer
        );
    }

    #[test]
    fn unsafe_paths_are_rejected_before_file_intake() {
        for path in [
            "../fixture.json",
            "/fixture.json",
            "C:/fixture.json",
            "a//b.json",
        ] {
            assert!(RuntimeBundleManifest::parse(&manifest(path, PNG)).is_err());
        }
    }

    #[test]
    fn exact_lengths_digests_and_file_set_are_required() {
        let parsed =
            RuntimeBundleManifest::parse(&manifest("rig/fixture.json", PNG)).expect("manifest");
        let mut missing = exact_files();
        missing.remove(Path::new("rig/textures/page.png"));
        assert!(matches!(
            parsed.clone().validate(missing),
            Err(RuntimeBundleError::FileSetMismatch)
        ));

        let mut extra = exact_files();
        extra.insert(PathBuf::from("extra.png"), PNG.to_vec());
        assert!(matches!(
            parsed.clone().validate(extra),
            Err(RuntimeBundleError::FileSetMismatch)
        ));

        let mut changed = exact_files();
        changed
            .get_mut(Path::new("rig/fixture.json"))
            .expect("JSON")
            .push(b' ');
        assert!(matches!(
            parsed.validate(changed),
            Err(RuntimeBundleError::FileLengthMismatch { .. })
        ));
    }

    #[test]
    fn arbitrary_or_corrupt_image_bytes_are_rejected() {
        let arbitrary = b"not a PNG";
        let error = RuntimeBundleManifest::parse(&manifest("rig/fixture.json", arbitrary))
            .expect("content-bound manifest")
            .validate(BTreeMap::from([
                (PathBuf::from("rig/fixture.json"), JSON.to_vec()),
                (PathBuf::from("rig/fixture.atlas"), ATLAS.to_vec()),
                (PathBuf::from("rig/textures/page.png"), arbitrary.to_vec()),
            ]))
            .expect_err("arbitrary image bytes");
        assert!(matches!(error, RuntimeBundleError::InvalidTexture { .. }));

        let mut corrupt = PNG.to_vec();
        corrupt[50] ^= 1;
        let document = String::from_utf8(manifest("rig/fixture.json", &corrupt)).expect("UTF-8");
        let error = RuntimeBundleManifest::parse(document.as_bytes())
            .expect("manifest")
            .validate(BTreeMap::from([
                (PathBuf::from("rig/fixture.json"), JSON.to_vec()),
                (PathBuf::from("rig/fixture.atlas"), ATLAS.to_vec()),
                (PathBuf::from("rig/textures/page.png"), corrupt),
            ]))
            .expect_err("corrupt image");
        assert!(matches!(error, RuntimeBundleError::InvalidTexture { .. }));
    }

    #[test]
    fn strict_png_profile_rejects_animation_metadata_and_wrong_profile() {
        let mut animated = PNG.to_vec();
        let mut chunk = Vec::new();
        chunk.extend_from_slice(&8_u32.to_be_bytes());
        chunk.extend_from_slice(b"acTL");
        chunk.extend_from_slice(&[0; 12]);
        animated.splice(33..33, chunk);
        assert!(validate_png(Path::new("page.png"), &animated).is_err());

        let mut indexed = PNG.to_vec();
        indexed[25] = 3;
        assert!(validate_png(Path::new("page.png"), &indexed).is_err());

        let mut corrupt_end_checksum = PNG.to_vec();
        let last = corrupt_end_checksum.len() - 1;
        corrupt_end_checksum[last] ^= 1;
        assert!(validate_png(Path::new("page.png"), &corrupt_end_checksum).is_err());
    }
}

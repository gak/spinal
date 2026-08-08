//! Strict manifest boundary for browser-hosted immutable export bundles.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::bundle::SourceBundle;

pub(crate) const MAX_MANIFEST_BYTES: usize = 64 * 1024;
pub(crate) const MAX_BROWSER_BUNDLE_BYTES: usize = 64 * 1024 * 1024;
const MAX_BROWSER_FILE_COUNT: usize = 128;
const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;
const MAX_ATLAS_BYTES: usize = 2 * 1024 * 1024;
const MAX_PAGE_BYTES: usize = 16 * 1024 * 1024;
const MAX_PAGE_DIMENSION: u32 = 4_096;
const MAX_PAGE_DECODED_BYTES: usize = 64 * 1024 * 1024;
const MAX_TOTAL_DECODED_BYTES: usize = 192 * 1024 * 1024;
const MANIFEST_FORMAT_VERSION: u32 = 1;

/// One URL-backed file declared by a browser bundle manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserFile {
    virtual_path: PathBuf,
    url_reference: Box<str>,
    max_bytes: usize,
    expected_bytes: usize,
    expected_sha256: [u8; 32],
}

impl BrowserFile {
    pub(crate) fn virtual_path(&self) -> &Path {
        &self.virtual_path
    }

    pub(crate) fn url_reference(&self) -> &str {
        &self.url_reference
    }

    pub(crate) const fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    pub(crate) const fn expected_bytes(&self) -> usize {
        self.expected_bytes
    }
}

/// Validated, versioned browser description of one complete runtime export.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserManifest {
    label: Box<str>,
    json_path: PathBuf,
    atlas_path: PathBuf,
    files: Box<[BrowserFile]>,
}

impl BrowserManifest {
    /// Parses a complete manifest using a fixed, deny-unknown-fields schema.
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, BrowserManifestError> {
        if bytes.is_empty() {
            return Err(BrowserManifestError::Invalid(
                "the browser manifest is empty".into(),
            ));
        }
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(BrowserManifestError::Invalid(
                format!("the browser manifest exceeds the {MAX_MANIFEST_BYTES}-byte limit").into(),
            ));
        }
        let document: ManifestDocument = serde_json::from_slice(bytes)
            .map_err(|error| BrowserManifestError::Invalid(error.to_string().into()))?;
        if document.format_version != MANIFEST_FORMAT_VERSION {
            return Err(BrowserManifestError::Invalid(
                format!(
                    "unsupported browser manifest version {}; expected {MANIFEST_FORMAT_VERSION}",
                    document.format_version
                )
                .into(),
            ));
        }
        validate_label(&document.source.label)?;
        if document.source.files.len() < 2 {
            return Err(BrowserManifestError::Invalid(
                "the browser manifest must declare JSON, atlas, and all referenced pages".into(),
            ));
        }
        if document.source.files.len() > MAX_BROWSER_FILE_COUNT {
            return Err(BrowserManifestError::Invalid(
                format!("the browser manifest exceeds the {MAX_BROWSER_FILE_COUNT}-file limit")
                    .into(),
            ));
        }

        let json_path = validate_virtual_path(&document.source.json)?;
        let atlas_path = validate_virtual_path(&document.source.atlas)?;
        if json_path == atlas_path {
            return Err(BrowserManifestError::DuplicatePath(json_path));
        }

        let mut seen_paths = BTreeSet::new();
        let mut seen_references = BTreeSet::new();
        let mut files = Vec::with_capacity(document.source.files.len());
        let mut declared_total = 0_usize;
        for entry in document.source.files {
            let virtual_path = validate_virtual_path(&entry.path)?;
            if !seen_paths.insert(virtual_path.clone()) {
                return Err(BrowserManifestError::DuplicatePath(virtual_path));
            }
            validate_url_reference(&entry.url)?;
            if !seen_references.insert(entry.url.clone()) {
                return Err(BrowserManifestError::DuplicateUrl(entry.url.into()));
            }
            let max_bytes = if virtual_path == json_path {
                MAX_JSON_BYTES
            } else if virtual_path == atlas_path {
                MAX_ATLAS_BYTES
            } else {
                MAX_PAGE_BYTES
            };
            let expected_bytes = usize::try_from(entry.byte_length).map_err(|_error| {
                BrowserManifestError::Invalid(
                    format!(
                        "declared byte length for `{}` does not fit this browser",
                        virtual_path.display()
                    )
                    .into(),
                )
            })?;
            if expected_bytes == 0 || expected_bytes > max_bytes {
                return Err(BrowserManifestError::Invalid(
                    format!(
                        "declared byte length for `{}` must be 1-{max_bytes}",
                        virtual_path.display()
                    )
                    .into(),
                ));
            }
            declared_total = declared_total.checked_add(expected_bytes).ok_or_else(|| {
                BrowserManifestError::Invalid("declared browser bundle size overflowed".into())
            })?;
            if declared_total > MAX_BROWSER_BUNDLE_BYTES {
                return Err(BrowserManifestError::Invalid(
                    format!(
                        "declared browser bundle exceeds the {MAX_BROWSER_BUNDLE_BYTES}-byte limit"
                    )
                    .into(),
                ));
            }
            let expected_sha256 = parse_sha256(&entry.sha256)?;
            files.push(BrowserFile {
                virtual_path,
                url_reference: entry.url.into(),
                max_bytes,
                expected_bytes,
                expected_sha256,
            });
        }
        for required in [&json_path, &atlas_path] {
            if !seen_paths.contains(required) {
                return Err(BrowserManifestError::MissingDeclaredFile(
                    required.to_path_buf(),
                ));
            }
        }

        Ok(Self {
            label: document.source.label.into(),
            json_path,
            atlas_path,
            files: files.into_boxed_slice(),
        })
    }

    pub(crate) fn label(&self) -> &str {
        &self.label
    }

    pub(crate) fn files(&self) -> &[BrowserFile] {
        &self.files
    }

    /// Creates the same immutable bundle used by the native filesystem host.
    pub(crate) fn into_bundle(
        self,
        mut downloaded: BTreeMap<PathBuf, Vec<u8>>,
    ) -> Result<SourceBundle, BrowserManifestError> {
        let expected = self
            .files
            .iter()
            .map(|file| file.virtual_path.clone())
            .collect::<BTreeSet<_>>();
        let actual = downloaded.keys().cloned().collect::<BTreeSet<_>>();
        if expected != actual {
            return Err(BrowserManifestError::DownloadedFileSetMismatch);
        }
        let mut decoded_total = 0_usize;
        for file in &self.files {
            let bytes = downloaded
                .get(&file.virtual_path)
                .expect("the downloaded set was checked above");
            if bytes.len() != file.expected_bytes {
                return Err(BrowserManifestError::FileLengthMismatch {
                    path: file.virtual_path.clone(),
                    expected: file.expected_bytes,
                    actual: bytes.len(),
                });
            }
            let actual_digest: [u8; 32] = Sha256::digest(bytes).into();
            if actual_digest != file.expected_sha256 {
                return Err(BrowserManifestError::FileDigestMismatch(
                    file.virtual_path.clone(),
                ));
            }
            if file.virtual_path != self.json_path && file.virtual_path != self.atlas_path {
                let decoded = png_decoded_bytes(&file.virtual_path, bytes)?;
                decoded_total = decoded_total.checked_add(decoded).ok_or_else(|| {
                    BrowserManifestError::InvalidTexture {
                        path: file.virtual_path.clone(),
                        detail: "decoded texture size overflowed".into(),
                    }
                })?;
                if decoded_total > MAX_TOTAL_DECODED_BYTES {
                    return Err(BrowserManifestError::DecodedTextureBudgetExceeded);
                }
            }
        }
        let bundle = SourceBundle::load(&self.json_path, &self.atlas_path, |request| {
            downloaded
                .remove(request.virtual_path())
                .ok_or_else(|| MissingDownloadedFile(request.virtual_path().to_path_buf()))
        })
        .map_err(|error| BrowserManifestError::InvalidBundle(error.to_string().into()))?;
        if let Some(unused) = downloaded.into_keys().next() {
            return Err(BrowserManifestError::UnusedFile(unused));
        }
        Ok(bundle)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestDocument {
    format_version: u32,
    source: SourceDocument,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceDocument {
    label: String,
    json: String,
    atlas: String,
    files: Vec<FileDocument>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileDocument {
    path: String,
    url: String,
    byte_length: u64,
    sha256: String,
}

/// Fail-closed browser manifest or downloaded-bundle error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BrowserManifestError {
    Invalid(Box<str>),
    DuplicatePath(PathBuf),
    DuplicateUrl(Box<str>),
    MissingDeclaredFile(PathBuf),
    DownloadedFileSetMismatch,
    FileLengthMismatch {
        path: PathBuf,
        expected: usize,
        actual: usize,
    },
    FileDigestMismatch(PathBuf),
    InvalidTexture {
        path: PathBuf,
        detail: Box<str>,
    },
    DecodedTextureBudgetExceeded,
    UnusedFile(PathBuf),
    InvalidBundle(Box<str>),
}

impl fmt::Display for BrowserManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(detail) => write!(formatter, "invalid browser manifest: {detail}"),
            Self::DuplicatePath(path) => {
                write!(
                    formatter,
                    "duplicate browser bundle path `{}`",
                    path.display()
                )
            }
            Self::DuplicateUrl(url) => write!(formatter, "duplicate browser bundle URL `{url}`"),
            Self::MissingDeclaredFile(path) => write!(
                formatter,
                "browser manifest has no file entry for `{}`",
                path.display()
            ),
            Self::DownloadedFileSetMismatch => {
                formatter.write_str("downloaded browser files do not match the manifest")
            }
            Self::FileLengthMismatch {
                path,
                expected,
                actual,
            } => write!(
                formatter,
                "browser bundle file `{}` has {actual} bytes; expected {expected}",
                path.display()
            ),
            Self::FileDigestMismatch(path) => write!(
                formatter,
                "browser bundle file `{}` failed its SHA-256 check",
                path.display()
            ),
            Self::InvalidTexture { path, detail } => write!(
                formatter,
                "invalid browser texture `{}`: {detail}",
                path.display()
            ),
            Self::DecodedTextureBudgetExceeded => write!(
                formatter,
                "browser textures exceed the {MAX_TOTAL_DECODED_BYTES}-byte decoded limit"
            ),
            Self::UnusedFile(path) => write!(
                formatter,
                "browser manifest declares unused file `{}`",
                path.display()
            ),
            Self::InvalidBundle(detail) => write!(formatter, "invalid browser bundle: {detail}"),
        }
    }
}

impl Error for BrowserManifestError {}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MissingDownloadedFile(PathBuf);

impl fmt::Display for MissingDownloadedFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "download missing for `{}`", self.0.display())
    }
}

impl Error for MissingDownloadedFile {}

fn validate_label(label: &str) -> Result<(), BrowserManifestError> {
    if label.is_empty()
        || label.len() > 128
        || label.chars().any(|character| character.is_control())
    {
        return Err(BrowserManifestError::Invalid(
            "source label must be 1-128 non-control UTF-8 bytes".into(),
        ));
    }
    Ok(())
}

fn validate_virtual_path(value: &str) -> Result<PathBuf, BrowserManifestError> {
    let invalid = value.is_empty()
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.contains('\\')
        || value.contains(':')
        || value.contains('#')
        || value.contains('?')
        || value.chars().any(|character| character.is_control())
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..");
    if invalid {
        return Err(BrowserManifestError::Invalid(
            format!("unsafe virtual browser bundle path `{value}`").into(),
        ));
    }
    Ok(PathBuf::from(value))
}

fn validate_url_reference(value: &str) -> Result<(), BrowserManifestError> {
    let invalid = value.is_empty()
        || value.len() > 2_048
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.contains(['\\', ':', '?', '#', '%'])
        || value.chars().any(|character| character.is_control())
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..");
    if invalid {
        return Err(BrowserManifestError::Invalid(
            "bundle URLs must be safe relative paths inside the manifest directory".into(),
        ));
    }
    Ok(())
}

fn parse_sha256(value: &str) -> Result<[u8; 32], BrowserManifestError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(BrowserManifestError::Invalid(
            "file SHA-256 values must be 64 lowercase hexadecimal characters".into(),
        ));
    }
    let mut digest = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
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

fn png_decoded_bytes(path: &Path, bytes: &[u8]) -> Result<usize, BrowserManifestError> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    let invalid = || {
        BrowserManifestError::InvalidTexture {
        path: path.to_path_buf(),
        detail: "expected a bounded non-animated 8-bit RGBA PNG using only approved fixed-size metadata chunks".into(),
    }
    };
    if bytes.len() < 33
        || &bytes[..8] != PNG_SIGNATURE
        || u32::from_be_bytes(bytes[8..12].try_into().expect("four-byte PNG field")) != 13
        || &bytes[12..16] != b"IHDR"
    {
        return Err(invalid());
    }
    let profile = (bytes[24], bytes[25], bytes[26], bytes[27], bytes[28]);
    if profile != (8, 6, 0, 0, 0) {
        return Err(invalid());
    }
    validate_png_chunk_profile(path, bytes)?;
    let width = u32::from_be_bytes(bytes[16..20].try_into().expect("four-byte PNG width"));
    let height = u32::from_be_bytes(bytes[20..24].try_into().expect("four-byte PNG height"));
    if width == 0 || height == 0 || width > MAX_PAGE_DIMENSION || height > MAX_PAGE_DIMENSION {
        return Err(BrowserManifestError::InvalidTexture {
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
        .ok_or_else(|| BrowserManifestError::InvalidTexture {
            path: path.to_path_buf(),
            detail: "decoded PNG size overflowed".into(),
        })?;
    if decoded > MAX_PAGE_DECODED_BYTES {
        return Err(BrowserManifestError::InvalidTexture {
            path: path.to_path_buf(),
            detail: format!(
                "decoded PNG requires {decoded} bytes; limit is {MAX_PAGE_DECODED_BYTES}"
            )
            .into(),
        });
    }
    Ok(decoded)
}

fn validate_png_chunk_profile(path: &Path, bytes: &[u8]) -> Result<(), BrowserManifestError> {
    #[derive(Clone, Copy, Eq, PartialEq)]
    enum Stage {
        Header,
        Metadata,
        ImageData,
    }

    let invalid = |detail: &'static str| BrowserManifestError::InvalidTexture {
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
        .map_err(|_error| invalid("PNG chunk length does not fit this browser"))?;
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
            b"IDAT" if matches!(stage, Stage::Metadata | Stage::ImageData) => {
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

    fn sha256_hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn manifest(extra: &str) -> Vec<u8> {
        format!(
            r#"{{
  "format_version": 1,
  "source": {{
    "label": "Generic fixture",
    "json": "rig/fixture.json",
    "atlas": "rig/fixture.atlas",
    "files": [
      {{"path":"rig/fixture.json","url":"fixture.json","byte_length":{},"sha256":"{}"}},
      {{"path":"rig/fixture.atlas","url":"fixture.atlas","byte_length":{},"sha256":"{}"}},
      {{"path":"rig/textures/page.png","url":"textures/page.png","byte_length":{},"sha256":"{}"}}
    ]
  }}{extra}
}}"#,
            JSON.len(),
            sha256_hex(JSON),
            ATLAS.len(),
            sha256_hex(ATLAS),
            PNG.len(),
            sha256_hex(PNG),
        )
        .into_bytes()
    }

    fn downloads() -> BTreeMap<PathBuf, Vec<u8>> {
        BTreeMap::from([
            (PathBuf::from("rig/fixture.json"), JSON.to_vec()),
            (PathBuf::from("rig/fixture.atlas"), ATLAS.to_vec()),
            (PathBuf::from("rig/textures/page.png"), PNG.to_vec()),
        ])
    }

    fn with_chunk_after_header(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut chunk = Vec::with_capacity(payload.len() + 12);
        chunk.extend_from_slice(
            &u32::try_from(payload.len())
                .expect("test PNG chunk length")
                .to_be_bytes(),
        );
        chunk.extend_from_slice(kind);
        chunk.extend_from_slice(payload);
        chunk.extend_from_slice(&[0; 4]);

        let mut png = PNG.to_vec();
        png.splice(33..33, chunk);
        png
    }

    #[test]
    fn exact_manifest_builds_the_shared_immutable_bundle() {
        let manifest = BrowserManifest::parse(&manifest("")).expect("valid manifest");
        assert_eq!(manifest.label(), "Generic fixture");
        assert_eq!(manifest.files().len(), 3);
        assert_eq!(
            manifest.files()[0].virtual_path(),
            Path::new("rig/fixture.json")
        );
        assert_eq!(manifest.files()[0].url_reference(), "fixture.json");
        assert_eq!(manifest.files()[0].max_bytes(), MAX_JSON_BYTES);
        assert_eq!(manifest.files()[0].expected_bytes(), JSON.len());
        assert_eq!(
            manifest.files()[0].expected_sha256,
            <[u8; 32]>::from(Sha256::digest(JSON))
        );
        let bundle = manifest.into_bundle(downloads()).expect("valid bundle");
        assert_eq!(bundle.json_asset_path(), Path::new("rig/fixture.json"));
        assert_eq!(bundle.atlas_reference(), "fixture.atlas");
    }

    #[test]
    fn schema_is_exact_and_versioned() {
        let mut wrong_version = manifest("");
        let index = wrong_version
            .windows(b"\"format_version\": 1".len())
            .position(|window| window == b"\"format_version\": 1")
            .expect("version field");
        let digit = index + b"\"format_version\": ".len();
        wrong_version[digit] = b'2';
        assert!(
            BrowserManifest::parse(&wrong_version)
                .expect_err("wrong version")
                .to_string()
                .contains("unsupported")
        );
        assert!(
            BrowserManifest::parse(&manifest(",\n  \"unknown\": true"))
                .expect_err("unknown field")
                .to_string()
                .contains("unknown field")
        );
    }

    #[test]
    fn unsafe_duplicate_missing_and_unused_entries_fail_closed() {
        let unsafe_path = manifest("").into_iter().collect::<Vec<_>>();
        let unsafe_path = String::from_utf8(unsafe_path)
            .expect("fixture UTF-8")
            .replace("rig/textures/page.png", "../page.png");
        assert!(BrowserManifest::parse(unsafe_path.as_bytes()).is_err());

        let duplicate = String::from_utf8(manifest(""))
            .expect("fixture UTF-8")
            .replace("rig/textures/page.png", "rig/fixture.json");
        assert!(matches!(
            BrowserManifest::parse(duplicate.as_bytes()),
            Err(BrowserManifestError::DuplicatePath(_))
        ));

        let parsed = BrowserManifest::parse(&manifest("")).expect("valid manifest");
        let mut missing = downloads();
        missing.remove(Path::new("rig/textures/page.png"));
        assert!(matches!(
            parsed.clone().into_bundle(missing),
            Err(BrowserManifestError::DownloadedFileSetMismatch)
        ));

        let mut extra = downloads();
        extra.insert(PathBuf::from("unlisted.png"), vec![9]);
        assert!(matches!(
            parsed.into_bundle(extra),
            Err(BrowserManifestError::DownloadedFileSetMismatch)
        ));

        for unsafe_url in ["/fixture.json", "../fixture.json", "%2e%2e/fixture.json"] {
            let document = String::from_utf8(manifest(""))
                .expect("fixture UTF-8")
                .replacen(
                    "\"url\":\"fixture.json\"",
                    &format!("\"url\":\"{unsafe_url}\""),
                    1,
                );
            assert!(BrowserManifest::parse(document.as_bytes()).is_err());
        }
    }

    #[test]
    fn downloaded_lengths_hashes_and_png_dimensions_fail_closed() {
        let parsed = BrowserManifest::parse(&manifest("")).expect("valid manifest");

        let mut wrong_length = downloads();
        wrong_length
            .get_mut(Path::new("rig/fixture.json"))
            .expect("JSON fixture")
            .push(b' ');
        assert!(matches!(
            parsed.clone().into_bundle(wrong_length),
            Err(BrowserManifestError::FileLengthMismatch { .. })
        ));

        let mut wrong_digest = downloads();
        wrong_digest
            .get_mut(Path::new("rig/fixture.json"))
            .expect("JSON fixture")[0] ^= 1;
        assert!(matches!(
            parsed.into_bundle(wrong_digest),
            Err(BrowserManifestError::FileDigestMismatch(path))
                if path == Path::new("rig/fixture.json")
        ));

        let mut oversized_header = PNG.to_vec();
        oversized_header[16..20].copy_from_slice(&(MAX_PAGE_DIMENSION + 1).to_be_bytes());
        assert!(matches!(
            png_decoded_bytes(Path::new("page.png"), &oversized_header),
            Err(BrowserManifestError::InvalidTexture { .. })
        ));
        for (field, invalid_value) in [(24, 16), (25, 2), (26, 1), (27, 1), (28, 1)] {
            let mut unsupported_profile = PNG.to_vec();
            unsupported_profile[field] = invalid_value;
            assert!(matches!(
                png_decoded_bytes(Path::new("page.png"), &unsupported_profile),
                Err(BrowserManifestError::InvalidTexture { .. })
            ));
        }
        assert_eq!(png_decoded_bytes(Path::new("page.png"), PNG), Ok(4));
    }

    #[test]
    fn compressed_metadata_animation_and_unknown_png_chunks_fail_before_decode() {
        for chunk in [
            b"iCCP", b"zTXt", b"iTXt", b"acTL", b"fcTL", b"fdAT", b"tEXt", b"vpAg",
        ] {
            let png = with_chunk_after_header(chunk, b"compressed-or-unbounded");
            let error = png_decoded_bytes(Path::new("page.png"), &png)
                .expect_err("unapproved PNG chunk must fail closed");
            assert!(error.to_string().contains("unsupported"));
        }

        let mut trailing = PNG.to_vec();
        trailing.push(0);
        assert!(png_decoded_bytes(Path::new("page.png"), &trailing).is_err());

        let mut truncated = PNG.to_vec();
        truncated.truncate(40);
        assert!(png_decoded_bytes(Path::new("page.png"), &truncated).is_err());
    }

    #[test]
    fn digest_and_declared_length_metadata_are_canonical_and_bounded() {
        let uppercase_digest = String::from_utf8(manifest(""))
            .expect("fixture UTF-8")
            .replacen(&sha256_hex(JSON), &sha256_hex(JSON).to_uppercase(), 1);
        assert!(BrowserManifest::parse(uppercase_digest.as_bytes()).is_err());

        let oversized_json = String::from_utf8(manifest(""))
            .expect("fixture UTF-8")
            .replacen(
                &format!("\"byte_length\":{}", JSON.len()),
                &format!("\"byte_length\":{}", MAX_JSON_BYTES + 1),
                1,
            );
        assert!(BrowserManifest::parse(oversized_json.as_bytes()).is_err());
    }

    #[test]
    fn manifest_and_file_limits_are_fixed() {
        assert!(BrowserManifest::parse(&vec![b' '; MAX_MANIFEST_BYTES + 1]).is_err());
        let parsed = BrowserManifest::parse(&manifest("")).expect("valid manifest");
        assert_eq!(parsed.files()[1].max_bytes(), MAX_ATLAS_BYTES);
        assert_eq!(parsed.files()[2].max_bytes(), MAX_PAGE_BYTES);
        let total_limit = std::hint::black_box(MAX_BROWSER_BUNDLE_BYTES);
        assert!(total_limit >= parsed.files()[2].max_bytes());
    }
}

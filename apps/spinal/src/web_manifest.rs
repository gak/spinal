//! Browser adapters for immutable viewer launches and runtime bundles.

use std::{collections::BTreeMap, error::Error, fmt, path::PathBuf};

use bevy_spinal::spinal::{
    MAX_RUNTIME_BUNDLE_BYTES, MAX_RUNTIME_DECODED_TEXTURE_BYTES, MAX_RUNTIME_FILE_COUNT,
    MAX_RUNTIME_MANIFEST_BYTES, RuntimeBundleError, RuntimeBundleFile, RuntimeBundleManifest,
    parse_runtime_bundle_sha256, validate_runtime_bundle_location_reference,
};
use serde::Deserialize;

use crate::bundle::SourceBundle;

pub(crate) const MAX_MANIFEST_BYTES: usize = MAX_RUNTIME_MANIFEST_BYTES;
pub(crate) const MAX_LAUNCH_MANIFEST_BYTES: usize = MAX_RUNTIME_MANIFEST_BYTES;
pub(crate) const MAX_BROWSER_BUNDLE_BYTES: usize = MAX_RUNTIME_BUNDLE_BYTES;
pub(crate) type BrowserManifestError = bevy_spinal::spinal::RuntimeBundleError;
const LAUNCH_MANIFEST_FORMAT_VERSION: u32 = 1;

/// Browser-facing adapter over the one native/browser manifest implementation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserManifest(RuntimeBundleManifest);

impl BrowserManifest {
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, BrowserManifestError> {
        RuntimeBundleManifest::parse(bytes).map(Self)
    }

    pub(crate) fn label(&self) -> &str {
        self.0.label()
    }

    pub(crate) fn files(&self) -> &[RuntimeBundleFile] {
        self.0.files()
    }

    pub(crate) fn file_count(&self) -> usize {
        self.0.file_count()
    }

    pub(crate) const fn encoded_bytes(&self) -> usize {
        self.0.encoded_bytes()
    }

    pub(crate) fn into_bundle(
        self,
        downloaded: BTreeMap<PathBuf, Vec<u8>>,
    ) -> Result<SourceBundle, BrowserManifestError> {
        let validated = self.0.validate(downloaded)?;
        Ok(SourceBundle::from_validated(validated))
    }
}

/// One exact child runtime-manifest reference in a browser launch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserManifestReference {
    location_reference: Box<str>,
    expected_bytes: usize,
    expected_sha256: Box<str>,
}

impl BrowserManifestReference {
    pub(crate) fn location_reference(&self) -> &str {
        &self.location_reference
    }

    pub(crate) const fn expected_bytes(&self) -> usize {
        self.expected_bytes
    }

    #[cfg(test)]
    pub(crate) fn expected_sha256(&self) -> &str {
        &self.expected_sha256
    }
}

/// Strict versioned launch manifest for one primary and optional comparison.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserLaunchManifest {
    primary: BrowserManifestReference,
    comparison: Option<BrowserManifestReference>,
}

impl BrowserLaunchManifest {
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, BrowserLaunchManifestError> {
        if bytes.is_empty() {
            return Err(BrowserLaunchManifestError::InvalidManifest(
                "the browser launch manifest is empty".into(),
            ));
        }
        if bytes.len() > MAX_LAUNCH_MANIFEST_BYTES {
            return Err(BrowserLaunchManifestError::InvalidManifest(
                format!(
                    "the browser launch manifest exceeds the {MAX_LAUNCH_MANIFEST_BYTES}-byte limit"
                )
                .into(),
            ));
        }
        let document: LaunchManifestDocument = serde_json::from_slice(bytes).map_err(|error| {
            BrowserLaunchManifestError::InvalidManifest(error.to_string().into())
        })?;
        if document.format_version != LAUNCH_MANIFEST_FORMAT_VERSION {
            return Err(BrowserLaunchManifestError::InvalidManifest(
                format!(
                    "unsupported browser launch manifest version {}; expected {LAUNCH_MANIFEST_FORMAT_VERSION}",
                    document.format_version
                )
                .into(),
            ));
        }
        Ok(Self {
            primary: parse_manifest_reference(document.primary)?,
            comparison: document
                .comparison
                .map(parse_manifest_reference)
                .transpose()?,
        })
    }

    pub(crate) const fn primary(&self) -> &BrowserManifestReference {
        &self.primary
    }

    pub(crate) const fn comparison(&self) -> Option<&BrowserManifestReference> {
        self.comparison.as_ref()
    }

    /// Authenticates and parses both child manifests, then applies the shared
    /// file-count and encoded-byte budgets to the whole launch.
    pub(crate) fn validate_runtime_manifests(
        &self,
        primary_bytes: &[u8],
        comparison_bytes: Option<&[u8]>,
    ) -> Result<BrowserLaunchManifests, BrowserLaunchManifestError> {
        let primary = validate_child_manifest("primary", &self.primary, primary_bytes)?;
        let comparison = match (&self.comparison, comparison_bytes) {
            (Some(reference), Some(bytes)) => {
                Some(validate_child_manifest("comparison", reference, bytes)?)
            }
            (Some(_reference), None) => {
                return Err(BrowserLaunchManifestError::MissingComparisonManifest);
            }
            (None, Some(_bytes)) => {
                return Err(BrowserLaunchManifestError::UnexpectedComparisonManifest);
            }
            (None, None) => None,
        };
        BrowserLaunchManifests::validate(primary, comparison)
    }
}

/// Authenticated child manifests whose aggregate declared footprint is valid.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BrowserLaunchManifests {
    primary: BrowserManifest,
    comparison: Option<BrowserManifest>,
}

impl BrowserLaunchManifests {
    fn validate(
        primary: BrowserManifest,
        comparison: Option<BrowserManifest>,
    ) -> Result<Self, BrowserLaunchManifestError> {
        validate_aggregate_budget(
            [Some(&primary), comparison.as_ref()]
                .into_iter()
                .flatten()
                .map(|manifest| BundleFootprint {
                    file_count: manifest.file_count(),
                    encoded_bytes: manifest.encoded_bytes(),
                    decoded_texture_bytes: 0,
                }),
            false,
        )?;
        Ok(Self {
            primary,
            comparison,
        })
    }

    #[cfg(test)]
    pub(crate) const fn primary(&self) -> &BrowserManifest {
        &self.primary
    }

    #[cfg(test)]
    pub(crate) const fn comparison(&self) -> Option<&BrowserManifest> {
        self.comparison.as_ref()
    }

    pub(crate) fn into_parts(self) -> (BrowserManifest, Option<BrowserManifest>) {
        (self.primary, self.comparison)
    }
}

/// Validated source snapshots whose whole-launch footprint fits global limits.
#[derive(Clone, Debug)]
pub(crate) struct BrowserLaunchBundles {
    primary: SourceBundle,
    comparison: Option<SourceBundle>,
}

impl BrowserLaunchBundles {
    pub(crate) fn validate(
        primary: SourceBundle,
        comparison: Option<SourceBundle>,
    ) -> Result<Self, BrowserLaunchManifestError> {
        validate_aggregate_budget(
            [Some(&primary), comparison.as_ref()]
                .into_iter()
                .flatten()
                .map(|bundle| BundleFootprint {
                    file_count: bundle.file_count(),
                    encoded_bytes: bundle.encoded_bytes(),
                    decoded_texture_bytes: bundle.decoded_texture_bytes(),
                }),
            true,
        )?;
        Ok(Self {
            primary,
            comparison,
        })
    }

    pub(crate) fn into_parts(self) -> (SourceBundle, Option<SourceBundle>) {
        (self.primary, self.comparison)
    }
}

/// Failure at the strict browser launch-manifest or aggregate-budget boundary.
#[derive(Debug)]
pub(crate) enum BrowserLaunchManifestError {
    InvalidManifest(Box<str>),
    RuntimeManifestLengthMismatch {
        role: &'static str,
        expected: usize,
        actual: usize,
    },
    RuntimeManifestDigestMismatch {
        role: &'static str,
    },
    InvalidRuntimeManifest {
        role: &'static str,
        source: RuntimeBundleError,
    },
    MissingComparisonManifest,
    UnexpectedComparisonManifest,
    AggregateFileBudgetExceeded,
    AggregateEncodedBudgetExceeded,
    AggregateDecodedBudgetExceeded,
}

impl fmt::Display for BrowserLaunchManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidManifest(detail) => {
                write!(formatter, "invalid browser launch manifest: {detail}")
            }
            Self::RuntimeManifestLengthMismatch {
                role,
                expected,
                actual,
            } => write!(
                formatter,
                "{role} runtime manifest has {actual} bytes; expected {expected}"
            ),
            Self::RuntimeManifestDigestMismatch { role } => {
                write!(
                    formatter,
                    "{role} runtime manifest failed its SHA-256 check"
                )
            }
            Self::InvalidRuntimeManifest { role, source } => {
                write!(formatter, "invalid {role} runtime manifest: {source}")
            }
            Self::MissingComparisonManifest => {
                formatter.write_str("the comparison runtime manifest is missing")
            }
            Self::UnexpectedComparisonManifest => {
                formatter.write_str("an undeclared comparison runtime manifest was supplied")
            }
            Self::AggregateFileBudgetExceeded => write!(
                formatter,
                "viewer bundles exceed the {MAX_RUNTIME_FILE_COUNT}-file total limit"
            ),
            Self::AggregateEncodedBudgetExceeded => write!(
                formatter,
                "viewer bundles exceed the {MAX_RUNTIME_BUNDLE_BYTES}-byte encoded total limit"
            ),
            Self::AggregateDecodedBudgetExceeded => write!(
                formatter,
                "viewer bundles exceed the {MAX_RUNTIME_DECODED_TEXTURE_BYTES}-byte decoded texture total limit"
            ),
        }
    }
}

impl Error for BrowserLaunchManifestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidRuntimeManifest { source, .. } => Some(source),
            _other => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LaunchManifestDocument {
    format_version: u32,
    primary: ManifestReferenceDocument,
    #[serde(default)]
    comparison: Option<ManifestReferenceDocument>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestReferenceDocument {
    url: String,
    byte_length: u64,
    sha256: String,
}

fn parse_manifest_reference(
    document: ManifestReferenceDocument,
) -> Result<BrowserManifestReference, BrowserLaunchManifestError> {
    validate_manifest_location_reference(&document.url)?;
    let expected_bytes = usize::try_from(document.byte_length).map_err(|_error| {
        BrowserLaunchManifestError::InvalidManifest(
            "runtime manifest byte length does not fit this host".into(),
        )
    })?;
    if expected_bytes == 0 || expected_bytes > MAX_MANIFEST_BYTES {
        return Err(BrowserLaunchManifestError::InvalidManifest(
            format!("runtime manifest byte length must be 1-{MAX_MANIFEST_BYTES}").into(),
        ));
    }
    validate_sha256(&document.sha256)?;
    Ok(BrowserManifestReference {
        location_reference: document.url.into(),
        expected_bytes,
        expected_sha256: document.sha256.into(),
    })
}

fn validate_child_manifest(
    role: &'static str,
    reference: &BrowserManifestReference,
    bytes: &[u8],
) -> Result<BrowserManifest, BrowserLaunchManifestError> {
    if bytes.len() != reference.expected_bytes {
        return Err(BrowserLaunchManifestError::RuntimeManifestLengthMismatch {
            role,
            expected: reference.expected_bytes,
            actual: bytes.len(),
        });
    }
    let manifest = BrowserManifest::parse(bytes)
        .map_err(|source| BrowserLaunchManifestError::InvalidRuntimeManifest { role, source })?;
    if manifest.0.manifest_sha256() != reference.expected_sha256.as_ref() {
        return Err(BrowserLaunchManifestError::RuntimeManifestDigestMismatch { role });
    }
    Ok(manifest)
}

pub(crate) fn validate_manifest_location_reference(
    value: &str,
) -> Result<(), BrowserLaunchManifestError> {
    validate_runtime_bundle_location_reference(value).map_err(|_error| {
        BrowserLaunchManifestError::InvalidManifest(
            "runtime manifest locations must be safe relative paths".into(),
        )
    })
}

fn validate_sha256(value: &str) -> Result<(), BrowserLaunchManifestError> {
    parse_runtime_bundle_sha256(value)
        .map(|_digest| ())
        .map_err(|_error| {
            BrowserLaunchManifestError::InvalidManifest(
                "runtime manifest SHA-256 values must be 64 lowercase hexadecimal characters"
                    .into(),
            )
        })
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct BundleFootprint {
    file_count: usize,
    encoded_bytes: usize,
    decoded_texture_bytes: usize,
}

fn validate_aggregate_budget(
    footprints: impl IntoIterator<Item = BundleFootprint>,
    check_decoded: bool,
) -> Result<(), BrowserLaunchManifestError> {
    let mut total = BundleFootprint::default();
    for footprint in footprints {
        total.file_count = total
            .file_count
            .checked_add(footprint.file_count)
            .ok_or(BrowserLaunchManifestError::AggregateFileBudgetExceeded)?;
        total.encoded_bytes = total
            .encoded_bytes
            .checked_add(footprint.encoded_bytes)
            .ok_or(BrowserLaunchManifestError::AggregateEncodedBudgetExceeded)?;
        total.decoded_texture_bytes = total
            .decoded_texture_bytes
            .checked_add(footprint.decoded_texture_bytes)
            .ok_or(BrowserLaunchManifestError::AggregateDecodedBudgetExceeded)?;
    }
    if total.file_count > MAX_RUNTIME_FILE_COUNT {
        return Err(BrowserLaunchManifestError::AggregateFileBudgetExceeded);
    }
    if total.encoded_bytes > MAX_RUNTIME_BUNDLE_BYTES {
        return Err(BrowserLaunchManifestError::AggregateEncodedBudgetExceeded);
    }
    if check_decoded && total.decoded_texture_bytes > MAX_RUNTIME_DECODED_TEXTURE_BYTES {
        return Err(BrowserLaunchManifestError::AggregateDecodedBudgetExceeded);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use sha2::{Digest, Sha256};

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

    fn manifest(page: &[u8], json_path: &str) -> Vec<u8> {
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

    fn downloads(page: &[u8]) -> BTreeMap<PathBuf, Vec<u8>> {
        BTreeMap::from([
            (PathBuf::from("rig/fixture.json"), JSON.to_vec()),
            (PathBuf::from("rig/fixture.atlas"), ATLAS.to_vec()),
            (PathBuf::from("rig/textures/page.png"), page.to_vec()),
        ])
    }

    fn launch_manifest(
        primary_url: &str,
        primary: &[u8],
        comparison: Option<(&str, &[u8])>,
    ) -> Vec<u8> {
        let mut document = serde_json::json!({
            "format_version": 1,
            "primary": {
                "url": primary_url,
                "byte_length": primary.len(),
                "sha256": sha256_hex(primary),
            }
        });
        if let Some((url, bytes)) = comparison {
            document["comparison"] = serde_json::json!({
                "url": url,
                "byte_length": bytes.len(),
                "sha256": sha256_hex(bytes),
            });
        }
        serde_json::to_vec(&document).expect("launch manifest JSON")
    }

    fn declared_manifest(prefix: &str, lengths: &[usize]) -> Vec<u8> {
        assert!(lengths.len() >= 2);
        let files = lengths
            .iter()
            .enumerate()
            .map(|(index, length)| {
                let extension = match index {
                    0 => "json",
                    1 => "atlas",
                    _other => "png",
                };
                serde_json::json!({
                    "path": format!("{prefix}/file-{index}.{extension}"),
                    "url": format!("file-{index}.{extension}"),
                    "byte_length": length,
                    "sha256": "0".repeat(64),
                })
            })
            .collect::<Vec<_>>();
        serde_json::to_vec(&serde_json::json!({
            "format_version": 1,
            "source": {
                "label": prefix,
                "json": format!("{prefix}/file-0.json"),
                "atlas": format!("{prefix}/file-1.atlas"),
                "files": files,
            }
        }))
        .expect("declared runtime manifest JSON")
    }

    #[test]
    fn browser_uses_the_shared_manifest_and_bundle_result() {
        let parsed =
            BrowserManifest::parse(&manifest(PNG, "rig/fixture.json")).expect("valid manifest");
        assert_eq!(parsed.label(), "Generic fixture");
        assert_eq!(parsed.files().len(), 3);
        assert_eq!(
            parsed.files()[0].virtual_path(),
            Path::new("rig/fixture.json")
        );
        assert_eq!(parsed.files()[0].location_reference(), "fixture.json");
        assert_eq!(parsed.files()[0].expected_bytes(), JSON.len());
        assert_eq!(parsed.files()[0].expected_sha256(), sha256_hex(JSON));
        let bundle = parsed.into_bundle(downloads(PNG)).expect("valid bundle");
        assert_eq!(bundle.json_asset_path(), Path::new("rig/fixture.json"));
        assert_eq!(bundle.atlas_reference(), "fixture.atlas");
    }

    #[test]
    fn launch_manifest_authenticates_one_or_two_canonical_child_manifests() {
        let primary_bytes = manifest(PNG, "rig/fixture.json");
        let comparison_bytes = manifest(PNG, "rig/fixture.json");
        let launch_bytes = launch_manifest(
            "primary/manifest.json",
            &primary_bytes,
            Some(("comparison/manifest.json", &comparison_bytes)),
        );
        let launch = BrowserLaunchManifest::parse(&launch_bytes).expect("strict launch manifest");
        assert_eq!(
            launch.primary().location_reference(),
            "primary/manifest.json"
        );
        assert_eq!(launch.primary().expected_bytes(), primary_bytes.len());
        assert_eq!(
            launch.primary().expected_sha256(),
            sha256_hex(&primary_bytes)
        );
        assert_eq!(
            launch
                .comparison()
                .expect("comparison reference")
                .location_reference(),
            "comparison/manifest.json"
        );

        let manifests = launch
            .validate_runtime_manifests(&primary_bytes, Some(&comparison_bytes))
            .expect("authenticated child manifests within one aggregate budget");
        assert_eq!(manifests.primary().file_count(), 3);
        assert_eq!(
            manifests.comparison().expect("comparison").encoded_bytes(),
            JSON.len() + ATLAS.len() + PNG.len()
        );
        let (primary, comparison) = manifests.into_parts();
        let primary = primary.into_bundle(downloads(PNG)).expect("primary bundle");
        let comparison = comparison
            .expect("comparison manifest")
            .into_bundle(downloads(PNG))
            .expect("comparison bundle");
        let bundles = BrowserLaunchBundles::validate(primary, Some(comparison))
            .expect("exact aggregate footprint");
        let (primary, comparison) = bundles.into_parts();
        assert_eq!(primary.file_count(), 3);
        assert_eq!(
            comparison
                .expect("comparison bundle")
                .decoded_texture_bytes(),
            4
        );

        let single_bytes = launch_manifest("manifest.json", &primary_bytes, None);
        let single = BrowserLaunchManifest::parse(&single_bytes).expect("single-source launch");
        assert!(single.comparison().is_none());
        assert!(
            single
                .validate_runtime_manifests(&primary_bytes, None)
                .is_ok()
        );
    }

    #[test]
    fn launch_schema_rejects_unknown_missing_and_wrong_version_fields() {
        let digest = "0".repeat(64);
        for document in [
            format!(
                r#"{{"format_version":1,"primary":{{"url":"manifest.json","byte_length":1,"sha256":"{digest}"}},"tertiary":{{}}}}"#
            ),
            r#"{"format_version":1}"#.to_owned(),
            format!(
                r#"{{"format_version":2,"primary":{{"url":"manifest.json","byte_length":1,"sha256":"{digest}"}}}}"#
            ),
            format!(
                r#"{{"format_version":1,"primary":{{"url":"manifest.json","byte_length":1,"sha256":"{digest}","label":"not allowed"}}}}"#
            ),
        ] {
            assert!(BrowserLaunchManifest::parse(document.as_bytes()).is_err());
        }
        assert!(BrowserLaunchManifest::parse(&[]).is_err());
        assert!(BrowserLaunchManifest::parse(&vec![b' '; MAX_LAUNCH_MANIFEST_BYTES + 1]).is_err());
    }

    #[test]
    fn launch_references_require_safe_relative_urls_lengths_and_lowercase_digests() {
        let child = manifest(PNG, "rig/fixture.json");
        for url in [
            "",
            "../manifest.json",
            "/manifest.json",
            "\\\\server\\manifest.json",
            "C:/manifest.json",
            "a//manifest.json",
            "./manifest.json",
            "a/./manifest.json",
            "a/../manifest.json",
            "https://example.invalid/manifest.json",
            "manifest.json?x=1",
            "manifest.json#x",
            "manifest%2ejson",
            "manifest\n.json",
        ] {
            assert!(
                BrowserLaunchManifest::parse(&launch_manifest(url, &child, None)).is_err(),
                "unsafe URL was admitted: {url:?}"
            );
        }

        let valid = String::from_utf8(launch_manifest("manifest.json", &child, None))
            .expect("UTF-8 launch manifest");
        let zero = valid.replace(
            &format!(r#""byte_length":{}"#, child.len()),
            r#""byte_length":0"#,
        );
        assert!(BrowserLaunchManifest::parse(zero.as_bytes()).is_err());
        let too_large = valid.replace(
            &format!(r#""byte_length":{}"#, child.len()),
            &format!(r#""byte_length":{}"#, MAX_MANIFEST_BYTES + 1),
        );
        assert!(BrowserLaunchManifest::parse(too_large.as_bytes()).is_err());
        let uppercase = valid.replace(&sha256_hex(&child), &sha256_hex(&child).to_uppercase());
        assert!(BrowserLaunchManifest::parse(uppercase.as_bytes()).is_err());
        let short = valid.replace(&sha256_hex(&child), &"0".repeat(63));
        assert!(BrowserLaunchManifest::parse(short.as_bytes()).is_err());
        let non_hex = valid.replace(&sha256_hex(&child), &"g".repeat(64));
        assert!(BrowserLaunchManifest::parse(non_hex.as_bytes()).is_err());
    }

    #[test]
    fn child_manifest_bytes_must_match_exact_reference_and_presence() {
        let child = manifest(PNG, "rig/fixture.json");
        let launch = BrowserLaunchManifest::parse(&launch_manifest(
            "primary.json",
            &child,
            Some(("comparison.json", &child)),
        ))
        .expect("launch manifest");
        assert!(matches!(
            launch.validate_runtime_manifests(&child[..child.len() - 1], Some(&child)),
            Err(BrowserLaunchManifestError::RuntimeManifestLengthMismatch {
                role: "primary",
                ..
            })
        ));

        let mut changed = child.clone();
        let index = changed.len() - 2;
        changed[index] = if changed[index] == b' ' { b'\n' } else { b' ' };
        assert_eq!(changed.len(), child.len());
        assert!(matches!(
            launch.validate_runtime_manifests(&changed, Some(&child)),
            Err(BrowserLaunchManifestError::RuntimeManifestDigestMismatch { role: "primary" })
        ));
        assert!(matches!(
            launch.validate_runtime_manifests(&child, None),
            Err(BrowserLaunchManifestError::MissingComparisonManifest)
        ));

        let single = BrowserLaunchManifest::parse(&launch_manifest("primary.json", &child, None))
            .expect("single-source launch manifest");
        assert!(matches!(
            single.validate_runtime_manifests(&child, Some(&child)),
            Err(BrowserLaunchManifestError::UnexpectedComparisonManifest)
        ));
    }

    #[test]
    fn declared_budgets_are_global_across_both_child_manifests() {
        let many_primary = declared_manifest("primary", &vec![1; 65]);
        let many_comparison = declared_manifest("comparison", &vec![1; 65]);
        let launch = BrowserLaunchManifest::parse(&launch_manifest(
            "primary.json",
            &many_primary,
            Some(("comparison.json", &many_comparison)),
        ))
        .expect("launch manifest");
        assert!(matches!(
            launch.validate_runtime_manifests(&many_primary, Some(&many_comparison)),
            Err(BrowserLaunchManifestError::AggregateFileBudgetExceeded)
        ));

        let large_lengths = [16 * 1024 * 1024, 2 * 1024 * 1024, 16 * 1024 * 1024];
        let large_primary = declared_manifest("primary", &large_lengths);
        let large_comparison = declared_manifest("comparison", &large_lengths);
        let launch = BrowserLaunchManifest::parse(&launch_manifest(
            "primary.json",
            &large_primary,
            Some(("comparison.json", &large_comparison)),
        ))
        .expect("launch manifest");
        assert!(matches!(
            launch.validate_runtime_manifests(&large_primary, Some(&large_comparison)),
            Err(BrowserLaunchManifestError::AggregateEncodedBudgetExceeded)
        ));
    }

    #[test]
    fn exact_aggregate_budget_math_rejects_limits_plus_one_and_overflow() {
        let exact = BundleFootprint {
            file_count: MAX_RUNTIME_FILE_COUNT,
            encoded_bytes: MAX_RUNTIME_BUNDLE_BYTES,
            decoded_texture_bytes: MAX_RUNTIME_DECODED_TEXTURE_BYTES,
        };
        assert!(validate_aggregate_budget([exact], true).is_ok());

        for (footprint, expected) in [
            (
                BundleFootprint {
                    file_count: MAX_RUNTIME_FILE_COUNT + 1,
                    ..BundleFootprint::default()
                },
                0,
            ),
            (
                BundleFootprint {
                    encoded_bytes: MAX_RUNTIME_BUNDLE_BYTES + 1,
                    ..BundleFootprint::default()
                },
                1,
            ),
            (
                BundleFootprint {
                    decoded_texture_bytes: MAX_RUNTIME_DECODED_TEXTURE_BYTES + 1,
                    ..BundleFootprint::default()
                },
                2,
            ),
        ] {
            let error = validate_aggregate_budget([footprint], true).expect_err("over budget");
            assert!(match (expected, error) {
                (0, BrowserLaunchManifestError::AggregateFileBudgetExceeded)
                | (1, BrowserLaunchManifestError::AggregateEncodedBudgetExceeded)
                | (2, BrowserLaunchManifestError::AggregateDecodedBudgetExceeded) => true,
                _other => false,
            });
        }

        assert!(matches!(
            validate_aggregate_budget(
                [
                    BundleFootprint {
                        encoded_bytes: usize::MAX,
                        ..BundleFootprint::default()
                    },
                    BundleFootprint {
                        encoded_bytes: 1,
                        ..BundleFootprint::default()
                    },
                ],
                true
            ),
            Err(BrowserLaunchManifestError::AggregateEncodedBudgetExceeded)
        ));
    }

    #[test]
    fn browser_rejects_unsafe_paths_through_the_shared_contract() {
        for path in ["../fixture.json", "/fixture.json", "C:/fixture.json"] {
            assert!(BrowserManifest::parse(&manifest(PNG, path)).is_err());
        }
    }

    #[test]
    fn browser_rejects_file_set_and_digest_mismatches_through_the_shared_contract() {
        let parsed = BrowserManifest::parse(&manifest(PNG, "rig/fixture.json")).expect("manifest");
        let mut missing = downloads(PNG);
        missing.remove(Path::new("rig/textures/page.png"));
        assert!(matches!(
            parsed.clone().into_bundle(missing),
            Err(BrowserManifestError::FileSetMismatch)
        ));

        let mut changed = downloads(PNG);
        changed
            .get_mut(Path::new("rig/fixture.json"))
            .expect("JSON")[0] ^= 1;
        assert!(matches!(
            parsed.into_bundle(changed),
            Err(BrowserManifestError::FileDigestMismatch(_))
        ));
    }

    #[test]
    fn browser_rejects_arbitrary_and_corrupt_images_through_the_shared_contract() {
        let arbitrary = b"not a PNG";
        let parsed = BrowserManifest::parse(&manifest(arbitrary, "rig/fixture.json"))
            .expect("bounded manifest");
        assert!(matches!(
            parsed.into_bundle(downloads(arbitrary)),
            Err(BrowserManifestError::InvalidTexture { .. })
        ));

        let mut corrupt = PNG.to_vec();
        corrupt[50] ^= 1;
        let parsed = BrowserManifest::parse(&manifest(&corrupt, "rig/fixture.json"))
            .expect("content-bound manifest");
        assert!(matches!(
            parsed.into_bundle(downloads(&corrupt)),
            Err(BrowserManifestError::InvalidTexture { .. })
        ));
    }

    #[test]
    fn browser_limits_are_the_shared_fixed_limits() {
        assert!(BrowserManifest::parse(&vec![b' '; MAX_MANIFEST_BYTES + 1]).is_err());
        let parsed = BrowserManifest::parse(&manifest(PNG, "rig/fixture.json")).expect("manifest");
        assert!(MAX_BROWSER_BUNDLE_BYTES >= parsed.files()[2].max_bytes());
    }

    #[cfg(feature = "native")]
    #[test]
    fn native_and_browser_acquisition_have_identical_content_identity() {
        use std::fs;

        use crate::source::{Options, ParseResult, PreparedSource};

        let directory = tempfile::tempdir().expect("temporary native bundle root");
        let rig = directory.path().join("rig");
        fs::create_dir_all(rig.join("textures")).expect("fixture directories");
        let json_path = rig.join("fixture.json");
        let atlas_path = rig.join("fixture.atlas");
        fs::write(&json_path, JSON).expect("fixture JSON");
        fs::write(&atlas_path, ATLAS).expect("fixture atlas");
        fs::write(rig.join("textures/page.png"), PNG).expect("fixture page");

        let ParseResult::Run(options) = Options::parse([
            json_path.display().to_string(),
            "--atlas".to_owned(),
            atlas_path.display().to_string(),
            "--bundle-root".to_owned(),
            directory.path().display().to_string(),
        ])
        .expect("native fixture arguments") else {
            panic!("expected native run options");
        };
        let native = PreparedSource::load(options).expect("native validated bundle");
        let browser = BrowserManifest::parse(&manifest(PNG, "rig/fixture.json"))
            .expect("browser manifest")
            .into_bundle(downloads(PNG))
            .expect("browser validated bundle");

        assert_eq!(
            native.bundle().provenance().content_sha256(),
            browser.provenance().content_sha256()
        );
        assert_ne!(
            native.bundle().provenance().manifest_sha256(),
            browser.provenance().manifest_sha256()
        );
        assert_eq!(
            native.bundle().file_paths().collect::<Vec<_>>(),
            browser.file_paths().collect::<Vec<_>>()
        );
    }
}

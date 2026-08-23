//! Native evidence over the exact manifest and files accepted by the browser.

use crate::digest::sha256_bytes;
use serde::Serialize;
#[cfg(test)]
use spinal::RuntimeBundleManifest;
use spinal::{DiagnosticSeverity, RuntimeBundleError, ValidatedRuntimeBundle};
use std::path::Path;
#[cfg(test)]
use std::{collections::BTreeMap, path::PathBuf};
use thiserror::Error;

/// One complete runtime bundle at the raw shared-validation boundary.
#[cfg(test)]
pub(crate) struct RuntimeBundleBytes {
    manifest: Vec<u8>,
    files: BTreeMap<PathBuf, Vec<u8>>,
}

#[cfg(test)]
impl RuntimeBundleBytes {
    pub(crate) fn new(manifest: Vec<u8>, files: BTreeMap<PathBuf, Vec<u8>>) -> Self {
        Self { manifest, files }
    }
}

/// Content-bound proof that native Spinal loaded one browser-identical bundle.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativeValidationEvidence {
    manifest_sha256: String,
    json: RuntimeFileEvidence,
    atlas: RuntimeFileEvidence,
    pages: Vec<RuntimeFileEvidence>,
    spine_version: String,
    bones: usize,
    slots: usize,
    skins: usize,
    animations: Vec<String>,
    constraints: usize,
    diagnostics: Vec<RuntimeDiagnosticEvidence>,
}

impl NativeValidationEvidence {
    #[cfg(test)]
    pub(crate) fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
    }

    pub(crate) fn json_path(&self) -> &str {
        &self.json.path
    }

    pub(crate) fn json_sha256(&self) -> &str {
        &self.json.sha256
    }

    pub(crate) fn atlas_path(&self) -> &str {
        &self.atlas.path
    }

    pub(crate) fn atlas_sha256(&self) -> &str {
        &self.atlas.sha256
    }

    pub(crate) const fn atlas_byte_length(&self) -> usize {
        self.atlas.byte_length
    }

    pub(crate) fn pages(&self) -> impl ExactSizeIterator<Item = (&str, usize, &str)> {
        self.pages
            .iter()
            .map(|page| (page.path.as_str(), page.byte_length, page.sha256.as_str()))
    }

    pub(crate) fn spine_version(&self) -> &str {
        &self.spine_version
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RuntimeFileEvidence {
    path: String,
    byte_length: usize,
    sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RuntimeDiagnosticEvidence {
    severity: String,
    code: String,
    scope: String,
    message: String,
}

/// A failure at the shared bundle boundary or in native no-degradation policy.
#[derive(Debug, Error)]
pub(crate) enum NativeValidationError {
    #[error(transparent)]
    Shared(#[from] RuntimeBundleError),
    #[error("Spinal reported output-changing runtime diagnostics")]
    Degraded,
}

/// Applies the exact shared manifest/file-map contract, then records native evidence.
#[cfg(test)]
pub(crate) fn validate_runtime_bundle(
    bundle: RuntimeBundleBytes,
) -> Result<NativeValidationEvidence, NativeValidationError> {
    let manifest = RuntimeBundleManifest::parse(&bundle.manifest)?;
    let validated = manifest.validate(bundle.files)?;
    validate_validated_runtime_bundle(&validated)
}

/// Records native evidence from an already shared-validated canonical bundle.
pub(crate) fn validate_validated_runtime_bundle(
    bundle: &ValidatedRuntimeBundle,
) -> Result<NativeValidationEvidence, NativeValidationError> {
    let asset = bundle.asset();
    if asset
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.severity() == DiagnosticSeverity::Degraded)
    {
        return Err(NativeValidationError::Degraded);
    }

    let json = file_evidence(bundle.json_path(), bundle.json_bytes());
    let atlas = file_evidence(bundle.atlas_path(), bundle.atlas_bytes());
    let pages = bundle
        .files()
        .filter(|(path, _bytes)| *path != bundle.json_path() && *path != bundle.atlas_path())
        .map(|(path, bytes)| file_evidence(path, bytes))
        .collect();
    let diagnostics = asset
        .diagnostics()
        .iter()
        .map(|diagnostic| RuntimeDiagnosticEvidence {
            severity: format!("{:?}", diagnostic.severity()).to_ascii_lowercase(),
            code: format!("{:?}", diagnostic.code()),
            scope: format!("{:?}", diagnostic.scope()),
            message: diagnostic.message().to_owned(),
        })
        .collect();

    Ok(NativeValidationEvidence {
        manifest_sha256: bundle.manifest_sha256().to_owned(),
        json,
        atlas,
        pages,
        spine_version: asset.spine_version().to_owned(),
        bones: asset.bones().len(),
        slots: asset.slots().len(),
        skins: asset.skins().len(),
        animations: asset
            .animations()
            .map(|animation| animation.name().to_owned())
            .collect(),
        constraints: asset.constraints().len(),
        diagnostics,
    })
}

fn file_evidence(path: &Path, bytes: &[u8]) -> RuntimeFileEvidence {
    let path = path
        .to_str()
        .expect("shared validation guarantees UTF-8 virtual paths")
        .to_owned();
    RuntimeFileEvidence {
        path,
        byte_length: bytes.len(),
        sha256: sha256_bytes(bytes),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    const JSON: &[u8] = br#"{
      "skeleton":{"spine":"4.3.23"},
      "bones":[{"name":"root"}],
      "slots":[{"name":"body-slot","bone":"root","attachment":"body"}],
      "skins":[{"name":"default","attachments":{"body-slot":{"body":{"width":8,"height":8}}}}],
      "animations":{"idle":{}}
    }"#;
    const ATLAS: &[u8] = b"images/page.png\n\tsize: 1, 1\n\tformat: RGBA8888\n\tfilter: Linear, Linear\n\trepeat: none\n\tpma: false\nbody\n\tbounds: 0, 0, 1, 1\n";
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

    fn files(json: &[u8], page: &[u8]) -> BTreeMap<PathBuf, Vec<u8>> {
        BTreeMap::from([
            (PathBuf::from("review/rig.json"), json.to_vec()),
            (PathBuf::from("review/rig.atlas"), ATLAS.to_vec()),
            (PathBuf::from("review/images/page.png"), page.to_vec()),
        ])
    }

    fn manifest(json: &[u8], page: &[u8], json_path: &str) -> Vec<u8> {
        format!(
            r#"{{
  "format_version":1,
  "source":{{
    "label":"Phase 0A fixture",
    "json":"{json_path}",
    "atlas":"review/rig.atlas",
    "files":[
      {{"path":"{json_path}","url":"rig.json","byte_length":{},"sha256":"{}"}},
      {{"path":"review/rig.atlas","url":"rig.atlas","byte_length":{},"sha256":"{}"}},
      {{"path":"review/images/page.png","url":"images/page.png","byte_length":{},"sha256":"{}"}}
    ]
  }}
}}"#,
            json.len(),
            sha256_hex(json),
            ATLAS.len(),
            sha256_hex(ATLAS),
            page.len(),
            sha256_hex(page),
        )
        .into_bytes()
    }

    fn bundle() -> RuntimeBundleBytes {
        RuntimeBundleBytes::new(manifest(JSON, PNG, "review/rig.json"), files(JSON, PNG))
    }

    #[test]
    fn evidence_binds_manifest_paths_lengths_and_every_digest() {
        let expected_manifest_sha256 = sha256_bytes(&bundle().manifest);
        let evidence = validate_runtime_bundle(bundle()).expect("valid runtime bundle");
        assert_eq!(evidence.manifest_sha256(), expected_manifest_sha256);
        assert_eq!(evidence.json_path(), "review/rig.json");
        assert_eq!(evidence.json_sha256(), sha256_bytes(JSON));
        assert_eq!(evidence.json.byte_length, JSON.len());
        assert_eq!(evidence.atlas_path(), "review/rig.atlas");
        assert_eq!(evidence.atlas_sha256(), sha256_bytes(ATLAS));
        assert_eq!(evidence.atlas.byte_length, ATLAS.len());
        assert_eq!(evidence.spine_version, "4.3.23");
        assert_eq!(evidence.animations, ["idle"]);
        assert_eq!(evidence.pages.len(), 1);
        assert_eq!(evidence.pages[0].byte_length, PNG.len());
        assert_eq!(evidence.pages[0].sha256, sha256_bytes(PNG));
    }

    #[test]
    fn native_rejects_unsafe_paths_and_file_set_mismatches_through_shared_contract() {
        let unsafe_bundle =
            RuntimeBundleBytes::new(manifest(JSON, PNG, "../rig.json"), files(JSON, PNG));
        assert!(matches!(
            validate_runtime_bundle(unsafe_bundle),
            Err(NativeValidationError::Shared(
                RuntimeBundleError::InvalidManifest(_)
            ))
        ));

        let mut missing = bundle();
        missing.files.remove(Path::new("review/images/page.png"));
        assert!(matches!(
            validate_runtime_bundle(missing),
            Err(NativeValidationError::Shared(
                RuntimeBundleError::FileSetMismatch
            ))
        ));
    }

    #[test]
    fn native_rejects_arbitrary_and_corrupt_pngs_through_shared_contract() {
        for page in [b"not a PNG".as_slice(), {
            let mut corrupt = PNG.to_vec();
            corrupt[50] ^= 1;
            Box::leak(corrupt.into_boxed_slice())
        }] {
            let invalid =
                RuntimeBundleBytes::new(manifest(JSON, page, "review/rig.json"), files(JSON, page));
            assert!(matches!(
                validate_runtime_bundle(invalid),
                Err(NativeValidationError::Shared(
                    RuntimeBundleError::InvalidTexture { .. }
                ))
            ));
        }
    }

    #[test]
    fn native_adds_no_degradation_policy_after_shared_acceptance() {
        let degraded_json = br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[{"name":"root","transform":"onlyTranslation"}],
          "animations":{}
        }"#;
        let degraded = RuntimeBundleBytes::new(
            manifest(degraded_json, PNG, "review/rig.json"),
            files(degraded_json, PNG),
        );
        assert!(matches!(
            validate_runtime_bundle(degraded),
            Err(NativeValidationError::Degraded)
        ));
    }

    #[test]
    fn canonical_builder_result_feeds_native_evidence_without_revalidation() {
        let (_manifest, validated) = RuntimeBundleManifest::build(
            "Phase 0A fixture",
            Path::new("review/rig.json"),
            Path::new("review/rig.atlas"),
            files(JSON, PNG),
        )
        .expect("canonical valid bundle");
        let evidence =
            validate_validated_runtime_bundle(&validated).expect("native validation evidence");
        assert_eq!(evidence.manifest_sha256(), validated.manifest_sha256());
    }
}

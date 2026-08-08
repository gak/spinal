//! Browser adapter for Spinal's shared immutable runtime-bundle contract.

use std::{collections::BTreeMap, path::PathBuf};

use bevy_spinal::spinal::{
    MAX_RUNTIME_BUNDLE_BYTES, MAX_RUNTIME_MANIFEST_BYTES, RuntimeBundleFile, RuntimeBundleManifest,
};

use crate::bundle::SourceBundle;

pub(crate) const MAX_MANIFEST_BYTES: usize = MAX_RUNTIME_MANIFEST_BYTES;
pub(crate) const MAX_BROWSER_BUNDLE_BYTES: usize = MAX_RUNTIME_BUNDLE_BYTES;
pub(crate) type BrowserManifestError = bevy_spinal::spinal::RuntimeBundleError;

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

    pub(crate) fn into_bundle(
        self,
        downloaded: BTreeMap<PathBuf, Vec<u8>>,
    ) -> Result<SourceBundle, BrowserManifestError> {
        let validated = self.0.validate(downloaded)?;
        Ok(SourceBundle::from_validated(validated))
    }
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

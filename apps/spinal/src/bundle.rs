//! Immutable virtual export bundles shared by every viewer host.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use bevy::asset::io::memory::{Dir, MemoryAssetReader, Value};
use bevy_spinal::spinal::{self, SkeletonAsset};

/// Stable acquisition and content identity retained with a loaded source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceProvenance {
    label: Box<str>,
    manifest_sha256: Box<str>,
    content_sha256: Box<str>,
}

#[allow(
    dead_code,
    reason = "provenance is retained now for coordinator audit UI in a later slice"
)]
impl SourceProvenance {
    pub(crate) fn label(&self) -> &str {
        &self.label
    }

    pub(crate) fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
    }

    pub(crate) fn content_sha256(&self) -> &str {
        &self.content_sha256
    }
}

/// Export bytes addressed by normalized virtual package paths.
///
/// The shared Spinal runtime-bundle boundary owns all format and dependency
/// validation. This viewer adapter only exposes the immutable snapshot through
/// Bevy's memory reader.
#[derive(Clone, Debug)]
pub(crate) struct SourceBundle {
    json_asset_path: PathBuf,
    atlas_asset_path: PathBuf,
    atlas_reference: Box<str>,
    files: Arc<BTreeMap<PathBuf, Arc<Vec<u8>>>>,
    skeleton: Arc<SkeletonAsset>,
    #[cfg_attr(
        not(feature = "web"),
        allow(
            dead_code,
            reason = "used by strict browser review aggregate validation"
        )
    )]
    file_count: usize,
    #[cfg_attr(
        not(feature = "web"),
        allow(
            dead_code,
            reason = "used by strict browser review aggregate validation"
        )
    )]
    encoded_bytes: usize,
    #[cfg_attr(
        not(feature = "web"),
        allow(
            dead_code,
            reason = "used by strict browser review aggregate validation"
        )
    )]
    decoded_texture_bytes: usize,
    #[allow(
        dead_code,
        reason = "provenance is retained now for coordinator audit UI in a later slice"
    )]
    provenance: SourceProvenance,
}

impl SourceBundle {
    /// Adapts the shared host-neutral validation result for Bevy's memory reader.
    pub(crate) fn from_validated(bundle: spinal::ValidatedRuntimeBundle) -> Self {
        let json_asset_path = bundle.json_path().to_path_buf();
        let atlas_asset_path = bundle.atlas_path().to_path_buf();
        let atlas_reference = relative_reference(&json_asset_path, &atlas_asset_path);
        let skeleton = Arc::clone(bundle.asset());
        let file_count = bundle.file_count();
        let encoded_bytes = bundle.encoded_bytes();
        let decoded_texture_bytes = bundle.decoded_texture_bytes();
        let provenance = SourceProvenance {
            label: bundle.label().into(),
            manifest_sha256: bundle.manifest_sha256().into(),
            content_sha256: bundle.content_sha256().into(),
        };
        let files = bundle
            .into_files()
            .into_iter()
            .map(|(path, bytes)| (path, Arc::new(bytes)))
            .collect();
        Self {
            json_asset_path,
            atlas_asset_path,
            atlas_reference,
            files: Arc::new(files),
            skeleton,
            file_count,
            encoded_bytes,
            decoded_texture_bytes,
            provenance,
        }
    }

    #[cfg(test)]
    pub(crate) fn from_test_files(
        label: &str,
        json_path: &Path,
        atlas_path: &Path,
        files: BTreeMap<PathBuf, Vec<u8>>,
    ) -> Self {
        let validated = spinal::RuntimeBundleManifest::build(label, json_path, atlas_path, files)
            .expect("valid strict test bundle")
            .1;
        Self::from_validated(validated)
    }

    /// Returns the typed skeleton path inside this virtual package.
    pub(crate) fn json_asset_path(&self) -> &Path {
        &self.json_asset_path
    }

    /// Returns the text-atlas path inside this virtual package.
    pub(crate) fn atlas_asset_path(&self) -> &Path {
        &self.atlas_asset_path
    }

    /// Returns the derived atlas reference relative to the virtual skeleton path.
    pub(crate) fn atlas_reference(&self) -> &str {
        &self.atlas_reference
    }

    /// Returns the clean-room parsed asset used to validate this snapshot.
    pub(crate) fn skeleton(&self) -> &Arc<SkeletonAsset> {
        &self.skeleton
    }

    /// Returns deterministic provenance from the strict shared bundle boundary.
    #[allow(
        dead_code,
        reason = "provenance is retained now for coordinator audit UI in a later slice"
    )]
    pub(crate) const fn provenance(&self) -> &SourceProvenance {
        &self.provenance
    }

    /// Returns the exact number of files in this validated snapshot.
    #[cfg_attr(
        not(feature = "web"),
        allow(
            dead_code,
            reason = "used by strict browser review aggregate validation"
        )
    )]
    pub(crate) const fn file_count(&self) -> usize {
        self.file_count
    }

    /// Returns the exact sum of encoded bytes in this validated snapshot.
    #[cfg_attr(
        not(feature = "web"),
        allow(
            dead_code,
            reason = "used by strict browser review aggregate validation"
        )
    )]
    pub(crate) const fn encoded_bytes(&self) -> usize {
        self.encoded_bytes
    }

    /// Returns the exact decoded RGBA texture bytes in this snapshot.
    #[cfg_attr(
        not(feature = "web"),
        allow(
            dead_code,
            reason = "used by strict browser review aggregate validation"
        )
    )]
    pub(crate) const fn decoded_texture_bytes(&self) -> usize {
        self.decoded_texture_bytes
    }

    /// Creates a read-only Bevy reader containing only this package's files.
    pub(crate) fn memory_reader(&self) -> MemoryAssetReader {
        let directory = Dir::default();
        for (path, bytes) in self.files.iter() {
            directory.insert_asset(path, Value::Vec(Arc::clone(bytes)));
        }
        MemoryAssetReader { root: directory }
    }

    #[cfg(test)]
    pub(crate) fn file_paths(&self) -> impl Iterator<Item = &Path> {
        self.files.keys().map(PathBuf::as_path)
    }

    #[cfg(test)]
    pub(crate) fn file(&self, path: &Path) -> Option<&[u8]> {
        self.files.get(path).map(AsRef::as_ref).map(Vec::as_slice)
    }
}

fn relative_reference(from_file: &Path, to_file: &Path) -> Box<str> {
    let from = path_segments(from_file);
    let to = path_segments(to_file);
    let from_directory = &from[..from.len() - 1];
    let common = from_directory
        .iter()
        .zip(&to)
        .take_while(|(left, right)| left == right)
        .count();
    let mut segments = vec![".."; from_directory.len() - common];
    segments.extend_from_slice(&to[common..]);
    segments.join("/").into_boxed_str()
}

fn path_segments(path: &Path) -> Vec<&str> {
    path.to_str()
        .expect("validated virtual paths are UTF-8")
        .split('/')
        .collect()
}

#[cfg(test)]
pub(crate) const TEST_BLUE_PIXEL_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 96, 96, 248, 255, 31, 0, 3,
    2, 1, 255, 230, 119, 11, 174, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

#[cfg(test)]
pub(crate) const TEST_RED_PIXEL_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240, 31, 0,
    5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &[u8] = br#"{"skeleton":{"spine":"4.3.23"},"bones":[{"name":"root"}]}"#;

    fn atlas(page: &str) -> Vec<u8> {
        format!(
            "{page}\n\tsize: 1, 1\n\tformat: RGBA8888\n\tfilter: Linear, Linear\n\trepeat: none\n\tpma: false\n"
        )
        .into_bytes()
    }

    fn validated(
        label: &str,
        json_path: &str,
        atlas_path: &str,
        page_path: &str,
        page_reference: &str,
        page: &[u8],
    ) -> spinal::ValidatedRuntimeBundle {
        let files = BTreeMap::from([
            (PathBuf::from(json_path), JSON.to_vec()),
            (PathBuf::from(atlas_path), atlas(page_reference)),
            (PathBuf::from(page_path), page.to_vec()),
        ]);
        spinal::RuntimeBundleManifest::build(
            label,
            Path::new(json_path),
            Path::new(atlas_path),
            files,
        )
        .expect("valid strict fixture bundle")
        .1
    }

    #[test]
    fn adapts_validated_bytes_paths_and_provenance_without_reparsing() {
        let validated = validated(
            "Nested fixture",
            "skeletons/rig.json",
            "atlases/rig.atlas",
            "textures/rig.png",
            "../textures/rig.png",
            TEST_BLUE_PIXEL_PNG,
        );
        let expected_manifest = validated.manifest_sha256().to_owned();
        let expected_content = validated.content_sha256().to_owned();
        let bundle = SourceBundle::from_validated(validated);

        assert_eq!(bundle.json_asset_path(), Path::new("skeletons/rig.json"));
        assert_eq!(bundle.atlas_asset_path(), Path::new("atlases/rig.atlas"));
        assert_eq!(bundle.atlas_reference(), "../atlases/rig.atlas");
        assert_eq!(bundle.provenance().label(), "Nested fixture");
        assert_eq!(bundle.provenance().manifest_sha256(), expected_manifest);
        assert_eq!(bundle.provenance().content_sha256(), expected_content);
        assert_eq!(bundle.file_count(), 3);
        assert_eq!(
            bundle.encoded_bytes(),
            JSON.len() + atlas("../textures/rig.png").len() + TEST_BLUE_PIXEL_PNG.len()
        );
        assert_eq!(bundle.decoded_texture_bytes(), 4);
        assert_eq!(
            bundle.file_paths().map(Path::to_owned).collect::<Vec<_>>(),
            [
                PathBuf::from("atlases/rig.atlas"),
                PathBuf::from("skeletons/rig.json"),
                PathBuf::from("textures/rig.png"),
            ]
        );
    }

    #[test]
    fn equal_virtual_names_keep_each_immutable_snapshot_isolated() {
        let primary = SourceBundle::from_validated(validated(
            "Primary",
            "rig.json",
            "rig.atlas",
            "textures/rig.png",
            "textures/rig.png",
            TEST_RED_PIXEL_PNG,
        ));
        let comparison = SourceBundle::from_validated(validated(
            "Comparison",
            "rig.json",
            "rig.atlas",
            "textures/rig.png",
            "textures/rig.png",
            TEST_BLUE_PIXEL_PNG,
        ));

        assert_eq!(
            primary.file(Path::new("textures/rig.png")),
            Some(TEST_RED_PIXEL_PNG)
        );
        assert_eq!(
            comparison.file(Path::new("textures/rig.png")),
            Some(TEST_BLUE_PIXEL_PNG)
        );
        assert_ne!(
            primary.provenance().content_sha256(),
            comparison.provenance().content_sha256()
        );
    }
}

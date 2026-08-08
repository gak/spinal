//! Immutable virtual export bundles shared by every viewer host.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use bevy::asset::io::memory::{Dir, MemoryAssetReader, Value};

/// Export bytes addressed by normalized virtual package paths.
///
/// This value contains no host filesystem path or URL. Host adapters validate
/// and copy an export into it before Bevy starts.
#[derive(Clone, Debug)]
pub(crate) struct SourceBundle {
    json_asset_path: PathBuf,
    atlas_reference: Box<str>,
    files: Arc<BTreeMap<PathBuf, Arc<Vec<u8>>>>,
}

impl SourceBundle {
    pub(crate) fn new(
        json_asset_path: impl Into<PathBuf>,
        atlas_reference: impl Into<Box<str>>,
        files: BTreeMap<PathBuf, Arc<Vec<u8>>>,
    ) -> Self {
        let json_asset_path = json_asset_path.into();
        debug_assert!(files.contains_key(&json_asset_path));
        Self {
            json_asset_path,
            atlas_reference: atlas_reference.into(),
            files: Arc::new(files),
        }
    }

    /// Returns the typed skeleton path inside this virtual package.
    pub(crate) fn json_asset_path(&self) -> &Path {
        &self.json_asset_path
    }

    /// Returns the atlas reference relative to the virtual skeleton path.
    pub(crate) fn atlas_reference(&self) -> &str {
        &self.atlas_reference
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

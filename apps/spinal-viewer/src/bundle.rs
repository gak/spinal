//! Immutable virtual export bundles shared by every viewer host.

use std::{
    collections::BTreeMap,
    error::Error,
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};

use bevy::asset::io::memory::{Dir, MemoryAssetReader, Value};
use bevy_spinal::spinal::{self, SkeletonAsset};

/// The purpose of one file requested while validating an export bundle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BundleFileRole<'a> {
    SkeletonJson,
    TextAtlas,
    AtlasPage(&'a str),
}

/// A host-neutral request for immutable bytes at one virtual package path.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BundleFileRequest<'a> {
    virtual_path: &'a Path,
    role: BundleFileRole<'a>,
}

impl<'a> BundleFileRequest<'a> {
    pub(crate) const fn virtual_path(self) -> &'a Path {
        self.virtual_path
    }

    pub(crate) const fn role(self) -> BundleFileRole<'a> {
        self.role
    }
}

/// Failure while constructing a validated immutable export bundle.
#[derive(Debug)]
pub(crate) enum SourceBundleError<E> {
    InvalidVirtualPath {
        path: PathBuf,
        reason: &'static str,
    },
    DuplicateVirtualPath {
        path: PathBuf,
    },
    Read {
        path: PathBuf,
        source: E,
    },
    InvalidExport {
        source: Box<spinal::LoadError>,
    },
    WrongSpineVersion {
        expected: &'static str,
        actual: Box<str>,
    },
    InvalidPageReference {
        page: Box<str>,
        reason: &'static str,
    },
}

impl<E: fmt::Display> fmt::Display for SourceBundleError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidVirtualPath { path, reason } => {
                write!(
                    formatter,
                    "invalid virtual path `{}`: {reason}",
                    path.display()
                )
            }
            Self::DuplicateVirtualPath { path } => {
                write!(formatter, "duplicate virtual path `{}`", path.display())
            }
            Self::Read { path, source } => {
                write!(
                    formatter,
                    "could not read virtual file `{}`: {source}",
                    path.display()
                )
            }
            Self::InvalidExport { source } => write!(formatter, "invalid Spine export: {source}"),
            Self::WrongSpineVersion { expected, actual } => write!(
                formatter,
                "expected Spine {expected}, but the export declares {actual}"
            ),
            Self::InvalidPageReference { page, reason } => {
                write!(formatter, "invalid atlas page reference `{page}`: {reason}")
            }
        }
    }
}

impl<E> Error for SourceBundleError<E>
where
    E: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::InvalidExport { source } => Some(source.as_ref()),
            _ => None,
        }
    }
}

/// Export bytes addressed by normalized virtual package paths.
///
/// This value contains no host filesystem path or URL. Its fallible constructor
/// owns all format and dependency validation; hosts supply bytes but do not
/// reinterpret Spine JSON or atlas syntax.
#[derive(Clone, Debug)]
pub(crate) struct SourceBundle {
    json_asset_path: PathBuf,
    atlas_reference: Box<str>,
    files: Arc<BTreeMap<PathBuf, Arc<Vec<u8>>>>,
    skeleton: Arc<SkeletonAsset>,
}

impl SourceBundle {
    /// Loads and validates one complete Spine runtime export.
    ///
    /// Every requested path is normalized, relative, UTF-8 package syntax.
    /// The byte provider may read from any host, but the resulting bundle is an
    /// isolated immutable snapshot with no authority to read more files later.
    /// A provider built from an enumerated manifest must reject duplicate input
    /// entries before indexing them; this boundary rejects duplicate dependency
    /// paths it can observe and never retains unrequested manifest entries.
    pub(crate) fn load<E>(
        json_asset_path: impl Into<PathBuf>,
        atlas_asset_path: impl Into<PathBuf>,
        mut read: impl FnMut(BundleFileRequest<'_>) -> Result<Vec<u8>, E>,
    ) -> Result<Self, SourceBundleError<E>> {
        let json_asset_path = validate_virtual_path(json_asset_path.into())?;
        let atlas_asset_path = validate_virtual_path(atlas_asset_path.into())?;
        if json_asset_path == atlas_asset_path {
            return Err(SourceBundleError::DuplicateVirtualPath {
                path: json_asset_path,
            });
        }

        let atlas_reference = relative_reference(&json_asset_path, &atlas_asset_path);
        let mut files = BTreeMap::new();
        let json = request_file(
            &mut files,
            &mut read,
            &json_asset_path,
            BundleFileRole::SkeletonJson,
        )?;
        let atlas = request_file(
            &mut files,
            &mut read,
            &atlas_asset_path,
            BundleFileRole::TextAtlas,
        )?;
        let skeleton = spinal::load_json(&json, &atlas)
            .map_err(|source| SourceBundleError::InvalidExport {
                source: Box::new(source),
            })?
            .into_asset();
        if skeleton.spine_version() != spinal::TARGET_SPINE_VERSION {
            return Err(SourceBundleError::WrongSpineVersion {
                expected: spinal::TARGET_SPINE_VERSION,
                actual: skeleton.spine_version().into(),
            });
        }

        for page in skeleton.atlas_pages() {
            let page_path =
                resolve_page_path(&atlas_asset_path, page.name()).map_err(|reason| {
                    SourceBundleError::InvalidPageReference {
                        page: page.name().into(),
                        reason,
                    }
                })?;
            request_file(
                &mut files,
                &mut read,
                &page_path,
                BundleFileRole::AtlasPage(page.name()),
            )?;
        }

        Ok(Self {
            json_asset_path,
            atlas_reference,
            files: Arc::new(files),
            skeleton,
        })
    }

    /// Returns the typed skeleton path inside this virtual package.
    pub(crate) fn json_asset_path(&self) -> &Path {
        &self.json_asset_path
    }

    /// Returns the derived atlas reference relative to the virtual skeleton path.
    pub(crate) fn atlas_reference(&self) -> &str {
        &self.atlas_reference
    }

    /// Returns the clean-room parsed asset used to validate this snapshot.
    pub(crate) fn skeleton(&self) -> &Arc<SkeletonAsset> {
        &self.skeleton
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

fn request_file<E>(
    files: &mut BTreeMap<PathBuf, Arc<Vec<u8>>>,
    read: &mut impl FnMut(BundleFileRequest<'_>) -> Result<Vec<u8>, E>,
    path: &Path,
    role: BundleFileRole<'_>,
) -> Result<Arc<Vec<u8>>, SourceBundleError<E>> {
    if files.contains_key(path) {
        return Err(SourceBundleError::DuplicateVirtualPath {
            path: path.to_owned(),
        });
    }
    let bytes = Arc::new(
        read(BundleFileRequest {
            virtual_path: path,
            role,
        })
        .map_err(|source| SourceBundleError::Read {
            path: path.to_owned(),
            source,
        })?,
    );
    files.insert(path.to_owned(), Arc::clone(&bytes));
    Ok(bytes)
}

fn validate_virtual_path<E>(path: PathBuf) -> Result<PathBuf, SourceBundleError<E>> {
    let Some(value) = path.to_str() else {
        return Err(SourceBundleError::InvalidVirtualPath {
            path,
            reason: "the path is not valid UTF-8",
        });
    };
    let reason = if value.is_empty() {
        Some("the path is empty")
    } else if value.starts_with('/') || value.starts_with('\\') || looks_like_windows_drive(value) {
        Some("absolute paths are not allowed")
    } else if value.contains('\\') {
        Some("path separators must be forward slashes")
    } else if value.contains("://") {
        Some("asset-source syntax is not allowed")
    } else if value.contains('#') {
        Some("asset-label syntax is not allowed")
    } else if value.split('/').any(str::is_empty) {
        Some("empty path components are not allowed")
    } else if value.split('/').any(|component| component == ".") {
        Some("dot path components are not allowed")
    } else if value.split('/').any(|component| component == "..") {
        Some("parent path components are not allowed")
    } else {
        None
    };
    if let Some(reason) = reason {
        Err(SourceBundleError::InvalidVirtualPath { path, reason })
    } else {
        Ok(path)
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
    if reference.contains('\\') {
        return Err("page paths must use forward slashes");
    }
    if reference.contains("://") {
        return Err("asset-source switching is not allowed");
    }
    if reference.contains('#') {
        return Err("asset labels are not allowed in page paths");
    }

    let mut resolved = path_segments(atlas_path);
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
    Ok(PathBuf::from(resolved.join("/")))
}

fn path_segments(path: &Path) -> Vec<&str> {
    path.to_str()
        .expect("validated virtual paths are UTF-8")
        .split('/')
        .collect()
}

fn looks_like_windows_drive(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct Missing(PathBuf);

    impl fmt::Display for Missing {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "missing `{}`", self.0.display())
        }
    }

    impl Error for Missing {}

    fn skeleton_json(version: &str) -> Vec<u8> {
        format!(r#"{{"skeleton":{{"spine":"{version}"}},"bones":[{{"name":"root"}}]}}"#)
            .into_bytes()
    }

    fn atlas(page: &str) -> Vec<u8> {
        format!(
            "{page}\n\tsize: 1, 1\n\tformat: RGBA8888\n\tfilter: Linear, Linear\n\trepeat: none\n\tpma: false\n"
        )
        .into_bytes()
    }

    fn load(
        entries: &BTreeMap<PathBuf, Vec<u8>>,
    ) -> Result<SourceBundle, SourceBundleError<Missing>> {
        SourceBundle::load("rig.json", "rig.atlas", |request| {
            entries
                .get(request.virtual_path())
                .cloned()
                .ok_or_else(|| Missing(request.virtual_path().to_owned()))
        })
    }

    fn complete(page_bytes: &[u8]) -> BTreeMap<PathBuf, Vec<u8>> {
        BTreeMap::from([
            (PathBuf::from("rig.json"), skeleton_json("4.3.23")),
            (PathBuf::from("rig.atlas"), atlas("textures/rig.png")),
            (PathBuf::from("textures/rig.png"), page_bytes.to_vec()),
        ])
    }

    #[test]
    fn requires_declared_json_atlas_and_every_page() {
        for missing in ["rig.json", "rig.atlas", "textures/rig.png"] {
            let mut entries = complete(b"page");
            entries.remove(Path::new(missing));
            let error = load(&entries).expect_err("a required file is absent");
            assert!(matches!(
                error,
                SourceBundleError::Read { path, .. } if path == Path::new(missing)
            ));
        }
    }

    #[test]
    fn rejects_unsafe_or_unnormalized_virtual_paths() {
        let entries = complete(b"page");
        for path in [
            "/rig.json",
            "C:/rig.json",
            "./rig.json",
            "exports/../rig.json",
            "exports\\rig.json",
            "exports//rig.json",
        ] {
            let error = SourceBundle::load(path, "rig.atlas", |request| {
                entries
                    .get(request.virtual_path())
                    .cloned()
                    .ok_or_else(|| Missing(request.virtual_path().to_owned()))
            })
            .expect_err("unsafe virtual path");
            assert!(matches!(
                error,
                SourceBundleError::InvalidVirtualPath { .. }
            ));
        }
    }

    #[test]
    fn rejects_duplicate_declared_or_resolved_paths() {
        let error = SourceBundle::load("same.asset", "same.asset", |_request| {
            Ok::<_, Missing>(Vec::new())
        })
        .expect_err("declared paths collide");
        assert!(matches!(
            error,
            SourceBundleError::DuplicateVirtualPath { ref path }
                if path == Path::new("same.asset")
        ));

        let mut entries = complete(b"page");
        entries.insert(PathBuf::from("rig.atlas"), atlas("rig.json"));
        let error = load(&entries).expect_err("two pages resolve to one virtual path");
        assert!(matches!(
            error,
            SourceBundleError::DuplicateVirtualPath { ref path }
                if path == Path::new("rig.json")
        ));
    }

    #[test]
    fn requires_the_exact_target_spine_version() {
        for version in ["4.3.22", "4.3.24"] {
            let mut entries = complete(b"page");
            entries.insert(PathBuf::from("rig.json"), skeleton_json(version));
            let error = load(&entries).expect_err("wrong patch version");
            assert!(matches!(
                error,
                SourceBundleError::WrongSpineVersion {
                    expected: "4.3.23",
                    ref actual,
                } if actual.as_ref() == version
            ));
        }

        for version in ["4.2.23", "4.4.0"] {
            let mut entries = complete(b"page");
            entries.insert(PathBuf::from("rig.json"), skeleton_json(version));
            assert!(matches!(
                load(&entries),
                Err(SourceBundleError::InvalidExport { .. })
            ));
        }

        let mut missing = complete(b"page");
        missing.insert(
            PathBuf::from("rig.json"),
            br#"{"skeleton":{},"bones":[{"name":"root"}]}"#.to_vec(),
        );
        assert!(matches!(
            load(&missing),
            Err(SourceBundleError::InvalidExport { .. })
        ));
    }

    #[test]
    fn requests_each_dependency_once_in_parser_order_with_its_role() {
        let entries = complete(b"page");
        let mut requests = Vec::new();
        SourceBundle::load("rig.json", "rig.atlas", |request| {
            let role = match request.role() {
                BundleFileRole::SkeletonJson => "skeleton JSON".to_owned(),
                BundleFileRole::TextAtlas => "text atlas".to_owned(),
                BundleFileRole::AtlasPage(page) => format!("atlas page {page}"),
            };
            requests.push((request.virtual_path().to_owned(), role));
            entries
                .get(request.virtual_path())
                .cloned()
                .ok_or_else(|| Missing(request.virtual_path().to_owned()))
        })
        .expect("complete bundle");

        assert_eq!(
            requests,
            [
                (PathBuf::from("rig.json"), "skeleton JSON".to_owned()),
                (PathBuf::from("rig.atlas"), "text atlas".to_owned()),
                (
                    PathBuf::from("textures/rig.png"),
                    "atlas page textures/rig.png".to_owned(),
                ),
            ]
        );
    }

    #[test]
    fn rejects_distinct_page_references_that_normalize_to_one_virtual_file() {
        let first = String::from_utf8(atlas("textures/../page.png")).expect("UTF-8 atlas");
        let second = String::from_utf8(atlas("page.png")).expect("UTF-8 atlas");
        let entries = BTreeMap::from([
            (PathBuf::from("rig.json"), skeleton_json("4.3.23")),
            (
                PathBuf::from("rig.atlas"),
                format!("{first}\n{second}").into_bytes(),
            ),
            (PathBuf::from("page.png"), b"page".to_vec()),
        ]);

        let error = load(&entries).expect_err("both page names resolve to page.png");
        assert!(matches!(
            error,
            SourceBundleError::DuplicateVirtualPath { ref path }
                if path == Path::new("page.png")
        ));
    }

    #[test]
    fn derives_relative_atlas_reference_and_resolves_pages() {
        let entries = BTreeMap::from([
            (PathBuf::from("skeletons/rig.json"), skeleton_json("4.3.23")),
            (
                PathBuf::from("atlases/rig.atlas"),
                atlas("../textures/rig.png"),
            ),
            (PathBuf::from("textures/rig.png"), b"page".to_vec()),
        ]);
        let bundle = SourceBundle::load("skeletons/rig.json", "atlases/rig.atlas", |request| {
            entries
                .get(request.virtual_path())
                .cloned()
                .ok_or_else(|| Missing(request.virtual_path().to_owned()))
        })
        .expect("valid nested bundle");

        assert_eq!(bundle.atlas_reference(), "../atlases/rig.atlas");
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
    fn equal_virtual_names_keep_each_bundles_bytes_isolated() {
        let primary = load(&complete(b"primary bytes")).expect("primary bundle");
        let comparison = load(&complete(b"comparison bytes")).expect("comparison bundle");

        assert_eq!(
            primary.file(Path::new("textures/rig.png")),
            Some(b"primary bytes".as_slice())
        );
        assert_eq!(
            comparison.file(Path::new("textures/rig.png")),
            Some(b"comparison bytes".as_slice())
        );
    }
}

//! Read-only command-line and filesystem preflight for a Spine JSON export.

use std::{
    collections::BTreeMap,
    error::Error,
    ffi::OsStr,
    fmt, fs, io,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use bevy_spinal::spinal::{self, AlphaEncoding, SkeletonAsset};

use crate::{
    bundle::SourceBundle,
    preview::{InvalidPreviewRate, PreviewRate},
};

pub(crate) const HELP: &str = "\
Spinal viewer

USAGE:
    spinal-viewer SKELETON.json [--atlas FILE.atlas] [--bundle-root DIR] [--fps FPS]

OPTIONS:
    --atlas FILE.atlas  Use this text atlas instead of discovering one
    --bundle-root DIR   Set package root (default: JSON file's directory)
    --fps FPS           Set the positive integer preview rate (default: 30)
    -h, --help          Print this help
";

/// Inputs accepted by the viewer before Bevy is started.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Options {
    json_path: PathBuf,
    atlas_path: Option<PathBuf>,
    bundle_root: Option<PathBuf>,
    preview_rate: PreviewRate,
}

impl Options {
    /// Parses arguments after the executable name.
    pub(crate) fn parse(
        arguments: impl IntoIterator<Item = String>,
    ) -> Result<ParseResult, OptionsError> {
        let mut arguments = arguments.into_iter();
        let mut json_path = None;
        let mut atlas_path = None;
        let mut bundle_root = None;
        let mut fps = None;
        let mut options_ended = false;

        while let Some(argument) = arguments.next() {
            if !options_ended {
                match argument.as_str() {
                    "-h" | "--help" => return Ok(ParseResult::Help),
                    "--" => {
                        options_ended = true;
                        continue;
                    }
                    "--atlas" => {
                        if atlas_path.is_some() {
                            return Err(OptionsError::DuplicateOption("--atlas"));
                        }
                        let value = next_value(&mut arguments, "--atlas")?;
                        if value.is_empty() {
                            return Err(OptionsError::EmptyValue("--atlas"));
                        }
                        atlas_path = Some(PathBuf::from(value));
                        continue;
                    }
                    "--bundle-root" => {
                        if bundle_root.is_some() {
                            return Err(OptionsError::DuplicateOption("--bundle-root"));
                        }
                        let value = next_value(&mut arguments, "--bundle-root")?;
                        if value.is_empty() {
                            return Err(OptionsError::EmptyValue("--bundle-root"));
                        }
                        bundle_root = Some(PathBuf::from(value));
                        continue;
                    }
                    "--fps" => {
                        if fps.is_some() {
                            return Err(OptionsError::DuplicateOption("--fps"));
                        }
                        fps = Some(parse_fps(next_value(&mut arguments, "--fps")?)?);
                        continue;
                    }
                    _ => {}
                }

                if let Some(value) = argument.strip_prefix("--atlas=") {
                    if atlas_path.is_some() {
                        return Err(OptionsError::DuplicateOption("--atlas"));
                    }
                    if value.is_empty() {
                        return Err(OptionsError::EmptyValue("--atlas"));
                    }
                    atlas_path = Some(PathBuf::from(value));
                    continue;
                }
                if let Some(value) = argument.strip_prefix("--bundle-root=") {
                    if bundle_root.is_some() {
                        return Err(OptionsError::DuplicateOption("--bundle-root"));
                    }
                    if value.is_empty() {
                        return Err(OptionsError::EmptyValue("--bundle-root"));
                    }
                    bundle_root = Some(PathBuf::from(value));
                    continue;
                }
                if let Some(value) = argument.strip_prefix("--fps=") {
                    if fps.is_some() {
                        return Err(OptionsError::DuplicateOption("--fps"));
                    }
                    fps = Some(parse_fps(value.to_owned())?);
                    continue;
                }
                if argument.starts_with('-') {
                    return Err(OptionsError::UnknownOption(argument));
                }
            }

            if json_path.is_some() {
                return Err(OptionsError::UnexpectedJsonPath(PathBuf::from(argument)));
            }
            json_path = Some(PathBuf::from(argument));
        }

        let json_path = json_path.ok_or(OptionsError::MissingJsonPath)?;
        let preview_rate =
            PreviewRate::from_override(fps).map_err(OptionsError::InvalidPreviewRate)?;
        Ok(ParseResult::Run(Self {
            json_path,
            atlas_path,
            bundle_root,
            preview_rate,
        }))
    }

    pub(crate) fn json_path(&self) -> &Path {
        &self.json_path
    }

    pub(crate) fn atlas_path(&self) -> Option<&Path> {
        self.atlas_path.as_deref()
    }

    pub(crate) fn bundle_root(&self) -> Option<&Path> {
        self.bundle_root.as_deref()
    }

    pub(crate) const fn preview_rate(&self) -> PreviewRate {
        self.preview_rate
    }
}

fn next_value(
    arguments: &mut impl Iterator<Item = String>,
    option: &'static str,
) -> Result<String, OptionsError> {
    arguments.next().ok_or(OptionsError::MissingValue(option))
}

fn parse_fps(value: String) -> Result<u32, OptionsError> {
    value
        .parse::<u32>()
        .map_err(|_error| OptionsError::InvalidFps(value))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ParseResult {
    Run(Options),
    Help,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum OptionsError {
    MissingJsonPath,
    UnexpectedJsonPath(PathBuf),
    UnknownOption(String),
    MissingValue(&'static str),
    EmptyValue(&'static str),
    DuplicateOption(&'static str),
    InvalidFps(String),
    InvalidPreviewRate(InvalidPreviewRate),
}

impl fmt::Display for OptionsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingJsonPath => formatter.write_str("a skeleton JSON path is required"),
            Self::UnexpectedJsonPath(path) => {
                write!(
                    formatter,
                    "unexpected second JSON path `{}`",
                    path.display()
                )
            }
            Self::UnknownOption(option) => write!(formatter, "unknown option `{option}`"),
            Self::MissingValue(option) => write!(formatter, "{option} requires a value"),
            Self::EmptyValue(option) => write!(formatter, "{option} requires a non-empty value"),
            Self::DuplicateOption(option) => {
                write!(formatter, "{option} may only be supplied once")
            }
            Self::InvalidFps(value) => write!(
                formatter,
                "invalid preview FPS `{value}`; --fps must be a positive integer"
            ),
            Self::InvalidPreviewRate(error) => write!(formatter, "invalid --fps: {error}"),
        }
    }
}

impl Error for OptionsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPreviewRate(error) => Some(error),
            _ => None,
        }
    }
}

/// One image page discovered from the parsed text atlas.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreparedPage {
    name: Box<str>,
    alpha_encoding: AlphaEncoding,
}

impl PreparedPage {
    /// Returns the page name exactly as authored in the atlas.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) const fn alpha_encoding(&self) -> AlphaEncoding {
        self.alpha_encoding
    }

    pub(crate) fn is_premultiplied(&self) -> bool {
        self.alpha_encoding == AlphaEncoding::Premultiplied
    }
}

/// A canonical native intake retaining display provenance and immutable bytes.
#[derive(Debug)]
pub(crate) struct PreparedSource {
    json_path: PathBuf,
    json_name: Box<str>,
    atlas_path: PathBuf,
    bundle: SourceBundle,
    preview_rate: PreviewRate,
    skeleton: Arc<SkeletonAsset>,
    pages: Box<[PreparedPage]>,
}

impl PreparedSource {
    /// Canonicalizes and validates the complete export without writing to it.
    pub(crate) fn load(options: Options) -> Result<Self, PrepareError> {
        ensure_json_filename(options.json_path())?;
        let json_path = canonical_file(options.json_path(), "skeleton JSON")?;
        ensure_json_filename(&json_path)?;
        let bundle_root = match options.bundle_root() {
            Some(path) => canonical_directory(path, "bundle root")?,
            None => json_path
                .parent()
                .expect("a canonical file has a parent directory")
                .to_owned(),
        };
        ensure_within_bundle_root("skeleton JSON", &json_path, &bundle_root)?;

        let atlas_path = match options.atlas_path() {
            Some(path) => canonical_file(path, "text atlas")?,
            None => discover_atlas(&json_path)?,
        };
        ensure_within_bundle_root("text atlas", &atlas_path, &bundle_root)?;

        let json = read_file(&json_path, "read skeleton JSON")?;
        let atlas = read_file(&atlas_path, "read text atlas")?;
        let skeleton = spinal::load_json(&json, &atlas)
            .map_err(|source| PrepareError::InvalidExport {
                json_path: json_path.clone(),
                atlas_path: atlas_path.clone(),
                source: Box::new(source),
            })?
            .into_asset();

        if skeleton.spine_version() != spinal::TARGET_SPINE_VERSION {
            return Err(PrepareError::WrongSpineVersion {
                expected: spinal::TARGET_SPINE_VERSION,
                actual: skeleton.spine_version().into(),
            });
        }

        let atlas_directory = atlas_path
            .parent()
            .expect("a canonical file has a parent directory");
        let json_asset_path = relative_asset_path(&bundle_root, &json_path)?;
        let atlas_asset_path = relative_asset_path(&bundle_root, &atlas_path)?;
        let atlas_reference = relative_file_reference(&json_path, &atlas_path, &bundle_root)?;
        let mut files = BTreeMap::new();
        files.insert(PathBuf::from(&json_asset_path), Arc::new(json));
        files.insert(PathBuf::from(atlas_asset_path), Arc::new(atlas));

        let mut pages = Vec::with_capacity(skeleton.atlas_pages().len());
        for page in skeleton.atlas_pages() {
            validate_embedded_reference(page.name()).map_err(|reason| {
                PrepareError::DisallowedPageReference {
                    atlas_path: atlas_path.clone(),
                    page: page.name().into(),
                    reason,
                }
            })?;
            let unresolved_path = atlas_directory.join(page.name());
            let path = fs::canonicalize(&unresolved_path).map_err(|source| {
                PrepareError::PageUnavailable {
                    page: page.name().into(),
                    path: unresolved_path,
                    source,
                }
            })?;
            if !path.is_file() {
                return Err(PrepareError::NotAFile {
                    role: "atlas page",
                    path,
                });
            }
            validate_page_within_root(&atlas_path, page.name(), &path, &bundle_root)?;
            let page_asset_path = relative_asset_path(&bundle_root, &path)?;
            let page_bytes = read_file(&path, "read atlas page")?;
            files
                .entry(PathBuf::from(page_asset_path))
                .or_insert_with(|| Arc::new(page_bytes));
            pages.push(PreparedPage {
                name: page.name().into(),
                alpha_encoding: page.alpha_encoding(),
            });
        }

        let json_name = json_path
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or_else(|| PrepareError::InvalidAssetPath {
                path: json_path.clone(),
                reason: "the filename is not valid UTF-8",
            })?
            .into();

        Ok(Self {
            json_path,
            json_name,
            atlas_path,
            bundle: SourceBundle::new(json_asset_path, atlas_reference, files),
            preview_rate: options.preview_rate(),
            skeleton,
            pages: pages.into_boxed_slice(),
        })
    }

    /// Returns the canonical JSON path for display and source auditing.
    pub(crate) fn json_path(&self) -> &Path {
        &self.json_path
    }

    /// Returns the JSON filename for compact UI labels.
    pub(crate) fn json_name(&self) -> &str {
        &self.json_name
    }

    /// Returns the canonical text-atlas path selected by preflight.
    pub(crate) fn atlas_path(&self) -> &Path {
        &self.atlas_path
    }

    /// Returns the typed JSON asset path inside the immutable bundle.
    #[cfg(test)]
    pub(crate) fn json_asset_path(&self) -> &str {
        self.bundle
            .json_asset_path()
            .to_str()
            .expect("validated virtual paths are UTF-8")
    }

    /// Returns the atlas setting relative to the skeleton JSON asset.
    #[cfg(test)]
    pub(crate) fn atlas_reference(&self) -> &str {
        self.bundle.atlas_reference()
    }

    /// Returns the immutable, host-independent export bytes.
    pub(crate) const fn bundle(&self) -> &SourceBundle {
        &self.bundle
    }

    pub(crate) const fn preview_rate(&self) -> PreviewRate {
        self.preview_rate
    }

    pub(crate) const fn preview_fps(&self) -> u32 {
        self.preview_rate.fps()
    }

    /// Returns the already parsed asset for building source-order UI catalogs.
    pub(crate) fn skeleton(&self) -> &Arc<SkeletonAsset> {
        &self.skeleton
    }

    pub(crate) fn pages(&self) -> &[PreparedPage] {
        &self.pages
    }

    /// Iterates pages that require premultiplied-alpha rendering.
    pub(crate) fn premultiplied_pages(&self) -> impl Iterator<Item = &PreparedPage> {
        self.pages.iter().filter(|page| page.is_premultiplied())
    }
}

fn ensure_json_filename(path: &Path) -> Result<(), PrepareError> {
    let Some(file_name) = path.file_name().and_then(OsStr::to_str) else {
        return Err(PrepareError::UnsupportedSkeletonPath {
            path: path.to_owned(),
        });
    };
    let valid = file_name
        .strip_suffix(".json")
        .is_some_and(|stem| !stem.is_empty());
    if valid {
        Ok(())
    } else {
        Err(PrepareError::UnsupportedSkeletonPath {
            path: path.to_owned(),
        })
    }
}

fn canonical_file(path: &Path, role: &'static str) -> Result<PathBuf, PrepareError> {
    let canonical = fs::canonicalize(path).map_err(|source| PrepareError::Io {
        action: "open",
        path: path.to_owned(),
        source,
    })?;
    if canonical.is_file() {
        Ok(canonical)
    } else {
        Err(PrepareError::NotAFile {
            role,
            path: canonical,
        })
    }
}

fn canonical_directory(path: &Path, role: &'static str) -> Result<PathBuf, PrepareError> {
    let canonical = fs::canonicalize(path).map_err(|source| PrepareError::Io {
        action: "open",
        path: path.to_owned(),
        source,
    })?;
    if canonical.is_dir() {
        Ok(canonical)
    } else {
        Err(PrepareError::NotADirectory {
            role,
            path: canonical,
        })
    }
}

fn ensure_within_bundle_root(
    role: &'static str,
    path: &Path,
    root: &Path,
) -> Result<(), PrepareError> {
    if path.strip_prefix(root).is_ok() {
        Ok(())
    } else {
        Err(PrepareError::OutsideBundleRoot {
            role,
            path: path.to_owned(),
            root: root.to_owned(),
        })
    }
}

fn read_file(path: &Path, action: &'static str) -> Result<Vec<u8>, PrepareError> {
    fs::read(path).map_err(|source| PrepareError::Io {
        action,
        path: path.to_owned(),
        source,
    })
}

fn discover_atlas(json_path: &Path) -> Result<PathBuf, PrepareError> {
    let conventional = conventional_atlas_path(json_path)?;
    if conventional.is_file() {
        return canonical_file(&conventional, "text atlas");
    }

    let directory = json_path
        .parent()
        .expect("a canonical file has a parent directory");
    let entries = fs::read_dir(directory).map_err(|source| PrepareError::Io {
        action: "inspect the JSON directory for a text atlas",
        path: directory.to_owned(),
        source,
    })?;
    let mut candidates = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|source| PrepareError::Io {
                action: "inspect an entry in the JSON directory",
                path: directory.to_owned(),
                source,
            })?
            .path();
        if path
            .extension()
            .is_some_and(|extension| extension == "atlas")
            && path.is_file()
        {
            candidates.push(path);
        }
    }
    candidates.sort();

    match candidates.as_slice() {
        [atlas] => canonical_file(atlas, "text atlas"),
        [] => Err(PrepareError::MissingAtlas {
            json_path: json_path.to_owned(),
            expected_path: conventional,
        }),
        _ => Err(PrepareError::AmbiguousAtlas {
            json_path: json_path.to_owned(),
            candidates,
        }),
    }
}

fn conventional_atlas_path(json_path: &Path) -> Result<PathBuf, PrepareError> {
    let file_name = json_path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| PrepareError::UnsupportedSkeletonPath {
            path: json_path.to_owned(),
        })?;
    let stem = file_name
        .strip_suffix(".spine.json")
        .or_else(|| file_name.strip_suffix(".json"))
        .filter(|stem| !stem.is_empty())
        .ok_or_else(|| PrepareError::UnsupportedSkeletonPath {
            path: json_path.to_owned(),
        })?;
    Ok(json_path.with_file_name(format!("{stem}.atlas")))
}

fn validate_embedded_reference(reference: &str) -> Result<(), &'static str> {
    if reference.is_empty() {
        return Err("the page name is empty");
    }
    if looks_like_windows_drive_path(reference) {
        return Err("Windows drive-prefixed page paths are not allowed");
    }
    if Path::new(reference).is_absolute() || reference.starts_with('\\') {
        return Err("absolute page paths are not allowed");
    }
    if reference.contains("://") {
        return Err("asset-source switching is not allowed");
    }
    if reference.contains('#') {
        return Err("asset labels are not allowed in page paths");
    }
    if reference.contains('\\') {
        return Err("page paths must use forward slashes");
    }
    if Path::new(reference).file_name().is_none() {
        return Err("the page path does not name a file");
    }
    Ok(())
}

fn looks_like_windows_drive_path(reference: &str) -> bool {
    let bytes = reference.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn relative_asset_path(root: &Path, path: &Path) -> Result<String, PrepareError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_error| PrepareError::InvalidAssetPath {
            path: path.to_owned(),
            reason: "the path is outside the authorized bundle root",
        })?;
    components_to_asset_string(relative, false).map_err(|reason| PrepareError::InvalidAssetPath {
        path: path.to_owned(),
        reason,
    })
}

fn relative_file_reference(
    from_file: &Path,
    to_file: &Path,
    root: &Path,
) -> Result<String, PrepareError> {
    let from_directory = from_file
        .parent()
        .ok_or_else(|| PrepareError::InvalidAssetPath {
            path: from_file.to_owned(),
            reason: "the source file has no parent directory",
        })?;
    let from = from_directory
        .strip_prefix(root)
        .map_err(|_error| PrepareError::InvalidAssetPath {
            path: from_file.to_owned(),
            reason: "the source path is outside the authorized bundle root",
        })?
        .components()
        .collect::<Vec<_>>();
    let to = to_file
        .strip_prefix(root)
        .map_err(|_error| PrepareError::InvalidAssetPath {
            path: to_file.to_owned(),
            reason: "the dependency path is outside the authorized bundle root",
        })?
        .components()
        .collect::<Vec<_>>();
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(left, right)| left == right)
        .count();

    let mut relative = PathBuf::new();
    for component in &from[common..] {
        if !matches!(component, Component::Normal(_)) {
            return Err(PrepareError::InvalidAssetPath {
                path: from_file.to_owned(),
                reason: "the source path has an invalid relative component",
            });
        }
        relative.push("..");
    }
    for component in &to[common..] {
        relative.push(component.as_os_str());
    }

    components_to_asset_string(&relative, true).map_err(|reason| PrepareError::InvalidAssetPath {
        path: to_file.to_owned(),
        reason,
    })
}

fn components_to_asset_string(path: &Path, allow_parent: bool) -> Result<String, &'static str> {
    let mut components = Vec::new();
    for component in path.components() {
        let value = match component {
            Component::Normal(value) => value
                .to_str()
                .ok_or("an asset-path component is not valid UTF-8")?,
            Component::ParentDir if allow_parent => "..",
            Component::CurDir => continue,
            _ => return Err("an asset path contains a disallowed component"),
        };
        if value.contains('#') || value.contains("://") || value.contains('\\') {
            return Err("an asset-path component has reserved syntax");
        }
        components.push(value);
    }
    if components.is_empty() {
        return Err("an asset path must name a file");
    }
    Ok(components.join("/"))
}

fn validate_page_within_root(
    atlas_path: &Path,
    page_name: &str,
    page_path: &Path,
    root: &Path,
) -> Result<(), PrepareError> {
    if page_path.strip_prefix(root).is_err() {
        return Err(PrepareError::DisallowedPageReference {
            atlas_path: atlas_path.to_owned(),
            page: page_name.into(),
            reason: "the page path escapes the authorized bundle root",
        });
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) enum PrepareError {
    UnsupportedSkeletonPath {
        path: PathBuf,
    },
    Io {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    NotAFile {
        role: &'static str,
        path: PathBuf,
    },
    NotADirectory {
        role: &'static str,
        path: PathBuf,
    },
    OutsideBundleRoot {
        role: &'static str,
        path: PathBuf,
        root: PathBuf,
    },
    MissingAtlas {
        json_path: PathBuf,
        expected_path: PathBuf,
    },
    AmbiguousAtlas {
        json_path: PathBuf,
        candidates: Vec<PathBuf>,
    },
    InvalidExport {
        json_path: PathBuf,
        atlas_path: PathBuf,
        source: Box<spinal::LoadError>,
    },
    WrongSpineVersion {
        expected: &'static str,
        actual: Box<str>,
    },
    DisallowedPageReference {
        atlas_path: PathBuf,
        page: Box<str>,
        reason: &'static str,
    },
    PageUnavailable {
        page: Box<str>,
        path: PathBuf,
        source: io::Error,
    },
    InvalidAssetPath {
        path: PathBuf,
        reason: &'static str,
    },
}

impl fmt::Display for PrepareError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSkeletonPath { path } => write!(
                formatter,
                "`{}` is not a Spine JSON export; choose a `.json` file (binary `.skel` exports are not supported)",
                path.display()
            ),
            Self::Io {
                action,
                path,
                source,
            } => write!(
                formatter,
                "could not {action} `{}`: {source}",
                path.display()
            ),
            Self::NotAFile { role, path } => {
                write!(
                    formatter,
                    "the {role} path `{}` is not a file",
                    path.display()
                )
            }
            Self::NotADirectory { role, path } => write!(
                formatter,
                "the {role} path `{}` is not a directory",
                path.display()
            ),
            Self::OutsideBundleRoot { role, path, root } => write!(
                formatter,
                "the {role} `{}` is outside the authorized bundle root `{}`; pass --bundle-root DIR naming a directory that contains the JSON, atlas, and pages",
                path.display(),
                root.display()
            ),
            Self::MissingAtlas {
                json_path,
                expected_path,
            } => write!(
                formatter,
                "no text atlas was found beside `{}` (looked for `{}`); pass --atlas FILE.atlas",
                json_path.display(),
                expected_path.display()
            ),
            Self::AmbiguousAtlas {
                json_path,
                candidates,
            } => {
                write!(
                    formatter,
                    "more than one text atlas was found beside `{}`; pass --atlas with one of:",
                    json_path.display()
                )?;
                for candidate in candidates {
                    write!(formatter, "\n  {}", candidate.display())?;
                }
                Ok(())
            }
            Self::InvalidExport {
                json_path,
                atlas_path,
                source,
            } => write!(
                formatter,
                "Spinal could not load JSON `{}` with atlas `{}`: {source}",
                json_path.display(),
                atlas_path.display()
            ),
            Self::WrongSpineVersion { expected, actual } => write!(
                formatter,
                "this viewer requires a Spine {expected} JSON export, but the file declares {actual}; re-export it from Spine {expected}"
            ),
            Self::DisallowedPageReference {
                atlas_path,
                page,
                reason,
            } => write!(
                formatter,
                "atlas `{}` has disallowed page path `{page}`: {reason}",
                atlas_path.display()
            ),
            Self::PageUnavailable { page, path, source } => write!(
                formatter,
                "atlas page `{page}` was not found at `{}`: {source}; keep the image at that relative path or correct the atlas",
                path.display()
            ),
            Self::InvalidAssetPath { path, reason } => write!(
                formatter,
                "`{}` cannot be represented as a Bevy asset path: {reason}",
                path.display()
            ),
        }
    }
}

impl Error for PrepareError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } | Self::PageUnavailable { source, .. } => Some(source),
            Self::InvalidExport { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TempDirectory(PathBuf);

    impl TempDirectory {
        fn new() -> Self {
            let ordinal = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "spinal-viewer-source-{}-{ordinal}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create isolated test directory");
            Self(path)
        }

        fn path(&self, relative: impl AsRef<Path>) -> PathBuf {
            self.0.join(relative)
        }

        fn write(&self, relative: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> PathBuf {
            let path = self.path(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create fixture directory");
            }
            fs::write(&path, bytes).expect("write fixture");
            path
        }
    }

    impl Drop for TempDirectory {
        fn drop(&mut self) {
            let _ignored = fs::remove_dir_all(&self.0);
        }
    }

    fn file_snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        fn visit(root: &Path, directory: &Path, snapshot: &mut Vec<(PathBuf, Vec<u8>)>) {
            let mut entries = fs::read_dir(directory)
                .expect("read fixture directory")
                .collect::<Result<Vec<_>, _>>()
                .expect("read fixture entries");
            entries.sort_by_key(|entry| entry.path());
            for entry in entries {
                let path = entry.path();
                if path.is_dir() {
                    visit(root, &path, snapshot);
                } else {
                    snapshot.push((
                        path.strip_prefix(root)
                            .expect("fixture file remains below root")
                            .to_owned(),
                        fs::read(&path).expect("read fixture file"),
                    ));
                }
            }
        }

        let mut snapshot = Vec::new();
        visit(root, root, &mut snapshot);
        snapshot
    }

    fn skeleton_json(version: &str) -> String {
        format!(r#"{{"skeleton":{{"spine":"{version}"}},"bones":[{{"name":"root"}}]}}"#)
    }

    fn atlas_page(name: &str, pma: bool) -> String {
        format!(
            "{name}\n\tsize: 1, 1\n\tformat: RGBA8888\n\tfilter: Linear, Linear\n\trepeat: none\n\tpma: {pma}\n"
        )
    }

    fn options(json_path: PathBuf) -> Options {
        Options {
            json_path,
            atlas_path: None,
            bundle_root: None,
            preview_rate: PreviewRate::default(),
        }
    }

    #[test]
    fn parses_paths_fps_equals_forms_and_help() {
        let ParseResult::Run(options) = Options::parse([
            "Project one/cat.spine.json".to_owned(),
            "--atlas=Project one/cat atlas.atlas".to_owned(),
            "--bundle-root".to_owned(),
            "Project one".to_owned(),
            "--fps".to_owned(),
            "48".to_owned(),
        ])
        .expect("valid arguments") else {
            panic!("expected run options");
        };
        assert_eq!(options.json_path(), Path::new("Project one/cat.spine.json"));
        assert_eq!(
            options.atlas_path(),
            Some(Path::new("Project one/cat atlas.atlas"))
        );
        assert_eq!(options.bundle_root(), Some(Path::new("Project one")));
        assert_eq!(options.preview_rate().fps(), 48);
        assert_eq!(
            Options::parse(["--help".to_owned()]).expect("help does not need a path"),
            ParseResult::Help
        );
    }

    #[test]
    fn fps_must_be_a_single_positive_integer() {
        for value in ["1.5", "-1", "many"] {
            assert!(matches!(
                Options::parse(["cat.json".to_owned(), "--fps".to_owned(), value.to_owned()]),
                Err(OptionsError::InvalidFps(_))
            ));
        }
        assert!(matches!(
            Options::parse(["cat.json".to_owned(), "--fps".to_owned(), "0".to_owned()]),
            Err(OptionsError::InvalidPreviewRate(InvalidPreviewRate::Zero))
        ));
        let too_fine = Options::parse([
            "cat.json".to_owned(),
            "--fps".to_owned(),
            "1000000001".to_owned(),
        ])
        .expect_err("sub-nanosecond frame grids are not representable");
        assert!(matches!(
            &too_fine,
            OptionsError::InvalidPreviewRate(InvalidPreviewRate::ExceedsNanosecondResolution {
                fps: 1_000_000_001
            })
        ));
        assert!(too_fine.to_string().contains("1,000,000,000 FPS"));
        assert!(matches!(
            Options::parse([
                "cat.json".to_owned(),
                "--fps=30".to_owned(),
                "--fps".to_owned(),
                "60".to_owned()
            ]),
            Err(OptionsError::DuplicateOption("--fps"))
        ));
        assert!(matches!(
            Options::parse([
                "cat.json".to_owned(),
                "--bundle-root=one".to_owned(),
                "--bundle-root".to_owned(),
                "two".to_owned(),
            ]),
            Err(OptionsError::DuplicateOption("--bundle-root"))
        ));
        assert!(matches!(
            Options::parse(["cat.json".to_owned(), "--bundle-root=".to_owned()]),
            Err(OptionsError::EmptyValue("--bundle-root"))
        ));
    }

    #[test]
    fn rejects_binary_and_extra_paths_before_preflight() {
        let directory = TempDirectory::new();
        let binary = directory.write("cat.skel", b"binary");
        assert!(matches!(
            PreparedSource::load(options(binary)),
            Err(PrepareError::UnsupportedSkeletonPath { .. })
        ));
        assert!(matches!(
            Options::parse(["one.json".to_owned(), "two.json".to_owned()]),
            Err(OptionsError::UnexpectedJsonPath(_))
        ));
    }

    #[test]
    fn falls_back_to_the_only_mismatched_atlas_name() {
        let directory = TempDirectory::new();
        let json = directory.write("export/cat.json", skeleton_json("4.3.23"));
        let atlas = directory.write("export/from-jose.atlas", atlas_page("cat.png", false));
        directory.write("export/cat.png", b"not decoded during preflight");

        let prepared = PreparedSource::load(options(json)).expect("unique atlas fallback");
        assert_eq!(prepared.atlas_path(), atlas.canonicalize().unwrap());
        assert_eq!(prepared.atlas_reference(), "from-jose.atlas");
    }

    #[test]
    fn compound_json_name_prefers_the_conventional_atlas() {
        let directory = TempDirectory::new();
        let json = directory.write("cat.spine.json", skeleton_json("4.3.23"));
        let atlas = directory.write("cat.atlas", atlas_page("cat.png", false));
        directory.write("cat.png", b"image");
        directory.write("other.atlas", atlas_page("other.png", false));
        directory.write("other.png", b"image");

        let prepared = PreparedSource::load(options(json)).expect("conventional atlas wins");
        assert_eq!(prepared.atlas_path(), atlas.canonicalize().unwrap());
    }

    #[test]
    fn ambiguous_atlas_lists_sorted_candidates() {
        let directory = TempDirectory::new();
        let json = directory.write("cat.json", skeleton_json("4.3.23"));
        let second = directory.write("z.atlas", atlas_page("z.png", false));
        let first = directory.write("a.atlas", atlas_page("a.png", false));

        let error = PreparedSource::load(options(json)).expect_err("atlas choice is ambiguous");
        let PrepareError::AmbiguousAtlas { candidates, .. } = &error else {
            panic!("expected ambiguous-atlas error");
        };
        assert_eq!(
            candidates,
            &[
                first.canonicalize().unwrap(),
                second.canonicalize().unwrap()
            ]
        );
        assert!(error.to_string().contains("pass --atlas"));
    }

    #[test]
    fn preflight_is_read_only_and_creates_no_sidecars() {
        let directory = TempDirectory::new();
        let json = directory.write("cat.json", skeleton_json("4.3.23"));
        directory.write("cat.atlas", atlas_page("pages/cat.png", false));
        directory.write("pages/cat.png", b"unchanged image bytes");
        directory.write("artist-notes.txt", b"do not touch");
        let before = file_snapshot(&directory.0);

        let prepared = PreparedSource::load(options(json)).expect("valid read-only preflight");
        assert_eq!(prepared.skeleton().spine_version(), "4.3.23");

        assert_eq!(file_snapshot(&directory.0), before);
    }

    #[test]
    fn bundle_inventory_contains_only_referenced_export_files() {
        let directory = TempDirectory::new();
        let json_bytes = skeleton_json("4.3.23");
        let atlas_bytes = atlas_page("pages/cat.png", false);
        let json = directory.write("export/cat.json", &json_bytes);
        directory.write("export/cat.atlas", &atlas_bytes);
        directory.write("export/pages/cat.png", b"page bytes");
        directory.write("export/artist-notes.txt", b"not part of the runtime export");

        let prepared = PreparedSource::load(options(json)).expect("valid export");
        assert_eq!(
            prepared
                .bundle()
                .file_paths()
                .map(Path::to_owned)
                .collect::<Vec<_>>(),
            [
                PathBuf::from("cat.atlas"),
                PathBuf::from("cat.json"),
                PathBuf::from("pages/cat.png"),
            ]
        );
        assert_eq!(
            prepared.bundle().file(Path::new("cat.json")),
            Some(json_bytes.as_bytes())
        );
        assert_eq!(
            prepared.bundle().file(Path::new("cat.atlas")),
            Some(atlas_bytes.as_bytes())
        );
        assert_eq!(
            prepared.bundle().file(Path::new("pages/cat.png")),
            Some(b"page bytes".as_slice())
        );
        assert!(
            prepared
                .bundle()
                .file(Path::new("artist-notes.txt"))
                .is_none()
        );
    }

    #[test]
    fn bundle_bytes_do_not_follow_filesystem_mutation() {
        let directory = TempDirectory::new();
        let json_bytes = skeleton_json("4.3.23");
        let atlas_bytes = atlas_page("cat.png", false);
        let json = directory.write("cat.json", &json_bytes);
        let atlas = directory.write("cat.atlas", &atlas_bytes);
        let page = directory.write("cat.png", b"original page bytes");

        let prepared = PreparedSource::load(options(json.clone())).expect("valid export snapshot");
        fs::write(json, b"mutated JSON").expect("mutate source JSON");
        fs::write(atlas, b"mutated atlas").expect("mutate source atlas");
        fs::write(page, b"mutated page").expect("mutate source page");

        assert_eq!(
            prepared.bundle().file(Path::new("cat.json")),
            Some(json_bytes.as_bytes())
        );
        assert_eq!(
            prepared.bundle().file(Path::new("cat.atlas")),
            Some(atlas_bytes.as_bytes())
        );
        assert_eq!(
            prepared.bundle().file(Path::new("cat.png")),
            Some(b"original page bytes".as_slice())
        );
    }

    #[test]
    fn explicit_atlas_wins_over_the_conventional_one() {
        let directory = TempDirectory::new();
        let json = directory.write("cat.json", skeleton_json("4.3.23"));
        directory.write("cat.atlas", atlas_page("unused.png", false));
        let explicit = directory.write("chosen.atlas", atlas_page("chosen.png", false));
        directory.write("chosen.png", b"image");
        let mut options = options(json);
        options.atlas_path = Some(explicit.clone());

        let prepared = PreparedSource::load(options).expect("explicit atlas");
        assert_eq!(prepared.atlas_path(), explicit.canonicalize().unwrap());
        assert_eq!(prepared.atlas_reference(), "chosen.atlas");
    }

    #[test]
    fn spaces_survive_canonical_and_bevy_relative_paths() {
        let directory = TempDirectory::new();
        let json = directory.write(
            "Project one cat/Export files/Miso Cat.json",
            skeleton_json("4.3.23"),
        );
        directory.write(
            "Project one cat/Export files/Miso Cat.atlas",
            atlas_page("Miso Cat.png", false),
        );
        directory.write("Project one cat/Export files/Miso Cat.png", b"image");

        let prepared = PreparedSource::load(options(json)).expect("paths with spaces");
        assert_eq!(prepared.json_name(), "Miso Cat.json");
        assert_eq!(prepared.json_asset_path(), "Miso Cat.json");
        assert_eq!(prepared.atlas_reference(), "Miso Cat.atlas");
    }

    #[test]
    fn default_bundle_root_does_not_authorize_sibling_directories() {
        let directory = TempDirectory::new();
        let json = directory.write("bundle/skeletons/cat.json", skeleton_json("4.3.23"));
        let atlas = directory.write("bundle/atlases/cat.atlas", atlas_page("cat.png", false));
        directory.write("bundle/atlases/cat.png", b"image");
        let mut options = options(json);
        options.atlas_path = Some(atlas);

        let error = PreparedSource::load(options).expect_err("JSON parent is the default boundary");
        assert!(matches!(
            error,
            PrepareError::OutsideBundleRoot {
                role: "text atlas",
                ..
            }
        ));
        assert!(error.to_string().contains("--bundle-root"));
    }

    #[test]
    fn explicit_bundle_root_supports_nested_multipage_exports() {
        let directory = TempDirectory::new();
        let json = directory.write("bundle/skeletons/cat.json", skeleton_json("4.3.23"));
        let mut atlas = atlas_page("pages/body.png", false);
        atlas.push('\n');
        atlas.push_str(&atlas_page("../textures/details.png", true));
        let atlas_path = directory.write("bundle/atlases/cat.atlas", atlas);
        directory.write("bundle/atlases/pages/body.png", b"image");
        directory.write("bundle/textures/details.png", b"image");
        let mut options = options(json);
        options.atlas_path = Some(atlas_path);
        options.bundle_root = Some(directory.path("bundle"));

        let prepared = PreparedSource::load(options).expect("nested multipage export");
        assert_eq!(prepared.json_asset_path(), "skeletons/cat.json");
        assert_eq!(prepared.atlas_reference(), "../atlases/cat.atlas");
        assert_eq!(
            prepared
                .pages()
                .iter()
                .map(PreparedPage::name)
                .collect::<Vec<_>>(),
            ["pages/body.png", "../textures/details.png"]
        );
        assert_eq!(
            prepared
                .premultiplied_pages()
                .map(PreparedPage::name)
                .collect::<Vec<_>>(),
            ["../textures/details.png"]
        );
        assert_eq!(prepared.preview_fps(), 30);
        assert_eq!(prepared.skeleton().spine_version(), "4.3.23");
    }

    #[test]
    fn page_reference_cannot_escape_the_authorized_bundle_root() {
        let directory = TempDirectory::new();
        let json = directory.write("trusted/skeletons/cat.json", skeleton_json("4.3.23"));
        let atlas = directory.write(
            "trusted/atlases/cat.atlas",
            atlas_page("../../outside.png", false),
        );
        directory.write("outside.png", b"outside the export root");
        let mut options = options(json);
        options.atlas_path = Some(atlas);
        options.bundle_root = Some(directory.path("trusted"));

        let error = PreparedSource::load(options).expect_err("page must remain in export root");
        assert!(matches!(
            error,
            PrepareError::DisallowedPageReference { ref page, .. }
                if page.as_ref() == "../../outside.png"
        ));
        assert!(error.to_string().contains("escapes"));
    }

    #[test]
    fn windows_drive_relative_page_reference_is_rejected_before_file_io() {
        assert_eq!(
            validate_embedded_reference("C:outside.png"),
            Err("Windows drive-prefixed page paths are not allowed")
        );
    }

    #[test]
    fn missing_atlas_and_page_are_actionable() {
        let directory = TempDirectory::new();
        let json = directory.write("missing-atlas.json", skeleton_json("4.3.23"));
        assert!(matches!(
            PreparedSource::load(options(json)),
            Err(PrepareError::MissingAtlas { .. })
        ));

        let json = directory.write("missing-page.json", skeleton_json("4.3.23"));
        directory.write(
            "missing-page.atlas",
            atlas_page("images/not-there.png", false),
        );
        let error = PreparedSource::load(options(json)).expect_err("page must exist");
        assert!(matches!(error, PrepareError::PageUnavailable { .. }));
        assert!(error.to_string().contains("images/not-there.png"));
    }

    #[test]
    fn invalid_export_and_wrong_patch_have_distinct_errors() {
        let directory = TempDirectory::new();
        let invalid = directory.write("invalid.json", b"not JSON");
        directory.write("invalid.atlas", atlas_page("page.png", false));
        assert!(matches!(
            PreparedSource::load(options(invalid)),
            Err(PrepareError::InvalidExport { .. })
        ));

        let future = directory.write("future.json", skeleton_json("4.3.24"));
        directory.write("future.atlas", atlas_page("page.png", false));
        let error = PreparedSource::load(options(future)).expect_err("exact patch required");
        assert!(matches!(
            error,
            PrepareError::WrongSpineVersion { ref actual, .. } if actual.as_ref() == "4.3.24"
        ));
    }

    #[test]
    fn source_switching_page_reference_is_rejected_before_file_io() {
        let directory = TempDirectory::new();
        let json = directory.write("cat.json", skeleton_json("4.3.23"));
        directory.write("cat.atlas", atlas_page("other://cat.png", false));

        let error = PreparedSource::load(options(json)).expect_err("source switching rejected");
        assert!(matches!(
            error,
            PrepareError::DisallowedPageReference { ref page, .. }
                if page.as_ref() == "other://cat.png"
        ));
    }
}

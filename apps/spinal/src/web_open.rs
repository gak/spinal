//! Read-only browser acquisition for one required and one optional local Spine runtime export.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::{Path, PathBuf},
};

use crate::web_manifest::MAX_BROWSER_BUNDLE_BYTES;
use bevy_spinal::spinal::{
    MAX_RUNTIME_ATLAS_BYTES, MAX_RUNTIME_BUNDLE_BYTES, MAX_RUNTIME_FILE_COUNT,
    MAX_RUNTIME_JSON_BYTES, MAX_RUNTIME_PAGE_BYTES,
};
#[cfg(target_arch = "wasm32")]
use bevy_spinal::spinal::{RuntimeBundleError, RuntimeBundleManifest};

#[cfg(target_arch = "wasm32")]
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[cfg(target_arch = "wasm32")]
use js_sys::{Function, JsString, Reflect, Uint8Array};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::{JsFuture, spawn_local};
#[cfg(target_arch = "wasm32")]
use web_sys::{Document, Element, Event, File, HtmlInputElement};

#[cfg(target_arch = "wasm32")]
use crate::{
    bundle::SourceBundle, preview::PreviewRate, runtime::LaunchConfig,
    web_manifest::BrowserLaunchBundles,
};

#[cfg(target_arch = "wasm32")]
const OPEN_PANEL_ELEMENT_ID: &str = "spinal-open-panel";
#[cfg(target_arch = "wasm32")]
const OPEN_FORM_ELEMENT_ID: &str = "spinal-open-form";
#[cfg(target_arch = "wasm32")]
const OPEN_PRIMARY_INPUT_ELEMENT_ID: &str = "spinal-open-files";
#[cfg(target_arch = "wasm32")]
const OPEN_COMPARISON_INPUT_ELEMENT_ID: &str = "spinal-open-comparison-files";
#[cfg(target_arch = "wasm32")]
const OPEN_ERROR_ELEMENT_ID: &str = "spinal-open-error";
#[cfg(target_arch = "wasm32")]
const OPEN_SUBMIT_ELEMENT_ID: &str = "spinal-open-submit";
#[cfg(target_arch = "wasm32")]
const VIEWER_ELEMENT_ID: &str = "spinal-viewer";
#[cfg(target_arch = "wasm32")]
const VIEWER_CANVAS_ELEMENT_ID: &str = "spinal-canvas";
/// The browser must enumerate every selected entry before it reads any bytes.
/// This limit leaves room for safe unrelated metadata while keeping that
/// enumeration independent of the smaller runtime-bundle file limit.
const MAX_SELECTED_FILE_COUNT: usize = MAX_RUNTIME_FILE_COUNT * 2;
const MAX_SELECTED_PATH_BYTES: usize = 2_048;
const MAX_SELECTED_COMPONENT_BYTES: usize = 255;
const MAX_SELECTED_METADATA_BYTES: usize = MAX_SELECTED_FILE_COUNT * MAX_SELECTED_PATH_BYTES * 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OpenSourceRole {
    Primary,
    Comparison,
}

impl OpenSourceRole {
    const fn label(self) -> &'static str {
        match self {
            Self::Primary => "Primary",
            Self::Comparison => "Comparison",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RequiredRole {
    Json,
    Atlas,
    Page,
}

impl RequiredRole {
    const fn label(self) -> &'static str {
        match self {
            Self::Json => "Spine JSON",
            Self::Atlas => "text atlas",
            Self::Page => "atlas page PNG",
        }
    }

    const fn byte_limit(self) -> usize {
        match self {
            Self::Json => MAX_RUNTIME_JSON_BYTES,
            Self::Atlas => MAX_RUNTIME_ATLAS_BYTES,
            Self::Page => MAX_RUNTIME_PAGE_BYTES,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum OpenSelectionError {
    EmptySelection,
    TooManySelectedFiles {
        actual: usize,
    },
    MetadataBudgetExceeded,
    InvalidBrowserMetadata,
    UnsafeSelectedPath,
    MixedSelectionLayouts,
    RootlessDirectoryPath,
    MixedDirectoryRoots,
    MetadataNameMismatch,
    DuplicatePath,
    PortablePathCollision,
    MissingJson,
    AmbiguousJson {
        actual: usize,
    },
    MissingAtlas,
    AmbiguousAtlas {
        actual: usize,
    },
    EmptyRequiredFile {
        role: RequiredRole,
    },
    RequiredFileTooLarge {
        role: RequiredRole,
        limit: usize,
    },
    TooManyRequiredFiles {
        actual: usize,
    },
    BundleTooLarge,
    InvalidRequiredPage,
    MissingRequiredPage {
        path: PathBuf,
    },
    #[cfg(target_arch = "wasm32")]
    FileReadFailed {
        role: RequiredRole,
    },
    #[cfg(target_arch = "wasm32")]
    FileLengthChanged {
        role: RequiredRole,
    },
    #[cfg(target_arch = "wasm32")]
    InvalidRuntimeExport(Box<str>),
    #[cfg(target_arch = "wasm32")]
    MissingShellElement(&'static str),
    #[cfg(target_arch = "wasm32")]
    InvalidShellElement(&'static str),
    #[cfg(target_arch = "wasm32")]
    ShellUpdateFailed,
}

impl OpenSelectionError {
    #[cfg(target_arch = "wasm32")]
    fn from_runtime(error: RuntimeBundleError) -> Self {
        let detail: Box<str> = match error {
            RuntimeBundleError::WrongSpineVersion { expected, .. } => format!(
                "This viewer requires a Spine {expected} JSON export. Re-export the selected source with that exact Spine version."
            )
            .into_boxed_str(),
            RuntimeBundleError::InvalidExport(_source) =>
                "The selected JSON and atlas are not one compatible Spine runtime export."
                    .into(),
            RuntimeBundleError::InvalidPageReference { .. }
            | RuntimeBundleError::DuplicateDependencyPath(_) =>
                "The selected atlas contains an unsafe or duplicate page reference."
                    .into(),
            RuntimeBundleError::InvalidTexture { .. } =>
                "A required atlas page is not a supported, fully decodable PNG."
                    .into(),
            RuntimeBundleError::DecodedTextureBudgetExceeded =>
                "The selected atlas pages exceed the decoded texture budget."
                    .into(),
            _other =>
                "The selected files do not form one complete, valid Spine runtime export."
                    .into(),
        };
        Self::InvalidRuntimeExport(detail)
    }
}

impl fmt::Display for OpenSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySelection => formatter.write_str(
                "No files were selected. Choose the required Primary runtime-export directory.",
            ),
            Self::TooManySelectedFiles { actual } => write!(
                formatter,
                "The selected directory contains {actual} files; the browser can inspect at most {MAX_SELECTED_FILE_COUNT}. Choose the runtime-export directory only."
            ),
            Self::MetadataBudgetExceeded => formatter.write_str(
                "The selected directory has too much filename metadata. Choose a smaller runtime-export directory.",
            ),
            Self::InvalidBrowserMetadata => formatter.write_str(
                "The browser returned invalid file metadata. Choose the runtime-export directory again.",
            ),
            Self::UnsafeSelectedPath => formatter.write_str(
                "The selected directory contains an unsafe filename. Names must be portable relative paths without backslashes, control characters, or dot segments.",
            ),
            Self::MixedSelectionLayouts => formatter.write_str(
                "The browser mixed flat files with directory entries. Choose one runtime-export directory again.",
            ),
            Self::RootlessDirectoryPath => formatter.write_str(
                "The browser did not provide one directory root for every selected file. Choose the runtime-export directory again.",
            ),
            Self::MixedDirectoryRoots => formatter.write_str(
                "Files from more than one directory root were selected. Choose exactly one runtime-export directory.",
            ),
            Self::MetadataNameMismatch => formatter.write_str(
                "The browser returned inconsistent file names. Choose the runtime-export directory again.",
            ),
            Self::DuplicatePath => formatter.write_str(
                "The selected directory contains duplicate relative file paths. Remove the duplicate and try again.",
            ),
            Self::PortablePathCollision => formatter.write_str(
                "Two selected files have names that collide on case-insensitive systems. Rename one file and try again.",
            ),
            Self::MissingJson => formatter.write_str(
                "The selected directory has no Spine JSON file. Choose a runtime export containing exactly one `.json` file.",
            ),
            Self::AmbiguousJson { actual } => write!(
                formatter,
                "The selected directory has {actual} `.json` files. Choose a runtime export containing exactly one Spine JSON file and remove unrelated JSON metadata."
            ),
            Self::MissingAtlas => formatter.write_str(
                "The selected directory has no text atlas. Choose a runtime export containing exactly one `.atlas` file.",
            ),
            Self::AmbiguousAtlas { actual } => write!(
                formatter,
                "The selected directory has {actual} `.atlas` files. Choose a runtime export containing exactly one text atlas."
            ),
            Self::EmptyRequiredFile { role } => {
                write!(formatter, "The selected {} is empty.", role.label())
            }
            Self::RequiredFileTooLarge { role, limit } => write!(
                formatter,
                "The selected {} exceeds its {limit}-byte limit.",
                role.label()
            ),
            Self::TooManyRequiredFiles { actual } => write!(
                formatter,
                "The runtime export needs {actual} files; at most {MAX_RUNTIME_FILE_COUNT} JSON, atlas, and page files are supported."
            ),
            Self::BundleTooLarge => write!(
                formatter,
                "The required runtime-export files exceed the {MAX_RUNTIME_BUNDLE_BYTES}-byte bundle limit."
            ),
            Self::InvalidRequiredPage => formatter.write_str(
                "The atlas resolved an invalid or duplicate page path. Re-export the runtime package and try again.",
            ),
            Self::MissingRequiredPage { path } => write!(
                formatter,
                "The atlas requires `{}`, but that PNG is missing from the selected directory. Choose the complete runtime export.",
                path.display()
            ),
            #[cfg(target_arch = "wasm32")]
            Self::FileReadFailed { role } => write!(
                formatter,
                "The browser could not read the selected {}. Choose the directory again.",
                role.label()
            ),
            #[cfg(target_arch = "wasm32")]
            Self::FileLengthChanged { role } => write!(
                formatter,
                "The selected {} changed while it was being opened. Choose the directory again.",
                role.label()
            ),
            #[cfg(target_arch = "wasm32")]
            Self::InvalidRuntimeExport(detail) => formatter.write_str(detail),
            #[cfg(target_arch = "wasm32")]
            Self::MissingShellElement(id) => {
                write!(formatter, "the page is missing required element `{id}`")
            }
            #[cfg(target_arch = "wasm32")]
            Self::InvalidShellElement(id) => {
                write!(formatter, "the page has an invalid `{id}` element")
            }
            #[cfg(target_arch = "wasm32")]
            Self::ShellUpdateFailed => {
                formatter.write_str("the page could not update the Open viewer surface")
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum OpenFormError {
    Source {
        role: OpenSourceRole,
        source: OpenSelectionError,
    },
    CombinedSelectedFileBudgetExceeded {
        actual: usize,
    },
    CombinedMetadataBudgetExceeded,
    CombinedRequiredFileBudgetExceeded {
        actual: usize,
    },
    CombinedRequiredByteBudgetExceeded {
        actual: usize,
    },
    #[cfg(target_arch = "wasm32")]
    CombinedRuntimeBudgetExceeded,
}

impl OpenFormError {
    const fn source(role: OpenSourceRole, source: OpenSelectionError) -> Self {
        Self::Source { role, source }
    }
}

impl fmt::Display for OpenFormError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source { role, source } => {
                write!(formatter, "{} directory: {source}", role.label())
            }
            Self::CombinedSelectedFileBudgetExceeded { actual } => write!(
                formatter,
                "The selected Primary and Comparison directories contain {actual} files together; the browser can inspect at most {MAX_SELECTED_FILE_COUNT}. Choose the runtime exports only."
            ),
            Self::CombinedMetadataBudgetExceeded => formatter.write_str(
                "The selected Primary and Comparison directories have too much filename metadata together. Choose smaller runtime-export directories.",
            ),
            Self::CombinedRequiredFileBudgetExceeded { actual } => write!(
                formatter,
                "The Primary and Comparison runtime exports require {actual} files together; the viewer supports at most {MAX_RUNTIME_FILE_COUNT}. Choose smaller exports or omit Comparison."
            ),
            Self::CombinedRequiredByteBudgetExceeded { actual } => write!(
                formatter,
                "The Primary and Comparison runtime exports require {actual} encoded bytes together; the viewer supports at most {MAX_BROWSER_BUNDLE_BYTES}. Choose smaller exports or omit Comparison."
            ),
            #[cfg(target_arch = "wasm32")]
            Self::CombinedRuntimeBudgetExceeded => formatter.write_str(
                "The Primary and Comparison runtime exports cannot be opened together because their atlas pages exceed the viewer's combined decoded texture budget. Choose smaller exports or omit Comparison.",
            ),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RawFileMetadata {
    browser_index: usize,
    name: Box<str>,
    relative_path: Box<str>,
    byte_length: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NormalizedFileMetadata {
    browser_index: usize,
    path: PathBuf,
    byte_length: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SelectionFootprint {
    file_count: usize,
    metadata_bytes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct NormalizedSelection {
    files: Vec<NormalizedFileMetadata>,
    footprint: SelectionFootprint,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SelectionPreflight {
    normalized: NormalizedSelection,
    candidates: CandidatePair,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CandidatePair {
    json_position: usize,
    atlas_position: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RequiredFilePlan {
    positions: Vec<usize>,
    encoded_bytes: usize,
}

fn normalize_selection_with_footprint(
    raw_files: &[RawFileMetadata],
) -> Result<NormalizedSelection, OpenSelectionError> {
    if raw_files.is_empty() {
        return Err(OpenSelectionError::EmptySelection);
    }
    if raw_files.len() > MAX_SELECTED_FILE_COUNT {
        return Err(OpenSelectionError::TooManySelectedFiles {
            actual: raw_files.len(),
        });
    }

    let mut metadata_bytes = 0_usize;
    let mut browser_indexes = BTreeSet::new();
    for raw in raw_files {
        metadata_bytes = metadata_bytes
            .checked_add(raw.name.len())
            .and_then(|total| total.checked_add(raw.relative_path.len()))
            .ok_or(OpenSelectionError::MetadataBudgetExceeded)?;
        if metadata_bytes > MAX_SELECTED_METADATA_BYTES
            || !browser_indexes.insert(raw.browser_index)
        {
            return Err(OpenSelectionError::MetadataBudgetExceeded);
        }
        let name_parts = validate_portable_path(&raw.name)?;
        if name_parts.len() != 1 {
            return Err(OpenSelectionError::UnsafeSelectedPath);
        }
    }

    let rooted = !raw_files[0].relative_path.is_empty();
    if raw_files
        .iter()
        .any(|raw| raw.relative_path.is_empty() == rooted)
    {
        return Err(OpenSelectionError::MixedSelectionLayouts);
    }

    let mut common_root: Option<&str> = None;
    let mut normalized = Vec::with_capacity(raw_files.len());
    for raw in raw_files {
        let path = if rooted {
            let parts = validate_portable_path(&raw.relative_path)?;
            if parts.len() < 2 {
                return Err(OpenSelectionError::RootlessDirectoryPath);
            }
            let root = parts[0];
            match common_root {
                Some(expected) if expected != root => {
                    return Err(OpenSelectionError::MixedDirectoryRoots);
                }
                None => common_root = Some(root),
                Some(_expected) => {}
            }
            if parts.last().copied() != Some(raw.name.as_ref()) {
                return Err(OpenSelectionError::MetadataNameMismatch);
            }
            PathBuf::from(parts[1..].join("/"))
        } else {
            PathBuf::from(raw.name.as_ref())
        };
        normalized.push(NormalizedFileMetadata {
            browser_index: raw.browser_index,
            path,
            byte_length: raw.byte_length,
        });
    }

    let mut exact_paths = BTreeSet::new();
    let mut portable_paths = BTreeSet::new();
    for file in &normalized {
        if !exact_paths.insert(file.path.clone()) {
            return Err(OpenSelectionError::DuplicatePath);
        }
        if !portable_paths.insert(portable_path_key(&file.path)) {
            return Err(OpenSelectionError::PortablePathCollision);
        }
    }
    Ok(NormalizedSelection {
        files: normalized,
        footprint: SelectionFootprint {
            file_count: raw_files.len(),
            metadata_bytes,
        },
    })
}

#[cfg(test)]
fn normalize_selection(
    raw_files: &[RawFileMetadata],
) -> Result<Vec<NormalizedFileMetadata>, OpenSelectionError> {
    normalize_selection_with_footprint(raw_files).map(|selection| selection.files)
}

fn validate_combined_selection_footprint(
    primary: SelectionFootprint,
    comparison: Option<SelectionFootprint>,
) -> Result<(), OpenFormError> {
    let comparison = comparison.unwrap_or(SelectionFootprint {
        file_count: 0,
        metadata_bytes: 0,
    });
    let file_count = primary
        .file_count
        .checked_add(comparison.file_count)
        .ok_or(OpenFormError::CombinedSelectedFileBudgetExceeded { actual: usize::MAX })?;
    if file_count > MAX_SELECTED_FILE_COUNT {
        return Err(OpenFormError::CombinedSelectedFileBudgetExceeded { actual: file_count });
    }
    let metadata_bytes = primary
        .metadata_bytes
        .checked_add(comparison.metadata_bytes)
        .ok_or(OpenFormError::CombinedMetadataBudgetExceeded)?;
    if metadata_bytes > MAX_SELECTED_METADATA_BYTES {
        return Err(OpenFormError::CombinedMetadataBudgetExceeded);
    }
    Ok(())
}

fn preflight_selection_metadata(
    raw_files: &[RawFileMetadata],
) -> Result<SelectionPreflight, OpenSelectionError> {
    let normalized = normalize_selection_with_footprint(raw_files)?;
    let candidates = derive_candidates(&normalized.files)?;
    precheck_candidates(&normalized.files, candidates)?;
    Ok(SelectionPreflight {
        normalized,
        candidates,
    })
}

fn validate_portable_path(value: &str) -> Result<Vec<&str>, OpenSelectionError> {
    let invalid = value.is_empty()
        || value.len() > MAX_SELECTED_PATH_BYTES
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.contains(['\\', ':', '#', '?', '%'])
        || value.chars().any(char::is_control);
    if invalid {
        return Err(OpenSelectionError::UnsafeSelectedPath);
    }
    let parts = value.split('/').collect::<Vec<_>>();
    if parts.iter().any(|part| {
        part.is_empty()
            || *part == "."
            || *part == ".."
            || part.len() > MAX_SELECTED_COMPONENT_BYTES
            || part.ends_with(['.', ' '])
            || is_reserved_portable_component(part)
    }) {
        return Err(OpenSelectionError::UnsafeSelectedPath);
    }
    Ok(parts)
}

fn is_reserved_portable_component(component: &str) -> bool {
    let stem = component
        .split_once('.')
        .map_or(component, |(stem, _suffix)| stem);
    let upper = stem.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || upper
            .strip_prefix("COM")
            .or_else(|| upper.strip_prefix("LPT"))
            .is_some_and(|suffix| suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9'))
}

fn portable_path_key(path: &Path) -> String {
    path.to_str()
        .expect("normalized selected paths are UTF-8")
        .to_lowercase()
}

fn derive_candidates(
    files: &[NormalizedFileMetadata],
) -> Result<CandidatePair, OpenSelectionError> {
    let json = extension_positions(files, "json");
    let atlas = extension_positions(files, "atlas");
    let json_position = match json.as_slice() {
        [position] => *position,
        [] => return Err(OpenSelectionError::MissingJson),
        _many => {
            return Err(OpenSelectionError::AmbiguousJson { actual: json.len() });
        }
    };
    let atlas_position = match atlas.as_slice() {
        [position] => *position,
        [] => return Err(OpenSelectionError::MissingAtlas),
        _many => {
            return Err(OpenSelectionError::AmbiguousAtlas {
                actual: atlas.len(),
            });
        }
    };
    Ok(CandidatePair {
        json_position,
        atlas_position,
    })
}

fn extension_positions(files: &[NormalizedFileMetadata], extension: &str) -> Vec<usize> {
    files
        .iter()
        .enumerate()
        .filter_map(|(position, file)| {
            file.path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case(extension))
                .then_some(position)
        })
        .collect()
}

fn precheck_candidates(
    files: &[NormalizedFileMetadata],
    candidates: CandidatePair,
) -> Result<(), OpenSelectionError> {
    let json = files
        .get(candidates.json_position)
        .ok_or(OpenSelectionError::InvalidBrowserMetadata)?;
    let atlas = files
        .get(candidates.atlas_position)
        .ok_or(OpenSelectionError::InvalidBrowserMetadata)?;
    precheck_required_file(json, RequiredRole::Json)?;
    precheck_required_file(atlas, RequiredRole::Atlas)?;
    let total = json
        .byte_length
        .checked_add(atlas.byte_length)
        .ok_or(OpenSelectionError::BundleTooLarge)?;
    if total > MAX_RUNTIME_BUNDLE_BYTES {
        return Err(OpenSelectionError::BundleTooLarge);
    }
    Ok(())
}

fn plan_required_files(
    files: &[NormalizedFileMetadata],
    candidates: CandidatePair,
    page_paths: &[PathBuf],
) -> Result<RequiredFilePlan, OpenSelectionError> {
    precheck_candidates(files, candidates)?;
    let actual = page_paths
        .len()
        .checked_add(2)
        .ok_or(OpenSelectionError::TooManyRequiredFiles { actual: usize::MAX })?;
    if actual > MAX_RUNTIME_FILE_COUNT {
        return Err(OpenSelectionError::TooManyRequiredFiles { actual });
    }

    let json = &files[candidates.json_position];
    let atlas = &files[candidates.atlas_position];
    let mut total = json
        .byte_length
        .checked_add(atlas.byte_length)
        .ok_or(OpenSelectionError::BundleTooLarge)?;
    let by_path = files
        .iter()
        .enumerate()
        .map(|(position, file)| (file.path.as_path(), position))
        .collect::<BTreeMap<_, _>>();
    let mut seen_pages = BTreeSet::new();
    let mut positions = Vec::with_capacity(actual);
    positions.extend([candidates.json_position, candidates.atlas_position]);
    for page_path in page_paths {
        let Some(value) = page_path.to_str() else {
            return Err(OpenSelectionError::InvalidRequiredPage);
        };
        validate_portable_path(value)?;
        if page_path == &json.path || page_path == &atlas.path || !seen_pages.insert(page_path) {
            return Err(OpenSelectionError::InvalidRequiredPage);
        }
        let position = *by_path.get(page_path.as_path()).ok_or_else(|| {
            OpenSelectionError::MissingRequiredPage {
                path: page_path.clone(),
            }
        })?;
        let page = &files[position];
        precheck_required_file(page, RequiredRole::Page)?;
        total = total
            .checked_add(page.byte_length)
            .ok_or(OpenSelectionError::BundleTooLarge)?;
        if total > MAX_RUNTIME_BUNDLE_BYTES {
            return Err(OpenSelectionError::BundleTooLarge);
        }
        positions.push(position);
    }
    Ok(RequiredFilePlan {
        positions,
        encoded_bytes: total,
    })
}

fn validate_combined_required_plans(
    primary: &RequiredFilePlan,
    comparison: Option<&RequiredFilePlan>,
) -> Result<(), OpenFormError> {
    let comparison_file_count = comparison.map_or(0, |plan| plan.positions.len());
    let file_count = primary
        .positions
        .len()
        .checked_add(comparison_file_count)
        .ok_or(OpenFormError::CombinedRequiredFileBudgetExceeded { actual: usize::MAX })?;
    if file_count > MAX_RUNTIME_FILE_COUNT {
        return Err(OpenFormError::CombinedRequiredFileBudgetExceeded { actual: file_count });
    }
    let encoded_bytes = primary
        .encoded_bytes
        .checked_add(comparison.map_or(0, |plan| plan.encoded_bytes))
        .ok_or(OpenFormError::CombinedRequiredByteBudgetExceeded { actual: usize::MAX })?;
    if encoded_bytes > MAX_BROWSER_BUNDLE_BYTES {
        return Err(OpenFormError::CombinedRequiredByteBudgetExceeded {
            actual: encoded_bytes,
        });
    }
    Ok(())
}

fn precheck_required_file(
    file: &NormalizedFileMetadata,
    role: RequiredRole,
) -> Result<(), OpenSelectionError> {
    if file.byte_length == 0 {
        return Err(OpenSelectionError::EmptyRequiredFile { role });
    }
    let limit = role.byte_limit();
    if file.byte_length > limit {
        return Err(OpenSelectionError::RequiredFileTooLarge { role, limit });
    }
    Ok(())
}

/// One fully validated browser launch with a required Primary and optional Comparison.
#[cfg(target_arch = "wasm32")]
pub(super) struct OpenLaunch {
    label: Box<str>,
    config: LaunchConfig,
}

#[cfg(target_arch = "wasm32")]
impl OpenLaunch {
    /// Consumes the local acquisition result for the ordinary browser runner.
    pub(super) fn into_parts(self) -> (Box<str>, LaunchConfig) {
        (self.label, self.config)
    }
}

#[cfg(target_arch = "wasm32")]
type SharedOpenLaunchCallback = Rc<RefCell<Option<Box<dyn FnOnce(OpenLaunch)>>>>;

#[cfg(target_arch = "wasm32")]
struct BrowserSelection {
    files: Vec<File>,
    metadata: Vec<NormalizedFileMetadata>,
    footprint: SelectionFootprint,
    candidates: CandidatePair,
}

#[cfg(target_arch = "wasm32")]
struct PreparedBrowserSource {
    selection: BrowserSelection,
    source_role: OpenSourceRole,
    json_path: PathBuf,
    atlas_path: PathBuf,
    json: Vec<u8>,
    atlas: Vec<u8>,
    plan: RequiredFilePlan,
}

/// Installs one persistent, retryable Open viewer form listener.
#[cfg(target_arch = "wasm32")]
pub(super) fn install_open_preview(
    on_launch: impl FnOnce(OpenLaunch) + 'static,
) -> Result<(), Box<str>> {
    let document = browser_document().map_err(boxed_error)?;
    let panel = required_element(&document, OPEN_PANEL_ELEMENT_ID).map_err(boxed_error)?;
    let viewer = required_element(&document, VIEWER_ELEMENT_ID).map_err(boxed_error)?;
    let form = required_element(&document, OPEN_FORM_ELEMENT_ID).map_err(boxed_error)?;
    let _alert = required_element(&document, OPEN_ERROR_ELEMENT_ID).map_err(boxed_error)?;
    let _submit = required_element(&document, OPEN_SUBMIT_ELEMENT_ID).map_err(boxed_error)?;
    let primary_input =
        required_input(&document, OPEN_PRIMARY_INPUT_ELEMENT_ID).map_err(boxed_error)?;
    let comparison_input =
        required_input(&document, OPEN_COMPARISON_INPUT_ELEMENT_ID).map_err(boxed_error)?;
    panel
        .remove_attribute("hidden")
        .map_err(|_error| boxed_error(OpenSelectionError::ShellUpdateFailed))?;
    viewer
        .set_attribute("hidden", "")
        .map_err(|_error| boxed_error(OpenSelectionError::ShellUpdateFailed))?;
    primary_input.set_value("");
    comparison_input.set_value("");
    hide_open_error(&document);
    set_shell_status(
        "open",
        "Choose a Primary runtime-export directory and optionally a Comparison directory.",
    );

    let busy = Rc::new(Cell::new(false));
    let complete = Rc::new(Cell::new(false));
    let callback: SharedOpenLaunchCallback = Rc::new(RefCell::new(Some(Box::new(on_launch))));
    let event_document = document.clone();
    let event_primary_input = primary_input.clone();
    let event_comparison_input = comparison_input.clone();
    let event_busy = Rc::clone(&busy);
    let event_complete = Rc::clone(&complete);
    let event_callback = Rc::clone(&callback);
    let handler = Closure::wrap(Box::new(move |event: Event| {
        event.prevent_default();
        if event_busy.get() || event_complete.get() {
            return;
        }
        let selections = (|| {
            let primary = collect_browser_selection(&event_primary_input)
                .map_err(|source| OpenFormError::source(OpenSourceRole::Primary, source))?
                .ok_or_else(|| {
                    OpenFormError::source(
                        OpenSourceRole::Primary,
                        OpenSelectionError::EmptySelection,
                    )
                })?;
            let comparison = collect_browser_selection(&event_comparison_input)
                .map_err(|source| OpenFormError::source(OpenSourceRole::Comparison, source))?;
            validate_combined_selection_footprint(
                primary.footprint,
                comparison.as_ref().map(|selection| selection.footprint),
            )?;
            Ok::<_, OpenFormError>((primary, comparison))
        })();
        let (primary, comparison) = match selections {
            Ok(selections) => selections,
            Err(error) => {
                recover_after_error(
                    &event_document,
                    &event_primary_input,
                    &event_comparison_input,
                    &event_busy,
                    &error,
                );
                return;
            }
        };
        event_busy.set(true);
        set_open_controls_disabled(
            &event_document,
            &event_primary_input,
            &event_comparison_input,
            true,
        );
        hide_open_error(&event_document);
        set_shell_status(
            "loading",
            if comparison.is_some() {
                "Checking selected runtime exports…"
            } else {
                "Checking selected runtime export…"
            },
        );

        let task_document = event_document.clone();
        let task_primary_input = event_primary_input.clone();
        let task_comparison_input = event_comparison_input.clone();
        let task_busy = Rc::clone(&event_busy);
        let task_complete = Rc::clone(&event_complete);
        let task_callback = Rc::clone(&event_callback);
        spawn_local(async move {
            match prepare_open_launch(primary, comparison).await {
                Ok(launch) => {
                    task_primary_input.set_value("");
                    task_comparison_input.set_value("");
                    task_complete.set(true);
                    if let Some(callback) = task_callback.borrow_mut().take() {
                        callback(launch);
                    }
                }
                Err(error) => {
                    recover_after_error(
                        &task_document,
                        &task_primary_input,
                        &task_comparison_input,
                        &task_busy,
                        &error,
                    );
                }
            }
        });
    }) as Box<dyn FnMut(Event)>);
    form.add_event_listener_with_callback("submit", handler.as_ref().unchecked_ref())
        .map_err(|_error| boxed_error(OpenSelectionError::ShellUpdateFailed))?;
    handler.forget();
    set_open_controls_disabled(&document, &primary_input, &comparison_input, false);
    Ok(())
}

/// Moves focus from the successful local Open form into the revealed viewer.
#[cfg(target_arch = "wasm32")]
pub(super) fn focus_viewer_canvas() -> Result<(), Box<str>> {
    let document = browser_document().map_err(boxed_error)?;
    let canvas = required_element(&document, VIEWER_CANVAS_ELEMENT_ID).map_err(boxed_error)?;
    focus_element(&canvas).map_err(boxed_error)
}

/// Hides the local picker and makes the ordinary viewer subtree available.
#[cfg(target_arch = "wasm32")]
pub(super) fn show_viewer_shell() -> Result<(), Box<str>> {
    let document = browser_document().map_err(boxed_error)?;
    let panel = required_element(&document, OPEN_PANEL_ELEMENT_ID).map_err(boxed_error)?;
    let viewer = required_element(&document, VIEWER_ELEMENT_ID).map_err(boxed_error)?;
    let primary_input =
        required_input(&document, OPEN_PRIMARY_INPUT_ELEMENT_ID).map_err(boxed_error)?;
    let comparison_input =
        required_input(&document, OPEN_COMPARISON_INPUT_ELEMENT_ID).map_err(boxed_error)?;
    panel
        .set_attribute("hidden", "")
        .map_err(|_error| boxed_error(OpenSelectionError::ShellUpdateFailed))?;
    viewer
        .remove_attribute("hidden")
        .map_err(|_error| boxed_error(OpenSelectionError::ShellUpdateFailed))?;
    set_open_controls_disabled(&document, &primary_input, &comparison_input, true);
    hide_open_error(&document);
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn collect_browser_selection(
    input: &HtmlInputElement,
) -> Result<Option<BrowserSelection>, OpenSelectionError> {
    let list = input.files().ok_or(OpenSelectionError::EmptySelection)?;
    let length = usize::try_from(list.length())
        .map_err(|_error| OpenSelectionError::InvalidBrowserMetadata)?;
    if length == 0 {
        return Ok(None);
    }
    if length > MAX_SELECTED_FILE_COUNT {
        return Err(OpenSelectionError::TooManySelectedFiles { actual: length });
    }
    let mut files = Vec::with_capacity(length);
    let mut raw = Vec::with_capacity(length);
    for browser_index in 0..length {
        let file = list
            .item(
                u32::try_from(browser_index)
                    .map_err(|_error| OpenSelectionError::InvalidBrowserMetadata)?,
            )
            .ok_or(OpenSelectionError::InvalidBrowserMetadata)?;
        let name = bounded_file_string(&file, "name", MAX_SELECTED_COMPONENT_BYTES, false)?;
        let relative_path =
            bounded_file_string(&file, "webkitRelativePath", MAX_SELECTED_PATH_BYTES, true)?;
        let byte_length = browser_file_size(&file)?;
        raw.push(RawFileMetadata {
            browser_index,
            name: name.into_boxed_str(),
            relative_path: relative_path.into_boxed_str(),
            byte_length,
        });
        files.push(file);
    }
    let preflight = preflight_selection_metadata(&raw)?;
    Ok(Some(BrowserSelection {
        files,
        metadata: preflight.normalized.files,
        footprint: preflight.normalized.footprint,
        candidates: preflight.candidates,
    }))
}

#[cfg(target_arch = "wasm32")]
fn bounded_file_string(
    file: &File,
    property: &str,
    maximum_code_units: usize,
    absent_is_empty: bool,
) -> Result<String, OpenSelectionError> {
    let value = Reflect::get(file.as_ref(), &JsValue::from_str(property))
        .map_err(|_error| OpenSelectionError::InvalidBrowserMetadata)?;
    if value.is_null() || value.is_undefined() {
        return absent_is_empty
            .then(String::new)
            .ok_or(OpenSelectionError::InvalidBrowserMetadata);
    }
    let value = value
        .dyn_ref::<JsString>()
        .ok_or(OpenSelectionError::InvalidBrowserMetadata)?;
    let length = usize::try_from(value.length())
        .map_err(|_error| OpenSelectionError::InvalidBrowserMetadata)?;
    if length > maximum_code_units || !value.is_valid_utf16() {
        return Err(OpenSelectionError::UnsafeSelectedPath);
    }
    Ok(String::from(value))
}

#[cfg(target_arch = "wasm32")]
fn browser_file_size(file: &File) -> Result<usize, OpenSelectionError> {
    let size = file.size();
    if !size.is_finite() || size < 0.0 || size.fract() != 0.0 || size > usize::MAX as f64 {
        return Err(OpenSelectionError::InvalidBrowserMetadata);
    }
    Ok(size as usize)
}

#[cfg(target_arch = "wasm32")]
async fn prepare_open_launch(
    primary: BrowserSelection,
    comparison: Option<BrowserSelection>,
) -> Result<OpenLaunch, OpenFormError> {
    let primary = prepare_browser_source_header(primary, OpenSourceRole::Primary)
        .await
        .map_err(|source| OpenFormError::source(OpenSourceRole::Primary, source))?;
    let comparison = match comparison {
        Some(selection) => Some(
            prepare_browser_source_header(selection, OpenSourceRole::Comparison)
                .await
                .map_err(|source| OpenFormError::source(OpenSourceRole::Comparison, source))?,
        ),
        None => None,
    };
    validate_combined_required_plans(
        &primary.plan,
        comparison.as_ref().map(|source| &source.plan),
    )?;
    let primary = materialize_browser_source(primary)
        .await
        .map_err(|source| OpenFormError::source(OpenSourceRole::Primary, source))?;
    let comparison = match comparison {
        Some(source) => Some(
            materialize_browser_source(source)
                .await
                .map_err(|source| OpenFormError::source(OpenSourceRole::Comparison, source))?,
        ),
        None => None,
    };
    let bundles = BrowserLaunchBundles::validate(primary, comparison)
        .map_err(|_error| OpenFormError::CombinedRuntimeBudgetExceeded)?;
    let (primary, comparison) = bundles.into_parts();
    let label: Box<str> = if comparison.is_some() {
        "Primary and Comparison".into()
    } else {
        "Primary".into()
    };
    Ok(OpenLaunch {
        label,
        config: LaunchConfig::from_bundles(primary, comparison, PreviewRate::default()),
    })
}

#[cfg(target_arch = "wasm32")]
async fn prepare_browser_source_header(
    selection: BrowserSelection,
    source_role: OpenSourceRole,
) -> Result<PreparedBrowserSource, OpenSelectionError> {
    let candidates = selection.candidates;
    let json_metadata = &selection.metadata[candidates.json_position];
    let atlas_metadata = &selection.metadata[candidates.atlas_position];
    let json_path = json_metadata.path.clone();
    let atlas_path = atlas_metadata.path.clone();
    let json_file = selection
        .files
        .get(json_metadata.browser_index)
        .ok_or(OpenSelectionError::InvalidBrowserMetadata)?;
    let atlas_file = selection
        .files
        .get(atlas_metadata.browser_index)
        .ok_or(OpenSelectionError::InvalidBrowserMetadata)?;
    let json = read_browser_file(json_file, json_metadata.byte_length, RequiredRole::Json).await?;
    let atlas =
        read_browser_file(atlas_file, atlas_metadata.byte_length, RequiredRole::Atlas).await?;
    let page_paths =
        RuntimeBundleManifest::required_page_paths(&json_path, &atlas_path, &json, &atlas)
            .map_err(OpenSelectionError::from_runtime)?;
    let plan = plan_required_files(&selection.metadata, candidates, &page_paths)?;
    Ok(PreparedBrowserSource {
        selection,
        source_role,
        json_path,
        atlas_path,
        json,
        atlas,
        plan,
    })
}

#[cfg(target_arch = "wasm32")]
async fn materialize_browser_source(
    prepared: PreparedBrowserSource,
) -> Result<SourceBundle, OpenSelectionError> {
    let PreparedBrowserSource {
        selection,
        source_role,
        json_path,
        atlas_path,
        json,
        atlas,
        plan,
    } = prepared;
    let mut bundle_files = BTreeMap::from([(json_path.clone(), json), (atlas_path.clone(), atlas)]);
    for position in plan.positions.into_iter().skip(2) {
        let metadata = selection
            .metadata
            .get(position)
            .ok_or(OpenSelectionError::InvalidBrowserMetadata)?;
        let file = selection
            .files
            .get(metadata.browser_index)
            .ok_or(OpenSelectionError::InvalidBrowserMetadata)?;
        let bytes = read_browser_file(file, metadata.byte_length, RequiredRole::Page).await?;
        if bundle_files.insert(metadata.path.clone(), bytes).is_some() {
            return Err(OpenSelectionError::InvalidRequiredPage);
        }
    }
    let validated =
        RuntimeBundleManifest::build(source_role.label(), &json_path, &atlas_path, bundle_files)
            .map_err(OpenSelectionError::from_runtime)?
            .1;
    Ok(SourceBundle::from_validated(validated))
}

#[cfg(target_arch = "wasm32")]
async fn read_browser_file(
    file: &File,
    expected_bytes: usize,
    role: RequiredRole,
) -> Result<Vec<u8>, OpenSelectionError> {
    let buffer = JsFuture::from(file.array_buffer())
        .await
        .map_err(|_error| OpenSelectionError::FileReadFailed { role })?;
    let view = Uint8Array::new(&buffer);
    let actual = usize::try_from(view.length())
        .map_err(|_error| OpenSelectionError::FileLengthChanged { role })?;
    if actual != expected_bytes {
        return Err(OpenSelectionError::FileLengthChanged { role });
    }
    let mut bytes = vec![0_u8; actual];
    view.copy_to(&mut bytes);
    Ok(bytes)
}

#[cfg(target_arch = "wasm32")]
fn browser_document() -> Result<Document, OpenSelectionError> {
    web_sys::window()
        .and_then(|window| window.document())
        .ok_or(OpenSelectionError::MissingShellElement("document"))
}

#[cfg(target_arch = "wasm32")]
fn required_element(document: &Document, id: &'static str) -> Result<Element, OpenSelectionError> {
    document
        .get_element_by_id(id)
        .ok_or(OpenSelectionError::MissingShellElement(id))
}

#[cfg(target_arch = "wasm32")]
fn required_input(
    document: &Document,
    id: &'static str,
) -> Result<HtmlInputElement, OpenSelectionError> {
    required_element(document, id).and_then(|element| {
        element
            .dyn_into::<HtmlInputElement>()
            .map_err(|_value| OpenSelectionError::InvalidShellElement(id))
    })
}

#[cfg(target_arch = "wasm32")]
fn recover_after_error(
    document: &Document,
    primary_input: &HtmlInputElement,
    comparison_input: &HtmlInputElement,
    busy: &Cell<bool>,
    error: &OpenFormError,
) {
    busy.set(false);
    primary_input.set_value("");
    comparison_input.set_value("");
    set_open_controls_disabled(document, primary_input, comparison_input, false);
    set_shell_status(
        "open",
        "Choose the Primary directory again and optionally a Comparison directory.",
    );
    let Some(alert) = document.get_element_by_id(OPEN_ERROR_ELEMENT_ID) else {
        return;
    };
    alert.set_text_content(Some(&error.to_string()));
    let _ignored = alert.remove_attribute("hidden");
    let _ignored = focus_element(&alert);
}

#[cfg(target_arch = "wasm32")]
fn set_open_controls_disabled(
    document: &Document,
    primary_input: &HtmlInputElement,
    comparison_input: &HtmlInputElement,
    disabled: bool,
) {
    primary_input.set_disabled(disabled);
    comparison_input.set_disabled(disabled);
    let Some(submit) = document.get_element_by_id(OPEN_SUBMIT_ELEMENT_ID) else {
        return;
    };
    if disabled {
        let _ignored = submit.set_attribute("disabled", "");
    } else {
        let _ignored = submit.remove_attribute("disabled");
    }
}

#[cfg(target_arch = "wasm32")]
fn hide_open_error(document: &Document) {
    let Some(alert) = document.get_element_by_id(OPEN_ERROR_ELEMENT_ID) else {
        return;
    };
    alert.set_text_content(None);
    let _ignored = alert.set_attribute("hidden", "");
}

#[cfg(target_arch = "wasm32")]
fn focus_element(element: &Element) -> Result<(), OpenSelectionError> {
    let focus = Reflect::get(element.as_ref(), &JsValue::from_str("focus"))
        .map_err(|_error| OpenSelectionError::ShellUpdateFailed)?;
    let focus = focus
        .dyn_ref::<Function>()
        .ok_or(OpenSelectionError::ShellUpdateFailed)?;
    focus
        .call0(element.as_ref())
        .map_err(|_error| OpenSelectionError::ShellUpdateFailed)?;
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn set_shell_status(kind: &str, message: &str) {
    let Ok(callback) = Reflect::get(
        &js_sys::global(),
        &JsValue::from_str("spinalSetShellStatus"),
    ) else {
        return;
    };
    let Some(callback) = callback.dyn_ref::<Function>() else {
        return;
    };
    let _ignored = callback.call2(
        &JsValue::NULL,
        &JsValue::from_str(kind),
        &JsValue::from_str(message),
    );
}

#[cfg(target_arch = "wasm32")]
fn boxed_error(error: OpenSelectionError) -> Box<str> {
    error.to_string().into_boxed_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(index: usize, name: &str, relative_path: &str, byte_length: usize) -> RawFileMetadata {
        RawFileMetadata {
            browser_index: index,
            name: name.into(),
            relative_path: relative_path.into(),
            byte_length,
        }
    }

    fn normalized(raw: &[RawFileMetadata]) -> Vec<NormalizedFileMetadata> {
        normalize_selection(raw).expect("valid selection metadata")
    }

    #[test]
    fn rooted_selection_strips_exactly_one_common_root_and_keeps_nested_paths() {
        let files = normalized(&[
            raw(0, "rig.json", "export/rig.json", 10),
            raw(1, "rig.atlas", "export/rig.atlas", 20),
            raw(2, "page.png", "export/textures/page.png", 30),
            raw(3, ".DS_Store", "export/.DS_Store", 0),
        ]);
        assert_eq!(
            files
                .iter()
                .map(|file| file.path.as_path())
                .collect::<Vec<_>>(),
            [
                Path::new("rig.json"),
                Path::new("rig.atlas"),
                Path::new("textures/page.png"),
                Path::new(".DS_Store"),
            ]
        );
    }

    #[test]
    fn flat_selection_uses_safe_file_names_without_inventing_a_root() {
        let files = normalized(&[
            raw(0, "rig.json", "", 10),
            raw(1, "rig.atlas", "", 20),
            raw(2, "page.png", "", 30),
        ]);
        assert_eq!(files[0].path, Path::new("rig.json"));
        assert_eq!(files[2].path, Path::new("page.png"));
    }

    #[test]
    fn unsafe_mixed_and_multi_root_metadata_is_rejected_before_reads() {
        assert_eq!(
            normalize_selection(&[
                raw(0, "rig.json", "export/rig.json", 10),
                raw(1, "rig.atlas", "", 20),
            ]),
            Err(OpenSelectionError::MixedSelectionLayouts)
        );
        assert_eq!(
            normalize_selection(&[
                raw(0, "rig.json", "first/rig.json", 10),
                raw(1, "rig.atlas", "second/rig.atlas", 20),
            ]),
            Err(OpenSelectionError::MixedDirectoryRoots)
        );
        for unsafe_path in [
            "export/../rig.json",
            "export\\rig.json",
            "export/./rig.json",
            "export/rig\0.json",
        ] {
            assert_eq!(
                normalize_selection(&[raw(0, "rig.json", unsafe_path, 10)]),
                Err(OpenSelectionError::UnsafeSelectedPath),
                "accepted {unsafe_path:?}"
            );
        }
    }

    #[test]
    fn exact_duplicates_and_portable_case_collisions_are_distinct_failures() {
        assert_eq!(
            normalize_selection(&[
                raw(0, "page.png", "export/page.png", 10),
                raw(1, "page.png", "export/page.png", 10),
            ]),
            Err(OpenSelectionError::DuplicatePath)
        );
        assert_eq!(
            normalize_selection(&[
                raw(0, "Page.png", "export/Page.png", 10),
                raw(1, "page.PNG", "export/page.PNG", 10),
            ]),
            Err(OpenSelectionError::PortablePathCollision)
        );
    }

    #[test]
    fn candidate_derivation_requires_exactly_one_json_and_one_atlas() {
        let files = normalized(&[
            raw(0, "rig.JSON", "export/rig.JSON", 10),
            raw(1, "rig.atlas", "export/rig.atlas", 20),
            raw(2, "notes.txt", "export/notes.txt", 30),
        ]);
        assert_eq!(
            derive_candidates(&files),
            Ok(CandidatePair {
                json_position: 0,
                atlas_position: 1,
            })
        );

        let ambiguous = normalized(&[
            raw(0, "rig.json", "export/rig.json", 10),
            raw(1, "metadata.json", "export/metadata.json", 10),
            raw(2, "rig.atlas", "export/rig.atlas", 20),
        ]);
        assert_eq!(
            derive_candidates(&ambiguous),
            Err(OpenSelectionError::AmbiguousJson { actual: 2 })
        );
    }

    #[test]
    fn required_plan_selects_exact_pages_and_ignores_safe_unrelated_metadata() {
        let files = normalized(&[
            raw(0, "rig.json", "export/rig.json", 10),
            raw(1, "rig.atlas", "export/rig.atlas", 20),
            raw(2, "page.png", "export/textures/page.png", 30),
            raw(3, "old.png", "export/textures/old.png", 40),
            raw(4, "notes.txt", "export/notes.txt", 50),
        ]);
        let candidates = derive_candidates(&files).expect("unambiguous candidates");
        let plan = plan_required_files(&files, candidates, &[PathBuf::from("textures/page.png")])
            .expect("complete required file plan");
        assert_eq!(plan.positions, vec![0, 1, 2]);
        assert_eq!(plan.encoded_bytes, 60);
    }

    #[test]
    fn required_pages_use_exact_paths_and_all_limits_are_prechecked() {
        let files = normalized(&[
            raw(0, "rig.json", "export/rig.json", 10),
            raw(1, "rig.atlas", "export/rig.atlas", 20),
            raw(2, "Page.png", "export/textures/Page.png", 30),
        ]);
        let candidates = derive_candidates(&files).expect("unambiguous candidates");
        assert_eq!(
            plan_required_files(&files, candidates, &[PathBuf::from("textures/page.png")],),
            Err(OpenSelectionError::MissingRequiredPage {
                path: PathBuf::from("textures/page.png"),
            })
        );

        let oversized = normalized(&[
            raw(0, "rig.json", "export/rig.json", MAX_RUNTIME_JSON_BYTES + 1),
            raw(1, "rig.atlas", "export/rig.atlas", 20),
        ]);
        let candidates = derive_candidates(&oversized).expect("unambiguous candidates");
        assert_eq!(
            precheck_candidates(&oversized, candidates),
            Err(OpenSelectionError::RequiredFileTooLarge {
                role: RequiredRole::Json,
                limit: MAX_RUNTIME_JSON_BYTES,
            })
        );
    }

    #[test]
    fn both_sources_finish_candidate_and_size_preflight_before_reads() {
        let primary = preflight_selection_metadata(&[
            raw(0, "primary.json", "primary/primary.json", 10),
            raw(1, "primary.atlas", "primary/primary.atlas", 20),
        ])
        .expect("Primary metadata preflights");
        assert_eq!(
            primary.candidates,
            CandidatePair {
                json_position: 0,
                atlas_position: 1,
            }
        );

        assert_eq!(
            preflight_selection_metadata(&[
                raw(
                    0,
                    "comparison.json",
                    "comparison/comparison.json",
                    MAX_RUNTIME_JSON_BYTES + 1,
                ),
                raw(1, "comparison.atlas", "comparison/comparison.atlas", 20,),
            ]),
            Err(OpenSelectionError::RequiredFileTooLarge {
                role: RequiredRole::Json,
                limit: MAX_RUNTIME_JSON_BYTES,
            })
        );
    }

    #[test]
    fn combined_metadata_budget_is_global_across_both_directories() {
        let primary = SelectionFootprint {
            file_count: MAX_SELECTED_FILE_COUNT - 1,
            metadata_bytes: MAX_SELECTED_METADATA_BYTES - 1,
        };
        assert_eq!(
            validate_combined_selection_footprint(
                primary,
                Some(SelectionFootprint {
                    file_count: 2,
                    metadata_bytes: 1,
                }),
            ),
            Err(OpenFormError::CombinedSelectedFileBudgetExceeded {
                actual: MAX_SELECTED_FILE_COUNT + 1,
            })
        );
        assert_eq!(
            validate_combined_selection_footprint(
                SelectionFootprint {
                    file_count: 1,
                    metadata_bytes: MAX_SELECTED_METADATA_BYTES,
                },
                Some(SelectionFootprint {
                    file_count: 1,
                    metadata_bytes: 1,
                }),
            ),
            Err(OpenFormError::CombinedMetadataBudgetExceeded)
        );
        assert_eq!(
            validate_combined_selection_footprint(
                SelectionFootprint {
                    file_count: 2,
                    metadata_bytes: 20,
                },
                None,
            ),
            Ok(())
        );
    }

    #[test]
    fn combined_required_plans_are_checked_before_page_materialization() {
        let primary = RequiredFilePlan {
            positions: vec![0, 1],
            encoded_bytes: MAX_BROWSER_BUNDLE_BYTES / 2,
        };
        let comparison = RequiredFilePlan {
            positions: vec![0, 1],
            encoded_bytes: MAX_BROWSER_BUNDLE_BYTES - primary.encoded_bytes,
        };
        assert_eq!(
            validate_combined_required_plans(&primary, Some(&comparison)),
            Ok(())
        );

        let too_many_primary = RequiredFilePlan {
            positions: vec![0; MAX_RUNTIME_FILE_COUNT],
            encoded_bytes: 1,
        };
        let one_comparison = RequiredFilePlan {
            positions: vec![0],
            encoded_bytes: 1,
        };
        assert_eq!(
            validate_combined_required_plans(&too_many_primary, Some(&one_comparison)),
            Err(OpenFormError::CombinedRequiredFileBudgetExceeded {
                actual: MAX_RUNTIME_FILE_COUNT + 1,
            })
        );

        let byte_limit = RequiredFilePlan {
            positions: vec![0, 1],
            encoded_bytes: MAX_BROWSER_BUNDLE_BYTES,
        };
        assert_eq!(
            validate_combined_required_plans(&byte_limit, Some(&one_comparison)),
            Err(OpenFormError::CombinedRequiredByteBudgetExceeded {
                actual: MAX_BROWSER_BUNDLE_BYTES + 1,
            })
        );
    }

    #[test]
    fn directory_errors_are_attributed_to_primary_or_comparison() {
        assert_eq!(
            OpenFormError::source(OpenSourceRole::Primary, OpenSelectionError::MissingJson)
                .to_string(),
            "Primary directory: The selected directory has no Spine JSON file. Choose a runtime export containing exactly one `.json` file."
        );
        assert!(
            OpenFormError::source(OpenSourceRole::Comparison, OpenSelectionError::MissingAtlas)
                .to_string()
                .starts_with("Comparison directory: ")
        );
    }
}

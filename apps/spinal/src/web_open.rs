//! Read-only browser acquisition for one local Spine runtime export.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::{Path, PathBuf},
};

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
use crate::{bundle::SourceBundle, preview::PreviewRate, runtime::LaunchConfig};

#[cfg(target_arch = "wasm32")]
const OPEN_PANEL_ELEMENT_ID: &str = "spinal-open-panel";
#[cfg(target_arch = "wasm32")]
const OPEN_FORM_ELEMENT_ID: &str = "spinal-open-form";
#[cfg(target_arch = "wasm32")]
const OPEN_INPUT_ELEMENT_ID: &str = "spinal-open-files";
#[cfg(target_arch = "wasm32")]
const OPEN_ERROR_ELEMENT_ID: &str = "spinal-open-error";
#[cfg(target_arch = "wasm32")]
const OPEN_SUBMIT_ELEMENT_ID: &str = "spinal-open-submit";
#[cfg(target_arch = "wasm32")]
const VIEWER_ELEMENT_ID: &str = "spinal-viewer";
#[cfg(target_arch = "wasm32")]
const SOURCE_LABEL: &str = "Browser directory export";

/// The browser must enumerate every selected entry before it reads any bytes.
/// This limit leaves room for safe unrelated metadata while keeping that
/// enumeration independent of the smaller runtime-bundle file limit.
const MAX_SELECTED_FILE_COUNT: usize = MAX_RUNTIME_FILE_COUNT * 2;
const MAX_SELECTED_PATH_BYTES: usize = 2_048;
const MAX_SELECTED_COMPONENT_BYTES: usize = 255;
const MAX_SELECTED_METADATA_BYTES: usize = MAX_SELECTED_FILE_COUNT * MAX_SELECTED_PATH_BYTES * 2;

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
                "No files were selected. Choose one runtime-export directory to open a preview.",
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
                formatter.write_str("the page could not update the Open Preview surface")
            }
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
struct CandidatePair {
    json_position: usize,
    atlas_position: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RequiredFilePlan {
    positions: Vec<usize>,
}

fn normalize_selection(
    raw_files: &[RawFileMetadata],
) -> Result<Vec<NormalizedFileMetadata>, OpenSelectionError> {
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
    Ok(normalized)
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
    Ok(RequiredFilePlan { positions })
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

/// One fully validated, single-source browser launch.
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
}

/// Installs one persistent, retryable Open Preview form listener.
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
    let input = required_element(&document, OPEN_INPUT_ELEMENT_ID)
        .and_then(|element| {
            element
                .dyn_into::<HtmlInputElement>()
                .map_err(|_value| OpenSelectionError::InvalidShellElement(OPEN_INPUT_ELEMENT_ID))
        })
        .map_err(boxed_error)?;
    panel
        .remove_attribute("hidden")
        .map_err(|_error| boxed_error(OpenSelectionError::ShellUpdateFailed))?;
    viewer
        .set_attribute("hidden", "")
        .map_err(|_error| boxed_error(OpenSelectionError::ShellUpdateFailed))?;
    input.set_value("");
    hide_open_error(&document);
    set_shell_status("open", "Choose a runtime-export directory to preview.");

    let busy = Rc::new(Cell::new(false));
    let complete = Rc::new(Cell::new(false));
    let callback: SharedOpenLaunchCallback = Rc::new(RefCell::new(Some(Box::new(on_launch))));
    let event_document = document.clone();
    let event_input = input.clone();
    let event_busy = Rc::clone(&busy);
    let event_complete = Rc::clone(&complete);
    let event_callback = Rc::clone(&callback);
    let handler = Closure::wrap(Box::new(move |event: Event| {
        event.prevent_default();
        if event_busy.get() || event_complete.get() {
            return;
        }
        let selection = match collect_browser_selection(&event_input) {
            Ok(selection) => selection,
            Err(error) => {
                recover_after_error(&event_document, &event_input, &event_busy, &error);
                return;
            }
        };
        event_busy.set(true);
        set_open_controls_disabled(&event_document, &event_input, true);
        hide_open_error(&event_document);
        set_shell_status("loading", "Checking selected export…");

        let task_document = event_document.clone();
        let task_input = event_input.clone();
        let task_busy = Rc::clone(&event_busy);
        let task_complete = Rc::clone(&event_complete);
        let task_callback = Rc::clone(&event_callback);
        spawn_local(async move {
            match prepare_browser_selection(selection).await {
                Ok(launch) => {
                    task_complete.set(true);
                    if let Some(callback) = task_callback.borrow_mut().take() {
                        callback(launch);
                    }
                }
                Err(error) => {
                    recover_after_error(&task_document, &task_input, &task_busy, &error);
                }
            }
        });
    }) as Box<dyn FnMut(Event)>);
    form.add_event_listener_with_callback("submit", handler.as_ref().unchecked_ref())
        .map_err(|_error| boxed_error(OpenSelectionError::ShellUpdateFailed))?;
    handler.forget();
    set_open_controls_disabled(&document, &input, false);
    Ok(())
}

/// Hides the local picker and makes the ordinary viewer subtree available.
#[cfg(target_arch = "wasm32")]
pub(super) fn show_viewer_shell() -> Result<(), Box<str>> {
    let document = browser_document().map_err(boxed_error)?;
    let panel = required_element(&document, OPEN_PANEL_ELEMENT_ID).map_err(boxed_error)?;
    let viewer = required_element(&document, VIEWER_ELEMENT_ID).map_err(boxed_error)?;
    panel
        .set_attribute("hidden", "")
        .map_err(|_error| boxed_error(OpenSelectionError::ShellUpdateFailed))?;
    viewer
        .remove_attribute("hidden")
        .map_err(|_error| boxed_error(OpenSelectionError::ShellUpdateFailed))?;
    if let Some(input) = document
        .get_element_by_id(OPEN_INPUT_ELEMENT_ID)
        .and_then(|element| element.dyn_into::<HtmlInputElement>().ok())
    {
        set_open_controls_disabled(&document, &input, true);
    }
    hide_open_error(&document);
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn collect_browser_selection(
    input: &HtmlInputElement,
) -> Result<BrowserSelection, OpenSelectionError> {
    let list = input.files().ok_or(OpenSelectionError::EmptySelection)?;
    let length = usize::try_from(list.length())
        .map_err(|_error| OpenSelectionError::InvalidBrowserMetadata)?;
    if length == 0 {
        return Err(OpenSelectionError::EmptySelection);
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
    let metadata = normalize_selection(&raw)?;
    Ok(BrowserSelection { files, metadata })
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
async fn prepare_browser_selection(
    selection: BrowserSelection,
) -> Result<OpenLaunch, OpenSelectionError> {
    let candidates = derive_candidates(&selection.metadata)?;
    precheck_candidates(&selection.metadata, candidates)?;
    let json_metadata = &selection.metadata[candidates.json_position];
    let atlas_metadata = &selection.metadata[candidates.atlas_position];
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
    let page_paths = RuntimeBundleManifest::required_page_paths(
        &json_metadata.path,
        &atlas_metadata.path,
        &json,
        &atlas,
    )
    .map_err(OpenSelectionError::from_runtime)?;
    let plan = plan_required_files(&selection.metadata, candidates, &page_paths)?;

    let json_path = json_metadata.path.clone();
    let atlas_path = atlas_metadata.path.clone();
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
        RuntimeBundleManifest::build(SOURCE_LABEL, &json_path, &atlas_path, bundle_files)
            .map_err(OpenSelectionError::from_runtime)?
            .1;
    let bundle = SourceBundle::from_validated(validated);
    Ok(OpenLaunch {
        label: SOURCE_LABEL.into(),
        config: LaunchConfig::from_bundles(bundle, None, PreviewRate::default()),
    })
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
fn recover_after_error(
    document: &Document,
    input: &HtmlInputElement,
    busy: &Cell<bool>,
    error: &OpenSelectionError,
) {
    busy.set(false);
    input.set_value("");
    set_open_controls_disabled(document, input, false);
    set_shell_status("open", "Choose another runtime-export directory.");
    let Some(alert) = document.get_element_by_id(OPEN_ERROR_ELEMENT_ID) else {
        return;
    };
    alert.set_text_content(Some(&error.to_string()));
    let _ignored = alert.remove_attribute("hidden");
    focus_element(&alert);
}

#[cfg(target_arch = "wasm32")]
fn set_open_controls_disabled(document: &Document, input: &HtmlInputElement, disabled: bool) {
    input.set_disabled(disabled);
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
fn focus_element(element: &Element) {
    let Ok(focus) = Reflect::get(element.as_ref(), &JsValue::from_str("focus")) else {
        return;
    };
    let Some(focus) = focus.dyn_ref::<Function>() else {
        return;
    };
    let _ignored = focus.call0(element.as_ref());
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
}

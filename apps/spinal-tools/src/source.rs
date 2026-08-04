use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs,
    io::{self, BufReader, Read},
    path::{Component, Path, PathBuf},
};

use spinal::{PixelSize, SkeletonAsset};
use thiserror::Error;

use crate::cli::CheckOptions;

const MAX_JSON_BYTES: u64 = 8 * 1024 * 1024;
const MAX_ATLAS_BYTES: u64 = 1024 * 1024;
const MAX_ATLAS_LINES: usize = 65_536;
const MAX_ATLAS_PAGES: usize = 64;
const MAX_AMBIGUOUS_CANDIDATES: usize = 16;
const MAX_PAGE_FILE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_PAGE_DIMENSION: u32 = 8192;
const MAX_PAGE_DECODED_BYTES: usize = 256 * 1024 * 1024;
const MAX_PNG_DECODER_BYTES: usize = 64 * 1024 * 1024;
const MAX_TOTAL_PAGE_PIXELS: u64 = 128 * 1024 * 1024;

#[derive(Debug)]
pub(crate) struct SourceFiles {
    json_path: PathBuf,
    atlas_path: PathBuf,
    atlas_root: PathBuf,
    json: Vec<u8>,
    atlas: Vec<u8>,
}

impl SourceFiles {
    pub(crate) fn open(options: &CheckOptions) -> Result<Self, SourceError> {
        let input = fs::canonicalize(&options.input).map_err(|source| SourceError::Io {
            action: "open input",
            path: options.input.clone(),
            source,
        })?;
        let json_path = if input.is_dir() {
            discover_json(&input)?
        } else {
            ensure_json_file(&input)?;
            input
        };
        let atlas_path = match options.atlas.as_deref() {
            Some(path) => canonical_file(path, "text atlas")?,
            None => discover_atlas(&json_path)?,
        };
        let atlas_root = atlas_path
            .parent()
            .filter(|root| root.parent().is_some())
            .ok_or_else(|| SourceError::UnsafeAtlasRoot {
                path: atlas_path.clone(),
            })?
            .to_owned();
        let json = read_bounded(&json_path, "skeleton JSON", MAX_JSON_BYTES)?;
        let atlas = read_bounded(&atlas_path, "text atlas", MAX_ATLAS_BYTES)?;
        validate_atlas_line_count(&atlas, &atlas_path, MAX_ATLAS_LINES)?;
        Ok(Self {
            json_path,
            atlas_path,
            atlas_root,
            json,
            atlas,
        })
    }

    pub(crate) fn json_path(&self) -> &Path {
        &self.json_path
    }

    pub(crate) fn atlas_path(&self) -> &Path {
        &self.atlas_path
    }

    pub(crate) fn json(&self) -> &[u8] {
        &self.json
    }

    pub(crate) fn atlas(&self) -> &[u8] {
        &self.atlas
    }

    pub(crate) fn inspect_pages(
        &self,
        asset: &SkeletonAsset,
    ) -> Result<Vec<PageInspection>, SourceError> {
        let atlas_directory =
            self.atlas_path
                .parent()
                .ok_or_else(|| SourceError::UnsafeAtlasRoot {
                    path: self.atlas_path.clone(),
                })?;
        if asset.atlas_pages().len() > MAX_ATLAS_PAGES {
            return Err(SourceError::TooManyPages {
                actual: asset.atlas_pages().len(),
                maximum: MAX_ATLAS_PAGES,
            });
        }
        let mut inspections = Vec::with_capacity(asset.atlas_pages().len());
        let mut portable_names = BTreeMap::<String, String>::new();
        let mut total_pixels = 0_u64;
        for page in asset.atlas_pages() {
            validate_embedded_reference(page.name()).map_err(|reason| {
                SourceError::DisallowedPageReference {
                    atlas_path: self.atlas_path.clone(),
                    page: page.name().to_owned(),
                    reason,
                }
            })?;
            let portable_key = page.name().to_lowercase();
            if let Some(first) = portable_names.insert(portable_key, page.name().to_owned()) {
                return Err(SourceError::PortablePageCollision {
                    first,
                    second: page.name().to_owned(),
                });
            }
            let declared = page.size();
            let pixels = u64::from(declared.width())
                .checked_mul(u64::from(declared.height()))
                .ok_or_else(|| SourceError::PageLimit {
                    page: page.name().to_owned(),
                    reason: "declared dimensions overflow the pixel budget".to_owned(),
                })?;
            total_pixels =
                total_pixels
                    .checked_add(pixels)
                    .ok_or_else(|| SourceError::PageLimit {
                        page: page.name().to_owned(),
                        reason: "total declared pixels overflow the bundle budget".to_owned(),
                    })?;
            if total_pixels > MAX_TOTAL_PAGE_PIXELS {
                return Err(SourceError::PageLimit {
                    page: page.name().to_owned(),
                    reason: format!(
                        "bundle pages exceed the {} pixel CI safety ceiling",
                        MAX_TOTAL_PAGE_PIXELS
                    ),
                });
            }
            let unresolved_path = atlas_directory.join(page.name());
            let path = match fs::canonicalize(&unresolved_path) {
                Ok(path) => path,
                Err(error) => {
                    inspections.push(PageInspection::failed(
                        page.name(),
                        unresolved_path,
                        page.size(),
                        format!("atlas page is unavailable: {error}"),
                    ));
                    continue;
                }
            };
            if path.strip_prefix(&self.atlas_root).is_err() {
                return Err(SourceError::DisallowedPageReference {
                    atlas_path: self.atlas_path.clone(),
                    page: page.name().to_owned(),
                    reason: "the page path escapes the text-atlas directory",
                });
            }
            if !path.is_file() {
                inspections.push(PageInspection::failed(
                    page.name(),
                    path,
                    page.size(),
                    "atlas page is not a regular file".to_owned(),
                ));
                continue;
            }
            inspections.push(inspect_png(page.name(), path, page.size()));
        }
        Ok(inspections)
    }
}

#[derive(Debug)]
pub(crate) struct PageInspection {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    pub(crate) declared_size: PixelSize,
    pub(crate) actual_size: Option<(u32, u32)>,
    pub(crate) color_type: Option<png::ColorType>,
    pub(crate) bit_depth: Option<png::BitDepth>,
    pub(crate) problem: Option<String>,
}

impl PageInspection {
    fn failed(name: &str, path: PathBuf, declared_size: PixelSize, problem: String) -> Self {
        Self {
            name: name.to_owned(),
            path,
            declared_size,
            actual_size: None,
            color_type: None,
            bit_depth: None,
            problem: Some(problem),
        }
    }
}

fn inspect_png(name: &str, path: PathBuf, declared_size: PixelSize) -> PageInspection {
    let result = (|| -> Result<((u32, u32), png::ColorType, png::BitDepth), String> {
        let file = fs::File::open(&path).map_err(|error| format!("could not open PNG: {error}"))?;
        let file_bytes = file
            .metadata()
            .map_err(|error| format!("could not inspect PNG size: {error}"))?
            .len();
        if file_bytes > MAX_PAGE_FILE_BYTES {
            return Err(format!(
                "PNG is {file_bytes} bytes, above the {MAX_PAGE_FILE_BYTES} byte file-size CI safety ceiling"
            ));
        }
        let mut decoder = png::Decoder::new_with_limits(
            BufReader::new(file),
            png::Limits {
                bytes: MAX_PNG_DECODER_BYTES,
            },
        );
        let (width, height, color_type, bit_depth) = {
            let header = decoder
                .read_header_info()
                .map_err(|error| format!("invalid PNG header: {error}"))?;
            (
                header.width,
                header.height,
                header.color_type,
                header.bit_depth,
            )
        };
        if width > MAX_PAGE_DIMENSION || height > MAX_PAGE_DIMENSION {
            return Err(format!(
                "PNG dimensions {}x{} exceed the {}x{} CI safety ceiling",
                width, height, MAX_PAGE_DIMENSION, MAX_PAGE_DIMENSION
            ));
        }
        if (width, height) != (declared_size.width(), declared_size.height()) {
            return Ok(((width, height), color_type, bit_depth));
        }
        let mut reader = decoder
            .read_info()
            .map_err(|error| format!("invalid PNG header: {error}"))?;
        let buffer_size = reader
            .output_buffer_size()
            .ok_or_else(|| "decoded PNG buffer is too large".to_owned())?;
        if buffer_size > MAX_PAGE_DECODED_BYTES {
            return Err(format!(
                "decoded PNG requires {buffer_size} bytes, above the {MAX_PAGE_DECODED_BYTES} byte CI safety ceiling"
            ));
        }
        let mut buffer = Vec::new();
        buffer
            .try_reserve_exact(buffer_size)
            .map_err(|error| format!("could not reserve decoded PNG buffer: {error}"))?;
        buffer.resize(buffer_size, 0);
        let info = reader
            .next_frame(&mut buffer)
            .map_err(|error| format!("invalid PNG pixels: {error}"))?;
        Ok(((info.width, info.height), info.color_type, info.bit_depth))
    })();
    match result {
        Ok((actual_size, color_type, bit_depth)) => PageInspection {
            name: name.to_owned(),
            path,
            declared_size,
            actual_size: Some(actual_size),
            color_type: Some(color_type),
            bit_depth: Some(bit_depth),
            problem: None,
        },
        Err(problem) => PageInspection::failed(name, path, declared_size, problem),
    }
}

fn read_bounded(path: &Path, role: &'static str, maximum: u64) -> Result<Vec<u8>, SourceError> {
    let file = fs::File::open(path).map_err(|source| SourceError::Io {
        action: "open input file",
        path: path.to_owned(),
        source,
    })?;
    let mut bytes = Vec::new();
    file.take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| SourceError::Io {
            action: "read input file",
            path: path.to_owned(),
            source,
        })?;
    if bytes.len() as u64 > maximum {
        return Err(SourceError::InputTooLarge {
            role,
            path: path.to_owned(),
            maximum,
        });
    }
    Ok(bytes)
}

fn validate_atlas_line_count(bytes: &[u8], path: &Path, maximum: usize) -> Result<(), SourceError> {
    let mut lines = 0_usize;
    let mut line_start = 0_usize;
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        if matches!(bytes[cursor], b'\n' | b'\r') {
            lines += 1;
            if lines > maximum {
                return Err(SourceError::AtlasLineLimit {
                    path: path.to_owned(),
                    maximum,
                });
            }
            if bytes[cursor] == b'\r' && bytes.get(cursor + 1).is_some_and(|next| *next == b'\n') {
                cursor += 1;
            }
            cursor += 1;
            line_start = cursor;
        } else {
            cursor += 1;
        }
    }
    if line_start < bytes.len() && lines == maximum {
        return Err(SourceError::AtlasLineLimit {
            path: path.to_owned(),
            maximum,
        });
    }
    Ok(())
}

fn discover_json(directory: &Path) -> Result<PathBuf, SourceError> {
    let entries = fs::read_dir(directory).map_err(|source| SourceError::Io {
        action: "inspect input directory",
        path: directory.to_owned(),
        source,
    })?;
    let mut candidates = CandidateList::default();
    for entry in entries {
        let path = entry
            .map_err(|source| SourceError::Io {
                action: "inspect an input-directory entry",
                path: directory.to_owned(),
                source,
            })?
            .path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
            && path.is_file()
        {
            candidates.push(path);
        }
    }
    match candidates.total {
        1 => canonical_file(&candidates.sample[0], "skeleton JSON"),
        0 => Err(SourceError::MissingJson {
            directory: directory.to_owned(),
        }),
        _ => Err(SourceError::AmbiguousJson {
            directory: directory.to_owned(),
            candidates,
        }),
    }
}

fn ensure_json_file(path: &Path) -> Result<(), SourceError> {
    if !path.is_file() {
        return Err(SourceError::NotAFile {
            role: "skeleton JSON",
            path: path.to_owned(),
        });
    }
    if path
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        Ok(())
    } else {
        Err(SourceError::UnsupportedSkeletonPath {
            path: path.to_owned(),
        })
    }
}

fn canonical_file(path: &Path, role: &'static str) -> Result<PathBuf, SourceError> {
    let canonical = fs::canonicalize(path).map_err(|source| SourceError::Io {
        action: "open file",
        path: path.to_owned(),
        source,
    })?;
    if canonical.is_file() {
        Ok(canonical)
    } else {
        Err(SourceError::NotAFile {
            role,
            path: canonical,
        })
    }
}

fn discover_atlas(json_path: &Path) -> Result<PathBuf, SourceError> {
    let conventional = conventional_atlas_path(json_path)?;
    if conventional.is_file() {
        return canonical_file(&conventional, "text atlas");
    }
    let directory = json_path
        .parent()
        .ok_or_else(|| SourceError::UnsafeAtlasRoot {
            path: json_path.to_owned(),
        })?;
    let entries = fs::read_dir(directory).map_err(|source| SourceError::Io {
        action: "inspect the JSON directory for a text atlas",
        path: directory.to_owned(),
        source,
    })?;
    let mut candidates = CandidateList::default();
    for entry in entries {
        let path = entry
            .map_err(|source| SourceError::Io {
                action: "inspect a JSON-directory entry",
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
    match candidates.total {
        1 => canonical_file(&candidates.sample[0], "text atlas"),
        0 => Err(SourceError::MissingAtlas {
            json_path: json_path.to_owned(),
            expected_path: conventional,
        }),
        _ => Err(SourceError::AmbiguousAtlas {
            json_path: json_path.to_owned(),
            candidates,
        }),
    }
}

#[derive(Debug, Default)]
pub(crate) struct CandidateList {
    sample: Vec<PathBuf>,
    total: usize,
}

impl CandidateList {
    fn push(&mut self, path: PathBuf) {
        self.total = self.total.saturating_add(1);
        if self.sample.len() < MAX_AMBIGUOUS_CANDIDATES {
            self.sample.push(path);
            self.sample.sort();
        } else if self.sample.last().is_some_and(|last| path < *last) {
            self.sample.pop();
            self.sample.push(path);
            self.sample.sort();
        }
    }
}

impl std::fmt::Display for CandidateList {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (ordinal, path) in self.sample.iter().enumerate() {
            if ordinal > 0 {
                formatter.write_str(", ")?;
            }
            write!(formatter, "{}", path.display())?;
        }
        if self.total > self.sample.len() {
            write!(
                formatter,
                " (showing {} of {})",
                self.sample.len(),
                self.total
            )?;
        }
        Ok(())
    }
}

fn conventional_atlas_path(json_path: &Path) -> Result<PathBuf, SourceError> {
    let file_name = json_path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| SourceError::UnsupportedSkeletonPath {
            path: json_path.to_owned(),
        })?;
    let stem = file_name
        .strip_suffix(".spine.json")
        .or_else(|| file_name.strip_suffix(".json"))
        .filter(|stem| !stem.is_empty())
        .ok_or_else(|| SourceError::UnsupportedSkeletonPath {
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
    for component in Path::new(reference).components() {
        let Component::Normal(component) = component else {
            return Err("page paths may not contain `.` or `..` components");
        };
        let Some(component) = component.to_str() else {
            return Err("page path components must be valid Unicode");
        };
        validate_windows_portable_component(component)?;
    }
    Ok(())
}

fn validate_windows_portable_component(component: &str) -> Result<(), &'static str> {
    if component.is_empty() {
        return Err("page path components may not be empty");
    }
    if component.ends_with(['.', ' ']) {
        return Err("page path components may not end in a dot or space on Windows");
    }
    if component.chars().any(|character| {
        character <= '\u{001f}'
            || character == '\u{007f}'
            || matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*')
    }) {
        return Err("page path contains a character forbidden in Windows filenames");
    }
    let base = component
        .split('.')
        .next()
        .unwrap_or(component)
        .to_uppercase();
    let reserved = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || base
            .strip_prefix("COM")
            .is_some_and(is_reserved_device_suffix)
        || base
            .strip_prefix("LPT")
            .is_some_and(is_reserved_device_suffix);
    if reserved {
        return Err("page path uses a Windows reserved device name");
    }
    Ok(())
}

fn is_reserved_device_suffix(suffix: &str) -> bool {
    matches!(
        suffix,
        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
    )
}

fn looks_like_windows_drive_path(reference: &str) -> bool {
    let bytes = reference.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

#[derive(Debug, Error)]
pub(crate) enum SourceError {
    #[error("could not {action} at {}: {source}", path.display())]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{role} exceeds the {maximum} byte CI safety ceiling: {}", path.display())]
    InputTooLarge {
        role: &'static str,
        path: PathBuf,
        maximum: u64,
    },
    #[error(
        "text atlas exceeds the {maximum} logical-line CI safety ceiling: {}",
        path.display()
    )]
    AtlasLineLimit { path: PathBuf, maximum: usize },
    #[error("{role} is not a regular file: {}", path.display())]
    NotAFile { role: &'static str, path: PathBuf },
    #[error("expected a .json Spine skeleton export, got {}", path.display())]
    UnsupportedSkeletonPath { path: PathBuf },
    #[error("no skeleton JSON file was found in {}", directory.display())]
    MissingJson { directory: PathBuf },
    #[error(
        "multiple skeleton JSON files were found in {}; choose one explicitly: {}",
        directory.display(),
        candidates
    )]
    AmbiguousJson {
        directory: PathBuf,
        candidates: CandidateList,
    },
    #[error(
        "no text atlas was found beside {}; expected {} or a sole sibling .atlas file",
        json_path.display(),
        expected_path.display()
    )]
    MissingAtlas {
        json_path: PathBuf,
        expected_path: PathBuf,
    },
    #[error(
        "multiple text atlases were found beside {}; use --atlas to choose one: {}",
        json_path.display(),
        candidates
    )]
    AmbiguousAtlas {
        json_path: PathBuf,
        candidates: CandidateList,
    },
    #[error("the text atlas must live below a bounded directory, got {}", path.display())]
    UnsafeAtlasRoot { path: PathBuf },
    #[error(
        "atlas {} references disallowed page `{page}`: {reason}",
        atlas_path.display()
    )]
    DisallowedPageReference {
        atlas_path: PathBuf,
        page: String,
        reason: &'static str,
    },
    #[error("the atlas has {actual} pages; the CI safety ceiling is {maximum}")]
    TooManyPages { actual: usize, maximum: usize },
    #[error("atlas pages `{first}` and `{second}` collide on Windows")]
    PortablePageCollision { first: String, second: String },
    #[error("atlas page `{page}` exceeds a CI safety limit: {reason}")]
    PageLimit { page: String, reason: String },
}

impl SourceError {
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::Io { .. } => "source-io",
            Self::InputTooLarge { .. } => "source-input-too-large",
            Self::AtlasLineLimit { .. } => "source-atlas-line-limit",
            Self::NotAFile { .. } => "source-not-file",
            Self::UnsupportedSkeletonPath { .. } => "source-not-json",
            Self::MissingJson { .. } => "source-missing-json",
            Self::AmbiguousJson { .. } => "source-ambiguous-json",
            Self::MissingAtlas { .. } => "source-missing-atlas",
            Self::AmbiguousAtlas { .. } => "source-ambiguous-atlas",
            Self::UnsafeAtlasRoot { .. } => "source-unsafe-root",
            Self::DisallowedPageReference { .. } => "source-unsafe-page-reference",
            Self::TooManyPages { .. } => "source-too-many-pages",
            Self::PortablePageCollision { .. } => "source-page-name-collision",
            Self::PageLimit { .. } => "source-page-limit",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_reader_stops_after_the_ceiling() {
        let path =
            std::env::temp_dir().join(format!("spinal-bounded-read-{}.json", std::process::id()));
        fs::write(&path, b"12345").expect("write bounded-read fixture");
        let result = read_bounded(&path, "test JSON", 4);
        let _removed = fs::remove_file(&path);
        assert!(matches!(result, Err(SourceError::InputTooLarge { .. })));
    }

    #[test]
    fn atlas_line_ceiling_matches_lf_crlf_and_bare_cr() {
        let path = Path::new("cat.atlas");
        for bytes in [b"one\ntwo".as_slice(), b"one\r\ntwo", b"one\rtwo"] {
            assert!(validate_atlas_line_count(bytes, path, 2).is_ok());
            assert!(matches!(
                validate_atlas_line_count(bytes, path, 1),
                Err(SourceError::AtlasLineLimit { maximum: 1, .. })
            ));
        }
        assert!(validate_atlas_line_count(b"", path, 0).is_ok());
        assert!(validate_atlas_line_count(b"one\n", path, 1).is_ok());
    }

    #[test]
    fn oversized_png_files_stop_before_header_or_pixel_decoding() {
        let path =
            std::env::temp_dir().join(format!("spinal-oversized-page-{}.png", std::process::id()));
        let file = fs::File::create(&path).expect("create sparse PNG fixture");
        file.set_len(256 * 1024 * 1024 + 1)
            .expect("size sparse PNG fixture");
        drop(file);

        let inspection = inspect_png("oversized.png", path.clone(), PixelSize::new(1, 1));
        let _removed = fs::remove_file(path);
        assert!(
            inspection
                .problem
                .as_ref()
                .is_some_and(|problem| problem.contains("file-size CI safety ceiling")),
            "{inspection:?}"
        );
    }

    #[test]
    fn portable_component_rules_are_platform_independent() {
        for component in [
            "CON.png",
            "nul.PNG",
            "COM1.png",
            "LPT³.png",
            "stream.png:secret",
            "trailing.",
            "trailing ",
            "bad?.png",
        ] {
            assert!(
                validate_windows_portable_component(component).is_err(),
                "{component:?} must be rejected"
            );
        }
        for component in ["cat.png", "Animation_2.png", "coat.v2.png"] {
            assert_eq!(
                validate_windows_portable_component(component),
                Ok(()),
                "{component:?} must remain portable"
            );
        }
    }
}

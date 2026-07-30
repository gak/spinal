use std::{collections::HashMap, ops::Range, str};

use crate::{
    geometry::{
        AlphaEncoding, AtlasRotation, PixelRect, PixelSize, TextureFilter, TextureFormat, Trim,
        WrapMode,
    },
    load::error::{LoadDocument, LoadError, LoadErrorKind, SourceLocation},
};

#[derive(Clone, Debug)]
pub(crate) struct ParsedAtlas {
    pub(crate) pages: Vec<ParsedAtlasPage>,
    pub(crate) regions: Vec<ParsedAtlasRegion>,
    pub(crate) issues: Vec<AtlasIssue>,
}

#[derive(Clone, Debug)]
pub(crate) struct ParsedAtlasPage {
    pub(crate) name: Box<str>,
    pub(crate) size: PixelSize,
    pub(crate) format: TextureFormat,
    pub(crate) format_token: Box<str>,
    pub(crate) min_filter: TextureFilter,
    pub(crate) min_filter_token: Box<str>,
    pub(crate) mag_filter: TextureFilter,
    pub(crate) mag_filter_token: Box<str>,
    pub(crate) wrap: WrapMode,
    pub(crate) alpha_encoding: AlphaEncoding,
    pub(crate) scale: f32,
    pub(crate) region_range: Range<usize>,
    pub(crate) extensions: Vec<AtlasExtension>,
}

#[derive(Clone, Debug)]
pub(crate) struct ParsedAtlasRegion {
    pub(crate) page: usize,
    pub(crate) name: Box<str>,
    pub(crate) index: Option<u32>,
    pub(crate) bounds: PixelRect,
    pub(crate) offsets: Trim,
    pub(crate) rotation: AtlasRotation,
    pub(crate) split: Option<[i32; 4]>,
    pub(crate) pad: Option<[i32; 4]>,
    pub(crate) extensions: Vec<AtlasExtension>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AtlasExtension {
    pub(crate) key: Box<str>,
    pub(crate) value: Box<str>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AtlasIssueTarget {
    Page(usize),
    Region(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AtlasIssueKind {
    PremultipliedAlpha,
    UnsupportedRotation,
    UnsupportedPageSetting,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AtlasIssue {
    target: AtlasIssueTarget,
    kind: AtlasIssueKind,
    message: Box<str>,
}

impl AtlasIssue {
    pub(crate) const fn target(&self) -> AtlasIssueTarget {
        self.target
    }

    pub(crate) const fn kind(&self) -> AtlasIssueKind {
        self.kind
    }

    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Clone, Copy)]
struct Line<'a> {
    text: &'a str,
    number: usize,
    byte_offset: usize,
}

impl Line<'_> {
    fn location(self, path: &str, byte_in_line: usize) -> SourceLocation {
        let byte_in_line = byte_in_line.min(self.text.len());
        let column = self.text[..byte_in_line].chars().count() + 1;
        SourceLocation::for_document(LoadDocument::Atlas)
            .with_path(path)
            .with_text_position(self.number, column, Some(self.byte_offset + byte_in_line))
    }
}

#[derive(Clone, Copy)]
struct Property<'a> {
    key: &'a str,
    value: &'a str,
    key_start: usize,
    value_start: usize,
}

#[derive(Clone, Copy)]
struct Token<'a> {
    text: &'a str,
    start: usize,
}

struct PageDraft {
    size: PixelSize,
    format: TextureFormat,
    format_token: Box<str>,
    min_filter: TextureFilter,
    min_filter_token: Box<str>,
    mag_filter: TextureFilter,
    mag_filter_token: Box<str>,
    wrap: WrapMode,
    alpha_encoding: AlphaEncoding,
    scale: f32,
    extensions: Vec<AtlasExtension>,
}

impl Default for PageDraft {
    fn default() -> Self {
        Self {
            size: PixelSize::new(0, 0),
            format: TextureFormat::Rgba8888,
            format_token: "RGBA8888".into(),
            min_filter: TextureFilter::Nearest,
            min_filter_token: "Nearest".into(),
            mag_filter: TextureFilter::Nearest,
            mag_filter_token: "Nearest".into(),
            wrap: WrapMode::CLAMP,
            alpha_encoding: AlphaEncoding::Straight,
            scale: 1.0,
            extensions: Vec::new(),
        }
    }
}

pub(crate) fn parse_atlas(bytes: &[u8]) -> Result<ParsedAtlas, LoadError> {
    let (bytes, initial_offset) = if let Some(bytes) = bytes.strip_prefix(b"\xEF\xBB\xBF") {
        (bytes, 3)
    } else {
        (bytes, 0)
    };
    let source = str::from_utf8(bytes).map_err(|error| {
        let byte_offset = initial_offset + error.valid_up_to();
        let (line, column) = text_position(bytes, error.valid_up_to());
        LoadError::new(
            LoadErrorKind::InvalidUtf8,
            "atlas data is not valid UTF-8",
            SourceLocation::for_document(LoadDocument::Atlas).with_text_position(
                line,
                column,
                Some(byte_offset),
            ),
        )
    })?;
    let lines = scan_lines(source, initial_offset);
    reject_nul(&lines)?;

    let mut cursor = 0;
    let mut pages = Vec::new();
    let mut regions = Vec::new();
    let mut issues = Vec::new();
    let mut page_names: HashMap<Box<str>, (usize, SourceLocation)> = HashMap::new();
    let mut region_names: HashMap<(Box<str>, Option<u32>), (usize, SourceLocation)> =
        HashMap::new();

    skip_blank_lines(&lines, &mut cursor);
    while cursor < lines.len() {
        let page_index = pages.len();
        let name_line = lines[cursor];
        let (name, name_start) = title(name_line);
        let name_path = format!("/pages/{page_index}/name");
        if name.is_empty() {
            return Err(error_at(
                LoadErrorKind::SchemaViolation,
                "atlas page name cannot be empty",
                name_line,
                &name_path,
                name_start,
            ));
        }
        let name_location = name_line.location(&name_path, name_start);
        if let Some((previous_index, previous_location)) = page_names.get(name) {
            return Err(LoadError::new(
                LoadErrorKind::DuplicateName,
                format!("atlas page name {name:?} duplicates page {previous_index}"),
                name_location,
            )
            .with_related_locations(vec![previous_location.clone()].into_boxed_slice()));
        }
        page_names.insert(name.into(), (page_index, name_location));
        cursor += 1;

        let mut page = PageDraft::default();
        let mut seen = HashMap::new();
        while cursor < lines.len() && !is_blank(lines[cursor]) {
            let Some(property) = property(lines[cursor])? else {
                break;
            };
            parse_page_property(
                page_index,
                lines[cursor],
                property,
                &mut page,
                &mut seen,
                &mut issues,
            )?;
            cursor += 1;
        }
        validate_page_size(page_index, page.size, seen.get("size"))?;

        let first_region = regions.len();
        while cursor < lines.len() && !is_blank(lines[cursor]) {
            if property(lines[cursor])?.is_some() {
                let path = format!("/regions/{}/name", regions.len());
                return Err(error_at(
                    LoadErrorKind::Syntax,
                    "expected an atlas region name",
                    lines[cursor],
                    &path,
                    first_non_horizontal(lines[cursor].text),
                ));
            }
            let region_index = regions.len();
            let region = parse_region(
                &lines,
                &mut cursor,
                page_index,
                region_index,
                page.size,
                &mut issues,
            )?;
            let region_path = format!("/regions/{region_index}/name");
            let region_location = lines[region.name_line].location(
                &region_path,
                first_non_horizontal(lines[region.name_line].text),
            );
            let duplicate_key = (region.data.name.clone(), region.data.index);
            if let Some((previous_index, previous_location)) = region_names.get(&duplicate_key) {
                return Err(LoadError::new(
                    LoadErrorKind::DuplicateName,
                    format!(
                        "atlas region ({:?}, {:?}) duplicates region {previous_index}",
                        region.data.name, region.data.index
                    ),
                    region_location,
                )
                .with_related_locations(vec![previous_location.clone()].into_boxed_slice()));
            }
            region_names.insert(duplicate_key, (region_index, region_location));
            regions.push(region.data);
        }
        let region_range = first_region..regions.len();
        pages.push(ParsedAtlasPage {
            name: name.into(),
            size: page.size,
            format: page.format,
            format_token: page.format_token,
            min_filter: page.min_filter,
            min_filter_token: page.min_filter_token,
            mag_filter: page.mag_filter,
            mag_filter_token: page.mag_filter_token,
            wrap: page.wrap,
            alpha_encoding: page.alpha_encoding,
            scale: page.scale,
            region_range,
            extensions: page.extensions,
        });
        skip_blank_lines(&lines, &mut cursor);
    }

    if pages.is_empty() {
        return Err(LoadError::new(
            LoadErrorKind::SchemaViolation,
            "atlas must contain at least one page",
            SourceLocation::for_document(LoadDocument::Atlas).with_path("/pages/0/name"),
        ));
    }

    Ok(ParsedAtlas {
        pages,
        regions,
        issues,
    })
}

struct ParsedRegionWithLocation {
    data: ParsedAtlasRegion,
    name_line: usize,
}

fn parse_region(
    lines: &[Line<'_>],
    cursor: &mut usize,
    page_index: usize,
    region_index: usize,
    page_size: PixelSize,
    issues: &mut Vec<AtlasIssue>,
) -> Result<ParsedRegionWithLocation, LoadError> {
    let name_line_index = *cursor;
    let name_line = lines[*cursor];
    let (name, name_start) = title(name_line);
    let name_path = format!("/regions/{region_index}/name");
    if name.is_empty() {
        return Err(error_at(
            LoadErrorKind::SchemaViolation,
            "atlas region name cannot be empty",
            name_line,
            &name_path,
            name_start,
        ));
    }
    *cursor += 1;

    let mut index = None;
    let mut bounds = PixelRect::new(0, 0, 0, 0);
    let mut offsets = None;
    let mut rotation = AtlasRotation::ZERO;
    let mut split = None;
    let mut pad = None;
    let mut extensions = Vec::new();
    let mut seen = HashMap::new();

    while *cursor < lines.len() && !is_blank(lines[*cursor]) {
        let Some(property) = property(lines[*cursor])? else {
            break;
        };
        let line = lines[*cursor];
        let field_path = format!("/regions/{region_index}/{}", property.key);
        match property.key {
            "index" => {
                reject_duplicate(&mut seen, "index", line, property, &field_path)?;
                index = parse_index(line, property, &field_path)?;
            }
            "bounds" => {
                reject_duplicate(&mut seen, "bounds", line, property, &field_path)?;
                let values = parse_u32_array::<4>(line, property, &field_path)?;
                bounds = PixelRect::new(values[0], values[1], values[2], values[3]);
            }
            "offsets" => {
                reject_duplicate(&mut seen, "offsets", line, property, &field_path)?;
                let values = parse_u32_array::<4>(line, property, &field_path)?;
                offsets = Some(Trim::new(values[0], values[1], values[2], values[3]));
            }
            "rotate" => {
                reject_duplicate(&mut seen, "rotate", line, property, &field_path)?;
                rotation = parse_rotation(line, property, &field_path)?;
            }
            "split" => {
                reject_duplicate(&mut seen, "split", line, property, &field_path)?;
                let values = parse_i32_array::<4>(line, property, &field_path)?;
                if values.iter().any(|value| *value < 0) {
                    return Err(error_at(
                        LoadErrorKind::SchemaViolation,
                        "atlas split values cannot be negative",
                        line,
                        &field_path,
                        property.value_start,
                    ));
                }
                split = Some(values);
            }
            "pad" => {
                reject_duplicate(&mut seen, "pad", line, property, &field_path)?;
                let values = parse_i32_array::<4>(line, property, &field_path)?;
                if values.iter().any(|value| *value < -1) {
                    return Err(error_at(
                        LoadErrorKind::SchemaViolation,
                        "atlas pad values cannot be less than -1",
                        line,
                        &field_path,
                        property.value_start,
                    ));
                }
                pad = Some(values);
            }
            _ => extensions.push(AtlasExtension {
                key: property.key.into(),
                value: property.value.into(),
            }),
        }
        *cursor += 1;
    }

    let offsets = offsets.unwrap_or_else(|| Trim::new(0, 0, bounds.width(), bounds.height()));
    validate_region_geometry(
        region_index,
        page_size,
        bounds,
        offsets,
        rotation,
        split,
        pad,
        &seen,
    )?;
    if !rotation.is_quarter_turn() {
        issues.push(AtlasIssue {
            target: AtlasIssueTarget::Region(region_index),
            kind: AtlasIssueKind::UnsupportedRotation,
            message: format!(
                "region {name:?} uses unsupported packed rotation {} degrees",
                rotation.as_degrees()
            )
            .into(),
        });
    }

    Ok(ParsedRegionWithLocation {
        data: ParsedAtlasRegion {
            page: page_index,
            name: name.into(),
            index,
            bounds,
            offsets,
            rotation,
            split,
            pad,
            extensions,
        },
        name_line: name_line_index,
    })
}

fn parse_page_property(
    page_index: usize,
    line: Line<'_>,
    property: Property<'_>,
    page: &mut PageDraft,
    seen: &mut HashMap<&'static str, SourceLocation>,
    issues: &mut Vec<AtlasIssue>,
) -> Result<(), LoadError> {
    let path = format!("/pages/{page_index}/{}", property.key);
    match property.key {
        "size" => {
            reject_duplicate(seen, "size", line, property, &path)?;
            let values = parse_u32_array::<2>(line, property, &path)?;
            page.size = PixelSize::new(values[0], values[1]);
        }
        "format" => {
            reject_duplicate(seen, "format", line, property, &path)?;
            require_value(line, property, &path)?;
            page.format_token = property.value.into();
            page.format = texture_format(property.value);
            if page.format == TextureFormat::Unknown {
                unsupported_page_setting(
                    issues,
                    page_index,
                    format!("unknown atlas texture format {:?}", property.value),
                );
            }
        }
        "filter" => {
            reject_duplicate(seen, "filter", line, property, &path)?;
            let tokens = csv_tokens(property);
            require_arity(line, property, &path, &tokens, 2)?;
            require_token(line, tokens[0], &path)?;
            require_token(line, tokens[1], &path)?;
            page.min_filter_token = tokens[0].text.into();
            page.mag_filter_token = tokens[1].text.into();
            page.min_filter = texture_filter(tokens[0].text);
            page.mag_filter = texture_filter(tokens[1].text);
            if page.min_filter == TextureFilter::Unknown
                || page.mag_filter == TextureFilter::Unknown
            {
                unsupported_page_setting(
                    issues,
                    page_index,
                    format!(
                        "unknown atlas texture filter {:?}, {:?}",
                        tokens[0].text, tokens[1].text
                    ),
                );
            }
        }
        "repeat" => {
            reject_duplicate(seen, "repeat", line, property, &path)?;
            require_value(line, property, &path)?;
            page.wrap = match property.value {
                "none" => WrapMode::CLAMP,
                "x" => WrapMode::new(true, false),
                "y" => WrapMode::new(false, true),
                "xy" => WrapMode::new(true, true),
                unknown => {
                    page.extensions.push(AtlasExtension {
                        key: "repeat".into(),
                        value: unknown.into(),
                    });
                    unsupported_page_setting(
                        issues,
                        page_index,
                        format!("unknown atlas repeat setting {unknown:?}"),
                    );
                    WrapMode::CLAMP
                }
            };
        }
        "pma" => {
            reject_duplicate(seen, "pma", line, property, &path)?;
            page.alpha_encoding = match property.value {
                "false" => AlphaEncoding::Straight,
                "true" => {
                    issues.push(AtlasIssue {
                        target: AtlasIssueTarget::Page(page_index),
                        kind: AtlasIssueKind::PremultipliedAlpha,
                        message: "premultiplied-alpha page differs from the straight-alpha profile"
                            .into(),
                    });
                    AlphaEncoding::Premultiplied
                }
                _ => {
                    return Err(error_at(
                        LoadErrorKind::Syntax,
                        "atlas pma must be true or false",
                        line,
                        &path,
                        property.value_start,
                    ));
                }
            };
        }
        "scale" => {
            reject_duplicate(seen, "scale", line, property, &path)?;
            page.scale = parse_f32(line, property, &path)?;
            if page.scale <= 0.0 {
                return Err(error_at(
                    LoadErrorKind::SchemaViolation,
                    "atlas page scale must be positive",
                    line,
                    &path,
                    property.value_start,
                ));
            }
        }
        _ => {
            page.extensions.push(AtlasExtension {
                key: property.key.into(),
                value: property.value.into(),
            });
            unsupported_page_setting(
                issues,
                page_index,
                format!(
                    "unknown atlas page setting {:?}: {:?}",
                    property.key, property.value
                ),
            );
        }
    }
    Ok(())
}

fn validate_page_size(
    page_index: usize,
    size: PixelSize,
    location: Option<&SourceLocation>,
) -> Result<(), LoadError> {
    if (size.width() == 0) != (size.height() == 0) {
        let location = location.cloned().unwrap_or_else(|| {
            SourceLocation::for_document(LoadDocument::Atlas)
                .with_path(format!("/pages/{page_index}/size"))
        });
        return Err(LoadError::new(
            LoadErrorKind::SchemaViolation,
            "atlas page size must have either two zero or two positive dimensions",
            location,
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_region_geometry(
    region_index: usize,
    page_size: PixelSize,
    bounds: PixelRect,
    offsets: Trim,
    rotation: AtlasRotation,
    split: Option<[i32; 4]>,
    pad: Option<[i32; 4]>,
    seen: &HashMap<&'static str, SourceLocation>,
) -> Result<(), LoadError> {
    let offsets_path = format!("/regions/{region_index}/offsets");
    let offsets_location = seen.get("offsets").cloned().unwrap_or_else(|| {
        SourceLocation::for_document(LoadDocument::Atlas).with_path(offsets_path.as_str())
    });
    let original = offsets.original_size();
    let horizontal = offsets
        .left()
        .checked_add(bounds.width())
        .is_some_and(|used| used <= original.width());
    let vertical = offsets
        .bottom()
        .checked_add(bounds.height())
        .is_some_and(|used| used <= original.height());
    if !horizontal || !vertical {
        return Err(LoadError::new(
            LoadErrorKind::SchemaViolation,
            "atlas offsets and packed bounds exceed the original image size",
            offsets_location,
        ));
    }

    validate_ninepatch(region_index, original, split, pad, seen)?;

    if page_size.width() == 0 || !rotation.is_quarter_turn() {
        return Ok(());
    }
    let quarter = rotation.as_degrees();
    let (width, height) = if matches!(quarter, 90.0 | 270.0) {
        (bounds.height(), bounds.width())
    } else {
        (bounds.width(), bounds.height())
    };
    let fits = bounds
        .x()
        .checked_add(width)
        .zip(bounds.y().checked_add(height))
        .is_some_and(|(right, top)| right <= page_size.width() && top <= page_size.height());
    if !fits {
        let path = format!("/regions/{region_index}/bounds");
        let location = seen.get("bounds").cloned().unwrap_or_else(|| {
            SourceLocation::for_document(LoadDocument::Atlas).with_path(path.as_str())
        });
        return Err(LoadError::new(
            LoadErrorKind::SchemaViolation,
            "atlas region bounds exceed the page dimensions",
            location,
        ));
    }
    Ok(())
}

fn validate_ninepatch(
    region_index: usize,
    original: PixelSize,
    split: Option<[i32; 4]>,
    pad: Option<[i32; 4]>,
    seen: &HashMap<&'static str, SourceLocation>,
) -> Result<(), LoadError> {
    if let Some(values) = split
        && !insets_fit(values, original, false)
    {
        return Err(ninepatch_error(region_index, "split", seen));
    }
    if let Some(values) = pad
        && !insets_fit(values, original, true)
    {
        return Err(ninepatch_error(region_index, "pad", seen));
    }
    Ok(())
}

fn insets_fit(values: [i32; 4], size: PixelSize, allow_absent: bool) -> bool {
    let pair_fits = |first: i32, second: i32, extent: u32| {
        if allow_absent && (first == -1 || second == -1) {
            true
        } else {
            first >= 0
                && second >= 0
                && u32::try_from(first)
                    .ok()
                    .and_then(|first| {
                        u32::try_from(second)
                            .ok()
                            .and_then(|second| first.checked_add(second))
                    })
                    .is_some_and(|used| used <= extent)
        }
    };
    pair_fits(values[0], values[1], size.width()) && pair_fits(values[2], values[3], size.height())
}

fn ninepatch_error(
    region_index: usize,
    field: &'static str,
    seen: &HashMap<&'static str, SourceLocation>,
) -> LoadError {
    let path = format!("/regions/{region_index}/{field}");
    let location = seen.get(field).cloned().unwrap_or_else(|| {
        SourceLocation::for_document(LoadDocument::Atlas).with_path(path.as_str())
    });
    LoadError::new(
        LoadErrorKind::SchemaViolation,
        format!("atlas {field} values exceed the original image size"),
        location,
    )
}

fn reject_duplicate(
    seen: &mut HashMap<&'static str, SourceLocation>,
    field: &'static str,
    line: Line<'_>,
    property: Property<'_>,
    path: &str,
) -> Result<(), LoadError> {
    let location = line.location(path, property.key_start);
    if let Some(previous) = seen.insert(field, location.clone()) {
        return Err(LoadError::new(
            LoadErrorKind::SchemaViolation,
            format!("atlas field {field:?} is duplicated"),
            location,
        )
        .with_related_locations(vec![previous].into_boxed_slice()));
    }
    Ok(())
}

fn parse_index(
    line: Line<'_>,
    property: Property<'_>,
    path: &str,
) -> Result<Option<u32>, LoadError> {
    require_value(line, property, path)?;
    let value = property.value.parse::<i128>().map_err(|_error| {
        error_at(
            LoadErrorKind::Syntax,
            "atlas index must be an integer",
            line,
            path,
            property.value_start,
        )
    })?;
    match value {
        -1 => Ok(None),
        value if (0..=i128::from(u32::MAX)).contains(&value) => Ok(Some(value as u32)),
        value if value > i128::from(u32::MAX) => Err(error_at(
            LoadErrorKind::CapacityExceeded,
            "atlas index exceeds the supported range",
            line,
            path,
            property.value_start,
        )),
        _ => Err(error_at(
            LoadErrorKind::SchemaViolation,
            "atlas index must be -1 or nonnegative",
            line,
            path,
            property.value_start,
        )),
    }
}

fn parse_rotation(
    line: Line<'_>,
    property: Property<'_>,
    path: &str,
) -> Result<AtlasRotation, LoadError> {
    let degrees = match property.value {
        "true" => 90.0,
        "false" => 0.0,
        _ => parse_f32(line, property, path)?,
    };
    AtlasRotation::new(degrees).ok_or_else(|| {
        error_at(
            LoadErrorKind::SchemaViolation,
            "atlas rotation must be between 0 and 360 degrees",
            line,
            path,
            property.value_start,
        )
    })
}

fn parse_f32(line: Line<'_>, property: Property<'_>, path: &str) -> Result<f32, LoadError> {
    require_value(line, property, path)?;
    let value = property.value.parse::<f32>().map_err(|_error| {
        error_at(
            LoadErrorKind::Syntax,
            "atlas value must be a number",
            line,
            path,
            property.value_start,
        )
    })?;
    if !value.is_finite() {
        return Err(error_at(
            LoadErrorKind::NonFiniteNumber,
            "atlas value must be finite",
            line,
            path,
            property.value_start,
        ));
    }
    Ok(value)
}

fn parse_u32_array<const N: usize>(
    line: Line<'_>,
    property: Property<'_>,
    path: &str,
) -> Result<[u32; N], LoadError> {
    let tokens = csv_tokens(property);
    require_arity(line, property, path, &tokens, N)?;
    let mut values = [0; N];
    for (output, token) in values.iter_mut().zip(tokens) {
        require_token(line, token, path)?;
        *output = token.text.parse::<u32>().map_err(|_error| {
            error_at(
                LoadErrorKind::Syntax,
                "atlas pixel values must be nonnegative integers",
                line,
                path,
                token.start,
            )
        })?;
    }
    Ok(values)
}

fn parse_i32_array<const N: usize>(
    line: Line<'_>,
    property: Property<'_>,
    path: &str,
) -> Result<[i32; N], LoadError> {
    let tokens = csv_tokens(property);
    require_arity(line, property, path, &tokens, N)?;
    let mut values = [0; N];
    for (output, token) in values.iter_mut().zip(tokens) {
        require_token(line, token, path)?;
        *output = token.text.parse::<i32>().map_err(|_error| {
            error_at(
                LoadErrorKind::Syntax,
                "atlas values must be 32-bit integers",
                line,
                path,
                token.start,
            )
        })?;
    }
    Ok(values)
}

fn require_arity(
    line: Line<'_>,
    property: Property<'_>,
    path: &str,
    tokens: &[Token<'_>],
    expected: usize,
) -> Result<(), LoadError> {
    if tokens.len() != expected {
        return Err(error_at(
            LoadErrorKind::SchemaViolation,
            format!(
                "atlas field {:?} requires {expected} comma-separated values",
                property.key
            ),
            line,
            path,
            property.value_start,
        ));
    }
    Ok(())
}

fn require_value(line: Line<'_>, property: Property<'_>, path: &str) -> Result<(), LoadError> {
    require_token(
        line,
        Token {
            text: property.value,
            start: property.value_start,
        },
        path,
    )
}

fn require_token(line: Line<'_>, token: Token<'_>, path: &str) -> Result<(), LoadError> {
    if token.text.is_empty() {
        return Err(error_at(
            LoadErrorKind::Syntax,
            "atlas field value cannot be empty",
            line,
            path,
            token.start,
        ));
    }
    Ok(())
}

fn texture_format(value: &str) -> TextureFormat {
    match value {
        "Alpha" => TextureFormat::Alpha,
        "Intensity" => TextureFormat::Intensity,
        "LuminanceAlpha" => TextureFormat::LuminanceAlpha,
        "RGB565" => TextureFormat::Rgb565,
        "RGBA4444" => TextureFormat::Rgba4444,
        "RGB888" => TextureFormat::Rgb888,
        "RGBA8888" => TextureFormat::Rgba8888,
        _ => TextureFormat::Unknown,
    }
}

fn texture_filter(value: &str) -> TextureFilter {
    match value {
        "Nearest" => TextureFilter::Nearest,
        "Linear" => TextureFilter::Linear,
        "MipMap" => TextureFilter::MipMap,
        "MipMapNearestNearest" => TextureFilter::MipMapNearestNearest,
        "MipMapLinearNearest" => TextureFilter::MipMapLinearNearest,
        "MipMapNearestLinear" => TextureFilter::MipMapNearestLinear,
        "MipMapLinearLinear" => TextureFilter::MipMapLinearLinear,
        _ => TextureFilter::Unknown,
    }
}

fn unsupported_page_setting(issues: &mut Vec<AtlasIssue>, page_index: usize, message: String) {
    issues.push(AtlasIssue {
        target: AtlasIssueTarget::Page(page_index),
        kind: AtlasIssueKind::UnsupportedPageSetting,
        message: message.into(),
    });
}

fn property(line: Line<'_>) -> Result<Option<Property<'_>>, LoadError> {
    let (outer_start, outer_end) = horizontal_bounds(line.text);
    let trimmed = &line.text[outer_start..outer_end];
    let Some(colon) = trimmed.find(':') else {
        return Ok(None);
    };
    let (key_relative_start, key_relative_end) = horizontal_bounds(&trimmed[..colon]);
    let key_start = outer_start + key_relative_start;
    let key = &trimmed[key_relative_start..key_relative_end];
    if key.is_empty() {
        return Err(error_at(
            LoadErrorKind::Syntax,
            "atlas property name cannot be empty",
            line,
            "/",
            outer_start + colon,
        ));
    }
    let value_source = &trimmed[colon + 1..];
    let (value_relative_start, value_relative_end) = horizontal_bounds(value_source);
    let value_start = outer_start + colon + 1 + value_relative_start;
    let value = &value_source[value_relative_start..value_relative_end];
    Ok(Some(Property {
        key,
        value,
        key_start,
        value_start,
    }))
}

fn csv_tokens(property: Property<'_>) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut start = 0;
    for (index, byte) in property.value.bytes().enumerate() {
        if byte == b',' {
            tokens.push(csv_token(property, start, index));
            start = index + 1;
        }
    }
    tokens.push(csv_token(property, start, property.value.len()));
    tokens
}

fn csv_token(property: Property<'_>, start: usize, end: usize) -> Token<'_> {
    let source = &property.value[start..end];
    let (trim_start, trim_end) = horizontal_bounds(source);
    Token {
        text: &source[trim_start..trim_end],
        start: property.value_start + start + trim_start,
    }
}

fn title(line: Line<'_>) -> (&str, usize) {
    let (start, end) = horizontal_bounds(line.text);
    (&line.text[start..end], start)
}

fn horizontal_bounds(value: &str) -> (usize, usize) {
    let start = value
        .bytes()
        .position(|byte| !matches!(byte, b' ' | b'\t'))
        .unwrap_or(value.len());
    let end = value
        .bytes()
        .rposition(|byte| !matches!(byte, b' ' | b'\t'))
        .map_or(start, |index| index + 1);
    (start, end)
}

fn first_non_horizontal(value: &str) -> usize {
    horizontal_bounds(value).0
}

fn is_blank(line: Line<'_>) -> bool {
    let (start, end) = horizontal_bounds(line.text);
    start == end
}

fn skip_blank_lines(lines: &[Line<'_>], cursor: &mut usize) {
    while *cursor < lines.len() && is_blank(lines[*cursor]) {
        *cursor += 1;
    }
}

fn scan_lines(source: &str, initial_offset: usize) -> Vec<Line<'_>> {
    let bytes = source.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0;
    let mut number = 1;
    let mut cursor = 0;
    while cursor < bytes.len() {
        if matches!(bytes[cursor], b'\n' | b'\r') {
            lines.push(Line {
                text: &source[start..cursor],
                number,
                byte_offset: initial_offset + start,
            });
            if bytes[cursor] == b'\r' && bytes.get(cursor + 1).is_some_and(|byte| *byte == b'\n') {
                cursor += 1;
            }
            cursor += 1;
            start = cursor;
            number += 1;
        } else {
            cursor += 1;
        }
    }
    if start < source.len() {
        lines.push(Line {
            text: &source[start..],
            number,
            byte_offset: initial_offset + start,
        });
    }
    lines
}

fn reject_nul(lines: &[Line<'_>]) -> Result<(), LoadError> {
    for line in lines {
        if let Some(byte) = line.text.bytes().position(|byte| byte == 0) {
            return Err(error_at(
                LoadErrorKind::Syntax,
                "atlas data cannot contain NUL bytes",
                *line,
                "/",
                byte,
            ));
        }
    }
    Ok(())
}

fn text_position(bytes: &[u8], end: usize) -> (usize, usize) {
    let valid = str::from_utf8(&bytes[..end]).unwrap_or_default();
    let mut line = 1;
    let mut column = 1;
    let mut chars = valid.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                line += 1;
                column = 1;
            }
            '\n' => {
                line += 1;
                column = 1;
            }
            _ => column += 1,
        }
    }
    (line, column)
}

fn error_at(
    kind: LoadErrorKind,
    message: impl Into<Box<str>>,
    line: Line<'_>,
    path: &str,
    byte_in_line: usize,
) -> LoadError {
    LoadError::new(kind, message, line.location(path, byte_in_line))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modern_multi_page_atlas_and_documented_defaults() {
        let atlas = parse_atlas(
            b"\xEF\xBB\xBF\r\n\
first.png\r\n\
\tsize: 16, 8\r\n\
\tformat: RGBA8888\r\n\
\tfilter: Linear, Nearest\r\n\
\trepeat: x\r\n\
\tpma: false\r\n\
spark\r\n\
\tindex: 0\r\n\
\tbounds: 1, 2, 3, 4\r\n\
\r\n\
empty.png\r\n\
\r\n\
third.png\r\n\
spark\r\n\
\tindex: 1",
        )
        .expect("modern atlas should parse");

        assert_eq!(atlas.pages.len(), 3);
        assert_eq!(atlas.regions.len(), 2);
        assert_eq!(atlas.pages[0].name.as_ref(), "first.png");
        assert_eq!(atlas.pages[0].region_range, 0..1);
        assert_eq!(atlas.pages[1].region_range, 1..1);
        assert_eq!(atlas.pages[2].region_range, 1..2);
        assert_eq!(atlas.pages[0].min_filter, TextureFilter::Linear);
        assert_eq!(atlas.pages[0].mag_filter, TextureFilter::Nearest);
        assert_eq!(atlas.pages[0].wrap, WrapMode::new(true, false));
        assert_eq!(atlas.regions[1].bounds, PixelRect::new(0, 0, 0, 0));
        assert_eq!(
            atlas.regions[1].offsets.original_size(),
            PixelSize::new(0, 0)
        );
        assert!(atlas.issues.is_empty());
    }

    #[test]
    fn retains_extensions_and_reports_supported_fallbacks() {
        let atlas = parse_atlas(
            b"cat.png\n\
\tformat: FutureColour\n\
\tfilter: FutureMin, Linear\n\
\tfuture-page: yes:still\n\
\tpma: true\n\
cat/body\n\
\tbounds: 1, 2, 3, 4\n\
\trotate: 45\n\
\torigin: 10, 20\n\
\torigin: 30, 40\n",
        )
        .expect("unsupported but safely delimited data should load");

        assert_eq!(atlas.pages[0].format, TextureFormat::Unknown);
        assert_eq!(atlas.pages[0].format_token.as_ref(), "FutureColour");
        assert_eq!(atlas.pages[0].extensions.len(), 1);
        assert_eq!(atlas.regions[0].extensions.len(), 2);
        assert_eq!(atlas.issues.len(), 5);
        assert!(atlas.issues.iter().any(|issue| {
            issue.target() == AtlasIssueTarget::Page(0)
                && issue.kind() == AtlasIssueKind::PremultipliedAlpha
        }));
        assert!(atlas.issues.iter().any(|issue| {
            issue.target() == AtlasIssueTarget::Region(0)
                && issue.kind() == AtlasIssueKind::UnsupportedRotation
        }));
    }

    #[test]
    fn boolean_rotation_and_rotated_page_extents_are_checked() {
        let atlas = parse_atlas(
            b"cat.png\n\
\tsize: 100, 50\n\
head\n\
\tbounds: 70, 0, 40, 30\n\
\trotate: true\n",
        )
        .expect("90 degree rotation swaps page-space extents");
        assert_eq!(atlas.regions[0].rotation.as_degrees(), 90.0);

        let error = parse_atlas(
            b"cat.png\n\
\tsize: 100, 50\n\
head\n\
\tbounds: 70, 0, 40, 31\n\
\trotate: true\n",
        )
        .expect_err("rotated width exceeds the page");
        assert_eq!(error.kind(), LoadErrorKind::SchemaViolation);
        assert_eq!(error.path(), Some("/regions/0/bounds"));
    }

    #[test]
    fn omitted_offsets_inherit_packed_size_and_explicit_offsets_are_validated() {
        let atlas = parse_atlas(b"cat.png\nbody\n\tbounds: 1,2,30,20\n").expect("valid defaults");
        assert_eq!(atlas.regions[0].offsets, Trim::new(0, 0, 30, 20));

        let error = parse_atlas(
            b"cat.png\n\
body\n\
\tbounds: 1,2,30,20\n\
\toffsets: 5,0,34,20\n",
        )
        .expect_err("trim cannot exceed original width");
        assert_eq!(error.path(), Some("/regions/0/offsets"));
    }

    #[test]
    fn rejects_duplicate_known_fields_pages_and_region_keys() {
        let duplicate_field =
            parse_atlas(b"cat.png\n\tsize: 1,1\n\tsize: 1,1\n").expect_err("duplicate field");
        assert_eq!(duplicate_field.path(), Some("/pages/0/size"));
        assert_eq!(duplicate_field.related_locations().len(), 1);

        let duplicate_page = parse_atlas(b"cat.png\n\ncat.png\n").expect_err("duplicate page");
        assert_eq!(duplicate_page.kind(), LoadErrorKind::DuplicateName);
        assert_eq!(duplicate_page.path(), Some("/pages/1/name"));

        let duplicate_region = parse_atlas(
            b"cat.png\n\
spark\n\
\tindex: 1\n\
\n\
other.png\n\
spark\n\
\tindex: 1\n",
        )
        .expect_err("duplicate composite region key");
        assert_eq!(duplicate_region.kind(), LoadErrorKind::DuplicateName);
        assert_eq!(duplicate_region.path(), Some("/regions/1/name"));
    }

    #[test]
    fn errors_include_the_original_line_column_and_byte_offset() {
        let error = parse_atlas(b"\xEF\xBB\xBFcat.png\r\n\tsize: 10, nope\r\n")
            .expect_err("invalid integer");
        assert_eq!(error.kind(), LoadErrorKind::Syntax);
        assert_eq!(error.location().line(), Some(2));
        assert_eq!(error.location().column(), Some(12));
        assert_eq!(error.location().byte_offset(), Some(23));
    }

    #[test]
    fn accepts_lone_carriage_returns_spaces_and_missing_final_newline() {
        let atlas = parse_atlas(
            b"  cat.png \r\
 size : 8 , 8 \r\
 body \r\
 bounds : 0 , 0 , 8 , 8 ",
        )
        .expect("horizontal whitespace and universal newlines should parse");
        assert_eq!(atlas.pages[0].name.as_ref(), "cat.png");
        assert_eq!(atlas.regions[0].name.as_ref(), "body");
    }

    #[test]
    fn arbitrary_bytes_never_panic() {
        for bytes in [
            &[][..],
            &[0xFF, 0xFE, 0xFD],
            b"\0\0\0",
            b"page.png\n\tsize: 1,\n",
            b"page.png\nregion\n\trotate: NaN\n",
        ] {
            let result = std::panic::catch_unwind(|| parse_atlas(bytes));
            assert!(result.is_ok(), "parser panicked for {bytes:?}");
            assert!(result.expect("catch unwind").is_err());
        }
    }

    #[test]
    fn invalid_utf8_after_a_newline_has_an_exact_location() {
        let error = parse_atlas(b"page.png\r\n\xFF").expect_err("invalid UTF-8");
        assert_eq!(error.kind(), LoadErrorKind::InvalidUtf8);
        assert_eq!(error.location().line(), Some(2));
        assert_eq!(error.location().column(), Some(1));
        assert_eq!(error.location().byte_offset(), Some(10));
    }

    #[test]
    fn historical_owned_fixtures_are_non_normative_smoke_tests() {
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets");
        if !assets.is_dir() {
            // Published packages intentionally exclude licensed example assets.
            return;
        }

        for (path, expected_regions) in [
            ("spineboy-ess-4.1/spineboy-ess.atlas", 26),
            ("spineboy-pro-4.1/spineboy-pro.atlas", 40),
            ("raptor-pro-4.1/raptor-pro.atlas", 38),
        ] {
            let bytes = std::fs::read(assets.join(path)).expect("tracked historical atlas exists");
            let atlas = parse_atlas(&bytes).expect("historical modern-format atlas should parse");
            assert_eq!(atlas.pages.len(), 1);
            assert_eq!(atlas.regions.len(), expected_regions);
        }
    }
}

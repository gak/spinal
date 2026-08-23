//! Deterministic, read-only native inspection without a Bevy application.

use std::{
    error::Error,
    ffi::{OsStr, OsString},
    fmt::{self, Write as FmtWrite},
    io::{self, Write as IoWrite},
    path::{Path, PathBuf},
};

use bevy::app::AppExit;
use bevy_spinal::spinal::{LoadErrorKind, RuntimeBundleError};
use serde::Serialize;

use crate::{
    inspection::{InspectionOutcome, SourceInspection},
    source::{PrepareError, PreparedSource},
};

pub(crate) const USAGE_EXIT_CODE: u8 = 2;
const DEGRADED_EXIT_CODE: u8 = 1;
const SOURCE_EXIT_CODE: u8 = 3;
const INTERNAL_EXIT_CODE: u8 = 4;
const MAX_ERROR_VALUE_BYTES: usize = 128;

const HELP: &str = "\
Spinal — Check

USAGE:
    spinal check SKELETON.json [--atlas FILE.atlas] [--bundle-root DIR] [--json]

OPTIONS:
    --atlas FILE.atlas  Use this text atlas instead of discovering one
    --bundle-root DIR   Set the package root (default: JSON directory)
    --json              Emit compact versioned JSON
    -h, --help          Print this help

EXIT STATUS:
    0  Compatible
    1  Loadable with degraded behavior
    2  Invalid command arguments
    3  Source unavailable or rejected
    4  Internal output failure
";

/// Runs a single read-only inspection and never constructs a Bevy application.
pub(crate) fn run<I, A>(arguments: I) -> AppExit
where
    I: IntoIterator<Item = A>,
    A: Into<OsString>,
{
    let options = match CheckOptions::parse(arguments) {
        Ok(CheckParseResult::Run(options)) => options,
        Ok(CheckParseResult::Help) => {
            return emit_stdout(HELP.as_bytes())
                .map_or_else(output_error_exit, |()| AppExit::Success);
        }
        Err(error) => {
            eprintln!("spinal check: {error}\n\n{HELP}");
            return AppExit::from_code(USAGE_EXIT_CODE);
        }
    };

    let prepared = match PreparedSource::load_single(
        options.json_path(),
        options.atlas_path(),
        options.bundle_root(),
    ) {
        Ok(prepared) => prepared,
        Err(error) => return source_error_exit(&options, &error),
    };
    let inspection = SourceInspection::capture(prepared.bundle());
    let bytes = if options.json() {
        inspection.to_canonical_json()
    } else {
        render_human(&inspection)
    };
    let mut bytes = match bytes {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("spinal check: could not serialize the inspection report: {error}");
            return AppExit::from_code(INTERNAL_EXIT_CODE);
        }
    };
    bytes.push(b'\n');
    if let Err(error) = emit_stdout(&bytes) {
        return output_error_exit(error);
    }

    outcome_exit(inspection.outcome())
}

fn outcome_exit(outcome: InspectionOutcome) -> AppExit {
    if outcome.is_degraded() {
        AppExit::from_code(DEGRADED_EXIT_CODE)
    } else {
        AppExit::Success
    }
}

fn render_human(inspection: &SourceInspection) -> Result<Vec<u8>, serde_json::Error> {
    let source = inspection.source();
    let inventory = inspection.inventory();
    let counts = *inventory.counts();
    let mut output = String::new();
    writeln!(
        output,
        "Spinal check: {}",
        match inspection.outcome() {
            InspectionOutcome::Compatible => "compatible",
            InspectionOutcome::Degraded => "degraded",
        }
    )
    .expect("writing to a String cannot fail");
    writeln!(output, "Report format: {}", inspection.format_version())
        .expect("writing to a String cannot fail");
    writeln!(
        output,
        "Spine: {} (target {})",
        source.declared_spine_version(),
        source.target_spine_version()
    )
    .expect("writing to a String cannot fail");
    writeln!(
        output,
        "Source: {} + {}",
        quote(source.json_path())?,
        quote(source.atlas_path())?
    )
    .expect("writing to a String cannot fail");
    writeln!(output, "Manifest SHA-256: {}", source.manifest_sha256())
        .expect("writing to a String cannot fail");
    writeln!(output, "Content SHA-256: {}", source.content_sha256())
        .expect("writing to a String cannot fail");
    writeln!(
        output,
        "Bundle: {} {}, {} encoded {}, {} decoded texture {}",
        source.file_count(),
        plural(source.file_count(), "file", "files"),
        source.encoded_bytes(),
        plural(source.encoded_bytes(), "byte", "bytes"),
        source.decoded_texture_bytes(),
        plural(source.decoded_texture_bytes(), "byte", "bytes")
    )
    .expect("writing to a String cannot fail");
    writeln!(
        output,
        "Inventory: {} {}, {} {}, {} {}, {} {}, {} {}",
        counts.bones(),
        plural(u64::from(counts.bones()), "bone", "bones"),
        counts.slots(),
        plural(u64::from(counts.slots()), "slot", "slots"),
        counts.skins(),
        plural(u64::from(counts.skins()), "skin", "skins"),
        counts.attachments(),
        plural(u64::from(counts.attachments()), "attachment", "attachments"),
        counts.animations(),
        plural(u64::from(counts.animations()), "animation", "animations")
    )
    .expect("writing to a String cannot fail");
    writeln!(
        output,
        "Constraints: {} IK, {} transform, {} total; {} {}",
        counts.ik_constraints(),
        counts.transform_constraints(),
        counts.constraints(),
        counts.events(),
        plural(u64::from(counts.events()), "event", "events")
    )
    .expect("writing to a String cannot fail");
    writeln!(
        output,
        "Atlas: {} {}, {} {}",
        counts.atlas_pages(),
        plural(u64::from(counts.atlas_pages()), "page", "pages"),
        counts.atlas_regions(),
        plural(u64::from(counts.atlas_regions()), "region", "regions")
    )
    .expect("writing to a String cannot fail");
    writeln!(output, "Animations:").expect("writing to a String cannot fail");
    if inventory.animations().is_empty() {
        writeln!(output, "  (none)").expect("writing to a String cannot fail");
    } else {
        for (index, animation) in inventory.animations().iter().enumerate() {
            writeln!(
                output,
                "  {}. {} — {}",
                index + 1,
                quote(animation.name())?,
                human_duration(animation.duration_ns())
            )
            .expect("writing to a String cannot fail");
            if animation.name_was_truncated() {
                writeln!(output, "     (name truncated)").expect("writing to a String cannot fail");
            }
        }
    }
    if inventory.animations_are_truncated() {
        writeln!(
            output,
            "  … {} more {} omitted",
            inventory.omitted_animation_count(),
            plural(
                u64::from(inventory.omitted_animation_count()),
                "animation",
                "animations"
            )
        )
        .expect("writing to a String cannot fail");
    }
    writeln!(output, "Skins:").expect("writing to a String cannot fail");
    if inventory.skins().is_empty() {
        writeln!(output, "  (none)").expect("writing to a String cannot fail");
    } else {
        for (index, skin) in inventory.skins().iter().enumerate() {
            writeln!(
                output,
                "  {}. {}{}",
                index + 1,
                quote(skin.name())?,
                if skin.is_default() { " (default)" } else { "" }
            )
            .expect("writing to a String cannot fail");
            if skin.name_was_truncated() {
                writeln!(output, "     (name truncated)").expect("writing to a String cannot fail");
            }
        }
    }
    if inventory.skins_are_truncated() {
        writeln!(
            output,
            "  … {} more {} omitted",
            inventory.omitted_skin_count(),
            plural(u64::from(inventory.omitted_skin_count()), "skin", "skins")
        )
        .expect("writing to a String cannot fail");
    }
    writeln!(output, "Diagnostics:").expect("writing to a String cannot fail");
    if inspection.diagnostics().is_empty() {
        writeln!(output, "  (none)").expect("writing to a String cannot fail");
    } else {
        for diagnostic in inspection.diagnostics() {
            write!(
                output,
                "  - {}/{} at {}: {}",
                serialized_name(&diagnostic.severity())?,
                serialized_name(&diagnostic.code())?,
                diagnostic.scope(),
                quote(diagnostic.message())?
            )
            .expect("writing to a String cannot fail");
            if diagnostic.scope_was_truncated() {
                write!(output, " [scope truncated]").expect("writing to a String cannot fail");
            }
            if diagnostic.message_was_truncated() {
                write!(output, " [message truncated]").expect("writing to a String cannot fail");
            }
            writeln!(output).expect("writing to a String cannot fail");
        }
    }
    Ok(output.into_bytes())
}

fn quote(value: &str) -> Result<String, serde_json::Error> {
    serde_json::to_string(value)
}

fn serialized_name(value: &impl Serialize) -> Result<String, serde_json::Error> {
    let quoted = serde_json::to_string(value)?;
    Ok(quoted
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(&quoted)
        .to_owned())
}

const fn plural<'a>(count: u64, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 { singular } else { plural }
}

fn human_duration(nanoseconds: u64) -> String {
    const NANOS_PER_SECOND: u64 = 1_000_000_000;
    let seconds = nanoseconds / NANOS_PER_SECOND;
    let remainder = nanoseconds % NANOS_PER_SECOND;
    if remainder == 0 {
        return format!("{seconds} s");
    }

    let mut fraction = format!("{remainder:09}");
    while fraction.ends_with('0') {
        fraction.pop();
    }
    format!("{seconds}.{fraction} s")
}

fn source_error_exit(options: &CheckOptions, error: &PrepareError) -> AppExit {
    if options.json() {
        match serde_json::to_vec(&ErrorReport::for_source(error)) {
            Ok(mut bytes) => {
                bytes.push(b'\n');
                if let Err(output_error) = emit_stdout(&bytes) {
                    return output_error_exit(output_error);
                }
            }
            Err(serialization_error) => {
                eprintln!(
                    "spinal check: could not serialize the source error: {serialization_error}"
                );
                return AppExit::from_code(INTERNAL_EXIT_CODE);
            }
        }
    } else {
        eprintln!("spinal check: {error}");
    }
    AppExit::from_code(SOURCE_EXIT_CODE)
}

fn emit_stdout(bytes: &[u8]) -> io::Result<()> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    stdout.write_all(bytes)?;
    stdout.flush()
}

fn output_error_exit(error: io::Error) -> AppExit {
    eprintln!("spinal check: could not write output: {error}");
    AppExit::from_code(INTERNAL_EXIT_CODE)
}

#[derive(Serialize)]
struct ErrorReport {
    format_version: u16,
    status: &'static str,
    error: ErrorDetail,
}

impl ErrorReport {
    fn for_source(error: &PrepareError) -> Self {
        Self {
            format_version: 1,
            status: "error",
            error: ErrorDetail::for_source(error),
        }
    }
}

#[derive(Serialize)]
struct ErrorDetail {
    code: &'static str,
    message: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expected: Option<Box<str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    actual: Option<Box<str>>,
}

impl ErrorDetail {
    fn for_source(error: &PrepareError) -> Self {
        match error {
            PrepareError::UnsupportedSkeletonPath { .. } => Self::fixed(
                "source_path_invalid",
                "the skeleton source path is not a supported JSON export",
            ),
            PrepareError::Io { .. } | PrepareError::PageUnavailable { .. } => Self::fixed(
                "source_unavailable",
                "one or more source files could not be read",
            ),
            PrepareError::NotAFile { .. } => {
                Self::fixed("source_not_file", "a required source path is not a file")
            }
            PrepareError::NotADirectory { .. } | PrepareError::OutsideBundleRoot { .. } => {
                Self::fixed(
                    "bundle_root_invalid",
                    "the selected bundle root is not a usable containing directory",
                )
            }
            PrepareError::MissingAtlas { .. } => Self::fixed(
                "atlas_missing",
                "no unambiguous text atlas was available for the skeleton export",
            ),
            PrepareError::AmbiguousAtlas { .. } => Self::fixed(
                "atlas_ambiguous",
                "more than one text atlas could match the skeleton export",
            ),
            PrepareError::InvalidExport { source, .. } => Self::with_reason(
                "export_invalid",
                "Spinal could not load the Spine JSON and atlas",
                load_error_reason(source.kind()),
            ),
            PrepareError::InvalidRuntimeBundle { source, .. } => Self::for_runtime_bundle(source),
            PrepareError::WrongSpineVersion { expected, actual } => {
                Self::version_mismatch(expected, actual)
            }
            PrepareError::DisallowedPageReference { .. } => Self::fixed(
                "atlas_page_reference_invalid",
                "an atlas page reference is not safe inside the bundle root",
            ),
            PrepareError::InvalidAssetPath { .. } => Self::fixed(
                "asset_path_invalid",
                "a source path cannot be represented as a portable bundle path",
            ),
            PrepareError::EncodedSourceFileTooLarge { .. } => Self::fixed(
                "source_file_too_large",
                "one runtime source file exceeds Spinal's fixed encoded size limit",
            ),
            PrepareError::EncodedBundleTooLarge { .. } => Self::fixed(
                "bundle_too_large",
                "the encoded runtime bundle exceeds Spinal's fixed size limit",
            ),
            PrepareError::TooManyBundleFiles { .. } => Self::fixed(
                "bundle_file_count_exceeded",
                "the runtime bundle exceeds Spinal's fixed file-count limit",
            ),
        }
    }

    const fn fixed(code: &'static str, message: &'static str) -> Self {
        Self {
            code,
            message,
            reason: None,
            expected: None,
            actual: None,
        }
    }

    const fn with_reason(code: &'static str, message: &'static str, reason: &'static str) -> Self {
        Self {
            code,
            message,
            reason: Some(reason),
            expected: None,
            actual: None,
        }
    }

    fn version_mismatch(expected: &str, actual: &str) -> Self {
        Self {
            code: "spine_version_mismatch",
            message: "the export does not target Spinal's required Spine editor version",
            reason: None,
            expected: Some(bounded_error_value(expected)),
            actual: Some(bounded_error_value(actual)),
        }
    }

    fn for_runtime_bundle(error: &RuntimeBundleError) -> Self {
        match error {
            RuntimeBundleError::InvalidTexture { .. } => Self::fixed(
                "texture_invalid",
                "a runtime texture is not a valid image in Spinal's fixed profile",
            ),
            RuntimeBundleError::DecodedTextureBudgetExceeded => Self::fixed(
                "texture_budget_exceeded",
                "decoded runtime textures exceed Spinal's fixed memory budget",
            ),
            RuntimeBundleError::InvalidExport(source) => Self::with_reason(
                "export_invalid",
                "Spinal could not load the Spine JSON and atlas",
                load_error_reason(source.kind()),
            ),
            RuntimeBundleError::WrongSpineVersion { expected, actual } => {
                Self::version_mismatch(expected, actual)
            }
            RuntimeBundleError::InvalidPageReference { .. } => Self::fixed(
                "atlas_page_reference_invalid",
                "an atlas page reference is not safe inside the runtime bundle",
            ),
            RuntimeBundleError::FileLengthMismatch { .. }
            | RuntimeBundleError::FileDigestMismatch(_)
            | RuntimeBundleError::FileSetMismatch
            | RuntimeBundleError::RuntimeFileSetMismatch => Self::with_reason(
                "bundle_integrity_invalid",
                "runtime bundle contents do not match their validated declaration",
                runtime_bundle_reason(error),
            ),
            _ => Self::with_reason(
                "runtime_bundle_invalid",
                "the source bundle did not satisfy Spinal's runtime-bundle contract",
                runtime_bundle_reason(error),
            ),
        }
    }
}

fn load_error_reason(kind: LoadErrorKind) -> &'static str {
    match kind {
        LoadErrorKind::InvalidUtf8 => "invalid_utf8",
        LoadErrorKind::Syntax => "syntax",
        LoadErrorKind::SchemaViolation => "schema_violation",
        LoadErrorKind::InvalidVersion => "invalid_version",
        LoadErrorKind::UnsupportedVersion => "unsupported_version",
        LoadErrorKind::NonFiniteNumber => "non_finite_number",
        LoadErrorKind::DuplicateField => "duplicate_field",
        LoadErrorKind::DuplicateName => "duplicate_name",
        LoadErrorKind::InvalidOrder => "invalid_order",
        LoadErrorKind::InvalidTopology => "invalid_topology",
        LoadErrorKind::UnresolvedReference => "unresolved_reference",
        LoadErrorKind::MissingAtlasRegion => "missing_atlas_region",
        LoadErrorKind::AmbiguousAtlasRegion => "ambiguous_atlas_region",
        LoadErrorKind::UnsupportedData => "unsupported_data",
        LoadErrorKind::CapacityExceeded => "capacity_exceeded",
        _ => "other",
    }
}

fn runtime_bundle_reason(error: &RuntimeBundleError) -> &'static str {
    match error {
        RuntimeBundleError::InvalidManifest(_) => "invalid_manifest",
        RuntimeBundleError::DuplicatePath(_) => "duplicate_path",
        RuntimeBundleError::DuplicateLocation(_) => "duplicate_location",
        RuntimeBundleError::MissingDeclaredFile(_) => "missing_declared_file",
        RuntimeBundleError::UnsafeInputPath(_) => "unsafe_input_path",
        RuntimeBundleError::FileSetMismatch => "file_set_mismatch",
        RuntimeBundleError::FileLengthMismatch { .. } => "file_length_mismatch",
        RuntimeBundleError::FileDigestMismatch(_) => "file_digest_mismatch",
        RuntimeBundleError::InvalidTexture { .. } => "invalid_texture",
        RuntimeBundleError::DecodedTextureBudgetExceeded => "decoded_texture_budget_exceeded",
        RuntimeBundleError::InvalidExport(_) => "invalid_export",
        RuntimeBundleError::WrongSpineVersion { .. } => "wrong_spine_version",
        RuntimeBundleError::InvalidPageReference { .. } => "invalid_page_reference",
        RuntimeBundleError::DuplicateDependencyPath(_) => "duplicate_dependency_path",
        RuntimeBundleError::RuntimeFileSetMismatch => "runtime_file_set_mismatch",
    }
}

fn bounded_error_value(value: &str) -> Box<str> {
    if value.len() <= MAX_ERROR_VALUE_BYTES {
        return value.into();
    }
    let mut end = MAX_ERROR_VALUE_BYTES;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].into()
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CheckOptions {
    json_path: PathBuf,
    atlas_path: Option<PathBuf>,
    bundle_root: Option<PathBuf>,
    json: bool,
}

impl CheckOptions {
    fn parse<I, A>(arguments: I) -> Result<CheckParseResult, CheckOptionsError>
    where
        I: IntoIterator<Item = A>,
        A: Into<OsString>,
    {
        let mut arguments = arguments.into_iter().map(Into::into);
        let mut json_path = None;
        let mut atlas_path = None;
        let mut bundle_root = None;
        let mut json = false;
        let mut options_ended = false;

        while let Some(argument) = arguments.next() {
            if !options_ended {
                match argument.to_str() {
                    Some("-h" | "--help") => return Ok(CheckParseResult::Help),
                    Some("--") => {
                        options_ended = true;
                        continue;
                    }
                    Some("--json") => {
                        if json {
                            return Err(CheckOptionsError::DuplicateOption("--json"));
                        }
                        json = true;
                        continue;
                    }
                    Some("--atlas") => {
                        set_path_option(
                            &mut atlas_path,
                            "--atlas",
                            next_value(&mut arguments, "--atlas")?,
                        )?;
                        continue;
                    }
                    Some("--bundle-root") => {
                        set_path_option(
                            &mut bundle_root,
                            "--bundle-root",
                            next_value(&mut arguments, "--bundle-root")?,
                        )?;
                        continue;
                    }
                    _ => {}
                }

                if let Some(value) = strip_os_prefix(&argument, "--atlas=") {
                    set_path_option(&mut atlas_path, "--atlas", value)?;
                    continue;
                }
                if let Some(value) = strip_os_prefix(&argument, "--bundle-root=") {
                    set_path_option(&mut bundle_root, "--bundle-root", value)?;
                    continue;
                }
                if starts_with_hyphen(&argument) {
                    return Err(CheckOptionsError::UnknownOption(argument));
                }
            }

            if json_path.is_some() {
                return Err(CheckOptionsError::UnexpectedJsonPath(PathBuf::from(
                    argument,
                )));
            }
            json_path = Some(PathBuf::from(argument));
        }

        let json_path = json_path.ok_or(CheckOptionsError::MissingJsonPath)?;
        Ok(CheckParseResult::Run(Self {
            json_path,
            atlas_path,
            bundle_root,
            json,
        }))
    }

    fn json_path(&self) -> &Path {
        &self.json_path
    }

    fn atlas_path(&self) -> Option<&Path> {
        self.atlas_path.as_deref()
    }

    fn bundle_root(&self) -> Option<&Path> {
        self.bundle_root.as_deref()
    }

    const fn json(&self) -> bool {
        self.json
    }
}

fn next_value(
    arguments: &mut impl Iterator<Item = OsString>,
    option: &'static str,
) -> Result<OsString, CheckOptionsError> {
    let value = arguments
        .next()
        .ok_or(CheckOptionsError::MissingValue(option))?;
    if starts_with_hyphen(&value) {
        Err(CheckOptionsError::MissingValue(option))
    } else {
        Ok(value)
    }
}

fn set_path_option(
    destination: &mut Option<PathBuf>,
    option: &'static str,
    value: OsString,
) -> Result<(), CheckOptionsError> {
    if destination.is_some() {
        return Err(CheckOptionsError::DuplicateOption(option));
    }
    if value.is_empty() {
        return Err(CheckOptionsError::EmptyValue(option));
    }
    *destination = Some(PathBuf::from(value));
    Ok(())
}

fn starts_with_hyphen(value: &OsStr) -> bool {
    value.to_string_lossy().starts_with('-')
}

#[cfg(unix)]
fn strip_os_prefix(value: &OsStr, prefix: &str) -> Option<OsString> {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    value
        .as_bytes()
        .strip_prefix(prefix.as_bytes())
        .map(|suffix| OsString::from_vec(suffix.to_vec()))
}

#[cfg(windows)]
fn strip_os_prefix(value: &OsStr, prefix: &str) -> Option<OsString> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    let value = value.encode_wide().collect::<Vec<_>>();
    let prefix = prefix.encode_utf16().collect::<Vec<_>>();
    value
        .strip_prefix(prefix.as_slice())
        .map(OsString::from_wide)
}

#[cfg(not(any(unix, windows)))]
fn strip_os_prefix(value: &OsStr, prefix: &str) -> Option<OsString> {
    value.to_str()?.strip_prefix(prefix).map(Into::into)
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CheckParseResult {
    Run(CheckOptions),
    Help,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CheckOptionsError {
    MissingJsonPath,
    UnexpectedJsonPath(PathBuf),
    UnknownOption(OsString),
    MissingValue(&'static str),
    EmptyValue(&'static str),
    DuplicateOption(&'static str),
}

impl fmt::Display for CheckOptionsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingJsonPath => formatter.write_str("a skeleton JSON path is required"),
            Self::UnexpectedJsonPath(path) => {
                write!(formatter, "unexpected positional path `{}`", path.display())
            }
            Self::UnknownOption(option) => {
                write!(formatter, "unknown option `{}`", option.to_string_lossy())
            }
            Self::MissingValue(option) => write!(formatter, "{option} requires a value"),
            Self::EmptyValue(option) => write!(formatter, "{option} requires a non-empty value"),
            Self::DuplicateOption(option) => {
                write!(formatter, "{option} may only be supplied once")
            }
        }
    }
}

impl Error for CheckOptionsError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(arguments: &[&str]) -> CheckOptions {
        let CheckParseResult::Run(options) =
            CheckOptions::parse(arguments.iter().map(ToString::to_string))
                .expect("arguments should parse")
        else {
            panic!("expected runnable check options");
        };
        options
    }

    #[test]
    fn parses_the_minimal_single_source_contract() {
        let options = parsed(&["rig.json"]);
        assert_eq!(options.json_path(), Path::new("rig.json"));
        assert_eq!(options.atlas_path(), None);
        assert_eq!(options.bundle_root(), None);
        assert!(!options.json());
    }

    #[test]
    fn parses_explicit_paths_and_compact_json() {
        let options = parsed(&[
            "--atlas=atlases/rig.atlas",
            "--bundle-root",
            "package",
            "--json",
            "skeletons/rig.json",
        ]);
        assert_eq!(options.atlas_path(), Some(Path::new("atlases/rig.atlas")));
        assert_eq!(options.bundle_root(), Some(Path::new("package")));
        assert!(options.json());
    }

    #[test]
    fn end_of_options_supports_a_hyphen_prefixed_json_path() {
        let options = parsed(&["--json", "--", "-rig.json"]);
        assert_eq!(options.json_path(), Path::new("-rig.json"));
        assert!(options.json());
    }

    #[cfg(unix)]
    #[test]
    fn equals_path_options_preserve_non_utf8_values() {
        use std::os::unix::ffi::OsStringExt;

        let atlas = OsString::from_vec(b"--atlas=rig\xff.atlas".to_vec());
        let root = OsString::from_vec(b"--bundle-root=package\xfe".to_vec());
        let CheckParseResult::Run(options) =
            CheckOptions::parse([atlas, root, OsString::from("rig.json")])
                .expect("non-UTF-8 path values should remain source arguments")
        else {
            panic!("expected runnable check options");
        };

        assert_eq!(
            options.atlas_path(),
            Some(Path::new(&OsString::from_vec(b"rig\xff.atlas".to_vec())))
        );
        assert_eq!(
            options.bundle_root(),
            Some(Path::new(&OsString::from_vec(b"package\xfe".to_vec())))
        );
    }

    #[test]
    fn help_is_a_distinct_success_path() {
        assert_eq!(
            CheckOptions::parse(["--help".to_owned()]),
            Ok(CheckParseResult::Help)
        );
    }

    #[test]
    fn rejects_preview_and_comparison_options() {
        for option in ["--fps=24", "--compare=other.json", "--compare-atlas=x"] {
            assert!(matches!(
                CheckOptions::parse([option.to_owned(), "rig.json".to_owned()]),
                Err(CheckOptionsError::UnknownOption(ref actual)) if actual == OsStr::new(option)
            ));
        }
    }

    #[test]
    fn rejects_missing_duplicate_empty_and_extra_arguments() {
        assert_eq!(
            CheckOptions::parse(Vec::<String>::new()),
            Err(CheckOptionsError::MissingJsonPath)
        );
        assert_eq!(
            CheckOptions::parse([
                "--json".to_owned(),
                "--json".to_owned(),
                "rig.json".to_owned()
            ]),
            Err(CheckOptionsError::DuplicateOption("--json"))
        );
        assert_eq!(
            CheckOptions::parse(["--atlas=".to_owned(), "rig.json".to_owned()]),
            Err(CheckOptionsError::EmptyValue("--atlas"))
        );
        assert_eq!(
            CheckOptions::parse(["--atlas".to_owned()]),
            Err(CheckOptionsError::MissingValue("--atlas"))
        );
        assert_eq!(
            CheckOptions::parse([
                "--atlas".to_owned(),
                "--json".to_owned(),
                "rig.json".to_owned()
            ]),
            Err(CheckOptionsError::MissingValue("--atlas"))
        );
        assert_eq!(
            CheckOptions::parse(["rig.json".to_owned(), "other.json".to_owned()]),
            Err(CheckOptionsError::UnexpectedJsonPath(PathBuf::from(
                "other.json"
            )))
        );
    }

    #[test]
    fn error_json_is_versioned_stable_and_path_independent() {
        let error = PrepareError::UnsupportedSkeletonPath {
            path: PathBuf::from("/machine-specific/rig.skel"),
        };

        let bytes = serde_json::to_vec(&ErrorReport::for_source(&error))
            .expect("the error schema serializes");

        assert_eq!(
            bytes,
            br#"{"format_version":1,"status":"error","error":{"code":"source_path_invalid","message":"the skeleton source path is not a supported JSON export"}}"#
        );
    }

    #[test]
    fn version_error_json_has_bounded_actionable_details_without_paths() {
        let actual = "é".repeat(MAX_ERROR_VALUE_BYTES);
        let error = PrepareError::WrongSpineVersion {
            expected: "4.3.23",
            actual: actual.into(),
        };

        let bytes = serde_json::to_vec(&ErrorReport::for_source(&error))
            .expect("the error schema serializes");
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).expect("error JSON should parse");

        assert_eq!(value["error"]["code"], "spine_version_mismatch");
        assert_eq!(value["error"]["expected"], "4.3.23");
        assert!(
            value["error"]["actual"]
                .as_str()
                .expect("actual is a string")
                .len()
                <= MAX_ERROR_VALUE_BYTES
        );
        assert!(!String::from_utf8_lossy(&bytes).contains("machine-specific"));
    }

    #[test]
    fn per_file_limit_error_has_a_stable_path_free_code() {
        let error = PrepareError::EncodedSourceFileTooLarge {
            role: "text atlas",
            path: PathBuf::from("/machine-specific/oversized.atlas"),
            limit: 2 * 1024 * 1024,
        };

        let bytes = serde_json::to_vec(&ErrorReport::for_source(&error))
            .expect("the error schema serializes");

        assert_eq!(
            bytes,
            br#"{"format_version":1,"status":"error","error":{"code":"source_file_too_large","message":"one runtime source file exceeds Spinal's fixed encoded size limit"}}"#
        );
    }

    #[test]
    fn degraded_is_the_only_nonfatal_nonzero_outcome() {
        assert_eq!(
            outcome_exit(InspectionOutcome::Compatible),
            AppExit::Success
        );
        assert_eq!(
            outcome_exit(InspectionOutcome::Degraded),
            AppExit::from_code(DEGRADED_EXIT_CODE)
        );
    }

    #[test]
    fn human_units_are_readable_and_exact() {
        assert_eq!(plural(0, "item", "items"), "items");
        assert_eq!(plural(1, "item", "items"), "item");
        assert_eq!(plural(2, "item", "items"), "items");
        assert_eq!(human_duration(0), "0 s");
        assert_eq!(human_duration(1_000_000_000), "1 s");
        assert_eq!(human_duration(1_250_000_000), "1.25 s");
        assert_eq!(human_duration(42), "0.000000042 s");
    }
}

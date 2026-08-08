use crate::digest::{is_sha256, sha256_bytes};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

const CASE_FORMAT_VERSION: u32 = 2;
const TARGET_SPINE_VERSION: &str = "4.3.23";
const APPROVED_VOLATILE_POINTERS: &[&str] = &["/skeleton/hash"];

/// A validated Phase 0A case and the digest of its source TOML bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedCase {
    manifest: CaseManifest,
    source_bytes: Vec<u8>,
    source_sha256: String,
}

/// Complete generic input contract for one Phase 0A evidence run.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaseManifest {
    /// Schema version. This implementation accepts only version `2`.
    pub format_version: u32,
    /// Stable, generic slug used to identify the evidence case.
    pub case_id: String,
    /// Required editor version. This implementation accepts only `4.3.23`.
    pub target_spine_version: String,
    /// Expected editor executable identity.
    pub editor: EditorExpectation,
    /// Complete package roots used by the evidence run.
    pub packages: PackageSet,
    /// Atlas in the current package used by all native validation targets.
    pub runtime_atlas: PathBuf,
    /// Exact skeleton names; no fallback guessing is permitted.
    pub skeletons: SkeletonNames,
    /// Existing and new animations exercised independently.
    pub animations: AnimationNames,
    /// Fixed JSON export policy.
    pub export: ExportPolicy,
    /// Narrow volatile-field policy.
    pub volatile: VolatilePolicy,
}

/// Expected identity of the installed editor executable.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EditorExpectation {
    /// Lowercase SHA-256 expected for the selected executable.
    pub expected_executable_sha256: String,
}

/// Full package contexts for the current project and two independent imports.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageSet {
    /// Current full project package from which candidates are copied.
    pub current: PackageSpec,
    /// Full package containing a replacement for an existing animation.
    pub replacement_submission: PackageSpec,
    /// Full package containing a previously absent animation.
    pub new_submission: PackageSpec,
}

/// One complete package and its required package-relative paths.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageSpec {
    /// Absolute path to the package directory. Packages remain outside Git.
    pub root: PathBuf,
    /// Safe package-relative path to the single source `.spine` project.
    pub project: PathBuf,
    /// Directories that must exist, including required empty directories.
    pub required_directories: Vec<PathBuf>,
    /// Asset directories included in structural and source-mutation evidence.
    pub asset_roots: Vec<PathBuf>,
}

/// Exact skeleton names used for current and submission projects.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkeletonNames {
    /// Destination skeleton in the current project.
    pub current: String,
    /// Source skeleton in the existing-animation submission.
    pub replacement_submission: String,
    /// Source skeleton in the new-animation submission.
    pub new_submission: String,
}

/// The two whole-animation import cases required by Phase 0A.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationNames {
    /// Animation that already exists and must be replaced explicitly.
    pub replacement: String,
    /// Animation that must be imported without replacement mode.
    pub new: String,
}

/// JSON export settings accepted by the evidence harness.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExportPolicy {
    /// Approved, fixed export preset.
    pub preset: ExportPreset,
}

/// Fixed export presets understood by the evidence harness.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ExportPreset {
    /// Pretty JSON with nonessential data included.
    #[serde(rename = "pretty-nonessential-json")]
    PrettyNonessentialJson,
}

/// Volatile JSON fields accepted when comparing round trips.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VolatilePolicy {
    /// Exact JSON pointers approved by checked-in policy.
    pub approved_json_pointers: Vec<String>,
}

/// Errors produced while reading or validating a Phase 0A case.
#[derive(Debug, Error)]
pub enum CaseError {
    /// The manifest could not be read.
    #[error("failed to read Phase 0A case `{path}`: {source}")]
    Read {
        /// Manifest path that could not be read.
        path: PathBuf,
        /// Underlying filesystem failure.
        #[source]
        source: std::io::Error,
    },
    /// TOML did not match the strict schema.
    #[error("failed to parse Phase 0A case: {0}")]
    Parse(#[from] toml::de::Error),
    /// A parsed value violated fixed evidence policy.
    #[error("invalid Phase 0A case: {0}")]
    Invalid(String),
}

/// Reads, hashes, strictly parses, and validates a case manifest.
pub fn load_case(path: impl AsRef<Path>) -> Result<LoadedCase, CaseError> {
    let path = path.as_ref();
    let bytes = fs::read(path).map_err(|source| CaseError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|error| CaseError::Invalid(format!("manifest is not UTF-8: {error}")))?;
    parse_case(text)
}

/// Strictly parses, validates, and hashes a case manifest from TOML text.
pub fn parse_case(text: &str) -> Result<LoadedCase, CaseError> {
    let manifest: CaseManifest = toml::from_str(text)?;
    manifest.validate()?;
    Ok(LoadedCase {
        manifest,
        source_bytes: text.as_bytes().to_vec(),
        source_sha256: sha256_bytes(text.as_bytes()),
    })
}

impl LoadedCase {
    /// Returns the immutable, validated case manifest.
    pub fn manifest(&self) -> &CaseManifest {
        &self.manifest
    }

    /// Returns the lowercase SHA-256 bound to the exact source TOML bytes.
    pub fn source_sha256(&self) -> &str {
        &self.source_sha256
    }

    /// Returns the exact validated TOML bytes bound to this case.
    pub fn source_bytes(&self) -> &[u8] {
        &self.source_bytes
    }
}

impl CaseManifest {
    fn validate(&self) -> Result<(), CaseError> {
        if self.format_version != CASE_FORMAT_VERSION {
            return invalid(format!(
                "format_version must be {CASE_FORMAT_VERSION}, got {}",
                self.format_version
            ));
        }
        if self.target_spine_version != TARGET_SPINE_VERSION {
            return invalid(format!(
                "target_spine_version must be {TARGET_SPINE_VERSION}, got `{}`",
                self.target_spine_version
            ));
        }
        validate_case_id(&self.case_id)?;
        if !is_sha256(&self.editor.expected_executable_sha256) {
            return invalid("editor.expected_executable_sha256 must be 64 lowercase hex digits");
        }

        validate_package("packages.current", &self.packages.current)?;
        validate_package(
            "packages.replacement_submission",
            &self.packages.replacement_submission,
        )?;
        validate_package("packages.new_submission", &self.packages.new_submission)?;
        validate_relative_path("runtime_atlas", &self.runtime_atlas)?;
        if self
            .runtime_atlas
            .extension()
            .and_then(|value| value.to_str())
            != Some("atlas")
        {
            return invalid("runtime_atlas must end in `.atlas`");
        }

        validate_skeleton_name("skeletons.current", &self.skeletons.current)?;
        validate_skeleton_name(
            "skeletons.replacement_submission",
            &self.skeletons.replacement_submission,
        )?;
        validate_skeleton_name("skeletons.new_submission", &self.skeletons.new_submission)?;
        validate_name("animations.replacement", &self.animations.replacement)?;
        validate_name("animations.new", &self.animations.new)?;
        if self.animations.replacement == self.animations.new {
            return invalid("replacement and new animation names must be different");
        }

        let approved = APPROVED_VOLATILE_POINTERS
            .iter()
            .map(|pointer| (*pointer).to_owned())
            .collect::<Vec<_>>();
        if self.volatile.approved_json_pointers != approved {
            return invalid(format!(
                "volatile.approved_json_pointers must be exactly {approved:?}"
            ));
        }
        Ok(())
    }
}

fn validate_case_id(value: &str) -> Result<(), CaseError> {
    let bytes = value.as_bytes();
    let endpoints_are_alphanumeric = bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric);
    if bytes.is_empty()
        || bytes.len() > 64
        || !endpoints_are_alphanumeric
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_".contains(byte))
    {
        return invalid(
            "case_id must be a 1-64 character lowercase ASCII slug with alphanumeric endpoints",
        );
    }
    Ok(())
}

fn validate_package(label: &str, package: &PackageSpec) -> Result<(), CaseError> {
    if !package.root.is_absolute() {
        return invalid(format!("{label}.root must be an absolute directory path"));
    }
    validate_relative_path(&format!("{label}.project"), &package.project)?;
    if package.project.extension().and_then(|value| value.to_str()) != Some("spine") {
        return invalid(format!("{label}.project must end in `.spine`"));
    }
    if package.required_directories.is_empty() {
        return invalid(format!("{label}.required_directories must not be empty"));
    }
    if package.asset_roots.is_empty() {
        return invalid(format!("{label}.asset_roots must not be empty"));
    }

    let required = validate_unique_paths(
        &format!("{label}.required_directories"),
        &package.required_directories,
    )?;
    let assets = validate_unique_paths(&format!("{label}.asset_roots"), &package.asset_roots)?;
    for asset in assets {
        if !required.contains(&asset) {
            return invalid(format!(
                "{label}.asset_roots entry `{asset}` must also appear in required_directories"
            ));
        }
    }
    Ok(())
}

fn validate_unique_paths(label: &str, paths: &[PathBuf]) -> Result<BTreeSet<String>, CaseError> {
    let mut unique = BTreeSet::new();
    for path in paths {
        validate_relative_path(label, path)?;
        let portable = path
            .to_str()
            .ok_or_else(|| CaseError::Invalid(format!("{label} must be valid UTF-8")))?
            .replace(std::path::MAIN_SEPARATOR, "/");
        if !unique.insert(portable.clone()) {
            return invalid(format!("{label} contains duplicate path `{portable}`"));
        }
    }
    Ok(unique)
}

fn validate_relative_path(label: &str, path: &Path) -> Result<(), CaseError> {
    let text = path
        .to_str()
        .ok_or_else(|| CaseError::Invalid(format!("{label} must be valid UTF-8")))?;
    if text.is_empty() || text.contains('\\') || text.contains('\0') {
        return invalid(format!(
            "{label} must be a nonempty portable path using `/` separators"
        ));
    }
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return invalid(format!(
            "{label} must be package-relative and may not contain `.` or `..`"
        ));
    }
    Ok(())
}

fn validate_name(label: &str, value: &str) -> Result<(), CaseError> {
    if value.starts_with('-') {
        return invalid(format!(
            "{label} may not begin with `-` because it is passed as a command argument"
        ));
    }
    if value.is_empty() || value.trim() != value || value.chars().any(char::is_control) {
        return invalid(format!(
            "{label} must be a nonempty exact name without surrounding whitespace or control characters"
        ));
    }
    Ok(())
}

fn validate_skeleton_name(label: &str, value: &str) -> Result<(), CaseError> {
    validate_name(label, value)?;
    crate::spine_cli::validate_json_export_skeleton_name(value).map_err(|_| {
        CaseError::Invalid(format!(
            "{label} must also be a portable single filename component for JSON export"
        ))
    })
}

fn invalid<T>(message: impl Into<String>) -> Result<T, CaseError> {
    Err(CaseError::Invalid(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn valid_case() -> String {
        r#"
format_version = 2
case_id = "generic-import-case"
target_spine_version = "4.3.23"
runtime_atlas = "character.atlas"

[editor]
expected_executable_sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[packages.current]
root = "/external/current"
project = "character.spine"
required_directories = ["images"]
asset_roots = ["images"]

[packages.replacement_submission]
root = "/external/replacement"
project = "character.spine"
required_directories = ["images"]
asset_roots = ["images"]

[packages.new_submission]
root = "/external/new"
project = "character.spine"
required_directories = ["images"]
asset_roots = ["images"]

[skeletons]
current = "Character"
replacement_submission = "Character"
new_submission = "Character"

[animations]
replacement = "idle"
new = "gesture"

[export]
preset = "pretty-nonessential-json"

[volatile]
approved_json_pointers = ["/skeleton/hash"]
"#
        .to_owned()
    }

    #[test]
    fn parses_the_fixed_generic_schema() {
        let parsed = parse_case(&valid_case()).expect("valid case should parse");
        assert_eq!(parsed.manifest().case_id, "generic-import-case");
        assert_eq!(
            parsed.manifest().export.preset,
            ExportPreset::PrettyNonessentialJson
        );
    }

    #[test]
    fn rejects_unknown_fields() {
        let text = valid_case().replace(
            "format_version = 2",
            "format_version = 2\nunreviewed_escape_hatch = true",
        );
        assert!(matches!(parse_case(&text), Err(CaseError::Parse(_))));
    }

    #[test]
    fn rejects_unsafe_relative_paths() {
        let text = valid_case().replace(
            "project = \"character.spine\"",
            "project = \"../character.spine\"",
        );
        let error = parse_case(&text).expect_err("parent traversal must fail");
        assert!(error.to_string().contains("package-relative"));
    }

    #[test]
    fn rejects_policy_weakening() {
        let wrong_version = valid_case().replace("4.3.23", "4.3.24");
        assert!(
            parse_case(&wrong_version)
                .expect_err("wrong version must fail")
                .to_string()
                .contains("target_spine_version")
        );

        let extra_volatile = valid_case().replace(
            "[\"/skeleton/hash\"]",
            "[\"/skeleton/hash\", \"/skeleton/other\"]",
        );
        assert!(
            parse_case(&extra_volatile)
                .expect_err("expanded volatile policy must fail")
                .to_string()
                .contains("must be exactly")
        );
    }

    #[test]
    fn rejects_dash_leading_skeleton_and_animation_names() {
        let skeleton = valid_case().replace("current = \"Character\"", "current = \"-Character\"");
        assert!(
            parse_case(&skeleton)
                .expect_err("dash-leading skeleton must fail")
                .to_string()
                .contains("may not begin with `-`")
        );

        let animation = valid_case().replace("replacement = \"idle\"", "replacement = \"-idle\"");
        assert!(
            parse_case(&animation)
                .expect_err("dash-leading animation must fail")
                .to_string()
                .contains("may not begin with `-`")
        );
    }

    #[test]
    fn rejects_skeleton_names_that_cannot_be_portable_export_filenames() {
        for name in ["Rig/Child", "Rig\\Child", "Rig:", "CON", "trailing."] {
            let text =
                valid_case().replace("current = \"Character\"", &format!("current = {name:?}"));
            assert!(
                parse_case(&text)
                    .expect_err("unsafe export filename must fail case validation")
                    .to_string()
                    .contains("portable single filename component")
            );
        }
    }

    #[test]
    fn hashes_the_unmodified_case_bytes() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("case.toml");
        let text = valid_case();
        fs::write(&path, &text).expect("write case");

        let loaded = load_case(&path).expect("load case");
        assert_eq!(loaded.source_bytes(), text.as_bytes());
        assert_eq!(loaded.source_sha256(), sha256_bytes(text.as_bytes()));
    }
}

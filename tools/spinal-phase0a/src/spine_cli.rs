use crate::digest::sha256_bytes;
use crate::process::{ProcessRequest, TranscriptPolicy};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

const TARGET_VERSION: &str = "4.3.23";
const MAX_RETAINED_BYTES: usize = 4 * 1024 * 1024;
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(30);
const APPROVED_EXPORT_PRESET: &[u8] =
    include_bytes!("../policy/spine-4.3.23-pretty-nonessential.export.json");

/// Returns the exact checked-in Spine 4.3.23 diagnostic export preset.
///
/// Coordinators may materialize these bytes inside a private run directory;
/// typed export execution verifies that the file still matches this policy.
pub const fn approved_export_preset_bytes() -> &'static [u8] {
    APPROVED_EXPORT_PRESET
}

/// Closed set of Spine 4.3.23 operations used by the Phase 0A gate.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpineOperationKind {
    /// Print exact editor and activation information.
    Version,
    /// Print the advanced argument contract used by the gate.
    AdvancedHelp,
    /// Export one staged project to diagnostic JSON.
    ExportJson,
    /// Prove that an omitted required `./images` directory is diagnosed.
    MissingImagesPathControl,
    /// Reconstruct a disposable project from exported JSON.
    ReconstructJson,
    /// Replace one existing whole animation in a staged project.
    ImportExistingAnimation,
    /// Add one new whole animation to a staged project.
    ImportNewAnimation,
    /// Print project and skeleton inventory information.
    ProjectInfo,
}

impl SpineOperationKind {
    fn operation_name(self) -> &'static str {
        match self {
            Self::Version => "spine-version",
            Self::AdvancedHelp => "spine-advanced-help",
            Self::ExportJson => "spine-export-json",
            Self::MissingImagesPathControl => "spine-missing-images-path-control",
            Self::ReconstructJson => "spine-reconstruct-json",
            Self::ImportExistingAnimation => "spine-import-existing-animation",
            Self::ImportNewAnimation => "spine-import-new-animation",
            Self::ProjectInfo => "spine-project-info",
        }
    }

    fn timeout(self) -> Duration {
        match self {
            Self::Version | Self::AdvancedHelp => Duration::from_secs(2 * 60),
            Self::ProjectInfo => Duration::from_secs(5 * 60),
            Self::ExportJson
            | Self::MissingImagesPathControl
            | Self::ReconstructJson
            | Self::ImportExistingAnimation
            | Self::ImportNewAnimation => Duration::from_secs(30 * 60),
        }
    }
}

/// Whether a successful editor call must create or update one exact file.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputMode {
    /// The output must not exist before the call and must be a file afterward.
    CreatedFile,
    /// The output must exist before the call and remain a file afterward.
    UpdatedFile,
}

/// One exact filesystem output discovered after an editor operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpectedOutput {
    id: String,
    path: PathBuf,
    mode: OutputMode,
}

/// One exact policy file whose bytes are fixed by the typed command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExpectedPolicyInput {
    id: String,
    path: PathBuf,
    expected_sha256: String,
}

impl ExpectedPolicyInput {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn expected_sha256(&self) -> &str {
        &self.expected_sha256
    }
}

impl ExpectedOutput {
    /// Returns the stable identifier recorded in process evidence.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the exact absolute output path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the required before-and-after filesystem state.
    pub fn mode(&self) -> OutputMode {
        self.mode
    }
}

/// A validated shell-free invocation pinned to Spine 4.3.23.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpineCommand {
    kind: SpineOperationKind,
    args: Vec<String>,
    expected_outputs: Vec<ExpectedOutput>,
    expected_policy_inputs: Vec<ExpectedPolicyInput>,
}

impl SpineCommand {
    /// Constructs the exact-version activation and version probe.
    pub fn version() -> Self {
        Self::without_paths(SpineOperationKind::Version, ["--version"])
    }

    /// Constructs the exact-version advanced-capability probe.
    pub fn advanced_help() -> Self {
        Self::without_paths(SpineOperationKind::AdvancedHelp, ["--advanced"])
    }

    /// Constructs a deterministic diagnostic JSON export.
    pub fn export_json(
        project: impl AsRef<Path>,
        output_json: impl AsRef<Path>,
        settings_json: impl AsRef<Path>,
    ) -> Result<Self, SpineCommandError> {
        let project = absolute_file_path("project", project, "spine")?;
        let output_json = absolute_file_path("output JSON", output_json, "json")?;
        let settings_json = absolute_file_path("export settings", settings_json, "json")?;
        let output_directory = Path::new(&output_json)
            .parent()
            .expect("an absolute output file always has a parent")
            .to_str()
            .expect("the validated output path is UTF-8")
            .to_owned();
        reject_aliases(&[
            ("project", &project),
            ("output JSON", &output_json),
            ("export settings", &settings_json),
        ])?;
        let mut command = Self::with_args_and_output(
            SpineOperationKind::ExportJson,
            vec![
                "--input".to_owned(),
                project,
                "--output".to_owned(),
                output_directory,
                "--export".to_owned(),
                settings_json.clone(),
            ],
            "export-json",
            output_json,
            OutputMode::CreatedFile,
        );
        command.expected_policy_inputs.push(ExpectedPolicyInput {
            id: "approved-export-preset".to_owned(),
            path: PathBuf::from(settings_json),
            expected_sha256: sha256_bytes(APPROVED_EXPORT_PRESET),
        });
        Ok(command)
    }

    /// Constructs the fixed missing-`./images` negative-control export.
    ///
    /// The staged package must intentionally omit its declared `images`
    /// directory. The exact diagnostic is assessed separately from the zero
    /// exit status and from whether Spine still produced diagnostic JSON.
    pub fn missing_images_path_control(
        project: impl AsRef<Path>,
        output_json: impl AsRef<Path>,
        settings_json: impl AsRef<Path>,
    ) -> Result<Self, SpineCommandError> {
        let mut command = Self::export_json(project, output_json, settings_json)?;
        command.kind = SpineOperationKind::MissingImagesPathControl;
        Ok(command)
    }

    /// Constructs a disposable project reconstruction from diagnostic JSON.
    pub fn reconstruct_json(
        input_json: impl AsRef<Path>,
        output_project: impl AsRef<Path>,
        skeleton_name: &str,
    ) -> Result<Self, SpineCommandError> {
        let input_json = absolute_file_path("input JSON", input_json, "json")?;
        let output_project = absolute_file_path("output project", output_project, "spine")?;
        let skeleton_name = exact_name("skeleton name", skeleton_name)?;
        reject_aliases(&[
            ("input JSON", &input_json),
            ("output project", &output_project),
        ])?;
        Ok(Self::with_args_and_output(
            SpineOperationKind::ReconstructJson,
            vec![
                "--input".to_owned(),
                input_json,
                "--output".to_owned(),
                output_project.clone(),
                "--to".to_owned(),
                skeleton_name,
                "--import".to_owned(),
            ],
            "reconstructed-project",
            output_project,
            OutputMode::CreatedFile,
        ))
    }

    /// Constructs replacement of exactly one existing whole animation.
    pub fn import_existing_animation(
        source_project: impl AsRef<Path>,
        destination_project: impl AsRef<Path>,
        source_skeleton: &str,
        destination_skeleton: &str,
        animation: &str,
    ) -> Result<Self, SpineCommandError> {
        Self::import_animation(
            SpineOperationKind::ImportExistingAnimation,
            source_project,
            destination_project,
            source_skeleton,
            destination_skeleton,
            animation,
            true,
        )
    }

    /// Constructs addition of exactly one new whole animation.
    pub fn import_new_animation(
        source_project: impl AsRef<Path>,
        destination_project: impl AsRef<Path>,
        source_skeleton: &str,
        destination_skeleton: &str,
        animation: &str,
    ) -> Result<Self, SpineCommandError> {
        Self::import_animation(
            SpineOperationKind::ImportNewAnimation,
            source_project,
            destination_project,
            source_skeleton,
            destination_skeleton,
            animation,
            false,
        )
    }

    /// Constructs a project/skeleton inventory probe.
    pub fn project_info(project: impl AsRef<Path>) -> Result<Self, SpineCommandError> {
        let project = absolute_file_path("project", project, "spine")?;
        Ok(Self::with_args(
            SpineOperationKind::ProjectInfo,
            vec!["--input".to_owned(), project],
        ))
    }

    /// Returns the closed operation kind.
    pub fn kind(&self) -> SpineOperationKind {
        self.kind
    }

    /// Returns the exact argument vector, including the fixed version pin.
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// Returns exact filesystem outputs that must be discovered after success.
    pub fn expected_outputs(&self) -> &[ExpectedOutput] {
        &self.expected_outputs
    }

    pub(crate) fn expected_policy_inputs(&self) -> &[ExpectedPolicyInput] {
        &self.expected_policy_inputs
    }

    /// Returns the closed transcript contract for this operation.
    pub const fn transcript_policy(&self) -> TranscriptPolicy {
        match self.kind {
            SpineOperationKind::Version => TranscriptPolicy::spine_4_3_23_version(),
            SpineOperationKind::AdvancedHelp => TranscriptPolicy::spine_4_3_23_advanced_help(),
            SpineOperationKind::ExportJson => TranscriptPolicy::spine_4_3_23_json_export(),
            SpineOperationKind::MissingImagesPathControl => {
                TranscriptPolicy::spine_4_3_23_missing_images_path_control()
            }
            SpineOperationKind::ReconstructJson => TranscriptPolicy::spine_4_3_23_project_import(),
            SpineOperationKind::ImportExistingAnimation
            | SpineOperationKind::ImportNewAnimation => {
                TranscriptPolicy::spine_4_3_23_animation_import()
            }
            SpineOperationKind::ProjectInfo => TranscriptPolicy::spine_4_3_23_project_info(),
        }
    }

    /// Converts this command into the fixed resource-bounded process contract.
    pub fn process_request(
        &self,
        program: impl AsRef<Path>,
        working_directory: impl AsRef<Path>,
        environment: BTreeMap<String, String>,
    ) -> Result<ProcessRequest, SpineCommandError> {
        let program = absolute_path("Spine executable", program)?;
        let working_directory = absolute_path("working directory", working_directory)?;
        Ok(ProcessRequest {
            operation: self.kind.operation_name().to_owned(),
            program,
            args: self.args.clone(),
            working_directory: PathBuf::from(working_directory),
            environment,
            timeout: self.kind.timeout(),
            cleanup_timeout: CLEANUP_TIMEOUT,
            max_retained_bytes_per_stream: MAX_RETAINED_BYTES,
            required_outputs: self
                .expected_outputs
                .iter()
                .map(|output| output.id.clone())
                .collect::<BTreeSet<_>>(),
        })
    }

    fn without_paths<const N: usize>(kind: SpineOperationKind, args: [&str; N]) -> Self {
        Self::with_args(kind, args.into_iter().map(str::to_owned).collect())
    }

    fn with_args(kind: SpineOperationKind, operation_args: Vec<String>) -> Self {
        let mut args = common_args();
        args.extend(operation_args);
        Self {
            kind,
            args,
            expected_outputs: Vec::new(),
            expected_policy_inputs: Vec::new(),
        }
    }

    fn with_args_and_output(
        kind: SpineOperationKind,
        operation_args: Vec<String>,
        id: &str,
        path: String,
        mode: OutputMode,
    ) -> Self {
        let mut command = Self::with_args(kind, operation_args);
        command.expected_outputs.push(ExpectedOutput {
            id: id.to_owned(),
            path: PathBuf::from(path),
            mode,
        });
        command
    }

    #[allow(clippy::too_many_arguments)]
    fn import_animation(
        kind: SpineOperationKind,
        source_project: impl AsRef<Path>,
        destination_project: impl AsRef<Path>,
        source_skeleton: &str,
        destination_skeleton: &str,
        animation: &str,
        replace: bool,
    ) -> Result<Self, SpineCommandError> {
        let source_project = absolute_file_path("source project", source_project, "spine")?;
        let destination_project =
            absolute_file_path("destination project", destination_project, "spine")?;
        let source_skeleton = exact_name("source skeleton", source_skeleton)?;
        let destination_skeleton = exact_name("destination skeleton", destination_skeleton)?;
        let animation = exact_name("animation", animation)?;
        reject_aliases(&[
            ("source project", &source_project),
            ("destination project", &destination_project),
        ])?;

        let mut args = vec![
            "--input".to_owned(),
            source_project,
            "--output".to_owned(),
            destination_project.clone(),
            "--from".to_owned(),
            source_skeleton,
            "--to".to_owned(),
            destination_skeleton,
            "--animation".to_owned(),
            animation,
        ];
        if replace {
            args.push("--replace".to_owned());
        }
        args.push("--import".to_owned());
        Ok(Self::with_args_and_output(
            kind,
            args,
            "destination-project",
            destination_project,
            OutputMode::UpdatedFile,
        ))
    }
}

/// Invalid input to the closed Spine command contract.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SpineCommandError {
    /// A path was not absolute, normalized, UTF-8, or NUL-free.
    #[error("{label} must be an absolute normalized UTF-8 path without NUL bytes")]
    InvalidPath {
        /// Name of the invalid command path.
        label: &'static str,
    },
    /// A typed file path used the wrong extension.
    #[error("{label} must end in `.{expected_extension}`")]
    WrongExtension {
        /// Name of the invalid command path.
        label: &'static str,
        /// Required lowercase extension.
        expected_extension: &'static str,
    },
    /// Two operation paths referred to the same lexical path.
    #[error("{left} and {right} must be different paths")]
    AliasedPaths {
        /// First path role.
        left: &'static str,
        /// Second path role.
        right: &'static str,
    },
    /// A skeleton or animation name was unsafe or ambiguous as an argument.
    #[error("{label} must be a nonempty trimmed name without controls or a leading `-`")]
    InvalidName {
        /// Name of the invalid command argument.
        label: &'static str,
    },
}

fn common_args() -> Vec<String> {
    [
        "--update",
        TARGET_VERSION,
        "--hide-license",
        "--disable-audio",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn absolute_file_path(
    label: &'static str,
    path: impl AsRef<Path>,
    extension: &'static str,
) -> Result<String, SpineCommandError> {
    let path = absolute_path(label, path)?;
    if Path::new(&path)
        .extension()
        .and_then(|value| value.to_str())
        != Some(extension)
    {
        return Err(SpineCommandError::WrongExtension {
            label,
            expected_extension: extension,
        });
    }
    Ok(path)
}

fn absolute_path(label: &'static str, path: impl AsRef<Path>) -> Result<String, SpineCommandError> {
    let path = path.as_ref();
    let valid_components = path.is_absolute()
        && path.components().all(|component| {
            matches!(
                component,
                Component::Prefix(_) | Component::RootDir | Component::Normal(_)
            )
        });
    let Some(text) = path.to_str() else {
        return Err(SpineCommandError::InvalidPath { label });
    };
    if !valid_components || text.contains('\0') {
        return Err(SpineCommandError::InvalidPath { label });
    }
    Ok(text.to_owned())
}

fn exact_name(label: &'static str, value: &str) -> Result<String, SpineCommandError> {
    if value.is_empty()
        || value.trim() != value
        || value.starts_with('-')
        || value.chars().any(char::is_control)
    {
        return Err(SpineCommandError::InvalidName { label });
    }
    Ok(value.to_owned())
}

fn reject_aliases(paths: &[(&'static str, &String)]) -> Result<(), SpineCommandError> {
    for (index, (left_label, left)) in paths.iter().enumerate() {
        for (right_label, right) in &paths[index + 1..] {
            if left.as_str() == right.as_str() {
                return Err(SpineCommandError::AliasedPaths {
                    left: left_label,
                    right: right_label,
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(command: &SpineCommand) -> Vec<&str> {
        command.args().iter().map(String::as_str).collect()
    }

    #[test]
    fn every_operation_uses_the_exact_version_and_privacy_prefix() {
        let commands = [
            SpineCommand::version(),
            SpineCommand::advanced_help(),
            SpineCommand::project_info("/staged/current/character.spine").expect("info"),
            SpineCommand::export_json(
                "/staged/current/character.spine",
                "/staged/output/character.json",
                "/staged/preset/export.json",
            )
            .expect("export"),
            SpineCommand::missing_images_path_control(
                "/staged/negative/character.spine",
                "/staged/negative-output/character.json",
                "/staged/preset/export.json",
            )
            .expect("negative control"),
            SpineCommand::reconstruct_json(
                "/staged/output/character.json",
                "/staged/reconstructed/character.spine",
                "Character",
            )
            .expect("reconstruct"),
            SpineCommand::import_existing_animation(
                "/staged/submission/character.spine",
                "/staged/candidate/character.spine",
                "Source",
                "Destination",
                "idle",
            )
            .expect("replace"),
            SpineCommand::import_new_animation(
                "/staged/submission/character.spine",
                "/staged/candidate/character.spine",
                "Source",
                "Destination",
                "gesture",
            )
            .expect("new"),
        ];
        for command in commands {
            assert_eq!(
                &arguments(&command)[..4],
                ["--update", "4.3.23", "--hide-license", "--disable-audio"]
            );
            assert!(!command.args().iter().any(|argument| argument == "4.3.xx"));
        }
    }

    #[test]
    fn replacement_is_scoped_to_one_exact_animation() {
        let replacement = SpineCommand::import_existing_animation(
            "/staged/submission/character.spine",
            "/staged/candidate/character.spine",
            "Source Rig",
            "Destination Rig",
            "idle loop",
        )
        .expect("replacement");
        assert_eq!(
            arguments(&replacement),
            [
                "--update",
                "4.3.23",
                "--hide-license",
                "--disable-audio",
                "--input",
                "/staged/submission/character.spine",
                "--output",
                "/staged/candidate/character.spine",
                "--from",
                "Source Rig",
                "--to",
                "Destination Rig",
                "--animation",
                "idle loop",
                "--replace",
                "--import",
            ]
        );
        assert_eq!(
            replacement.expected_outputs()[0].mode(),
            OutputMode::UpdatedFile
        );

        let new = SpineCommand::import_new_animation(
            "/staged/submission/character.spine",
            "/staged/candidate/character.spine",
            "Source Rig",
            "Destination Rig",
            "gesture",
        )
        .expect("new animation");
        assert!(!new.args().iter().any(|argument| argument == "--replace"));
        assert_eq!(
            new.args()
                .iter()
                .filter(|argument| argument.as_str() == "--animation")
                .count(),
            1
        );
    }

    #[test]
    fn unsafe_paths_names_and_aliases_are_rejected_before_process_construction() {
        assert!(matches!(
            SpineCommand::project_info("relative.spine"),
            Err(SpineCommandError::InvalidPath { .. })
        ));
        assert!(matches!(
            SpineCommand::reconstruct_json(
                "/staged/source.json",
                "/staged/output.spine",
                "-fallback"
            ),
            Err(SpineCommandError::InvalidName { .. })
        ));
        assert!(matches!(
            SpineCommand::export_json(
                "/staged/same.spine",
                "/staged/same.spine",
                "/staged/export.json"
            ),
            Err(SpineCommandError::WrongExtension { .. })
                | Err(SpineCommandError::AliasedPaths { .. })
        ));
        assert!(matches!(
            SpineCommand::export_json(
                "/staged/current/../current/character.spine",
                "/staged/output/character.json",
                "/staged/export.json"
            ),
            Err(SpineCommandError::InvalidPath { .. })
        ));
    }

    #[test]
    fn process_request_binds_fixed_limits_and_typed_output_ids() {
        let command = SpineCommand::export_json(
            "/staged/current/character.spine",
            "/staged/output/character.json",
            "/staged/preset/export.json",
        )
        .expect("export");
        let request = command
            .process_request(
                "/Applications/Spine.app/Contents/MacOS/Spine",
                "/staged/current",
                BTreeMap::from([("LANG".to_owned(), "C".to_owned())]),
            )
            .expect("request");
        assert_eq!(request.operation, "spine-export-json");
        assert_eq!(request.timeout, Duration::from_secs(30 * 60));
        assert_eq!(request.cleanup_timeout, Duration::from_secs(30));
        assert_eq!(request.max_retained_bytes_per_stream, 4 * 1024 * 1024);
        assert_eq!(
            arguments(&command),
            [
                "--update",
                "4.3.23",
                "--hide-license",
                "--disable-audio",
                "--input",
                "/staged/current/character.spine",
                "--output",
                "/staged/output",
                "--export",
                "/staged/preset/export.json",
            ]
        );
        assert_eq!(
            request.required_outputs,
            BTreeSet::from(["export-json".to_owned()])
        );
    }

    #[test]
    fn missing_images_control_is_a_distinct_policy_bound_operation() {
        let command = SpineCommand::missing_images_path_control(
            "/staged/negative/character.spine",
            "/staged/output/character.json",
            "/staged/preset/export.json",
        )
        .expect("negative control");
        let request = command
            .process_request("/evidence/editor", "/staged", BTreeMap::new())
            .expect("request");
        assert_eq!(command.kind(), SpineOperationKind::MissingImagesPathControl);
        assert_eq!(request.operation, "spine-missing-images-path-control");
        assert_eq!(
            command.transcript_policy().profile(),
            crate::TranscriptProfile::MissingImagesPathControl
        );
        assert_eq!(command.expected_policy_inputs().len(), 1);
        assert_eq!(
            command.expected_policy_inputs()[0].expected_sha256(),
            sha256_bytes(approved_export_preset_bytes())
        );
        assert_eq!(command.expected_outputs().len(), 1);
    }

    #[test]
    fn probe_commands_select_structured_transcript_contracts() {
        assert_eq!(
            SpineCommand::version().transcript_policy(),
            TranscriptPolicy::spine_4_3_23_version()
        );
        assert_eq!(
            SpineCommand::advanced_help().transcript_policy(),
            TranscriptPolicy::spine_4_3_23_advanced_help()
        );
        assert_eq!(
            SpineCommand::project_info("/staged/current/character.spine")
                .expect("info")
                .transcript_policy(),
            TranscriptPolicy::spine_4_3_23_project_info()
        );
    }
}

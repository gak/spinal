//! Closed operation order and cross-operation bindings for Phase 0A.

use crate::case::LoadedCase;
use crate::process::{ExecutableIdentity, ProcessFailureCode, TranscriptProfile};
use crate::spine_cli::{
    JsonExportTarget, OutputMode, SpineCommand, SpineCommandError, SpineOperationKind,
};
use crate::spine_run::SpineRunEvidence;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

const OPERATION_COUNT: usize = 22;

/// The one accepted Phase 0A operation sequence.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum OperationId {
    Version,
    AdvancedHelp,
    InfoCurrent,
    InfoReplacement,
    InfoNew,
    ExportCurrentA,
    ExportReplacementSubmission,
    ExportNewSubmission,
    ReconstructA,
    ExportReconstructedA,
    ExportCurrentB,
    ReconstructB,
    ExportReconstructedB,
    ImportExistingFirst,
    ExportExistingFirst,
    ImportExistingRepeat,
    ExportExistingRepeat,
    ImportNewFirst,
    ExportNewFirst,
    ImportNewCollisionControl,
    ExportNewCollisionControl,
    MissingImagesPathControl,
}

impl OperationId {
    pub(crate) const ORDER: [Self; OPERATION_COUNT] = [
        Self::Version,
        Self::AdvancedHelp,
        Self::InfoCurrent,
        Self::InfoReplacement,
        Self::InfoNew,
        Self::ExportCurrentA,
        Self::ExportReplacementSubmission,
        Self::ExportNewSubmission,
        Self::ReconstructA,
        Self::ExportReconstructedA,
        Self::ExportCurrentB,
        Self::ReconstructB,
        Self::ExportReconstructedB,
        Self::ImportExistingFirst,
        Self::ExportExistingFirst,
        Self::ImportExistingRepeat,
        Self::ExportExistingRepeat,
        Self::ImportNewFirst,
        Self::ExportNewFirst,
        Self::ImportNewCollisionControl,
        Self::ExportNewCollisionControl,
        Self::MissingImagesPathControl,
    ];

    const fn index(self) -> usize {
        match self {
            Self::Version => 0,
            Self::AdvancedHelp => 1,
            Self::InfoCurrent => 2,
            Self::InfoReplacement => 3,
            Self::InfoNew => 4,
            Self::ExportCurrentA => 5,
            Self::ExportReplacementSubmission => 6,
            Self::ExportNewSubmission => 7,
            Self::ReconstructA => 8,
            Self::ExportReconstructedA => 9,
            Self::ExportCurrentB => 10,
            Self::ReconstructB => 11,
            Self::ExportReconstructedB => 12,
            Self::ImportExistingFirst => 13,
            Self::ExportExistingFirst => 14,
            Self::ImportExistingRepeat => 15,
            Self::ExportExistingRepeat => 16,
            Self::ImportNewFirst => 17,
            Self::ExportNewFirst => 18,
            Self::ImportNewCollisionControl => 19,
            Self::ExportNewCollisionControl => 20,
            Self::MissingImagesPathControl => 21,
        }
    }
}

/// Exact commands and workspace bindings derived only from a validated case.
pub(crate) struct OperationRecipe {
    root: PathBuf,
    expected_executable_sha256: String,
    entries: Vec<RecipeEntry>,
}

struct RecipeEntry {
    id: OperationId,
    command: SpineCommand,
}

impl OperationRecipe {
    /// Builds the fixed recipe beneath one normalized absolute workspace root.
    pub(crate) fn new(case: &LoadedCase, root: impl AsRef<Path>) -> Result<Self, RecipeError> {
        let root = validate_root(root.as_ref())?;
        let manifest = case.manifest();
        let current = root
            .join("packages/current")
            .join(&manifest.packages.current.project);
        let replacement = root
            .join("packages/replacement-submission")
            .join(&manifest.packages.replacement_submission.project);
        let new_submission = root
            .join("packages/new-submission")
            .join(&manifest.packages.new_submission.project);
        let new_collision_control = root
            .join("packages/new-collision-control")
            .join(&manifest.packages.new_submission.project);
        let negative = root
            .join("packages/missing-images-control")
            .join(&manifest.packages.current.project);
        let preset = root.join("policy/pretty-nonessential.export.json");

        let current_a = json_target(
            &root,
            "outputs/round-trip/a/source",
            &manifest.skeletons.current,
        )?;
        // Reconstructed projects must live beside the staged current package's
        // root-level `images` directory. Spine resolves the export preset's
        // `./images` path from the project context, so an output-only directory
        // would turn the follow-up export into another missing-images case.
        let reconstructed_a = root.join("packages/current/phase0a-round-trip-a.spine");
        let reconstructed_a_json = json_target(
            &root,
            "outputs/round-trip/a/reconstructed-json",
            &manifest.skeletons.current,
        )?;
        let current_b = json_target(
            &root,
            "outputs/round-trip/b/source",
            &manifest.skeletons.current,
        )?;
        let reconstructed_b = root.join("packages/current/phase0a-round-trip-b.spine");
        let reconstructed_b_json = json_target(
            &root,
            "outputs/round-trip/b/reconstructed-json",
            &manifest.skeletons.current,
        )?;
        let replacement_json = json_target(
            &root,
            "outputs/submissions/replacement",
            &manifest.skeletons.replacement_submission,
        )?;
        let new_submission_json = json_target(
            &root,
            "outputs/submissions/new",
            &manifest.skeletons.new_submission,
        )?;

        let existing_candidate = root
            .join("packages/existing-candidate")
            .join(&manifest.packages.current.project);
        let existing_first_json = json_target(
            &root,
            "outputs/candidates/existing/first",
            &manifest.skeletons.current,
        )?;
        let existing_repeat_json = json_target(
            &root,
            "outputs/candidates/existing/repeat",
            &manifest.skeletons.current,
        )?;
        let new_candidate = root
            .join("packages/new-candidate")
            .join(&manifest.packages.current.project);
        let new_first_json = json_target(
            &root,
            "outputs/candidates/new/first",
            &manifest.skeletons.current,
        )?;
        let new_collision_control_json = json_target(
            &root,
            "outputs/candidates/new/collision-control",
            &manifest.skeletons.new_submission,
        )?;
        let negative_json = json_target(
            &root,
            "outputs/negative-control",
            &manifest.skeletons.current,
        )?;

        let commands = vec![
            SpineCommand::version(),
            SpineCommand::advanced_help(),
            SpineCommand::project_info(&current)?,
            SpineCommand::project_info(&replacement)?,
            SpineCommand::project_info(&new_submission)?,
            SpineCommand::export_json(&current, &current_a, &preset)?,
            SpineCommand::export_json(&replacement, &replacement_json, &preset)?,
            SpineCommand::export_json(&new_submission, &new_submission_json, &preset)?,
            SpineCommand::reconstruct_json(
                current_a.output_json(),
                &reconstructed_a,
                &manifest.skeletons.current,
            )?,
            SpineCommand::export_json(&reconstructed_a, &reconstructed_a_json, &preset)?,
            SpineCommand::export_json(&current, &current_b, &preset)?,
            SpineCommand::reconstruct_json(
                current_b.output_json(),
                &reconstructed_b,
                &manifest.skeletons.current,
            )?,
            SpineCommand::export_json(&reconstructed_b, &reconstructed_b_json, &preset)?,
            SpineCommand::import_existing_animation(
                &replacement,
                &existing_candidate,
                &manifest.skeletons.replacement_submission,
                &manifest.skeletons.current,
                &manifest.animations.replacement,
            )?,
            SpineCommand::export_json(&existing_candidate, &existing_first_json, &preset)?,
            SpineCommand::import_existing_animation(
                &replacement,
                &existing_candidate,
                &manifest.skeletons.replacement_submission,
                &manifest.skeletons.current,
                &manifest.animations.replacement,
            )?,
            SpineCommand::export_json(&existing_candidate, &existing_repeat_json, &preset)?,
            SpineCommand::import_new_animation(
                &new_submission,
                &new_candidate,
                &manifest.skeletons.new_submission,
                &manifest.skeletons.current,
                &manifest.animations.new,
            )?,
            SpineCommand::export_json(&new_candidate, &new_first_json, &preset)?,
            SpineCommand::new_animation_collision_control(
                &new_submission,
                &new_collision_control,
                &manifest.skeletons.new_submission,
                &manifest.skeletons.new_submission,
                &manifest.animations.new,
            )?,
            SpineCommand::export_json(
                &new_collision_control,
                &new_collision_control_json,
                &preset,
            )?,
            SpineCommand::missing_images_path_control(&negative, &negative_json, &preset)?,
        ];
        let entries = OperationId::ORDER
            .into_iter()
            .zip(commands)
            .map(|(id, command)| RecipeEntry { id, command })
            .collect::<Vec<_>>();
        debug_assert_eq!(entries.len(), OPERATION_COUNT);

        Ok(Self {
            root,
            expected_executable_sha256: manifest.editor.expected_executable_sha256.clone(),
            entries,
        })
    }

    /// Returns the exact typed command for one inventory slot.
    pub(crate) fn command(&self, id: OperationId) -> &SpineCommand {
        let entry = &self.entries[id.index()];
        debug_assert_eq!(entry.id, id);
        &entry.command
    }

    #[cfg(test)]
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the case-pinned editor digest required for every invocation.
    pub(crate) fn expected_executable_sha256(&self) -> &str {
        &self.expected_executable_sha256
    }
}

/// One operation label paired only with facts extracted from execution evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OperationRecord {
    id: OperationId,
    observed: ObservedOperation,
}

impl OperationRecord {
    /// Extracts all validation facts from trusted execution evidence.
    ///
    /// Callers label the ordered slot, but cannot supply pass state, expected
    /// commands, path bindings, transcript profiles, or digests independently.
    pub(crate) fn from_run(id: OperationId, run: &SpineRunEvidence) -> Self {
        let process = run.process();
        Self {
            id,
            observed: ObservedOperation {
                kind: run.operation_kind(),
                process_passed: process.assessment().passed(),
                process_failures: process
                    .assessment()
                    .failures()
                    .iter()
                    .map(|failure| failure.code)
                    .collect(),
                operation: process.operation().to_owned(),
                program: process.program().to_owned(),
                executable_identity: process.executable_identity().clone(),
                args: process.args().to_vec(),
                working_directory: process.working_directory().to_path_buf(),
                timeout: process.timeout(),
                transcript_profile: process.transcript_profile(),
                required_outputs: process.required_outputs().clone(),
                observed_outputs: process.observed_outputs().clone(),
                inputs: run
                    .inputs()
                    .iter()
                    .map(|input| ObservedInput {
                        id: input.id().to_owned(),
                        path: input.path().to_path_buf(),
                        expected_sha256: input.expected_sha256().map(str::to_owned),
                        before_sha256: input.before().sha256().to_owned(),
                        after_sha256: input.after().sha256().to_owned(),
                    })
                    .collect(),
                outputs: run
                    .outputs()
                    .iter()
                    .map(|output| ObservedOutput {
                        id: output.id().to_owned(),
                        path: output.path().to_path_buf(),
                        mode: output.mode(),
                        before_sha256: output.before().map(|value| value.sha256().to_owned()),
                        after_sha256: output.after().map(|value| value.sha256().to_owned()),
                    })
                    .collect(),
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ObservedOperation {
    kind: SpineOperationKind,
    process_passed: bool,
    process_failures: Vec<ProcessFailureCode>,
    operation: String,
    program: String,
    executable_identity: ExecutableIdentity,
    args: Vec<String>,
    working_directory: PathBuf,
    timeout: Duration,
    transcript_profile: TranscriptProfile,
    required_outputs: BTreeSet<String>,
    observed_outputs: BTreeSet<String>,
    inputs: Vec<ObservedInput>,
    outputs: Vec<ObservedOutput>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ObservedInput {
    id: String,
    path: PathBuf,
    expected_sha256: Option<String>,
    before_sha256: String,
    after_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ObservedOutput {
    id: String,
    path: PathBuf,
    mode: OutputMode,
    before_sha256: Option<String>,
    after_sha256: Option<String>,
}

/// Proof that every fixed slot and every cross-operation binding validated.
///
/// There is deliberately no public constructor and no mutable access to the
/// records. Only `validate` can mint this token.
pub(crate) struct CompletedOperationInventory {
    records: Vec<OperationRecord>,
}

impl CompletedOperationInventory {
    pub(crate) fn validate(
        recipe: &OperationRecipe,
        records: Vec<OperationRecord>,
    ) -> Result<Self, RecipeError> {
        validate_slots(recipe, &records)?;
        validate_editor_identity(recipe, &records)?;
        validate_digest_chains(&records)?;
        Ok(Self { records })
    }

    pub(crate) fn records(&self) -> &[OperationRecord] {
        &self.records
    }
}

/// A fail-closed reason why the fixed operation inventory was not completed.
#[derive(Debug, Error)]
pub(crate) enum RecipeError {
    #[error("invalid Phase 0A operation recipe: {0}")]
    InvalidRecipe(String),
    #[error("expected exactly {expected} operation records, observed {actual}")]
    WrongRecordCount { expected: usize, actual: usize },
    #[error("operation slot {index} must be {expected:?}, observed {actual:?}")]
    WrongOrder {
        index: usize,
        expected: OperationId,
        actual: OperationId,
    },
    #[error("operation {id:?} did not pass its process policy")]
    ProcessDidNotPass { id: OperationId },
    #[error("operation {id:?} did not match its exact {field} binding")]
    BindingMismatch {
        id: OperationId,
        field: &'static str,
    },
    #[error("operation {id:?} did not use its dedicated negative-control contract")]
    WrongNegativeControl { id: OperationId },
    #[error("operation digest chain `{chain}` did not match")]
    DigestChainMismatch { chain: &'static str },
    #[error(transparent)]
    Command(#[from] SpineCommandError),
}

fn validate_slots(
    recipe: &OperationRecipe,
    records: &[OperationRecord],
) -> Result<(), RecipeError> {
    if records.len() != OPERATION_COUNT {
        return Err(RecipeError::WrongRecordCount {
            expected: OPERATION_COUNT,
            actual: records.len(),
        });
    }
    let baseline_program = &records[0].observed.program;
    for (index, ((expected_id, record), entry)) in OperationId::ORDER
        .iter()
        .copied()
        .zip(records)
        .zip(&recipe.entries)
        .enumerate()
    {
        if record.id != expected_id {
            return Err(RecipeError::WrongOrder {
                index,
                expected: expected_id,
                actual: record.id,
            });
        }
        debug_assert_eq!(entry.id, expected_id);
        validate_record(
            recipe,
            expected_id,
            &entry.command,
            &record.observed,
            baseline_program,
        )?;
    }
    Ok(())
}

fn validate_record(
    recipe: &OperationRecipe,
    id: OperationId,
    command: &SpineCommand,
    observed: &ObservedOperation,
    baseline_program: &str,
) -> Result<(), RecipeError> {
    if matches!(
        id,
        OperationId::ImportNewCollisionControl | OperationId::MissingImagesPathControl
    ) {
        validate_negative_control_process(id, observed)?;
    } else if !observed.process_passed || !observed.process_failures.is_empty() {
        return Err(RecipeError::ProcessDidNotPass { id });
    }
    if observed.kind != command.kind() {
        return mismatch(id, "operation kind");
    }
    if observed.program != baseline_program || !Path::new(&observed.program).is_absolute() {
        return mismatch(id, "editor program");
    }
    if observed.args != command.args() {
        return mismatch(id, "argument vector");
    }
    if observed.working_directory != recipe.root {
        return mismatch(id, "working directory");
    }
    if observed.transcript_profile != command.transcript_policy().profile() {
        if matches!(
            id,
            OperationId::ImportNewCollisionControl | OperationId::MissingImagesPathControl
        ) {
            return Err(RecipeError::WrongNegativeControl { id });
        }
        return mismatch(id, "transcript profile");
    }

    let expected_request =
        command.process_request(baseline_program, &recipe.root, Default::default())?;
    if observed.operation != expected_request.operation {
        return mismatch(id, "process operation name");
    }
    if observed.timeout != expected_request.timeout {
        return mismatch(id, "timeout");
    }
    if observed.required_outputs != expected_request.required_outputs {
        return mismatch(id, "exact output discovery");
    }
    validate_inputs(id, command, &observed.inputs)?;
    validate_outputs(id, command, &observed.outputs, &observed.observed_outputs)?;

    if id == OperationId::MissingImagesPathControl
        && (observed.kind != SpineOperationKind::MissingImagesPathControl
            || observed.transcript_profile != TranscriptProfile::MissingImagesPathControl)
    {
        return Err(RecipeError::WrongNegativeControl { id });
    }
    if id == OperationId::ImportNewCollisionControl
        && (observed.kind != SpineOperationKind::NewAnimationCollisionControl
            || observed.transcript_profile != TranscriptProfile::NewAnimationCollisionControl)
    {
        return Err(RecipeError::WrongNegativeControl { id });
    }
    Ok(())
}

fn validate_negative_control_process(
    id: OperationId,
    observed: &ObservedOperation,
) -> Result<(), RecipeError> {
    if id == OperationId::ImportNewCollisionControl {
        if observed.process_passed
            || observed.process_failures != [ProcessFailureCode::BlockingDiagnostic]
        {
            return Err(RecipeError::WrongNegativeControl { id });
        }
        return Ok(());
    }
    let exact_diagnostic = [ProcessFailureCode::BlockingDiagnostic];
    let exact_diagnostic_without_output = [
        ProcessFailureCode::BlockingDiagnostic,
        ProcessFailureCode::MissingOutput,
    ];
    if observed.process_passed
        || (observed.process_failures != exact_diagnostic
            && observed.process_failures != exact_diagnostic_without_output)
    {
        return Err(RecipeError::WrongNegativeControl { id });
    }
    Ok(())
}

fn validate_inputs(
    id: OperationId,
    command: &SpineCommand,
    observed: &[ObservedInput],
) -> Result<(), RecipeError> {
    if observed.len() != command.expected_inputs().len() {
        return mismatch(id, "input inventory");
    }
    for (actual, expected) in observed.iter().zip(command.expected_inputs()) {
        if actual.id != expected.id()
            || actual.path != expected.path()
            || actual.expected_sha256.as_deref() != expected.expected_sha256()
            || actual.before_sha256 != actual.after_sha256
            || !valid_sha256(&actual.before_sha256)
            || actual
                .expected_sha256
                .as_ref()
                .is_some_and(|digest| digest != &actual.before_sha256)
        {
            return mismatch(id, "input inventory");
        }
    }
    Ok(())
}

fn validate_outputs(
    id: OperationId,
    command: &SpineCommand,
    observed: &[ObservedOutput],
    observed_output_ids: &BTreeSet<String>,
) -> Result<(), RecipeError> {
    if observed.len() != command.expected_outputs().len() {
        return mismatch(id, "output inventory");
    }
    let mut outputs_present = BTreeSet::new();
    for (actual, expected) in observed.iter().zip(command.expected_outputs()) {
        let before_is_valid = match expected.mode() {
            OutputMode::CreatedFile => actual.before_sha256.is_none(),
            OutputMode::UpdatedFile => actual.before_sha256.as_deref().is_some_and(valid_sha256),
        };
        let after_is_valid = if id == OperationId::MissingImagesPathControl {
            actual.after_sha256.as_deref().is_none_or(valid_sha256)
        } else {
            actual.after_sha256.as_deref().is_some_and(valid_sha256)
        };
        if actual.after_sha256.is_some() {
            outputs_present.insert(actual.id.clone());
        }
        if actual.id != expected.id()
            || actual.path != expected.path()
            || actual.mode != expected.mode()
            || !before_is_valid
            || !after_is_valid
        {
            return mismatch(id, "output inventory");
        }
    }
    if &outputs_present != observed_output_ids {
        return mismatch(id, "exact output discovery");
    }
    Ok(())
}

fn validate_editor_identity(
    recipe: &OperationRecipe,
    records: &[OperationRecord],
) -> Result<(), RecipeError> {
    let first = &records[0].observed;
    for record in records {
        let observed = &record.observed;
        if observed.executable_identity.sha256() != recipe.expected_executable_sha256
            || observed.executable_identity != first.executable_identity
        {
            return mismatch(record.id, "editor executable identity");
        }
    }
    Ok(())
}

fn validate_digest_chains(records: &[OperationRecord]) -> Result<(), RecipeError> {
    let current = input_digest(records, OperationId::InfoCurrent, "project")?;
    same(
        current,
        input_digest(records, OperationId::ExportCurrentA, "project")?,
        "current-info-to-export-a",
    )?;
    same(
        current,
        input_digest(records, OperationId::ExportCurrentB, "project")?,
        "current-info-to-export-b",
    )?;
    same(
        current,
        output_before(
            records,
            OperationId::ImportExistingFirst,
            "destination-project",
        )?,
        "current-to-existing-first-base",
    )?;
    same(
        current,
        output_before(records, OperationId::ImportNewFirst, "destination-project")?,
        "current-to-new-first-base",
    )?;
    same(
        current,
        input_digest(records, OperationId::MissingImagesPathControl, "project")?,
        "current-to-negative-control",
    )?;

    let replacement = input_digest(records, OperationId::InfoReplacement, "project")?;
    same(
        replacement,
        input_digest(records, OperationId::ExportReplacementSubmission, "project")?,
        "replacement-info-to-export",
    )?;
    same(
        replacement,
        input_digest(records, OperationId::ImportExistingFirst, "source-project")?,
        "replacement-to-existing-first",
    )?;
    same(
        replacement,
        input_digest(records, OperationId::ImportExistingRepeat, "source-project")?,
        "replacement-to-existing-repeat",
    )?;

    let new_submission = input_digest(records, OperationId::InfoNew, "project")?;
    same(
        new_submission,
        input_digest(records, OperationId::ExportNewSubmission, "project")?,
        "new-info-to-export",
    )?;
    same(
        new_submission,
        input_digest(records, OperationId::ImportNewFirst, "source-project")?,
        "new-to-first-import",
    )?;
    same(
        new_submission,
        input_digest(
            records,
            OperationId::ImportNewCollisionControl,
            "source-project",
        )?,
        "new-to-collision-control-source",
    )?;
    same(
        new_submission,
        output_before(
            records,
            OperationId::ImportNewCollisionControl,
            "destination-project",
        )?,
        "new-to-collision-control-base",
    )?;

    bind_output_to_input(
        records,
        OperationId::ExportCurrentA,
        "export-json",
        OperationId::ReconstructA,
        "source-json",
        "round-trip-a-json-to-reconstruct",
    )?;
    bind_output_to_input(
        records,
        OperationId::ReconstructA,
        "reconstructed-project",
        OperationId::ExportReconstructedA,
        "project",
        "round-trip-a-project-to-export",
    )?;
    bind_output_to_input(
        records,
        OperationId::ExportCurrentB,
        "export-json",
        OperationId::ReconstructB,
        "source-json",
        "round-trip-b-json-to-reconstruct",
    )?;
    bind_output_to_input(
        records,
        OperationId::ReconstructB,
        "reconstructed-project",
        OperationId::ExportReconstructedB,
        "project",
        "round-trip-b-project-to-export",
    )?;
    bind_output_to_input(
        records,
        OperationId::ImportExistingFirst,
        "destination-project",
        OperationId::ExportExistingFirst,
        "project",
        "existing-first-to-export",
    )?;
    same(
        output_after(
            records,
            OperationId::ImportExistingFirst,
            "destination-project",
        )?,
        output_before(
            records,
            OperationId::ImportExistingRepeat,
            "destination-project",
        )?,
        "existing-first-to-repeat",
    )?;
    same(
        output_after(
            records,
            OperationId::ImportExistingFirst,
            "destination-project",
        )?,
        output_after(
            records,
            OperationId::ImportExistingRepeat,
            "destination-project",
        )?,
        "existing-repeat-idempotence",
    )?;
    bind_output_to_input(
        records,
        OperationId::ImportExistingRepeat,
        "destination-project",
        OperationId::ExportExistingRepeat,
        "project",
        "existing-repeat-to-export",
    )?;
    bind_output_to_input(
        records,
        OperationId::ImportNewFirst,
        "destination-project",
        OperationId::ExportNewFirst,
        "project",
        "new-first-to-export",
    )?;
    different(
        output_before(
            records,
            OperationId::ImportNewCollisionControl,
            "destination-project",
        )?,
        output_after(
            records,
            OperationId::ImportNewCollisionControl,
            "destination-project",
        )?,
        "new-collision-control-mutated",
    )?;
    bind_output_to_input(
        records,
        OperationId::ImportNewCollisionControl,
        "destination-project",
        OperationId::ExportNewCollisionControl,
        "project",
        "new-collision-control-to-export",
    )?;

    same(
        output_after(records, OperationId::ExportCurrentA, "export-json")?,
        output_after(records, OperationId::ExportCurrentB, "export-json")?,
        "repeat-current-export",
    )?;
    same(
        output_after(records, OperationId::ExportExistingFirst, "export-json")?,
        output_after(records, OperationId::ExportExistingRepeat, "export-json")?,
        "repeat-existing-import-export",
    )?;
    Ok(())
}

fn different(left: &str, right: &str, chain: &'static str) -> Result<(), RecipeError> {
    if left != right {
        Ok(())
    } else {
        Err(RecipeError::DigestChainMismatch { chain })
    }
}

fn bind_output_to_input(
    records: &[OperationRecord],
    output_id: OperationId,
    output_role: &str,
    input_id: OperationId,
    input_role: &str,
    chain: &'static str,
) -> Result<(), RecipeError> {
    same(
        output_after(records, output_id, output_role)?,
        input_digest(records, input_id, input_role)?,
        chain,
    )
}

fn record(records: &[OperationRecord], id: OperationId) -> &OperationRecord {
    let record = &records[id.index()];
    debug_assert_eq!(record.id, id);
    record
}

fn input_digest<'a>(
    records: &'a [OperationRecord],
    id: OperationId,
    role: &str,
) -> Result<&'a str, RecipeError> {
    record(records, id)
        .observed
        .inputs
        .iter()
        .find(|input| input.id == role)
        .map(|input| input.before_sha256.as_str())
        .ok_or(RecipeError::BindingMismatch {
            id,
            field: "digest-chain input",
        })
}

fn output_before<'a>(
    records: &'a [OperationRecord],
    id: OperationId,
    role: &str,
) -> Result<&'a str, RecipeError> {
    record(records, id)
        .observed
        .outputs
        .iter()
        .find(|output| output.id == role)
        .and_then(|output| output.before_sha256.as_deref())
        .ok_or(RecipeError::BindingMismatch {
            id,
            field: "digest-chain pre-operation output",
        })
}

fn output_after<'a>(
    records: &'a [OperationRecord],
    id: OperationId,
    role: &str,
) -> Result<&'a str, RecipeError> {
    record(records, id)
        .observed
        .outputs
        .iter()
        .find(|output| output.id == role)
        .and_then(|output| output.after_sha256.as_deref())
        .ok_or(RecipeError::BindingMismatch {
            id,
            field: "digest-chain post-operation output",
        })
}

fn same(left: &str, right: &str, chain: &'static str) -> Result<(), RecipeError> {
    if left == right {
        Ok(())
    } else {
        Err(RecipeError::DigestChainMismatch { chain })
    }
}

fn mismatch<T>(id: OperationId, field: &'static str) -> Result<T, RecipeError> {
    Err(RecipeError::BindingMismatch { id, field })
}

fn validate_root(root: &Path) -> Result<PathBuf, RecipeError> {
    let valid = root.is_absolute()
        && root.components().all(|component| {
            matches!(
                component,
                Component::Prefix(_) | Component::RootDir | Component::Normal(_)
            )
        });
    if !valid || root.to_str().is_none_or(|text| text.contains('\0')) {
        return Err(RecipeError::InvalidRecipe(
            "workspace root must be an absolute normalized UTF-8 path".to_owned(),
        ));
    }
    Ok(root.to_path_buf())
}

fn json_target(
    root: &Path,
    directory: &str,
    skeleton_name: &str,
) -> Result<JsonExportTarget, RecipeError> {
    JsonExportTarget::new(root.join(directory), skeleton_name).map_err(RecipeError::from)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::case::parse_case;
    use crate::digest::sha256_bytes;

    const PROGRAM: &str = "/Applications/Spine.app/Contents/MacOS/Spine";
    const EXECUTABLE_SHA256: &str =
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn case() -> LoadedCase {
        parse_case(
            r#"
format_version = 2
case_id = "operation-recipe-test"
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
project = "replacement.spine"
required_directories = ["images"]
asset_roots = ["images"]

[packages.new_submission]
root = "/external/new"
project = "new.spine"
required_directories = ["images"]
asset_roots = ["images"]

[skeletons]
current = "Character"
replacement_submission = "Replacement"
new_submission = "New"

[animations]
replacement = "idle"
new = "gesture"

[export]
preset = "pretty-nonessential-json"

[volatile]
approved_json_pointers = ["/skeleton/hash"]
"#,
        )
        .expect("valid test case")
    }

    fn recipe() -> OperationRecipe {
        OperationRecipe::new(&case(), "/private/tmp/spinal-operation-recipe").expect("valid recipe")
    }

    fn digest(label: &str) -> String {
        sha256_bytes(label.as_bytes())
    }

    fn expected_record(recipe: &OperationRecipe, id: OperationId) -> OperationRecord {
        let command = recipe.command(id);
        let request = command
            .process_request(PROGRAM, recipe.root(), Default::default())
            .expect("fixed command request");
        let inputs = command
            .expected_inputs()
            .iter()
            .map(|input| {
                let value = input
                    .expected_sha256()
                    .map(str::to_owned)
                    .unwrap_or_else(|| digest(&format!("{id:?}:input:{}", input.id())));
                ObservedInput {
                    id: input.id().to_owned(),
                    path: input.path().to_path_buf(),
                    expected_sha256: input.expected_sha256().map(str::to_owned),
                    before_sha256: value.clone(),
                    after_sha256: value,
                }
            })
            .collect();
        let outputs = command
            .expected_outputs()
            .iter()
            .map(|output| ObservedOutput {
                id: output.id().to_owned(),
                path: output.path().to_path_buf(),
                mode: output.mode(),
                before_sha256: (output.mode() == OutputMode::UpdatedFile)
                    .then(|| digest(&format!("{id:?}:before:{}", output.id()))),
                after_sha256: Some(digest(&format!("{id:?}:after:{}", output.id()))),
            })
            .collect();
        OperationRecord {
            id,
            observed: ObservedOperation {
                kind: command.kind(),
                process_passed: !matches!(
                    id,
                    OperationId::ImportNewCollisionControl | OperationId::MissingImagesPathControl
                ),
                process_failures: if matches!(
                    id,
                    OperationId::ImportNewCollisionControl | OperationId::MissingImagesPathControl
                ) {
                    vec![ProcessFailureCode::BlockingDiagnostic]
                } else {
                    Vec::new()
                },
                operation: request.operation,
                program: PROGRAM.to_owned(),
                executable_identity: executable_identity(1),
                args: command.args().to_vec(),
                working_directory: recipe.root().to_path_buf(),
                timeout: request.timeout,
                transcript_profile: command.transcript_policy().profile(),
                required_outputs: request.required_outputs.clone(),
                observed_outputs: request.required_outputs,
                inputs,
                outputs,
            },
        }
    }

    fn executable_identity(inode: u64) -> ExecutableIdentity {
        ExecutableIdentity::new(
            PathBuf::from(PROGRAM),
            EXECUTABLE_SHA256.to_owned(),
            42,
            1,
            inode,
            0o100755,
            501,
            1,
            2,
            3,
            4,
        )
    }

    fn valid_records(recipe: &OperationRecipe) -> Vec<OperationRecord> {
        let mut records = OperationId::ORDER
            .into_iter()
            .map(|id| expected_record(recipe, id))
            .collect::<Vec<_>>();

        let current = digest("current-project");
        for id in [
            OperationId::InfoCurrent,
            OperationId::ExportCurrentA,
            OperationId::ExportCurrentB,
            OperationId::MissingImagesPathControl,
        ] {
            set_input(&mut records, id, "project", &current);
        }
        for id in [
            OperationId::ImportExistingFirst,
            OperationId::ImportNewFirst,
        ] {
            set_output_before(&mut records, id, "destination-project", &current);
        }

        let replacement = digest("replacement-project");
        for id in [
            OperationId::InfoReplacement,
            OperationId::ExportReplacementSubmission,
            OperationId::ImportExistingFirst,
            OperationId::ImportExistingRepeat,
        ] {
            let role = if matches!(
                id,
                OperationId::ImportExistingFirst | OperationId::ImportExistingRepeat
            ) {
                "source-project"
            } else {
                "project"
            };
            set_input(&mut records, id, role, &replacement);
        }

        let new_submission = digest("new-submission-project");
        for id in [
            OperationId::InfoNew,
            OperationId::ExportNewSubmission,
            OperationId::ImportNewFirst,
            OperationId::ImportNewCollisionControl,
        ] {
            let role = if matches!(
                id,
                OperationId::ImportNewFirst | OperationId::ImportNewCollisionControl
            ) {
                "source-project"
            } else {
                "project"
            };
            set_input(&mut records, id, role, &new_submission);
        }

        let current_json = digest("current-json");
        set_output_after(
            &mut records,
            OperationId::ExportCurrentA,
            "export-json",
            &current_json,
        );
        set_input(
            &mut records,
            OperationId::ReconstructA,
            "source-json",
            &current_json,
        );
        set_output_after(
            &mut records,
            OperationId::ExportCurrentB,
            "export-json",
            &current_json,
        );
        set_input(
            &mut records,
            OperationId::ReconstructB,
            "source-json",
            &current_json,
        );

        let reconstructed_a = digest("reconstructed-a-project");
        set_output_after(
            &mut records,
            OperationId::ReconstructA,
            "reconstructed-project",
            &reconstructed_a,
        );
        set_input(
            &mut records,
            OperationId::ExportReconstructedA,
            "project",
            &reconstructed_a,
        );
        let reconstructed_b = digest("reconstructed-b-project");
        set_output_after(
            &mut records,
            OperationId::ReconstructB,
            "reconstructed-project",
            &reconstructed_b,
        );
        set_input(
            &mut records,
            OperationId::ExportReconstructedB,
            "project",
            &reconstructed_b,
        );

        let existing_candidate = digest("existing-candidate-project");
        for (import, export) in [
            (
                OperationId::ImportExistingFirst,
                OperationId::ExportExistingFirst,
            ),
            (
                OperationId::ImportExistingRepeat,
                OperationId::ExportExistingRepeat,
            ),
        ] {
            set_output_after(
                &mut records,
                import,
                "destination-project",
                &existing_candidate,
            );
            set_input(&mut records, export, "project", &existing_candidate);
        }
        set_output_before(
            &mut records,
            OperationId::ImportExistingRepeat,
            "destination-project",
            &existing_candidate,
        );
        let existing_json = digest("existing-candidate-json");
        set_output_after(
            &mut records,
            OperationId::ExportExistingFirst,
            "export-json",
            &existing_json,
        );
        set_output_after(
            &mut records,
            OperationId::ExportExistingRepeat,
            "export-json",
            &existing_json,
        );

        let new_candidate = digest("new-candidate-project");
        set_output_after(
            &mut records,
            OperationId::ImportNewFirst,
            "destination-project",
            &new_candidate,
        );
        set_input(
            &mut records,
            OperationId::ExportNewFirst,
            "project",
            &new_candidate,
        );
        let collision_candidate = digest("new-collision-control-project");
        set_output_before(
            &mut records,
            OperationId::ImportNewCollisionControl,
            "destination-project",
            &new_submission,
        );
        set_output_after(
            &mut records,
            OperationId::ImportNewCollisionControl,
            "destination-project",
            &collision_candidate,
        );
        set_input(
            &mut records,
            OperationId::ExportNewCollisionControl,
            "project",
            &collision_candidate,
        );
        let new_json = digest("new-candidate-json");
        set_output_after(
            &mut records,
            OperationId::ExportNewFirst,
            "export-json",
            &new_json,
        );
        set_output_after(
            &mut records,
            OperationId::ExportNewCollisionControl,
            "export-json",
            &digest("new-collision-control-json"),
        );
        records
    }

    fn operation(records: &mut [OperationRecord], id: OperationId) -> &mut ObservedOperation {
        &mut records[id.index()].observed
    }

    fn set_input(records: &mut [OperationRecord], id: OperationId, role: &str, value: &str) {
        let input = operation(records, id)
            .inputs
            .iter_mut()
            .find(|input| input.id == role)
            .expect("input role");
        input.before_sha256 = value.to_owned();
        input.after_sha256 = value.to_owned();
    }

    fn set_output_before(
        records: &mut [OperationRecord],
        id: OperationId,
        role: &str,
        value: &str,
    ) {
        operation(records, id)
            .outputs
            .iter_mut()
            .find(|output| output.id == role)
            .expect("output role")
            .before_sha256 = Some(value.to_owned());
    }

    fn set_output_after(records: &mut [OperationRecord], id: OperationId, role: &str, value: &str) {
        operation(records, id)
            .outputs
            .iter_mut()
            .find(|output| output.id == role)
            .expect("output role")
            .after_sha256 = Some(value.to_owned());
    }

    #[test]
    fn exact_inventory_is_the_only_path_to_completion() {
        let recipe = recipe();
        let completed = CompletedOperationInventory::validate(&recipe, valid_records(&recipe))
            .expect("exact inventory");
        assert_eq!(completed.records().len(), OPERATION_COUNT);
    }

    #[test]
    fn equal_launcher_bytes_with_changed_file_identity_are_rejected() {
        let recipe = recipe();
        let mut records = valid_records(&recipe);
        operation(&mut records, OperationId::ExportCurrentA).executable_identity =
            executable_identity(2);
        assert!(matches!(
            CompletedOperationInventory::validate(&recipe, records),
            Err(RecipeError::BindingMismatch {
                id: OperationId::ExportCurrentA,
                ..
            })
        ));
    }

    #[test]
    fn export_paths_are_derived_from_exact_case_skeletons() {
        let recipe = recipe();
        let root = recipe.root();
        let output = |id| recipe.command(id).expected_outputs()[0].path();

        assert_eq!(
            output(OperationId::ExportCurrentA),
            root.join("outputs/round-trip/a/source/Character.json")
        );
        assert_eq!(
            output(OperationId::ExportReplacementSubmission),
            root.join("outputs/submissions/replacement/Replacement.json")
        );
        assert_eq!(
            output(OperationId::ExportNewSubmission),
            root.join("outputs/submissions/new/New.json")
        );
        assert_eq!(
            output(OperationId::ExportReconstructedA),
            root.join("outputs/round-trip/a/reconstructed-json/Character.json")
        );
        assert_eq!(
            output(OperationId::ReconstructA),
            root.join("packages/current/phase0a-round-trip-a.spine")
        );
        assert_eq!(
            output(OperationId::ReconstructB),
            root.join("packages/current/phase0a-round-trip-b.spine")
        );
    }

    #[test]
    fn removing_each_individual_slot_is_rejected() {
        let recipe = recipe();
        let baseline = valid_records(&recipe);
        for index in 0..OPERATION_COUNT {
            let mut records = baseline.clone();
            records.remove(index);
            assert!(
                matches!(
                    CompletedOperationInventory::validate(&recipe, records),
                    Err(RecipeError::WrongRecordCount { .. })
                ),
                "removed slot {index} was accepted"
            );
        }
    }

    #[test]
    fn every_adjacent_swap_is_rejected() {
        let recipe = recipe();
        let baseline = valid_records(&recipe);
        for index in 0..OPERATION_COUNT - 1 {
            let mut records = baseline.clone();
            records.swap(index, index + 1);
            assert!(
                matches!(
                    CompletedOperationInventory::validate(&recipe, records),
                    Err(RecipeError::WrongOrder { .. })
                ),
                "adjacent swap at {index} was accepted"
            );
        }
    }

    #[test]
    fn duplicate_and_extra_records_are_rejected() {
        let recipe = recipe();
        let baseline = valid_records(&recipe);

        let mut duplicate = baseline.clone();
        duplicate[OperationId::InfoReplacement.index()] =
            duplicate[OperationId::InfoCurrent.index()].clone();
        assert!(matches!(
            CompletedOperationInventory::validate(&recipe, duplicate),
            Err(RecipeError::WrongOrder { .. })
        ));

        let mut extra = baseline;
        extra.push(extra[0].clone());
        assert!(matches!(
            CompletedOperationInventory::validate(&recipe, extra),
            Err(RecipeError::WrongRecordCount { .. })
        ));
    }

    #[test]
    fn relabeled_same_kind_exports_do_not_pass() {
        let recipe = recipe();
        for (left, right) in [
            (OperationId::ExportCurrentA, OperationId::ExportCurrentB),
            (
                OperationId::ExportExistingFirst,
                OperationId::ExportExistingRepeat,
            ),
            (
                OperationId::ExportNewFirst,
                OperationId::ExportNewCollisionControl,
            ),
        ] {
            let mut records = valid_records(&recipe);
            let left_observed = records[left.index()].observed.clone();
            records[left.index()].observed = records[right.index()].observed.clone();
            records[right.index()].observed = left_observed;
            assert!(matches!(
                CompletedOperationInventory::validate(&recipe, records),
                Err(RecipeError::BindingMismatch { .. })
            ));
        }
    }

    #[test]
    fn cross_chain_reconstruction_is_rejected_even_when_digest_matches() {
        let recipe = recipe();
        let mut records = valid_records(&recipe);
        let wrong_path = operation(&mut records, OperationId::ReconstructB).inputs[0]
            .path
            .clone();
        let reconstruct_a = operation(&mut records, OperationId::ReconstructA);
        reconstruct_a.inputs[0].path = wrong_path.clone();
        let input_index = reconstruct_a
            .args
            .iter()
            .position(|arg| arg == "--input")
            .expect("input argument")
            + 1;
        reconstruct_a.args[input_index] = wrong_path.into_os_string().into_string().expect("UTF-8");
        assert!(matches!(
            CompletedOperationInventory::validate(&recipe, records),
            Err(RecipeError::BindingMismatch { .. })
        ));
    }

    #[test]
    fn import_source_and_destination_swap_is_rejected() {
        let recipe = recipe();
        let mut records = valid_records(&recipe);
        let import = operation(&mut records, OperationId::ImportExistingFirst);
        let input_arg = import
            .args
            .iter()
            .position(|arg| arg == "--input")
            .expect("input argument")
            + 1;
        let output_arg = import
            .args
            .iter()
            .position(|arg| arg == "--output")
            .expect("output argument")
            + 1;
        import.args.swap(input_arg, output_arg);
        std::mem::swap(&mut import.inputs[0].path, &mut import.outputs[0].path);
        assert!(matches!(
            CompletedOperationInventory::validate(&recipe, records),
            Err(RecipeError::BindingMismatch { .. })
        ));
    }

    #[test]
    fn existing_repeat_slot_cannot_reuse_first_run_evidence() {
        let recipe = recipe();
        for (first, repeat) in [
            (
                OperationId::ImportExistingFirst,
                OperationId::ImportExistingRepeat,
            ),
            (
                OperationId::ExportExistingFirst,
                OperationId::ExportExistingRepeat,
            ),
        ] {
            let mut records = valid_records(&recipe);
            records[repeat.index()].observed = records[first.index()].observed.clone();
            assert!(matches!(
                CompletedOperationInventory::validate(&recipe, records),
                Err(RecipeError::BindingMismatch { .. })
                    | Err(RecipeError::DigestChainMismatch { .. })
            ));
        }
    }

    #[test]
    fn collision_control_cannot_reuse_positive_import_or_export_evidence() {
        let recipe = recipe();

        let mut import_reuse = valid_records(&recipe);
        import_reuse[OperationId::ImportNewCollisionControl.index()].observed = import_reuse
            [OperationId::ImportNewFirst.index()]
        .observed
        .clone();
        assert!(matches!(
            CompletedOperationInventory::validate(&recipe, import_reuse),
            Err(RecipeError::WrongNegativeControl {
                id: OperationId::ImportNewCollisionControl
            })
        ));

        let mut export_reuse = valid_records(&recipe);
        export_reuse[OperationId::ExportNewCollisionControl.index()].observed = export_reuse
            [OperationId::ExportNewFirst.index()]
        .observed
        .clone();
        assert!(matches!(
            CompletedOperationInventory::validate(&recipe, export_reuse),
            Err(RecipeError::BindingMismatch {
                id: OperationId::ExportNewCollisionControl,
                ..
            })
        ));
    }

    #[test]
    fn collision_control_digest_chain_binds_base_mutation_and_export() {
        let recipe = recipe();

        let mut wrong_base = valid_records(&recipe);
        let positive_candidate = output_after(
            &wrong_base,
            OperationId::ImportNewFirst,
            "destination-project",
        )
        .expect("positive candidate digest")
        .to_owned();
        set_output_before(
            &mut wrong_base,
            OperationId::ImportNewCollisionControl,
            "destination-project",
            &positive_candidate,
        );
        assert!(matches!(
            CompletedOperationInventory::validate(&recipe, wrong_base),
            Err(RecipeError::DigestChainMismatch {
                chain: "new-to-collision-control-base"
            })
        ));

        let mut unchanged = valid_records(&recipe);
        let collision_before = output_before(
            &unchanged,
            OperationId::ImportNewCollisionControl,
            "destination-project",
        )
        .expect("collision base digest")
        .to_owned();
        set_output_after(
            &mut unchanged,
            OperationId::ImportNewCollisionControl,
            "destination-project",
            &collision_before,
        );
        set_input(
            &mut unchanged,
            OperationId::ExportNewCollisionControl,
            "project",
            &collision_before,
        );
        assert!(matches!(
            CompletedOperationInventory::validate(&recipe, unchanged),
            Err(RecipeError::DigestChainMismatch {
                chain: "new-collision-control-mutated"
            })
        ));

        let mut unbound_export = valid_records(&recipe);
        set_input(
            &mut unbound_export,
            OperationId::ExportNewCollisionControl,
            "project",
            &digest("unrelated-collision-project"),
        );
        assert!(matches!(
            CompletedOperationInventory::validate(&recipe, unbound_export),
            Err(RecipeError::DigestChainMismatch {
                chain: "new-collision-control-to-export"
            })
        ));
    }

    #[test]
    fn negative_control_cannot_be_relabeled_or_use_a_normal_profile() {
        let recipe = recipe();
        let mut relabeled = valid_records(&recipe);
        relabeled[OperationId::MissingImagesPathControl.index()].id =
            OperationId::ExportNewCollisionControl;
        assert!(matches!(
            CompletedOperationInventory::validate(&recipe, relabeled),
            Err(RecipeError::WrongOrder { .. })
        ));

        let mut wrong_profile = valid_records(&recipe);
        operation(&mut wrong_profile, OperationId::MissingImagesPathControl).transcript_profile =
            TranscriptProfile::JsonExport;
        assert!(matches!(
            CompletedOperationInventory::validate(&recipe, wrong_profile),
            Err(RecipeError::WrongNegativeControl { .. })
        ));

        let mut wrong_kind = valid_records(&recipe);
        operation(&mut wrong_kind, OperationId::MissingImagesPathControl).kind =
            SpineOperationKind::ExportJson;
        assert!(matches!(
            CompletedOperationInventory::validate(&recipe, wrong_kind),
            Err(RecipeError::BindingMismatch { .. })
        ));
    }

    #[test]
    fn negative_control_accepts_only_its_exact_failure_with_optional_missing_output() {
        let recipe = recipe();
        let mut without_output = valid_records(&recipe);
        let negative = operation(&mut without_output, OperationId::MissingImagesPathControl);
        negative.process_failures = vec![
            ProcessFailureCode::BlockingDiagnostic,
            ProcessFailureCode::MissingOutput,
        ];
        negative.observed_outputs.clear();
        negative.outputs[0].after_sha256 = None;
        assert!(CompletedOperationInventory::validate(&recipe, without_output).is_ok());

        let mut extra_failure = valid_records(&recipe);
        operation(&mut extra_failure, OperationId::MissingImagesPathControl)
            .process_failures
            .push(ProcessFailureCode::UnknownTranscriptLine);
        assert!(matches!(
            CompletedOperationInventory::validate(&recipe, extra_failure),
            Err(RecipeError::WrongNegativeControl { .. })
        ));

        let mut unexpected_success = valid_records(&recipe);
        let negative = operation(
            &mut unexpected_success,
            OperationId::MissingImagesPathControl,
        );
        negative.process_passed = true;
        negative.process_failures.clear();
        assert!(matches!(
            CompletedOperationInventory::validate(&recipe, unexpected_success),
            Err(RecipeError::WrongNegativeControl { .. })
        ));
    }

    #[test]
    fn broken_digest_chain_and_wrong_cwd_fail_closed() {
        let recipe = recipe();
        let mut broken_chain = valid_records(&recipe);
        set_input(
            &mut broken_chain,
            OperationId::ExportExistingFirst,
            "project",
            &digest("unrelated-project"),
        );
        assert!(matches!(
            CompletedOperationInventory::validate(&recipe, broken_chain),
            Err(RecipeError::DigestChainMismatch { .. })
        ));

        let mut wrong_cwd = valid_records(&recipe);
        operation(&mut wrong_cwd, OperationId::Version).working_directory =
            PathBuf::from("/private/tmp/other-run");
        assert!(matches!(
            CompletedOperationInventory::validate(&recipe, wrong_cwd),
            Err(RecipeError::BindingMismatch { .. })
        ));
    }
}

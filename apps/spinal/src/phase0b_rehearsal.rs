//! Internal browser-only semantic observation harness for the fixed Phase 0B case.
//!
//! The feature is deliberately non-default. It drives the same viewer session,
//! commands, entities, runtime, and renderer as the ordinary browser app.

#[cfg(test)]
use std::time::Duration;

use bevy_spinal::spinal::SemanticFrame;
use serde::Serialize;
#[cfg(target_arch = "wasm32")]
use spinal_phase0b::contract::{ALTERNATE_SKIN_NAME, ANIMATION_DURATION, ANIMATION_NAME};
use spinal_phase0b::contract::{SAMPLE_SCHEDULE as FIXED_SAMPLES, Sample as FixedSample};

#[cfg(target_arch = "wasm32")]
use crate::bundle::SourceProvenance;

const READY_UPDATE_LIMIT: usize = 1_800;
const SAMPLE_UPDATE_LIMIT: usize = 8;
const MAX_SEMANTIC_FRAME_BYTES: usize = 1024 * 1024;
const MAX_OBSERVATION_BYTES: usize = 8 * MAX_SEMANTIC_FRAME_BYTES + 64 * 1024;
const MAX_ERROR_MESSAGE_BYTES: usize = 4 * 1024;
#[cfg(any(target_arch = "wasm32", test))]
const OUTPUT_ELEMENT_ID: &str = "spinal-phase0b-observation";
#[cfg(target_arch = "wasm32")]
const COMPLETE_ATTRIBUTE: &str = "data-spinal-phase0b-complete";
#[cfg(target_arch = "wasm32")]
const STATE_ATTRIBUTE: &str = "data-spinal-phase0b-state";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CaptureSource {
    Current,
    Proposed,
}

impl CaptureSource {
    const ALL: [Self; 2] = [Self::Current, Self::Proposed];

    const fn index(self) -> usize {
        match self {
            Self::Current => 0,
            Self::Proposed => 1,
        }
    }

    const fn id(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Proposed => "proposed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct IssuedBaseline {
    frame_revision: u64,
    play_revision: u64,
    seek_revision: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CaptureExpectation {
    baseline_frame_revision: u64,
    play_revision: u64,
    seek_revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ObservedSource {
    frame_present: bool,
    frame_revision: u64,
    acknowledged_play_revision: Option<u64>,
    acknowledged_seek_revision: Option<u64>,
    skin_layers: Vec<Box<str>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PendingReason {
    MissingFrame,
    FrameRevisionNotAdvanced {
        baseline: u64,
        observed: u64,
    },
    PlayRevisionMismatch {
        expected: u64,
        observed: Option<u64>,
    },
    SeekRevisionMismatch {
        expected: u64,
        observed: Option<u64>,
    },
    SkinLayersMismatch {
        expected: Vec<Box<str>>,
        observed: Vec<Box<str>>,
    },
}

fn classify_observation(
    expected: CaptureExpectation,
    expected_skin_layers: &[&str],
    observed: &ObservedSource,
) -> Result<(), PendingReason> {
    if !observed.frame_present {
        return Err(PendingReason::MissingFrame);
    }
    if observed.frame_revision <= expected.baseline_frame_revision {
        return Err(PendingReason::FrameRevisionNotAdvanced {
            baseline: expected.baseline_frame_revision,
            observed: observed.frame_revision,
        });
    }
    if observed.acknowledged_play_revision != Some(expected.play_revision) {
        return Err(PendingReason::PlayRevisionMismatch {
            expected: expected.play_revision,
            observed: observed.acknowledged_play_revision,
        });
    }
    if observed.acknowledged_seek_revision != Some(expected.seek_revision) {
        return Err(PendingReason::SeekRevisionMismatch {
            expected: expected.seek_revision,
            observed: observed.acknowledged_seek_revision,
        });
    }
    let expected_layers = expected_skin_layers
        .iter()
        .copied()
        .map(Into::into)
        .collect::<Vec<Box<str>>>();
    if observed.skin_layers != expected_layers {
        return Err(PendingReason::SkinLayersMismatch {
            expected: expected_layers,
            observed: observed.skin_layers.clone(),
        });
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RehearsalFailure {
    kind: &'static str,
    message: Box<str>,
}

impl RehearsalFailure {
    fn new(kind: &'static str, message: impl Into<Box<str>>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

#[derive(Debug)]
enum MachineState {
    WaitingForReady {
        updates: usize,
    },
    ReadyToIssue {
        sample_index: usize,
    },
    CommandsQueued {
        sample_index: usize,
        baselines: [IssuedBaseline; 2],
    },
    AwaitingFrames {
        sample_index: usize,
        expectations: [CaptureExpectation; 2],
        updates: usize,
        accepted: [bool; 2],
        pending: [Option<PendingReason>; 2],
    },
    Complete,
    Failed(RehearsalFailure),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ObservationDecision {
    newly_accepted: [bool; 2],
}

#[derive(Debug)]
struct RehearsalMachine {
    state: MachineState,
}

impl Default for RehearsalMachine {
    fn default() -> Self {
        Self {
            state: MachineState::WaitingForReady { updates: 0 },
        }
    }
}

impl RehearsalMachine {
    fn update_readiness(&mut self, ready: bool) {
        let MachineState::WaitingForReady { updates } = &mut self.state else {
            return;
        };
        if ready {
            self.state = MachineState::ReadyToIssue { sample_index: 0 };
            return;
        }
        *updates = updates.saturating_add(1);
        if *updates >= READY_UPDATE_LIMIT {
            self.fail(
                "ready_timeout",
                format!("both viewer sources were not ready within {READY_UPDATE_LIMIT} updates"),
            );
        }
    }

    fn sample_to_issue(&self) -> Option<FixedSample> {
        let MachineState::ReadyToIssue { sample_index } = self.state else {
            return None;
        };
        FIXED_SAMPLES.get(sample_index).copied()
    }

    fn note_commands_queued(&mut self, baselines: [IssuedBaseline; 2]) {
        let MachineState::ReadyToIssue { sample_index } = self.state else {
            self.fail(
                "internal_state",
                "sample commands were queued from an invalid state",
            );
            return;
        };
        self.state = MachineState::CommandsQueued {
            sample_index,
            baselines,
        };
    }

    fn record_expectations(&mut self, expectations: [CaptureExpectation; 2]) {
        let MachineState::CommandsQueued {
            sample_index,
            baselines,
        } = self.state
        else {
            self.fail(
                "internal_state",
                "command generations were recorded from an invalid state",
            );
            return;
        };
        for source in CaptureSource::ALL {
            let index = source.index();
            if expectations[index].play_revision == baselines[index].play_revision
                || expectations[index].seek_revision == baselines[index].seek_revision
            {
                self.fail(
                    "command_projection",
                    format!(
                        "{} did not receive fresh play and seek generations",
                        source.id()
                    ),
                );
                return;
            }
            if expectations[index].baseline_frame_revision != baselines[index].frame_revision {
                self.fail(
                    "internal_state",
                    format!("{} frame baseline changed before observation", source.id()),
                );
                return;
            }
        }
        self.state = MachineState::AwaitingFrames {
            sample_index,
            expectations,
            updates: 0,
            accepted: [false; 2],
            pending: [
                Some(PendingReason::MissingFrame),
                Some(PendingReason::MissingFrame),
            ],
        };
    }

    fn observe(&mut self, observed: [&ObservedSource; 2]) -> ObservationDecision {
        let MachineState::AwaitingFrames {
            sample_index,
            expectations,
            updates,
            accepted,
            pending,
        } = &mut self.state
        else {
            return ObservationDecision {
                newly_accepted: [false; 2],
            };
        };
        let sample = FIXED_SAMPLES[*sample_index];
        let mut newly_accepted = [false; 2];
        for source in CaptureSource::ALL {
            let index = source.index();
            if accepted[index] {
                continue;
            }
            match classify_observation(expectations[index], sample.skin_layers(), observed[index]) {
                Ok(()) => {
                    accepted[index] = true;
                    pending[index] = None;
                    newly_accepted[index] = true;
                }
                Err(reason) => pending[index] = Some(reason),
            }
        }
        *updates = updates.saturating_add(1);
        if accepted.iter().all(|accepted| *accepted) {
            let next = sample_index.saturating_add(1);
            self.state = if next == FIXED_SAMPLES.len() {
                MachineState::Complete
            } else {
                MachineState::ReadyToIssue { sample_index: next }
            };
        } else if *updates >= SAMPLE_UPDATE_LIMIT {
            let message = format!(
                "sample {} was not observed within {SAMPLE_UPDATE_LIMIT} updates (current: {:?}; proposed: {:?})",
                sample.id(),
                pending[0],
                pending[1]
            );
            self.fail("sample_timeout", message);
        }
        ObservationDecision { newly_accepted }
    }

    fn fail(&mut self, kind: &'static str, message: impl Into<Box<str>>) {
        self.state = MachineState::Failed(RehearsalFailure::new(kind, message));
    }

    fn failure(&self) -> Option<&RehearsalFailure> {
        match &self.state {
            MachineState::Failed(failure) => Some(failure),
            _other => None,
        }
    }

    const fn is_complete(&self) -> bool {
        matches!(self.state, MachineState::Complete)
    }
}

#[derive(Debug, Serialize)]
struct CapturedObservation {
    source: &'static str,
    sample: &'static str,
    frame_revision: u64,
    acknowledged_play_revision: u64,
    acknowledged_seek_revision: u64,
    frame: SemanticFrame,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct CapturedRuntimeIdentity {
    manifest_sha256: Box<str>,
    content_sha256: Box<str>,
}

#[cfg(target_arch = "wasm32")]
impl CapturedRuntimeIdentity {
    fn from_provenance(provenance: &SourceProvenance) -> Self {
        Self {
            manifest_sha256: provenance.manifest_sha256().into(),
            content_sha256: provenance.content_sha256().into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct CapturedRuntimeSources {
    current: CapturedRuntimeIdentity,
    proposed: CapturedRuntimeIdentity,
}

#[derive(Serialize)]
struct CompleteDocument<'a> {
    format_version: u8,
    state: &'static str,
    runtime_sources: &'a CapturedRuntimeSources,
    observations: &'a [CapturedObservation],
}

#[derive(Serialize)]
struct ErrorDetail<'a> {
    kind: &'a str,
    message: &'a str,
}

#[derive(Serialize)]
struct ErrorDocument<'a> {
    format_version: u8,
    state: &'static str,
    error: ErrorDetail<'a>,
}

fn encode_complete(
    runtime_sources: &CapturedRuntimeSources,
    observations: &[CapturedObservation],
) -> Result<Vec<u8>, RehearsalFailure> {
    if observations.len() != FIXED_SAMPLES.len() * CaptureSource::ALL.len() {
        return Err(RehearsalFailure::new(
            "incomplete_observations",
            format!(
                "expected eight semantic observations, received {}",
                observations.len()
            ),
        ));
    }
    let bytes = serde_json::to_vec(&CompleteDocument {
        format_version: 1,
        state: "complete",
        runtime_sources,
        observations,
    })
    .map_err(|error| RehearsalFailure::new("encode_error", error.to_string()))?;
    if bytes.len() > MAX_OBSERVATION_BYTES {
        return Err(RehearsalFailure::new(
            "output_too_large",
            format!(
                "observation JSON has {} bytes; maximum is {MAX_OBSERVATION_BYTES}",
                bytes.len()
            ),
        ));
    }
    Ok(bytes)
}

fn bounded_error_json(kind: &str, message: &str) -> Vec<u8> {
    let message = truncate_utf8(message, MAX_ERROR_MESSAGE_BYTES);
    serde_json::to_vec(&ErrorDocument {
        format_version: 1,
        state: "error",
        error: ErrorDetail {
            kind,
            message: &message,
        },
    })
    .unwrap_or_else(|_error| {
        br#"{"format_version":1,"state":"error","error":{"kind":"encode_error","message":"could not encode rehearsal error"}}"#.to_vec()
    })
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes.saturating_sub(3).min(value.len());
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}...", &value[..end])
}

#[cfg(target_arch = "wasm32")]
mod browser_integration {
    use bevy::prelude::*;
    use bevy_spinal::{
        SpinalAnimator, SpinalSemanticCapture, SpinalSet, SpinalSkinLayers, spinal::PlaybackMode,
    };
    use wasm_bindgen::JsValue;

    use super::*;
    use crate::{
        command::{SkinSelection, ViewerCommand},
        runtime::{CommandInbox, ViewerLoadState, ViewerRuntime, ViewerRuntimeSet},
        session::SourceSlot,
    };

    #[derive(Resource, Debug, Default)]
    struct BrowserRehearsal {
        machine: RehearsalMachine,
        observations: Vec<CapturedObservation>,
        terminal_published: bool,
    }

    pub(crate) fn initialize_dom() -> Result<(), String> {
        let document = web_sys::window()
            .and_then(|window| window.document())
            .ok_or_else(|| "document is unavailable".to_owned())?;
        if document.get_element_by_id(OUTPUT_ELEMENT_ID).is_some() {
            return Err(format!(
                "the page already contains reserved element `{OUTPUT_ELEMENT_ID}`"
            ));
        }
        let app = document
            .get_element_by_id("spinal-app")
            .ok_or_else(|| "the viewer application element is unavailable".to_owned())?;
        let output = document
            .create_element("script")
            .map_err(|_| "could not create the rehearsal output element".to_owned())?;
        output
            .set_attribute("id", OUTPUT_ELEMENT_ID)
            .and_then(|()| output.set_attribute("type", "application/json"))
            .and_then(|()| output.set_attribute(COMPLETE_ATTRIBUTE, "false"))
            .and_then(|()| output.set_attribute(STATE_ATTRIBUTE, "running"))
            .map_err(|_| "could not configure the rehearsal output element".to_owned())?;
        output.set_text_content(Some(r#"{"format_version":1,"state":"running"}"#));
        app.append_child(&output)
            .map_err(|_| "could not attach the rehearsal output element".to_owned())?;
        Ok(())
    }

    pub(crate) fn publish_external_error(kind: &str, message: &str) {
        publish_terminal_bytes("error", &bounded_error_json(kind, message));
    }

    pub(crate) fn install(app: &mut App) {
        app.init_resource::<BrowserRehearsal>()
            .add_systems(
                Startup,
                attach_semantic_capture.after(ViewerRuntimeSet::Setup),
            )
            .add_systems(
                Update,
                issue_sample_commands
                    .after(ViewerRuntimeSet::Poll)
                    .before(ViewerRuntimeSet::Commands),
            )
            .add_systems(
                Update,
                record_issued_generations
                    .after(ViewerRuntimeSet::Commands)
                    .before(SpinalSet::Animate),
            )
            .add_systems(
                Update,
                observe_semantic_frames.after(ViewerRuntimeSet::Observe),
            )
            .add_systems(
                Update,
                publish_terminal_output.after(observe_semantic_frames),
            );
    }

    fn attach_semantic_capture(mut commands: Commands<'_, '_>, runtime: Res<'_, ViewerRuntime>) {
        for source in runtime.sources() {
            commands
                .entity(source.entity())
                .insert(SpinalSemanticCapture::default());
        }
    }

    fn issue_sample_commands(
        runtime: Res<'_, ViewerRuntime>,
        captures: Query<'_, '_, (&SpinalAnimator, &SpinalSemanticCapture)>,
        mut inbox: ResMut<'_, CommandInbox>,
        mut rehearsal: ResMut<'_, BrowserRehearsal>,
    ) {
        if rehearsal.machine.failure().is_some() || rehearsal.machine.is_complete() {
            return;
        }
        if let Some(failure) = definitive_runtime_failure(&runtime) {
            rehearsal.machine.fail(failure.kind, failure.message);
            return;
        }
        rehearsal.machine.update_readiness(runtime.controls_ready());
        let Some(sample) = rehearsal.machine.sample_to_issue() else {
            return;
        };
        if let Err(failure) = validate_fixed_fixture(&runtime) {
            rehearsal.machine.fail(failure.kind, failure.message);
            return;
        }
        let entities = match source_entities(&runtime) {
            Ok(entities) => entities,
            Err(failure) => {
                rehearsal.machine.fail(failure.kind, failure.message);
                return;
            }
        };
        let mut baselines = [IssuedBaseline {
            frame_revision: 0,
            play_revision: 0,
            seek_revision: 0,
        }; 2];
        for source in CaptureSource::ALL {
            let index = source.index();
            let Ok((animator, capture)) = captures.get(entities[index]) else {
                rehearsal.machine.fail(
                    "missing_component",
                    format!(
                        "{} is missing SpinalAnimator or SpinalSemanticCapture",
                        source.id()
                    ),
                );
                return;
            };
            baselines[index] = IssuedBaseline {
                frame_revision: capture.frame_revision(),
                play_revision: animator.revision(),
                seek_revision: animator.seek_revision(),
            };
        }

        inbox.push(ViewerCommand::SetLooping(false));
        inbox.push(ViewerCommand::SelectAnimation(ANIMATION_NAME.into()));
        inbox.push(ViewerCommand::SelectSkin(match sample.skin_layers() {
            [] => SkinSelection::Default,
            [ALTERNATE_SKIN_NAME] => SkinSelection::Named(ALTERNATE_SKIN_NAME.into()),
            _other => unreachable!("the fixed schedule has only two skin selections"),
        }));
        if !runtime.model().transport().is_paused() {
            inbox.push(ViewerCommand::TogglePause);
        }
        inbox.push(ViewerCommand::SeekAbsolute(sample.time()));
        rehearsal.machine.note_commands_queued(baselines);
    }

    fn record_issued_generations(
        runtime: Res<'_, ViewerRuntime>,
        controls: Query<'_, '_, (&SpinalAnimator, &SpinalSkinLayers)>,
        mut rehearsal: ResMut<'_, BrowserRehearsal>,
    ) {
        let MachineState::CommandsQueued {
            sample_index,
            baselines,
        } = rehearsal.machine.state
        else {
            return;
        };
        let sample = FIXED_SAMPLES[sample_index];
        if runtime.model().transport().selected_animation() != Some(ANIMATION_NAME)
            || runtime.model().transport().is_looping()
            || !runtime.model().transport().is_paused()
            || runtime.model().transport().position() != sample.time()
            || runtime.model().selected_skin().name() != sample.skin_layers().first().copied()
        {
            rehearsal.machine.fail(
                "command_projection",
                format!(
                    "viewer session did not retain the exact {} command state",
                    sample.id()
                ),
            );
            return;
        }
        let entities = match source_entities(&runtime) {
            Ok(entities) => entities,
            Err(failure) => {
                rehearsal.machine.fail(failure.kind, failure.message);
                return;
            }
        };
        let mut expectations = [CaptureExpectation {
            baseline_frame_revision: 0,
            play_revision: 0,
            seek_revision: 0,
        }; 2];
        for source in CaptureSource::ALL {
            let index = source.index();
            let Ok((animator, skin_layers)) = controls.get(entities[index]) else {
                rehearsal.machine.fail(
                    "missing_component",
                    format!(
                        "{} is missing SpinalAnimator or SpinalSkinLayers",
                        source.id()
                    ),
                );
                return;
            };
            if animator.animation() != Some(ANIMATION_NAME)
                || animator.mode() != Some(PlaybackMode::Once)
                || !animator.is_paused()
                || animator.seek_position() != Some(sample.time())
                || !skin_layers.iter().eq(sample.skin_layers().iter().copied())
            {
                rehearsal.machine.fail(
                    "command_projection",
                    format!(
                        "{} did not receive the exact {} playback and skin intent",
                        source.id(),
                        sample.id()
                    ),
                );
                return;
            }
            expectations[index] = CaptureExpectation {
                baseline_frame_revision: baselines[index].frame_revision,
                play_revision: animator.revision(),
                seek_revision: animator.seek_revision(),
            };
        }
        rehearsal.machine.record_expectations(expectations);
    }

    fn observe_semantic_frames(
        runtime: Res<'_, ViewerRuntime>,
        captures: Query<'_, '_, &SpinalSemanticCapture>,
        mut rehearsal: ResMut<'_, BrowserRehearsal>,
    ) {
        let MachineState::AwaitingFrames { sample_index, .. } = rehearsal.machine.state else {
            return;
        };
        let sample = FIXED_SAMPLES[sample_index];
        let entities = match source_entities(&runtime) {
            Ok(entities) => entities,
            Err(failure) => {
                rehearsal.machine.fail(failure.kind, failure.message);
                return;
            }
        };
        let mut owned = Vec::with_capacity(2);
        for source in CaptureSource::ALL {
            let index = source.index();
            let Ok(capture) = captures.get(entities[index]) else {
                rehearsal.machine.fail(
                    "missing_component",
                    format!("{} is missing SpinalSemanticCapture", source.id()),
                );
                return;
            };
            owned.push(ObservedSource {
                frame_present: capture.frame().is_some(),
                frame_revision: capture.frame_revision(),
                acknowledged_play_revision: capture.acknowledged_play_revision(),
                acknowledged_seek_revision: capture.acknowledged_seek_revision(),
                skin_layers: capture.frame().map_or_else(Vec::new, |frame| {
                    frame.skin_layers().map(Into::into).collect()
                }),
            });
        }
        let decision = rehearsal.machine.observe([&owned[0], &owned[1]]);
        for source in CaptureSource::ALL {
            let index = source.index();
            if !decision.newly_accepted[index] {
                continue;
            }
            let capture = captures
                .get(entities[index])
                .expect("the capture was read immediately above");
            let Some(frame) = capture.frame() else {
                rehearsal.machine.fail(
                    "internal_state",
                    format!("{} accepted a missing semantic frame", source.id()),
                );
                return;
            };
            let canonical = match frame.to_canonical_json() {
                Ok(bytes) => bytes,
                Err(error) => {
                    rehearsal.machine.fail("encode_error", error.to_string());
                    return;
                }
            };
            if canonical.len() > MAX_SEMANTIC_FRAME_BYTES {
                rehearsal.machine.fail(
                    "frame_too_large",
                    format!(
                        "{} {} semantic frame has {} bytes; maximum is {MAX_SEMANTIC_FRAME_BYTES}",
                        source.id(),
                        sample.id(),
                        canonical.len()
                    ),
                );
                return;
            }
            rehearsal.observations.push(CapturedObservation {
                source: source.id(),
                sample: sample.id(),
                frame_revision: capture.frame_revision(),
                acknowledged_play_revision: capture
                    .acknowledged_play_revision()
                    .expect("accepted observations acknowledge a play command"),
                acknowledged_seek_revision: capture
                    .acknowledged_seek_revision()
                    .expect("accepted observations acknowledge a seek command"),
                frame: frame.clone(),
            });
        }
    }

    fn publish_terminal_output(
        runtime: Res<'_, ViewerRuntime>,
        mut rehearsal: ResMut<'_, BrowserRehearsal>,
    ) {
        if rehearsal.terminal_published {
            return;
        }
        if let Some(failure) = rehearsal.machine.failure() {
            publish_terminal_bytes("error", &bounded_error_json(failure.kind, &failure.message));
            rehearsal.terminal_published = true;
            return;
        }
        if !rehearsal.machine.is_complete() {
            return;
        }
        let runtime_sources = match captured_runtime_sources(&runtime) {
            Ok(runtime_sources) => runtime_sources,
            Err(failure) => {
                rehearsal.machine.fail(failure.kind, failure.message);
                return;
            }
        };
        rehearsal
            .observations
            .sort_by_key(|observation| observation_order(observation.sample, observation.source));
        match encode_complete(&runtime_sources, &rehearsal.observations) {
            Ok(bytes) => {
                publish_terminal_bytes("complete", &bytes);
                rehearsal.terminal_published = true;
            }
            Err(failure) => rehearsal.machine.fail(failure.kind, failure.message),
        }
    }

    fn captured_runtime_sources(
        runtime: &ViewerRuntime,
    ) -> Result<CapturedRuntimeSources, RehearsalFailure> {
        let identity = |slot, label| {
            runtime
                .source(slot)
                .map(|source| CapturedRuntimeIdentity::from_provenance(source.provenance()))
                .ok_or_else(|| {
                    RehearsalFailure::new(
                        "missing_source_identity",
                        format!("the {label} runtime source identity is unavailable"),
                    )
                })
        };
        Ok(CapturedRuntimeSources {
            current: identity(SourceSlot::Primary, "Current")?,
            proposed: identity(SourceSlot::Comparison, "Proposed")?,
        })
    }

    fn validate_fixed_fixture(runtime: &ViewerRuntime) -> Result<(), RehearsalFailure> {
        let entities = source_entities(runtime)?;
        if runtime
            .model()
            .animations()
            .iter()
            .all(|name| name.as_ref() != ANIMATION_NAME)
        {
            return Err(RehearsalFailure::new(
                "fixture_contract",
                "the synchronized catalog does not contain `sway`",
            ));
        }
        let alternate = SkinSelection::Named(ALTERNATE_SKIN_NAME.into());
        for (source, slot) in [
            (CaptureSource::Current, SourceSlot::Primary),
            (CaptureSource::Proposed, SourceSlot::Comparison),
        ] {
            if runtime.model().duration(slot, ANIMATION_NAME) != Some(ANIMATION_DURATION) {
                return Err(RehearsalFailure::new(
                    "fixture_contract",
                    format!("{} must contain a one-second `sway` animation", source.id()),
                ));
            }
            if !runtime.model().skin_present(slot, &alternate) {
                return Err(RehearsalFailure::new(
                    "fixture_contract",
                    format!("{} must contain the `alternate` skin", source.id()),
                ));
            }
        }
        let _entities_are_resolved = entities;
        Ok(())
    }

    fn source_entities(runtime: &ViewerRuntime) -> Result<[Entity; 2], RehearsalFailure> {
        if runtime.sources().len() != 2 {
            return Err(RehearsalFailure::new(
                "missing_comparison",
                "the browser rehearsal requires exactly Current and Proposed sources",
            ));
        }
        let current = runtime
            .source(SourceSlot::Primary)
            .map(|source| source.entity())
            .ok_or_else(|| {
                RehearsalFailure::new("missing_source", "the Current source is unavailable")
            })?;
        let proposed = runtime
            .source(SourceSlot::Comparison)
            .map(|source| source.entity())
            .ok_or_else(|| {
                RehearsalFailure::new("missing_source", "the Proposed source is unavailable")
            })?;
        Ok([current, proposed])
    }

    fn definitive_runtime_failure(runtime: &ViewerRuntime) -> Option<RehearsalFailure> {
        runtime.sources().iter().find_map(|source| {
            let label = match source.slot() {
                SourceSlot::Primary => "current",
                SourceSlot::Comparison => "proposed",
            };
            match source.load_state() {
                ViewerLoadState::Failed(_error) => Some(RehearsalFailure::new(
                    "source_load_error",
                    format!("{label} source failed to load"),
                )),
                ViewerLoadState::Loading | ViewerLoadState::Ready => None,
            }
        })
    }

    fn observation_order(sample: &str, source: &str) -> (usize, usize) {
        let sample_index = FIXED_SAMPLES
            .iter()
            .position(|candidate| candidate.id() == sample)
            .unwrap_or(usize::MAX);
        let source_index = usize::from(source == CaptureSource::Proposed.id());
        (sample_index, source_index)
    }

    fn publish_terminal_bytes(state: &str, bytes: &[u8]) {
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return;
        };
        let Some(output) = document.get_element_by_id(OUTPUT_ELEMENT_ID) else {
            return;
        };
        if output.get_attribute(COMPLETE_ATTRIBUTE).as_deref() == Some("true") {
            return;
        }
        let Ok(json) = std::str::from_utf8(bytes) else {
            web_sys::console::error_1(&JsValue::from_str("rehearsal output was not valid UTF-8"));
            return;
        };
        output.set_text_content(Some(json));
        let _ignored = output.set_attribute(STATE_ATTRIBUTE, state);
        let _ignored = output.set_attribute(COMPLETE_ATTRIBUTE, "true");
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) use browser_integration::{initialize_dom, install, publish_external_error};

#[cfg(test)]
mod tests {
    use super::*;

    const BASELINE: [IssuedBaseline; 2] = [
        IssuedBaseline {
            frame_revision: 4,
            play_revision: 8,
            seek_revision: 12,
        },
        IssuedBaseline {
            frame_revision: 5,
            play_revision: 9,
            seek_revision: 13,
        },
    ];
    const EXPECTED: [CaptureExpectation; 2] = [
        CaptureExpectation {
            baseline_frame_revision: 4,
            play_revision: 9,
            seek_revision: 14,
        },
        CaptureExpectation {
            baseline_frame_revision: 5,
            play_revision: 10,
            seek_revision: 15,
        },
    ];

    fn observed(index: usize, skin_layers: &[&str]) -> ObservedSource {
        ObservedSource {
            frame_present: true,
            frame_revision: EXPECTED[index].baseline_frame_revision + 1,
            acknowledged_play_revision: Some(EXPECTED[index].play_revision),
            acknowledged_seek_revision: Some(EXPECTED[index].seek_revision),
            skin_layers: skin_layers.iter().copied().map(Into::into).collect(),
        }
    }

    fn runtime_sources() -> CapturedRuntimeSources {
        CapturedRuntimeSources {
            current: CapturedRuntimeIdentity {
                manifest_sha256: "a".repeat(64).into(),
                content_sha256: "b".repeat(64).into(),
            },
            proposed: CapturedRuntimeIdentity {
                manifest_sha256: "c".repeat(64).into(),
                content_sha256: "d".repeat(64).into(),
            },
        }
    }

    fn begin_sample(machine: &mut RehearsalMachine) -> FixedSample {
        let sample = machine.sample_to_issue().expect("sample ready to issue");
        machine.note_commands_queued(BASELINE);
        machine.record_expectations(EXPECTED);
        sample
    }

    #[test]
    fn fixed_schedule_is_literal_and_closed() {
        assert_eq!(
            FIXED_SAMPLES.map(|sample| (sample.id(), sample.time(), sample.skin_layers())),
            [
                ("sway-start", Duration::ZERO, &[] as &[&str]),
                ("sway-middle", Duration::from_millis(500), &[]),
                (
                    "sway-alternate-skin",
                    Duration::from_millis(750),
                    &["alternate"]
                ),
                ("sway-end", Duration::from_secs(1), &[]),
            ]
        );
    }

    #[test]
    fn ordinary_browser_shell_contains_no_rehearsal_mode_or_output_element() {
        let shell = include_str!("../web/index.html");
        assert!(!shell.contains(OUTPUT_ELEMENT_ID));
        assert!(!shell.contains("phase0b-rehearsal"));
    }

    #[test]
    fn readiness_has_a_fixed_update_timeout() {
        let mut machine = RehearsalMachine::default();
        for _ in 0..READY_UPDATE_LIMIT - 1 {
            machine.update_readiness(false);
            assert!(machine.failure().is_none());
        }
        machine.update_readiness(false);
        assert_eq!(
            machine.failure().map(|failure| failure.kind),
            Some("ready_timeout")
        );
    }

    #[test]
    fn sources_can_be_accepted_on_different_updates_without_losing_exact_seek_ack() {
        let mut machine = RehearsalMachine::default();
        machine.update_readiness(true);
        assert_eq!(begin_sample(&mut machine), FixedSample::SwayStart);

        let current = observed(0, &[]);
        let mut proposed = observed(1, &[]);
        proposed.acknowledged_seek_revision = None;
        let first = machine.observe([&current, &proposed]);
        assert_eq!(first.newly_accepted, [true, false]);

        let stale_current = ObservedSource {
            frame_present: false,
            ..current
        };
        let proposed = observed(1, &[]);
        let second = machine.observe([&stale_current, &proposed]);
        assert_eq!(second.newly_accepted, [false, true]);
        assert_eq!(machine.sample_to_issue(), Some(FixedSample::SwayMiddle));
    }

    #[test]
    fn every_sample_requires_fresh_generations_and_exact_ordered_skin_layers() {
        let mut machine = RehearsalMachine::default();
        machine.update_readiness(true);
        for sample in FIXED_SAMPLES {
            assert_eq!(begin_sample(&mut machine), sample);
            let current = observed(0, sample.skin_layers());
            let proposed = observed(1, sample.skin_layers());
            let decision = machine.observe([&current, &proposed]);
            assert_eq!(decision.newly_accepted, [true, true]);
        }
        assert!(machine.is_complete());

        let mut wrong_order = observed(0, &["alternate", "accessory"]);
        assert_eq!(
            classify_observation(EXPECTED[0], &["accessory", "alternate"], &wrong_order),
            Err(PendingReason::SkinLayersMismatch {
                expected: vec!["accessory".into(), "alternate".into()],
                observed: vec!["alternate".into(), "accessory".into()],
            })
        );
        wrong_order.frame_revision = EXPECTED[0].baseline_frame_revision;
        assert!(matches!(
            classify_observation(EXPECTED[0], &[], &wrong_order),
            Err(PendingReason::FrameRevisionNotAdvanced { .. })
        ));
    }

    #[test]
    fn sample_timeout_reports_the_last_rejection() {
        let mut machine = RehearsalMachine::default();
        machine.update_readiness(true);
        begin_sample(&mut machine);
        let missing = ObservedSource {
            frame_present: false,
            frame_revision: 0,
            acknowledged_play_revision: None,
            acknowledged_seek_revision: None,
            skin_layers: Vec::new(),
        };
        for _ in 0..SAMPLE_UPDATE_LIMIT {
            machine.observe([&missing, &missing]);
        }
        let failure = machine.failure().expect("fixed sample timeout");
        assert_eq!(failure.kind, "sample_timeout");
        assert!(failure.message.contains("sway-start"));
        assert!(failure.message.contains("MissingFrame"));
    }

    #[test]
    fn observation_document_is_deterministic_bounded_and_contains_only_observations() {
        let frame = SemanticFrame::from_json(
            br#"{"format_version":1,"default_skin":null,"skin_layers":[],"bones":[],"slots":[],"draw_items":[],"ik_constraints":[],"transform_constraints":[],"active_diagnostics":[]}"#,
        )
        .expect("minimal semantic frame");
        let mut observations = Vec::new();
        for sample in FIXED_SAMPLES {
            for source in CaptureSource::ALL {
                observations.push(CapturedObservation {
                    source: source.id(),
                    sample: sample.id(),
                    frame_revision: 1,
                    acknowledged_play_revision: 2,
                    acknowledged_seek_revision: 3,
                    frame: frame.clone(),
                });
            }
        }
        let runtime_sources = runtime_sources();
        let first = encode_complete(&runtime_sources, &observations).expect("bounded output");
        let second =
            encode_complete(&runtime_sources, &observations).expect("deterministic output");
        assert_eq!(first, second);
        assert!(first.len() <= MAX_OBSERVATION_BYTES);
        let text = std::str::from_utf8(&first).unwrap();
        assert!(text.contains(&format!(
            r#""runtime_sources":{{"current":{{"manifest_sha256":"{}","content_sha256":"{}"}},"proposed":{{"manifest_sha256":"{}","content_sha256":"{}"}}}}"#,
            "a".repeat(64),
            "b".repeat(64),
            "c".repeat(64),
            "d".repeat(64),
        )));
        assert!(text.contains(r#""observations":[{"source":"current","sample":"sway-start""#));
        for excluded in [
            "\"pass\"",
            "\"gate\"",
            "\"pixels\"",
            "\"events\"",
            "\"approval\"",
        ] {
            assert!(!text.contains(excluded));
        }
    }

    #[test]
    fn external_error_json_is_terminal_machine_data_and_bounded() {
        let bytes = bounded_error_json("sample_timeout", &"å".repeat(MAX_ERROR_MESSAGE_BYTES));
        assert!(bytes.len() < MAX_ERROR_MESSAGE_BYTES + 256);
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["state"], "error");
        assert_eq!(value["error"]["kind"], "sample_timeout");
    }
}

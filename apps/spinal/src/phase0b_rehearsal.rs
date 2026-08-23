//! Internal browser-only semantic observation harness for the fixed Phase 0B case.
//!
//! The feature is deliberately non-default. It drives the same viewer session,
//! commands, entities, runtime, and renderer as the ordinary browser app.

use std::time::Duration;

use bevy_spinal::spinal::{SemanticDiagnosticCode, SemanticFrame};
use serde::Serialize;
#[cfg(target_arch = "wasm32")]
use spinal_phase0b::contract::{ALTERNATE_SKIN_NAME, ANIMATION_DURATION};
use spinal_phase0b::contract::{
    ANIMATION_NAME, EVENT_WINDOW_END_NS, EVENT_WINDOW_ID, EVENT_WINDOW_START_NS,
    SAMPLE_SCHEDULE as FIXED_SAMPLES, Sample as FixedSample,
};
use spinal_phase0b::event_compare::{
    EVENT_WINDOW_FORMAT_VERSION, EventWindowDocument, MAX_DIAGNOSTIC_CODES, MAX_EVENT_COUNT,
    MAX_EVENT_IDENTIFIER_BYTES, MAX_EVENT_STRING_BYTES, MAX_EVENT_WINDOW_BYTES,
    parse_event_window_json,
};

const READY_UPDATE_LIMIT: usize = 1_800;
const EVENT_CAPTURE_UPDATE_LIMIT: usize = 1_800;
const SAMPLE_UPDATE_LIMIT: usize = 8;
const MAX_SEMANTIC_FRAME_BYTES: usize = 1024 * 1024;
const MAX_OBSERVATION_BYTES: usize =
    8 * MAX_SEMANTIC_FRAME_BYTES + 2 * MAX_EVENT_WINDOW_BYTES + 64 * 1024;
const MAX_ERROR_MESSAGE_BYTES: usize = 4 * 1024;
#[cfg(any(target_arch = "wasm32", test))]
const OUTPUT_ELEMENT_ID: &str = "spinal-phase0b-observation";
#[cfg(any(target_arch = "wasm32", test))]
const CONTROL_ELEMENT_ID: &str = "spinal-phase0b-control";
#[cfg(target_arch = "wasm32")]
const COMPLETE_ATTRIBUTE: &str = "data-spinal-phase0b-complete";
#[cfg(target_arch = "wasm32")]
const STATE_ATTRIBUTE: &str = "data-spinal-phase0b-state";
#[cfg(target_arch = "wasm32")]
const INBOUND_ATTRIBUTE: &str = "data-spinal-phase0b-inbound";
#[cfg(target_arch = "wasm32")]
const OUTBOUND_ATTRIBUTE: &str = "data-spinal-phase0b-outbound";

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

#[derive(Clone, Debug)]
struct ObservedEvent {
    entity_bits: u64,
    track: Option<Box<str>>,
    playback: u64,
    animation: Box<str>,
    name: Box<str>,
    loop_index: u128,
    local_time: Duration,
    integer: i32,
    float: f32,
    string: Option<Box<str>>,
    volume: f32,
    balance: f32,
    diagnostic_codes: Vec<SemanticDiagnosticCode>,
    degraded: bool,
}

#[derive(Clone, Debug, Serialize)]
struct CapturedEvent {
    animation: &'static str,
    name: Box<str>,
    local_time_ns: u64,
    loop_index: u64,
    integer: i32,
    float: f64,
    string: Option<Box<str>>,
    volume: f64,
    balance: f64,
    diagnostic_codes: Vec<SemanticDiagnosticCode>,
}

#[derive(Debug)]
struct EventSourceAccumulator {
    source: CaptureSource,
    entity_bits: u64,
    playback: Option<u64>,
    previous_playback_time: Option<Duration>,
    previous_event_time: Option<Duration>,
    events: Vec<CapturedEvent>,
    encoded_event_bytes: usize,
}

impl EventSourceAccumulator {
    fn new(source: CaptureSource, entity_bits: u64) -> Self {
        Self {
            source,
            entity_bits,
            playback: None,
            previous_playback_time: None,
            previous_event_time: None,
            events: Vec::new(),
            encoded_event_bytes: 0,
        }
    }

    fn bind_playback(&mut self, playback: u64) -> Result<(), RehearsalFailure> {
        if playback == 0 {
            return Err(RehearsalFailure::new(
                "event_playback",
                format!("{} event playback identifier is zero", self.source.id()),
            ));
        }
        if let Some(expected) = self.playback {
            if playback != expected {
                return Err(RehearsalFailure::new(
                    "event_playback",
                    format!(
                        "{} event playback changed from {expected} to {playback}",
                        self.source.id()
                    ),
                ));
            }
        } else {
            self.playback = Some(playback);
        }
        Ok(())
    }

    fn observe_playback(
        &mut self,
        position: Duration,
        complete: bool,
    ) -> Result<(), RehearsalFailure> {
        let end = Duration::from_nanos(EVENT_WINDOW_END_NS);
        if position > end {
            return Err(RehearsalFailure::new(
                "event_playback",
                format!(
                    "{} event playback exceeded the fixed window",
                    self.source.id()
                ),
            ));
        }
        if self
            .previous_playback_time
            .is_some_and(|previous| position < previous)
        {
            return Err(RehearsalFailure::new(
                "event_playback",
                format!("{} event playback moved backwards", self.source.id()),
            ));
        }
        if complete != (position == end) {
            return Err(RehearsalFailure::new(
                "event_playback",
                format!(
                    "{} event completion did not match its exact window position",
                    self.source.id()
                ),
            ));
        }
        self.previous_playback_time = Some(position);
        Ok(())
    }

    fn push(&mut self, event: ObservedEvent) -> Result<(), RehearsalFailure> {
        if event.entity_bits != self.entity_bits {
            return Err(RehearsalFailure::new(
                "event_entity",
                "an event came from an entity outside the hidden capture pair",
            ));
        }
        let expected_playback = self.playback.ok_or_else(|| {
            RehearsalFailure::new(
                "event_playback",
                format!(
                    "{} emitted an event before playback was observed",
                    self.source.id()
                ),
            )
        })?;
        if event.playback != expected_playback {
            return Err(RehearsalFailure::new(
                "event_playback",
                format!(
                    "{} event playback was {}; expected {expected_playback}",
                    self.source.id(),
                    event.playback
                ),
            ));
        }
        if event.track.is_some() {
            return Err(RehearsalFailure::new(
                "event_track",
                format!("{} event came from an override track", self.source.id()),
            ));
        }
        if event.animation.as_ref() != ANIMATION_NAME || event.loop_index != 0 {
            return Err(RehearsalFailure::new(
                "event_binding",
                format!("{} event changed animation or loop", self.source.id()),
            ));
        }
        let end = Duration::from_nanos(EVENT_WINDOW_END_NS);
        if !(Duration::from_nanos(EVENT_WINDOW_START_NS)..=end).contains(&event.local_time) {
            return Err(RehearsalFailure::new(
                "event_time",
                format!("{} event was outside the fixed window", self.source.id()),
            ));
        }
        if self
            .previous_event_time
            .is_some_and(|previous| event.local_time < previous)
        {
            return Err(RehearsalFailure::new(
                "event_order",
                format!("{} event emission order moved backwards", self.source.id()),
            ));
        }
        if event.degraded || !event.diagnostic_codes.is_empty() {
            return Err(RehearsalFailure::new(
                "event_degraded",
                format!("{} emitted a degraded event", self.source.id()),
            ));
        }
        if event.name.is_empty()
            || event.name.len() > MAX_EVENT_IDENTIFIER_BYTES
            || event
                .string
                .as_deref()
                .is_some_and(|value| value.len() > MAX_EVENT_STRING_BYTES)
            || event.diagnostic_codes.len() > MAX_DIAGNOSTIC_CODES
        {
            return Err(RehearsalFailure::new(
                "event_field_limit",
                format!("{} event exceeded a fixed field bound", self.source.id()),
            ));
        }
        if !event.float.is_finite() || !event.volume.is_finite() || !event.balance.is_finite() {
            return Err(RehearsalFailure::new(
                "event_value",
                format!("{} event contained a nonfinite value", self.source.id()),
            ));
        }
        if self.events.len() == MAX_EVENT_COUNT {
            return Err(RehearsalFailure::new(
                "event_limit",
                format!("{} exceeded the fixed event count", self.source.id()),
            ));
        }
        let captured = CapturedEvent {
            animation: ANIMATION_NAME,
            name: event.name,
            local_time_ns: event.local_time.as_nanos() as u64,
            loop_index: 0,
            integer: event.integer,
            float: f64::from(event.float),
            string: event.string,
            volume: f64::from(event.volume),
            balance: f64::from(event.balance),
            diagnostic_codes: event.diagnostic_codes,
        };
        let encoded = serde_json::to_vec(&captured)
            .map_err(|error| RehearsalFailure::new("event_encode", error.to_string()))?;
        self.encoded_event_bytes = self
            .encoded_event_bytes
            .checked_add(encoded.len())
            .ok_or_else(|| RehearsalFailure::new("event_limit", "event byte count overflowed"))?;
        if self.encoded_event_bytes > MAX_EVENT_WINDOW_BYTES {
            return Err(RehearsalFailure::new(
                "event_limit",
                format!("{} event bytes exceeded the fixed limit", self.source.id()),
            ));
        }
        self.previous_event_time = Some(event.local_time);
        self.events.push(captured);
        Ok(())
    }

    fn finish(self) -> Result<EventWindowDocument, RehearsalFailure> {
        #[derive(Serialize)]
        struct EventWindowDto {
            format_version: u16,
            window_id: &'static str,
            animation: &'static str,
            start_ns: u64,
            end_ns: u64,
            events: Vec<CapturedEvent>,
        }
        let bytes = serde_json::to_vec(&EventWindowDto {
            format_version: EVENT_WINDOW_FORMAT_VERSION,
            window_id: EVENT_WINDOW_ID,
            animation: ANIMATION_NAME,
            start_ns: EVENT_WINDOW_START_NS,
            end_ns: EVENT_WINDOW_END_NS,
            events: self.events,
        })
        .map_err(|error| RehearsalFailure::new("event_encode", error.to_string()))?;
        if bytes.len() > MAX_EVENT_WINDOW_BYTES {
            return Err(RehearsalFailure::new(
                "event_limit",
                "serialized event window exceeded the fixed limit",
            ));
        }
        parse_event_window_json(&bytes)
            .map_err(|error| RehearsalFailure::new("event_document", error.to_string()))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct CapturedEventWindows {
    current: EventWindowDocument,
    proposed: EventWindowDocument,
}

fn validate_frozen_capture_generations(
    source: CaptureSource,
    expected_frame_revision: u64,
    actual_frame_revision: u64,
    expectation: CaptureExpectation,
    acknowledged_play_revision: Option<u64>,
    acknowledged_seek_revision: Option<u64>,
) -> Result<(), RehearsalFailure> {
    if actual_frame_revision != expected_frame_revision
        || acknowledged_play_revision != Some(expectation.play_revision)
        || acknowledged_seek_revision != Some(expectation.seek_revision)
    {
        return Err(RehearsalFailure::new(
            "semantic_mutation",
            format!(
                "{} frozen semantic frame or command generation changed",
                source.id()
            ),
        ));
    }
    Ok(())
}

#[derive(Debug)]
enum MachineState {
    WaitingForChallenge,
    WaitingForReady {
        updates: usize,
    },
    ReadyToSpawnEventCapture,
    CapturingEventWindow {
        updates: usize,
    },
    ReadyToFinalizeEventWindow,
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
        accepted_frame_revisions: [Option<u64>; 2],
        pending: [Option<PendingReason>; 2],
    },
    ReadyToPresent {
        sample_index: usize,
        source_index: usize,
        expectations: [CaptureExpectation; 2],
        frame_revisions: [u64; 2],
    },
    HoldingPresentation {
        binding: PresentationBinding,
        held_updates: u8,
    },
    ReadyToRequest {
        binding: PresentationBinding,
    },
    AwaitingScreenshotAck {
        binding: PresentationBinding,
    },
    Complete,
    Failed(RehearsalFailure),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PresentationBinding {
    sequence: u8,
    sample_index: usize,
    source: CaptureSource,
    expectations: [CaptureExpectation; 2],
    frame_revisions: [u64; 2],
}

impl PresentationBinding {
    const fn expectation(self) -> CaptureExpectation {
        self.expectations[self.source.index()]
    }

    const fn frame_revision(self) -> u64 {
        self.frame_revisions[self.source.index()]
    }
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
            state: MachineState::WaitingForChallenge,
        }
    }
}

impl RehearsalMachine {
    fn accept_challenge(&mut self) {
        if !matches!(self.state, MachineState::WaitingForChallenge) {
            self.fail(
                "challenge_state",
                "a browser challenge was accepted from an invalid state",
            );
            return;
        }
        self.state = MachineState::WaitingForReady { updates: 0 };
    }

    fn update_readiness(&mut self, ready: bool) {
        let MachineState::WaitingForReady { updates } = &mut self.state else {
            return;
        };
        if ready {
            self.state = MachineState::ReadyToSpawnEventCapture;
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

    const fn event_capture_to_spawn(&self) -> bool {
        matches!(self.state, MachineState::ReadyToSpawnEventCapture)
    }

    fn note_event_capture_spawned(&mut self) {
        if !self.event_capture_to_spawn() {
            self.fail(
                "event_state",
                "hidden event instances were spawned from an invalid state",
            );
            return;
        }
        self.state = MachineState::CapturingEventWindow { updates: 0 };
    }

    const fn event_capture_active(&self) -> bool {
        matches!(self.state, MachineState::CapturingEventWindow { .. })
    }

    fn note_event_capture_update(&mut self, complete: bool) {
        let MachineState::CapturingEventWindow { updates } = &mut self.state else {
            self.fail(
                "event_state",
                "event playback was observed from an invalid state",
            );
            return;
        };
        *updates = updates.saturating_add(1);
        if complete {
            self.state = MachineState::ReadyToFinalizeEventWindow;
        } else if *updates >= EVENT_CAPTURE_UPDATE_LIMIT {
            self.fail(
                "event_timeout",
                format!(
                    "the hidden event pair did not complete within {EVENT_CAPTURE_UPDATE_LIMIT} updates"
                ),
            );
        }
    }

    const fn event_capture_to_finalize(&self) -> bool {
        matches!(self.state, MachineState::ReadyToFinalizeEventWindow)
    }

    fn note_event_capture_finalized(&mut self) {
        if !self.event_capture_to_finalize() {
            self.fail(
                "event_state",
                "event windows were finalized from an invalid state",
            );
            return;
        }
        self.state = MachineState::ReadyToIssue { sample_index: 0 };
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
            accepted_frame_revisions: [None; 2],
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
            accepted_frame_revisions,
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
                    accepted_frame_revisions[index] = Some(observed[index].frame_revision);
                    pending[index] = None;
                    newly_accepted[index] = true;
                }
                Err(reason) => pending[index] = Some(reason),
            }
        }
        *updates = updates.saturating_add(1);
        if accepted.iter().all(|accepted| *accepted) {
            self.state = MachineState::ReadyToPresent {
                sample_index: *sample_index,
                source_index: 0,
                expectations: *expectations,
                frame_revisions: [
                    accepted_frame_revisions[0].expect("accepted Current has a frame revision"),
                    accepted_frame_revisions[1].expect("accepted Proposed has a frame revision"),
                ],
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

    fn presentation_to_apply(&self) -> Option<PresentationBinding> {
        let MachineState::ReadyToPresent {
            sample_index,
            source_index,
            expectations,
            frame_revisions,
        } = self.state
        else {
            return None;
        };
        let source = CaptureSource::ALL[source_index];
        Some(PresentationBinding {
            sequence: u8::try_from(sample_index * 2 + source_index)
                .expect("the fixed sequence has eight entries"),
            sample_index,
            source,
            expectations,
            frame_revisions,
        })
    }

    fn bind_frozen_frame_revisions(&mut self, frame_revisions: [u64; 2]) {
        let MachineState::ReadyToPresent {
            frame_revisions: previously_bound,
            ..
        } = self.state
        else {
            self.fail(
                "presentation_state",
                "frozen semantic generations were bound from an invalid state",
            );
            return;
        };
        for source in CaptureSource::ALL {
            let index = source.index();
            if frame_revisions[index] < previously_bound[index] {
                self.fail(
                    "semantic_mutation",
                    format!(
                        "{} semantic frame generation moved backwards before presentation",
                        source.id()
                    ),
                );
                return;
            }
        }
        let MachineState::ReadyToPresent {
            frame_revisions: bound,
            ..
        } = &mut self.state
        else {
            unreachable!("the state was matched immediately above")
        };
        *bound = frame_revisions;
    }

    const fn animation_updates_enabled(&self) -> bool {
        !matches!(
            self.state,
            MachineState::ReadyToPresent { .. }
                | MachineState::HoldingPresentation { .. }
                | MachineState::ReadyToRequest { .. }
                | MachineState::AwaitingScreenshotAck { .. }
                | MachineState::ReadyToFinalizeEventWindow
                | MachineState::Complete
                | MachineState::Failed(_)
        )
    }

    fn note_presentation_applied(&mut self, binding: PresentationBinding) {
        if self.presentation_to_apply() != Some(binding) {
            self.fail(
                "presentation_state",
                "a source presentation was applied from an invalid state",
            );
            return;
        }
        self.state = MachineState::HoldingPresentation {
            binding,
            held_updates: 0,
        };
    }

    fn hold_presented_update(&mut self, binding: PresentationBinding) {
        let MachineState::HoldingPresentation {
            binding: expected,
            held_updates,
        } = &mut self.state
        else {
            self.fail(
                "presentation_state",
                "a presentation hold was recorded from an invalid state",
            );
            return;
        };
        if *expected != binding {
            self.fail(
                "presentation_mutation",
                "the held screenshot presentation binding changed",
            );
            return;
        }
        *held_updates = held_updates.saturating_add(1);
        if *held_updates == 2 {
            self.state = MachineState::ReadyToRequest { binding };
        }
    }

    fn request_to_publish(&self) -> Option<PresentationBinding> {
        match &self.state {
            MachineState::ReadyToRequest { binding } => Some(*binding),
            _other => None,
        }
    }

    fn note_request_published(&mut self, binding: PresentationBinding) {
        if self.request_to_publish() != Some(binding) {
            self.fail(
                "request_state",
                "a screenshot request was published from an invalid state",
            );
            return;
        }
        self.state = MachineState::AwaitingScreenshotAck { binding };
    }

    fn awaited_ack(&self) -> Option<PresentationBinding> {
        match &self.state {
            MachineState::AwaitingScreenshotAck { binding } => Some(*binding),
            _other => None,
        }
    }

    fn accept_screenshot_ack(&mut self, binding: PresentationBinding) {
        if self.awaited_ack() != Some(binding) {
            self.fail(
                "ack_state",
                "a screenshot acknowledgement did not match the pending request",
            );
            return;
        }
        let next_source = binding.source.index() + 1;
        if next_source < CaptureSource::ALL.len() {
            let MachineState::AwaitingScreenshotAck { .. } = self.state else {
                unreachable!("the awaited binding was checked above")
            };
            self.state = MachineState::ReadyToPresent {
                sample_index: binding.sample_index,
                source_index: next_source,
                expectations: binding.expectations,
                frame_revisions: binding.frame_revisions,
            };
            return;
        }
        let next_sample = binding.sample_index + 1;
        self.state = if next_sample == FIXED_SAMPLES.len() {
            MachineState::Complete
        } else {
            MachineState::ReadyToIssue {
                sample_index: next_sample,
            }
        };
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

#[derive(Serialize)]
struct CompleteDocument<'a, Capture: Serialize> {
    format_version: u8,
    state: &'static str,
    browser_capture: &'a Capture,
    event_windows: &'a CapturedEventWindows,
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

fn encode_complete<Capture: Serialize>(
    browser_capture: &Capture,
    event_windows: &CapturedEventWindows,
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
        format_version: 3,
        state: "complete",
        browser_capture,
        event_windows,
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
        format_version: 3,
        state: "error",
        error: ErrorDetail {
            kind,
            message: &message,
        },
    })
    .unwrap_or_else(|_error| {
        br#"{"format_version":3,"state":"error","error":{"kind":"encode_error","message":"could not encode rehearsal error"}}"#.to_vec()
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
    use bevy::{
        camera::Projection,
        ecs::message::{MessageCursor, Messages},
        prelude::*,
        window::PrimaryWindow,
    };
    use bevy_spinal::{
        SpinalAnimationEvent, SpinalAnimator, SpinalInstance, SpinalPlaybackState,
        SpinalSemanticCapture, SpinalSet, SpinalSkinLayers,
        spinal::{PlaybackMode, Transition},
    };
    use spinal_phase0b::browser_capture::{
        BrowserCaptureComplete, BrowserCaptureProgress, BrowserCaptureSession,
        BrowserControlMessage, CaptureSource as ProtocolSource, DriverMessage, RuntimeIdentity,
        RuntimeSources, ScreenshotPresentation, parse_driver_message,
    };
    use wasm_bindgen::{JsCast, JsValue};
    use web_sys::HtmlCanvasElement;

    use super::*;
    use crate::{
        command::{SkinSelection, ViewerCommand},
        runtime::{CommandInbox, ViewerLoadState, ViewerRuntime, ViewerRuntimeSet},
        session::SourceSlot,
        viewport::{Phase0bViewportControl, Phase0bViewportSet},
    };

    const APP_STYLE: &str = "display:block;width:640px;height:480px;min-height:0;overflow:hidden";
    const ROOT_STYLE: &str = "display:block;width:640px;height:480px;min-width:0;min-height:0;margin:0;padding:0;overflow:hidden;background:#07090d";
    const PREVIEW_STYLE: &str =
        "display:block;width:640px;height:480px;min-height:0;margin:0;padding:0;overflow:hidden";
    const FRAME_STYLE: &str = "display:block;width:640px;height:480px;min-height:0;margin:0;padding:0;border:0;border-radius:0;overflow:hidden;background:#07090d";
    const CANVAS_STYLE: &str = "display:block;width:640px;height:480px;margin:0;padding:0;border:0";
    const HIDDEN_STYLE: &str = "display:none";

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct PresentationSnapshot {
        binding: PresentationBinding,
        source_transforms: [[u32; 16]; 2],
        camera_states: [CameraState; 2],
        semantic_json: [Box<[u8]>; 2],
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct CameraState {
        entity: Entity,
        source: CaptureSource,
        active: bool,
        order: isize,
        viewport_position: UVec2,
        viewport_size: UVec2,
        transform: [u32; 16],
        projection: Box<str>,
    }

    #[derive(Debug)]
    struct EventCaptureRun {
        entities: [Entity; 2],
        cursor: MessageCursor<SpinalAnimationEvent>,
        sources: [EventSourceAccumulator; 2],
    }

    #[derive(Resource, Debug)]
    struct BrowserRehearsal {
        machine: RehearsalMachine,
        observations: Vec<CapturedObservation>,
        event_capture: Option<EventCaptureRun>,
        event_windows: Option<CapturedEventWindows>,
        capture_session: Option<BrowserCaptureSession>,
        capture_complete: Option<BrowserCaptureComplete>,
        last_inbound: Option<Box<str>>,
        expected_outbound: Option<Box<str>>,
        presentation_snapshot: Option<PresentationSnapshot>,
        terminal_published: bool,
    }

    impl Default for BrowserRehearsal {
        fn default() -> Self {
            Self {
                machine: RehearsalMachine::default(),
                observations: Vec::with_capacity(8),
                event_capture: None,
                event_windows: None,
                capture_session: None,
                capture_complete: None,
                last_inbound: None,
                expected_outbound: None,
                presentation_snapshot: None,
                terminal_published: false,
            }
        }
    }

    pub(crate) fn initialize_dom() -> Result<(), String> {
        let document = web_sys::window()
            .and_then(|window| window.document())
            .ok_or_else(|| "document is unavailable".to_owned())?;
        if document.get_element_by_id(OUTPUT_ELEMENT_ID).is_some()
            || document.get_element_by_id(CONTROL_ELEMENT_ID).is_some()
        {
            return Err("the page already contains a reserved Phase 0B element".to_owned());
        }
        let app = document
            .get_element_by_id("spinal-app")
            .ok_or_else(|| "the viewer application element is unavailable".to_owned())?;
        apply_capture_layout(&document)?;
        let control = document
            .create_element("script")
            .map_err(|_| "could not create the rehearsal control element".to_owned())?;
        control
            .set_attribute("id", CONTROL_ELEMENT_ID)
            .and_then(|()| control.set_attribute("type", "application/json"))
            .map_err(|_| "could not configure the rehearsal control element".to_owned())?;
        let output = document
            .create_element("script")
            .map_err(|_| "could not create the rehearsal output element".to_owned())?;
        output
            .set_attribute("id", OUTPUT_ELEMENT_ID)
            .and_then(|()| output.set_attribute("type", "application/json"))
            .and_then(|()| output.set_attribute(COMPLETE_ATTRIBUTE, "false"))
            .and_then(|()| output.set_attribute(STATE_ATTRIBUTE, "running"))
            .map_err(|_| "could not configure the rehearsal output element".to_owned())?;
        output.set_text_content(Some(r#"{"format_version":3,"state":"running"}"#));
        app.append_child(&control)
            .and_then(|_node| app.append_child(&output))
            .map_err(|_| "could not attach the rehearsal output element".to_owned())?;
        Ok(())
    }

    pub(crate) fn publish_external_error(kind: &str, message: &str) {
        publish_terminal_bytes("error", &bounded_error_json(kind, message));
    }

    pub(crate) fn install(app: &mut App) {
        app.init_resource::<BrowserRehearsal>()
            .configure_sets(
                Update,
                SpinalSet::Animate.run_if(phase0b_animation_updates_enabled),
            )
            .add_systems(
                Startup,
                attach_semantic_capture.after(ViewerRuntimeSet::Setup),
            )
            .add_systems(
                Update,
                poll_control_input
                    .after(ViewerRuntimeSet::Poll)
                    .before(issue_sample_commands),
            )
            .add_systems(
                Update,
                issue_sample_commands.before(ViewerRuntimeSet::Commands),
            )
            .add_systems(
                Update,
                begin_event_capture
                    .after(issue_sample_commands)
                    .before(ViewerRuntimeSet::Commands),
            )
            .add_systems(
                Update,
                capture_event_window
                    .after(ViewerRuntimeSet::Observe)
                    .before(observe_semantic_frames),
            )
            .add_systems(Update, drain_animation_events.after(capture_event_window))
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
                begin_presentation
                    .after(observe_semantic_frames)
                    .before(Phase0bViewportSet),
            )
            .add_systems(
                Update,
                advance_presentation
                    .after(Phase0bViewportSet)
                    .after(crate::camera_fit::ViewerCameraFitSet),
            )
            .add_systems(Update, publish_terminal_output.after(advance_presentation));
    }

    fn phase0b_animation_updates_enabled(rehearsal: Res<'_, BrowserRehearsal>) -> bool {
        rehearsal.machine.animation_updates_enabled()
    }

    fn apply_capture_layout(document: &web_sys::Document) -> Result<(), String> {
        let root = document
            .document_element()
            .ok_or_else(|| "document root is unavailable".to_owned())?;
        let body = document
            .body()
            .ok_or_else(|| "document body is unavailable".to_owned())?;
        root.set_attribute("style", ROOT_STYLE)
            .and_then(|()| body.set_attribute("style", ROOT_STYLE))
            .map_err(|_| "could not fix the capture document surface".to_owned())?;
        set_element_style(document, "#spinal-app", APP_STYLE)?;
        set_element_style(document, ".preview-region", PREVIEW_STYLE)?;
        set_element_style(document, ".canvas-frame", FRAME_STYLE)?;
        set_element_style(document, "#spinal-canvas", CANVAS_STYLE)?;
        for selector in [
            ".app-header",
            "#preview-heading",
            "#spinal-camera-help",
            "#spinal-source-labels",
            "#spinal-transport",
            "#spinal-diagnostics",
        ] {
            set_element_style(document, selector, HIDDEN_STYLE)?;
        }
        let canvas = document
            .get_element_by_id("spinal-canvas")
            .and_then(|element| element.dyn_into::<HtmlCanvasElement>().ok())
            .ok_or_else(|| "the capture canvas is unavailable".to_owned())?;
        canvas.set_width(640);
        canvas.set_height(480);
        Ok(())
    }

    fn set_element_style(
        document: &web_sys::Document,
        selector: &str,
        style: &str,
    ) -> Result<(), String> {
        let element = document
            .query_selector(selector)
            .map_err(|_| format!("could not query capture element `{selector}`"))?
            .ok_or_else(|| format!("capture element `{selector}` is unavailable"))?;
        element
            .set_attribute("style", style)
            .map_err(|_| format!("could not configure capture element `{selector}`"))
    }

    fn poll_control_input(
        runtime: Res<'_, ViewerRuntime>,
        windows: Query<'_, '_, &Window, With<PrimaryWindow>>,
        cameras: Query<
            '_,
            '_,
            (
                Entity,
                &crate::camera_fit::PreviewCamera,
                &Camera,
                &Transform,
                &Projection,
            ),
        >,
        transforms: Query<'_, '_, &Transform>,
        animators: Query<'_, '_, (&SpinalAnimator, &SpinalSkinLayers, &SpinalSemanticCapture)>,
        mut viewport: ResMut<'_, Phase0bViewportControl>,
        mut rehearsal: ResMut<'_, BrowserRehearsal>,
    ) {
        if rehearsal.terminal_published {
            return;
        }
        if rehearsal.machine.failure().is_none()
            && !rehearsal.machine.is_complete()
            && external_terminal_was_published()
        {
            rehearsal.machine.fail(
                "external_terminal",
                "an external browser failure terminated the rehearsal",
            );
            return;
        }
        if rehearsal.machine.failure().is_some() {
            return;
        }
        let result = poll_control_input_inner(
            &runtime,
            &windows,
            &cameras,
            &transforms,
            &animators,
            &mut viewport,
            &mut rehearsal,
        );
        if let Err(failure) = result {
            rehearsal.machine.fail(failure.kind, failure.message);
        }
    }

    fn external_terminal_was_published() -> bool {
        web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.get_element_by_id(OUTPUT_ELEMENT_ID))
            .and_then(|output| output.get_attribute(COMPLETE_ATTRIBUTE))
            .as_deref()
            == Some("true")
    }

    fn poll_control_input_inner(
        runtime: &ViewerRuntime,
        windows: &Query<'_, '_, &Window, With<PrimaryWindow>>,
        cameras: &Query<
            '_,
            '_,
            (
                Entity,
                &crate::camera_fit::PreviewCamera,
                &Camera,
                &Transform,
                &Projection,
            ),
        >,
        transforms: &Query<'_, '_, &Transform>,
        animators: &Query<'_, '_, (&SpinalAnimator, &SpinalSkinLayers, &SpinalSemanticCapture)>,
        viewport: &mut Phase0bViewportControl,
        rehearsal: &mut BrowserRehearsal,
    ) -> Result<(), RehearsalFailure> {
        let document = web_sys::window()
            .and_then(|window| window.document())
            .ok_or_else(|| RehearsalFailure::new("control_dom", "document is unavailable"))?;
        let control = document
            .get_element_by_id(CONTROL_ELEMENT_ID)
            .ok_or_else(|| {
                RehearsalFailure::new("control_dom", "the reserved control element was removed")
            })?;
        match (
            &rehearsal.expected_outbound,
            control.get_attribute(OUTBOUND_ATTRIBUTE),
        ) {
            (None, None) => {}
            (Some(expected), Some(actual)) if expected.as_ref() == actual => {}
            _other => {
                return Err(RehearsalFailure::new(
                    "outbound_mutation",
                    "the atomic browser control output was changed",
                ));
            }
        }
        let inbound = control.get_attribute(INBOUND_ATTRIBUTE);
        if rehearsal.last_inbound.is_some() && inbound.is_none() {
            return Err(RehearsalFailure::new(
                "inbound_mutation",
                "the atomic browser control input was removed",
            ));
        }
        let Some(inbound) = inbound else {
            return Ok(());
        };
        if rehearsal.last_inbound.as_deref() == Some(inbound.as_str()) {
            return Ok(());
        }
        let message = parse_driver_message(inbound.as_bytes())
            .map_err(|error| RehearsalFailure::new("invalid_driver_message", error.to_string()))?;
        rehearsal.last_inbound = Some(inbound.into());

        match message {
            DriverMessage::Challenge { .. } => {
                if rehearsal.capture_session.is_some() {
                    return Err(RehearsalFailure::new(
                        "rewritten_challenge",
                        "the browser challenge was rewritten after acceptance",
                    ));
                }
                let mut session = BrowserCaptureSession::new(captured_runtime_sources(runtime)?);
                let response = session.accept_challenge(message).map_err(|error| {
                    RehearsalFailure::new("challenge_rejected", error.to_string())
                })?;
                // Winit's `fit_canvas_to_parent` setup intentionally writes
                // 100% canvas dimensions after the early DOM reservation.
                // The driver has fixed device metrics before issuing this
                // challenge, so normalize once here and reject every later
                // layout mutation through `validate_capture_surface`.
                apply_capture_layout(&document)
                    .map_err(|message| RehearsalFailure::new("capture_surface", message))?;
                publish_control_message(&control, &response, rehearsal)?;
                rehearsal.capture_session = Some(session);
                rehearsal.machine.accept_challenge();
                set_output_state("awaiting_capture");
            }
            DriverMessage::ScreenshotAck { .. } => {
                let binding = rehearsal.machine.awaited_ack().ok_or_else(|| {
                    RehearsalFailure::new(
                        "ack_before_request",
                        "a screenshot acknowledgement arrived before an outstanding request",
                    )
                })?;
                validate_presentation(
                    runtime, windows, cameras, transforms, animators, viewport, rehearsal, binding,
                )?;
                let progress = rehearsal
                    .capture_session
                    .as_mut()
                    .ok_or_else(|| {
                        RehearsalFailure::new("missing_session", "capture session is unavailable")
                    })?
                    .accept_screenshot_ack(message)
                    .map_err(|error| RehearsalFailure::new("ack_rejected", error.to_string()))?;
                viewport.release();
                rehearsal.presentation_snapshot = None;
                rehearsal.machine.accept_screenshot_ack(binding);
                if let BrowserCaptureProgress::Complete(complete) = progress {
                    let bytes = complete.to_json().map_err(|error| {
                        RehearsalFailure::new("encode_error", error.to_string())
                    })?;
                    publish_control_bytes(&control, &bytes, rehearsal)?;
                    rehearsal.capture_complete = Some(complete);
                }
            }
        }
        Ok(())
    }

    fn attach_semantic_capture(mut commands: Commands<'_, '_>, runtime: Res<'_, ViewerRuntime>) {
        for source in runtime.sources() {
            commands
                .entity(source.entity())
                .insert(SpinalSemanticCapture::default());
        }
    }

    fn begin_event_capture(
        mut commands: Commands<'_, '_>,
        runtime: Res<'_, ViewerRuntime>,
        messages: Res<'_, Messages<SpinalAnimationEvent>>,
        mut rehearsal: ResMut<'_, BrowserRehearsal>,
    ) {
        if !rehearsal.machine.event_capture_to_spawn() {
            return;
        }
        if rehearsal.event_capture.is_some() || rehearsal.event_windows.is_some() {
            rehearsal.machine.fail(
                "event_state",
                "event capture storage was already initialized",
            );
            return;
        }
        let handles = match (
            runtime.source(SourceSlot::Primary),
            runtime.source(SourceSlot::Comparison),
        ) {
            (Some(current), Some(proposed)) if runtime.sources().len() == 2 => {
                [current.asset().clone(), proposed.asset().clone()]
            }
            _other => {
                rehearsal.machine.fail(
                    "missing_comparison",
                    "the event pass requires exactly Current and Proposed sources",
                );
                return;
            }
        };
        // This cursor is the exact admission boundary. It is established before
        // either hidden entity exists, independently of the always-draining reader.
        let cursor = messages.get_cursor_current();
        let entities = handles.map(|asset| {
            commands
                .spawn((
                    SpinalInstance::new(asset),
                    SpinalAnimator::once(ANIMATION_NAME),
                    Visibility::Hidden,
                ))
                .id()
        });
        rehearsal.event_capture = Some(EventCaptureRun {
            entities,
            cursor,
            sources: [
                EventSourceAccumulator::new(CaptureSource::Current, entities[0].to_bits()),
                EventSourceAccumulator::new(CaptureSource::Proposed, entities[1].to_bits()),
            ],
        });
        rehearsal.machine.note_event_capture_spawned();
    }

    fn drain_animation_events(mut events: MessageReader<'_, '_, SpinalAnimationEvent>) {
        events.read().for_each(drop);
    }

    fn validate_event_source(
        source: &mut EventSourceAccumulator,
        animator: &SpinalAnimator,
        playback: &SpinalPlaybackState,
    ) -> Result<bool, RehearsalFailure> {
        if animator.animation() != Some(ANIMATION_NAME)
            || animator.mode() != Some(PlaybackMode::Once)
            || animator.transition() != Transition::Immediate
            || animator.is_paused()
            || animator.speed().to_bits() != 1.0_f32.to_bits()
            || animator.revision() != 1
            || animator.seek_revision() != 0
            || animator.seek_position().is_some()
        {
            return Err(RehearsalFailure::new(
                "event_intent",
                format!("{} hidden animator intent changed", source.source.id()),
            ));
        }
        let Some(playback_id) = playback.playback() else {
            if playback.animation().is_none()
                && playback.mode().is_none()
                && playback.position().is_none()
                && playback.loop_index().is_none()
                && !playback.is_complete()
            {
                return Ok(false);
            }
            return Err(RehearsalFailure::new(
                "event_playback",
                format!(
                    "{} hidden playback was only partially initialized",
                    source.source.id()
                ),
            ));
        };
        source.bind_playback(playback_id)?;
        if playback.animation() != Some(ANIMATION_NAME)
            || playback.mode() != Some(PlaybackMode::Once)
            || playback.loop_index() != Some(0)
        {
            return Err(RehearsalFailure::new(
                "event_playback",
                format!(
                    "{} hidden playback changed animation, mode, or loop",
                    source.source.id()
                ),
            ));
        }
        let position = playback.position().ok_or_else(|| {
            RehearsalFailure::new(
                "event_playback",
                format!("{} hidden playback has no position", source.source.id()),
            )
        })?;
        source.observe_playback(position, playback.is_complete())?;
        Ok(playback.is_complete())
    }

    fn observed_event(event: &SpinalAnimationEvent) -> ObservedEvent {
        ObservedEvent {
            entity_bits: event.entity().to_bits(),
            track: event.track().map(Into::into),
            playback: event.playback(),
            animation: event.animation().into(),
            name: event.event().into(),
            loop_index: event.loop_index(),
            local_time: event.local_time(),
            integer: event.integer(),
            float: event.float(),
            string: event.string().map(Into::into),
            volume: event.volume(),
            balance: event.balance(),
            diagnostic_codes: event.diagnostic_codes().to_vec(),
            degraded: event.is_degraded(),
        }
    }

    fn capture_event_window(
        mut commands: Commands<'_, '_>,
        messages: Res<'_, Messages<SpinalAnimationEvent>>,
        playbacks: Query<'_, '_, (&SpinalAnimator, &SpinalPlaybackState)>,
        mut rehearsal: ResMut<'_, BrowserRehearsal>,
    ) {
        if !rehearsal.machine.event_capture_active() {
            return;
        }
        let result = (|| {
            let run = rehearsal.event_capture.as_mut().ok_or_else(|| {
                RehearsalFailure::new("event_state", "hidden event capture storage is missing")
            })?;
            let mut complete = [false; 2];
            for source in CaptureSource::ALL {
                let index = source.index();
                match playbacks.get(run.entities[index]) {
                    Ok((animator, playback)) => {
                        complete[index] =
                            validate_event_source(&mut run.sources[index], animator, playback)?;
                    }
                    Err(_error) if run.sources[index].playback.is_none() => {}
                    Err(_error) => {
                        return Err(RehearsalFailure::new(
                            "event_entity",
                            format!("{} hidden event entity disappeared", source.id()),
                        ));
                    }
                }
            }

            // Clone the newly admitted suffix so the cursor borrow ends before
            // source accumulators are mutated. This includes events emitted by
            // the exact update whose playback states were just observed.
            let events = run.cursor.read(&messages).cloned().collect::<Vec<_>>();
            for event in events {
                let Some(index) = run
                    .entities
                    .iter()
                    .position(|entity| *entity == event.entity())
                else {
                    // The cursor is global, but only the two hidden capture
                    // entities are admitted to this observation. Visible viewer
                    // traffic is intentionally outside the event-window boundary.
                    continue;
                };
                run.sources[index].push(observed_event(&event))?;
            }
            Ok(complete.into_iter().all(|complete| complete))
        })();

        let complete = match result {
            Ok(complete) => complete,
            Err(failure) => {
                if let Some(run) = rehearsal.event_capture.take() {
                    for entity in run.entities {
                        commands.entity(entity).despawn();
                    }
                }
                rehearsal.machine.fail(failure.kind, failure.message);
                return;
            }
        };
        rehearsal.machine.note_event_capture_update(complete);
        if !rehearsal.machine.event_capture_to_finalize() {
            return;
        }

        let run = rehearsal
            .event_capture
            .take()
            .expect("the completed event pass retains its hidden entities");
        for entity in run.entities {
            commands.entity(entity).despawn();
        }
        let [current, proposed] = run.sources;
        let windows = current.finish().and_then(|current| {
            proposed
                .finish()
                .map(|proposed| CapturedEventWindows { current, proposed })
        });
        match windows {
            Ok(windows) => {
                rehearsal.event_windows = Some(windows);
                rehearsal.machine.note_event_capture_finalized();
            }
            Err(failure) => rehearsal.machine.fail(failure.kind, failure.message),
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
        if rehearsal.machine.presentation_to_apply().is_some()
            && let Err(failure) =
                bind_frozen_semantic_generations(&captures, entities, &mut rehearsal)
        {
            rehearsal.machine.fail(failure.kind, failure.message);
        }
    }

    fn bind_frozen_semantic_generations(
        captures: &Query<'_, '_, &SpinalSemanticCapture>,
        entities: [Entity; 2],
        rehearsal: &mut BrowserRehearsal,
    ) -> Result<(), RehearsalFailure> {
        let binding = rehearsal.machine.presentation_to_apply().ok_or_else(|| {
            RehearsalFailure::new(
                "presentation_state",
                "semantic generations were frozen from an invalid state",
            )
        })?;
        let sample = FIXED_SAMPLES[binding.sample_index];
        let mut observation_indices = [0_usize; 2];
        let mut frame_revisions = [0_u64; 2];
        let mut frames = Vec::with_capacity(2);

        for source in CaptureSource::ALL {
            let index = source.index();
            let expectation = binding.expectations[index];
            let capture = captures.get(entities[index]).map_err(|_error| {
                RehearsalFailure::new(
                    "missing_component",
                    format!(
                        "{} lost SpinalSemanticCapture before presentation",
                        source.id()
                    ),
                )
            })?;
            let frame = capture.frame().ok_or_else(|| {
                RehearsalFailure::new(
                    "missing_frame",
                    format!("{} lost its accepted semantic frame", source.id()),
                )
            })?;
            if capture.acknowledged_play_revision() != Some(expectation.play_revision)
                || capture.acknowledged_seek_revision() != Some(expectation.seek_revision)
                || capture.frame_revision() < binding.frame_revisions[index]
            {
                return Err(RehearsalFailure::new(
                    "semantic_mutation",
                    format!(
                        "{} semantic generations changed before the pair could be frozen",
                        source.id()
                    ),
                ));
            }
            let matching = rehearsal
                .observations
                .iter()
                .enumerate()
                .filter(|(_observation_index, observation)| {
                    observation.source == source.id() && observation.sample == sample.id()
                })
                .map(|(observation_index, _observation)| observation_index)
                .collect::<Vec<_>>();
            let [observation_index] = matching.as_slice() else {
                return Err(RehearsalFailure::new(
                    "missing_observation",
                    format!(
                        "{} must have exactly one accepted {} observation",
                        source.id(),
                        sample.id()
                    ),
                ));
            };
            let accepted = &rehearsal.observations[*observation_index];
            let accepted_json = accepted
                .frame
                .to_canonical_json()
                .map_err(|error| RehearsalFailure::new("encode_error", error.to_string()))?;
            let frozen_json = frame
                .to_canonical_json()
                .map_err(|error| RehearsalFailure::new("encode_error", error.to_string()))?;
            if accepted.frame_revision != binding.frame_revisions[index]
                || accepted.acknowledged_play_revision != expectation.play_revision
                || accepted.acknowledged_seek_revision != expectation.seek_revision
                || accepted_json != frozen_json
            {
                return Err(RehearsalFailure::new(
                    "semantic_mutation",
                    format!(
                        "{} semantic bytes or acknowledgements changed before presentation",
                        source.id()
                    ),
                ));
            }
            observation_indices[index] = *observation_index;
            frame_revisions[index] = capture.frame_revision();
            frames.push(frame.clone());
        }

        for source in CaptureSource::ALL {
            let index = source.index();
            let observation = &mut rehearsal.observations[observation_indices[index]];
            observation.frame_revision = frame_revisions[index];
            observation.frame = frames[index].clone();
        }
        rehearsal
            .machine
            .bind_frozen_frame_revisions(frame_revisions);
        Ok(())
    }

    fn begin_presentation(
        mut viewport: ResMut<'_, Phase0bViewportControl>,
        mut rehearsal: ResMut<'_, BrowserRehearsal>,
    ) {
        let Some(binding) = rehearsal.machine.presentation_to_apply() else {
            return;
        };
        let source = source_slot(binding.source);
        if let Err(message) = viewport.request(source) {
            rehearsal.machine.fail("presentation_state", message);
        }
    }

    fn advance_presentation(
        runtime: Res<'_, ViewerRuntime>,
        windows: Query<'_, '_, &Window, With<PrimaryWindow>>,
        cameras: Query<
            '_,
            '_,
            (
                Entity,
                &crate::camera_fit::PreviewCamera,
                &Camera,
                &Transform,
                &Projection,
            ),
        >,
        transforms: Query<'_, '_, &Transform>,
        animators: Query<'_, '_, (&SpinalAnimator, &SpinalSkinLayers, &SpinalSemanticCapture)>,
        viewport: Res<'_, Phase0bViewportControl>,
        mut rehearsal: ResMut<'_, BrowserRehearsal>,
    ) {
        if rehearsal.machine.failure().is_some() || rehearsal.machine.is_complete() {
            return;
        }
        let binding =
            rehearsal
                .machine
                .presentation_to_apply()
                .or_else(|| match &rehearsal.machine.state {
                    MachineState::HoldingPresentation { binding, .. }
                    | MachineState::ReadyToRequest { binding }
                    | MachineState::AwaitingScreenshotAck { binding } => Some(*binding),
                    _other => None,
                });
        let Some(binding) = binding else {
            return;
        };
        let result = advance_presentation_inner(
            &runtime,
            &windows,
            &cameras,
            &transforms,
            &animators,
            &viewport,
            &mut rehearsal,
            binding,
        );
        if let Err(failure) = result {
            rehearsal.machine.fail(failure.kind, failure.message);
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the pure transition receives explicit read-only Bevy query views"
    )]
    fn advance_presentation_inner(
        runtime: &ViewerRuntime,
        windows: &Query<'_, '_, &Window, With<PrimaryWindow>>,
        cameras: &Query<
            '_,
            '_,
            (
                Entity,
                &crate::camera_fit::PreviewCamera,
                &Camera,
                &Transform,
                &Projection,
            ),
        >,
        transforms: &Query<'_, '_, &Transform>,
        animators: &Query<'_, '_, (&SpinalAnimator, &SpinalSkinLayers, &SpinalSemanticCapture)>,
        viewport: &Phase0bViewportControl,
        rehearsal: &mut BrowserRehearsal,
        binding: PresentationBinding,
    ) -> Result<(), RehearsalFailure> {
        if rehearsal.machine.presentation_to_apply() == Some(binding) {
            if !viewport.is_applied(source_slot(binding.source)) {
                return Ok(());
            }
            rehearsal.machine.note_presentation_applied(binding);
            return Ok(());
        }

        if matches!(
            rehearsal.machine.state,
            MachineState::HoldingPresentation { .. }
        ) {
            if rehearsal.presentation_snapshot.is_none() {
                let snapshot = capture_presentation(
                    runtime, windows, cameras, transforms, animators, viewport, rehearsal, binding,
                )?;
                rehearsal.presentation_snapshot = Some(snapshot);
            } else {
                validate_presentation(
                    runtime, windows, cameras, transforms, animators, viewport, rehearsal, binding,
                )?;
            }
            rehearsal.machine.hold_presented_update(binding);
        } else {
            validate_presentation(
                runtime, windows, cameras, transforms, animators, viewport, rehearsal, binding,
            )?;
        }
        if rehearsal.machine.request_to_publish() == Some(binding) {
            let expectation = binding.expectation();
            let presentation = ScreenshotPresentation::new(
                protocol_source(binding.source),
                FIXED_SAMPLES[binding.sample_index],
                binding.frame_revision(),
                expectation.play_revision,
                expectation.seek_revision,
            )
            .map_err(|error| RehearsalFailure::new("request_binding", error.to_string()))?;
            let message = rehearsal
                .capture_session
                .as_mut()
                .ok_or_else(|| {
                    RehearsalFailure::new("missing_session", "capture session is unavailable")
                })?
                .request_screenshot(presentation)
                .map_err(|error| RehearsalFailure::new("request_rejected", error.to_string()))?;
            let document = web_sys::window()
                .and_then(|window| window.document())
                .ok_or_else(|| RehearsalFailure::new("control_dom", "document is unavailable"))?;
            let control = document
                .get_element_by_id(CONTROL_ELEMENT_ID)
                .ok_or_else(|| {
                    RehearsalFailure::new("control_dom", "the reserved control element was removed")
                })?;
            publish_control_message(&control, &message, rehearsal)?;
            rehearsal.machine.note_request_published(binding);
        }
        Ok(())
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "validation keeps every mutable and read-only dependency explicit"
    )]
    fn validate_presentation(
        runtime: &ViewerRuntime,
        windows: &Query<'_, '_, &Window, With<PrimaryWindow>>,
        cameras: &Query<
            '_,
            '_,
            (
                Entity,
                &crate::camera_fit::PreviewCamera,
                &Camera,
                &Transform,
                &Projection,
            ),
        >,
        transforms: &Query<'_, '_, &Transform>,
        animators: &Query<'_, '_, (&SpinalAnimator, &SpinalSkinLayers, &SpinalSemanticCapture)>,
        viewport: &Phase0bViewportControl,
        rehearsal: &BrowserRehearsal,
        binding: PresentationBinding,
    ) -> Result<(), RehearsalFailure> {
        let actual = capture_presentation(
            runtime, windows, cameras, transforms, animators, viewport, rehearsal, binding,
        )?;
        let expected = rehearsal.presentation_snapshot.as_ref().ok_or_else(|| {
            RehearsalFailure::new(
                "missing_snapshot",
                "the held presentation snapshot is unavailable",
            )
        })?;
        if &actual != expected {
            return Err(RehearsalFailure::new(
                "presentation_mutation",
                "the camera, viewport, runtime pose, or semantic frame changed while awaiting capture",
            ));
        }
        Ok(())
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the snapshot binds all independent Bevy and protocol state"
    )]
    fn capture_presentation(
        runtime: &ViewerRuntime,
        windows: &Query<'_, '_, &Window, With<PrimaryWindow>>,
        cameras: &Query<
            '_,
            '_,
            (
                Entity,
                &crate::camera_fit::PreviewCamera,
                &Camera,
                &Transform,
                &Projection,
            ),
        >,
        transforms: &Query<'_, '_, &Transform>,
        animators: &Query<'_, '_, (&SpinalAnimator, &SpinalSkinLayers, &SpinalSemanticCapture)>,
        viewport: &Phase0bViewportControl,
        rehearsal: &BrowserRehearsal,
        binding: PresentationBinding,
    ) -> Result<PresentationSnapshot, RehearsalFailure> {
        validate_capture_surface(windows)?;
        if let Some(violation) = viewport.violation() {
            return Err(RehearsalFailure::new(
                "viewport_mutation",
                violation.to_owned(),
            ));
        }
        if !viewport.is_applied(source_slot(binding.source)) {
            return Err(RehearsalFailure::new(
                "viewport_mutation",
                "the requested source is not the only active full-viewport camera",
            ));
        }
        let sample = FIXED_SAMPLES[binding.sample_index];
        if runtime.model().transport().selected_animation() != Some(ANIMATION_NAME)
            || runtime.model().transport().is_looping()
            || !runtime.model().transport().is_paused()
            || runtime.model().transport().position() != sample.time()
            || runtime.model().selected_skin().name() != sample.skin_layers().first().copied()
        {
            return Err(RehearsalFailure::new(
                "runtime_mutation",
                "the fixed viewer transport or skin state changed during presentation",
            ));
        }
        let entities = source_entities(runtime)?;
        let mut semantic_json = Vec::with_capacity(2);
        for source in CaptureSource::ALL {
            let index = source.index();
            let expectation = binding.expectations[index];
            let (animator, skins, capture) = animators.get(entities[index]).map_err(|_error| {
                RehearsalFailure::new(
                    "missing_component",
                    format!("{} lost runtime controls", source.id()),
                )
            })?;
            if animator.animation() != Some(ANIMATION_NAME)
                || animator.mode() != Some(PlaybackMode::Once)
                || !animator.is_paused()
                || animator.seek_position() != Some(sample.time())
                || animator.revision() != expectation.play_revision
                || animator.seek_revision() != expectation.seek_revision
                || !skins.iter().eq(sample.skin_layers().iter().copied())
            {
                return Err(RehearsalFailure::new(
                    "runtime_mutation",
                    format!("{} command generations or pose intent changed", source.id()),
                ));
            }
            validate_frozen_capture_generations(
                source,
                binding.frame_revisions[index],
                capture.frame_revision(),
                expectation,
                capture.acknowledged_play_revision(),
                capture.acknowledged_seek_revision(),
            )?;
            let frame = capture.frame().ok_or_else(|| {
                RehearsalFailure::new(
                    "missing_frame",
                    format!("{} semantic frame is unavailable", source.id()),
                )
            })?;
            let canonical = frame
                .to_canonical_json()
                .map_err(|error| RehearsalFailure::new("encode_error", error.to_string()))?;
            let accepted = rehearsal
                .observations
                .iter()
                .find(|observation| {
                    observation.source == source.id() && observation.sample == sample.id()
                })
                .ok_or_else(|| {
                    RehearsalFailure::new(
                        "missing_observation",
                        format!(
                            "{} accepted semantic observation is unavailable",
                            source.id()
                        ),
                    )
                })?;
            if accepted.frame_revision != binding.frame_revisions[index]
                || accepted.acknowledged_play_revision != expectation.play_revision
                || accepted.acknowledged_seek_revision != expectation.seek_revision
                || accepted.frame.to_canonical_json().ok().as_deref() != Some(&canonical)
            {
                return Err(RehearsalFailure::new(
                    "semantic_mutation",
                    format!("{} semantic frame changed after acceptance", source.id()),
                ));
            }
            semantic_json.push(canonical.into_boxed_slice());
        }
        let semantic_json: [Box<[u8]>; 2] = semantic_json
            .try_into()
            .expect("the closed source loop produces exactly two frames");

        let mut camera_states = cameras
            .iter()
            .map(|(entity, marker, camera, transform, projection)| {
                let viewport = camera.viewport.as_ref().ok_or_else(|| {
                    RehearsalFailure::new("viewport_mutation", "a source camera lost its viewport")
                })?;
                Ok(CameraState {
                    entity,
                    source: capture_source(marker.0),
                    active: camera.is_active,
                    order: camera.order,
                    viewport_position: viewport.physical_position,
                    viewport_size: viewport.physical_size,
                    transform: matrix_bits(transform),
                    projection: format!("{projection:?}").into(),
                })
            })
            .collect::<Result<Vec<_>, RehearsalFailure>>()?;
        camera_states.sort_by_key(|state| state.source.index());
        let camera_states: [CameraState; 2] =
            camera_states.try_into().map_err(|states: Vec<_>| {
                RehearsalFailure::new(
                    "camera_count",
                    format!("expected two source cameras, observed {}", states.len()),
                )
            })?;
        let size = Phase0bViewportControl::capture_size();
        for state in &camera_states {
            if state.active != (state.source == binding.source)
                || state.viewport_position != UVec2::ZERO
                || state.viewport_size != size
            {
                return Err(RehearsalFailure::new(
                    "viewport_mutation",
                    "source camera activation or full viewport did not match the request",
                ));
            }
        }
        let source_transforms = [
            matrix_bits(transforms.get(entities[0]).map_err(|_error| {
                RehearsalFailure::new(
                    "missing_transform",
                    "Current source transform is unavailable",
                )
            })?),
            matrix_bits(transforms.get(entities[1]).map_err(|_error| {
                RehearsalFailure::new(
                    "missing_transform",
                    "Proposed source transform is unavailable",
                )
            })?),
        ];
        Ok(PresentationSnapshot {
            binding,
            source_transforms,
            camera_states,
            semantic_json,
        })
    }

    fn matrix_bits(transform: &Transform) -> [u32; 16] {
        transform.to_matrix().to_cols_array().map(f32::to_bits)
    }

    fn publish_terminal_output(
        mut commands: Commands<'_, '_>,
        mut viewport: ResMut<'_, Phase0bViewportControl>,
        mut rehearsal: ResMut<'_, BrowserRehearsal>,
    ) {
        if rehearsal.terminal_published {
            return;
        }
        if rehearsal.machine.failure().is_some()
            && let Some(run) = rehearsal.event_capture.take()
        {
            for entity in run.entities {
                commands.entity(entity).despawn();
            }
        }
        if (rehearsal.machine.failure().is_some() || rehearsal.machine.is_complete())
            && !viewport.is_normal()
        {
            viewport.restore();
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
        rehearsal
            .observations
            .sort_by_key(|observation| observation_order(observation.sample, observation.source));
        let Some(browser_capture) = rehearsal.capture_complete.as_ref() else {
            rehearsal.machine.fail(
                "missing_capture_complete",
                "the state machine completed without eight screenshot receipts",
            );
            return;
        };
        let Some(event_windows) = rehearsal.event_windows.as_ref() else {
            rehearsal.machine.fail(
                "missing_event_windows",
                "the state machine completed without both strict event windows",
            );
            return;
        };
        match encode_complete(browser_capture, event_windows, &rehearsal.observations) {
            Ok(bytes) => {
                publish_terminal_bytes("complete", &bytes);
                rehearsal.terminal_published = true;
            }
            Err(failure) => rehearsal.machine.fail(failure.kind, failure.message),
        }
    }

    fn captured_runtime_sources(
        runtime: &ViewerRuntime,
    ) -> Result<RuntimeSources, RehearsalFailure> {
        let identity = |slot, label| {
            runtime
                .source(slot)
                .ok_or_else(|| {
                    RehearsalFailure::new(
                        "missing_source_identity",
                        format!("the {label} runtime source identity is unavailable"),
                    )
                })
                .and_then(|source| {
                    RuntimeIdentity::new(
                        source.provenance().manifest_sha256(),
                        source.provenance().content_sha256(),
                    )
                    .map_err(|error| {
                        RehearsalFailure::new(
                            "invalid_source_identity",
                            format!("the {label} runtime source identity is invalid: {error}"),
                        )
                    })
                })
        };
        Ok(RuntimeSources::new(
            identity(SourceSlot::Primary, "Current")?,
            identity(SourceSlot::Comparison, "Proposed")?,
        ))
    }

    fn validate_capture_surface(
        windows: &Query<'_, '_, &Window, With<PrimaryWindow>>,
    ) -> Result<(), RehearsalFailure> {
        let window = windows.single().map_err(|_error| {
            RehearsalFailure::new("capture_surface", "the primary Bevy window is unavailable")
        })?;
        let size = Phase0bViewportControl::capture_size();
        if window.physical_width() != size.x || window.physical_height() != size.y {
            return Err(RehearsalFailure::new(
                "capture_surface",
                format!(
                    "the physical Bevy window is {}x{}; expected {}x{}",
                    window.physical_width(),
                    window.physical_height(),
                    size.x,
                    size.y
                ),
            ));
        }
        let browser = web_sys::window().ok_or_else(|| {
            RehearsalFailure::new("capture_surface", "browser window is unavailable")
        })?;
        let document = browser
            .document()
            .ok_or_else(|| RehearsalFailure::new("capture_surface", "document is unavailable"))?;
        validate_style(&document, "html", ROOT_STYLE)?;
        validate_style(&document, "body", ROOT_STYLE)?;
        validate_style(&document, "#spinal-app", APP_STYLE)?;
        validate_style(&document, ".preview-region", PREVIEW_STYLE)?;
        validate_style(&document, ".canvas-frame", FRAME_STYLE)?;
        validate_style(&document, "#spinal-canvas", CANVAS_STYLE)?;
        for selector in [
            ".app-header",
            "#preview-heading",
            "#spinal-camera-help",
            "#spinal-source-labels",
            "#spinal-transport",
            "#spinal-diagnostics",
        ] {
            validate_style(&document, selector, HIDDEN_STYLE)?;
        }
        let canvas = document
            .get_element_by_id("spinal-canvas")
            .and_then(|element| element.dyn_into::<HtmlCanvasElement>().ok())
            .ok_or_else(|| {
                RehearsalFailure::new("capture_surface", "capture canvas is unavailable")
            })?;
        if canvas.width() != size.x
            || canvas.height() != size.y
            || canvas.client_width() != i32::try_from(size.x).expect("capture width fits i32")
            || canvas.client_height() != i32::try_from(size.y).expect("capture height fits i32")
        {
            return Err(RehearsalFailure::new(
                "capture_surface",
                "the canvas backing or presented size is not exactly 640x480",
            ));
        }
        Ok(())
    }

    fn validate_style(
        document: &web_sys::Document,
        selector: &str,
        expected: &str,
    ) -> Result<(), RehearsalFailure> {
        let element = document
            .query_selector(selector)
            .map_err(|_| RehearsalFailure::new("capture_surface", "capture layout query failed"))?
            .ok_or_else(|| {
                RehearsalFailure::new(
                    "capture_surface",
                    format!("capture layout element `{selector}` was removed"),
                )
            })?;
        if element.get_attribute("style").as_deref() != Some(expected) {
            return Err(RehearsalFailure::new(
                "capture_surface",
                format!("capture layout element `{selector}` was changed"),
            ));
        }
        Ok(())
    }

    const fn capture_source(slot: SourceSlot) -> CaptureSource {
        match slot {
            SourceSlot::Primary => CaptureSource::Current,
            SourceSlot::Comparison => CaptureSource::Proposed,
        }
    }

    const fn source_slot(source: CaptureSource) -> SourceSlot {
        match source {
            CaptureSource::Current => SourceSlot::Primary,
            CaptureSource::Proposed => SourceSlot::Comparison,
        }
    }

    const fn protocol_source(source: CaptureSource) -> ProtocolSource {
        match source {
            CaptureSource::Current => ProtocolSource::Current,
            CaptureSource::Proposed => ProtocolSource::Proposed,
        }
    }

    fn publish_control_message(
        control: &web_sys::Element,
        message: &BrowserControlMessage,
        rehearsal: &mut BrowserRehearsal,
    ) -> Result<(), RehearsalFailure> {
        let bytes = message
            .to_json()
            .map_err(|error| RehearsalFailure::new("encode_error", error.to_string()))?;
        publish_control_bytes(control, &bytes, rehearsal)
    }

    fn publish_control_bytes(
        control: &web_sys::Element,
        bytes: &[u8],
        rehearsal: &mut BrowserRehearsal,
    ) -> Result<(), RehearsalFailure> {
        let json = std::str::from_utf8(bytes)
            .map_err(|_error| RehearsalFailure::new("encode_error", "control JSON is not UTF-8"))?;
        control
            .set_attribute(OUTBOUND_ATTRIBUTE, json)
            .map_err(|_| {
                RehearsalFailure::new("control_dom", "could not publish atomic control output")
            })?;
        rehearsal.expected_outbound = Some(json.into());
        Ok(())
    }

    fn set_output_state(state: &str) {
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return;
        };
        let Some(output) = document.get_element_by_id(OUTPUT_ELEMENT_ID) else {
            return;
        };
        let _ignored = output.set_attribute(STATE_ATTRIBUTE, state);
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

    fn browser_capture() -> serde_json::Value {
        serde_json::json!({
            "format_version": 1,
            "state": "complete",
            "nonce": "e".repeat(64),
            "runtime_sources": {
                "current": {
                    "manifest_sha256": "a".repeat(64),
                    "content_sha256": "b".repeat(64),
                },
                "proposed": {
                    "manifest_sha256": "c".repeat(64),
                    "content_sha256": "d".repeat(64),
                },
            },
            "screenshots": [],
        })
    }

    fn event_windows() -> CapturedEventWindows {
        let bytes = br#"{"format_version":1,"window_id":"sway-events","animation":"sway","start_ns":0,"end_ns":1000000000,"events":[]}"#;
        CapturedEventWindows {
            current: parse_event_window_json(bytes).expect("Current event window"),
            proposed: parse_event_window_json(bytes).expect("Proposed event window"),
        }
    }

    fn finish_event_phase(machine: &mut RehearsalMachine) {
        assert!(machine.event_capture_to_spawn());
        machine.note_event_capture_spawned();
        assert!(machine.event_capture_active());
        machine.note_event_capture_update(true);
        assert!(machine.event_capture_to_finalize());
        machine.note_event_capture_finalized();
    }

    fn observed_event(
        entity_bits: u64,
        playback: u64,
        name: &str,
        time: Duration,
    ) -> ObservedEvent {
        ObservedEvent {
            entity_bits,
            track: None,
            playback,
            animation: ANIMATION_NAME.into(),
            name: name.into(),
            loop_index: 0,
            local_time: time,
            integer: 7,
            float: 1.25,
            string: Some("payload".into()),
            volume: 0.5,
            balance: -0.25,
            diagnostic_codes: Vec::new(),
            degraded: false,
        }
    }

    fn begin_sample(machine: &mut RehearsalMachine) -> FixedSample {
        let sample = machine.sample_to_issue().expect("sample ready to issue");
        machine.note_commands_queued(BASELINE);
        machine.record_expectations(EXPECTED);
        sample
    }

    fn accept_challenge_and_ready(machine: &mut RehearsalMachine) {
        machine.accept_challenge();
        machine.update_readiness(true);
        finish_event_phase(machine);
    }

    fn finish_presentations(machine: &mut RehearsalMachine) {
        for expected_source in CaptureSource::ALL {
            let binding = machine.presentation_to_apply().expect("presentation ready");
            assert_eq!(binding.source, expected_source);
            assert_eq!(binding.expectation(), EXPECTED[expected_source.index()]);
            assert_eq!(
                binding.frame_revision(),
                EXPECTED[expected_source.index()].baseline_frame_revision + 1
            );
            machine.note_presentation_applied(binding);
            assert_eq!(machine.request_to_publish(), None);
            machine.hold_presented_update(binding);
            assert_eq!(machine.request_to_publish(), None);
            machine.hold_presented_update(binding);
            assert_eq!(machine.request_to_publish(), Some(binding));
            machine.note_request_published(binding);
            assert_eq!(machine.awaited_ack(), Some(binding));
            machine.accept_screenshot_ack(binding);
        }
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
        assert!(!shell.contains(CONTROL_ELEMENT_ID));
        assert!(!shell.contains("phase0b-rehearsal"));
        assert!(!shell.contains("data-spinal-phase0b-inbound"));
    }

    #[test]
    fn readiness_has_a_fixed_update_timeout() {
        let mut machine = RehearsalMachine::default();
        machine.accept_challenge();
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
    fn challenge_and_readiness_gate_events_before_any_screenshot_commands() {
        let mut machine = RehearsalMachine::default();
        machine.update_readiness(true);
        assert_eq!(machine.sample_to_issue(), None);
        assert!(matches!(machine.state, MachineState::WaitingForChallenge));

        machine.accept_challenge();
        machine.update_readiness(true);
        assert!(machine.event_capture_to_spawn());
        assert_eq!(machine.sample_to_issue(), None);
        machine.note_event_capture_spawned();
        assert_eq!(machine.sample_to_issue(), None);
        machine.note_event_capture_update(true);
        assert_eq!(machine.sample_to_issue(), None);
        machine.note_event_capture_finalized();
        assert_eq!(machine.sample_to_issue(), Some(FixedSample::SwayStart));

        machine.accept_challenge();
        assert_eq!(
            machine.failure().map(|failure| failure.kind),
            Some("challenge_state")
        );
    }

    #[test]
    fn event_capture_has_a_fixed_update_timeout() {
        let mut machine = RehearsalMachine::default();
        machine.accept_challenge();
        machine.update_readiness(true);
        machine.note_event_capture_spawned();
        for _ in 0..EVENT_CAPTURE_UPDATE_LIMIT - 1 {
            machine.note_event_capture_update(false);
            assert!(machine.failure().is_none());
        }
        machine.note_event_capture_update(false);
        assert_eq!(
            machine.failure().map(|failure| failure.kind),
            Some("event_timeout")
        );
    }

    #[test]
    fn event_accumulator_retains_time_zero_and_equal_time_emission_order() {
        let mut source = EventSourceAccumulator::new(CaptureSource::Current, 11);
        source.bind_playback(7).unwrap();
        source
            .push(observed_event(11, 7, "start", Duration::ZERO))
            .unwrap();
        source
            .push(observed_event(11, 7, "same-a", Duration::from_millis(500)))
            .unwrap();
        source
            .push(observed_event(11, 7, "same-b", Duration::from_millis(500)))
            .unwrap();
        let document = source.finish().expect("strict fixed window");
        assert_eq!(
            document
                .events()
                .iter()
                .map(|event| (event.name(), event.local_time_ns()))
                .collect::<Vec<_>>(),
            [
                ("start", 0),
                ("same-a", 500_000_000),
                ("same-b", 500_000_000),
            ]
        );
    }

    #[test]
    fn event_accumulator_rejects_wrong_entity_playback_track_order_and_degradation() {
        let fresh = || {
            let mut source = EventSourceAccumulator::new(CaptureSource::Proposed, 22);
            source.bind_playback(9).unwrap();
            source
        };

        let mut source = fresh();
        assert_eq!(
            source
                .push(observed_event(23, 9, "wrong-entity", Duration::ZERO))
                .unwrap_err()
                .kind,
            "event_entity"
        );

        let mut source = fresh();
        assert_eq!(
            source
                .push(observed_event(22, 10, "wrong-playback", Duration::ZERO))
                .unwrap_err()
                .kind,
            "event_playback"
        );

        let mut source = fresh();
        let mut track = observed_event(22, 9, "track", Duration::ZERO);
        track.track = Some("override".into());
        assert_eq!(source.push(track).unwrap_err().kind, "event_track");

        let mut source = fresh();
        source
            .push(observed_event(22, 9, "later", Duration::from_millis(500)))
            .unwrap();
        assert_eq!(
            source
                .push(observed_event(22, 9, "earlier", Duration::from_millis(499)))
                .unwrap_err()
                .kind,
            "event_order"
        );

        let mut source = fresh();
        let mut degraded = observed_event(22, 9, "degraded", Duration::ZERO);
        degraded.degraded = true;
        degraded.diagnostic_codes = vec![SemanticDiagnosticCode::UnknownField];
        assert_eq!(source.push(degraded).unwrap_err().kind, "event_degraded");
    }

    #[test]
    fn event_playback_binding_is_stable_bounded_and_completes_only_at_exact_end() {
        let mut source = EventSourceAccumulator::new(CaptureSource::Current, 11);
        assert_eq!(source.bind_playback(0).unwrap_err().kind, "event_playback");
        source.bind_playback(7).unwrap();
        assert_eq!(source.bind_playback(8).unwrap_err().kind, "event_playback");

        let mut source = EventSourceAccumulator::new(CaptureSource::Current, 11);
        source.bind_playback(7).unwrap();
        source.observe_playback(Duration::ZERO, false).unwrap();
        assert_eq!(
            source
                .observe_playback(Duration::from_secs(1), false)
                .unwrap_err()
                .kind,
            "event_playback"
        );
        source
            .observe_playback(Duration::from_secs(1), true)
            .unwrap();
    }

    #[test]
    fn acknowledgements_are_impossible_before_request_and_wait_without_timeout() {
        let mut early = RehearsalMachine::default();
        accept_challenge_and_ready(&mut early);
        begin_sample(&mut early);
        let current = observed(0, &[]);
        let proposed = observed(1, &[]);
        early.observe([&current, &proposed]);
        let binding = early.presentation_to_apply().unwrap();
        early.accept_screenshot_ack(binding);
        assert_eq!(
            early.failure().map(|failure| failure.kind),
            Some("ack_state")
        );

        let mut waiting = RehearsalMachine::default();
        accept_challenge_and_ready(&mut waiting);
        begin_sample(&mut waiting);
        waiting.observe([&current, &proposed]);
        let binding = waiting.presentation_to_apply().unwrap();
        waiting.note_presentation_applied(binding);
        waiting.hold_presented_update(binding);
        waiting.hold_presented_update(binding);
        waiting.note_request_published(binding);
        for _ in 0..(SAMPLE_UPDATE_LIMIT * 100) {
            waiting.update_readiness(false);
            waiting.observe([&current, &proposed]);
        }
        assert_eq!(waiting.awaited_ack(), Some(binding));
        assert!(waiting.failure().is_none());
    }

    #[test]
    fn held_presentation_rejects_any_binding_mutation() {
        let mut machine = RehearsalMachine::default();
        accept_challenge_and_ready(&mut machine);
        begin_sample(&mut machine);
        let current = observed(0, &[]);
        let proposed = observed(1, &[]);
        machine.observe([&current, &proposed]);
        let binding = machine.presentation_to_apply().unwrap();
        machine.note_presentation_applied(binding);
        let mut changed = binding;
        changed.sequence = changed.sequence.saturating_add(1);
        machine.hold_presented_update(changed);
        assert_eq!(
            machine.failure().map(|failure| failure.kind),
            Some("presentation_mutation")
        );
    }

    #[test]
    fn pair_freeze_rebinds_an_earlier_acceptance_to_the_latest_exact_generation() {
        let mut machine = RehearsalMachine::default();
        accept_challenge_and_ready(&mut machine);
        begin_sample(&mut machine);

        let current = observed(0, &[]);
        let mut proposed_missing = observed(1, &[]);
        proposed_missing.frame_present = false;
        let first = machine.observe([&current, &proposed_missing]);
        assert_eq!(first.newly_accepted, [true, false]);

        let mut current_republished = observed(0, &[]);
        current_republished.frame_revision += 3;
        let proposed = observed(1, &[]);
        let second = machine.observe([&current_republished, &proposed]);
        assert_eq!(second.newly_accepted, [false, true]);
        let accepted_binding = machine.presentation_to_apply().unwrap();
        assert_eq!(
            accepted_binding.frame_revisions,
            [current.frame_revision, proposed.frame_revision]
        );

        let frozen = [current_republished.frame_revision, proposed.frame_revision];
        machine.bind_frozen_frame_revisions(frozen);
        let frozen_binding = machine.presentation_to_apply().unwrap();
        assert_eq!(frozen_binding.frame_revisions, frozen);
        assert!(!machine.animation_updates_enabled());
    }

    #[test]
    fn same_semantic_bytes_with_a_new_live_revision_are_rejected_while_frozen() {
        assert!(
            validate_frozen_capture_generations(
                CaptureSource::Current,
                17,
                17,
                EXPECTED[0],
                Some(EXPECTED[0].play_revision),
                Some(EXPECTED[0].seek_revision),
            )
            .is_ok()
        );
        let failure = validate_frozen_capture_generations(
            CaptureSource::Current,
            17,
            18,
            EXPECTED[0],
            Some(EXPECTED[0].play_revision),
            Some(EXPECTED[0].seek_revision),
        )
        .expect_err("a same-pose republish must change the exact live generation");
        assert_eq!(failure.kind, "semantic_mutation");
    }

    #[test]
    fn sources_can_be_accepted_on_different_updates_without_losing_exact_seek_ack() {
        let mut machine = RehearsalMachine::default();
        accept_challenge_and_ready(&mut machine);
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
        finish_presentations(&mut machine);
        assert_eq!(machine.sample_to_issue(), Some(FixedSample::SwayMiddle));
    }

    #[test]
    fn every_sample_requires_fresh_generations_and_exact_ordered_skin_layers() {
        let mut machine = RehearsalMachine::default();
        accept_challenge_and_ready(&mut machine);
        for sample in FIXED_SAMPLES {
            assert_eq!(begin_sample(&mut machine), sample);
            let current = observed(0, sample.skin_layers());
            let proposed = observed(1, sample.skin_layers());
            let decision = machine.observe([&current, &proposed]);
            assert_eq!(decision.newly_accepted, [true, true]);
            finish_presentations(&mut machine);
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
        accept_challenge_and_ready(&mut machine);
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
        let browser_capture = browser_capture();
        let event_windows = event_windows();
        let first = encode_complete(&browser_capture, &event_windows, &observations)
            .expect("bounded output");
        let second = encode_complete(&browser_capture, &event_windows, &observations)
            .expect("deterministic output");
        assert_eq!(first, second);
        assert!(first.len() <= MAX_OBSERVATION_BYTES);
        let text = std::str::from_utf8(&first).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&first).unwrap();
        assert_eq!(value["format_version"], 3);
        assert_eq!(value["state"], "complete");
        assert_eq!(value["browser_capture"], browser_capture);
        assert_eq!(
            value["event_windows"]["current"]["window_id"],
            EVENT_WINDOW_ID
        );
        assert_eq!(
            value["event_windows"]["proposed"]["window_id"],
            EVENT_WINDOW_ID
        );
        assert!(text.starts_with(r#"{"format_version":3,"state":"complete","browser_capture":"#));
        assert!(text.contains(r#""event_windows":{"current":{"format_version":1"#));
        assert!(text.contains(r#""observations":[{"source":"current","sample":"sway-start""#));
        for excluded in ["\"pass\"", "\"gate\"", "\"pixels\"", "\"approval\""] {
            assert!(!text.contains(excluded));
        }
    }

    #[test]
    fn external_error_json_is_terminal_machine_data_and_bounded() {
        let bytes = bounded_error_json("sample_timeout", &"å".repeat(MAX_ERROR_MESSAGE_BYTES));
        assert!(bytes.len() < MAX_ERROR_MESSAGE_BYTES + 256);
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["format_version"], 3);
        assert_eq!(value["state"], "error");
        assert_eq!(value["error"]["kind"], "sample_timeout");
    }
}

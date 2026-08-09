//! Identity-bound native capture of the frozen Phase 0B v1 event window.
//!
//! This bounded native primitive cannot satisfy a Phase 0 gate.

use std::{array, sync::Arc, time::Duration};

use bevy::{
    asset::{AssetPlugin, Assets, Handle},
    ecs::message::Messages,
    image::Image,
    prelude::{App, Entity, MinimalPlugins},
    time::TimeUpdateStrategy,
};
use bevy_spinal::{
    SpinalAnimationEvent, SpinalAnimator, SpinalAsset, SpinalAssetLoaderError, SpinalAtlasPage,
    SpinalInstance, SpinalPlaybackState, SpinalPlugin,
};
use serde::Serialize;
use spinal::{PlaybackMode, SemanticDiagnosticCode, ValidatedRuntimeBundle};
use thiserror::Error;

use crate::{
    LoadedCaseRuntimeBundles,
    contract::{
        ANIMATION_DURATION, ANIMATION_NAME, EVENT_WINDOW_END_NS, EVENT_WINDOW_ID,
        EVENT_WINDOW_START_NS,
    },
    event_compare::{
        EVENT_WINDOW_FORMAT_VERSION, EventWindowDocument, EventWindowError, MAX_DIAGNOSTIC_CODES,
        MAX_EVENT_COUNT, MAX_EVENT_IDENTIFIER_BYTES, MAX_EVENT_STRING_BYTES,
        MAX_EVENT_WINDOW_BYTES, parse_event_window_json,
    },
};

/// Nanoseconds advanced by each deterministic playback update.
pub const EVENT_CAPTURE_STEP_NS: u64 = 100_000_000;
/// Nonzero updates used to advance from zero through one second.
pub const EVENT_CAPTURE_ADVANCE_COUNT: usize = 10;
/// Total update bound, including initialization at time zero.
pub const MAX_EVENT_CAPTURE_UPDATES: usize = EVENT_CAPTURE_ADVANCE_COUNT + 1;
/// Maximum occurrences retained for either source.
pub const MAX_CAPTURED_EVENTS_PER_SOURCE: usize = MAX_EVENT_COUNT;
/// This native-only primitive is never gate-eligible.
pub const NATIVE_EVENT_CAPTURE_GATE_ELIGIBLE: bool = false;

const EVENT_CAPTURE_STEP: Duration = Duration::from_nanos(EVENT_CAPTURE_STEP_NS);
const _: () = assert!(
    EVENT_CAPTURE_STEP_NS * EVENT_CAPTURE_ADVANCE_COUNT as u64
        == EVENT_WINDOW_END_NS - EVENT_WINDOW_START_NS
);

/// One immutable side of the capture.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum NativeEventSource {
    /// Current runtime.
    Current,
    /// Proposed runtime.
    Proposed,
}

impl NativeEventSource {
    const ALL: [Self; 2] = [Self::Current, Self::Proposed];

    const fn index(self) -> usize {
        match self {
            Self::Current => 0,
            Self::Proposed => 1,
        }
    }
}

impl std::fmt::Display for NativeEventSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Current => "Current",
            Self::Proposed => "Proposed",
        })
    }
}

/// Exact retained bundle identity used for one event document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeEventBundleIdentity {
    manifest_sha256: Box<str>,
    content_sha256: Box<str>,
}

impl NativeEventBundleIdentity {
    fn from_bundle(bundle: &ValidatedRuntimeBundle) -> Self {
        Self {
            manifest_sha256: bundle.manifest_sha256().into(),
            content_sha256: bundle.content_sha256().into(),
        }
    }

    /// SHA-256 of the exact retained runtime manifest.
    pub fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
    }

    /// SHA-256 of normalized runtime paths and exact retained bytes.
    pub fn content_sha256(&self) -> &str {
        &self.content_sha256
    }
}

/// Identity-bound Current and Proposed event documents.
#[derive(Clone, Debug, PartialEq)]
pub struct IdentityBoundNativeEventCapture {
    current: EventWindowDocument,
    proposed: EventWindowDocument,
    current_identity: NativeEventBundleIdentity,
    proposed_identity: NativeEventBundleIdentity,
}

impl IdentityBoundNativeEventCapture {
    /// Current event document.
    pub const fn current(&self) -> &EventWindowDocument {
        &self.current
    }

    /// Proposed event document.
    pub const fn proposed(&self) -> &EventWindowDocument {
        &self.proposed
    }

    /// Exact Current bundle identity.
    pub const fn current_identity(&self) -> &NativeEventBundleIdentity {
        &self.current_identity
    }

    /// Exact Proposed bundle identity.
    pub const fn proposed_identity(&self) -> &NativeEventBundleIdentity {
        &self.proposed_identity
    }

    /// Always false for this native-only primitive.
    pub const fn gate_eligible(&self) -> bool {
        NATIVE_EVENT_CAPTURE_GATE_ELIGIBLE
    }
}

/// Failure while constructing or executing native event capture.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum NativeEventCaptureError {
    /// The fixed animation is absent.
    #[error("the {capture_source} bundle has no fixed sway animation")]
    MissingAnimation {
        /// Source that failed.
        capture_source: NativeEventSource,
    },
    /// The fixed animation has the wrong duration.
    #[error("the {capture_source} sway duration is {actual:?}; expected one second")]
    AnimationDurationMismatch {
        /// Source that failed.
        capture_source: NativeEventSource,
        /// Authored duration.
        actual: Duration,
    },
    /// Retained core asset and page handles could not form a Bevy asset.
    #[error("could not construct the {capture_source} Bevy asset: {cause}")]
    AssetConstruction {
        /// Source that failed.
        capture_source: NativeEventSource,
        /// Typed adapter error.
        #[source]
        cause: SpinalAssetLoaderError,
    },
    /// An owned entity disappeared.
    #[error("the {capture_source} event entity disappeared")]
    EntityDisappeared {
        /// Affected source.
        capture_source: NativeEventSource,
    },
    /// Playback observation disappeared.
    #[error("the {capture_source} entity has no playback observation")]
    MissingPlaybackState {
        /// Affected source.
        capture_source: NativeEventSource,
    },
    /// Playback selected the wrong animation.
    #[error("the {capture_source} playback animation is {actual:?}")]
    UnexpectedPlaybackAnimation {
        /// Affected source.
        capture_source: NativeEventSource,
        /// Observed animation.
        actual: Option<Box<str>>,
    },
    /// Playback selected the wrong mode.
    #[error("the {capture_source} playback mode is {actual:?}")]
    UnexpectedPlaybackMode {
        /// Affected source.
        capture_source: NativeEventSource,
        /// Observed mode.
        actual: Option<PlaybackMode>,
    },
    /// Playback changed identifier.
    #[error("the {capture_source} playback changed from {expected} to {actual}")]
    PlaybackChanged {
        /// Affected source.
        capture_source: NativeEventSource,
        /// First identifier.
        expected: u64,
        /// Later identifier.
        actual: u64,
    },
    /// Playback reported the wrong loop.
    #[error("the {capture_source} playback loop is {actual:?}")]
    UnexpectedPlaybackLoop {
        /// Affected source.
        capture_source: NativeEventSource,
        /// Observed loop.
        actual: Option<u128>,
    },
    /// Playback reported the wrong deterministic time.
    #[error("the {capture_source} position is {actual:?}; expected {expected:?}")]
    UnexpectedPlaybackTime {
        /// Affected source.
        capture_source: NativeEventSource,
        /// Expected position.
        expected: Duration,
        /// Observed position.
        actual: Option<Duration>,
    },
    /// Once completion disagreed with the fixed phase.
    #[error("the {capture_source} completion state is {actual}; expected {expected}")]
    UnexpectedCompletion {
        /// Affected source.
        capture_source: NativeEventSource,
        /// Expected state.
        expected: bool,
        /// Observed state.
        actual: bool,
    },
    /// A message came from an entity outside this fresh harness.
    #[error("an event came from unexpected entity {entity:?}")]
    UnexpectedEntity {
        /// Unexpected entity.
        entity: Entity,
    },
    /// A message came from an override track.
    #[error("the {capture_source} event {event:?} came from track {track:?}")]
    UnexpectedTrack {
        /// Affected source.
        capture_source: NativeEventSource,
        /// Event name.
        event: Box<str>,
        /// Unexpected track.
        track: Box<str>,
    },
    /// A message reported the wrong animation.
    #[error("the {capture_source} event {event:?} reported animation {actual:?}")]
    UnexpectedEventAnimation {
        /// Affected source.
        capture_source: NativeEventSource,
        /// Event name.
        event: Box<str>,
        /// Observed animation.
        actual: Box<str>,
    },
    /// A message reported the wrong playback.
    #[error("the {capture_source} event {event:?} playback is {actual}; expected {expected}")]
    UnexpectedEventPlayback {
        /// Affected source.
        capture_source: NativeEventSource,
        /// Event name.
        event: Box<str>,
        /// Expected playback.
        expected: u64,
        /// Observed playback.
        actual: u64,
    },
    /// A message reported a nonzero loop.
    #[error("the {capture_source} event {event:?} loop is {actual}")]
    UnexpectedEventLoop {
        /// Affected source.
        capture_source: NativeEventSource,
        /// Event name.
        event: Box<str>,
        /// Observed loop.
        actual: u128,
    },
    /// A message time was outside the inclusive fixed window.
    #[error("the {capture_source} event {event:?} time {actual:?} is outside the window")]
    UnexpectedEventTime {
        /// Affected source.
        capture_source: NativeEventSource,
        /// Event name.
        event: Box<str>,
        /// Observed time.
        actual: Duration,
    },
    /// Per-source emission time moved backward.
    #[error("the {capture_source} event order moved from {previous:?} to {actual:?}")]
    EventOrder {
        /// Affected source.
        capture_source: NativeEventSource,
        /// Previous time.
        previous: Duration,
        /// Current time.
        actual: Duration,
    },
    /// A degraded event cannot be accepted.
    #[error("the {capture_source} event {event:?} is degraded: {diagnostic_codes:?}")]
    DegradedEvent {
        /// Affected source.
        capture_source: NativeEventSource,
        /// Event name.
        event: Box<str>,
        /// Exact stable codes.
        diagnostic_codes: Vec<SemanticDiagnosticCode>,
    },
    /// An event field exceeded a parser bound.
    #[error(
        "the {capture_source} event {event:?} field {field} has {actual}; maximum is {maximum}"
    )]
    EventFieldLimit {
        /// Affected source.
        capture_source: NativeEventSource,
        /// Bounded event name.
        event: Box<str>,
        /// Stable field.
        field: &'static str,
        /// Observed count.
        actual: usize,
        /// Fixed maximum.
        maximum: usize,
    },
    /// Too many occurrences were emitted.
    #[error("the {capture_source} source exceeded {maximum} events")]
    EventLimit {
        /// Affected source.
        capture_source: NativeEventSource,
        /// Fixed maximum.
        maximum: usize,
    },
    /// JSON serialization failed.
    #[error("could not serialize the {capture_source} event document: {message}")]
    Serialization {
        /// Affected source.
        capture_source: NativeEventSource,
        /// Bounded detail.
        message: Box<str>,
    },
    /// Serialized output exceeded the event-reference bound.
    #[error("the {capture_source} event document has {actual} bytes; maximum is {maximum}")]
    OutputLimit {
        /// Affected source.
        capture_source: NativeEventSource,
        /// Observed bytes.
        actual: usize,
        /// Fixed maximum.
        maximum: usize,
    },
    /// The closed DTO failed shared strict parsing.
    #[error("the {capture_source} event document is invalid: {cause}")]
    InvalidDocument {
        /// Affected source.
        capture_source: NativeEventSource,
        /// Strict parser error.
        #[source]
        cause: EventWindowError,
    },
}

/// Captures the fixed window from exact retained Current and Proposed assets.
pub fn capture_loaded_case_event_windows(
    bundles: &LoadedCaseRuntimeBundles,
) -> Result<IdentityBoundNativeEventCapture, NativeEventCaptureError> {
    capture_pair(bundles.current(), bundles.proposed())
}

fn capture_pair(
    current: &ValidatedRuntimeBundle,
    proposed: &ValidatedRuntimeBundle,
) -> Result<IdentityBoundNativeEventCapture, NativeEventCaptureError> {
    preflight(NativeEventSource::Current, current)?;
    preflight(NativeEventSource::Proposed, proposed)?;
    let current_identity = NativeEventBundleIdentity::from_bundle(current);
    let proposed_identity = NativeEventBundleIdentity::from_bundle(proposed);
    let mut app = new_app();
    let handles = [
        insert_asset(&mut app, NativeEventSource::Current, current)?,
        insert_asset(&mut app, NativeEventSource::Proposed, proposed)?,
    ];
    let entities = [
        spawn(&mut app, handles[0].clone()),
        spawn(&mut app, handles[1].clone()),
    ];
    let [current_events, proposed_events] = capture_entities(&mut app, entities)?;
    Ok(IdentityBoundNativeEventCapture {
        current: finish_document(NativeEventSource::Current, current_events)?,
        proposed: finish_document(NativeEventSource::Proposed, proposed_events)?,
        current_identity,
        proposed_identity,
    })
}

fn preflight(
    capture_source: NativeEventSource,
    bundle: &ValidatedRuntimeBundle,
) -> Result<(), NativeEventCaptureError> {
    let Some(animation) = bundle.asset().animation_id(ANIMATION_NAME) else {
        return Err(NativeEventCaptureError::MissingAnimation { capture_source });
    };
    let actual = bundle
        .asset()
        .animation(animation)
        .expect("a resolved retained animation remains asset-scoped")
        .duration();
    if actual == ANIMATION_DURATION {
        Ok(())
    } else {
        Err(NativeEventCaptureError::AnimationDurationMismatch {
            capture_source,
            actual,
        })
    }
}

fn new_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), SpinalPlugin));
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    app
}

fn insert_asset(
    app: &mut App,
    capture_source: NativeEventSource,
    bundle: &ValidatedRuntimeBundle,
) -> Result<Handle<SpinalAsset>, NativeEventCaptureError> {
    let pages = bundle
        .asset()
        .atlas_pages()
        .map(|page| {
            let image = app
                .world_mut()
                .resource_mut::<Assets<Image>>()
                .add(Image::default());
            SpinalAtlasPage::new(page.name(), image)
        })
        .collect();
    let asset = SpinalAsset::new(Arc::clone(bundle.asset()), pages).map_err(|cause| {
        NativeEventCaptureError::AssetConstruction {
            capture_source,
            cause,
        }
    })?;
    Ok(app
        .world_mut()
        .resource_mut::<Assets<SpinalAsset>>()
        .add(asset))
}

fn spawn(app: &mut App, asset: Handle<SpinalAsset>) -> Entity {
    app.world_mut()
        .spawn((
            SpinalInstance::new(asset),
            SpinalAnimator::once(ANIMATION_NAME),
        ))
        .id()
}

fn capture_entities(
    app: &mut App,
    entities: [Entity; 2],
) -> Result<[EventAccumulator; 2], NativeEventCaptureError> {
    let mut cursor = app
        .world()
        .resource::<Messages<SpinalAnimationEvent>>()
        .get_cursor_current();
    let mut playback_ids = [None, None];
    let mut accumulators = array::from_fn(|_| EventAccumulator::default());
    for update in 0..MAX_EVENT_CAPTURE_UPDATES {
        if update == 1 {
            app.insert_resource(TimeUpdateStrategy::ManualDuration(EVENT_CAPTURE_STEP));
        }
        app.update();
        let expected_time =
            Duration::from_nanos(EVENT_CAPTURE_STEP_NS * update as u64).min(ANIMATION_DURATION);
        for capture_source in NativeEventSource::ALL {
            validate_playback(
                app,
                capture_source,
                entities[capture_source.index()],
                expected_time,
                update == EVENT_CAPTURE_ADVANCE_COUNT,
                &mut playback_ids[capture_source.index()],
            )?;
        }
        let messages = app.world().resource::<Messages<SpinalAnimationEvent>>();
        for event in cursor.read(messages) {
            let capture_source = if event.entity() == entities[0] {
                NativeEventSource::Current
            } else if event.entity() == entities[1] {
                NativeEventSource::Proposed
            } else {
                return Err(NativeEventCaptureError::UnexpectedEntity {
                    entity: event.entity(),
                });
            };
            let playback = playback_ids[capture_source.index()]
                .expect("playback validation runs before message admission");
            let captured = capture_event(capture_source, playback, event)?;
            accumulators[capture_source.index()].push(capture_source, captured)?;
        }
    }
    Ok(accumulators)
}

fn validate_playback(
    app: &App,
    capture_source: NativeEventSource,
    entity: Entity,
    expected_time: Duration,
    expected_complete: bool,
    expected_playback: &mut Option<u64>,
) -> Result<(), NativeEventCaptureError> {
    let entity = app
        .world()
        .get_entity(entity)
        .map_err(|_| NativeEventCaptureError::EntityDisappeared { capture_source })?;
    let state = entity
        .get::<SpinalPlaybackState>()
        .ok_or(NativeEventCaptureError::MissingPlaybackState { capture_source })?;
    if state.animation() != Some(ANIMATION_NAME) {
        return Err(NativeEventCaptureError::UnexpectedPlaybackAnimation {
            capture_source,
            actual: state.animation().map(Into::into),
        });
    }
    if state.mode() != Some(PlaybackMode::Once) {
        return Err(NativeEventCaptureError::UnexpectedPlaybackMode {
            capture_source,
            actual: state.mode(),
        });
    }
    let playback = state
        .playback()
        .expect("an accepted animation has a playback identifier");
    if let Some(expected) = *expected_playback {
        if playback != expected {
            return Err(NativeEventCaptureError::PlaybackChanged {
                capture_source,
                expected,
                actual: playback,
            });
        }
    } else {
        *expected_playback = Some(playback);
    }
    if state.loop_index() != Some(0) {
        return Err(NativeEventCaptureError::UnexpectedPlaybackLoop {
            capture_source,
            actual: state.loop_index(),
        });
    }
    if state.position() != Some(expected_time) {
        return Err(NativeEventCaptureError::UnexpectedPlaybackTime {
            capture_source,
            expected: expected_time,
            actual: state.position(),
        });
    }
    if state.is_complete() != expected_complete {
        return Err(NativeEventCaptureError::UnexpectedCompletion {
            capture_source,
            expected: expected_complete,
            actual: state.is_complete(),
        });
    }
    Ok(())
}

fn capture_event(
    capture_source: NativeEventSource,
    playback: u64,
    event: &SpinalAnimationEvent,
) -> Result<CapturedEvent, NativeEventCaptureError> {
    let name: Box<str> = event.event().into();
    if let Some(track) = event.track() {
        return Err(NativeEventCaptureError::UnexpectedTrack {
            capture_source,
            event: name,
            track: track.into(),
        });
    }
    if event.animation() != ANIMATION_NAME {
        return Err(NativeEventCaptureError::UnexpectedEventAnimation {
            capture_source,
            event: name,
            actual: event.animation().into(),
        });
    }
    if event.playback() != playback {
        return Err(NativeEventCaptureError::UnexpectedEventPlayback {
            capture_source,
            event: name,
            expected: playback,
            actual: event.playback(),
        });
    }
    if event.loop_index() != 0 {
        return Err(NativeEventCaptureError::UnexpectedEventLoop {
            capture_source,
            event: name,
            actual: event.loop_index(),
        });
    }
    if !(Duration::ZERO..=ANIMATION_DURATION).contains(&event.local_time()) {
        return Err(NativeEventCaptureError::UnexpectedEventTime {
            capture_source,
            event: name,
            actual: event.local_time(),
        });
    }
    if event.is_degraded() {
        return Err(NativeEventCaptureError::DegradedEvent {
            capture_source,
            event: name,
            diagnostic_codes: event.diagnostic_codes().to_vec(),
        });
    }
    field_limit(
        capture_source,
        &name,
        "name",
        event.event().len(),
        MAX_EVENT_IDENTIFIER_BYTES,
    )?;
    if let Some(string) = event.string() {
        field_limit(
            capture_source,
            &name,
            "string",
            string.len(),
            MAX_EVENT_STRING_BYTES,
        )?;
    }
    field_limit(
        capture_source,
        &name,
        "diagnostic_codes",
        event.diagnostic_codes().len(),
        MAX_DIAGNOSTIC_CODES,
    )?;
    Ok(CapturedEvent {
        animation: ANIMATION_NAME,
        name,
        local_time_ns: event.local_time().as_nanos() as u64,
        loop_index: 0,
        integer: event.integer(),
        float: f64::from(event.float()),
        string: event.string().map(Into::into),
        volume: f64::from(event.volume()),
        balance: f64::from(event.balance()),
        diagnostic_codes: event.diagnostic_codes().to_vec(),
    })
}

fn field_limit(
    capture_source: NativeEventSource,
    event: &str,
    field: &'static str,
    actual: usize,
    maximum: usize,
) -> Result<(), NativeEventCaptureError> {
    if actual <= maximum {
        Ok(())
    } else {
        Err(NativeEventCaptureError::EventFieldLimit {
            capture_source,
            event: event
                .chars()
                .take(MAX_EVENT_IDENTIFIER_BYTES)
                .collect::<String>()
                .into_boxed_str(),
            field,
            actual,
            maximum,
        })
    }
}

#[derive(Default)]
struct EventAccumulator {
    events: Vec<CapturedEvent>,
    encoded_event_bytes: usize,
    previous_time: Option<Duration>,
}

impl EventAccumulator {
    fn push(
        &mut self,
        capture_source: NativeEventSource,
        event: CapturedEvent,
    ) -> Result<(), NativeEventCaptureError> {
        if self.events.len() == MAX_CAPTURED_EVENTS_PER_SOURCE {
            return Err(NativeEventCaptureError::EventLimit {
                capture_source,
                maximum: MAX_CAPTURED_EVENTS_PER_SOURCE,
            });
        }
        let time = Duration::from_nanos(event.local_time_ns);
        if let Some(previous) = self.previous_time
            && time < previous
        {
            return Err(NativeEventCaptureError::EventOrder {
                capture_source,
                previous,
                actual: time,
            });
        }
        let encoded = serialize(capture_source, &event)?;
        self.encoded_event_bytes = self.encoded_event_bytes.checked_add(encoded.len()).ok_or(
            NativeEventCaptureError::OutputLimit {
                capture_source,
                actual: usize::MAX,
                maximum: MAX_EVENT_WINDOW_BYTES,
            },
        )?;
        if self.encoded_event_bytes > MAX_EVENT_WINDOW_BYTES {
            return Err(NativeEventCaptureError::OutputLimit {
                capture_source,
                actual: self.encoded_event_bytes,
                maximum: MAX_EVENT_WINDOW_BYTES,
            });
        }
        self.previous_time = Some(time);
        self.events.push(event);
        Ok(())
    }
}

#[derive(Serialize)]
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

#[derive(Serialize)]
struct EventWindowDto {
    format_version: u16,
    window_id: &'static str,
    animation: &'static str,
    start_ns: u64,
    end_ns: u64,
    events: Vec<CapturedEvent>,
}

fn finish_document(
    capture_source: NativeEventSource,
    captured: EventAccumulator,
) -> Result<EventWindowDocument, NativeEventCaptureError> {
    let json = serialize(
        capture_source,
        &EventWindowDto {
            format_version: EVENT_WINDOW_FORMAT_VERSION,
            window_id: EVENT_WINDOW_ID,
            animation: ANIMATION_NAME,
            start_ns: EVENT_WINDOW_START_NS,
            end_ns: EVENT_WINDOW_END_NS,
            events: captured.events,
        },
    )?;
    if json.len() > MAX_EVENT_WINDOW_BYTES {
        return Err(NativeEventCaptureError::OutputLimit {
            capture_source,
            actual: json.len(),
            maximum: MAX_EVENT_WINDOW_BYTES,
        });
    }
    parse_event_window_json(&json).map_err(|cause| NativeEventCaptureError::InvalidDocument {
        capture_source,
        cause,
    })
}

fn serialize(
    capture_source: NativeEventSource,
    value: &impl Serialize,
) -> Result<Vec<u8>, NativeEventCaptureError> {
    serde_json::to_vec(value).map_err(|error| NativeEventCaptureError::Serialization {
        capture_source,
        message: error.to_string().into(),
    })
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
    };

    use sha2::{Digest, Sha256};
    use spinal::RuntimeBundleManifest;

    use crate::{load_case, load_case_runtime_bundles};

    use super::*;

    const CASE: &str = include_str!("../cases/generic-bevy-0.18.1.toml");
    const ATLAS: &[u8] = b"cat.png\n\tsize: 1, 1\n";
    const PNG: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 96, 96, 248, 255, 31,
        0, 3, 2, 1, 255, 230, 119, 11, 174, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];

    struct AuthenticatedFixture {
        _directory: tempfile::TempDir,
        case_path: PathBuf,
    }

    impl AuthenticatedFixture {
        fn new(current_json: &[u8], proposed_json: &[u8]) -> Self {
            let directory = tempfile::tempdir().expect("temporary event fixture");
            let current_manifest = write_bundle(
                directory.path(),
                "current/runtime.json",
                "Current events",
                current_json,
            );
            let proposed_manifest = write_bundle(
                directory.path(),
                "proposed/runtime.json",
                "Proposed events",
                proposed_json,
            );
            let mut case = provide_manifest(
                CASE.to_owned(),
                "runtime_manifest = { required = true }",
                directory.path(),
                &current_manifest,
            );
            case = provide_manifest(
                case,
                "runtime_manifest = {required = true}",
                directory.path(),
                &proposed_manifest,
            );
            let case_path = directory.path().join("case.toml");
            fs::write(&case_path, case).expect("write event case");
            Self {
                _directory: directory,
                case_path,
            }
        }

        fn load(&self) -> LoadedCaseRuntimeBundles {
            let case = load_case(&self.case_path).expect("authenticate event case");
            load_case_runtime_bundles(&case).expect("load exact runtime bundles")
        }
    }

    fn write_bundle(root: &Path, relative: &str, label: &str, json: &[u8]) -> PathBuf {
        let manifest_path = root.join(relative);
        let directory = manifest_path.parent().expect("manifest parent");
        let files = BTreeMap::from([
            (PathBuf::from("rig/cat.json"), json.to_vec()),
            (PathBuf::from("rig/cat.atlas"), ATLAS.to_vec()),
            (PathBuf::from("rig/cat.png"), PNG.to_vec()),
        ]);
        let manifest = RuntimeBundleManifest::build(
            label,
            Path::new("rig/cat.json"),
            Path::new("rig/cat.atlas"),
            files.clone(),
        )
        .expect("build event bundle")
        .0;
        fs::create_dir_all(directory).expect("create manifest directory");
        fs::write(&manifest_path, manifest).expect("write manifest");
        for (path, bytes) in files {
            let output = directory.join(path);
            fs::create_dir_all(output.parent().expect("runtime parent"))
                .expect("create runtime directory");
            fs::write(output, bytes).expect("write runtime file");
        }
        manifest_path
    }

    fn provide_manifest(case: String, slot: &str, root: &Path, manifest: &Path) -> String {
        assert_eq!(case.matches(slot).count(), 1);
        let relative = manifest
            .strip_prefix(root)
            .expect("manifest remains under fixture")
            .to_str()
            .expect("portable path");
        let bytes = fs::read(manifest).expect("read manifest");
        let field = slot.split_once('=').expect("manifest field").0;
        case.replacen(
            slot,
            &format!(
                "{field}= {{ required = true, path = \"{relative}\", byte_length = {}, sha256 = \"{}\" }}",
                bytes.len(),
                sha256_hex(&bytes)
            ),
            1,
        )
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn event_json(base: i32, degraded: bool) -> Vec<u8> {
        let future = if degraded {
            ",\"futurePayload\":true"
        } else {
            ""
        };
        format!(
            r#"{{
              "skeleton":{{"spine":"4.3.23"}},
              "bones":[{{"name":"root"}}],
              "events":{{
                "start":{{"int":{base}{future}}},
                "middle":{{"int":{},"float":1.25,"string":"middle"}},
                "end":{{"int":{},"volume":0.5,"balance":-0.25}}
              }},
              "animations":{{"sway":{{"events":[
                {{"name":"start"}},
                {{"time":0.5,"name":"middle"}},
                {{"time":1,"name":"end"}}
              ]}}}}
            }}"#,
            base + 1,
            base + 2,
        )
        .into_bytes()
    }

    #[test]
    fn authenticated_pair_captures_boundaries_payloads_identities_and_roles() {
        let fixture = AuthenticatedFixture::new(&event_json(10, false), &event_json(20, false));
        let bundles = fixture.load();
        let capture =
            capture_loaded_case_event_windows(&bundles).expect("fixed event capture succeeds");

        assert_eq!(
            capture.current_identity().manifest_sha256(),
            bundles.current().manifest_sha256()
        );
        assert_eq!(
            capture.current_identity().content_sha256(),
            bundles.current().content_sha256()
        );
        assert_eq!(
            capture.proposed_identity().manifest_sha256(),
            bundles.proposed().manifest_sha256()
        );
        assert_eq!(
            capture.proposed_identity().content_sha256(),
            bundles.proposed().content_sha256()
        );
        assert_ne!(capture.current_identity(), capture.proposed_identity());

        assert_eq!(
            capture
                .current()
                .events()
                .iter()
                .map(|event| (event.name(), event.local_time_ns(), event.integer()))
                .collect::<Vec<_>>(),
            [
                ("start", 0, 10),
                ("middle", 500_000_000, 11),
                ("end", 1_000_000_000, 12),
            ]
        );
        assert_eq!(
            capture
                .proposed()
                .events()
                .iter()
                .map(|event| event.integer())
                .collect::<Vec<_>>(),
            [20, 21, 22]
        );
        assert!(
            capture
                .current()
                .events()
                .iter()
                .chain(capture.proposed().events())
                .all(|event| event.diagnostic_codes().is_empty())
        );
        assert!(!capture.gate_eligible());
        assert!(!capture.current().gate_eligible());
        assert_eq!(MAX_EVENT_CAPTURE_UPDATES, 11);
    }

    #[test]
    fn degraded_event_retains_stable_code_and_proposed_role() {
        let fixture = AuthenticatedFixture::new(&event_json(10, false), &event_json(20, true));
        let bundles = fixture.load();
        assert!(matches!(
            capture_loaded_case_event_windows(&bundles),
            Err(NativeEventCaptureError::DegradedEvent {
                capture_source: NativeEventSource::Proposed,
                diagnostic_codes,
                ..
            }) if diagnostic_codes == [SemanticDiagnosticCode::UnknownField]
        ));
    }

    #[test]
    fn animation_preflight_errors_preserve_the_exact_role() {
        let current = event_json(10, false);
        let missing = String::from_utf8(event_json(20, false))
            .expect("fixture UTF-8")
            .replace("\"sway\"", "\"idle\"")
            .into_bytes();
        let fixture = AuthenticatedFixture::new(&current, &missing);
        let bundles = fixture.load();
        assert!(matches!(
            capture_loaded_case_event_windows(&bundles),
            Err(NativeEventCaptureError::MissingAnimation {
                capture_source: NativeEventSource::Proposed
            })
        ));

        let short = String::from_utf8(event_json(20, false))
            .expect("fixture UTF-8")
            .replacen("\"time\":1", "\"time\":0.75", 1)
            .into_bytes();
        let fixture = AuthenticatedFixture::new(&current, &short);
        let bundles = fixture.load();
        assert!(matches!(
            capture_loaded_case_event_windows(&bundles),
            Err(NativeEventCaptureError::AnimationDurationMismatch {
                capture_source: NativeEventSource::Proposed,
                actual
            }) if actual == Duration::from_millis(750)
        ));
    }

    #[test]
    fn emitted_event_overflow_is_bounded_and_current_scoped() {
        let frames = std::iter::repeat_n(r#"{"name":"cue"}"#, MAX_CAPTURED_EVENTS_PER_SOURCE + 1)
            .collect::<Vec<_>>()
            .join(",");
        let overflow = format!(
            r#"{{
              "skeleton":{{"spine":"4.3.23"}},
              "bones":[{{"name":"root"}}],
              "events":{{"cue":{{}}}},
              "animations":{{"sway":{{
                "bones":{{"root":{{"translate":[{{"x":0}},{{"time":1,"x":1}}]}}}},
                "events":[{frames}]
              }}}}
            }}"#
        );
        let fixture = AuthenticatedFixture::new(overflow.as_bytes(), &event_json(20, false));
        let bundles = fixture.load();
        assert!(matches!(
            capture_loaded_case_event_windows(&bundles),
            Err(NativeEventCaptureError::EventLimit {
                capture_source: NativeEventSource::Current,
                maximum: MAX_CAPTURED_EVENTS_PER_SOURCE
            })
        ));
    }

    #[test]
    fn invalid_closed_document_is_typed_and_source_scoped() {
        let captured = EventAccumulator {
            events: vec![CapturedEvent {
                animation: ANIMATION_NAME,
                name: "unsafe\nname".into(),
                local_time_ns: 0,
                loop_index: 0,
                integer: 0,
                float: 0.0,
                string: None,
                volume: 1.0,
                balance: 0.0,
                diagnostic_codes: Vec::new(),
            }],
            encoded_event_bytes: 0,
            previous_time: Some(Duration::ZERO),
        };
        assert!(matches!(
            finish_document(NativeEventSource::Proposed, captured),
            Err(NativeEventCaptureError::InvalidDocument {
                capture_source: NativeEventSource::Proposed,
                ..
            })
        ));
    }
}

//! Strict parsing of the fixed browser semantic-observation envelope.
//!
//! The parser binds the two claimed browser runtime identities to one already
//! loaded [`crate::LoadedCaseRuntimeBundles`] pair and validates the complete
//! fixed v1 schedule and Current/Proposed event windows. A nested capture
//! document binds a fresh driver nonce and eight PNG receipts to the same
//! semantic generations and runtime identities. It still cannot authenticate
//! browser/build provenance, so parsed values are conformance observations,
//! never Phase 0B evidence or a gate decision.

use serde::{Deserialize, Deserializer};
use serde_json::value::RawValue;
use spinal::SemanticFrame;
use thiserror::Error;

use crate::{
    LoadedCaseRuntimeBundles,
    browser_capture::{
        BrowserCaptureComplete, CaptureSource as BrowserCaptureSource,
        RuntimeIdentity as CaptureRuntimeIdentity,
    },
    capture::{NativeSample, NativeSource},
    contract::{SAMPLE_COUNT, SAMPLE_SCHEDULE},
    event_compare::{EventWindowDocument, MAX_EVENT_WINDOW_BYTES, parse_event_window_json},
};

/// Browser observation-envelope schema accepted by this parser.
pub const BROWSER_OBSERVATION_FORMAT_VERSION: u8 = 3;

const EVENT_WINDOWS_ENVELOPE_OVERHEAD_BYTES: usize = 64;

/// Maximum complete browser observation document size.
pub const MAX_BROWSER_OBSERVATION_BYTES: usize = 8 * 1024 * 1024
    + 64 * 1024
    + 2 * MAX_EVENT_WINDOW_BYTES
    + EVENT_WINDOWS_ENVELOPE_OVERHEAD_BYTES;

const MAX_ERROR_KIND_BYTES: usize = 64;
const MAX_ERROR_MESSAGE_BYTES: usize = 4 * 1024;

/// Exact identity claimed for one browser-loaded runtime bundle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserRuntimeIdentity {
    manifest_sha256: Box<str>,
    content_sha256: Box<str>,
}

impl BrowserRuntimeIdentity {
    /// Returns the SHA-256 of the exact runtime-manifest bytes.
    #[must_use]
    pub fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
    }

    /// Returns the normalized runtime-bundle content SHA-256.
    #[must_use]
    pub fn content_sha256(&self) -> &str {
        &self.content_sha256
    }
}

/// One validated fixed-schedule browser semantic observation.
#[derive(Clone, Debug, PartialEq)]
pub struct BrowserSemanticObservation {
    source: NativeSource,
    sample: NativeSample,
    frame_revision: u64,
    acknowledged_play_revision: u64,
    acknowledged_seek_revision: u64,
    frame: SemanticFrame,
}

impl BrowserSemanticObservation {
    /// Returns the immutable source slot that produced the frame.
    #[must_use]
    pub const fn source(&self) -> NativeSource {
        self.source
    }

    /// Returns the fixed schedule sample represented by the frame.
    #[must_use]
    pub const fn sample(&self) -> NativeSample {
        self.sample
    }

    /// Returns the successful semantic-capture generation.
    #[must_use]
    pub const fn frame_revision(&self) -> u64 {
        self.frame_revision
    }

    /// Returns the play-command generation acknowledged by the frame.
    #[must_use]
    pub const fn acknowledged_play_revision(&self) -> u64 {
        self.acknowledged_play_revision
    }

    /// Returns the seek-command generation acknowledged by the frame.
    #[must_use]
    pub const fn acknowledged_seek_revision(&self) -> u64 {
        self.acknowledged_seek_revision
    }

    /// Returns the complete validated renderer-neutral frame.
    #[must_use]
    pub const fn frame(&self) -> &SemanticFrame {
        &self.frame
    }
}

/// A complete identity-bound parse of the frozen browser schedule.
#[derive(Clone, Debug, PartialEq)]
pub struct BrowserSemanticObservations {
    current_identity: BrowserRuntimeIdentity,
    proposed_identity: BrowserRuntimeIdentity,
    browser_capture: BrowserCaptureComplete,
    current_event_window: EventWindowDocument,
    proposed_event_window: EventWindowDocument,
    observations: Box<[BrowserSemanticObservation]>,
}

impl BrowserSemanticObservations {
    /// Returns the exact Current runtime identity claimed by the browser.
    #[must_use]
    pub const fn current_identity(&self) -> &BrowserRuntimeIdentity {
        &self.current_identity
    }

    /// Returns the exact Proposed runtime identity claimed by the browser.
    #[must_use]
    pub const fn proposed_identity(&self) -> &BrowserRuntimeIdentity {
        &self.proposed_identity
    }

    /// Returns the eight observations in sample-major, Current-first order.
    #[must_use]
    pub fn observations(&self) -> &[BrowserSemanticObservation] {
        &self.observations
    }

    /// Returns the nonce- and PNG-bound fixed presentation capture.
    #[must_use]
    pub const fn browser_capture(&self) -> &BrowserCaptureComplete {
        &self.browser_capture
    }

    /// Returns the strict Current authored-event window.
    #[must_use]
    pub const fn current_event_window(&self) -> &EventWindowDocument {
        &self.current_event_window
    }

    /// Returns the strict Proposed authored-event window.
    #[must_use]
    pub const fn proposed_event_window(&self) -> &EventWindowDocument {
        &self.proposed_event_window
    }

    /// Returns `false`; this parser cannot mint representative evidence.
    #[must_use]
    pub const fn representative_gate_eligible(&self) -> bool {
        false
    }
}

/// Failure while parsing or binding a browser observation document.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum BrowserObservationError {
    /// The document is empty or exceeds the fixed byte budget.
    #[error("browser observation bytes must have length 1-{MAX_BROWSER_OBSERVATION_BYTES}")]
    InvalidLength,
    /// JSON or one of its closed nested schemas is invalid.
    #[error("invalid browser observation JSON: {message}")]
    InvalidJson {
        /// Bounded parser detail.
        message: Box<str>,
    },
    /// The browser published a terminal error instead of observations.
    #[error("browser rehearsal reported `{kind}`: {message}")]
    BrowserReported {
        /// Stable bounded browser failure category.
        kind: Box<str>,
        /// Bounded browser failure detail.
        message: Box<str>,
    },
    /// The envelope declares an unsupported schema version.
    #[error("unsupported browser observation format {actual}; expected {expected}")]
    WrongFormatVersion {
        /// Only accepted schema version.
        expected: u8,
        /// Version declared by the document.
        actual: u8,
    },
    /// One runtime digest is malformed or differs from the loaded bundle.
    #[error("browser {capture_source:?} runtime {field} does not match the loaded bundle")]
    RuntimeIdentityMismatch {
        /// Runtime side whose identity failed.
        capture_source: NativeSource,
        /// Stable identity field name.
        field: &'static str,
    },
    /// The caller's independently generated session nonce is malformed.
    #[error("expected browser capture nonce must be 64 lowercase hexadecimal characters")]
    InvalidExpectedNonce,
    /// The embedded capture belongs to another browser session.
    #[error("browser capture nonce does not match the expected session")]
    NonceMismatch,
    /// The document does not contain exactly two observations per sample.
    #[error("browser observation count was {actual}; expected {expected}")]
    ObservationCount {
        /// Required fixed count.
        expected: usize,
        /// Count declared by the document.
        actual: usize,
    },
    /// An observation is absent, duplicated, reordered, or mislabeled.
    #[error(
        "browser observation {index} was `{actual_source}`/`{actual_sample}`; expected `{expected_source}`/`{expected_sample}`"
    )]
    ObservationOrder {
        /// Zero-based observation position.
        index: usize,
        /// Required lowercase source label.
        expected_source: &'static str,
        /// Required fixed sample identifier.
        expected_sample: &'static str,
        /// Supplied source label.
        actual_source: Box<str>,
        /// Supplied sample label.
        actual_sample: Box<str>,
    },
    /// A command/capture generation is zero or not strictly newer for its side.
    #[error("browser {capture_source:?} {field} is not a nonzero strictly increasing generation")]
    InvalidGeneration {
        /// Source whose generation failed.
        capture_source: NativeSource,
        /// Stable generation field.
        field: &'static str,
    },
    /// The frame's complete skin-layer selection differs from its sample.
    #[error("browser {capture_source:?} frame for `{sample}` has the wrong skin layers")]
    SkinLayersMismatch {
        /// Source whose frame failed.
        capture_source: NativeSource,
        /// Fixed sample identifier.
        sample: &'static str,
    },
    /// A screenshot receipt does not bind the corresponding semantic frame.
    #[error("browser screenshot {index} changed semantic binding field {field}")]
    ScreenshotBindingMismatch {
        /// Fixed sample-major receipt position.
        index: usize,
        /// Stable mismatched field.
        field: &'static str,
    },
}

/// Parses a fixed browser envelope and binds it to exact loaded bundle bytes.
pub fn parse_browser_semantic_observations(
    bytes: &[u8],
    expected_nonce: &str,
    bundles: &LoadedCaseRuntimeBundles,
) -> Result<BrowserSemanticObservations, BrowserObservationError> {
    let expected = ExpectedRuntimeSources {
        nonce: expected_nonce,
        current: ExpectedRuntimeIdentity {
            manifest_sha256: bundles.current().manifest_sha256(),
            content_sha256: bundles.current().content_sha256(),
        },
        proposed: ExpectedRuntimeIdentity {
            manifest_sha256: bundles.proposed().manifest_sha256(),
            content_sha256: bundles.proposed().content_sha256(),
        },
    };
    parse_with_expected_sources(bytes, expected)
}

#[derive(Clone, Copy)]
struct ExpectedRuntimeIdentity<'a> {
    manifest_sha256: &'a str,
    content_sha256: &'a str,
}

#[derive(Clone, Copy)]
struct ExpectedRuntimeSources<'a> {
    nonce: &'a str,
    current: ExpectedRuntimeIdentity<'a>,
    proposed: ExpectedRuntimeIdentity<'a>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BrowserDocument {
    format_version: u8,
    state: Box<str>,
    #[serde(default, deserialize_with = "deserialize_present_non_null")]
    browser_capture: Option<BrowserCaptureComplete>,
    #[serde(default, deserialize_with = "deserialize_present_non_null")]
    event_windows: Option<Box<EventWindowsWire>>,
    #[serde(default, deserialize_with = "deserialize_present_non_null")]
    observations: Option<Vec<ObservationWire>>,
    #[serde(default, deserialize_with = "deserialize_present_non_null")]
    error: Option<BrowserErrorWire>,
}

fn deserialize_present_non_null<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ObservationWire {
    source: Box<str>,
    sample: Box<str>,
    frame_revision: u64,
    acknowledged_play_revision: u64,
    acknowledged_seek_revision: u64,
    frame: SemanticFrame,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventWindowsWire {
    current: Box<RawValue>,
    proposed: Box<RawValue>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BrowserErrorWire {
    kind: Box<str>,
    message: Box<str>,
}

fn parse_with_expected_sources(
    bytes: &[u8],
    expected: ExpectedRuntimeSources<'_>,
) -> Result<BrowserSemanticObservations, BrowserObservationError> {
    if bytes.is_empty() || bytes.len() > MAX_BROWSER_OBSERVATION_BYTES {
        return Err(BrowserObservationError::InvalidLength);
    }
    let document: BrowserDocument =
        serde_json::from_slice(bytes).map_err(|error| BrowserObservationError::InvalidJson {
            message: bounded(error.to_string(), 512),
        })?;
    let format_version = document.format_version;
    let (browser_capture, event_windows, observations) = match document.state.as_ref() {
        "complete" if document.error.is_none() => (
            document.browser_capture.ok_or_else(|| {
                invalid_json("complete browser observation is missing browser_capture")
            })?,
            document.event_windows.ok_or_else(|| {
                invalid_json("complete browser observation is missing event_windows")
            })?,
            document.observations.ok_or_else(|| {
                invalid_json("complete browser observation is missing observations")
            })?,
        ),
        "error"
            if document.browser_capture.is_none()
                && document.event_windows.is_none()
                && document.observations.is_none() =>
        {
            validate_format_version(format_version)?;
            let error = document
                .error
                .ok_or_else(|| invalid_json("browser error document is missing error"))?;
            if !valid_text(&error.kind, MAX_ERROR_KIND_BYTES)
                || !valid_text(&error.message, MAX_ERROR_MESSAGE_BYTES)
            {
                return Err(invalid_json("browser error fields violate fixed bounds"));
            }
            return Err(BrowserObservationError::BrowserReported {
                kind: error.kind,
                message: error.message,
            });
        }
        _ => {
            return Err(invalid_json(
                "browser observation state and fields do not form a closed terminal document",
            ));
        }
    };
    validate_format_version(format_version)?;
    let current_event_window = parse_raw_event_window(&event_windows.current, "current")?;
    let proposed_event_window = parse_raw_event_window(&event_windows.proposed, "proposed")?;
    if !is_sha256(expected.nonce) {
        return Err(BrowserObservationError::InvalidExpectedNonce);
    }
    if browser_capture.nonce() != expected.nonce {
        return Err(BrowserObservationError::NonceMismatch);
    }

    let current_identity = validate_identity(
        NativeSource::Current,
        browser_capture.runtime_sources().current(),
        expected.current,
    )?;
    let proposed_identity = validate_identity(
        NativeSource::Proposed,
        browser_capture.runtime_sources().proposed(),
        expected.proposed,
    )?;
    let expected_count = SAMPLE_COUNT * 2;
    if observations.len() != expected_count {
        return Err(BrowserObservationError::ObservationCount {
            expected: expected_count,
            actual: observations.len(),
        });
    }

    let mut converted = Vec::with_capacity(expected_count);
    let mut prior_generations = [[0_u64; 3]; 2];
    for (index, wire) in observations.into_iter().enumerate() {
        let sample = SAMPLE_SCHEDULE[index / 2];
        let source = if index % 2 == 0 {
            NativeSource::Current
        } else {
            NativeSource::Proposed
        };
        let expected_source = source_id(source);
        if wire.source.as_ref() != expected_source || wire.sample.as_ref() != sample.id() {
            return Err(BrowserObservationError::ObservationOrder {
                index,
                expected_source,
                expected_sample: sample.id(),
                actual_source: wire.source,
                actual_sample: wire.sample,
            });
        }
        let source_index = usize::from(source == NativeSource::Proposed);
        for (field_index, (field, generation)) in [
            ("frame_revision", wire.frame_revision),
            (
                "acknowledged_play_revision",
                wire.acknowledged_play_revision,
            ),
            (
                "acknowledged_seek_revision",
                wire.acknowledged_seek_revision,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            if generation == 0 || generation <= prior_generations[source_index][field_index] {
                return Err(BrowserObservationError::InvalidGeneration {
                    capture_source: source,
                    field,
                });
            }
            prior_generations[source_index][field_index] = generation;
        }
        if !wire
            .frame
            .skin_layers()
            .eq(sample.skin_layers().iter().copied())
        {
            return Err(BrowserObservationError::SkinLayersMismatch {
                capture_source: source,
                sample: sample.id(),
            });
        }
        converted.push(BrowserSemanticObservation {
            source,
            sample,
            frame_revision: wire.frame_revision,
            acknowledged_play_revision: wire.acknowledged_play_revision,
            acknowledged_seek_revision: wire.acknowledged_seek_revision,
            frame: wire.frame,
        });
    }

    for (index, (observation, receipt)) in converted
        .iter()
        .zip(browser_capture.screenshots())
        .enumerate()
    {
        let expected_source = match observation.source {
            NativeSource::Current => BrowserCaptureSource::Current,
            NativeSource::Proposed => BrowserCaptureSource::Proposed,
        };
        for (field, agrees) in [
            ("source", receipt.source() == expected_source),
            ("sample", receipt.sample() == observation.sample),
            (
                "frame_revision",
                receipt.frame_revision() == observation.frame_revision,
            ),
            (
                "acknowledged_play_revision",
                receipt.acknowledged_play_revision() == observation.acknowledged_play_revision,
            ),
            (
                "acknowledged_seek_revision",
                receipt.acknowledged_seek_revision() == observation.acknowledged_seek_revision,
            ),
        ] {
            if !agrees {
                return Err(BrowserObservationError::ScreenshotBindingMismatch { index, field });
            }
        }
    }

    Ok(BrowserSemanticObservations {
        current_identity,
        proposed_identity,
        browser_capture,
        current_event_window,
        proposed_event_window,
        observations: converted.into_boxed_slice(),
    })
}

fn parse_raw_event_window(
    raw: &RawValue,
    source: &'static str,
) -> Result<EventWindowDocument, BrowserObservationError> {
    parse_event_window_json(raw.get().as_bytes()).map_err(|error| {
        BrowserObservationError::InvalidJson {
            message: bounded(format!("invalid {source} event window: {error}"), 512),
        }
    })
}

fn invalid_json(message: impl Into<Box<str>>) -> BrowserObservationError {
    BrowserObservationError::InvalidJson {
        message: message.into(),
    }
}

fn validate_format_version(actual: u8) -> Result<(), BrowserObservationError> {
    if actual == BROWSER_OBSERVATION_FORMAT_VERSION {
        Ok(())
    } else {
        Err(BrowserObservationError::WrongFormatVersion {
            expected: BROWSER_OBSERVATION_FORMAT_VERSION,
            actual,
        })
    }
}

fn validate_identity(
    capture_source: NativeSource,
    actual: &CaptureRuntimeIdentity,
    expected: ExpectedRuntimeIdentity<'_>,
) -> Result<BrowserRuntimeIdentity, BrowserObservationError> {
    for (field, value, expected_value) in [
        (
            "manifest_sha256",
            actual.manifest_sha256(),
            expected.manifest_sha256,
        ),
        (
            "content_sha256",
            actual.content_sha256(),
            expected.content_sha256,
        ),
    ] {
        if !is_sha256(value) || value != expected_value {
            return Err(BrowserObservationError::RuntimeIdentityMismatch {
                capture_source,
                field,
            });
        }
    }
    Ok(BrowserRuntimeIdentity {
        manifest_sha256: actual.manifest_sha256().into(),
        content_sha256: actual.content_sha256().into(),
    })
}

const fn source_id(source: NativeSource) -> &'static str {
    match source {
        NativeSource::Current => "current",
        NativeSource::Proposed => "proposed",
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_text(value: &str, max_bytes: usize) -> bool {
    !value.is_empty() && value.len() <= max_bytes && !value.chars().any(char::is_control)
}

fn bounded(mut value: String, max_bytes: usize) -> Box<str> {
    if value.len() > max_bytes {
        let mut end = max_bytes.saturating_sub(3);
        while !value.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        value.truncate(end);
        value.push_str("...");
    }
    value.into_boxed_str()
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    const CURRENT_MANIFEST: &str =
        "1111111111111111111111111111111111111111111111111111111111111111";
    const CURRENT_CONTENT: &str =
        "2222222222222222222222222222222222222222222222222222222222222222";
    const PROPOSED_MANIFEST: &str =
        "3333333333333333333333333333333333333333333333333333333333333333";
    const PROPOSED_CONTENT: &str =
        "4444444444444444444444444444444444444444444444444444444444444444";
    const NONCE: &str = "5555555555555555555555555555555555555555555555555555555555555555";
    const PNG_SHA256: &str = "6666666666666666666666666666666666666666666666666666666666666666";

    fn expected() -> ExpectedRuntimeSources<'static> {
        ExpectedRuntimeSources {
            nonce: NONCE,
            current: ExpectedRuntimeIdentity {
                manifest_sha256: CURRENT_MANIFEST,
                content_sha256: CURRENT_CONTENT,
            },
            proposed: ExpectedRuntimeIdentity {
                manifest_sha256: PROPOSED_MANIFEST,
                content_sha256: PROPOSED_CONTENT,
            },
        }
    }

    fn frame(skin_layers: &[&str]) -> Value {
        json!({
            "format_version": 1,
            "default_skin": "default",
            "skin_layers": skin_layers,
            "bones": [],
            "slots": [],
            "draw_items": [],
            "ik_constraints": [],
            "transform_constraints": [],
            "active_diagnostics": []
        })
    }

    fn event_window(integer: i32) -> Value {
        json!({
            "format_version": 1,
            "window_id": "sway-events",
            "animation": "sway",
            "start_ns": 0,
            "end_ns": 1_000_000_000_u64,
            "events": [
                {
                    "animation": "sway",
                    "name": "start",
                    "local_time_ns": 0,
                    "loop_index": 0,
                    "integer": integer,
                    "float": 0.0,
                    "string": null,
                    "volume": 1.0,
                    "balance": 0.0,
                    "diagnostic_codes": []
                },
                {
                    "animation": "sway",
                    "name": "end",
                    "local_time_ns": 1_000_000_000_u64,
                    "loop_index": 0,
                    "integer": integer + 1,
                    "float": 1.25,
                    "string": "done",
                    "volume": 0.5,
                    "balance": -0.25,
                    "diagnostic_codes": []
                }
            ]
        })
    }

    fn complete_value() -> Value {
        let mut observations = Vec::new();
        let mut screenshots = Vec::new();
        for (sample_index, sample) in SAMPLE_SCHEDULE.into_iter().enumerate() {
            for (source_index, source) in ["current", "proposed"].into_iter().enumerate() {
                let generation = u64::try_from(sample_index * 2 + source_index + 1)
                    .expect("eight generations fit u64");
                let runtime_identity = if source == "current" {
                    json!({
                        "manifest_sha256": CURRENT_MANIFEST,
                        "content_sha256": CURRENT_CONTENT
                    })
                } else {
                    json!({
                        "manifest_sha256": PROPOSED_MANIFEST,
                        "content_sha256": PROPOSED_CONTENT
                    })
                };
                observations.push(json!({
                    "source": source,
                    "sample": sample.id(),
                    "frame_revision": generation,
                    "acknowledged_play_revision": generation + 10,
                    "acknowledged_seek_revision": generation + 20,
                    "frame": frame(sample.skin_layers()),
                }));
                screenshots.push(json!({
                    "sequence": sample_index * 2 + source_index,
                    "source": source,
                    "sample": sample.id(),
                    "runtime_identity": runtime_identity,
                    "frame_revision": generation,
                    "acknowledged_play_revision": generation + 10,
                    "acknowledged_seek_revision": generation + 20,
                    "png_byte_length": 1024 + sample_index * 2 + source_index,
                    "png_sha256": PNG_SHA256
                }));
            }
        }
        json!({
            "format_version": 3,
            "state": "complete",
            "browser_capture": {
                "format_version": 1,
                "state": "complete",
                "nonce": NONCE,
                "runtime_sources": {
                    "current": {
                        "manifest_sha256": CURRENT_MANIFEST,
                        "content_sha256": CURRENT_CONTENT
                    },
                    "proposed": {
                        "manifest_sha256": PROPOSED_MANIFEST,
                        "content_sha256": PROPOSED_CONTENT
                    }
                },
                "screenshots": screenshots
            },
            "event_windows": {
                "current": event_window(10),
                "proposed": event_window(20)
            },
            "observations": observations
        })
    }

    fn complete_bytes() -> Vec<u8> {
        serde_json::to_vec(&complete_value()).expect("test document encodes")
    }

    #[test]
    fn complete_fixed_schedule_is_bound_and_permanently_gate_ineligible() {
        let parsed = parse_with_expected_sources(&complete_bytes(), expected())
            .expect("closed browser output is accepted");

        assert_eq!(parsed.observations().len(), SAMPLE_COUNT * 2);
        assert_eq!(parsed.current_identity().content_sha256(), CURRENT_CONTENT);
        assert_eq!(
            parsed.proposed_identity().manifest_sha256(),
            PROPOSED_MANIFEST
        );
        assert!(!parsed.representative_gate_eligible());
        assert_eq!(parsed.browser_capture().nonce(), NONCE);
        assert_eq!(
            parsed.browser_capture().screenshots().len(),
            SAMPLE_COUNT * 2
        );
        assert_eq!(
            parsed
                .current_event_window()
                .events()
                .iter()
                .map(|event| event.integer())
                .collect::<Vec<_>>(),
            [10, 11]
        );
        assert_eq!(
            parsed
                .proposed_event_window()
                .events()
                .iter()
                .map(|event| event.integer())
                .collect::<Vec<_>>(),
            [20, 21]
        );
        assert!(!parsed.current_event_window().gate_eligible());
        assert!(!parsed.proposed_event_window().gate_eligible());
        for (index, observation) in parsed.observations().iter().enumerate() {
            assert_eq!(observation.sample(), SAMPLE_SCHEDULE[index / 2]);
            assert_eq!(
                observation.source(),
                if index % 2 == 0 {
                    NativeSource::Current
                } else {
                    NativeSource::Proposed
                }
            );
        }
    }

    #[test]
    fn identity_swap_or_malformed_digest_fails_closed() {
        let mut swapped = complete_value();
        swapped["browser_capture"]["runtime_sources"]["current"]["content_sha256"] =
            Value::String(PROPOSED_CONTENT.to_owned());
        for receipt in swapped["browser_capture"]["screenshots"]
            .as_array_mut()
            .expect("test screenshots are an array")
            .iter_mut()
            .filter(|receipt| receipt["source"] == "current")
        {
            receipt["runtime_identity"]["content_sha256"] =
                Value::String(PROPOSED_CONTENT.to_owned());
        }
        assert_eq!(
            parse_with_expected_sources(
                &serde_json::to_vec(&swapped).expect("test document encodes"),
                expected(),
            ),
            Err(BrowserObservationError::RuntimeIdentityMismatch {
                capture_source: NativeSource::Current,
                field: "content_sha256",
            })
        );

        let mut uppercase = complete_value();
        uppercase["browser_capture"]["runtime_sources"]["proposed"]["manifest_sha256"] =
            Value::String("A".repeat(64));
        assert!(matches!(
            parse_with_expected_sources(
                &serde_json::to_vec(&uppercase).expect("test document encodes"),
                expected(),
            ),
            Err(BrowserObservationError::InvalidJson { .. })
        ));
    }

    #[test]
    fn event_windows_are_required_closed_strict_and_ordered() {
        let rejected = |value: &Value| {
            assert!(matches!(
                parse_with_expected_sources(
                    &serde_json::to_vec(value).expect("test document encodes"),
                    expected(),
                ),
                Err(BrowserObservationError::InvalidJson { .. })
            ));
        };

        let mut missing_windows = complete_value();
        missing_windows
            .as_object_mut()
            .expect("test document is an object")
            .remove("event_windows");
        rejected(&missing_windows);

        let mut missing_role = complete_value();
        missing_role["event_windows"]
            .as_object_mut()
            .expect("test event windows are an object")
            .remove("current");
        rejected(&missing_role);

        let mut unknown = complete_value();
        unknown["event_windows"]["extra"] = json!(true);
        rejected(&unknown);

        let mut malformed = complete_value();
        malformed["event_windows"]["current"]["events"][0]
            .as_object_mut()
            .expect("test event is an object")
            .remove("string");
        rejected(&malformed);

        let mut wrong_contract = complete_value();
        wrong_contract["event_windows"]["current"]["start_ns"] = json!(1);
        rejected(&wrong_contract);

        let mut wrong_order = complete_value();
        wrong_order["event_windows"]["proposed"]["events"]
            .as_array_mut()
            .expect("test events are an array")
            .swap(0, 1);
        rejected(&wrong_order);

        let value = complete_value();
        let duplicate_value =
            serde_json::to_string(&value["event_windows"]).expect("test windows encode");
        let text = serde_json::to_string(&value).expect("test document encodes");
        let duplicate = text.replacen(
            "\"event_windows\":",
            &format!("\"event_windows\":{duplicate_value},\"event_windows\":"),
            1,
        );
        assert!(matches!(
            parse_with_expected_sources(duplicate.as_bytes(), expected()),
            Err(BrowserObservationError::InvalidJson { .. })
        ));

        let padded = text.replacen(
            "\"event_windows\":{\"current\":{",
            &format!(
                "\"event_windows\":{{\"current\":{{{}",
                " ".repeat(MAX_EVENT_WINDOW_BYTES)
            ),
            1,
        );
        assert!(padded.len() < MAX_BROWSER_OBSERVATION_BYTES);
        assert!(matches!(
            parse_with_expected_sources(padded.as_bytes(), expected()),
            Err(BrowserObservationError::InvalidJson { .. })
        ));
    }

    #[test]
    fn schedule_requires_exact_count_order_labels_and_skin_layers() {
        let mut missing = complete_value();
        missing["observations"]
            .as_array_mut()
            .expect("test observations are an array")
            .pop();
        assert!(matches!(
            parse_with_expected_sources(
                &serde_json::to_vec(&missing).expect("test document encodes"),
                expected(),
            ),
            Err(BrowserObservationError::ObservationCount { actual: 7, .. })
        ));

        let mut reordered = complete_value();
        reordered["observations"]
            .as_array_mut()
            .expect("test observations are an array")
            .swap(0, 1);
        assert!(matches!(
            parse_with_expected_sources(
                &serde_json::to_vec(&reordered).expect("test document encodes"),
                expected(),
            ),
            Err(BrowserObservationError::ObservationOrder { index: 0, .. })
        ));

        let mut wrong_skin = complete_value();
        wrong_skin["observations"][4]["frame"]["skin_layers"] = json!([]);
        assert_eq!(
            parse_with_expected_sources(
                &serde_json::to_vec(&wrong_skin).expect("test document encodes"),
                expected(),
            ),
            Err(BrowserObservationError::SkinLayersMismatch {
                capture_source: NativeSource::Current,
                sample: "sway-alternate-skin",
            })
        );
    }

    #[test]
    fn generations_must_be_nonzero_and_increase_per_source() {
        let mut zero = complete_value();
        zero["observations"][0]["frame_revision"] = json!(0);
        assert_eq!(
            parse_with_expected_sources(
                &serde_json::to_vec(&zero).expect("test document encodes"),
                expected(),
            ),
            Err(BrowserObservationError::InvalidGeneration {
                capture_source: NativeSource::Current,
                field: "frame_revision",
            })
        );

        let mut stale = complete_value();
        stale["observations"][2]["acknowledged_seek_revision"] =
            stale["observations"][0]["acknowledged_seek_revision"].clone();
        assert_eq!(
            parse_with_expected_sources(
                &serde_json::to_vec(&stale).expect("test document encodes"),
                expected(),
            ),
            Err(BrowserObservationError::InvalidGeneration {
                capture_source: NativeSource::Current,
                field: "acknowledged_seek_revision",
            })
        );
    }

    #[test]
    fn screenshot_receipts_must_bind_the_exact_semantic_generations() {
        let mut mismatched = complete_value();
        mismatched["observations"][0]["frame_revision"] = json!(2);
        assert_eq!(
            parse_with_expected_sources(
                &serde_json::to_vec(&mismatched).expect("test document encodes"),
                expected(),
            ),
            Err(BrowserObservationError::ScreenshotBindingMismatch {
                index: 0,
                field: "frame_revision",
            })
        );

        let mut invalid_nonce = complete_value();
        invalid_nonce["browser_capture"]["nonce"] = json!("short");
        assert!(matches!(
            parse_with_expected_sources(
                &serde_json::to_vec(&invalid_nonce).expect("test document encodes"),
                expected(),
            ),
            Err(BrowserObservationError::InvalidJson { .. })
        ));

        let mut replayed = complete_value();
        replayed["browser_capture"]["nonce"] = json!("7".repeat(64));
        assert_eq!(
            parse_with_expected_sources(
                &serde_json::to_vec(&replayed).expect("test document encodes"),
                expected(),
            ),
            Err(BrowserObservationError::NonceMismatch)
        );

        let mut invalid_expected = expected();
        invalid_expected.nonce = "short";
        assert_eq!(
            parse_with_expected_sources(&complete_bytes(), invalid_expected),
            Err(BrowserObservationError::InvalidExpectedNonce)
        );
    }

    #[test]
    fn closed_json_rejects_unknown_and_duplicate_fields() {
        let mut unknown = complete_value();
        unknown["browser_capture"]["runtime_sources"]["current"]["extra"] = json!(true);
        assert!(matches!(
            parse_with_expected_sources(
                &serde_json::to_vec(&unknown).expect("test document encodes"),
                expected(),
            ),
            Err(BrowserObservationError::InvalidJson { .. })
        ));

        let bytes = complete_bytes();
        let text = String::from_utf8(bytes).expect("test JSON is UTF-8");
        let duplicate = text.replacen(
            "\"format_version\":3",
            "\"format_version\":3,\"format_version\":3",
            1,
        );
        assert!(matches!(
            parse_with_expected_sources(duplicate.as_bytes(), expected()),
            Err(BrowserObservationError::InvalidJson { .. })
        ));

        let mut null_error = complete_value();
        null_error["error"] = Value::Null;
        assert!(matches!(
            parse_with_expected_sources(
                &serde_json::to_vec(&null_error).expect("test document encodes"),
                expected(),
            ),
            Err(BrowserObservationError::InvalidJson { .. })
        ));

        let mut nested = complete_value();
        nested["observations"][0]["frame"]["active_diagnostics"] = json!([{
            "severity": "warning",
            "code": "unknown_field",
            "scope": {"kind": "asset", "extra": true},
            "message": "detail"
        }]);
        assert!(matches!(
            parse_with_expected_sources(
                &serde_json::to_vec(&nested).expect("test document encodes"),
                expected(),
            ),
            Err(BrowserObservationError::InvalidJson { .. })
        ));
    }

    #[test]
    fn terminal_browser_error_is_bounded_and_never_observations() {
        let error = br#"{
          "format_version":3,
          "state":"error",
          "error":{"kind":"sample_timeout","message":"no frame"}
        }"#;
        assert_eq!(
            parse_with_expected_sources(error, expected()),
            Err(BrowserObservationError::BrowserReported {
                kind: "sample_timeout".into(),
                message: "no frame".into(),
            })
        );

        let error_with_null_complete_fields = br#"{
          "format_version":3,
          "state":"error",
          "browser_capture":null,
          "event_windows":null,
          "observations":null,
          "error":{"kind":"sample_timeout","message":"no frame"}
        }"#;
        assert!(matches!(
            parse_with_expected_sources(error_with_null_complete_fields, expected()),
            Err(BrowserObservationError::InvalidJson { .. })
        ));
    }

    #[test]
    fn wrong_format_and_byte_budget_fail_closed() {
        let mut wrong = complete_value();
        wrong["format_version"] = json!(2);
        assert_eq!(
            parse_with_expected_sources(
                &serde_json::to_vec(&wrong).expect("test document encodes"),
                expected(),
            ),
            Err(BrowserObservationError::WrongFormatVersion {
                expected: 3,
                actual: 2,
            })
        );
        assert_eq!(
            MAX_BROWSER_OBSERVATION_BYTES,
            8 * 1024 * 1024
                + 64 * 1024
                + 2 * MAX_EVENT_WINDOW_BYTES
                + EVENT_WINDOWS_ENVELOPE_OVERHEAD_BYTES
        );
        assert_eq!(
            parse_with_expected_sources(&[], expected()),
            Err(BrowserObservationError::InvalidLength)
        );
        assert_eq!(
            parse_with_expected_sources(&vec![b' '; MAX_BROWSER_OBSERVATION_BYTES + 1], expected(),),
            Err(BrowserObservationError::InvalidLength)
        );
    }
}

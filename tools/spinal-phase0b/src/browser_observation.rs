//! Strict parsing of the fixed browser semantic-observation envelope.
//!
//! The parser binds the two claimed browser runtime identities to one already
//! loaded [`crate::LoadedCaseRuntimeBundles`] pair and validates the complete
//! fixed v1 schedule. It deliberately cannot authenticate which browser
//! process produced DOM bytes: the current browser harness has no run nonce or
//! build provenance. Parsed values are therefore conformance observations,
//! never Phase 0B evidence or a gate decision.

use serde::Deserialize;
use spinal::SemanticFrame;
use thiserror::Error;

use crate::{
    LoadedCaseRuntimeBundles,
    capture::{NativeSample, NativeSource},
    contract::{SAMPLE_COUNT, SAMPLE_SCHEDULE},
};

/// Browser observation-envelope schema accepted by this parser.
pub const BROWSER_OBSERVATION_FORMAT_VERSION: u8 = 1;

/// Maximum complete browser observation document size.
pub const MAX_BROWSER_OBSERVATION_BYTES: usize = 8 * 1024 * 1024 + 64 * 1024;

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
}

/// Parses a fixed browser envelope and binds it to exact loaded bundle bytes.
pub fn parse_browser_semantic_observations(
    bytes: &[u8],
    bundles: &LoadedCaseRuntimeBundles,
) -> Result<BrowserSemanticObservations, BrowserObservationError> {
    let expected = ExpectedRuntimeSources {
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
    current: ExpectedRuntimeIdentity<'a>,
    proposed: ExpectedRuntimeIdentity<'a>,
}

#[derive(Deserialize)]
#[serde(tag = "state", deny_unknown_fields)]
enum BrowserDocument {
    #[serde(rename = "complete")]
    Complete {
        format_version: u8,
        runtime_sources: RuntimeSourcesWire,
        observations: Vec<ObservationWire>,
    },
    #[serde(rename = "error")]
    Error {
        format_version: u8,
        error: BrowserErrorWire,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeSourcesWire {
    current: RuntimeIdentityWire,
    proposed: RuntimeIdentityWire,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeIdentityWire {
    manifest_sha256: Box<str>,
    content_sha256: Box<str>,
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
    let (format_version, runtime_sources, observations) = match document {
        BrowserDocument::Complete {
            format_version,
            runtime_sources,
            observations,
        } => (format_version, runtime_sources, observations),
        BrowserDocument::Error {
            format_version,
            error,
        } => {
            validate_format_version(format_version)?;
            if !valid_text(&error.kind, MAX_ERROR_KIND_BYTES)
                || !valid_text(&error.message, MAX_ERROR_MESSAGE_BYTES)
            {
                return Err(BrowserObservationError::InvalidJson {
                    message: "browser error fields violate fixed bounds".into(),
                });
            }
            return Err(BrowserObservationError::BrowserReported {
                kind: error.kind,
                message: error.message,
            });
        }
    };
    validate_format_version(format_version)?;

    let current_identity = validate_identity(
        NativeSource::Current,
        runtime_sources.current,
        expected.current,
    )?;
    let proposed_identity = validate_identity(
        NativeSource::Proposed,
        runtime_sources.proposed,
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

    Ok(BrowserSemanticObservations {
        current_identity,
        proposed_identity,
        observations: converted.into_boxed_slice(),
    })
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
    actual: RuntimeIdentityWire,
    expected: ExpectedRuntimeIdentity<'_>,
) -> Result<BrowserRuntimeIdentity, BrowserObservationError> {
    for (field, value, expected_value) in [
        (
            "manifest_sha256",
            actual.manifest_sha256.as_ref(),
            expected.manifest_sha256,
        ),
        (
            "content_sha256",
            actual.content_sha256.as_ref(),
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
        manifest_sha256: actual.manifest_sha256,
        content_sha256: actual.content_sha256,
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

    fn expected() -> ExpectedRuntimeSources<'static> {
        ExpectedRuntimeSources {
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

    fn complete_value() -> Value {
        let mut observations = Vec::new();
        for (sample_index, sample) in SAMPLE_SCHEDULE.into_iter().enumerate() {
            for (source_index, source) in ["current", "proposed"].into_iter().enumerate() {
                let generation = u64::try_from(sample_index * 2 + source_index + 1)
                    .expect("eight generations fit u64");
                observations.push(json!({
                    "source": source,
                    "sample": sample.id(),
                    "frame_revision": generation,
                    "acknowledged_play_revision": generation + 10,
                    "acknowledged_seek_revision": generation + 20,
                    "frame": frame(sample.skin_layers()),
                }));
            }
        }
        json!({
            "format_version": 1,
            "state": "complete",
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
        swapped["runtime_sources"]["current"]["content_sha256"] =
            Value::String(PROPOSED_CONTENT.to_owned());
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
        uppercase["runtime_sources"]["proposed"]["manifest_sha256"] = Value::String("A".repeat(64));
        assert_eq!(
            parse_with_expected_sources(
                &serde_json::to_vec(&uppercase).expect("test document encodes"),
                expected(),
            ),
            Err(BrowserObservationError::RuntimeIdentityMismatch {
                capture_source: NativeSource::Proposed,
                field: "manifest_sha256",
            })
        );
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
    fn closed_json_rejects_unknown_and_duplicate_fields() {
        let mut unknown = complete_value();
        unknown["runtime_sources"]["current"]["extra"] = json!(true);
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
            "\"format_version\":1",
            "\"format_version\":1,\"format_version\":1",
            1,
        );
        assert!(matches!(
            parse_with_expected_sources(duplicate.as_bytes(), expected()),
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
          "format_version":1,
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
                expected: 1,
                actual: 2,
            })
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

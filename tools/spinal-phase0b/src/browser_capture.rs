//! Closed browser/driver presentation-capture protocol for Phase 0B rehearsal.
//!
//! The browser owns the fixed semantic presentation schedule. A driver proves
//! freshness with a nonce, captures only requested frames, and acknowledges
//! each PNG by exact byte length and digest. This module authenticates neither
//! browser nor driver provenance, so every value remains permanently
//! ineligible to decide a representative gate.

use std::fmt;

use serde::{Deserialize, Serialize, Serializer};
use thiserror::Error;

use crate::{
    capture::NativeSource,
    contract::{SAMPLE_COUNT, SAMPLE_SCHEDULE, Sample},
    pixel_compare::MAX_ENCODED_PNG_BYTES,
};

/// Browser presentation-capture protocol version.
pub const BROWSER_CAPTURE_FORMAT_VERSION: u8 = 1;

/// Maximum complete JSON message size accepted or emitted by this protocol.
pub const MAX_BROWSER_CAPTURE_JSON_BYTES: usize = 64 * 1024;

/// Exact number of screenshots in the fixed sample-major, Current-first run.
pub const SCREENSHOT_SEQUENCE_COUNT: usize = SAMPLE_COUNT * 2;

/// Gate eligibility of every result produced by this protocol.
pub const BROWSER_CAPTURE_GATE_ELIGIBLE: bool = false;

/// Largest generation that round-trips exactly through a JSON/JavaScript number.
pub const MAX_BROWSER_CAPTURE_GENERATION: u64 = 9_007_199_254_740_991;

/// One immutable side of the browser presentation comparison.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptureSource {
    /// The immutable Current runtime.
    Current,
    /// The immutable Proposed runtime.
    Proposed,
}

impl CaptureSource {
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

impl From<NativeSource> for CaptureSource {
    fn from(value: NativeSource) -> Self {
        match value {
            NativeSource::Current => Self::Current,
            NativeSource::Proposed => Self::Proposed,
        }
    }
}

/// Exact immutable identity of one browser runtime bundle.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeIdentity {
    manifest_sha256: Box<str>,
    content_sha256: Box<str>,
}

impl RuntimeIdentity {
    /// Constructs an identity from two lowercase SHA-256 digests.
    pub fn new(
        manifest_sha256: impl Into<Box<str>>,
        content_sha256: impl Into<Box<str>>,
    ) -> Result<Self, BrowserCaptureError> {
        let value = Self {
            manifest_sha256: manifest_sha256.into(),
            content_sha256: content_sha256.into(),
        };
        validate_digest("manifest_sha256", &value.manifest_sha256)?;
        validate_digest("content_sha256", &value.content_sha256)?;
        Ok(value)
    }

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

    /// Returns `false`; an identity is not representative gate evidence.
    #[must_use]
    pub const fn gate_eligible(&self) -> bool {
        BROWSER_CAPTURE_GATE_ELIGIBLE
    }
}

/// Exact identities of both immutable browser runtime sources.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeSources {
    current: RuntimeIdentity,
    proposed: RuntimeIdentity,
}

impl RuntimeSources {
    /// Constructs the fixed Current/Proposed identity pair.
    #[must_use]
    pub const fn new(current: RuntimeIdentity, proposed: RuntimeIdentity) -> Self {
        Self { current, proposed }
    }

    /// Returns the immutable Current identity.
    #[must_use]
    pub const fn current(&self) -> &RuntimeIdentity {
        &self.current
    }

    /// Returns the immutable Proposed identity.
    #[must_use]
    pub const fn proposed(&self) -> &RuntimeIdentity {
        &self.proposed
    }

    /// Returns `false`; source bindings alone are not gate evidence.
    #[must_use]
    pub const fn gate_eligible(&self) -> bool {
        BROWSER_CAPTURE_GATE_ELIGIBLE
    }

    fn get(&self, source: CaptureSource) -> &RuntimeIdentity {
        match source {
            CaptureSource::Current => &self.current,
            CaptureSource::Proposed => &self.proposed,
        }
    }
}

/// Exact semantic generations presented before a browser screenshot request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScreenshotPresentation {
    source: CaptureSource,
    sample: Sample,
    frame_revision: u64,
    acknowledged_play_revision: u64,
    acknowledged_seek_revision: u64,
}

impl ScreenshotPresentation {
    /// Constructs a presentation binding; every generation must be nonzero.
    pub fn new(
        source: CaptureSource,
        sample: Sample,
        frame_revision: u64,
        acknowledged_play_revision: u64,
        acknowledged_seek_revision: u64,
    ) -> Result<Self, BrowserCaptureError> {
        let value = Self {
            source,
            sample,
            frame_revision,
            acknowledged_play_revision,
            acknowledged_seek_revision,
        };
        validate_generations(0, &value)?;
        Ok(value)
    }

    /// Returns the immutable runtime side.
    #[must_use]
    pub const fn source(&self) -> CaptureSource {
        self.source
    }

    /// Returns the fixed semantic sample.
    #[must_use]
    pub const fn sample(&self) -> Sample {
        self.sample
    }

    /// Returns the captured semantic-frame generation.
    #[must_use]
    pub const fn frame_revision(&self) -> u64 {
        self.frame_revision
    }

    /// Returns the acknowledged play-command generation.
    #[must_use]
    pub const fn acknowledged_play_revision(&self) -> u64 {
        self.acknowledged_play_revision
    }

    /// Returns the acknowledged seek-command generation.
    #[must_use]
    pub const fn acknowledged_seek_revision(&self) -> u64 {
        self.acknowledged_seek_revision
    }

    /// Returns `false`; a presentation binding is not gate evidence.
    #[must_use]
    pub const fn gate_eligible(&self) -> bool {
        BROWSER_CAPTURE_GATE_ELIGIBLE
    }
}

/// One exact screenshot acknowledgement retained by the browser.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ScreenshotReceipt {
    sequence: u8,
    source: CaptureSource,
    #[serde(serialize_with = "serialize_sample")]
    sample: Sample,
    runtime_identity: RuntimeIdentity,
    frame_revision: u64,
    acknowledged_play_revision: u64,
    acknowledged_seek_revision: u64,
    png_byte_length: usize,
    png_sha256: Box<str>,
}

impl ScreenshotReceipt {
    /// Returns the zero-based fixed sequence number.
    #[must_use]
    pub const fn sequence(&self) -> u8 {
        self.sequence
    }

    /// Returns the immutable runtime side.
    #[must_use]
    pub const fn source(&self) -> CaptureSource {
        self.source
    }

    /// Returns the fixed semantic sample.
    #[must_use]
    pub const fn sample(&self) -> Sample {
        self.sample
    }

    /// Returns the exact runtime identity acknowledged for this PNG.
    #[must_use]
    pub const fn runtime_identity(&self) -> &RuntimeIdentity {
        &self.runtime_identity
    }

    /// Returns the semantic-frame generation bound to this PNG.
    #[must_use]
    pub const fn frame_revision(&self) -> u64 {
        self.frame_revision
    }

    /// Returns the play generation bound to this PNG.
    #[must_use]
    pub const fn acknowledged_play_revision(&self) -> u64 {
        self.acknowledged_play_revision
    }

    /// Returns the seek generation bound to this PNG.
    #[must_use]
    pub const fn acknowledged_seek_revision(&self) -> u64 {
        self.acknowledged_seek_revision
    }

    /// Returns the complete encoded PNG byte length.
    #[must_use]
    pub const fn png_byte_length(&self) -> usize {
        self.png_byte_length
    }

    /// Returns the lowercase SHA-256 of the complete encoded PNG bytes.
    #[must_use]
    pub fn png_sha256(&self) -> &str {
        &self.png_sha256
    }

    /// Returns `false`; a receipt is not representative gate evidence.
    #[must_use]
    pub const fn gate_eligible(&self) -> bool {
        BROWSER_CAPTURE_GATE_ELIGIBLE
    }
}

/// A validated inbound driver message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DriverMessage {
    /// Freshness challenge that must begin a session.
    Challenge {
        /// Runner-generated 256-bit lowercase hexadecimal nonce.
        nonce: Box<str>,
    },
    /// Exact PNG acknowledgement for the outstanding request.
    ScreenshotAck {
        /// Session nonce echoed by the driver.
        nonce: Box<str>,
        /// Acknowledged screenshot metadata.
        receipt: ScreenshotReceipt,
    },
}

impl DriverMessage {
    fn kind(&self) -> &'static str {
        match self {
            Self::Challenge { .. } => "challenge",
            Self::ScreenshotAck { .. } => "screenshot_ack",
        }
    }

    /// Returns `false`; a parsed transport message is never gate evidence.
    #[must_use]
    pub const fn gate_eligible(&self) -> bool {
        BROWSER_CAPTURE_GATE_ELIGIBLE
    }
}

/// One validated outbound browser control message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BrowserControlMessage {
    /// Browser acceptance of the fresh challenge and exact runtime identities.
    ChallengeAck {
        /// Echoed session nonce.
        nonce: Box<str>,
        /// Exact browser-loaded runtime identities.
        runtime_sources: RuntimeSources,
    },
    /// Request to capture the currently presented fixed-schedule sample.
    ScreenshotRequest {
        /// Session nonce.
        nonce: Box<str>,
        /// Zero-based fixed sequence number.
        sequence: u8,
        /// Exact runtime identity and semantic generations to capture.
        receipt_binding: ScreenshotReceipt,
    },
}

impl BrowserControlMessage {
    /// Serializes the closed message as compact bounded JSON.
    pub fn to_json(&self) -> Result<Box<[u8]>, BrowserCaptureError> {
        validate_control_message(self)?;
        encode_json(&ControlMessageWireRef::from(self))
    }

    /// Returns `false`; a control message is never representative evidence.
    #[must_use]
    pub const fn gate_eligible(&self) -> bool {
        BROWSER_CAPTURE_GATE_ELIGIBLE
    }
}

/// Complete fixed-order screenshot receipts for one nonce and runtime pair.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserCaptureComplete {
    nonce: Box<str>,
    runtime_sources: RuntimeSources,
    screenshots: Box<[ScreenshotReceipt]>,
}

impl BrowserCaptureComplete {
    /// Returns the exact driver challenge nonce.
    #[must_use]
    pub fn nonce(&self) -> &str {
        &self.nonce
    }

    /// Returns the exact browser-loaded runtime identities.
    #[must_use]
    pub const fn runtime_sources(&self) -> &RuntimeSources {
        &self.runtime_sources
    }

    /// Returns all eight receipts in sample-major, Current-first order.
    #[must_use]
    pub fn screenshots(&self) -> &[ScreenshotReceipt] {
        &self.screenshots
    }

    /// Binds this complete document to the host's expected session and bundles.
    pub fn validate_binding(
        &self,
        expected_nonce: &str,
        expected_sources: &RuntimeSources,
    ) -> Result<(), BrowserCaptureError> {
        validate_nonce(expected_nonce)?;
        if self.nonce.as_ref() != expected_nonce {
            return Err(BrowserCaptureError::NonceMismatch);
        }
        if &self.runtime_sources != expected_sources {
            return Err(BrowserCaptureError::BindingMismatch {
                sequence: 0,
                field: "runtime_sources",
            });
        }
        Ok(())
    }

    /// Serializes the closed terminal document as compact bounded JSON.
    pub fn to_json(&self) -> Result<Box<[u8]>, BrowserCaptureError> {
        encode_json(&CompleteWireRef {
            format_version: BROWSER_CAPTURE_FORMAT_VERSION,
            state: "complete",
            nonce: &self.nonce,
            runtime_sources: &self.runtime_sources,
            screenshots: &self.screenshots,
        })
    }

    /// Returns `false`; this unauthenticated protocol cannot open a gate.
    #[must_use]
    pub const fn gate_eligible(&self) -> bool {
        BROWSER_CAPTURE_GATE_ELIGIBLE
    }
}

impl Serialize for BrowserCaptureComplete {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        CompleteWireRef {
            format_version: BROWSER_CAPTURE_FORMAT_VERSION,
            state: "complete",
            nonce: &self.nonce,
            runtime_sources: &self.runtime_sources,
            screenshots: &self.screenshots,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for BrowserCaptureComplete {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = CompleteWire::deserialize(deserializer)?;
        validate_version(wire.format_version).map_err(serde::de::Error::custom)?;
        validate_nonce(&wire.nonce).map_err(serde::de::Error::custom)?;
        let sources = wire
            .runtime_sources
            .try_into_validated()
            .map_err(serde::de::Error::custom)?;
        validate_complete(wire.nonce, sources, wire.screenshots).map_err(serde::de::Error::custom)
    }
}

/// Result of accepting one screenshot acknowledgement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BrowserCaptureProgress {
    /// The acknowledgement was retained and another presentation is required.
    Pending,
    /// The eighth acknowledgement completed the fixed capture schedule.
    Complete(BrowserCaptureComplete),
}

impl BrowserCaptureProgress {
    /// Returns `false`; transport progress is never representative evidence.
    #[must_use]
    pub const fn gate_eligible(&self) -> bool {
        BROWSER_CAPTURE_GATE_ELIGIBLE
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SessionPhase {
    AwaitingChallenge,
    Ready,
    AwaitingReceipt(ScreenshotReceipt),
    Complete,
}

/// Small deterministic validator for one browser/driver capture session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserCaptureSession {
    runtime_sources: RuntimeSources,
    nonce: Option<Box<str>>,
    receipts: Vec<ScreenshotReceipt>,
    prior_generations: [[u64; 3]; 2],
    phase: SessionPhase,
}

impl BrowserCaptureSession {
    /// Creates a session awaiting exactly one fresh challenge.
    #[must_use]
    pub fn new(runtime_sources: RuntimeSources) -> Self {
        Self {
            runtime_sources,
            nonce: None,
            receipts: Vec::with_capacity(SCREENSHOT_SEQUENCE_COUNT),
            prior_generations: [[0; 3]; 2],
            phase: SessionPhase::AwaitingChallenge,
        }
    }

    /// Accepts the only legal initial challenge and returns its acknowledgement.
    pub fn accept_challenge(
        &mut self,
        message: DriverMessage,
    ) -> Result<BrowserControlMessage, BrowserCaptureError> {
        if self.phase != SessionPhase::AwaitingChallenge {
            return Err(BrowserCaptureError::InvalidState {
                expected: "awaiting_challenge",
                actual: self.phase.id(),
            });
        }
        let DriverMessage::Challenge { nonce } = message else {
            return Err(BrowserCaptureError::UnexpectedMessage {
                expected: "challenge",
                actual: message.kind(),
            });
        };
        validate_nonce(&nonce)?;
        self.nonce = Some(nonce.clone());
        self.phase = SessionPhase::Ready;
        Ok(BrowserControlMessage::ChallengeAck {
            nonce,
            runtime_sources: self.runtime_sources.clone(),
        })
    }

    /// Binds the next fixed schedule entry to freshly presented generations.
    pub fn request_screenshot(
        &mut self,
        presentation: ScreenshotPresentation,
    ) -> Result<BrowserControlMessage, BrowserCaptureError> {
        if self.phase != SessionPhase::Ready {
            return Err(BrowserCaptureError::InvalidState {
                expected: "ready",
                actual: self.phase.id(),
            });
        }
        let sequence = self.receipts.len();
        validate_schedule(sequence, presentation.source, presentation.sample)?;
        validate_generations(sequence, &presentation)?;
        let source_index = presentation.source.index();
        let generations = [
            presentation.frame_revision,
            presentation.acknowledged_play_revision,
            presentation.acknowledged_seek_revision,
        ];
        for (field, (actual, prior)) in GENERATION_FIELDS.into_iter().zip(
            generations
                .into_iter()
                .zip(self.prior_generations[source_index]),
        ) {
            if actual <= prior {
                return Err(BrowserCaptureError::GenerationOrder {
                    sequence,
                    capture_source: presentation.source,
                    field,
                });
            }
        }
        let receipt_binding = ScreenshotReceipt {
            sequence: u8::try_from(sequence).expect("fixed screenshot count fits u8"),
            source: presentation.source,
            sample: presentation.sample,
            runtime_identity: self.runtime_sources.get(presentation.source).clone(),
            frame_revision: presentation.frame_revision,
            acknowledged_play_revision: presentation.acknowledged_play_revision,
            acknowledged_seek_revision: presentation.acknowledged_seek_revision,
            png_byte_length: 1,
            png_sha256: ZERO_DIGEST.into(),
        };
        self.phase = SessionPhase::AwaitingReceipt(receipt_binding.clone());
        Ok(BrowserControlMessage::ScreenshotRequest {
            nonce: self.nonce.clone().expect("ready session has nonce"),
            sequence: receipt_binding.sequence,
            receipt_binding,
        })
    }

    /// Accepts only the exact outstanding request binding and PNG metadata.
    pub fn accept_screenshot_ack(
        &mut self,
        message: DriverMessage,
    ) -> Result<BrowserCaptureProgress, BrowserCaptureError> {
        let SessionPhase::AwaitingReceipt(expected) = &self.phase else {
            return Err(BrowserCaptureError::InvalidState {
                expected: "awaiting_receipt",
                actual: self.phase.id(),
            });
        };
        let DriverMessage::ScreenshotAck { nonce, receipt } = message else {
            return Err(BrowserCaptureError::UnexpectedMessage {
                expected: "screenshot_ack",
                actual: message.kind(),
            });
        };
        if Some(nonce.as_ref()) != self.nonce.as_deref() {
            return Err(BrowserCaptureError::NonceMismatch);
        }
        compare_receipt_binding(expected, &receipt)?;
        let source_index = receipt.source.index();
        self.prior_generations[source_index] = [
            receipt.frame_revision,
            receipt.acknowledged_play_revision,
            receipt.acknowledged_seek_revision,
        ];
        self.receipts.push(receipt);
        if self.receipts.len() == SCREENSHOT_SEQUENCE_COUNT {
            self.phase = SessionPhase::Complete;
            Ok(BrowserCaptureProgress::Complete(BrowserCaptureComplete {
                nonce: self.nonce.clone().expect("active session has nonce"),
                runtime_sources: self.runtime_sources.clone(),
                screenshots: self.receipts.clone().into_boxed_slice(),
            }))
        } else {
            self.phase = SessionPhase::Ready;
            Ok(BrowserCaptureProgress::Pending)
        }
    }

    /// Returns `false`; session validation alone cannot open a gate.
    #[must_use]
    pub const fn gate_eligible(&self) -> bool {
        BROWSER_CAPTURE_GATE_ELIGIBLE
    }
}

impl SessionPhase {
    const fn id(&self) -> &'static str {
        match self {
            Self::AwaitingChallenge => "awaiting_challenge",
            Self::Ready => "ready",
            Self::AwaitingReceipt(_) => "awaiting_receipt",
            Self::Complete => "complete",
        }
    }
}

/// Failure to parse, build, or advance the closed capture protocol.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum BrowserCaptureError {
    /// A complete JSON message is empty or exceeds 64 KiB.
    #[error("browser capture JSON must have length 1-{MAX_BROWSER_CAPTURE_JSON_BYTES}")]
    InvalidJsonLength,
    /// JSON syntax or a closed schema is invalid.
    #[error("invalid browser capture JSON: {message}")]
    InvalidJson {
        /// Bounded serde detail.
        message: Box<str>,
    },
    /// A message declares an unsupported protocol version.
    #[error("unsupported browser capture format {actual}; expected {expected}")]
    WrongFormatVersion {
        /// Required version.
        expected: u8,
        /// Supplied version.
        actual: u8,
    },
    /// A nonce is not exactly 64 lowercase hexadecimal characters.
    #[error("browser capture nonce must be 64 lowercase hexadecimal characters")]
    InvalidNonce,
    /// A digest is not exactly 64 lowercase hexadecimal characters.
    #[error("browser capture {field} must be 64 lowercase hexadecimal characters")]
    InvalidDigest {
        /// Stable rejected field name.
        field: &'static str,
    },
    /// Encoded PNG byte length falls outside the fixed bounds.
    #[error("PNG byte length {actual} is outside 1-{MAX_ENCODED_PNG_BYTES}")]
    InvalidPngByteLength {
        /// Supplied encoded length.
        actual: usize,
    },
    /// The message kind is illegal at this transition.
    #[error("expected `{expected}` message, got `{actual}`")]
    UnexpectedMessage {
        /// Required message kind.
        expected: &'static str,
        /// Supplied message kind.
        actual: &'static str,
    },
    /// An operation is illegal in the current deterministic session phase.
    #[error("expected session state `{expected}`, got `{actual}`")]
    InvalidState {
        /// Required phase.
        expected: &'static str,
        /// Actual phase.
        actual: &'static str,
    },
    /// A sequence is replayed or skips ahead of the only legal next index.
    #[error("screenshot sequence was {actual}; expected {expected}")]
    SequenceMismatch {
        /// Only legal sequence.
        expected: usize,
        /// Supplied sequence.
        actual: usize,
    },
    /// A source/sample pair does not match the frozen schedule.
    #[error(
        "screenshot {sequence} was `{actual_source}`/`{actual_sample}`; expected `{expected_source}`/`{expected_sample}`"
    )]
    ScheduleMismatch {
        /// Fixed sequence index.
        sequence: usize,
        /// Required source.
        expected_source: &'static str,
        /// Required sample.
        expected_sample: &'static str,
        /// Supplied source.
        actual_source: Box<str>,
        /// Supplied sample.
        actual_sample: Box<str>,
    },
    /// A semantic generation falls outside the exact JSON integer range.
    #[error("screenshot {sequence} {field} must be in 1-{MAX_BROWSER_CAPTURE_GENERATION}")]
    InvalidGeneration {
        /// Fixed sequence index.
        sequence: usize,
        /// Rejected generation field.
        field: &'static str,
    },
    /// A generation is not strictly newer than the prior sample for its side.
    #[error("screenshot {sequence} {capture_source:?} {field} did not increase")]
    GenerationOrder {
        /// Fixed sequence index.
        sequence: usize,
        /// Runtime side.
        capture_source: CaptureSource,
        /// Rejected generation field.
        field: &'static str,
    },
    /// An acknowledgement does not echo the exact outstanding binding.
    #[error("screenshot {sequence} acknowledgement changed {field}")]
    BindingMismatch {
        /// Outstanding fixed sequence index.
        sequence: usize,
        /// Stable differing field name.
        field: &'static str,
    },
    /// The challenge/session nonce differs.
    #[error("browser capture nonce does not match the active session")]
    NonceMismatch,
    /// A terminal document does not contain exactly eight receipts.
    #[error("terminal screenshot count was {actual}; expected {expected}")]
    ReceiptCount {
        /// Required fixed count.
        expected: usize,
        /// Supplied count.
        actual: usize,
    },
}

/// Strictly parses one bounded inbound driver message.
pub fn parse_driver_message(bytes: &[u8]) -> Result<DriverMessage, BrowserCaptureError> {
    let wire: DriverMessageWire = parse_json(bytes)?;
    wire.try_into()
}

/// Strictly parses one bounded outbound browser control message.
pub fn parse_browser_control_message(
    bytes: &[u8],
) -> Result<BrowserControlMessage, BrowserCaptureError> {
    let wire: ControlMessageWire = parse_json(bytes)?;
    wire.try_into()
}

/// Parses and binds a terminal document to an expected nonce and runtime pair.
pub fn parse_browser_capture_complete(
    bytes: &[u8],
    expected_nonce: &str,
    expected_sources: &RuntimeSources,
) -> Result<BrowserCaptureComplete, BrowserCaptureError> {
    validate_nonce(expected_nonce)?;
    let wire: CompleteWire = parse_json(bytes)?;
    validate_version(wire.format_version)?;
    if wire.nonce.as_ref() != expected_nonce {
        return Err(BrowserCaptureError::NonceMismatch);
    }
    let sources = wire.runtime_sources.try_into_validated()?;
    if &sources != expected_sources {
        return Err(BrowserCaptureError::BindingMismatch {
            sequence: 0,
            field: "runtime_sources",
        });
    }
    validate_complete(wire.nonce, sources, wire.screenshots)
}

const ZERO_DIGEST: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const GENERATION_FIELDS: [&str; 3] = [
    "frame_revision",
    "acknowledged_play_revision",
    "acknowledged_seek_revision",
];

#[derive(Deserialize)]
#[serde(tag = "message", deny_unknown_fields)]
enum DriverMessageWire {
    #[serde(rename = "challenge")]
    Challenge { format_version: u8, nonce: Box<str> },
    #[serde(rename = "screenshot_ack")]
    ScreenshotAck {
        format_version: u8,
        nonce: Box<str>,
        sequence: usize,
        source: Box<str>,
        sample: Box<str>,
        runtime_identity: RuntimeIdentityWire,
        frame_revision: u64,
        acknowledged_play_revision: u64,
        acknowledged_seek_revision: u64,
        png_byte_length: usize,
        png_sha256: Box<str>,
    },
}

#[derive(Deserialize)]
#[serde(tag = "message", deny_unknown_fields)]
enum ControlMessageWire {
    #[serde(rename = "challenge_ack")]
    ChallengeAck {
        format_version: u8,
        nonce: Box<str>,
        runtime_sources: RuntimeSourcesWire,
    },
    #[serde(rename = "screenshot_request")]
    ScreenshotRequest {
        format_version: u8,
        nonce: Box<str>,
        sequence: usize,
        source: Box<str>,
        sample: Box<str>,
        runtime_identity: RuntimeIdentityWire,
        frame_revision: u64,
        acknowledged_play_revision: u64,
        acknowledged_seek_revision: u64,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompleteWire {
    format_version: u8,
    #[serde(rename = "state")]
    _state: CompleteState,
    nonce: Box<str>,
    runtime_sources: RuntimeSourcesWire,
    screenshots: Vec<ScreenshotReceiptWire>,
}

#[derive(Deserialize)]
enum CompleteState {
    #[serde(rename = "complete")]
    Complete,
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
struct ScreenshotReceiptWire {
    sequence: usize,
    source: Box<str>,
    sample: Box<str>,
    runtime_identity: RuntimeIdentityWire,
    frame_revision: u64,
    acknowledged_play_revision: u64,
    acknowledged_seek_revision: u64,
    png_byte_length: usize,
    png_sha256: Box<str>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum ControlMessageWireRef<'a> {
    ChallengeAck(ChallengeAckWireRef<'a>),
    ScreenshotRequest(ScreenshotRequestWireRef<'a>),
}

#[derive(Serialize)]
struct ChallengeAckWireRef<'a> {
    format_version: u8,
    message: &'static str,
    nonce: &'a str,
    runtime_sources: &'a RuntimeSources,
}

#[derive(Serialize)]
struct ScreenshotRequestWireRef<'a> {
    format_version: u8,
    message: &'static str,
    nonce: &'a str,
    sequence: u8,
    source: CaptureSource,
    #[serde(serialize_with = "serialize_sample")]
    sample: Sample,
    runtime_identity: &'a RuntimeIdentity,
    frame_revision: u64,
    acknowledged_play_revision: u64,
    acknowledged_seek_revision: u64,
}

impl<'a> From<&'a BrowserControlMessage> for ControlMessageWireRef<'a> {
    fn from(value: &'a BrowserControlMessage) -> Self {
        match value {
            BrowserControlMessage::ChallengeAck {
                nonce,
                runtime_sources,
            } => Self::ChallengeAck(ChallengeAckWireRef {
                format_version: BROWSER_CAPTURE_FORMAT_VERSION,
                message: "challenge_ack",
                nonce,
                runtime_sources,
            }),
            BrowserControlMessage::ScreenshotRequest {
                nonce,
                sequence,
                receipt_binding,
            } => Self::ScreenshotRequest(ScreenshotRequestWireRef {
                format_version: BROWSER_CAPTURE_FORMAT_VERSION,
                message: "screenshot_request",
                nonce,
                sequence: *sequence,
                source: receipt_binding.source,
                sample: receipt_binding.sample,
                runtime_identity: &receipt_binding.runtime_identity,
                frame_revision: receipt_binding.frame_revision,
                acknowledged_play_revision: receipt_binding.acknowledged_play_revision,
                acknowledged_seek_revision: receipt_binding.acknowledged_seek_revision,
            }),
        }
    }
}

#[derive(Serialize)]
struct CompleteWireRef<'a> {
    format_version: u8,
    state: &'static str,
    nonce: &'a str,
    runtime_sources: &'a RuntimeSources,
    screenshots: &'a [ScreenshotReceipt],
}

impl TryFrom<DriverMessageWire> for DriverMessage {
    type Error = BrowserCaptureError;

    fn try_from(value: DriverMessageWire) -> Result<Self, Self::Error> {
        match value {
            DriverMessageWire::Challenge {
                format_version,
                nonce,
            } => {
                validate_version(format_version)?;
                validate_nonce(&nonce)?;
                Ok(Self::Challenge { nonce })
            }
            DriverMessageWire::ScreenshotAck {
                format_version,
                nonce,
                sequence,
                source,
                sample,
                runtime_identity,
                frame_revision,
                acknowledged_play_revision,
                acknowledged_seek_revision,
                png_byte_length,
                png_sha256,
            } => {
                validate_version(format_version)?;
                validate_nonce(&nonce)?;
                Ok(Self::ScreenshotAck {
                    nonce,
                    receipt: receipt_from_wire(ScreenshotReceiptWire {
                        sequence,
                        source,
                        sample,
                        runtime_identity,
                        frame_revision,
                        acknowledged_play_revision,
                        acknowledged_seek_revision,
                        png_byte_length,
                        png_sha256,
                    })?,
                })
            }
        }
    }
}

impl TryFrom<ControlMessageWire> for BrowserControlMessage {
    type Error = BrowserCaptureError;

    fn try_from(value: ControlMessageWire) -> Result<Self, Self::Error> {
        match value {
            ControlMessageWire::ChallengeAck {
                format_version,
                nonce,
                runtime_sources,
            } => {
                validate_version(format_version)?;
                validate_nonce(&nonce)?;
                Ok(Self::ChallengeAck {
                    nonce,
                    runtime_sources: runtime_sources.try_into_validated()?,
                })
            }
            ControlMessageWire::ScreenshotRequest {
                format_version,
                nonce,
                sequence,
                source,
                sample,
                runtime_identity,
                frame_revision,
                acknowledged_play_revision,
                acknowledged_seek_revision,
            } => {
                validate_version(format_version)?;
                validate_nonce(&nonce)?;
                let mut receipt = receipt_from_wire(ScreenshotReceiptWire {
                    sequence,
                    source,
                    sample,
                    runtime_identity,
                    frame_revision,
                    acknowledged_play_revision,
                    acknowledged_seek_revision,
                    png_byte_length: 1,
                    png_sha256: ZERO_DIGEST.into(),
                })?;
                receipt.png_byte_length = 1;
                receipt.png_sha256 = ZERO_DIGEST.into();
                Ok(Self::ScreenshotRequest {
                    nonce,
                    sequence: receipt.sequence,
                    receipt_binding: receipt,
                })
            }
        }
    }
}

impl RuntimeSourcesWire {
    fn try_into_validated(self) -> Result<RuntimeSources, BrowserCaptureError> {
        Ok(RuntimeSources::new(
            self.current.try_into_validated()?,
            self.proposed.try_into_validated()?,
        ))
    }
}

impl RuntimeIdentityWire {
    fn try_into_validated(self) -> Result<RuntimeIdentity, BrowserCaptureError> {
        RuntimeIdentity::new(self.manifest_sha256, self.content_sha256)
    }
}

fn receipt_from_wire(
    wire: ScreenshotReceiptWire,
) -> Result<ScreenshotReceipt, BrowserCaptureError> {
    let source =
        parse_source(&wire.source).ok_or_else(|| BrowserCaptureError::ScheduleMismatch {
            sequence: wire.sequence,
            expected_source: expected_source(wire.sequence).id(),
            expected_sample: expected_sample(wire.sequence).id(),
            actual_source: wire.source.clone(),
            actual_sample: wire.sample.clone(),
        })?;
    let sample =
        parse_sample(&wire.sample).ok_or_else(|| BrowserCaptureError::ScheduleMismatch {
            sequence: wire.sequence,
            expected_source: expected_source(wire.sequence).id(),
            expected_sample: expected_sample(wire.sequence).id(),
            actual_source: wire.source.clone(),
            actual_sample: wire.sample.clone(),
        })?;
    validate_schedule(wire.sequence, source, sample)?;
    let presentation = ScreenshotPresentation {
        source,
        sample,
        frame_revision: wire.frame_revision,
        acknowledged_play_revision: wire.acknowledged_play_revision,
        acknowledged_seek_revision: wire.acknowledged_seek_revision,
    };
    validate_generations(wire.sequence, &presentation)?;
    validate_png_length(wire.png_byte_length)?;
    validate_digest("png_sha256", &wire.png_sha256)?;
    Ok(ScreenshotReceipt {
        sequence: u8::try_from(wire.sequence).map_err(|_| {
            BrowserCaptureError::SequenceMismatch {
                expected: wire
                    .sequence
                    .min(SCREENSHOT_SEQUENCE_COUNT.saturating_sub(1)),
                actual: wire.sequence,
            }
        })?,
        source,
        sample,
        runtime_identity: wire.runtime_identity.try_into_validated()?,
        frame_revision: wire.frame_revision,
        acknowledged_play_revision: wire.acknowledged_play_revision,
        acknowledged_seek_revision: wire.acknowledged_seek_revision,
        png_byte_length: wire.png_byte_length,
        png_sha256: wire.png_sha256,
    })
}

fn validate_complete(
    nonce: Box<str>,
    runtime_sources: RuntimeSources,
    wires: Vec<ScreenshotReceiptWire>,
) -> Result<BrowserCaptureComplete, BrowserCaptureError> {
    if wires.len() != SCREENSHOT_SEQUENCE_COUNT {
        return Err(BrowserCaptureError::ReceiptCount {
            expected: SCREENSHOT_SEQUENCE_COUNT,
            actual: wires.len(),
        });
    }
    let mut screenshots = Vec::with_capacity(SCREENSHOT_SEQUENCE_COUNT);
    let mut prior = [[0_u64; 3]; 2];
    for (sequence, wire) in wires.into_iter().enumerate() {
        if wire.sequence != sequence {
            return Err(BrowserCaptureError::SequenceMismatch {
                expected: sequence,
                actual: wire.sequence,
            });
        }
        let receipt = receipt_from_wire(wire)?;
        if &receipt.runtime_identity != runtime_sources.get(receipt.source) {
            return Err(BrowserCaptureError::BindingMismatch {
                sequence,
                field: "runtime_identity",
            });
        }
        let current = [
            receipt.frame_revision,
            receipt.acknowledged_play_revision,
            receipt.acknowledged_seek_revision,
        ];
        for (field, (actual, previous)) in GENERATION_FIELDS
            .into_iter()
            .zip(current.into_iter().zip(prior[receipt.source.index()]))
        {
            if actual <= previous {
                return Err(BrowserCaptureError::GenerationOrder {
                    sequence,
                    capture_source: receipt.source,
                    field,
                });
            }
        }
        prior[receipt.source.index()] = current;
        screenshots.push(receipt);
    }
    Ok(BrowserCaptureComplete {
        nonce,
        runtime_sources,
        screenshots: screenshots.into_boxed_slice(),
    })
}

fn compare_receipt_binding(
    expected: &ScreenshotReceipt,
    actual: &ScreenshotReceipt,
) -> Result<(), BrowserCaptureError> {
    let sequence = usize::from(expected.sequence);
    for (field, matches) in [
        ("sequence", expected.sequence == actual.sequence),
        ("source", expected.source == actual.source),
        ("sample", expected.sample == actual.sample),
        (
            "runtime_identity",
            expected.runtime_identity == actual.runtime_identity,
        ),
        (
            "frame_revision",
            expected.frame_revision == actual.frame_revision,
        ),
        (
            "acknowledged_play_revision",
            expected.acknowledged_play_revision == actual.acknowledged_play_revision,
        ),
        (
            "acknowledged_seek_revision",
            expected.acknowledged_seek_revision == actual.acknowledged_seek_revision,
        ),
    ] {
        if !matches {
            return Err(BrowserCaptureError::BindingMismatch { sequence, field });
        }
    }
    Ok(())
}

fn validate_schedule(
    sequence: usize,
    source: CaptureSource,
    sample: Sample,
) -> Result<(), BrowserCaptureError> {
    if sequence >= SCREENSHOT_SEQUENCE_COUNT {
        return Err(BrowserCaptureError::SequenceMismatch {
            expected: SCREENSHOT_SEQUENCE_COUNT - 1,
            actual: sequence,
        });
    }
    let expected_source = expected_source(sequence);
    let expected_sample = expected_sample(sequence);
    if source != expected_source || sample != expected_sample {
        return Err(BrowserCaptureError::ScheduleMismatch {
            sequence,
            expected_source: expected_source.id(),
            expected_sample: expected_sample.id(),
            actual_source: source.id().into(),
            actual_sample: sample.id().into(),
        });
    }
    Ok(())
}

const fn expected_source(sequence: usize) -> CaptureSource {
    if sequence.is_multiple_of(2) {
        CaptureSource::Current
    } else {
        CaptureSource::Proposed
    }
}

fn expected_sample(sequence: usize) -> Sample {
    SAMPLE_SCHEDULE
        .get(sequence / 2)
        .copied()
        .unwrap_or(SAMPLE_SCHEDULE[SAMPLE_COUNT - 1])
}

fn validate_generations(
    sequence: usize,
    presentation: &ScreenshotPresentation,
) -> Result<(), BrowserCaptureError> {
    for (field, value) in [
        ("frame_revision", presentation.frame_revision),
        (
            "acknowledged_play_revision",
            presentation.acknowledged_play_revision,
        ),
        (
            "acknowledged_seek_revision",
            presentation.acknowledged_seek_revision,
        ),
    ] {
        if !(1..=MAX_BROWSER_CAPTURE_GENERATION).contains(&value) {
            return Err(BrowserCaptureError::InvalidGeneration { sequence, field });
        }
    }
    Ok(())
}

fn validate_control_message(value: &BrowserControlMessage) -> Result<(), BrowserCaptureError> {
    match value {
        BrowserControlMessage::ChallengeAck { nonce, .. } => validate_nonce(nonce),
        BrowserControlMessage::ScreenshotRequest {
            nonce,
            sequence,
            receipt_binding,
        } => {
            validate_nonce(nonce)?;
            let actual = usize::from(receipt_binding.sequence);
            if usize::from(*sequence) != actual {
                return Err(BrowserCaptureError::SequenceMismatch {
                    expected: usize::from(*sequence),
                    actual,
                });
            }
            validate_schedule(actual, receipt_binding.source, receipt_binding.sample)?;
            validate_generations(
                actual,
                &ScreenshotPresentation {
                    source: receipt_binding.source,
                    sample: receipt_binding.sample,
                    frame_revision: receipt_binding.frame_revision,
                    acknowledged_play_revision: receipt_binding.acknowledged_play_revision,
                    acknowledged_seek_revision: receipt_binding.acknowledged_seek_revision,
                },
            )
        }
    }
}

fn validate_nonce(value: &str) -> Result<(), BrowserCaptureError> {
    if is_lower_hex_256(value) {
        Ok(())
    } else {
        Err(BrowserCaptureError::InvalidNonce)
    }
}

fn validate_digest(field: &'static str, value: &str) -> Result<(), BrowserCaptureError> {
    if is_lower_hex_256(value) {
        Ok(())
    } else {
        Err(BrowserCaptureError::InvalidDigest { field })
    }
}

fn validate_png_length(value: usize) -> Result<(), BrowserCaptureError> {
    if (1..=MAX_ENCODED_PNG_BYTES).contains(&value) {
        Ok(())
    } else {
        Err(BrowserCaptureError::InvalidPngByteLength { actual: value })
    }
}

fn is_lower_hex_256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_version(actual: u8) -> Result<(), BrowserCaptureError> {
    if actual == BROWSER_CAPTURE_FORMAT_VERSION {
        Ok(())
    } else {
        Err(BrowserCaptureError::WrongFormatVersion {
            expected: BROWSER_CAPTURE_FORMAT_VERSION,
            actual,
        })
    }
}

fn parse_source(value: &str) -> Option<CaptureSource> {
    match value {
        "current" => Some(CaptureSource::Current),
        "proposed" => Some(CaptureSource::Proposed),
        _ => None,
    }
}

fn parse_sample(value: &str) -> Option<Sample> {
    SAMPLE_SCHEDULE
        .into_iter()
        .find(|sample| sample.id() == value)
}

fn parse_json<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, BrowserCaptureError> {
    if bytes.is_empty() || bytes.len() > MAX_BROWSER_CAPTURE_JSON_BYTES {
        return Err(BrowserCaptureError::InvalidJsonLength);
    }
    serde_json::from_slice(bytes).map_err(|error| BrowserCaptureError::InvalidJson {
        message: bounded(error.to_string(), 512),
    })
}

fn encode_json(value: &impl Serialize) -> Result<Box<[u8]>, BrowserCaptureError> {
    let bytes = serde_json::to_vec(value).map_err(|error| BrowserCaptureError::InvalidJson {
        message: bounded(error.to_string(), 512),
    })?;
    if bytes.is_empty() || bytes.len() > MAX_BROWSER_CAPTURE_JSON_BYTES {
        return Err(BrowserCaptureError::InvalidJsonLength);
    }
    Ok(bytes.into_boxed_slice())
}

fn serialize_sample<S: Serializer>(sample: &Sample, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(sample.id())
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

impl fmt::Display for CaptureSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.id())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    const NONCE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OTHER_NONCE: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    const CURRENT_MANIFEST: &str =
        "1111111111111111111111111111111111111111111111111111111111111111";
    const CURRENT_CONTENT: &str =
        "2222222222222222222222222222222222222222222222222222222222222222";
    const PROPOSED_MANIFEST: &str =
        "3333333333333333333333333333333333333333333333333333333333333333";
    const PROPOSED_CONTENT: &str =
        "4444444444444444444444444444444444444444444444444444444444444444";
    const PNG_DIGEST: &str = "5555555555555555555555555555555555555555555555555555555555555555";

    fn sources() -> RuntimeSources {
        RuntimeSources::new(
            RuntimeIdentity::new(CURRENT_MANIFEST, CURRENT_CONTENT).unwrap(),
            RuntimeIdentity::new(PROPOSED_MANIFEST, PROPOSED_CONTENT).unwrap(),
        )
    }

    fn challenge() -> DriverMessage {
        parse_driver_message(
            format!("{{\"format_version\":1,\"message\":\"challenge\",\"nonce\":\"{NONCE}\"}}")
                .as_bytes(),
        )
        .unwrap()
    }

    fn started_session() -> BrowserCaptureSession {
        let mut session = BrowserCaptureSession::new(sources());
        let control = session.accept_challenge(challenge()).unwrap();
        let expected = format!(
            "{{\"format_version\":1,\"message\":\"challenge_ack\",\"nonce\":\"{NONCE}\",\"runtime_sources\":{{\"current\":{{\"manifest_sha256\":\"{CURRENT_MANIFEST}\",\"content_sha256\":\"{CURRENT_CONTENT}\"}},\"proposed\":{{\"manifest_sha256\":\"{PROPOSED_MANIFEST}\",\"content_sha256\":\"{PROPOSED_CONTENT}\"}}}}}}"
        );
        assert_eq!(control.to_json().unwrap().as_ref(), expected.as_bytes());
        assert_eq!(
            parse_browser_control_message(&control.to_json().unwrap()).unwrap(),
            control
        );
        session
    }

    fn presentation(sequence: usize) -> ScreenshotPresentation {
        let generation = u64::try_from(sequence / 2 + 1).unwrap();
        ScreenshotPresentation::new(
            expected_source(sequence),
            expected_sample(sequence),
            generation,
            generation + 10,
            generation + 20,
        )
        .unwrap()
    }

    fn ack_value(request: &BrowserControlMessage) -> Value {
        let mut value: Value = serde_json::from_slice(&request.to_json().unwrap()).unwrap();
        value["message"] = json!("screenshot_ack");
        let object = value.as_object_mut().unwrap();
        object.insert("png_byte_length".into(), json!(1234));
        object.insert("png_sha256".into(), json!(PNG_DIGEST));
        value
    }

    fn parse_ack(value: &Value) -> DriverMessage {
        parse_driver_message(&serde_json::to_vec(value).unwrap()).unwrap()
    }

    fn request_and_ack(
        session: &mut BrowserCaptureSession,
        sequence: usize,
    ) -> BrowserCaptureProgress {
        let request = session.request_screenshot(presentation(sequence)).unwrap();
        if sequence == 0 {
            let expected = format!(
                "{{\"format_version\":1,\"message\":\"screenshot_request\",\"nonce\":\"{NONCE}\",\"sequence\":0,\"source\":\"current\",\"sample\":\"sway-start\",\"runtime_identity\":{{\"manifest_sha256\":\"{CURRENT_MANIFEST}\",\"content_sha256\":\"{CURRENT_CONTENT}\"}},\"frame_revision\":1,\"acknowledged_play_revision\":11,\"acknowledged_seek_revision\":21}}"
            );
            assert_eq!(request.to_json().unwrap().as_ref(), expected.as_bytes());
            assert_eq!(
                parse_browser_control_message(&request.to_json().unwrap()).unwrap(),
                request
            );
        }
        session
            .accept_screenshot_ack(parse_ack(&ack_value(&request)))
            .unwrap()
    }

    fn complete_capture() -> BrowserCaptureComplete {
        let mut session = started_session();
        for sequence in 0..SCREENSHOT_SEQUENCE_COUNT {
            let progress = request_and_ack(&mut session, sequence);
            if sequence + 1 == SCREENSHOT_SEQUENCE_COUNT {
                let BrowserCaptureProgress::Complete(complete) = progress else {
                    panic!("eighth receipt must complete capture");
                };
                return complete;
            }
            assert_eq!(progress, BrowserCaptureProgress::Pending);
        }
        unreachable!("fixed schedule is nonempty")
    }

    #[test]
    fn complete_eight_receipt_run_is_canonical_bound_and_gate_ineligible() {
        let complete = complete_capture();
        assert_eq!(complete.nonce(), NONCE);
        assert_eq!(complete.runtime_sources(), &sources());
        assert_eq!(complete.screenshots().len(), SCREENSHOT_SEQUENCE_COUNT);
        assert!(!complete.gate_eligible());
        assert!(!BrowserCaptureProgress::Complete(complete.clone()).gate_eligible());
        for (sequence, receipt) in complete.screenshots().iter().enumerate() {
            assert_eq!(usize::from(receipt.sequence()), sequence);
            assert_eq!(receipt.source(), expected_source(sequence));
            assert_eq!(receipt.sample(), expected_sample(sequence));
            assert_eq!(receipt.runtime_identity(), sources().get(receipt.source()));
            assert_eq!(receipt.png_byte_length(), 1234);
            assert_eq!(receipt.png_sha256(), PNG_DIGEST);
            assert!(!receipt.gate_eligible());
        }

        complete.validate_binding(NONCE, &sources()).unwrap();
        assert_eq!(
            complete.validate_binding(OTHER_NONCE, &sources()),
            Err(BrowserCaptureError::NonceMismatch)
        );
        let bytes = complete.to_json().unwrap();
        assert!(bytes.starts_with(b"{\"format_version\":1,\"state\":\"complete\""));
        let parsed = parse_browser_capture_complete(&bytes, NONCE, &sources()).unwrap();
        assert_eq!(parsed, complete);
        let embedded: BrowserCaptureComplete = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(embedded, complete);
        assert_eq!(serde_json::to_vec(&embedded).unwrap(), bytes.as_ref());
    }

    #[test]
    fn nonce_and_digest_forms_are_closed_and_revalidated_by_state() {
        for invalid in ["a", &"A".repeat(64), &"g".repeat(64)] {
            let raw = format!(
                "{{\"format_version\":1,\"message\":\"challenge\",\"nonce\":\"{invalid}\"}}"
            );
            assert_eq!(
                parse_driver_message(raw.as_bytes()),
                Err(BrowserCaptureError::InvalidNonce)
            );
        }
        assert_eq!(
            RuntimeIdentity::new("A".repeat(64), CURRENT_CONTENT),
            Err(BrowserCaptureError::InvalidDigest {
                field: "manifest_sha256"
            })
        );

        let mut session = BrowserCaptureSession::new(sources());
        assert_eq!(
            session.accept_challenge(DriverMessage::Challenge {
                nonce: "A".repeat(64).into_boxed_str()
            }),
            Err(BrowserCaptureError::InvalidNonce)
        );

        let mut session = started_session();
        let request = session.request_screenshot(presentation(0)).unwrap();
        let mut ack = ack_value(&request);
        ack["png_sha256"] = json!("A".repeat(64));
        assert_eq!(
            parse_driver_message(&serde_json::to_vec(&ack).unwrap()),
            Err(BrowserCaptureError::InvalidDigest {
                field: "png_sha256"
            })
        );
    }

    #[test]
    fn json_bounds_and_closed_schemas_reject_missing_duplicate_and_unknown_fields() {
        assert_eq!(
            parse_driver_message(&[]),
            Err(BrowserCaptureError::InvalidJsonLength)
        );
        assert_eq!(
            parse_driver_message(&vec![b' '; MAX_BROWSER_CAPTURE_JSON_BYTES + 1]),
            Err(BrowserCaptureError::InvalidJsonLength)
        );
        for raw in [
            format!(
                "{{\"format_version\":1,\"message\":\"challenge\",\"nonce\":\"{NONCE}\",\"unknown\":true}}"
            ),
            "{\"format_version\":1,\"message\":\"challenge\"}".to_owned(),
            format!(
                "{{\"format_version\":1,\"message\":\"challenge\",\"nonce\":\"{NONCE}\",\"nonce\":\"{NONCE}\"}}"
            ),
        ] {
            assert!(matches!(
                parse_driver_message(raw.as_bytes()),
                Err(BrowserCaptureError::InvalidJson { .. })
            ));
        }

        let mut control: Value = serde_json::from_slice(
            &started_session()
                .request_screenshot(presentation(0))
                .unwrap()
                .to_json()
                .unwrap(),
        )
        .unwrap();
        control["unknown"] = json!(true);
        assert!(matches!(
            parse_browser_control_message(&serde_json::to_vec(&control).unwrap()),
            Err(BrowserCaptureError::InvalidJson { .. })
        ));

        let complete = complete_capture();
        let mut terminal: Value = serde_json::from_slice(&complete.to_json().unwrap()).unwrap();
        terminal["runtime_sources"]["current"]["unknown"] = json!(true);
        assert!(serde_json::from_value::<BrowserCaptureComplete>(terminal).is_err());
    }

    #[test]
    fn png_byte_bounds_are_exact() {
        let mut session = started_session();
        let request = session.request_screenshot(presentation(0)).unwrap();
        for invalid in [0, MAX_ENCODED_PNG_BYTES + 1] {
            let mut ack = ack_value(&request);
            ack["png_byte_length"] = json!(invalid);
            assert_eq!(
                parse_driver_message(&serde_json::to_vec(&ack).unwrap()),
                Err(BrowserCaptureError::InvalidPngByteLength { actual: invalid })
            );
        }
        let mut maximum = ack_value(&request);
        maximum["png_byte_length"] = json!(MAX_ENCODED_PNG_BYTES);
        assert!(parse_driver_message(&serde_json::to_vec(&maximum).unwrap()).is_ok());
    }

    #[test]
    fn request_schedule_and_generation_order_are_fixed() {
        let mut session = started_session();
        assert!(matches!(
            session.request_screenshot(
                ScreenshotPresentation::new(CaptureSource::Proposed, Sample::SwayStart, 1, 1, 1)
                    .unwrap()
            ),
            Err(BrowserCaptureError::ScheduleMismatch { .. })
        ));
        assert!(matches!(
            session.request_screenshot(
                ScreenshotPresentation::new(CaptureSource::Current, Sample::SwayMiddle, 1, 1, 1)
                    .unwrap()
            ),
            Err(BrowserCaptureError::ScheduleMismatch { .. })
        ));
        assert_eq!(
            ScreenshotPresentation::new(CaptureSource::Current, Sample::SwayStart, 0, 1, 1),
            Err(BrowserCaptureError::InvalidGeneration {
                sequence: 0,
                field: "frame_revision"
            })
        );
        assert_eq!(
            ScreenshotPresentation::new(
                CaptureSource::Current,
                Sample::SwayStart,
                MAX_BROWSER_CAPTURE_GENERATION + 1,
                1,
                1
            ),
            Err(BrowserCaptureError::InvalidGeneration {
                sequence: 0,
                field: "frame_revision"
            })
        );
        assert!(
            ScreenshotPresentation::new(
                CaptureSource::Current,
                Sample::SwayStart,
                MAX_BROWSER_CAPTURE_GENERATION,
                1,
                1
            )
            .is_ok()
        );

        assert_eq!(
            BrowserControlMessage::ChallengeAck {
                nonce: "A".repeat(64).into_boxed_str(),
                runtime_sources: sources()
            }
            .to_json(),
            Err(BrowserCaptureError::InvalidNonce)
        );

        request_and_ack(&mut session, 0);
        request_and_ack(&mut session, 1);
        assert_eq!(
            session.request_screenshot(
                ScreenshotPresentation::new(CaptureSource::Current, Sample::SwayMiddle, 1, 12, 22)
                    .unwrap()
            ),
            Err(BrowserCaptureError::GenerationOrder {
                sequence: 2,
                capture_source: CaptureSource::Current,
                field: "frame_revision"
            })
        );
    }

    #[test]
    fn acknowledgement_replay_future_nonce_identity_and_generation_changes_fail() {
        let mut session = started_session();
        let request = session.request_screenshot(presentation(0)).unwrap();
        let valid = parse_ack(&ack_value(&request));

        let mut future_value = ack_value(&request);
        future_value["sequence"] = json!(2);
        future_value["sample"] = json!(Sample::SwayMiddle.id());
        let future = parse_ack(&future_value);
        assert!(matches!(
            session.accept_screenshot_ack(future),
            Err(BrowserCaptureError::BindingMismatch {
                field: "sequence",
                ..
            })
        ));

        let mut wrong_nonce_value = ack_value(&request);
        wrong_nonce_value["nonce"] = json!(OTHER_NONCE);
        assert_eq!(
            session.accept_screenshot_ack(parse_ack(&wrong_nonce_value)),
            Err(BrowserCaptureError::NonceMismatch)
        );

        let mut wrong_identity_value = ack_value(&request);
        wrong_identity_value["runtime_identity"]["content_sha256"] = json!(PROPOSED_CONTENT);
        assert!(matches!(
            session.accept_screenshot_ack(parse_ack(&wrong_identity_value)),
            Err(BrowserCaptureError::BindingMismatch {
                field: "runtime_identity",
                ..
            })
        ));

        let mut wrong_generation_value = ack_value(&request);
        wrong_generation_value["frame_revision"] = json!(2);
        assert!(matches!(
            session.accept_screenshot_ack(parse_ack(&wrong_generation_value)),
            Err(BrowserCaptureError::BindingMismatch {
                field: "frame_revision",
                ..
            })
        ));

        assert_eq!(
            session.accept_screenshot_ack(valid.clone()).unwrap(),
            BrowserCaptureProgress::Pending
        );
        assert_eq!(
            session.accept_screenshot_ack(valid),
            Err(BrowserCaptureError::InvalidState {
                expected: "awaiting_receipt",
                actual: "ready"
            })
        );
    }

    #[test]
    fn terminal_parser_rejects_count_order_identity_generation_and_binding_changes() {
        let complete = complete_capture();
        let base: Value = serde_json::from_slice(&complete.to_json().unwrap()).unwrap();

        let mut wrong_count = base.clone();
        wrong_count["screenshots"].as_array_mut().unwrap().pop();
        assert_eq!(
            parse_browser_capture_complete(
                &serde_json::to_vec(&wrong_count).unwrap(),
                NONCE,
                &sources()
            ),
            Err(BrowserCaptureError::ReceiptCount {
                expected: 8,
                actual: 7
            })
        );

        let mut replay = base.clone();
        replay["screenshots"][1]["sequence"] = json!(0);
        assert_eq!(
            parse_browser_capture_complete(
                &serde_json::to_vec(&replay).unwrap(),
                NONCE,
                &sources()
            ),
            Err(BrowserCaptureError::SequenceMismatch {
                expected: 1,
                actual: 0
            })
        );

        let mut identity = base.clone();
        identity["screenshots"][0]["runtime_identity"]["content_sha256"] = json!(PROPOSED_CONTENT);
        assert!(matches!(
            parse_browser_capture_complete(
                &serde_json::to_vec(&identity).unwrap(),
                NONCE,
                &sources()
            ),
            Err(BrowserCaptureError::BindingMismatch {
                field: "runtime_identity",
                ..
            })
        ));

        let mut generation = base.clone();
        generation["screenshots"][2]["frame_revision"] = json!(1);
        assert!(matches!(
            parse_browser_capture_complete(
                &serde_json::to_vec(&generation).unwrap(),
                NONCE,
                &sources()
            ),
            Err(BrowserCaptureError::GenerationOrder { sequence: 2, .. })
        ));

        let mut outer_sources = sources();
        outer_sources.proposed = RuntimeIdentity::new(PROPOSED_MANIFEST, CURRENT_CONTENT).unwrap();
        assert!(matches!(
            parse_browser_capture_complete(&complete.to_json().unwrap(), NONCE, &outer_sources),
            Err(BrowserCaptureError::BindingMismatch {
                field: "runtime_sources",
                ..
            })
        ));
    }

    #[test]
    fn duplicate_terminal_key_is_rejected_by_direct_deserialization() {
        let complete = complete_capture();
        let raw = String::from_utf8(complete.to_json().unwrap().into_vec()).unwrap();
        let duplicated = raw.replacen(
            "\"nonce\":",
            &format!("\"nonce\":\"{NONCE}\",\"nonce\":"),
            1,
        );
        assert!(serde_json::from_str::<BrowserCaptureComplete>(&duplicated).is_err());

        let mut missing: Value = serde_json::from_str(&raw).unwrap();
        let screenshots = missing["screenshots"].as_array_mut().unwrap();
        let first = screenshots[0].as_object_mut().unwrap();
        first.remove("sample");
        assert!(serde_json::from_value::<BrowserCaptureComplete>(missing).is_err());
    }
}

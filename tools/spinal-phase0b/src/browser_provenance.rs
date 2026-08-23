//! Strict parsing of the final generic Phase 0B browser-provenance receipt.
//!
//! The final receipt binds self-reported build, browser, and graphics context
//! to an independently known browser-capture transaction. Acceptance proves
//! only closed-schema, canonical-byte, and identity binding conformance. It
//! does not rehash the declared files, authenticate the browser or driver, or
//! make the receipt representative gate evidence.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    browser_capture::{
        BrowserCaptureComplete, RuntimeIdentity, RuntimeSources, SCREENSHOT_SEQUENCE_COUNT,
    },
    browser_observation::MAX_BROWSER_OBSERVATION_BYTES,
};

/// Browser-provenance receipt schema accepted by this parser.
pub const BROWSER_PROVENANCE_FORMAT_VERSION: u8 = 1;
/// Exact v1 artifact kind.
pub const BROWSER_PROVENANCE_ARTIFACT_KIND: &str = "phase0b_browser_provenance_receipt";
/// Exact v1 evidence class.
pub const BROWSER_PROVENANCE_EVIDENCE_CLASS: &str = "non_representative_rehearsal";
/// Exact v1 relationship declaration.
pub const BROWSER_PROVENANCE_RELATIONSHIP: &str = "self_reported_context_not_binary_attestation";
/// Gate eligibility of every accepted receipt.
pub const BROWSER_PROVENANCE_GATE_ELIGIBLE: bool = false;
/// Maximum complete canonical receipt size.
pub const MAX_BROWSER_PROVENANCE_BYTES: usize = 64 * 1024;
/// Maximum complete canonical browser-capture manifest size.
pub const MAX_BROWSER_CAPTURE_MANIFEST_BYTES: usize = 64 * 1024;
/// Maximum UTF-8 byte length of one ordinary free-text field.
pub const MAX_BROWSER_PROVENANCE_TEXT_BYTES: usize = 1024;
/// Maximum UTF-8 byte length of one portable relative path.
pub const MAX_BROWSER_PROVENANCE_PATH_BYTES: usize = 240;
/// Maximum number of declared served files.
pub const MAX_SERVED_FILE_COUNT: usize = 256;
/// Maximum number of CDP system devices.
pub const MAX_SYSTEM_DEVICE_COUNT: usize = 8;
/// Maximum number of feature-status entries.
pub const MAX_FEATURE_STATUS_COUNT: usize = 128;
/// Maximum number of driver bug workarounds.
pub const MAX_DRIVER_BUG_WORKAROUND_COUNT: usize = 128;
/// Maximum declared `Cargo.lock` size.
pub const MAX_CARGO_LOCK_BYTES: u64 = 4 * 1024 * 1024;
/// Maximum declared `Trunk.toml` size.
pub const MAX_TRUNK_CONFIG_BYTES: u64 = 64 * 1024;
/// Maximum declared CDP driver source size.
pub const MAX_DRIVER_BYTES: u64 = 4 * 1024 * 1024;
/// Maximum individual and aggregate served-file size.
pub const MAX_SERVED_FILE_BYTES: u64 = 128 * 1024 * 1024;

/// Exact browser-capture manifest file bound by v1.
pub const BROWSER_CAPTURE_MANIFEST_FILE: &str = "phase0b-browser-capture-manifest.json";
/// Exact browser terminal file bound by v1.
pub const BROWSER_TERMINAL_FILE: &str = "phase0b-browser-terminal.json";

const TARGET: &str = "wasm32-unknown-unknown";
const FEATURE: &str = "phase0b-rehearsal";
const BEVY_VERSION: &str = "0.19.0";
const HEADLESS: &str = "new";
const GL: &str = "angle";
const ANGLE_BACKEND: &str = "swiftshader";
const GRAPHICS_API: &str = "webgl2";
const WIDTH_PX: u32 = 640;
const HEIGHT_PX: u32 = 480;
const DEVICE_SCALE_FACTOR: u32 = 1;
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const BROWSER_CAPTURE_ARTIFACT_KIND: &str = "phase0b_browser_capture";
const SCREENSHOT_FILES: [&str; SCREENSHOT_SEQUENCE_COUNT] = [
    "00-sway-start-current.png",
    "01-sway-start-proposed.png",
    "02-sway-middle-current.png",
    "03-sway-middle-proposed.png",
    "04-sway-alternate-skin-current.png",
    "05-sway-alternate-skin-proposed.png",
    "06-sway-end-current.png",
    "07-sway-end-proposed.png",
];

/// Exact byte identity of an already-known capture artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserProvenanceArtifact {
    file: Box<str>,
    byte_length: u64,
    sha256: Box<str>,
}

impl BrowserProvenanceArtifact {
    /// Constructs an expected capture-artifact descriptor.
    pub fn new(
        file: impl Into<Box<str>>,
        byte_length: u64,
        sha256: impl Into<Box<str>>,
    ) -> Result<Self, BrowserProvenanceError> {
        let value = Self {
            file: file.into(),
            byte_length,
            sha256: sha256.into(),
        };
        if !valid_portable_path(&value.file) {
            return Err(BrowserProvenanceError::InvalidPath {
                field: "expected_artifact.file",
            });
        }
        validate_file_length(
            "expected_artifact.byte_length",
            value.byte_length,
            1,
            MAX_BROWSER_OBSERVATION_BYTES as u64,
        )?;
        validate_digest("expected_artifact.sha256", &value.sha256)?;
        Ok(value)
    }

    /// Returns the portable file name.
    #[must_use]
    pub fn file(&self) -> &str {
        &self.file
    }

    /// Returns the exact byte length.
    #[must_use]
    pub const fn byte_length(&self) -> u64 {
        self.byte_length
    }

    /// Returns the lowercase SHA-256.
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

/// One validated PNG descriptor from the canonical capture manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureManifestScreenshot {
    sequence: u8,
    file: Box<str>,
    byte_length: usize,
    sha256: Box<str>,
}

impl CaptureManifestScreenshot {
    /// Returns the fixed zero-based sequence.
    #[must_use]
    pub const fn sequence(&self) -> u8 {
        self.sequence
    }

    /// Returns the exact fixed PNG file name.
    #[must_use]
    pub fn file(&self) -> &str {
        &self.file
    }

    /// Returns the exact PNG byte length.
    #[must_use]
    pub const fn byte_length(&self) -> usize {
        self.byte_length
    }

    /// Returns the exact lowercase PNG SHA-256.
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

/// Canonical capture-manifest token bound to one validated browser capture.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedBrowserCaptureManifest {
    descriptor: BrowserProvenanceArtifact,
    terminal: BrowserProvenanceArtifact,
    screenshots: Box<[CaptureManifestScreenshot]>,
}

impl ValidatedBrowserCaptureManifest {
    /// Returns the measured identity of the exact canonical manifest bytes.
    #[must_use]
    pub const fn descriptor(&self) -> &BrowserProvenanceArtifact {
        &self.descriptor
    }

    /// Returns the independently measured terminal descriptor bound by the manifest.
    #[must_use]
    pub const fn terminal(&self) -> &BrowserProvenanceArtifact {
        &self.terminal
    }

    /// Returns all eight fixed PNG descriptors in sequence order.
    #[must_use]
    pub fn screenshots(&self) -> &[CaptureManifestScreenshot] {
        &self.screenshots
    }

    /// Returns `false`; a generic capture manifest is never gate evidence.
    #[must_use]
    pub const fn gate_eligible(&self) -> bool {
        BROWSER_PROVENANCE_GATE_ELIGIBLE
    }
}

/// One canonical, cross-bound, permanently gate-ineligible receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserProvenanceReceipt {
    binding: ProvenanceBinding,
    build: BuildProvenance,
    browser: BrowserProvenance,
    graphics: GraphicsProvenance,
}

impl BrowserProvenanceReceipt {
    /// Returns the independently verified capture bindings.
    #[must_use]
    pub const fn binding(&self) -> &ProvenanceBinding {
        &self.binding
    }

    /// Returns the self-reported build context.
    #[must_use]
    pub const fn build(&self) -> &BuildProvenance {
        &self.build
    }

    /// Returns the browser observation and requested launch profile.
    #[must_use]
    pub const fn browser(&self) -> &BrowserProvenance {
        &self.browser
    }

    /// Returns the SystemInfo and effective WebGL2 context.
    #[must_use]
    pub const fn graphics(&self) -> &GraphicsProvenance {
        &self.graphics
    }

    /// Returns `false`; this context receipt can never open a gate.
    #[must_use]
    pub const fn gate_eligible(&self) -> bool {
        BROWSER_PROVENANCE_GATE_ELIGIBLE
    }

    /// Returns `false`; this context receipt can never open a representative gate.
    #[must_use]
    pub const fn representative_gate_eligible(&self) -> bool {
        BROWSER_PROVENANCE_GATE_ELIGIBLE
    }
}

/// Binding to the exact browser-capture transaction and its two runtimes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProvenanceBinding {
    nonce: Box<str>,
    runtime_sources: RuntimeSources,
    capture_manifest: BrowserProvenanceArtifact,
    terminal: BrowserProvenanceArtifact,
}

impl ProvenanceBinding {
    /// Returns the independently supplied capture nonce.
    #[must_use]
    pub fn nonce(&self) -> &str {
        &self.nonce
    }

    /// Returns the independently supplied runtime identities.
    #[must_use]
    pub const fn runtime_sources(&self) -> &RuntimeSources {
        &self.runtime_sources
    }

    /// Returns the exact capture-manifest descriptor.
    #[must_use]
    pub const fn capture_manifest(&self) -> &BrowserProvenanceArtifact {
        &self.capture_manifest
    }

    /// Returns the exact browser-terminal descriptor.
    #[must_use]
    pub const fn terminal(&self) -> &BrowserProvenanceArtifact {
        &self.terminal
    }
}

/// Self-reported build context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuildProvenance {
    checkout: CheckoutProvenance,
    cargo_lock: DeclaredFile,
    trunk_config: DeclaredFile,
    driver: DeclaredFile,
    driver_host: DriverHost,
    toolchain: ToolchainProvenance,
    invocation: BuildInvocation,
    served_files: Box<[ServedFile]>,
}

impl BuildProvenance {
    /// Returns the checkout identity and status declaration.
    #[must_use]
    pub const fn checkout(&self) -> &CheckoutProvenance {
        &self.checkout
    }
    /// Returns the declared `Cargo.lock` identity.
    #[must_use]
    pub const fn cargo_lock(&self) -> &DeclaredFile {
        &self.cargo_lock
    }
    /// Returns the declared `Trunk.toml` identity.
    #[must_use]
    pub const fn trunk_config(&self) -> &DeclaredFile {
        &self.trunk_config
    }
    /// Returns the declared CDP driver source identity.
    #[must_use]
    pub const fn driver(&self) -> &DeclaredFile {
        &self.driver
    }
    /// Returns the driver-host context.
    #[must_use]
    pub const fn driver_host(&self) -> &DriverHost {
        &self.driver_host
    }
    /// Returns the declared toolchain context.
    #[must_use]
    pub const fn toolchain(&self) -> &ToolchainProvenance {
        &self.toolchain
    }
    /// Returns the exact fixed build invocation.
    #[must_use]
    pub const fn invocation(&self) -> &BuildInvocation {
        &self.invocation
    }
    /// Returns declared served files in strict path order.
    #[must_use]
    pub fn served_files(&self) -> &[ServedFile] {
        &self.served_files
    }
}

/// Checkout identity and dirty-state declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckoutProvenance {
    head: Box<str>,
    dirty: bool,
    status_sha256: Box<str>,
}

impl CheckoutProvenance {
    /// Returns the lowercase 40- or 64-hex checkout head.
    #[must_use]
    pub fn head(&self) -> &str {
        &self.head
    }
    /// Returns the producer-declared dirty state.
    #[must_use]
    pub const fn dirty(&self) -> bool {
        self.dirty
    }
    /// Returns the SHA-256 of canonical checkout-status bytes.
    #[must_use]
    pub fn status_sha256(&self) -> &str {
        &self.status_sha256
    }
}

/// Declared byte identity of one build input or tool.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclaredFile {
    byte_length: u64,
    sha256: Box<str>,
}

impl DeclaredFile {
    /// Returns the declared byte length.
    #[must_use]
    pub const fn byte_length(&self) -> u64 {
        self.byte_length
    }
    /// Returns the declared lowercase SHA-256.
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

/// Self-reported host running the CDP driver.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DriverHost {
    platform: Box<str>,
    architecture: Box<str>,
    node_version: Box<str>,
}

impl DriverHost {
    /// Returns the host platform.
    #[must_use]
    pub fn platform(&self) -> &str {
        &self.platform
    }
    /// Returns the host architecture.
    #[must_use]
    pub fn architecture(&self) -> &str {
        &self.architecture
    }
    /// Returns the Node.js version.
    #[must_use]
    pub fn node_version(&self) -> &str {
        &self.node_version
    }
}

/// Self-reported build toolchain.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolchainProvenance {
    rustc_release: Box<str>,
    rustc_commit_hash: Option<Box<str>>,
    rustc_host: Box<str>,
    cargo_version: Box<str>,
    trunk_version: Box<str>,
    bevy_version: Box<str>,
}

impl ToolchainProvenance {
    /// Returns the rustc release.
    #[must_use]
    pub fn rustc_release(&self) -> &str {
        &self.rustc_release
    }
    /// Returns the optional lowercase rustc commit hash.
    #[must_use]
    pub fn rustc_commit_hash(&self) -> Option<&str> {
        self.rustc_commit_hash.as_deref()
    }
    /// Returns the rustc host triple.
    #[must_use]
    pub fn rustc_host(&self) -> &str {
        &self.rustc_host
    }
    /// Returns the Cargo version.
    #[must_use]
    pub fn cargo_version(&self) -> &str {
        &self.cargo_version
    }
    /// Returns the Trunk version.
    #[must_use]
    pub fn trunk_version(&self) -> &str {
        &self.trunk_version
    }
    /// Returns `0.19.0`.
    #[must_use]
    pub fn bevy_version(&self) -> &str {
        &self.bevy_version
    }
}

/// Exact generic rehearsal build invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuildInvocation {
    trunk_release: bool,
    target: Box<str>,
    features: Box<[Box<str>]>,
}

impl BuildInvocation {
    /// Returns `true`; v1 accepts only a Trunk release build.
    #[must_use]
    pub const fn trunk_release(&self) -> bool {
        self.trunk_release
    }
    /// Returns `wasm32-unknown-unknown`.
    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }
    /// Returns the exact singleton `phase0b-rehearsal` list.
    #[must_use]
    pub fn features(&self) -> &[Box<str>] {
        &self.features
    }
}

/// One declared file served to the rehearsal browser.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServedFile {
    path: Box<str>,
    byte_length: u64,
    sha256: Box<str>,
}

impl ServedFile {
    /// Returns the normalized portable relative path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
    /// Returns the declared byte length.
    #[must_use]
    pub const fn byte_length(&self) -> u64 {
        self.byte_length
    }
    /// Returns the declared lowercase SHA-256.
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

/// Browser.getVersion observation plus fixed requested launch profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserProvenance {
    protocol_version: Box<str>,
    product: Box<str>,
    revision: Box<str>,
    js_version: Box<str>,
    requested_launch: RequestedLaunch,
}

impl BrowserProvenance {
    /// Returns the observed DevTools protocol version.
    #[must_use]
    pub fn protocol_version(&self) -> &str {
        &self.protocol_version
    }
    /// Returns the observed browser product.
    #[must_use]
    pub fn product(&self) -> &str {
        &self.product
    }
    /// Returns the observed browser revision.
    #[must_use]
    pub fn revision(&self) -> &str {
        &self.revision
    }
    /// Returns the observed JavaScript engine version.
    #[must_use]
    pub fn js_version(&self) -> &str {
        &self.js_version
    }
    /// Returns the exact requested launch profile.
    #[must_use]
    pub const fn requested_launch(&self) -> &RequestedLaunch {
        &self.requested_launch
    }
}

/// Exact requested Chrome launch profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestedLaunch {
    headless: Box<str>,
    gl: Box<str>,
    angle_backend: Box<str>,
    width_px: u32,
    height_px: u32,
    device_scale_factor: u32,
}

impl RequestedLaunch {
    /// Returns the requested headless mode.
    #[must_use]
    pub fn headless(&self) -> &str {
        &self.headless
    }
    /// Returns the requested GL implementation.
    #[must_use]
    pub fn gl(&self) -> &str {
        &self.gl
    }
    /// Returns the requested ANGLE backend.
    #[must_use]
    pub fn angle_backend(&self) -> &str {
        &self.angle_backend
    }
    /// Returns the requested viewport width.
    #[must_use]
    pub const fn width_px(&self) -> u32 {
        self.width_px
    }
    /// Returns the requested viewport height.
    #[must_use]
    pub const fn height_px(&self) -> u32 {
        self.height_px
    }
    /// Returns the requested device scale factor.
    #[must_use]
    pub const fn device_scale_factor(&self) -> u32 {
        self.device_scale_factor
    }
}

/// Self-reported CDP SystemInfo and effective WebGL2 context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphicsProvenance {
    system_devices: Box<[SystemDevice]>,
    feature_status: Box<[FeatureStatus]>,
    driver_bug_workarounds: Box<[Box<str>]>,
    effective_context: EffectiveGraphicsContext,
}

impl GraphicsProvenance {
    /// Returns devices in CDP order; element zero remains the primary device.
    #[must_use]
    pub fn system_devices(&self) -> &[SystemDevice] {
        &self.system_devices
    }
    /// Returns feature status in strict name order.
    #[must_use]
    pub fn feature_status(&self) -> &[FeatureStatus] {
        &self.feature_status
    }
    /// Returns sorted, unique driver bug workarounds.
    #[must_use]
    pub fn driver_bug_workarounds(&self) -> &[Box<str>] {
        &self.driver_bug_workarounds
    }
    /// Returns the effective page WebGL2 context.
    #[must_use]
    pub const fn effective_context(&self) -> &EffectiveGraphicsContext {
        &self.effective_context
    }
}

/// One CDP GPU device. Empty descriptive strings are retained explicitly.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemDevice {
    vendor_id: u32,
    device_id: u32,
    vendor_string: Box<str>,
    device_string: Box<str>,
    driver_vendor: Box<str>,
    driver_version: Box<str>,
}

impl SystemDevice {
    /// Returns the CDP vendor identifier.
    #[must_use]
    pub const fn vendor_id(&self) -> u32 {
        self.vendor_id
    }
    /// Returns the CDP device identifier.
    #[must_use]
    pub const fn device_id(&self) -> u32 {
        self.device_id
    }
    /// Returns the possibly empty vendor description.
    #[must_use]
    pub fn vendor_string(&self) -> &str {
        &self.vendor_string
    }
    /// Returns the possibly empty device description.
    #[must_use]
    pub fn device_string(&self) -> &str {
        &self.device_string
    }
    /// Returns the possibly empty driver vendor.
    #[must_use]
    pub fn driver_vendor(&self) -> &str {
        &self.driver_vendor
    }
    /// Returns the possibly empty driver version.
    #[must_use]
    pub fn driver_version(&self) -> &str {
        &self.driver_version
    }
}

/// One named CDP graphics feature status.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeatureStatus {
    name: Box<str>,
    status: Box<str>,
}

impl FeatureStatus {
    /// Returns the unique feature name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Returns the observed feature status.
    #[must_use]
    pub fn status(&self) -> &str {
        &self.status
    }
}

/// Effective WebGL2 context observed in the rehearsal page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectiveGraphicsContext {
    api: Box<str>,
    drawing_buffer_width: u32,
    drawing_buffer_height: u32,
    vendor: Box<str>,
    renderer: Box<str>,
    version: Box<str>,
    shading_language_version: Box<str>,
    unmasked_vendor: Box<str>,
    unmasked_renderer: Box<str>,
}

impl EffectiveGraphicsContext {
    /// Returns `webgl2`.
    #[must_use]
    pub fn api(&self) -> &str {
        &self.api
    }
    /// Returns the fixed drawing-buffer width.
    #[must_use]
    pub const fn drawing_buffer_width(&self) -> u32 {
        self.drawing_buffer_width
    }
    /// Returns the fixed drawing-buffer height.
    #[must_use]
    pub const fn drawing_buffer_height(&self) -> u32 {
        self.drawing_buffer_height
    }
    /// Returns the masked vendor.
    #[must_use]
    pub fn vendor(&self) -> &str {
        &self.vendor
    }
    /// Returns the masked renderer.
    #[must_use]
    pub fn renderer(&self) -> &str {
        &self.renderer
    }
    /// Returns the WebGL version.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }
    /// Returns the shading-language version.
    #[must_use]
    pub fn shading_language_version(&self) -> &str {
        &self.shading_language_version
    }
    /// Returns the unmasked vendor.
    #[must_use]
    pub fn unmasked_vendor(&self) -> &str {
        &self.unmasked_vendor
    }
    /// Returns the unmasked renderer.
    #[must_use]
    pub fn unmasked_renderer(&self) -> &str {
        &self.unmasked_renderer
    }
}

/// Failure to parse, canonicalize, or bind a browser-provenance receipt.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum BrowserProvenanceError {
    /// The receipt is empty or exceeds 64 KiB.
    #[error("browser provenance bytes must have length 1-{MAX_BROWSER_PROVENANCE_BYTES}")]
    InvalidLength,
    /// JSON or one of its closed nested schemas is invalid.
    #[error("invalid browser provenance JSON: {message}")]
    InvalidJson {
        /// Bounded serde detail.
        message: Box<str>,
    },
    /// Valid JSON is not the exact compact canonical encoding.
    #[error("browser provenance JSON is not in canonical v1 byte form")]
    NonCanonicalJson,
    /// The caller's expected nonce is malformed.
    #[error("expected browser provenance nonce must be 64 lowercase hexadecimal characters")]
    InvalidExpectedNonce,
    /// The receipt nonce is malformed.
    #[error("browser provenance nonce must be 64 lowercase hexadecimal characters")]
    InvalidNonce,
    /// The schema version is unsupported.
    #[error("unsupported browser provenance format {actual}; expected {expected}")]
    WrongFormatVersion {
        /// Only accepted schema version.
        expected: u8,
        /// Supplied schema version.
        actual: u8,
    },
    /// One fixed v1 value differs.
    #[error("browser provenance {field} must equal `{expected}`")]
    WrongConstant {
        /// Stable rejected field name.
        field: &'static str,
        /// Exact required value.
        expected: &'static str,
    },
    /// A receipt binding differs from an independently supplied value.
    #[error("browser provenance binding does not match expected {field}")]
    BindingMismatch {
        /// Stable mismatched binding name.
        field: &'static str,
    },
    /// A digest is not lowercase SHA-256.
    #[error("browser provenance {field} must be 64 lowercase hexadecimal characters")]
    InvalidDigest {
        /// Stable rejected field name.
        field: &'static str,
    },
    /// A Git hash is not 40 or 64 lowercase hexadecimal characters.
    #[error("browser provenance {field} must be 40 or 64 lowercase hexadecimal characters")]
    InvalidGitHash {
        /// Stable rejected field name.
        field: &'static str,
    },
    /// A version is not the exact bare ASCII semver shape shared with the producer.
    #[error("browser provenance {field} must be a bare ASCII semantic version")]
    InvalidVersion {
        /// Stable rejected field name.
        field: &'static str,
    },
    /// The dirty flag and canonical status digest disagree.
    #[error("browser provenance checkout dirty flag is inconsistent with status_sha256")]
    CheckoutStatusMismatch,
    /// A nonempty text field violates its fixed bound or contains controls.
    #[error(
        "browser provenance {field} must be nonempty control-free UTF-8 of at most {maximum} bytes"
    )]
    InvalidText {
        /// Stable rejected field name.
        field: &'static str,
        /// Fixed UTF-8 byte budget.
        maximum: usize,
    },
    /// A possibly empty device string violates its fixed bound or contains controls.
    #[error("browser provenance {field} must be control-free UTF-8 of at most {maximum} bytes")]
    InvalidDeviceText {
        /// Stable rejected field name.
        field: &'static str,
        /// Fixed UTF-8 byte budget.
        maximum: usize,
    },
    /// A relative path is unsafe or non-portable.
    #[error("browser provenance {field} is not a safe normalized portable relative path")]
    InvalidPath {
        /// Stable rejected field name.
        field: &'static str,
    },
    /// A declared byte length violates its fixed inclusive budget.
    #[error("browser provenance {field} byte length {actual} is outside {minimum}-{maximum}")]
    InvalidFileByteLength {
        /// Stable rejected field name.
        field: &'static str,
        /// Supplied value.
        actual: u64,
        /// Inclusive minimum.
        minimum: u64,
        /// Inclusive maximum.
        maximum: u64,
    },
    /// An array length violates its fixed inclusive budget.
    #[error("browser provenance {field} count {actual} is outside {minimum}-{maximum}")]
    InvalidArrayLength {
        /// Stable rejected field name.
        field: &'static str,
        /// Supplied count.
        actual: usize,
        /// Inclusive minimum.
        minimum: usize,
        /// Inclusive maximum.
        maximum: usize,
    },
    /// An array required to be sorted and unique is not.
    #[error("browser provenance {field}[{index}] is not after its predecessor")]
    InvalidOrder {
        /// Stable array field name.
        field: &'static str,
        /// First rejected index.
        index: usize,
    },
    /// Served file lengths sum to zero or more than 128 MiB.
    #[error(
        "browser provenance served-file aggregate {actual} is outside 1-{MAX_SERVED_FILE_BYTES}"
    )]
    InvalidServedFileAggregate {
        /// Supplied aggregate.
        actual: u64,
    },
}

/// Failure to parse or bind the canonical browser-capture manifest v1.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum BrowserCaptureManifestError {
    /// The manifest is empty or exceeds 64 KiB.
    #[error(
        "browser capture manifest bytes must have length 1-{MAX_BROWSER_CAPTURE_MANIFEST_BYTES}"
    )]
    InvalidLength,
    /// JSON or one of its closed nested schemas is invalid.
    #[error("invalid browser capture manifest JSON: {message}")]
    InvalidJson {
        /// Bounded serde detail.
        message: Box<str>,
    },
    /// Valid JSON is not the exact compact canonical encoding.
    #[error("browser capture manifest JSON is not in canonical v1 byte form")]
    NonCanonicalJson,
    /// The schema version is unsupported.
    #[error("unsupported browser capture manifest format {actual}; expected {expected}")]
    WrongFormatVersion {
        /// Only accepted schema version.
        expected: u8,
        /// Supplied schema version.
        actual: u8,
    },
    /// One fixed manifest value differs from v1.
    #[error("browser capture manifest {field} must equal `{expected}`")]
    WrongConstant {
        /// Stable rejected field name.
        field: &'static str,
        /// Exact required value.
        expected: &'static str,
    },
    /// The manifest nonce differs from its validated terminal capture.
    #[error("browser capture manifest nonce does not match the terminal capture")]
    NonceMismatch,
    /// The manifest terminal descriptor is malformed or differs from the measured terminal.
    #[error("browser capture manifest terminal descriptor does not match the measured terminal")]
    TerminalMismatch,
    /// The manifest does not declare exactly eight screenshots.
    #[error(
        "browser capture manifest screenshot count was {actual}; expected {SCREENSHOT_SEQUENCE_COUNT}"
    )]
    ScreenshotCount {
        /// Supplied screenshot count.
        actual: usize,
    },
    /// One sequence, filename, byte length, or digest differs from the validated receipt.
    #[error("browser capture manifest screenshot {index} changed {field}")]
    ScreenshotMismatch {
        /// Fixed screenshot index.
        index: usize,
        /// Stable mismatched field.
        field: &'static str,
    },
}

/// Parses canonical manifest v1 bytes and binds them to one validated capture.
///
/// The caller must independently measure `expected_terminal`. The returned
/// token computes the exact manifest descriptor for use as the provenance
/// receipt's independently expected capture-manifest binding.
pub fn parse_browser_capture_manifest(
    bytes: &[u8],
    capture: &BrowserCaptureComplete,
    expected_terminal: &BrowserProvenanceArtifact,
) -> Result<ValidatedBrowserCaptureManifest, BrowserCaptureManifestError> {
    if bytes.is_empty() || bytes.len() > MAX_BROWSER_CAPTURE_MANIFEST_BYTES {
        return Err(BrowserCaptureManifestError::InvalidLength);
    }
    let wire: CaptureManifestWire = serde_json::from_slice(bytes).map_err(|error| {
        BrowserCaptureManifestError::InvalidJson {
            message: bounded(error.to_string(), 512),
        }
    })?;
    let canonical =
        serde_json::to_vec(&wire).map_err(|error| BrowserCaptureManifestError::InvalidJson {
            message: bounded(error.to_string(), 512),
        })?;
    if canonical != bytes {
        return Err(BrowserCaptureManifestError::NonCanonicalJson);
    }
    if wire.format_version != BROWSER_PROVENANCE_FORMAT_VERSION {
        return Err(BrowserCaptureManifestError::WrongFormatVersion {
            expected: BROWSER_PROVENANCE_FORMAT_VERSION,
            actual: wire.format_version,
        });
    }
    require_manifest_constant(
        "artifact_kind",
        &wire.artifact_kind,
        BROWSER_CAPTURE_ARTIFACT_KIND,
    )?;
    require_manifest_constant(
        "evidence_class",
        &wire.evidence_class,
        BROWSER_PROVENANCE_EVIDENCE_CLASS,
    )?;
    if wire.gate_eligible {
        return Err(BrowserCaptureManifestError::WrongConstant {
            field: "gate_eligible",
            expected: "false",
        });
    }
    if !is_sha256(&wire.nonce) || wire.nonce.as_ref() != capture.nonce() {
        return Err(BrowserCaptureManifestError::NonceMismatch);
    }
    require_manifest_constant("terminal.file", &wire.terminal.file, BROWSER_TERMINAL_FILE)?;
    if wire.terminal.byte_length == 0
        || wire.terminal.byte_length > MAX_BROWSER_OBSERVATION_BYTES as u64
        || !is_sha256(&wire.terminal.sha256)
        || wire.terminal.file.as_ref() != expected_terminal.file()
        || wire.terminal.byte_length != expected_terminal.byte_length()
        || wire.terminal.sha256.as_ref() != expected_terminal.sha256()
    {
        return Err(BrowserCaptureManifestError::TerminalMismatch);
    }
    if wire.screenshots.len() != SCREENSHOT_SEQUENCE_COUNT {
        return Err(BrowserCaptureManifestError::ScreenshotCount {
            actual: wire.screenshots.len(),
        });
    }
    if capture.screenshots().len() != SCREENSHOT_SEQUENCE_COUNT {
        return Err(BrowserCaptureManifestError::ScreenshotCount {
            actual: capture.screenshots().len(),
        });
    }
    let mut screenshots = Vec::with_capacity(SCREENSHOT_SEQUENCE_COUNT);
    for (index, (wire, receipt)) in wire
        .screenshots
        .into_iter()
        .zip(capture.screenshots())
        .enumerate()
    {
        if usize::from(wire.sequence) != index || usize::from(receipt.sequence()) != index {
            return Err(BrowserCaptureManifestError::ScreenshotMismatch {
                index,
                field: "sequence",
            });
        }
        if wire.file.as_ref() != SCREENSHOT_FILES[index] {
            return Err(BrowserCaptureManifestError::ScreenshotMismatch {
                index,
                field: "file",
            });
        }
        if wire.byte_length != receipt.png_byte_length() {
            return Err(BrowserCaptureManifestError::ScreenshotMismatch {
                index,
                field: "byte_length",
            });
        }
        if !is_sha256(&wire.sha256) || wire.sha256.as_ref() != receipt.png_sha256() {
            return Err(BrowserCaptureManifestError::ScreenshotMismatch {
                index,
                field: "sha256",
            });
        }
        screenshots.push(CaptureManifestScreenshot {
            sequence: wire.sequence,
            file: wire.file,
            byte_length: wire.byte_length,
            sha256: wire.sha256,
        });
    }
    Ok(ValidatedBrowserCaptureManifest {
        descriptor: BrowserProvenanceArtifact {
            file: BROWSER_CAPTURE_MANIFEST_FILE.into(),
            byte_length: bytes.len() as u64,
            sha256: sha256_hex(bytes),
        },
        terminal: expected_terminal.clone(),
        screenshots: screenshots.into_boxed_slice(),
    })
}

/// Parses a canonical v1 receipt and cross-binds all capture transaction identities.
///
/// The expected artifacts must have been independently measured. This function
/// does not open or rehash any path declared by the receipt.
pub fn parse_browser_provenance_receipt(
    bytes: &[u8],
    expected_nonce: &str,
    expected_runtime_sources: &RuntimeSources,
    expected_capture_manifest: &BrowserProvenanceArtifact,
    expected_terminal: &BrowserProvenanceArtifact,
) -> Result<BrowserProvenanceReceipt, BrowserProvenanceError> {
    if !is_sha256(expected_nonce) {
        return Err(BrowserProvenanceError::InvalidExpectedNonce);
    }
    if bytes.is_empty() || bytes.len() > MAX_BROWSER_PROVENANCE_BYTES {
        return Err(BrowserProvenanceError::InvalidLength);
    }
    let wire: ReceiptWire =
        serde_json::from_slice(bytes).map_err(|error| BrowserProvenanceError::InvalidJson {
            message: bounded(error.to_string(), 512),
        })?;
    let canonical =
        serde_json::to_vec(&wire).map_err(|error| BrowserProvenanceError::InvalidJson {
            message: bounded(error.to_string(), 512),
        })?;
    if canonical != bytes {
        return Err(BrowserProvenanceError::NonCanonicalJson);
    }
    validate_constants(&wire)?;
    let binding = validate_binding(
        wire.binding,
        expected_nonce,
        expected_runtime_sources,
        expected_capture_manifest,
        expected_terminal,
    )?;
    Ok(BrowserProvenanceReceipt {
        binding,
        build: validate_build(wire.build)?,
        browser: validate_browser(wire.browser)?,
        graphics: validate_graphics(wire.graphics)?,
    })
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReceiptWire {
    format_version: u8,
    artifact_kind: Box<str>,
    evidence_class: Box<str>,
    gate_eligible: bool,
    relationship: Box<str>,
    binding: BindingWire,
    build: BuildWire,
    browser: BrowserWire,
    graphics: GraphicsWire,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BindingWire {
    nonce: Box<str>,
    runtime_sources: RuntimeSourcesWire,
    capture_manifest: ArtifactWire,
    terminal: ArtifactWire,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RuntimeSourcesWire {
    current: RuntimeIdentityWire,
    proposed: RuntimeIdentityWire,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RuntimeIdentityWire {
    manifest_sha256: Box<str>,
    content_sha256: Box<str>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ArtifactWire {
    file: Box<str>,
    byte_length: u64,
    sha256: Box<str>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CaptureManifestWire {
    format_version: u8,
    artifact_kind: Box<str>,
    evidence_class: Box<str>,
    gate_eligible: bool,
    nonce: Box<str>,
    terminal: ArtifactWire,
    screenshots: Vec<CaptureManifestScreenshotWire>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CaptureManifestScreenshotWire {
    sequence: u8,
    file: Box<str>,
    byte_length: usize,
    sha256: Box<str>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BuildWire {
    checkout: CheckoutWire,
    cargo_lock: DeclaredFileWire,
    trunk_config: DeclaredFileWire,
    driver: DeclaredFileWire,
    driver_host: DriverHostWire,
    toolchain: ToolchainWire,
    invocation: InvocationWire,
    served_files: Vec<ServedFileWire>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CheckoutWire {
    head: Box<str>,
    dirty: bool,
    status_sha256: Box<str>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DeclaredFileWire {
    byte_length: u64,
    sha256: Box<str>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DriverHostWire {
    platform: Box<str>,
    architecture: Box<str>,
    node_version: Box<str>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ToolchainWire {
    rustc_release: Box<str>,
    rustc_commit_hash: RequiredNullable<Box<str>>,
    rustc_host: Box<str>,
    cargo_version: Box<str>,
    trunk_version: Box<str>,
    bevy_version: Box<str>,
}

#[derive(Deserialize, Serialize)]
#[serde(transparent)]
struct RequiredNullable<T>(Option<T>);

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct InvocationWire {
    trunk_release: bool,
    target: Box<str>,
    features: Vec<Box<str>>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ServedFileWire {
    path: Box<str>,
    byte_length: u64,
    sha256: Box<str>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BrowserWire {
    protocol_version: Box<str>,
    product: Box<str>,
    revision: Box<str>,
    js_version: Box<str>,
    requested_launch: RequestedLaunchWire,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RequestedLaunchWire {
    headless: Box<str>,
    gl: Box<str>,
    angle_backend: Box<str>,
    width_px: u32,
    height_px: u32,
    device_scale_factor: u32,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GraphicsWire {
    system_devices: Vec<SystemDeviceWire>,
    feature_status: Vec<FeatureStatusWire>,
    driver_bug_workarounds: Vec<Box<str>>,
    effective_context: EffectiveContextWire,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SystemDeviceWire {
    vendor_id: u32,
    device_id: u32,
    vendor_string: Box<str>,
    device_string: Box<str>,
    driver_vendor: Box<str>,
    driver_version: Box<str>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FeatureStatusWire {
    name: Box<str>,
    status: Box<str>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EffectiveContextWire {
    api: Box<str>,
    drawing_buffer_width: u32,
    drawing_buffer_height: u32,
    vendor: Box<str>,
    renderer: Box<str>,
    version: Box<str>,
    shading_language_version: Box<str>,
    unmasked_vendor: Box<str>,
    unmasked_renderer: Box<str>,
}

fn validate_constants(wire: &ReceiptWire) -> Result<(), BrowserProvenanceError> {
    if wire.format_version != BROWSER_PROVENANCE_FORMAT_VERSION {
        return Err(BrowserProvenanceError::WrongFormatVersion {
            expected: BROWSER_PROVENANCE_FORMAT_VERSION,
            actual: wire.format_version,
        });
    }
    require_constant(
        "artifact_kind",
        &wire.artifact_kind,
        BROWSER_PROVENANCE_ARTIFACT_KIND,
    )?;
    require_constant(
        "evidence_class",
        &wire.evidence_class,
        BROWSER_PROVENANCE_EVIDENCE_CLASS,
    )?;
    if wire.gate_eligible {
        return Err(BrowserProvenanceError::WrongConstant {
            field: "gate_eligible",
            expected: "false",
        });
    }
    require_constant(
        "relationship",
        &wire.relationship,
        BROWSER_PROVENANCE_RELATIONSHIP,
    )
}

fn validate_binding(
    wire: BindingWire,
    expected_nonce: &str,
    expected_sources: &RuntimeSources,
    expected_capture_manifest: &BrowserProvenanceArtifact,
    expected_terminal: &BrowserProvenanceArtifact,
) -> Result<ProvenanceBinding, BrowserProvenanceError> {
    if !is_sha256(&wire.nonce) {
        return Err(BrowserProvenanceError::InvalidNonce);
    }
    if wire.nonce.as_ref() != expected_nonce {
        return Err(BrowserProvenanceError::BindingMismatch { field: "nonce" });
    }
    let runtime_sources = validate_runtime_sources(wire.runtime_sources, expected_sources)?;
    let capture_manifest = validate_bound_artifact(
        wire.capture_manifest,
        "binding.capture_manifest",
        BROWSER_CAPTURE_MANIFEST_FILE,
        MAX_BROWSER_PROVENANCE_BYTES as u64,
        expected_capture_manifest,
    )?;
    let terminal = validate_bound_artifact(
        wire.terminal,
        "binding.terminal",
        BROWSER_TERMINAL_FILE,
        MAX_BROWSER_OBSERVATION_BYTES as u64,
        expected_terminal,
    )?;
    Ok(ProvenanceBinding {
        nonce: wire.nonce,
        runtime_sources,
        capture_manifest,
        terminal,
    })
}

fn validate_runtime_sources(
    wire: RuntimeSourcesWire,
    expected: &RuntimeSources,
) -> Result<RuntimeSources, BrowserProvenanceError> {
    for (field, actual, required) in [
        (
            "runtime_sources.current.manifest_sha256",
            wire.current.manifest_sha256.as_ref(),
            expected.current().manifest_sha256(),
        ),
        (
            "runtime_sources.current.content_sha256",
            wire.current.content_sha256.as_ref(),
            expected.current().content_sha256(),
        ),
        (
            "runtime_sources.proposed.manifest_sha256",
            wire.proposed.manifest_sha256.as_ref(),
            expected.proposed().manifest_sha256(),
        ),
        (
            "runtime_sources.proposed.content_sha256",
            wire.proposed.content_sha256.as_ref(),
            expected.proposed().content_sha256(),
        ),
    ] {
        validate_digest(field, actual)?;
        if actual != required {
            return Err(BrowserProvenanceError::BindingMismatch { field });
        }
    }
    let current = RuntimeIdentity::new(wire.current.manifest_sha256, wire.current.content_sha256)
        .expect("validated runtime identity");
    let proposed =
        RuntimeIdentity::new(wire.proposed.manifest_sha256, wire.proposed.content_sha256)
            .expect("validated runtime identity");
    Ok(RuntimeSources::new(current, proposed))
}

fn validate_bound_artifact(
    wire: ArtifactWire,
    field: &'static str,
    required_file: &'static str,
    maximum: u64,
    expected: &BrowserProvenanceArtifact,
) -> Result<BrowserProvenanceArtifact, BrowserProvenanceError> {
    if !valid_portable_path(&wire.file) {
        return Err(BrowserProvenanceError::InvalidPath { field });
    }
    require_constant(field, &wire.file, required_file)?;
    validate_file_length(field, wire.byte_length, 1, maximum)?;
    let digest_field = match field {
        "binding.capture_manifest" => "binding.capture_manifest.sha256",
        "binding.terminal" => "binding.terminal.sha256",
        _ => "binding.artifact.sha256",
    };
    validate_digest(digest_field, &wire.sha256)?;
    let value = BrowserProvenanceArtifact {
        file: wire.file,
        byte_length: wire.byte_length,
        sha256: wire.sha256,
    };
    if &value != expected {
        return Err(BrowserProvenanceError::BindingMismatch { field });
    }
    Ok(value)
}

fn validate_build(wire: BuildWire) -> Result<BuildProvenance, BrowserProvenanceError> {
    validate_git_hash("build.checkout.head", &wire.checkout.head)?;
    validate_digest("build.checkout.status_sha256", &wire.checkout.status_sha256)?;
    if wire.checkout.dirty == (wire.checkout.status_sha256.as_ref() == EMPTY_SHA256) {
        return Err(BrowserProvenanceError::CheckoutStatusMismatch);
    }
    let checkout = CheckoutProvenance {
        head: wire.checkout.head,
        dirty: wire.checkout.dirty,
        status_sha256: wire.checkout.status_sha256,
    };
    let cargo_lock =
        validate_declared_file(wire.cargo_lock, "build.cargo_lock", MAX_CARGO_LOCK_BYTES)?;
    let trunk_config = validate_declared_file(
        wire.trunk_config,
        "build.trunk_config",
        MAX_TRUNK_CONFIG_BYTES,
    )?;
    let driver = validate_declared_file(wire.driver, "build.driver", MAX_DRIVER_BYTES)?;
    let driver_host = validate_driver_host(wire.driver_host)?;
    let toolchain = validate_toolchain(wire.toolchain)?;
    let invocation = validate_invocation(wire.invocation)?;
    let served_files = validate_served_files(wire.served_files)?;
    Ok(BuildProvenance {
        checkout,
        cargo_lock,
        trunk_config,
        driver,
        driver_host,
        toolchain,
        invocation,
        served_files,
    })
}

fn validate_declared_file(
    wire: DeclaredFileWire,
    field: &'static str,
    maximum: u64,
) -> Result<DeclaredFile, BrowserProvenanceError> {
    validate_file_length(field, wire.byte_length, 1, maximum)?;
    let digest_field = match field {
        "build.cargo_lock" => "build.cargo_lock.sha256",
        "build.trunk_config" => "build.trunk_config.sha256",
        "build.driver" => "build.driver.sha256",
        _ => "build.file.sha256",
    };
    validate_digest(digest_field, &wire.sha256)?;
    Ok(DeclaredFile {
        byte_length: wire.byte_length,
        sha256: wire.sha256,
    })
}

fn validate_driver_host(wire: DriverHostWire) -> Result<DriverHost, BrowserProvenanceError> {
    validate_text("build.driver_host.platform", &wire.platform)?;
    validate_text("build.driver_host.architecture", &wire.architecture)?;
    validate_version("build.driver_host.node_version", &wire.node_version)?;
    Ok(DriverHost {
        platform: wire.platform,
        architecture: wire.architecture,
        node_version: wire.node_version,
    })
}

fn validate_toolchain(wire: ToolchainWire) -> Result<ToolchainProvenance, BrowserProvenanceError> {
    validate_version("build.toolchain.rustc_release", &wire.rustc_release)?;
    if let Some(value) = wire.rustc_commit_hash.0.as_deref() {
        validate_git_hash("build.toolchain.rustc_commit_hash", value)?;
    }
    validate_text("build.toolchain.rustc_host", &wire.rustc_host)?;
    validate_version("build.toolchain.cargo_version", &wire.cargo_version)?;
    validate_version("build.toolchain.trunk_version", &wire.trunk_version)?;
    require_constant(
        "build.toolchain.bevy_version",
        &wire.bevy_version,
        BEVY_VERSION,
    )?;
    Ok(ToolchainProvenance {
        rustc_release: wire.rustc_release,
        rustc_commit_hash: wire.rustc_commit_hash.0,
        rustc_host: wire.rustc_host,
        cargo_version: wire.cargo_version,
        trunk_version: wire.trunk_version,
        bevy_version: wire.bevy_version,
    })
}

fn validate_invocation(wire: InvocationWire) -> Result<BuildInvocation, BrowserProvenanceError> {
    if !wire.trunk_release {
        return Err(BrowserProvenanceError::WrongConstant {
            field: "build.invocation.trunk_release",
            expected: "true",
        });
    }
    require_constant("build.invocation.target", &wire.target, TARGET)?;
    if wire.features.len() != 1 || wire.features[0].as_ref() != FEATURE {
        return Err(BrowserProvenanceError::WrongConstant {
            field: "build.invocation.features",
            expected: "[\"phase0b-rehearsal\"]",
        });
    }
    Ok(BuildInvocation {
        trunk_release: wire.trunk_release,
        target: wire.target,
        features: wire.features.into_boxed_slice(),
    })
}

fn validate_served_files(
    wires: Vec<ServedFileWire>,
) -> Result<Box<[ServedFile]>, BrowserProvenanceError> {
    validate_array_length("build.served_files", wires.len(), 1, MAX_SERVED_FILE_COUNT)?;
    let mut total = 0_u64;
    let mut files = Vec::with_capacity(wires.len());
    for (index, wire) in wires.into_iter().enumerate() {
        if !valid_portable_path(&wire.path) {
            return Err(BrowserProvenanceError::InvalidPath {
                field: "build.served_files[].path",
            });
        }
        if files
            .last()
            .is_some_and(|prior: &ServedFile| prior.path.as_ref() >= wire.path.as_ref())
        {
            return Err(BrowserProvenanceError::InvalidOrder {
                field: "build.served_files",
                index,
            });
        }
        validate_file_length(
            "build.served_files[].byte_length",
            wire.byte_length,
            0,
            MAX_SERVED_FILE_BYTES,
        )?;
        validate_digest("build.served_files[].sha256", &wire.sha256)?;
        total = total
            .checked_add(wire.byte_length)
            .ok_or(BrowserProvenanceError::InvalidServedFileAggregate { actual: u64::MAX })?;
        if total > MAX_SERVED_FILE_BYTES {
            return Err(BrowserProvenanceError::InvalidServedFileAggregate { actual: total });
        }
        files.push(ServedFile {
            path: wire.path,
            byte_length: wire.byte_length,
            sha256: wire.sha256,
        });
    }
    if total == 0 {
        return Err(BrowserProvenanceError::InvalidServedFileAggregate { actual: 0 });
    }
    Ok(files.into_boxed_slice())
}

fn validate_browser(wire: BrowserWire) -> Result<BrowserProvenance, BrowserProvenanceError> {
    validate_text("browser.protocol_version", &wire.protocol_version)?;
    validate_text("browser.product", &wire.product)?;
    validate_text("browser.revision", &wire.revision)?;
    validate_text("browser.js_version", &wire.js_version)?;
    Ok(BrowserProvenance {
        protocol_version: wire.protocol_version,
        product: wire.product,
        revision: wire.revision,
        js_version: wire.js_version,
        requested_launch: validate_requested_launch(wire.requested_launch)?,
    })
}

fn validate_requested_launch(
    wire: RequestedLaunchWire,
) -> Result<RequestedLaunch, BrowserProvenanceError> {
    require_constant(
        "browser.requested_launch.headless",
        &wire.headless,
        HEADLESS,
    )?;
    require_constant("browser.requested_launch.gl", &wire.gl, GL)?;
    require_constant(
        "browser.requested_launch.angle_backend",
        &wire.angle_backend,
        ANGLE_BACKEND,
    )?;
    require_number("browser.requested_launch.width_px", wire.width_px, WIDTH_PX)?;
    require_number(
        "browser.requested_launch.height_px",
        wire.height_px,
        HEIGHT_PX,
    )?;
    require_number(
        "browser.requested_launch.device_scale_factor",
        wire.device_scale_factor,
        DEVICE_SCALE_FACTOR,
    )?;
    Ok(RequestedLaunch {
        headless: wire.headless,
        gl: wire.gl,
        angle_backend: wire.angle_backend,
        width_px: wire.width_px,
        height_px: wire.height_px,
        device_scale_factor: wire.device_scale_factor,
    })
}

fn validate_graphics(wire: GraphicsWire) -> Result<GraphicsProvenance, BrowserProvenanceError> {
    validate_array_length(
        "graphics.system_devices",
        wire.system_devices.len(),
        1,
        MAX_SYSTEM_DEVICE_COUNT,
    )?;
    let mut devices = Vec::with_capacity(wire.system_devices.len());
    for wire in wire.system_devices {
        validate_device_text(
            "graphics.system_devices[].vendor_string",
            &wire.vendor_string,
        )?;
        validate_device_text(
            "graphics.system_devices[].device_string",
            &wire.device_string,
        )?;
        validate_device_text(
            "graphics.system_devices[].driver_vendor",
            &wire.driver_vendor,
        )?;
        validate_device_text(
            "graphics.system_devices[].driver_version",
            &wire.driver_version,
        )?;
        devices.push(SystemDevice {
            vendor_id: wire.vendor_id,
            device_id: wire.device_id,
            vendor_string: wire.vendor_string,
            device_string: wire.device_string,
            driver_vendor: wire.driver_vendor,
            driver_version: wire.driver_version,
        });
    }

    validate_array_length(
        "graphics.feature_status",
        wire.feature_status.len(),
        1,
        MAX_FEATURE_STATUS_COUNT,
    )?;
    let mut statuses = Vec::with_capacity(wire.feature_status.len());
    for (index, wire) in wire.feature_status.into_iter().enumerate() {
        validate_text("graphics.feature_status[].name", &wire.name)?;
        validate_text("graphics.feature_status[].status", &wire.status)?;
        if statuses
            .last()
            .is_some_and(|prior: &FeatureStatus| prior.name.as_ref() >= wire.name.as_ref())
        {
            return Err(BrowserProvenanceError::InvalidOrder {
                field: "graphics.feature_status",
                index,
            });
        }
        statuses.push(FeatureStatus {
            name: wire.name,
            status: wire.status,
        });
    }

    validate_array_length(
        "graphics.driver_bug_workarounds",
        wire.driver_bug_workarounds.len(),
        0,
        MAX_DRIVER_BUG_WORKAROUND_COUNT,
    )?;
    for (index, value) in wire.driver_bug_workarounds.iter().enumerate() {
        validate_text("graphics.driver_bug_workarounds[]", value)?;
        if index > 0 && wire.driver_bug_workarounds[index - 1].as_ref() >= value.as_ref() {
            return Err(BrowserProvenanceError::InvalidOrder {
                field: "graphics.driver_bug_workarounds",
                index,
            });
        }
    }
    Ok(GraphicsProvenance {
        system_devices: devices.into_boxed_slice(),
        feature_status: statuses.into_boxed_slice(),
        driver_bug_workarounds: wire.driver_bug_workarounds.into_boxed_slice(),
        effective_context: validate_effective_context(wire.effective_context)?,
    })
}

fn validate_effective_context(
    wire: EffectiveContextWire,
) -> Result<EffectiveGraphicsContext, BrowserProvenanceError> {
    require_constant("graphics.effective_context.api", &wire.api, GRAPHICS_API)?;
    require_number(
        "graphics.effective_context.drawing_buffer_width",
        wire.drawing_buffer_width,
        WIDTH_PX,
    )?;
    require_number(
        "graphics.effective_context.drawing_buffer_height",
        wire.drawing_buffer_height,
        HEIGHT_PX,
    )?;
    for (field, value) in [
        ("graphics.effective_context.vendor", wire.vendor.as_ref()),
        (
            "graphics.effective_context.renderer",
            wire.renderer.as_ref(),
        ),
        ("graphics.effective_context.version", wire.version.as_ref()),
        (
            "graphics.effective_context.shading_language_version",
            wire.shading_language_version.as_ref(),
        ),
        (
            "graphics.effective_context.unmasked_vendor",
            wire.unmasked_vendor.as_ref(),
        ),
        (
            "graphics.effective_context.unmasked_renderer",
            wire.unmasked_renderer.as_ref(),
        ),
    ] {
        validate_text(field, value)?;
    }
    Ok(EffectiveGraphicsContext {
        api: wire.api,
        drawing_buffer_width: wire.drawing_buffer_width,
        drawing_buffer_height: wire.drawing_buffer_height,
        vendor: wire.vendor,
        renderer: wire.renderer,
        version: wire.version,
        shading_language_version: wire.shading_language_version,
        unmasked_vendor: wire.unmasked_vendor,
        unmasked_renderer: wire.unmasked_renderer,
    })
}

fn validate_digest(field: &'static str, value: &str) -> Result<(), BrowserProvenanceError> {
    if is_sha256(value) {
        Ok(())
    } else {
        Err(BrowserProvenanceError::InvalidDigest { field })
    }
}

fn validate_git_hash(field: &'static str, value: &str) -> Result<(), BrowserProvenanceError> {
    if matches!(value.len(), 40 | 64) && is_lower_hex(value) {
        Ok(())
    } else {
        Err(BrowserProvenanceError::InvalidGitHash { field })
    }
}

fn validate_version(field: &'static str, value: &str) -> Result<(), BrowserProvenanceError> {
    if valid_bare_semver(value) {
        Ok(())
    } else {
        Err(BrowserProvenanceError::InvalidVersion { field })
    }
}

fn validate_text(field: &'static str, value: &str) -> Result<(), BrowserProvenanceError> {
    if !value.is_empty()
        && value.len() <= MAX_BROWSER_PROVENANCE_TEXT_BYTES
        && !value.chars().any(char::is_control)
    {
        Ok(())
    } else {
        Err(BrowserProvenanceError::InvalidText {
            field,
            maximum: MAX_BROWSER_PROVENANCE_TEXT_BYTES,
        })
    }
}

fn validate_device_text(field: &'static str, value: &str) -> Result<(), BrowserProvenanceError> {
    if value.len() <= MAX_BROWSER_PROVENANCE_TEXT_BYTES && !value.chars().any(char::is_control) {
        Ok(())
    } else {
        Err(BrowserProvenanceError::InvalidDeviceText {
            field,
            maximum: MAX_BROWSER_PROVENANCE_TEXT_BYTES,
        })
    }
}

fn validate_file_length(
    field: &'static str,
    actual: u64,
    minimum: u64,
    maximum: u64,
) -> Result<(), BrowserProvenanceError> {
    if (minimum..=maximum).contains(&actual) {
        Ok(())
    } else {
        Err(BrowserProvenanceError::InvalidFileByteLength {
            field,
            actual,
            minimum,
            maximum,
        })
    }
}

fn validate_array_length(
    field: &'static str,
    actual: usize,
    minimum: usize,
    maximum: usize,
) -> Result<(), BrowserProvenanceError> {
    if (minimum..=maximum).contains(&actual) {
        Ok(())
    } else {
        Err(BrowserProvenanceError::InvalidArrayLength {
            field,
            actual,
            minimum,
            maximum,
        })
    }
}

fn require_constant(
    field: &'static str,
    actual: &str,
    expected: &'static str,
) -> Result<(), BrowserProvenanceError> {
    if actual == expected {
        Ok(())
    } else {
        Err(BrowserProvenanceError::WrongConstant { field, expected })
    }
}

fn require_manifest_constant(
    field: &'static str,
    actual: &str,
    expected: &'static str,
) -> Result<(), BrowserCaptureManifestError> {
    if actual == expected {
        Ok(())
    } else {
        Err(BrowserCaptureManifestError::WrongConstant { field, expected })
    }
}

fn require_number(
    field: &'static str,
    actual: u32,
    expected: u32,
) -> Result<(), BrowserProvenanceError> {
    if actual == expected {
        Ok(())
    } else {
        let expected = match expected {
            WIDTH_PX => "640",
            HEIGHT_PX => "480",
            DEVICE_SCALE_FACTOR => "1",
            _ => "fixed v1 value",
        };
        Err(BrowserProvenanceError::WrongConstant { field, expected })
    }
}

fn valid_portable_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_BROWSER_PROVENANCE_PATH_BYTES
        && value.split('/').all(valid_portable_component)
}

fn valid_portable_component(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=128).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn valid_bare_semver(value: &str) -> bool {
    let (core, prerelease) = value
        .split_once('-')
        .map_or((value, None), |(core, suffix)| (core, Some(suffix)));
    let mut numbers = core.split('.');
    let core_is_valid = (0..3).all(|_| {
        numbers
            .next()
            .is_some_and(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    }) && numbers.next().is_none();
    core_is_valid
        && prerelease.is_none_or(|suffix| {
            !suffix.is_empty()
                && suffix
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
        })
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && is_lower_hex(value)
}

fn is_lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn sha256_hex(bytes: &[u8]) -> Box<str> {
    format!("{:x}", Sha256::digest(bytes)).into_boxed_str()
}

fn bounded(mut value: String, maximum: usize) -> Box<str> {
    if value.len() > maximum {
        let mut end = maximum.saturating_sub(3);
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
    use super::*;
    use crate::browser_capture::parse_browser_capture_complete;
    use sha2::{Digest, Sha256};

    const NONCE: &str = "9999999999999999999999999999999999999999999999999999999999999999";

    fn digest(character: char) -> Box<str> {
        character.to_string().repeat(64).into_boxed_str()
    }

    fn identity(manifest: char, content: char) -> RuntimeIdentityWire {
        RuntimeIdentityWire {
            manifest_sha256: digest(manifest),
            content_sha256: digest(content),
        }
    }

    fn artifact(file: &str, byte_length: u64, hash: char) -> ArtifactWire {
        ArtifactWire {
            file: file.into(),
            byte_length,
            sha256: digest(hash),
        }
    }

    fn declared_file(byte_length: u64, hash: char) -> DeclaredFileWire {
        DeclaredFileWire {
            byte_length,
            sha256: digest(hash),
        }
    }

    fn served_file(path: &str, byte_length: u64, hash: char) -> ServedFileWire {
        ServedFileWire {
            path: path.into(),
            byte_length,
            sha256: digest(hash),
        }
    }

    fn system_device(vendor_id: u32, device_id: u32, device: &str) -> SystemDeviceWire {
        SystemDeviceWire {
            vendor_id,
            device_id,
            vendor_string: "Google Inc.".into(),
            device_string: device.into(),
            driver_vendor: "Mesa".into(),
            driver_version: "1.2.3".into(),
        }
    }

    fn valid_wire() -> ReceiptWire {
        ReceiptWire {
            format_version: BROWSER_PROVENANCE_FORMAT_VERSION,
            artifact_kind: BROWSER_PROVENANCE_ARTIFACT_KIND.into(),
            evidence_class: BROWSER_PROVENANCE_EVIDENCE_CLASS.into(),
            gate_eligible: false,
            relationship: BROWSER_PROVENANCE_RELATIONSHIP.into(),
            binding: BindingWire {
                nonce: NONCE.into(),
                runtime_sources: RuntimeSourcesWire {
                    current: identity('a', 'b'),
                    proposed: identity('c', 'd'),
                },
                capture_manifest: artifact("phase0b-browser-capture-manifest.json", 900, 'e'),
                terminal: artifact("phase0b-browser-terminal.json", 12_000, 'f'),
            },
            build: BuildWire {
                checkout: CheckoutWire {
                    head: "1".repeat(40).into_boxed_str(),
                    dirty: true,
                    status_sha256: digest('2'),
                },
                cargo_lock: declared_file(1_000, '3'),
                trunk_config: declared_file(500, '4'),
                driver: declared_file(50_000, '5'),
                driver_host: DriverHostWire {
                    platform: "darwin".into(),
                    architecture: "arm64".into(),
                    node_version: "24.1.0".into(),
                },
                toolchain: ToolchainWire {
                    rustc_release: "1.95.0".into(),
                    rustc_commit_hash: RequiredNullable(Some("6".repeat(40).into_boxed_str())),
                    rustc_host: "aarch64-apple-darwin".into(),
                    cargo_version: "1.95.0".into(),
                    trunk_version: "0.21.14".into(),
                    bevy_version: BEVY_VERSION.into(),
                },
                invocation: InvocationWire {
                    trunk_release: true,
                    target: TARGET.into(),
                    features: vec![FEATURE.into()],
                },
                served_files: vec![
                    served_file("index.html", 2_000, '7'),
                    served_file("spinal-app.js", 10_000, '8'),
                ],
            },
            browser: BrowserWire {
                protocol_version: "1.3".into(),
                product: "HeadlessChrome/140.0.0.0".into(),
                revision: "@revision".into(),
                js_version: "14.0.0".into(),
                requested_launch: RequestedLaunchWire {
                    headless: HEADLESS.into(),
                    gl: GL.into(),
                    angle_backend: ANGLE_BACKEND.into(),
                    width_px: WIDTH_PX,
                    height_px: HEIGHT_PX,
                    device_scale_factor: DEVICE_SCALE_FACTOR,
                },
            },
            graphics: GraphicsWire {
                system_devices: vec![system_device(1, 2, "SwiftShader Device")],
                feature_status: vec![
                    FeatureStatusWire {
                        name: "gpu_compositing".into(),
                        status: "enabled".into(),
                    },
                    FeatureStatusWire {
                        name: "webgl".into(),
                        status: "enabled".into(),
                    },
                ],
                driver_bug_workarounds: vec![
                    "clear_uniforms_before_first_program_use".into(),
                    "disable_program_caching_for_transform_feedback".into(),
                ],
                effective_context: EffectiveContextWire {
                    api: GRAPHICS_API.into(),
                    drawing_buffer_width: WIDTH_PX,
                    drawing_buffer_height: HEIGHT_PX,
                    vendor: "WebKit".into(),
                    renderer: "WebKit WebGL".into(),
                    version: "WebGL 2.0".into(),
                    shading_language_version: "WebGL GLSL ES 3.00".into(),
                    unmasked_vendor: "Google Inc.".into(),
                    unmasked_renderer: "ANGLE SwiftShader".into(),
                },
            },
        }
    }

    fn canonical(wire: &ReceiptWire) -> Vec<u8> {
        serde_json::to_vec(wire).unwrap()
    }

    fn expected_sources() -> RuntimeSources {
        RuntimeSources::new(
            RuntimeIdentity::new(digest('a'), digest('b')).unwrap(),
            RuntimeIdentity::new(digest('c'), digest('d')).unwrap(),
        )
    }

    fn expected_capture_manifest() -> BrowserProvenanceArtifact {
        BrowserProvenanceArtifact::new("phase0b-browser-capture-manifest.json", 900, digest('e'))
            .unwrap()
    }

    fn expected_terminal() -> BrowserProvenanceArtifact {
        BrowserProvenanceArtifact::new("phase0b-browser-terminal.json", 12_000, digest('f'))
            .unwrap()
    }

    fn parse(bytes: &[u8]) -> Result<BrowserProvenanceReceipt, BrowserProvenanceError> {
        parse_browser_provenance_receipt(
            bytes,
            NONCE,
            &expected_sources(),
            &expected_capture_manifest(),
            &expected_terminal(),
        )
    }

    fn fixed_manifest_capture() -> BrowserCaptureComplete {
        let nonce = digest('1');
        let sources = RuntimeSources::new(
            RuntimeIdentity::new(digest('2'), digest('2')).unwrap(),
            RuntimeIdentity::new(digest('3'), digest('3')).unwrap(),
        );
        let samples = [
            "sway-start",
            "sway-start",
            "sway-middle",
            "sway-middle",
            "sway-alternate-skin",
            "sway-alternate-skin",
            "sway-end",
            "sway-end",
        ];
        let screenshots = (0..SCREENSHOT_SEQUENCE_COUNT)
            .map(|sequence| {
                let source = if sequence.is_multiple_of(2) {
                    "current"
                } else {
                    "proposed"
                };
                let identity_digit = if sequence.is_multiple_of(2) { '2' } else { '3' };
                let png_digit = char::from(b'0' + ((sequence + 5) % 10) as u8);
                serde_json::json!({
                    "sequence": sequence,
                    "source": source,
                    "sample": samples[sequence],
                    "runtime_identity": {
                        "manifest_sha256": digest(identity_digit),
                        "content_sha256": digest(identity_digit),
                    },
                    "frame_revision": sequence + 1,
                    "acknowledged_play_revision": sequence + 11,
                    "acknowledged_seek_revision": sequence + 21,
                    "png_byte_length": 100 + sequence,
                    "png_sha256": digest(png_digit),
                })
            })
            .collect::<Vec<_>>();
        let terminal = serde_json::json!({
            "format_version": 1,
            "state": "complete",
            "nonce": nonce,
            "runtime_sources": {
                "current": {
                    "manifest_sha256": digest('2'),
                    "content_sha256": digest('2'),
                },
                "proposed": {
                    "manifest_sha256": digest('3'),
                    "content_sha256": digest('3'),
                },
            },
            "screenshots": screenshots,
        });
        parse_browser_capture_complete(
            &serde_json::to_vec(&terminal).unwrap(),
            digest('1').as_ref(),
            &sources,
        )
        .unwrap()
    }

    fn fixed_manifest_wire(capture: &BrowserCaptureComplete) -> CaptureManifestWire {
        CaptureManifestWire {
            format_version: 1,
            artifact_kind: BROWSER_CAPTURE_ARTIFACT_KIND.into(),
            evidence_class: BROWSER_PROVENANCE_EVIDENCE_CLASS.into(),
            gate_eligible: false,
            nonce: capture.nonce().into(),
            terminal: artifact(BROWSER_TERMINAL_FILE, 17, '4'),
            screenshots: capture
                .screenshots()
                .iter()
                .enumerate()
                .map(|(index, receipt)| CaptureManifestScreenshotWire {
                    sequence: index as u8,
                    file: SCREENSHOT_FILES[index].into(),
                    byte_length: receipt.png_byte_length(),
                    sha256: receipt.png_sha256().into(),
                })
                .collect(),
        }
    }

    fn fixed_manifest_terminal() -> BrowserProvenanceArtifact {
        BrowserProvenanceArtifact::new(BROWSER_TERMINAL_FILE, 17, digest('4')).unwrap()
    }

    #[test]
    fn canonical_receipt_exposes_all_context_and_is_never_gate_eligible() {
        let receipt = parse(&canonical(&valid_wire())).unwrap();
        assert_eq!(receipt.binding().nonce(), NONCE);
        assert_eq!(
            receipt
                .binding()
                .runtime_sources()
                .current()
                .manifest_sha256(),
            digest('a').as_ref()
        );
        assert_eq!(receipt.binding().capture_manifest().byte_length(), 900);
        assert_eq!(
            receipt.binding().terminal().file(),
            "phase0b-browser-terminal.json"
        );
        assert_eq!(receipt.build().checkout().head().len(), 40);
        assert!(receipt.build().checkout().dirty());
        assert_eq!(receipt.build().cargo_lock().byte_length(), 1_000);
        assert_eq!(
            receipt.build().trunk_config().sha256(),
            digest('4').as_ref()
        );
        assert_eq!(receipt.build().driver().byte_length(), 50_000);
        assert_eq!(receipt.build().driver_host().architecture(), "arm64");
        assert_eq!(receipt.build().toolchain().rustc_release(), "1.95.0");
        assert_eq!(
            receipt.build().toolchain().rustc_commit_hash(),
            Some("6666666666666666666666666666666666666666")
        );
        assert!(receipt.build().invocation().trunk_release());
        assert_eq!(receipt.build().served_files()[1].path(), "spinal-app.js");
        assert_eq!(receipt.browser().protocol_version(), "1.3");
        assert_eq!(receipt.browser().requested_launch().width_px(), WIDTH_PX);
        assert_eq!(receipt.graphics().system_devices()[0].vendor_id(), 1);
        assert_eq!(receipt.graphics().feature_status()[1].name(), "webgl");
        assert_eq!(receipt.graphics().driver_bug_workarounds().len(), 2);
        assert_eq!(
            receipt.graphics().effective_context().unmasked_renderer(),
            "ANGLE SwiftShader"
        );
        assert!(!receipt.gate_eligible());
        assert!(!receipt.representative_gate_eligible());
    }

    #[test]
    fn receipt_length_and_exact_canonical_bytes_are_enforced() {
        assert_eq!(parse(&[]), Err(BrowserProvenanceError::InvalidLength));
        assert_eq!(
            parse(&vec![b' '; MAX_BROWSER_PROVENANCE_BYTES + 1]),
            Err(BrowserProvenanceError::InvalidLength)
        );
        let bytes = canonical(&valid_wire());
        let mut spaced = vec![b' '];
        spaced.extend_from_slice(&bytes);
        assert_eq!(
            parse(&spaced),
            Err(BrowserProvenanceError::NonCanonicalJson)
        );

        let text = String::from_utf8(bytes).unwrap();
        let prefix = format!(
            "{{\"format_version\":1,\"artifact_kind\":\"{BROWSER_PROVENANCE_ARTIFACT_KIND}\","
        );
        let swapped = format!(
            "{{\"artifact_kind\":\"{BROWSER_PROVENANCE_ARTIFACT_KIND}\",\"format_version\":1,"
        );
        assert_eq!(
            parse(text.replacen(&prefix, &swapped, 1).as_bytes()),
            Err(BrowserProvenanceError::NonCanonicalJson)
        );
    }

    #[test]
    fn closed_schema_rejects_unknown_missing_null_and_recursive_duplicates() {
        let text = String::from_utf8(canonical(&valid_wire())).unwrap();
        let unknown = text.replacen('{', "{\"unknown\":0,", 1);
        assert!(matches!(
            parse(unknown.as_bytes()),
            Err(BrowserProvenanceError::InvalidJson { .. })
        ));

        let relationship = format!("\"relationship\":\"{BROWSER_PROVENANCE_RELATIONSHIP}\",");
        let missing = text.replacen(&relationship, "", 1);
        assert!(matches!(
            parse(missing.as_bytes()),
            Err(BrowserProvenanceError::InvalidJson { .. })
        ));

        let null = text.replacen(
            "\"product\":\"HeadlessChrome/140.0.0.0\"",
            "\"product\":null",
            1,
        );
        assert!(matches!(
            parse(null.as_bytes()),
            Err(BrowserProvenanceError::InvalidJson { .. })
        ));

        let duplicate = text.replacen(
            "\"platform\":\"darwin\"",
            "\"platform\":\"darwin\",\"platform\":\"linux\"",
            1,
        );
        assert!(matches!(
            parse(duplicate.as_bytes()),
            Err(BrowserProvenanceError::InvalidJson { .. })
        ));

        let missing_nullable = text.replacen(
            "\"rustc_commit_hash\":\"6666666666666666666666666666666666666666\",",
            "",
            1,
        );
        assert!(parse(missing_nullable.as_bytes()).is_err());
        let explicit_null = text.replacen(
            "\"rustc_commit_hash\":\"6666666666666666666666666666666666666666\"",
            "\"rustc_commit_hash\":null",
            1,
        );
        assert!(parse(explicit_null.as_bytes()).is_ok());
    }

    #[test]
    fn every_top_level_and_fixed_execution_constant_is_closed() {
        let mut wire = valid_wire();
        wire.format_version = 2;
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::WrongFormatVersion { .. })
        ));
        for mutate in [
            |wire: &mut ReceiptWire| wire.artifact_kind = "wrong".into(),
            |wire: &mut ReceiptWire| wire.evidence_class = "wrong".into(),
            |wire: &mut ReceiptWire| wire.relationship = "wrong".into(),
            |wire: &mut ReceiptWire| wire.build.toolchain.bevy_version = "0.18.1".into(),
            |wire: &mut ReceiptWire| wire.build.invocation.target = "native".into(),
            |wire: &mut ReceiptWire| wire.browser.requested_launch.headless = "false".into(),
            |wire: &mut ReceiptWire| wire.browser.requested_launch.gl = "desktop".into(),
            |wire: &mut ReceiptWire| {
                wire.browser.requested_launch.angle_backend = "metal".into();
            },
            |wire: &mut ReceiptWire| wire.graphics.effective_context.api = "webgl".into(),
        ] {
            let mut wire = valid_wire();
            mutate(&mut wire);
            assert!(matches!(
                parse(&canonical(&wire)),
                Err(BrowserProvenanceError::WrongConstant { .. })
            ));
        }

        let mut wire = valid_wire();
        wire.gate_eligible = true;
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::WrongConstant {
                field: "gate_eligible",
                ..
            })
        ));
        let mut wire = valid_wire();
        wire.build.invocation.trunk_release = false;
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::WrongConstant { .. })
        ));
        let mut wire = valid_wire();
        wire.build.invocation.features.push("extra".into());
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::WrongConstant { .. })
        ));
        for dimension in [0, 641] {
            let mut wire = valid_wire();
            wire.browser.requested_launch.width_px = dimension;
            assert!(matches!(
                parse(&canonical(&wire)),
                Err(BrowserProvenanceError::WrongConstant { .. })
            ));
        }
        let mut wire = valid_wire();
        wire.graphics.effective_context.drawing_buffer_height = 479;
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::WrongConstant { .. })
        ));
    }

    #[test]
    fn nonce_runtime_and_artifact_bindings_are_independent_and_exact() {
        let bytes = canonical(&valid_wire());
        assert_eq!(
            parse_browser_provenance_receipt(
                &bytes,
                "bad",
                &expected_sources(),
                &expected_capture_manifest(),
                &expected_terminal(),
            ),
            Err(BrowserProvenanceError::InvalidExpectedNonce)
        );

        let mut wire = valid_wire();
        wire.binding.nonce = digest('A');
        assert_eq!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::InvalidNonce)
        );
        let mut wire = valid_wire();
        wire.binding.nonce = digest('0');
        assert_eq!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::BindingMismatch { field: "nonce" })
        );

        let mut wire = valid_wire();
        wire.binding.runtime_sources.current.manifest_sha256 = "bad".into();
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::InvalidDigest { .. })
        ));
        let mut wire = valid_wire();
        wire.binding.runtime_sources.proposed.content_sha256 = digest('0');
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::BindingMismatch { .. })
        ));

        let mut wire = valid_wire();
        wire.binding.capture_manifest.byte_length += 1;
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::BindingMismatch {
                field: "binding.capture_manifest"
            })
        ));
        let mut wire = valid_wire();
        wire.binding.terminal.file = "other.json".into();
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::WrongConstant {
                field: "binding.terminal",
                ..
            })
        ));
    }

    #[test]
    fn lowercase_digests_git_hashes_and_context_text_are_bounded() {
        let mut wire = valid_wire();
        wire.build.checkout.head = "A".repeat(40).into_boxed_str();
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::InvalidGitHash { .. })
        ));
        let mut wire = valid_wire();
        wire.build.toolchain.rustc_commit_hash = RequiredNullable(Some("short".into()));
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::InvalidGitHash { .. })
        ));
        for mutate in [
            |wire: &mut ReceiptWire| wire.build.checkout.status_sha256 = digest('A'),
            |wire: &mut ReceiptWire| wire.build.cargo_lock.sha256 = "bad".into(),
            |wire: &mut ReceiptWire| wire.build.driver.sha256 = digest('A'),
            |wire: &mut ReceiptWire| wire.build.served_files[0].sha256 = digest('A'),
        ] {
            let mut wire = valid_wire();
            mutate(&mut wire);
            assert!(matches!(
                parse(&canonical(&wire)),
                Err(BrowserProvenanceError::InvalidDigest { .. })
            ));
        }

        for text in [
            String::new(),
            "x".repeat(MAX_BROWSER_PROVENANCE_TEXT_BYTES + 1),
            "bad\u{0085}value".to_owned(),
        ] {
            let mut wire = valid_wire();
            wire.browser.product = text.into_boxed_str();
            assert!(matches!(
                parse(&canonical(&wire)),
                Err(BrowserProvenanceError::InvalidText { .. })
            ));
        }
        let mut wire = valid_wire();
        wire.graphics.system_devices[0].vendor_string = "".into();
        assert!(parse(&canonical(&wire)).is_ok());
        let mut wire = valid_wire();
        wire.graphics.system_devices[0].driver_version = "bad\u{009f}".into();
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::InvalidDeviceText { .. })
        ));

        for invalid in ["v24.1.0", "1.2", "1.2.3+build", "1.2.3\u{0085}bad"] {
            let mut wire = valid_wire();
            wire.build.driver_host.node_version = invalid.into();
            assert!(matches!(
                parse(&canonical(&wire)),
                Err(BrowserProvenanceError::InvalidVersion { .. })
            ));
        }
        let mut wire = valid_wire();
        wire.build.toolchain.cargo_version = "1.95.0-beta.1".into();
        assert!(parse(&canonical(&wire)).is_ok());
    }

    #[test]
    fn build_file_and_served_file_budgets_paths_and_order_are_enforced() {
        for (field, over) in [
            ("cargo", MAX_CARGO_LOCK_BYTES + 1),
            ("trunk", MAX_TRUNK_CONFIG_BYTES + 1),
            ("driver", MAX_DRIVER_BYTES + 1),
        ] {
            let mut wire = valid_wire();
            match field {
                "cargo" => wire.build.cargo_lock.byte_length = over,
                "trunk" => wire.build.trunk_config.byte_length = over,
                "driver" => wire.build.driver.byte_length = over,
                _ => unreachable!(),
            }
            assert!(matches!(
                parse(&canonical(&wire)),
                Err(BrowserProvenanceError::InvalidFileByteLength { .. })
            ));
        }
        let mut wire = valid_wire();
        wire.build.served_files.clear();
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::InvalidArrayLength { .. })
        ));
        let mut wire = valid_wire();
        wire.build.served_files = (0..=MAX_SERVED_FILE_COUNT)
            .map(|index| served_file(&format!("{index:03}.js"), 1, '7'))
            .collect();
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::InvalidArrayLength { .. })
        ));
        for path in [
            "../escape.js".to_owned(),
            "/absolute.js".to_owned(),
            "a\\b.js".to_owned(),
            "a//b.js".to_owned(),
            ".hidden.js".to_owned(),
            "has space.js".to_owned(),
            "unicodé.js".to_owned(),
            format!("{}/a.js", "x".repeat(129)),
            format!("{}.js", "x".repeat(MAX_BROWSER_PROVENANCE_PATH_BYTES)),
        ] {
            let mut wire = valid_wire();
            wire.build.served_files[0].path = path.into_boxed_str();
            assert!(matches!(
                parse(&canonical(&wire)),
                Err(BrowserProvenanceError::InvalidPath { .. })
            ));
        }
        let mut wire = valid_wire();
        wire.build.served_files.swap(0, 1);
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::InvalidOrder { .. })
        ));
        let mut wire = valid_wire();
        wire.build.served_files[1].path = wire.build.served_files[0].path.clone();
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::InvalidOrder { .. })
        ));
        let mut wire = valid_wire();
        wire.build.served_files[0].byte_length = MAX_SERVED_FILE_BYTES + 1;
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::InvalidFileByteLength { .. })
        ));
        let mut wire = valid_wire();
        wire.build.served_files[0].byte_length = MAX_SERVED_FILE_BYTES;
        wire.build.served_files[1].byte_length = 1;
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::InvalidServedFileAggregate { .. })
        ));
        let mut wire = valid_wire();
        for file in &mut wire.build.served_files {
            file.byte_length = 0;
        }
        assert_eq!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::InvalidServedFileAggregate { actual: 0 })
        );
    }

    #[test]
    fn graphics_arrays_have_fixed_bounds_and_required_ordering() {
        let mut wire = valid_wire();
        wire.graphics.system_devices.clear();
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::InvalidArrayLength { .. })
        ));
        let mut wire = valid_wire();
        wire.graphics.system_devices = (0..=MAX_SYSTEM_DEVICE_COUNT)
            .map(|index| system_device(index as u32, index as u32, "device"))
            .collect();
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::InvalidArrayLength { .. })
        ));

        let mut wire = valid_wire();
        wire.graphics.system_devices = vec![
            system_device(2, 2, "primary"),
            system_device(1, 1, "secondary"),
        ];
        let receipt = parse(&canonical(&wire)).unwrap();
        assert_eq!(
            receipt.graphics().system_devices()[0].device_string(),
            "primary"
        );

        let mut wire = valid_wire();
        wire.graphics.feature_status.clear();
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::InvalidArrayLength { .. })
        ));
        let mut wire = valid_wire();
        wire.graphics.feature_status.swap(0, 1);
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::InvalidOrder { .. })
        ));
        let mut wire = valid_wire();
        wire.graphics.feature_status[1].name = wire.graphics.feature_status[0].name.clone();
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::InvalidOrder { .. })
        ));
        let mut wire = valid_wire();
        wire.graphics.feature_status = (0..=MAX_FEATURE_STATUS_COUNT)
            .map(|index| FeatureStatusWire {
                name: format!("feature-{index:03}").into_boxed_str(),
                status: "enabled".into(),
            })
            .collect();
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::InvalidArrayLength { .. })
        ));

        let mut wire = valid_wire();
        wire.graphics.driver_bug_workarounds.swap(0, 1);
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::InvalidOrder { .. })
        ));
        let mut wire = valid_wire();
        wire.graphics.driver_bug_workarounds[1] = wire.graphics.driver_bug_workarounds[0].clone();
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::InvalidOrder { .. })
        ));
        let mut wire = valid_wire();
        wire.graphics.driver_bug_workarounds = (0..=MAX_DRIVER_BUG_WORKAROUND_COUNT)
            .map(|index| format!("workaround-{index:03}").into_boxed_str())
            .collect();
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::InvalidArrayLength { .. })
        ));
    }

    #[test]
    fn expected_artifact_descriptors_are_validated_before_use() {
        assert!(matches!(
            BrowserProvenanceArtifact::new("../manifest.json", 1, digest('a')),
            Err(BrowserProvenanceError::InvalidPath { .. })
        ));
        assert!(matches!(
            BrowserProvenanceArtifact::new("manifest.json", 0, digest('a')),
            Err(BrowserProvenanceError::InvalidFileByteLength { .. })
        ));
        assert!(matches!(
            BrowserProvenanceArtifact::new("manifest.json", 1, "BAD"),
            Err(BrowserProvenanceError::InvalidDigest { .. })
        ));
    }

    #[test]
    fn canonical_receipt_matches_the_javascript_golden_vector() {
        let mut wire = valid_wire();
        wire.binding.nonce = digest('1');
        wire.binding.runtime_sources = RuntimeSourcesWire {
            current: identity('2', '2'),
            proposed: identity('3', '3'),
        };
        wire.binding.capture_manifest = artifact(BROWSER_CAPTURE_MANIFEST_FILE, 17, '4');
        wire.binding.terminal = artifact(BROWSER_TERMINAL_FILE, 19, '5');
        wire.build.checkout = CheckoutWire {
            head: "6".repeat(40).into_boxed_str(),
            dirty: false,
            status_sha256: EMPTY_SHA256.into(),
        };
        wire.build.cargo_lock = declared_file(23, '7');
        wire.build.trunk_config = declared_file(29, '8');
        wire.build.driver = declared_file(31, '9');
        wire.build.driver_host = DriverHostWire {
            platform: "test-os".into(),
            architecture: "test-arch".into(),
            node_version: "24.1.0".into(),
        };
        wire.build.toolchain = ToolchainWire {
            rustc_release: "1.95.0".into(),
            rustc_commit_hash: RequiredNullable(None),
            rustc_host: "test-target".into(),
            cargo_version: "1.95.0".into(),
            trunk_version: "0.21.14".into(),
            bevy_version: BEVY_VERSION.into(),
        };
        wire.build.served_files = vec![
            served_file("assets/app.js", 37, 'a'),
            served_file("index.html", 41, 'b'),
        ];
        wire.browser = BrowserWire {
            protocol_version: "1.3".into(),
            product: "Chrome/140.0.0.0".into(),
            revision: "@test-revision".into(),
            js_version: "14.0.0".into(),
            requested_launch: RequestedLaunchWire {
                headless: HEADLESS.into(),
                gl: GL.into(),
                angle_backend: ANGLE_BACKEND.into(),
                width_px: WIDTH_PX,
                height_px: HEIGHT_PX,
                device_scale_factor: DEVICE_SCALE_FACTOR,
            },
        };
        wire.graphics = GraphicsWire {
            system_devices: vec![SystemDeviceWire {
                vendor_id: 1,
                device_id: 2,
                vendor_string: "Test Vendor".into(),
                device_string: "Test Device".into(),
                driver_vendor: "Test Driver Vendor".into(),
                driver_version: "3.0".into(),
            }],
            feature_status: vec![
                FeatureStatusWire {
                    name: "alpha".into(),
                    status: "enabled".into(),
                },
                FeatureStatusWire {
                    name: "zeta".into(),
                    status: "disabled".into(),
                },
            ],
            driver_bug_workarounds: vec!["first_workaround".into(), "second_workaround".into()],
            effective_context: EffectiveContextWire {
                api: GRAPHICS_API.into(),
                drawing_buffer_width: WIDTH_PX,
                drawing_buffer_height: HEIGHT_PX,
                vendor: "WebGL Vendor".into(),
                renderer: "WebGL Renderer".into(),
                version: "WebGL 2.0".into(),
                shading_language_version: "WebGL GLSL ES 3.00".into(),
                unmasked_vendor: "Unmasked Vendor".into(),
                unmasked_renderer: "Unmasked Renderer".into(),
            },
        };

        let bytes = canonical(&wire);
        let hash = Sha256::digest(&bytes);
        assert_eq!(
            format!("{hash:x}"),
            "d922648d9edcc34d18a487adafce959e93ba8b4d821f42d5c1017c7afe50b31f"
        );
        let sources = RuntimeSources::new(
            RuntimeIdentity::new(digest('2'), digest('2')).unwrap(),
            RuntimeIdentity::new(digest('3'), digest('3')).unwrap(),
        );
        let manifest =
            BrowserProvenanceArtifact::new(BROWSER_CAPTURE_MANIFEST_FILE, 17, digest('4')).unwrap();
        let terminal =
            BrowserProvenanceArtifact::new(BROWSER_TERMINAL_FILE, 19, digest('5')).unwrap();
        assert!(
            parse_browser_provenance_receipt(
                &bytes,
                digest('1').as_ref(),
                &sources,
                &manifest,
                &terminal,
            )
            .is_ok()
        );
    }

    #[test]
    fn canonical_capture_manifest_matches_javascript_and_returns_measured_token() {
        let capture = fixed_manifest_capture();
        let bytes = serde_json::to_vec(&fixed_manifest_wire(&capture)).unwrap();
        assert_eq!(
            sha256_hex(&bytes).as_ref(),
            "0efa5e779d475458a66a823ebf61bcd39243c644f294aca591227ff68e28d8e4"
        );
        let terminal = fixed_manifest_terminal();
        let token = parse_browser_capture_manifest(&bytes, &capture, &terminal).unwrap();
        assert_eq!(token.descriptor().file(), BROWSER_CAPTURE_MANIFEST_FILE);
        assert_eq!(token.descriptor().byte_length(), bytes.len() as u64);
        assert_eq!(token.descriptor().sha256(), sha256_hex(&bytes).as_ref());
        assert_eq!(token.terminal(), &terminal);
        assert_eq!(token.screenshots().len(), SCREENSHOT_SEQUENCE_COUNT);
        assert_eq!(token.screenshots()[7].sequence(), 7);
        assert_eq!(token.screenshots()[7].file(), SCREENSHOT_FILES[7]);
        assert_eq!(token.screenshots()[7].byte_length(), 107);
        assert_eq!(token.screenshots()[7].sha256(), digest('2').as_ref());
        assert!(!token.gate_eligible());

        let mut receipt_wire = valid_wire();
        receipt_wire.binding.nonce = capture.nonce().into();
        receipt_wire.binding.runtime_sources = RuntimeSourcesWire {
            current: identity('2', '2'),
            proposed: identity('3', '3'),
        };
        receipt_wire.binding.capture_manifest = ArtifactWire {
            file: token.descriptor().file().into(),
            byte_length: token.descriptor().byte_length(),
            sha256: token.descriptor().sha256().into(),
        };
        receipt_wire.binding.terminal = ArtifactWire {
            file: terminal.file().into(),
            byte_length: terminal.byte_length(),
            sha256: terminal.sha256().into(),
        };
        assert!(
            parse_browser_provenance_receipt(
                &canonical(&receipt_wire),
                capture.nonce(),
                capture.runtime_sources(),
                token.descriptor(),
                &terminal,
            )
            .is_ok()
        );
    }

    #[test]
    fn capture_manifest_is_closed_canonical_and_bounded() {
        let capture = fixed_manifest_capture();
        let terminal = fixed_manifest_terminal();
        assert_eq!(
            parse_browser_capture_manifest(&[], &capture, &terminal),
            Err(BrowserCaptureManifestError::InvalidLength)
        );
        assert_eq!(
            parse_browser_capture_manifest(
                &vec![b' '; MAX_BROWSER_CAPTURE_MANIFEST_BYTES + 1],
                &capture,
                &terminal,
            ),
            Err(BrowserCaptureManifestError::InvalidLength)
        );
        let bytes = serde_json::to_vec(&fixed_manifest_wire(&capture)).unwrap();
        let mut spaced = vec![b' '];
        spaced.extend_from_slice(&bytes);
        assert_eq!(
            parse_browser_capture_manifest(&spaced, &capture, &terminal),
            Err(BrowserCaptureManifestError::NonCanonicalJson)
        );
        let text = String::from_utf8(bytes).unwrap();
        let unknown = text.replacen('{', "{\"unknown\":0,", 1);
        assert!(matches!(
            parse_browser_capture_manifest(unknown.as_bytes(), &capture, &terminal),
            Err(BrowserCaptureManifestError::InvalidJson { .. })
        ));
        let duplicate = text.replacen(
            "\"file\":\"phase0b-browser-terminal.json\"",
            "\"file\":\"phase0b-browser-terminal.json\",\"file\":\"other.json\"",
            1,
        );
        assert!(matches!(
            parse_browser_capture_manifest(duplicate.as_bytes(), &capture, &terminal),
            Err(BrowserCaptureManifestError::InvalidJson { .. })
        ));
    }

    #[test]
    fn capture_manifest_constants_nonce_and_terminal_are_exact() {
        let capture = fixed_manifest_capture();
        let terminal = fixed_manifest_terminal();
        let mut wire = fixed_manifest_wire(&capture);
        wire.format_version = 2;
        assert!(matches!(
            parse_browser_capture_manifest(
                &serde_json::to_vec(&wire).unwrap(),
                &capture,
                &terminal,
            ),
            Err(BrowserCaptureManifestError::WrongFormatVersion { .. })
        ));
        for mutate in [
            |wire: &mut CaptureManifestWire| wire.artifact_kind = "wrong".into(),
            |wire: &mut CaptureManifestWire| wire.evidence_class = "wrong".into(),
            |wire: &mut CaptureManifestWire| wire.terminal.file = "terminal.json".into(),
        ] {
            let mut wire = fixed_manifest_wire(&capture);
            mutate(&mut wire);
            assert!(matches!(
                parse_browser_capture_manifest(
                    &serde_json::to_vec(&wire).unwrap(),
                    &capture,
                    &terminal,
                ),
                Err(BrowserCaptureManifestError::WrongConstant { .. })
            ));
        }
        let mut wire = fixed_manifest_wire(&capture);
        wire.gate_eligible = true;
        assert!(matches!(
            parse_browser_capture_manifest(
                &serde_json::to_vec(&wire).unwrap(),
                &capture,
                &terminal,
            ),
            Err(BrowserCaptureManifestError::WrongConstant { .. })
        ));
        let mut wire = fixed_manifest_wire(&capture);
        wire.nonce = digest('9');
        assert_eq!(
            parse_browser_capture_manifest(
                &serde_json::to_vec(&wire).unwrap(),
                &capture,
                &terminal,
            ),
            Err(BrowserCaptureManifestError::NonceMismatch)
        );
        for mutate in [
            |wire: &mut CaptureManifestWire| wire.terminal.byte_length += 1,
            |wire: &mut CaptureManifestWire| wire.terminal.sha256 = digest('f'),
        ] {
            let mut wire = fixed_manifest_wire(&capture);
            mutate(&mut wire);
            assert_eq!(
                parse_browser_capture_manifest(
                    &serde_json::to_vec(&wire).unwrap(),
                    &capture,
                    &terminal,
                ),
                Err(BrowserCaptureManifestError::TerminalMismatch)
            );
        }
    }

    #[test]
    fn capture_manifest_requires_all_eight_exact_png_descriptors() {
        let capture = fixed_manifest_capture();
        let terminal = fixed_manifest_terminal();
        let mut wire = fixed_manifest_wire(&capture);
        wire.screenshots.pop();
        assert!(matches!(
            parse_browser_capture_manifest(
                &serde_json::to_vec(&wire).unwrap(),
                &capture,
                &terminal,
            ),
            Err(BrowserCaptureManifestError::ScreenshotCount { .. })
        ));
        for mutate in [
            |wire: &mut CaptureManifestWire| wire.screenshots[1].sequence = 0,
            |wire: &mut CaptureManifestWire| wire.screenshots[1].file = "01-wrong.png".into(),
            |wire: &mut CaptureManifestWire| wire.screenshots[1].byte_length += 1,
            |wire: &mut CaptureManifestWire| wire.screenshots[1].sha256 = digest('f'),
            |wire: &mut CaptureManifestWire| wire.screenshots.swap(0, 1),
        ] {
            let mut wire = fixed_manifest_wire(&capture);
            mutate(&mut wire);
            assert!(matches!(
                parse_browser_capture_manifest(
                    &serde_json::to_vec(&wire).unwrap(),
                    &capture,
                    &terminal,
                ),
                Err(BrowserCaptureManifestError::ScreenshotMismatch { .. })
            ));
        }
    }

    #[test]
    fn binding_filenames_and_checkout_status_consistency_are_closed() {
        let mut wire = valid_wire();
        wire.binding.capture_manifest.file = "manifest.json".into();
        assert!(matches!(
            parse(&canonical(&wire)),
            Err(BrowserProvenanceError::WrongConstant {
                field: "binding.capture_manifest",
                ..
            })
        ));

        let mut clean = valid_wire();
        clean.build.checkout.dirty = false;
        clean.build.checkout.status_sha256 = EMPTY_SHA256.into();
        assert!(parse(&canonical(&clean)).is_ok());
        clean.build.checkout.status_sha256 = digest('2');
        assert_eq!(
            parse(&canonical(&clean)),
            Err(BrowserProvenanceError::CheckoutStatusMismatch)
        );

        let mut dirty = valid_wire();
        dirty.build.checkout.status_sha256 = EMPTY_SHA256.into();
        assert_eq!(
            parse(&canonical(&dirty)),
            Err(BrowserProvenanceError::CheckoutStatusMismatch)
        );
    }
}

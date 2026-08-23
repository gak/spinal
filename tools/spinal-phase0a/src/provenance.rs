//! Closed provenance derivation for Phase 0A evidence.

use crate::case::{ExportPreset, LoadedCase};
use crate::digest::{is_sha256, sha256_bytes};
use crate::operation_recipe::OperationId;
use crate::package::CasePackageInventories;
use crate::process::{ExecutableIdentity, ProcessEvidence};
use crate::spine_cli::approved_export_preset_bytes;
use crate::subprocess::inspect_executable_identity;
use serde::Serialize;
use std::env;
use thiserror::Error;

const EMBEDDED_WORKSPACE_CARGO_LOCK: &[u8] = include_bytes!("../../../Cargo.lock");

/// Exact build context embedded in the hashed harness binary.
///
/// This is contextual self-reporting, not an attestation that the observed
/// checkout produced the executable.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EmbeddedBuildContext {
    relationship: BuildContextRelationship,
    checkout: BuildCheckoutIdentity,
    cargo_lock: CargoLockIdentity,
    rustc: RustcIdentity,
    build_host_triple: String,
    target_triple: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum BuildContextRelationship {
    ContextOnlyNotBinaryAttestation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct BuildCheckoutIdentity {
    head: String,
    dirty: bool,
    status_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CargoLockIdentity {
    sha256: String,
    size: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RustcIdentity {
    verbose_version_sha256: String,
    release: String,
    commit_hash: Option<String>,
    host_triple: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProvenanceUnavailableReason {
    BuildCheckout,
    CargoLock,
    Rustc,
    BuildTriples,
    MalformedBuildContext,
    HarnessExecutable,
    HarnessReobservation,
    PackageInventories,
    SpineLauncher,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "status")]
enum ObservationState<T> {
    Available { value: T },
    Unavailable { reason: ProvenanceUnavailableReason },
    Changed { before: T, after: T },
}

impl<T> ObservationState<T> {
    fn available(value: T) -> Self {
        Self::Available { value }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct PortableExecutableIdentity {
    sha256: String,
    size: u64,
    stable_file_identity_sha256: String,
}

impl PortableExecutableIdentity {
    fn from_observed(identity: &ExecutableIdentity) -> Self {
        Self {
            sha256: identity.sha256().to_owned(),
            size: identity.size(),
            stable_file_identity_sha256: identity.stable_file_identity_sha256(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RuntimeHostIdentity {
    operating_system: String,
    process_architecture: String,
    kernel_family: String,
}

impl RuntimeHostIdentity {
    fn observe() -> Self {
        Self {
            operating_system: env::consts::OS.to_owned(),
            process_architecture: env::consts::ARCH.to_owned(),
            kernel_family: env::consts::FAMILY.to_owned(),
        }
    }
}

/// Runtime provenance captured before admission and rechecked before report
/// preparation.
pub(crate) struct ProvenanceSession {
    build_context: ObservationState<EmbeddedBuildContext>,
    harness_before: Result<ExecutableIdentity, ProvenanceUnavailableReason>,
    runtime_host: RuntimeHostIdentity,
}

/// Clean, fully parsed build observations used only for representative
/// admission. Checkout context remains explicitly contextual rather than a
/// binary attestation; the separate harness digest binds the actual bytes.
pub(crate) struct RepresentativeBuildObservation {
    source_revision: String,
    cargo_lock_sha256: String,
    harness_executable_sha256: String,
}

impl RepresentativeBuildObservation {
    pub(crate) fn source_revision(&self) -> &str {
        &self.source_revision
    }

    pub(crate) fn cargo_lock_sha256(&self) -> &str {
        &self.cargo_lock_sha256
    }

    pub(crate) fn harness_executable_sha256(&self) -> &str {
        &self.harness_executable_sha256
    }
}

impl ProvenanceSession {
    pub(crate) fn begin() -> Self {
        Self {
            build_context: embedded_build_context_state(),
            harness_before: observe_current_harness()
                .map_err(|()| ProvenanceUnavailableReason::HarnessExecutable),
            runtime_host: RuntimeHostIdentity::observe(),
        }
    }

    pub(crate) fn initially_complete(&self) -> bool {
        matches!(self.build_context, ObservationState::Available { .. })
            && self.harness_before.is_ok()
    }

    /// Returns true only when this exact harness was built with complete,
    /// clean-checkout context and its observed bytes match a reviewed
    /// representative binding.
    pub(crate) fn representative_build_observation(
        &self,
    ) -> Option<RepresentativeBuildObservation> {
        let ObservationState::Available { value } = &self.build_context else {
            return None;
        };
        if value.checkout.dirty {
            return None;
        }
        let harness = self.harness_before.as_ref().ok()?;
        Some(RepresentativeBuildObservation {
            source_revision: value.checkout.head.clone(),
            cargo_lock_sha256: value.cargo_lock.sha256.clone(),
            harness_executable_sha256: harness.sha256().to_owned(),
        })
    }

    pub(crate) fn representative_admission_ready(
        &self,
        expected_harness_sha256: &str,
        expected_source_revision: &str,
        expected_cargo_lock_sha256: &str,
    ) -> bool {
        self.representative_build_observation()
            .is_some_and(|observed| {
                observed.harness_executable_sha256 == expected_harness_sha256
                    && observed.source_revision == expected_source_revision
                    && observed.cargo_lock_sha256 == expected_cargo_lock_sha256
            })
    }

    pub(crate) fn representative_reobservation_ready(
        &self,
        expected_harness_sha256: &str,
        expected_source_revision: &str,
        expected_cargo_lock_sha256: &str,
    ) -> bool {
        if !self.representative_admission_ready(
            expected_harness_sha256,
            expected_source_revision,
            expected_cargo_lock_sha256,
        ) {
            return false;
        }
        matches!(
            self.snapshot().harness_executable,
            ObservationState::Available { value }
                if value.sha256 == expected_harness_sha256
        )
    }

    pub(crate) fn snapshot(&self) -> RuntimeProvenanceSnapshot {
        let harness = match (&self.harness_before, observe_current_harness()) {
            (Ok(before), Ok(after)) if before == &after => {
                ObservationState::available(PortableExecutableIdentity::from_observed(before))
            }
            (Ok(before), Ok(after)) => ObservationState::Changed {
                before: PortableExecutableIdentity::from_observed(before),
                after: PortableExecutableIdentity::from_observed(&after),
            },
            (Err(reason), _) => ObservationState::Unavailable { reason: *reason },
            (Ok(_), Err(())) => ObservationState::Unavailable {
                reason: ProvenanceUnavailableReason::HarnessReobservation,
            },
        };
        RuntimeProvenanceSnapshot {
            build_context: self.build_context.clone(),
            harness_executable: harness,
            runtime_host: self.runtime_host.clone(),
        }
    }
}

fn observe_current_harness() -> Result<ExecutableIdentity, ()> {
    let executable = env::current_exe().map_err(|_| ())?;
    inspect_executable_identity(&executable).map_err(|_| ())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeProvenanceSnapshot {
    build_context: ObservationState<EmbeddedBuildContext>,
    harness_executable: ObservationState<PortableExecutableIdentity>,
    runtime_host: RuntimeHostIdentity,
}

impl RuntimeProvenanceSnapshot {
    pub(crate) fn require_complete(&self) -> Result<CompleteRuntimeProvenance, ProvenanceError> {
        let ObservationState::Available {
            value: build_context,
        } = &self.build_context
        else {
            return Err(ProvenanceError::IncompleteRuntime);
        };
        let ObservationState::Available {
            value: harness_executable,
        } = &self.harness_executable
        else {
            return Err(ProvenanceError::IncompleteRuntime);
        };
        Ok(CompleteRuntimeProvenance {
            build_context: build_context.clone(),
            harness_executable: harness_executable.clone(),
            runtime_host: self.runtime_host.clone(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompleteRuntimeProvenance {
    build_context: EmbeddedBuildContext,
    harness_executable: PortableExecutableIdentity,
    runtime_host: RuntimeHostIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct PackageTreeIdentities {
    current: String,
    replacement_submission: String,
    new_submission: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ExportPresetIdentity {
    preset: ExportPreset,
    sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct FixtureIdentity {
    case_sha256: String,
    target_spine_version: String,
    packages: PackageTreeIdentities,
    export_preset: ExportPresetIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SpineLauncherIdentity {
    expected_sha256: String,
    observed: PortableExecutableIdentity,
    target_spine_version: String,
    observed_processes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "status")]
enum SpineLauncherState {
    Available {
        value: SpineLauncherIdentity,
    },
    Unavailable {
        reason: ProvenanceUnavailableReason,
    },
    Inconsistent {
        first: PortableExecutableIdentity,
        conflicting: PortableExecutableIdentity,
    },
    ExpectedDigestMismatch {
        expected_sha256: String,
        observed: PortableExecutableIdentity,
    },
}

/// Complete provenance required by a passing Phase 0A report.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompletePhase0aProvenance {
    environment: CompleteRuntimeProvenance,
    fixture: FixtureIdentity,
    spine_launcher: SpineLauncherIdentity,
}

/// Partial provenance retained by an admitted, always-failing attempt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ControlledPhase0aProvenance {
    environment: RuntimeProvenanceSnapshot,
    fixture: ControlledFixtureIdentity,
    spine_launcher: SpineLauncherState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ControlledFixtureIdentity {
    case_sha256: String,
    target_spine_version: String,
    packages: ObservationState<PackageTreeIdentities>,
    export_preset: ExportPresetIdentity,
}

pub(crate) fn complete_phase0a_provenance<'a>(
    environment: CompleteRuntimeProvenance,
    case: &LoadedCase,
    packages: &CasePackageInventories,
    processes: impl ExactSizeIterator<Item = &'a ProcessEvidence>,
) -> Result<CompletePhase0aProvenance, ProvenanceError> {
    let fixture = fixture_identity(case, packages)?;
    let spine_launcher = match launcher_state(case, processes, true)? {
        SpineLauncherState::Available { value } => value,
        _ => return Err(ProvenanceError::IncompleteLauncher),
    };
    Ok(CompletePhase0aProvenance {
        environment,
        fixture,
        spine_launcher,
    })
}

pub(crate) fn controlled_phase0a_provenance(
    environment: RuntimeProvenanceSnapshot,
    case: &LoadedCase,
    packages: Option<&CasePackageInventories>,
    processes: &[ProcessEvidence],
) -> ControlledPhase0aProvenance {
    let package_state = packages.map_or(
        ObservationState::Unavailable {
            reason: ProvenanceUnavailableReason::PackageInventories,
        },
        |packages| match package_tree_identities(packages) {
            Ok(value) => ObservationState::available(value),
            Err(_) => ObservationState::Unavailable {
                reason: ProvenanceUnavailableReason::PackageInventories,
            },
        },
    );
    let manifest = case.manifest();
    let fixture = ControlledFixtureIdentity {
        case_sha256: case.source_sha256().to_owned(),
        target_spine_version: manifest.target_spine_version.clone(),
        packages: package_state,
        export_preset: ExportPresetIdentity {
            preset: manifest.export.preset,
            sha256: sha256_bytes(approved_export_preset_bytes()),
        },
    };
    let spine_launcher =
        launcher_state(case, processes.iter(), false).unwrap_or(SpineLauncherState::Unavailable {
            reason: ProvenanceUnavailableReason::SpineLauncher,
        });
    ControlledPhase0aProvenance {
        environment,
        fixture,
        spine_launcher,
    }
}

fn fixture_identity(
    case: &LoadedCase,
    packages: &CasePackageInventories,
) -> Result<FixtureIdentity, ProvenanceError> {
    if !is_sha256(case.source_sha256()) {
        return Err(ProvenanceError::InvalidDerivedDigest);
    }
    let manifest = case.manifest();
    Ok(FixtureIdentity {
        case_sha256: case.source_sha256().to_owned(),
        target_spine_version: manifest.target_spine_version.clone(),
        packages: package_tree_identities(packages)?,
        export_preset: ExportPresetIdentity {
            preset: manifest.export.preset,
            sha256: sha256_bytes(approved_export_preset_bytes()),
        },
    })
}

fn package_tree_identities(
    packages: &CasePackageInventories,
) -> Result<PackageTreeIdentities, ProvenanceError> {
    let values = [
        &packages.current.tree_sha256,
        &packages.replacement_submission.tree_sha256,
        &packages.new_submission.tree_sha256,
    ];
    if values.into_iter().any(|value| !is_sha256(value)) {
        return Err(ProvenanceError::InvalidDerivedDigest);
    }
    Ok(PackageTreeIdentities {
        current: packages.current.tree_sha256.clone(),
        replacement_submission: packages.replacement_submission.tree_sha256.clone(),
        new_submission: packages.new_submission.tree_sha256.clone(),
    })
}

fn launcher_state<'a>(
    case: &LoadedCase,
    processes: impl ExactSizeIterator<Item = &'a ProcessEvidence>,
    require_complete: bool,
) -> Result<SpineLauncherState, ProvenanceError> {
    let count = processes.len();
    if count == 0 {
        return Ok(SpineLauncherState::Unavailable {
            reason: ProvenanceUnavailableReason::SpineLauncher,
        });
    }
    if require_complete && count != OperationId::ORDER.len() {
        return Err(ProvenanceError::IncompleteLauncher);
    }
    derive_launcher_state(
        &case.manifest().editor.expected_executable_sha256,
        &case.manifest().target_spine_version,
        processes.map(ProcessEvidence::executable_identity),
    )
}

fn derive_launcher_state<'a>(
    expected_sha256: &str,
    target_spine_version: &str,
    mut identities: impl Iterator<Item = &'a ExecutableIdentity>,
) -> Result<SpineLauncherState, ProvenanceError> {
    if !is_sha256(expected_sha256) {
        return Err(ProvenanceError::InvalidDerivedDigest);
    }
    let Some(first) = identities.next() else {
        return Ok(SpineLauncherState::Unavailable {
            reason: ProvenanceUnavailableReason::SpineLauncher,
        });
    };
    let mut count = 1_usize;
    for identity in identities {
        count = count.saturating_add(1);
        if identity != first {
            return Ok(SpineLauncherState::Inconsistent {
                first: PortableExecutableIdentity::from_observed(first),
                conflicting: PortableExecutableIdentity::from_observed(identity),
            });
        }
    }
    let observed = PortableExecutableIdentity::from_observed(first);
    if observed.sha256 != expected_sha256 {
        return Ok(SpineLauncherState::ExpectedDigestMismatch {
            expected_sha256: expected_sha256.to_owned(),
            observed,
        });
    }
    Ok(SpineLauncherState::Available {
        value: SpineLauncherIdentity {
            expected_sha256: expected_sha256.to_owned(),
            observed,
            target_spine_version: target_spine_version.to_owned(),
            observed_processes: count,
        },
    })
}

#[derive(Clone, Copy)]
struct RawBuildContext<'a> {
    checkout_state: Option<&'a str>,
    checkout_head: Option<&'a str>,
    checkout_dirty: Option<&'a str>,
    checkout_status_sha256: Option<&'a str>,
    cargo_lock_state: Option<&'a str>,
    cargo_lock_sha256: Option<&'a str>,
    cargo_lock_size: Option<&'a str>,
    rustc_state: Option<&'a str>,
    rustc_vv_sha256: Option<&'a str>,
    rustc_release: Option<&'a str>,
    rustc_commit_hash: Option<&'a str>,
    rustc_host: Option<&'a str>,
    triples_state: Option<&'a str>,
    build_host_triple: Option<&'a str>,
    target_triple: Option<&'a str>,
}

fn embedded_build_context_state() -> ObservationState<EmbeddedBuildContext> {
    match parse_build_context(compiled_raw_build_context()) {
        Ok(value)
            if value.cargo_lock.sha256 == sha256_bytes(EMBEDDED_WORKSPACE_CARGO_LOCK)
                && value.cargo_lock.size
                    == u64::try_from(EMBEDDED_WORKSPACE_CARGO_LOCK.len()).unwrap_or(u64::MAX) =>
        {
            ObservationState::available(value)
        }
        Ok(_) => ObservationState::Unavailable {
            reason: ProvenanceUnavailableReason::MalformedBuildContext,
        },
        Err(error) => ObservationState::Unavailable {
            reason: error.reason(),
        },
    }
}

fn compiled_raw_build_context() -> RawBuildContext<'static> {
    RawBuildContext {
        checkout_state: option_env!("SPINAL_PHASE0A_BUILD_CHECKOUT_STATE"),
        checkout_head: option_env!("SPINAL_PHASE0A_BUILD_CHECKOUT_HEAD"),
        checkout_dirty: option_env!("SPINAL_PHASE0A_BUILD_CHECKOUT_DIRTY"),
        checkout_status_sha256: option_env!("SPINAL_PHASE0A_BUILD_CHECKOUT_STATUS_SHA256"),
        cargo_lock_state: option_env!("SPINAL_PHASE0A_BUILD_CARGO_LOCK_STATE"),
        cargo_lock_sha256: option_env!("SPINAL_PHASE0A_BUILD_CARGO_LOCK_SHA256"),
        cargo_lock_size: option_env!("SPINAL_PHASE0A_BUILD_CARGO_LOCK_SIZE"),
        rustc_state: option_env!("SPINAL_PHASE0A_BUILD_RUSTC_STATE"),
        rustc_vv_sha256: option_env!("SPINAL_PHASE0A_BUILD_RUSTC_VV_SHA256"),
        rustc_release: option_env!("SPINAL_PHASE0A_BUILD_RUSTC_RELEASE"),
        rustc_commit_hash: option_env!("SPINAL_PHASE0A_BUILD_RUSTC_COMMIT_HASH"),
        rustc_host: option_env!("SPINAL_PHASE0A_BUILD_RUSTC_HOST"),
        triples_state: option_env!("SPINAL_PHASE0A_BUILD_TRIPLES_STATE"),
        build_host_triple: option_env!("SPINAL_PHASE0A_BUILD_BUILD_HOST_TRIPLE"),
        target_triple: option_env!("SPINAL_PHASE0A_BUILD_TARGET_TRIPLE"),
    }
}

fn parse_build_context(
    raw: RawBuildContext<'_>,
) -> Result<EmbeddedBuildContext, BuildContextError> {
    require_available(raw.checkout_state, BuildContextError::CheckoutUnavailable)?;
    require_available(
        raw.cargo_lock_state,
        BuildContextError::CargoLockUnavailable,
    )?;
    require_available(raw.rustc_state, BuildContextError::RustcUnavailable)?;
    require_available(raw.triples_state, BuildContextError::TriplesUnavailable)?;

    let head = required(raw.checkout_head)?;
    if !valid_hex_id(head) {
        return Err(BuildContextError::Malformed);
    }
    let dirty = match required(raw.checkout_dirty)? {
        "true" => true,
        "false" => false,
        _ => return Err(BuildContextError::Malformed),
    };
    let status_sha256 = required_sha256(raw.checkout_status_sha256)?;
    let lock_sha256 = required_sha256(raw.cargo_lock_sha256)?;
    let lock_size = required(raw.cargo_lock_size)?
        .parse::<u64>()
        .ok()
        .filter(|size| *size > 0)
        .ok_or(BuildContextError::Malformed)?;
    let rustc_sha256 = required_sha256(raw.rustc_vv_sha256)?;
    let release = required_safe_token(raw.rustc_release)?;
    let rustc_host = required_safe_token(raw.rustc_host)?;
    let build_host = required_safe_token(raw.build_host_triple)?;
    let target = required_safe_token(raw.target_triple)?;
    if rustc_host != build_host {
        return Err(BuildContextError::Malformed);
    }
    let commit_hash = raw
        .rustc_commit_hash
        .filter(|value| !value.is_empty())
        .map(|value| {
            if valid_hex_id(value) {
                Ok(value.to_owned())
            } else {
                Err(BuildContextError::Malformed)
            }
        })
        .transpose()?;

    Ok(EmbeddedBuildContext {
        relationship: BuildContextRelationship::ContextOnlyNotBinaryAttestation,
        checkout: BuildCheckoutIdentity {
            head: head.to_owned(),
            dirty,
            status_sha256,
        },
        cargo_lock: CargoLockIdentity {
            sha256: lock_sha256,
            size: lock_size,
        },
        rustc: RustcIdentity {
            verbose_version_sha256: rustc_sha256,
            release,
            commit_hash,
            host_triple: rustc_host,
        },
        build_host_triple: build_host,
        target_triple: target,
    })
}

fn require_available(
    state: Option<&str>,
    unavailable: BuildContextError,
) -> Result<(), BuildContextError> {
    match state {
        Some("available") => Ok(()),
        Some("unavailable") => Err(unavailable),
        _ => Err(BuildContextError::Malformed),
    }
}

fn required(value: Option<&str>) -> Result<&str, BuildContextError> {
    value
        .filter(|value| !value.is_empty())
        .ok_or(BuildContextError::Malformed)
}

fn required_sha256(value: Option<&str>) -> Result<String, BuildContextError> {
    let value = required(value)?;
    is_sha256(value)
        .then(|| value.to_owned())
        .ok_or(BuildContextError::Malformed)
}

fn required_safe_token(value: Option<&str>) -> Result<String, BuildContextError> {
    let value = required(value)?;
    safe_token(value)
        .then(|| value.to_owned())
        .ok_or(BuildContextError::Malformed)
}

fn valid_hex_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn safe_token(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && value.bytes().all(|byte| byte.is_ascii_graphic())
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
enum BuildContextError {
    #[error("build checkout context was unavailable")]
    CheckoutUnavailable,
    #[error("build Cargo.lock context was unavailable")]
    CargoLockUnavailable,
    #[error("build rustc context was unavailable")]
    RustcUnavailable,
    #[error("build target triples were unavailable")]
    TriplesUnavailable,
    #[error("embedded build context was malformed")]
    Malformed,
}

impl BuildContextError {
    const fn reason(self) -> ProvenanceUnavailableReason {
        match self {
            Self::CheckoutUnavailable => ProvenanceUnavailableReason::BuildCheckout,
            Self::CargoLockUnavailable => ProvenanceUnavailableReason::CargoLock,
            Self::RustcUnavailable => ProvenanceUnavailableReason::Rustc,
            Self::TriplesUnavailable => ProvenanceUnavailableReason::BuildTriples,
            Self::Malformed => ProvenanceUnavailableReason::MalformedBuildContext,
        }
    }
}

/// Failures while deriving mandatory passing provenance from sealed proofs.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub(crate) enum ProvenanceError {
    #[error("runtime provenance was unavailable or changed")]
    IncompleteRuntime,
    #[error("a proof-derived provenance digest was invalid")]
    InvalidDerivedDigest,
    #[error("the Spine launcher identity was incomplete or inconsistent")]
    IncompleteLauncher,
}

#[cfg(test)]
pub(crate) fn synthetic_complete_provenance(case: &LoadedCase) -> CompletePhase0aProvenance {
    const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const SHA_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    let executable = ExecutableIdentity::new(
        std::path::PathBuf::from("/private/synthetic-executable"),
        case.manifest().editor.expected_executable_sha256.clone(),
        42,
        1,
        1,
        0o100755,
        501,
        1,
        2,
        3,
        4,
    );
    let portable = PortableExecutableIdentity::from_observed(&executable);
    CompletePhase0aProvenance {
        environment: CompleteRuntimeProvenance {
            build_context: parse_build_context(RawBuildContext {
                checkout_state: Some("available"),
                checkout_head: Some("0123456789abcdef0123456789abcdef01234567"),
                checkout_dirty: Some("true"),
                checkout_status_sha256: Some(SHA_A),
                cargo_lock_state: Some("available"),
                cargo_lock_sha256: Some(SHA_B),
                cargo_lock_size: Some("123"),
                rustc_state: Some("available"),
                rustc_vv_sha256: Some(SHA_C),
                rustc_release: Some("1.93.1"),
                rustc_commit_hash: Some("abcdef0123456789abcdef0123456789abcdef01"),
                rustc_host: Some("aarch64-apple-darwin"),
                triples_state: Some("available"),
                build_host_triple: Some("aarch64-apple-darwin"),
                target_triple: Some("aarch64-apple-darwin"),
            })
            .expect("synthetic build context"),
            harness_executable: portable.clone(),
            runtime_host: RuntimeHostIdentity::observe(),
        },
        fixture: FixtureIdentity {
            case_sha256: case.source_sha256().to_owned(),
            target_spine_version: case.manifest().target_spine_version.clone(),
            packages: PackageTreeIdentities {
                current: SHA_A.to_owned(),
                replacement_submission: SHA_B.to_owned(),
                new_submission: SHA_C.to_owned(),
            },
            export_preset: ExportPresetIdentity {
                preset: case.manifest().export.preset,
                sha256: sha256_bytes(approved_export_preset_bytes()),
            },
        },
        spine_launcher: SpineLauncherIdentity {
            expected_sha256: case.manifest().editor.expected_executable_sha256.clone(),
            observed: portable,
            target_spine_version: case.manifest().target_spine_version.clone(),
            observed_processes: OperationId::ORDER.len(),
        },
    }
}

#[cfg(test)]
pub(crate) fn synthetic_controlled_provenance(
    case: &LoadedCase,
    processes: &[ProcessEvidence],
) -> ControlledPhase0aProvenance {
    let complete = synthetic_complete_provenance(case);
    controlled_phase0a_provenance(
        RuntimeProvenanceSnapshot {
            build_context: ObservationState::available(complete.environment.build_context),
            harness_executable: ObservationState::available(
                complete.environment.harness_executable,
            ),
            runtime_host: complete.environment.runtime_host,
        },
        case,
        None,
        processes,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::PackageInventory;
    use serde_json::Value;
    use std::path::PathBuf;

    const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const SHA_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    fn raw() -> RawBuildContext<'static> {
        RawBuildContext {
            checkout_state: Some("available"),
            checkout_head: Some("0123456789abcdef0123456789abcdef01234567"),
            checkout_dirty: Some("true"),
            checkout_status_sha256: Some(SHA_A),
            cargo_lock_state: Some("available"),
            cargo_lock_sha256: Some(SHA_B),
            cargo_lock_size: Some("123"),
            rustc_state: Some("available"),
            rustc_vv_sha256: Some(SHA_C),
            rustc_release: Some("1.93.1"),
            rustc_commit_hash: Some("abcdef0123456789abcdef0123456789abcdef01"),
            rustc_host: Some("aarch64-apple-darwin"),
            triples_state: Some("available"),
            build_host_triple: Some("aarch64-apple-darwin"),
            target_triple: Some("aarch64-apple-darwin"),
        }
    }

    fn executable(inode: u64, digest: &str) -> ExecutableIdentity {
        ExecutableIdentity::new(
            PathBuf::from("/private/harness"),
            digest.to_owned(),
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

    #[test]
    fn complete_build_context_is_context_not_attestation() {
        let context = parse_build_context(raw()).expect("complete context");
        let value = serde_json::to_value(context).expect("serialize context");
        assert_eq!(value["relationship"], "context_only_not_binary_attestation");
        assert_eq!(value["checkout"]["dirty"], true);
        let text = serde_json::to_string(&value).expect("context JSON");
        let macos_home_root = ["/", "Users", "/"].concat();
        assert!(!text.contains(&macos_home_root));
        assert!(!text.contains("username"));
        assert!(!text.contains("hostname"));
    }

    #[test]
    fn unavailable_and_malformed_build_context_cannot_complete() {
        let mut unavailable = raw();
        unavailable.checkout_state = Some("unavailable");
        assert_eq!(
            parse_build_context(unavailable),
            Err(BuildContextError::CheckoutUnavailable)
        );
        let mut malformed = raw();
        malformed.cargo_lock_sha256 = Some("not-a-digest");
        assert_eq!(
            parse_build_context(malformed),
            Err(BuildContextError::Malformed)
        );
        let mut wrong_host = raw();
        wrong_host.build_host_triple = Some("x86_64-unknown-linux-gnu");
        assert_eq!(
            parse_build_context(wrong_host),
            Err(BuildContextError::Malformed)
        );
    }

    #[test]
    fn full_file_identity_change_is_not_hidden_by_equal_bytes() {
        let before = executable(11, SHA_A);
        let after = executable(12, SHA_A);
        assert_ne!(before, after);
        let state = if before == after {
            ObservationState::available(PortableExecutableIdentity::from_observed(&before))
        } else {
            ObservationState::Changed {
                before: PortableExecutableIdentity::from_observed(&before),
                after: PortableExecutableIdentity::from_observed(&after),
            }
        };
        assert!(matches!(state, ObservationState::Changed { .. }));
        let serialized = serde_json::to_value(state).expect("serialize changed state");
        assert_ne!(
            serialized["before"]["stable_file_identity_sha256"],
            serialized["after"]["stable_file_identity_sha256"]
        );
    }

    #[test]
    fn inconsistent_launcher_identity_fails_even_with_equal_digest() {
        let first = executable(11, SHA_A);
        let second = executable(12, SHA_A);
        assert!(matches!(
            derive_launcher_state(SHA_A, "4.3.23", [&first, &second].into_iter())
                .expect("derived state"),
            SpineLauncherState::Inconsistent { .. }
        ));
    }

    #[test]
    fn package_roles_and_preset_are_bound_explicitly() {
        let inventory = |digest: &str| PackageInventory {
            tree_sha256: digest.to_owned(),
            entries: Vec::new(),
        };
        let packages = CasePackageInventories {
            current: inventory(SHA_A),
            replacement_submission: inventory(SHA_B),
            new_submission: inventory(SHA_C),
        };
        let identities = package_tree_identities(&packages).expect("package identities");
        let value = serde_json::to_value(identities).expect("serialize packages");
        assert_eq!(value["current"], SHA_A);
        assert_eq!(value["replacement_submission"], SHA_B);
        assert_eq!(value["new_submission"], SHA_C);
        assert_eq!(sha256_bytes(approved_export_preset_bytes()).len(), 64);
    }

    #[test]
    fn partial_runtime_provenance_cannot_be_promoted_to_complete() {
        let snapshot = RuntimeProvenanceSnapshot {
            build_context: ObservationState::Unavailable {
                reason: ProvenanceUnavailableReason::Rustc,
            },
            harness_executable: ObservationState::available(
                PortableExecutableIdentity::from_observed(&executable(1, SHA_A)),
            ),
            runtime_host: RuntimeHostIdentity::observe(),
        };
        assert_eq!(
            snapshot.require_complete(),
            Err(ProvenanceError::IncompleteRuntime)
        );
        let value = serde_json::to_value(snapshot).expect("serialize snapshot");
        let object = value.as_object().expect("object");
        assert!(!object.contains_key("bevy"));
        assert!(!object.contains_key("wasm"));
        assert!(!object.contains_key("browser"));
        assert!(!object.contains_key("gpu"));
    }

    #[test]
    fn compiled_context_never_serializes_raw_checkout_paths() {
        let state = embedded_build_context_state();
        assert!(matches!(state, ObservationState::Available { .. }));
        let value = serde_json::to_value(state).expect("serialize compiled state");
        let macos_home_root = ["/", "Users", "/"].concat();
        let windows_home_root = ["\\", "Users", "\\"].concat();
        fn walk(value: &Value, macos_home_root: &str, windows_home_root: &str) {
            match value {
                Value::Object(values) => {
                    for (key, value) in values {
                        assert!(!key.contains("path"));
                        walk(value, macos_home_root, windows_home_root);
                    }
                }
                Value::Array(values) => values
                    .iter()
                    .for_each(|value| walk(value, macos_home_root, windows_home_root)),
                Value::String(value) => {
                    assert!(!value.contains(macos_home_root));
                    assert!(!value.contains(windows_home_root));
                }
                _ => {}
            }
        }
        walk(&value, &macos_home_root, &windows_home_root);
    }
}

use crate::digest::sha256_bytes;
use crate::report::{
    ArtifactEvidence, AssertionId, AssertionStatus, ControlledFailureReport, EvidenceReport,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

const REPORT_PATH: &str = "report.json";
const REPORT_PART_PATH: &str = "report.json.part";
const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
const PRIVATE_FILE_MODE: u32 = 0o600;
const PRODUCTION_BUDGETS: EvidenceBudgets = EvidenceBudgets {
    max_artifact_bytes: 64 * 1024 * 1024,
    max_report_bytes: 64 * 1024 * 1024,
    max_total_bytes: 512 * 1024 * 1024,
};

/// The closed artifact catalog for one Phase 0A evidence directory.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum ArtifactSlot {
    CaseManifest,
    PackageInventories,
    NativeValidations,
    RoundtripComparison,
    ExistingImportComparison,
    NewImportComparison,
    ProcessStdout(usize),
    ProcessStderr(usize),
}

impl ArtifactSlot {
    fn role(self) -> &'static str {
        match self {
            Self::CaseManifest => "case-manifest",
            Self::PackageInventories => "package-inventories",
            Self::NativeValidations => "native-validations",
            Self::RoundtripComparison => "roundtrip-comparison",
            Self::ExistingImportComparison => "existing-import-comparison",
            Self::NewImportComparison => "new-import-comparison",
            Self::ProcessStdout(_) => "process-stdout",
            Self::ProcessStderr(_) => "process-stderr",
        }
    }

    fn path(self) -> String {
        match self {
            Self::CaseManifest => "case.toml".to_owned(),
            Self::PackageInventories => "package-inventories.json".to_owned(),
            Self::NativeValidations => "native-validations.json".to_owned(),
            Self::RoundtripComparison => "comparisons/roundtrip.json".to_owned(),
            Self::ExistingImportComparison => "comparisons/existing-import.json".to_owned(),
            Self::NewImportComparison => "comparisons/new-import.json".to_owned(),
            Self::ProcessStdout(index) => format!("processes/{index:04}.stdout.txt"),
            Self::ProcessStderr(index) => format!("processes/{index:04}.stderr.txt"),
        }
    }

    fn identity(self, bytes: &[u8]) -> ArtifactEvidence {
        ArtifactEvidence::from_bytes(self.role(), self.path(), bytes)
            .expect("closed artifact slots always produce valid identities")
    }
}

/// Bytes bound to an identity created by the closed artifact catalog.
#[derive(Clone, Debug)]
pub(crate) struct ArtifactPayload {
    identity: ArtifactEvidence,
    bytes: Vec<u8>,
}

impl ArtifactPayload {
    pub(crate) fn new(slot: ArtifactSlot, bytes: Vec<u8>) -> Self {
        Self {
            identity: slot.identity(&bytes),
            bytes,
        }
    }

    pub(crate) fn identity(&self) -> &ArtifactEvidence {
        &self.identity
    }
}

/// The distinct closed artifact catalog for one controlled-failure attempt.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum ControlledFailureArtifactSlot {
    Failure,
    CaseManifest,
    ProcessStdout(usize),
    ProcessStderr(usize),
}

impl ControlledFailureArtifactSlot {
    fn role(self) -> &'static str {
        match self {
            Self::Failure => "controlled-failure",
            Self::CaseManifest => "case-manifest",
            Self::ProcessStdout(_) => "process-stdout",
            Self::ProcessStderr(_) => "process-stderr",
        }
    }

    fn path(self) -> String {
        match self {
            Self::Failure => "attempt/failure.json".to_owned(),
            Self::CaseManifest => "attempt/case.toml".to_owned(),
            Self::ProcessStdout(index) => format!("attempt/processes/{index:04}.stdout.txt"),
            Self::ProcessStderr(index) => format!("attempt/processes/{index:04}.stderr.txt"),
        }
    }

    fn identity(self, bytes: &[u8]) -> ArtifactEvidence {
        ArtifactEvidence::from_bytes(self.role(), self.path(), bytes)
            .expect("closed controlled-failure slots are valid")
    }
}

/// Bytes bound to the distinct controlled-failure artifact catalog.
#[derive(Clone, Debug)]
pub(crate) struct ControlledFailureArtifactPayload {
    identity: ArtifactEvidence,
    bytes: Vec<u8>,
}

impl ControlledFailureArtifactPayload {
    pub(crate) fn new(slot: ControlledFailureArtifactSlot, bytes: Vec<u8>) -> Self {
        Self {
            identity: slot.identity(&bytes),
            bytes,
        }
    }

    pub(crate) fn identity(&self) -> &ArtifactEvidence {
        &self.identity
    }
}

/// Complete immutable input accepted by the evidence writer.
pub(crate) struct EvidenceBundle {
    report: EvidenceReport,
    payloads: Vec<ArtifactPayload>,
}

/// Complete immutable controlled-failure graph accepted only by its writer.
pub(crate) struct ControlledFailureEvidenceBundle {
    report: ControlledFailureReport,
    payloads: Vec<ControlledFailureArtifactPayload>,
}

impl ControlledFailureEvidenceBundle {
    pub(crate) fn new(
        report: ControlledFailureReport,
        payloads: Vec<ControlledFailureArtifactPayload>,
    ) -> Self {
        Self { report, payloads }
    }
}

impl EvidenceBundle {
    pub(crate) fn new(report: EvidenceReport, payloads: Vec<ArtifactPayload>) -> Self {
        Self { report, payloads }
    }
}

/// Identity of a successfully published evidence report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PersistedEvidence {
    destination: PathBuf,
    report_sha256: String,
}

impl PersistedEvidence {
    pub(crate) fn destination(&self) -> &Path {
        &self.destination
    }

    pub(crate) fn report_sha256(&self) -> &str {
        &self.report_sha256
    }
}

/// Failures while validating or publishing a private evidence directory.
#[derive(Debug, Error)]
pub(crate) enum EvidenceWriterError {
    #[error("evidence destination already exists")]
    DestinationExists,
    #[error("evidence destination parent is unavailable")]
    DestinationParentUnavailable,
    #[error("evidence bundle does not match the fixed artifact layout")]
    InvalidBundle,
    #[error("evidence contains unredacted license identity text")]
    SensitiveLicenseText,
    #[error("evidence exceeds a fixed publication byte budget")]
    SizeLimit,
    #[cfg_attr(
        any(target_os = "linux", target_os = "macos"),
        allow(
            dead_code,
            reason = "constructed only by the non-Unix fail-closed writer"
        )
    )]
    #[error("this platform cannot enforce private evidence permissions")]
    UnsupportedPlatform,
    #[error("evidence filesystem operation failed during {action}")]
    Io {
        action: &'static str,
        #[source]
        source: io::Error,
    },
    #[cfg(test)]
    #[error("injected evidence-writer failure")]
    InjectedFailure,
}

/// Unforgeable proof that the complete artifact/report graph passed layout,
/// privacy, identity, serialization, and fixed byte-budget checks.
pub(crate) struct PreparedEvidenceBundle {
    payloads: Vec<ArtifactPayload>,
    report_bytes: Vec<u8>,
    report_sha256: String,
}

/// Preflighted controlled-failure evidence; not accepted by the pass writer.
pub(crate) struct PreparedControlledFailureEvidenceBundle {
    payloads: Vec<ControlledFailureArtifactPayload>,
    report_bytes: Vec<u8>,
    report_sha256: String,
}

impl PreparedControlledFailureEvidenceBundle {
    pub(crate) fn report_sha256(&self) -> &str {
        &self.report_sha256
    }
}

impl PreparedEvidenceBundle {
    pub(crate) fn report_sha256(&self) -> &str {
        &self.report_sha256
    }
}

#[derive(Clone, Copy)]
struct EvidenceBudgets {
    max_artifact_bytes: u64,
    max_report_bytes: u64,
    max_total_bytes: u64,
}

/// Performs every content-dependent and privacy-sensitive check without
/// creating a destination. A completed runner can own only this prepared token.
pub(crate) fn prepare_evidence_bundle(
    bundle: EvidenceBundle,
) -> Result<PreparedEvidenceBundle, EvidenceWriterError> {
    prepare_with_budgets(bundle, PRODUCTION_BUDGETS)
}

/// Preflights the distinct controlled-failure graph without creating files.
pub(crate) fn prepare_controlled_failure_evidence_bundle(
    bundle: ControlledFailureEvidenceBundle,
) -> Result<PreparedControlledFailureEvidenceBundle, EvidenceWriterError> {
    prepare_controlled_failure_with_budgets(bundle, PRODUCTION_BUDGETS)
}

/// Publishes an already prepared bundle to one new private directory.
pub(crate) fn write_prepared_evidence_bundle(
    destination: &Path,
    prepared: PreparedEvidenceBundle,
) -> Result<PersistedEvidence, EvidenceWriterError> {
    require_absent_destination(destination)?;
    write_prepared(destination, prepared, false)
}

/// Publishes only a preflighted controlled-failure graph.
pub(crate) fn write_prepared_controlled_failure_evidence_bundle(
    destination: &Path,
    prepared: PreparedControlledFailureEvidenceBundle,
) -> Result<PersistedEvidence, EvidenceWriterError> {
    require_absent_destination(destination)?;
    write_prepared_controlled_failure(destination, prepared, false)
}

#[cfg(test)]
fn write_evidence_bundle(
    destination: &Path,
    bundle: EvidenceBundle,
) -> Result<PersistedEvidence, EvidenceWriterError> {
    let prepared = prepare_evidence_bundle(bundle)?;
    write_prepared_evidence_bundle(destination, prepared)
}

fn require_absent_destination(destination: &Path) -> Result<(), EvidenceWriterError> {
    match fs::symlink_metadata(destination) {
        Ok(_) => Err(EvidenceWriterError::DestinationExists),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let Some(parent) = destination.parent() else {
                return Err(EvidenceWriterError::DestinationParentUnavailable);
            };
            let metadata = fs::metadata(parent).map_err(|source| EvidenceWriterError::Io {
                action: "checking the destination parent",
                source,
            })?;
            if metadata.is_dir() {
                Ok(())
            } else {
                Err(EvidenceWriterError::DestinationParentUnavailable)
            }
        }
        Err(source) => Err(EvidenceWriterError::Io {
            action: "checking the destination",
            source,
        }),
    }
}

fn prepare_with_budgets(
    bundle: EvidenceBundle,
    budgets: EvidenceBudgets,
) -> Result<PreparedEvidenceBundle, EvidenceWriterError> {
    let process_count = bundle.report.processes().len();
    let expected_slots = required_slots(process_count);
    let mut expected_by_path = BTreeMap::new();
    for slot in expected_slots {
        expected_by_path.insert(slot.path(), slot);
    }

    if bundle.payloads.len() != expected_by_path.len() {
        return Err(EvidenceWriterError::InvalidBundle);
    }

    let mut payload_by_path = BTreeMap::new();
    let mut case_folded_paths = BTreeSet::new();
    let mut payload_identities = BTreeSet::new();
    let mut total_bytes = 0_u64;
    for payload in &bundle.payloads {
        let identity = payload.identity();
        let Some(slot) = expected_by_path.get(identity.path()) else {
            return Err(EvidenceWriterError::InvalidBundle);
        };
        if !case_folded_paths.insert(identity.path().to_ascii_lowercase())
            || slot.identity(&payload.bytes) != *identity
            || payload_by_path
                .insert(identity.path().to_owned(), identity.clone())
                .is_some()
            || !payload_identities.insert(identity.clone())
        {
            return Err(EvidenceWriterError::InvalidBundle);
        }
        let byte_length =
            u64::try_from(payload.bytes.len()).map_err(|_| EvidenceWriterError::SizeLimit)?;
        if byte_length > budgets.max_artifact_bytes {
            return Err(EvidenceWriterError::SizeLimit);
        }
        total_bytes = total_bytes
            .checked_add(byte_length)
            .filter(|total| *total <= budgets.max_total_bytes)
            .ok_or(EvidenceWriterError::SizeLimit)?;
        reject_sensitive_license_text(&payload.bytes)?;
    }

    if payload_by_path.len() != expected_by_path.len()
        || !expected_by_path
            .keys()
            .all(|path| payload_by_path.contains_key(path))
    {
        return Err(EvidenceWriterError::InvalidBundle);
    }

    let report_identities: BTreeSet<_> = bundle.report.artifacts().iter().cloned().collect();
    if report_identities.len() != bundle.report.artifacts().len()
        || report_identities != payload_identities
    {
        return Err(EvidenceWriterError::InvalidBundle);
    }

    for (index, process) in bundle.report.processes().iter().enumerate() {
        let stdout_path = ArtifactSlot::ProcessStdout(index).path();
        let stderr_path = ArtifactSlot::ProcessStderr(index).path();
        if payload_by_path.get(&stdout_path) != Some(process.stdout_artifact())
            || payload_by_path.get(&stderr_path) != Some(process.stderr_artifact())
        {
            return Err(EvidenceWriterError::InvalidBundle);
        }
    }

    let mut report_bytes =
        serde_json::to_vec_pretty(&bundle.report).map_err(|source| EvidenceWriterError::Io {
            action: "serializing the report",
            source: io::Error::new(io::ErrorKind::InvalidData, source),
        })?;
    report_bytes.push(b'\n');
    let report_byte_length =
        u64::try_from(report_bytes.len()).map_err(|_| EvidenceWriterError::SizeLimit)?;
    if report_byte_length > budgets.max_report_bytes {
        return Err(EvidenceWriterError::SizeLimit);
    }
    total_bytes = total_bytes
        .checked_add(report_byte_length)
        .filter(|total| *total <= budgets.max_total_bytes)
        .ok_or(EvidenceWriterError::SizeLimit)?;
    debug_assert!(total_bytes <= budgets.max_total_bytes);
    reject_sensitive_license_text(&report_bytes)?;
    let report_sha256 = sha256_bytes(&report_bytes);

    let mut payloads = bundle.payloads;
    payloads.sort_by(|left, right| left.identity.path().cmp(right.identity.path()));
    Ok(PreparedEvidenceBundle {
        payloads,
        report_bytes,
        report_sha256,
    })
}

fn prepare_controlled_failure_with_budgets(
    bundle: ControlledFailureEvidenceBundle,
    budgets: EvidenceBudgets,
) -> Result<PreparedControlledFailureEvidenceBundle, EvidenceWriterError> {
    if bundle.report.passed()
        || bundle.report.assertions().len() != AssertionId::required().len()
        || bundle
            .report
            .assertions()
            .iter()
            .zip(AssertionId::required())
            .any(|(result, required)| result.id() != *required || result.evidence().is_empty())
        || bundle
            .report
            .assertions()
            .iter()
            .all(|result| result.status() == AssertionStatus::Passed)
    {
        return Err(EvidenceWriterError::InvalidBundle);
    }

    let mut payload_by_path = BTreeMap::new();
    let mut case_folded_paths = BTreeSet::new();
    let mut payload_identities = BTreeSet::new();
    let mut total_bytes = 0_u64;
    for payload in &bundle.payloads {
        let identity = payload.identity();
        if !controlled_failure_identity_is_allowed(identity)
            || !case_folded_paths.insert(identity.path().to_ascii_lowercase())
            || payload_by_path
                .insert(identity.path().to_owned(), identity.clone())
                .is_some()
            || !payload_identities.insert(identity.clone())
            || ArtifactEvidence::from_bytes(identity.role(), identity.path(), &payload.bytes)
                .map_err(|_| EvidenceWriterError::InvalidBundle)?
                != *identity
        {
            return Err(EvidenceWriterError::InvalidBundle);
        }
        let byte_length =
            u64::try_from(payload.bytes.len()).map_err(|_| EvidenceWriterError::SizeLimit)?;
        if byte_length > budgets.max_artifact_bytes {
            return Err(EvidenceWriterError::SizeLimit);
        }
        total_bytes = total_bytes
            .checked_add(byte_length)
            .filter(|total| *total <= budgets.max_total_bytes)
            .ok_or(EvidenceWriterError::SizeLimit)?;
        reject_sensitive_license_text(&payload.bytes)?;
    }

    let failure_path = ControlledFailureArtifactSlot::Failure.path();
    if payload_by_path
        .get(&failure_path)
        .is_none_or(|identity| identity.role() != "controlled-failure")
    {
        return Err(EvidenceWriterError::InvalidBundle);
    }
    let report_identities = bundle
        .report
        .artifacts()
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if report_identities.len() != bundle.report.artifacts().len()
        || report_identities != payload_identities
        || bundle.report.assertions().iter().any(|assertion| {
            assertion
                .evidence()
                .iter()
                .any(|identity| !payload_identities.contains(identity))
        })
    {
        return Err(EvidenceWriterError::InvalidBundle);
    }

    for (index, process) in bundle.report.processes().iter().enumerate() {
        let stdout_path = ControlledFailureArtifactSlot::ProcessStdout(index).path();
        let stderr_path = ControlledFailureArtifactSlot::ProcessStderr(index).path();
        match process.transcript_artifacts() {
            Some((stdout, stderr))
                if payload_by_path.get(&stdout_path) == Some(stdout)
                    && payload_by_path.get(&stderr_path) == Some(stderr) => {}
            None if !payload_by_path.contains_key(&stdout_path)
                && !payload_by_path.contains_key(&stderr_path) => {}
            Some(_) | None => return Err(EvidenceWriterError::InvalidBundle),
        }
    }
    let process_count = bundle.report.processes().len();
    if payload_by_path.keys().any(|path| {
        controlled_failure_process_index(path).is_some_and(|index| index >= process_count)
    }) {
        return Err(EvidenceWriterError::InvalidBundle);
    }

    let mut report_bytes =
        serde_json::to_vec_pretty(&bundle.report).map_err(|source| EvidenceWriterError::Io {
            action: "serializing the controlled-failure report",
            source: io::Error::new(io::ErrorKind::InvalidData, source),
        })?;
    report_bytes.push(b'\n');
    let report_byte_length =
        u64::try_from(report_bytes.len()).map_err(|_| EvidenceWriterError::SizeLimit)?;
    if report_byte_length > budgets.max_report_bytes {
        return Err(EvidenceWriterError::SizeLimit);
    }
    total_bytes = total_bytes
        .checked_add(report_byte_length)
        .filter(|total| *total <= budgets.max_total_bytes)
        .ok_or(EvidenceWriterError::SizeLimit)?;
    debug_assert!(total_bytes <= budgets.max_total_bytes);
    reject_sensitive_license_text(&report_bytes)?;
    let report_sha256 = sha256_bytes(&report_bytes);

    let mut payloads = bundle.payloads;
    payloads.sort_by(|left, right| left.identity.path().cmp(right.identity.path()));
    Ok(PreparedControlledFailureEvidenceBundle {
        payloads,
        report_bytes,
        report_sha256,
    })
}

fn controlled_failure_identity_is_allowed(identity: &ArtifactEvidence) -> bool {
    match identity.path() {
        "attempt/failure.json" => identity.role() == "controlled-failure",
        "attempt/case.toml" => identity.role() == "case-manifest",
        path if path.ends_with(".stdout.txt") => {
            controlled_failure_process_index(path).is_some() && identity.role() == "process-stdout"
        }
        path if path.ends_with(".stderr.txt") => {
            controlled_failure_process_index(path).is_some() && identity.role() == "process-stderr"
        }
        _ => false,
    }
}

fn controlled_failure_process_index(path: &str) -> Option<usize> {
    let name = path.strip_prefix("attempt/processes/")?;
    let (digits, suffix) = name.split_once('.')?;
    if digits.len() != 4
        || !digits.bytes().all(|byte| byte.is_ascii_digit())
        || !matches!(suffix, "stdout.txt" | "stderr.txt")
    {
        return None;
    }
    digits.parse().ok()
}

fn required_slots(process_count: usize) -> Vec<ArtifactSlot> {
    let mut slots = vec![
        ArtifactSlot::CaseManifest,
        ArtifactSlot::PackageInventories,
        ArtifactSlot::NativeValidations,
        ArtifactSlot::RoundtripComparison,
        ArtifactSlot::ExistingImportComparison,
        ArtifactSlot::NewImportComparison,
    ];
    for index in 0..process_count {
        slots.push(ArtifactSlot::ProcessStdout(index));
        slots.push(ArtifactSlot::ProcessStderr(index));
    }
    slots
}

fn reject_sensitive_license_text(bytes: &[u8]) -> Result<(), EvidenceWriterError> {
    let mut line_start = 0;
    let mut cursor = 0;
    while cursor < bytes.len() {
        if matches!(bytes[cursor], b'\r' | b'\n') {
            validate_license_line(&bytes[line_start..cursor])?;
            if bytes[cursor] == b'\r' && bytes.get(cursor.saturating_add(1)).copied() == Some(b'\n')
            {
                cursor += 1;
            }
            line_start = cursor + 1;
        }
        cursor += 1;
    }
    validate_license_line(&bytes[line_start..])
}

pub(crate) fn evidence_bytes_are_privacy_safe(bytes: &[u8]) -> bool {
    reject_sensitive_license_text(bytes).is_ok()
}

pub(crate) fn evidence_json_string_is_privacy_safe(value: &str) -> bool {
    const MARKER: &[u8] = b"Licensed to:";
    !value.as_bytes().windows(MARKER.len()).any(|candidate| {
        candidate
            .iter()
            .zip(MARKER)
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
    })
}

fn validate_license_line(line: &[u8]) -> Result<(), EvidenceWriterError> {
    const MARKER: &[u8] = b"Licensed to:";
    const SAFE_LINE: &[u8] = b"Licensed to: <hidden>";
    let contains_marker = line.windows(MARKER.len()).any(|candidate| {
        candidate
            .iter()
            .zip(MARKER)
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
    });
    if contains_marker && line != SAFE_LINE {
        Err(EvidenceWriterError::SensitiveLicenseText)
    } else {
        Ok(())
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn write_prepared(
    destination: &Path,
    prepared: PreparedEvidenceBundle,
    fail_before_report: bool,
) -> Result<PersistedEvidence, EvidenceWriterError> {
    create_private_directory(destination)?;

    let mut parents = BTreeSet::new();
    for payload in &prepared.payloads {
        let path = Path::new(payload.identity.path());
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            parents.insert(parent.to_owned());
        }
    }
    for parent in parents {
        create_private_directory(&destination.join(parent))?;
    }

    for payload in &prepared.payloads {
        write_private_file(
            &destination.join(payload.identity.path()),
            &payload.bytes,
            payload.identity.sha256(),
        )?;
    }

    if fail_before_report {
        #[cfg(test)]
        return Err(EvidenceWriterError::InjectedFailure);
        #[cfg(not(test))]
        unreachable!("production writer never requests failure injection");
    }

    let report_part = destination.join(REPORT_PART_PATH);
    write_private_file(
        &report_part,
        &prepared.report_bytes,
        &prepared.report_sha256,
    )?;
    sync_directory(destination)?;
    let report_path = destination.join(REPORT_PATH);
    // `write_private_file` already reopened and verified the exact bytes,
    // metadata, ownership, and single-link identity of `report.json.part`.
    // Keep the same-directory rename as the final fallible operation: once
    // `report.json` is visible this function must not be able to return `Err`.
    fs::rename(&report_part, &report_path).map_err(|source| EvidenceWriterError::Io {
        action: "publishing report.json",
        source,
    })?;

    Ok(PersistedEvidence {
        destination: destination.to_owned(),
        report_sha256: prepared.report_sha256,
    })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn write_prepared_controlled_failure(
    destination: &Path,
    prepared: PreparedControlledFailureEvidenceBundle,
    fail_before_report: bool,
) -> Result<PersistedEvidence, EvidenceWriterError> {
    create_private_directory(destination)?;

    let mut parents = BTreeSet::new();
    for payload in &prepared.payloads {
        let path = Path::new(payload.identity.path());
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            for ancestor in parent
                .ancestors()
                .filter(|value| !value.as_os_str().is_empty())
            {
                parents.insert(ancestor.to_owned());
            }
        }
    }
    let mut parents = parents.into_iter().collect::<Vec<_>>();
    parents.sort_by_key(|path| path.components().count());
    for parent in parents {
        create_private_directory(&destination.join(parent))?;
    }

    for payload in &prepared.payloads {
        write_private_file(
            &destination.join(payload.identity.path()),
            &payload.bytes,
            payload.identity.sha256(),
        )?;
    }

    if fail_before_report {
        #[cfg(test)]
        return Err(EvidenceWriterError::InjectedFailure);
        #[cfg(not(test))]
        unreachable!("production writer never requests failure injection");
    }

    let report_part = destination.join(REPORT_PART_PATH);
    write_private_file(
        &report_part,
        &prepared.report_bytes,
        &prepared.report_sha256,
    )?;
    sync_directory(destination)?;
    let report_path = destination.join(REPORT_PATH);
    fs::rename(&report_part, &report_path).map_err(|source| EvidenceWriterError::Io {
        action: "publishing controlled-failure report.json",
        source,
    })?;

    Ok(PersistedEvidence {
        destination: destination.to_owned(),
        report_sha256: prepared.report_sha256,
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn write_prepared_controlled_failure(
    _destination: &Path,
    _prepared: PreparedControlledFailureEvidenceBundle,
    _fail_before_report: bool,
) -> Result<PersistedEvidence, EvidenceWriterError> {
    Err(EvidenceWriterError::UnsupportedPlatform)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn write_prepared(
    _destination: &Path,
    _prepared: PreparedEvidenceBundle,
    _fail_before_report: bool,
) -> Result<PersistedEvidence, EvidenceWriterError> {
    Err(EvidenceWriterError::UnsupportedPlatform)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn create_private_directory(path: &Path) -> Result<(), EvidenceWriterError> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    let mut builder = fs::DirBuilder::new();
    builder.mode(PRIVATE_DIRECTORY_MODE);
    builder
        .create(path)
        .map_err(|source| EvidenceWriterError::Io {
            action: "creating a private directory",
            source,
        })?;
    fs::set_permissions(path, fs::Permissions::from_mode(PRIVATE_DIRECTORY_MODE)).map_err(
        |source| EvidenceWriterError::Io {
            action: "setting private directory permissions",
            source,
        },
    )?;
    verify_private_metadata(path, true)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn write_private_file(
    path: &Path,
    bytes: &[u8],
    expected_sha256: &str,
) -> Result<(), EvidenceWriterError> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .mode(PRIVATE_FILE_MODE)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let mut file = options
        .open(path)
        .map_err(|source| EvidenceWriterError::Io {
            action: "creating a private evidence file",
            source,
        })?;
    file.set_permissions(fs::Permissions::from_mode(PRIVATE_FILE_MODE))
        .map_err(|source| EvidenceWriterError::Io {
            action: "setting private file permissions",
            source,
        })?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|source| EvidenceWriterError::Io {
            action: "writing a private evidence file",
            source,
        })?;
    drop(file);
    verify_private_metadata(path, false)?;
    verify_file_hash(path, expected_sha256)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn verify_private_metadata(path: &Path, directory: bool) -> Result<(), EvidenceWriterError> {
    use std::os::unix::fs::MetadataExt;

    let metadata = fs::symlink_metadata(path).map_err(|source| EvidenceWriterError::Io {
        action: "verifying private evidence metadata",
        source,
    })?;
    let expected_mode = if directory {
        PRIVATE_DIRECTORY_MODE
    } else {
        PRIVATE_FILE_MODE
    };
    let type_matches = if directory {
        metadata.file_type().is_dir()
    } else {
        metadata.file_type().is_file() && metadata.nlink() == 1
    };
    if !type_matches
        || metadata.mode() & 0o7777 != expected_mode
        || metadata.uid() != rustix::process::geteuid().as_raw()
    {
        return Err(EvidenceWriterError::Io {
            action: "verifying private evidence metadata",
            source: io::Error::new(io::ErrorKind::PermissionDenied, "unsafe evidence metadata"),
        });
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn verify_file_hash(path: &Path, expected_sha256: &str) -> Result<(), EvidenceWriterError> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let mut file = options
        .open(path)
        .map_err(|source| EvidenceWriterError::Io {
            action: "reopening a private evidence file",
            source,
        })?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|source| EvidenceWriterError::Io {
            action: "hashing a private evidence file",
            source,
        })?;
    if sha256_bytes(&bytes) != expected_sha256 {
        return Err(EvidenceWriterError::Io {
            action: "verifying a private evidence file hash",
            source: io::Error::new(io::ErrorKind::InvalidData, "evidence hash mismatch"),
        });
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn sync_directory(path: &Path) -> Result<(), EvidenceWriterError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| EvidenceWriterError::Io {
            action: "synchronizing the evidence directory",
            source,
        })
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;
    use crate::{ReportBuilder, parse_case};
    use std::os::unix::fs::{MetadataExt, symlink};

    fn payloads_with(slot_override: Option<(ArtifactSlot, &[u8])>) -> Vec<ArtifactPayload> {
        required_slots(0)
            .into_iter()
            .map(|slot| {
                let bytes = slot_override
                    .filter(|(selected, _)| *selected == slot)
                    .map_or(b"{}\n".as_slice(), |(_, bytes)| bytes);
                ArtifactPayload::new(slot, bytes.to_vec())
            })
            .collect()
    }

    fn bundle_with(slot_override: Option<(ArtifactSlot, &[u8])>) -> EvidenceBundle {
        let payloads = payloads_with(slot_override);
        let case = parse_case(include_str!("../cases/example.toml")).expect("example case");
        let mut builder = ReportBuilder::new(&case);
        for payload in &payloads {
            builder.push_artifact(payload.identity().clone());
        }
        EvidenceBundle::new(builder.finish(), payloads)
    }

    #[test]
    fn writes_fixed_private_layout_and_returns_report_hash() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let destination = temporary.path().join("evidence");
        let prepared = prepare_evidence_bundle(bundle_with(None)).expect("prepare bundle");
        let prepared_report_sha256 = prepared.report_sha256().to_owned();
        assert!(!destination.exists());
        let result = write_prepared_evidence_bundle(&destination, prepared).expect("write bundle");

        assert_eq!(result.destination(), destination);
        assert_eq!(result.report_sha256(), prepared_report_sha256);
        let report_bytes = fs::read(destination.join(REPORT_PATH)).expect("report bytes");
        assert_eq!(result.report_sha256(), sha256_bytes(&report_bytes));
        assert!(
            !report_bytes
                .windows(b"report_sha256".len())
                .any(|value| value == b"report_sha256")
        );
        assert!(!destination.join(REPORT_PART_PATH).exists());
        assert_eq!(
            fs::metadata(&destination).expect("root metadata").mode() & 0o777,
            PRIVATE_DIRECTORY_MODE
        );
        assert_eq!(
            fs::metadata(destination.join("comparisons"))
                .expect("comparison directory metadata")
                .mode()
                & 0o777,
            PRIVATE_DIRECTORY_MODE
        );
        for slot in required_slots(0) {
            assert_eq!(
                fs::metadata(destination.join(slot.path()))
                    .expect("artifact metadata")
                    .mode()
                    & 0o777,
                PRIVATE_FILE_MODE
            );
        }
        assert_eq!(
            fs::metadata(destination.join(REPORT_PATH))
                .expect("report metadata")
                .mode()
                & 0o777,
            PRIVATE_FILE_MODE
        );
    }

    #[test]
    fn duplicate_content_hashes_across_slots_are_written() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let destination = temporary.path().join("evidence");
        write_evidence_bundle(&destination, bundle_with(None)).expect("duplicate content is safe");
        assert!(destination.join("comparisons/roundtrip.json").is_file());
        assert!(destination.join("comparisons/new-import.json").is_file());
    }

    #[test]
    fn payload_bytes_must_match_their_identity_before_destination_creation() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let destination = temporary.path().join("evidence");
        let mut bundle = bundle_with(None);
        bundle.payloads[0].bytes.extend_from_slice(b"tampered");

        assert!(matches!(
            write_evidence_bundle(&destination, bundle),
            Err(EvidenceWriterError::InvalidBundle)
        ));
        assert!(!destination.exists());
    }

    #[test]
    fn report_and_payload_identities_must_match_exactly() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let destination = temporary.path().join("evidence");
        let mut bundle = bundle_with(None);
        bundle.payloads.pop();

        assert!(matches!(
            write_evidence_bundle(&destination, bundle),
            Err(EvidenceWriterError::InvalidBundle)
        ));
        assert!(!destination.exists());
    }

    #[test]
    fn privacy_failure_happens_before_destination_creation_and_hides_content() {
        for unsafe_bytes in [
            b"Licensed to: person@example.test".as_slice(),
            b"prefix LICENSED TO: secret suffix".as_slice(),
            b"Licensed to:".as_slice(),
            b"Licensed to: <hid".as_slice(),
            b"Licensed to: <hidden>tail".as_slice(),
            b"licensed to: <hidden>".as_slice(),
            b"{\"message\":\"Licensed to: <hidden>\"}".as_slice(),
            b"before\nLicensed to: secret\nafter".as_slice(),
            b"before\r\nLicensed to: secret\r\nafter".as_slice(),
            b"before\rLicensed to: secret\rafter".as_slice(),
        ] {
            let temporary = tempfile::tempdir().expect("temporary directory");
            let destination = temporary.path().join("evidence");
            let error = write_evidence_bundle(
                &destination,
                bundle_with(Some((ArtifactSlot::CaseManifest, unsafe_bytes))),
            )
            .expect_err("unredacted license identity must fail");
            assert!(matches!(error, EvidenceWriterError::SensitiveLicenseText));
            assert_eq!(
                error.to_string(),
                "evidence contains unredacted license identity text"
            );
            assert!(!destination.exists());
        }
    }

    #[test]
    fn exact_hidden_license_line_is_safe_for_all_supported_line_endings() {
        for bytes in [
            b"before\nLicensed to: <hidden>\nafter".as_slice(),
            b"before\r\nLicensed to: <hidden>\r\nafter".as_slice(),
            b"before\rLicensed to: <hidden>\rafter".as_slice(),
        ] {
            reject_sensitive_license_text(bytes).expect("exact hidden line");
        }
        assert!(!evidence_json_string_is_privacy_safe(
            "Licensed to: <hidden>"
        ));
        assert!(evidence_json_string_is_privacy_safe("ordinary diagnostic"));
    }

    #[test]
    fn preexisting_file_directory_and_symlink_are_refused() {
        for kind in ["file", "directory", "symlink"] {
            let temporary = tempfile::tempdir().expect("temporary directory");
            let destination = temporary.path().join("evidence");
            match kind {
                "file" => fs::write(&destination, b"existing").expect("existing file"),
                "directory" => fs::create_dir(&destination).expect("existing directory"),
                "symlink" => symlink("missing-target", &destination).expect("dangling symlink"),
                _ => unreachable!(),
            }
            assert!(matches!(
                write_evidence_bundle(&destination, bundle_with(None)),
                Err(EvidenceWriterError::DestinationExists)
            ));
        }
    }

    #[test]
    fn a_failure_after_payloads_never_leaves_report_json() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let destination = temporary.path().join("evidence");
        require_absent_destination(&destination).expect("absent destination");
        let prepared = prepare_evidence_bundle(bundle_with(None)).expect("prepare bundle");

        assert!(matches!(
            write_prepared(&destination, prepared, true),
            Err(EvidenceWriterError::InjectedFailure)
        ));
        assert!(destination.is_dir());
        assert!(!destination.join(REPORT_PATH).exists());
        assert!(!destination.join(REPORT_PART_PATH).exists());
    }

    #[test]
    fn fixed_byte_budgets_fail_during_preparation_without_creating_a_destination() {
        let tiny_artifact = EvidenceBudgets {
            max_artifact_bytes: 1,
            max_report_bytes: u64::MAX,
            max_total_bytes: u64::MAX,
        };
        assert!(matches!(
            prepare_with_budgets(bundle_with(None), tiny_artifact),
            Err(EvidenceWriterError::SizeLimit)
        ));

        let tiny_report = EvidenceBudgets {
            max_artifact_bytes: u64::MAX,
            max_report_bytes: 1,
            max_total_bytes: u64::MAX,
        };
        assert!(matches!(
            prepare_with_budgets(bundle_with(None), tiny_report),
            Err(EvidenceWriterError::SizeLimit)
        ));

        let payload_total = payloads_with(None)
            .iter()
            .map(|payload| u64::try_from(payload.bytes.len()).expect("payload length"))
            .sum::<u64>();
        let tiny_total = EvidenceBudgets {
            max_artifact_bytes: u64::MAX,
            max_report_bytes: u64::MAX,
            max_total_bytes: payload_total,
        };
        assert!(matches!(
            prepare_with_budgets(bundle_with(None), tiny_total),
            Err(EvidenceWriterError::SizeLimit)
        ));
    }
}

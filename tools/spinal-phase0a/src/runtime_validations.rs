//! Closed native-runtime validation gate for the three Phase 0A review bundles.

use crate::{
    digest::sha256_bytes,
    native_validator::{
        NativeValidationError, NativeValidationEvidence, validate_validated_runtime_bundle,
    },
    phase0_analysis::{CompletedPhase0Analysis, JsonDocumentRole},
    run_workspace::{RuntimeFileBinding, RuntimeInputs, RuntimeTargetInput},
};
use serde::Serialize;
use spinal::{
    RuntimeBundleError, RuntimeBundleManifest, TARGET_SPINE_VERSION, ValidatedRuntimeBundle,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::PathBuf,
};
use thiserror::Error;

const ARTIFACT_FORMAT_VERSION: u32 = 1;

/// The three fixed runtime roles accepted by the Phase 0A gate.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RuntimeValidationRole {
    CurrentB,
    ExistingRepeat,
    NewFirst,
}

impl RuntimeValidationRole {
    const fn analysis_role(self) -> JsonDocumentRole {
        match self {
            Self::CurrentB => JsonDocumentRole::CurrentB,
            Self::ExistingRepeat => JsonDocumentRole::ExistingRepeat,
            Self::NewFirst => JsonDocumentRole::NewFirst,
        }
    }

    const fn manifest_label(self) -> &'static str {
        match self {
            Self::CurrentB => "Phase 0A current B runtime",
            Self::ExistingRepeat => "Phase 0A existing repeat runtime",
            Self::NewFirst => "Phase 0A new first-import runtime",
        }
    }
}

impl fmt::Display for RuntimeValidationRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::CurrentB => "current_b",
            Self::ExistingRepeat => "existing_repeat",
            Self::NewFirst => "new_first",
        };
        formatter.write_str(value)
    }
}

/// Deterministic serializable view for native-validations.json.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeValidationsArtifact {
    format_version: u32,
    target_spine_version: &'static str,
    validations: [RuntimeValidationRecord; 3],
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RuntimeValidationRecord {
    role: RuntimeValidationRole,
    analysis_json: AnalysisJsonIdentity,
    workspace_files: Vec<RuntimeFileBinding>,
    native: NativeValidationEvidence,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct AnalysisJsonIdentity {
    role: RuntimeValidationRole,
    sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RuntimeAssetIdentity {
    atlas: RuntimeFileIdentity,
    pages: Vec<RuntimeFileIdentity>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RuntimeFileIdentity {
    path: String,
    byte_length: usize,
    sha256: String,
}

struct ValidatedTarget {
    bundle: ValidatedRuntimeBundle,
    bindings: Vec<RuntimeFileBinding>,
}

struct BoundTarget {
    record: RuntimeValidationRecord,
    assets: RuntimeAssetIdentity,
    json_path: PathBuf,
}

/// Unforgeable proof that all three exact runtime bundles passed native checks
/// and were bound to the matching closed semantic-analysis roles.
pub(crate) struct CompletedRuntimeValidations {
    artifact: RuntimeValidationsArtifact,
}

impl CompletedRuntimeValidations {
    /// Borrows the deterministic serializable native-validation artifact.
    #[cfg(test)]
    pub(crate) fn artifact_view(&self) -> &RuntimeValidationsArtifact {
        &self.artifact
    }

    /// Serializes native-validations.json with stable field and role order.
    pub(crate) fn artifact_bytes(&self) -> Result<Vec<u8>, RuntimeValidationsError> {
        let mut bytes = serde_json::to_vec_pretty(&self.artifact)
            .map_err(RuntimeValidationsError::ArtifactSerialization)?;
        bytes.push(b'\n');
        Ok(bytes)
    }
}

/// A fail-closed runtime-gate failure.
#[derive(Debug, Error)]
pub(crate) enum RuntimeValidationsError {
    #[error("workspace runtime bindings did not exactly match {role} bytes")]
    WorkspaceBindingMismatch { role: RuntimeValidationRole },
    #[error("shared runtime validation failed for {role}: {source}")]
    Shared {
        role: RuntimeValidationRole,
        #[source]
        source: RuntimeBundleError,
    },
    #[error("native runtime validation failed for {role}: {source}")]
    Native {
        role: RuntimeValidationRole,
        #[source]
        source: NativeValidationError,
    },
    #[error("native JSON bytes for {role} do not match the closed analysis role")]
    JsonIdentityMismatch { role: RuntimeValidationRole },
    #[error("runtime JSON virtual paths differ at {role}")]
    JsonPathMismatch { role: RuntimeValidationRole },
    #[error("runtime atlas or page identities differ at {role}")]
    AssetIdentityMismatch { role: RuntimeValidationRole },
    #[error("native runtime for {role} is not exact Spine {TARGET_SPINE_VERSION}")]
    VersionMismatch { role: RuntimeValidationRole },
    #[error("could not serialize native-validations.json: {0}")]
    ArtifactSerialization(serde_json::Error),
}

/// Consumes the sole workspace-produced runtime token and mints the closed
/// three-role native-validation proof. No caller-supplied role, digest, or pass
/// state is accepted.
pub(crate) fn complete_runtime_validations(
    analysis: &CompletedPhase0Analysis,
    runtime_inputs: RuntimeInputs,
) -> Result<CompletedRuntimeValidations, RuntimeValidationsError> {
    let (current, existing, new) = runtime_inputs.into_targets();
    let current = build_target(RuntimeValidationRole::CurrentB, current)?;
    let existing = build_target(RuntimeValidationRole::ExistingRepeat, existing)?;
    let new = build_target(RuntimeValidationRole::NewFirst, new)?;
    bind_validated_targets(analysis, current, existing, new)
}

fn build_target(
    role: RuntimeValidationRole,
    target: RuntimeTargetInput,
) -> Result<ValidatedTarget, RuntimeValidationsError> {
    validate_workspace_bindings(role, target.files(), target.bindings())?;
    let mut bindings = target.bindings().to_vec();
    bindings.sort_by(|left, right| left.virtual_path().cmp(right.virtual_path()));
    let (json_path, atlas_path, files) = target.into_bundle_parts();
    let (_manifest, bundle) =
        RuntimeBundleManifest::build(role.manifest_label(), &json_path, &atlas_path, files)
            .map_err(|source| RuntimeValidationsError::Shared { role, source })?;
    Ok(ValidatedTarget { bundle, bindings })
}

fn validate_workspace_bindings(
    role: RuntimeValidationRole,
    files: &BTreeMap<PathBuf, Vec<u8>>,
    bindings: &[RuntimeFileBinding],
) -> Result<(), RuntimeValidationsError> {
    if files.len() != bindings.len() {
        return Err(RuntimeValidationsError::WorkspaceBindingMismatch { role });
    }
    let mut bound_paths = BTreeSet::new();
    for binding in bindings {
        let Some(bytes) = files.get(binding.virtual_path()) else {
            return Err(RuntimeValidationsError::WorkspaceBindingMismatch { role });
        };
        let Ok(byte_length) = u64::try_from(bytes.len()) else {
            return Err(RuntimeValidationsError::WorkspaceBindingMismatch { role });
        };
        if binding.workspace_path().is_empty()
            || binding.size() != byte_length
            || binding.sha256() != sha256_bytes(bytes)
            || !bound_paths.insert(binding.virtual_path().to_path_buf())
        {
            return Err(RuntimeValidationsError::WorkspaceBindingMismatch { role });
        }
    }
    if bound_paths != files.keys().cloned().collect() {
        return Err(RuntimeValidationsError::WorkspaceBindingMismatch { role });
    }
    Ok(())
}

fn bind_validated_targets(
    analysis: &CompletedPhase0Analysis,
    current: ValidatedTarget,
    existing: ValidatedTarget,
    new: ValidatedTarget,
) -> Result<CompletedRuntimeValidations, RuntimeValidationsError> {
    let current = bind_target(analysis, RuntimeValidationRole::CurrentB, current)?;
    let existing = bind_target(analysis, RuntimeValidationRole::ExistingRepeat, existing)?;
    let new = bind_target(analysis, RuntimeValidationRole::NewFirst, new)?;

    for (role, target) in [
        (RuntimeValidationRole::ExistingRepeat, &existing),
        (RuntimeValidationRole::NewFirst, &new),
    ] {
        if target.json_path != current.json_path {
            return Err(RuntimeValidationsError::JsonPathMismatch { role });
        }
        if target.assets != current.assets {
            return Err(RuntimeValidationsError::AssetIdentityMismatch { role });
        }
    }

    let BoundTarget {
        record: current_record,
        ..
    } = current;
    let BoundTarget {
        record: existing_record,
        ..
    } = existing;
    let BoundTarget {
        record: new_record, ..
    } = new;

    Ok(CompletedRuntimeValidations {
        artifact: RuntimeValidationsArtifact {
            format_version: ARTIFACT_FORMAT_VERSION,
            target_spine_version: TARGET_SPINE_VERSION,
            validations: [current_record, existing_record, new_record],
        },
    })
}

fn bind_target(
    analysis: &CompletedPhase0Analysis,
    role: RuntimeValidationRole,
    target: ValidatedTarget,
) -> Result<BoundTarget, RuntimeValidationsError> {
    let native = validate_validated_runtime_bundle(&target.bundle)
        .map_err(|source| RuntimeValidationsError::Native { role, source })?;
    if native.spine_version() != TARGET_SPINE_VERSION {
        return Err(RuntimeValidationsError::VersionMismatch { role });
    }

    let analysis_role = role.analysis_role();
    let expected = analysis.source_identity(analysis_role);
    if native.json_sha256() != expected.sha256()
        || target.bundle.json_bytes() != analysis.raw_document(analysis_role)
    {
        return Err(RuntimeValidationsError::JsonIdentityMismatch { role });
    }

    let assets = RuntimeAssetIdentity {
        atlas: RuntimeFileIdentity {
            path: native.atlas_path().to_owned(),
            byte_length: native.atlas_byte_length(),
            sha256: native.atlas_sha256().to_owned(),
        },
        pages: native
            .pages()
            .map(|(path, byte_length, sha256)| RuntimeFileIdentity {
                path: path.to_owned(),
                byte_length,
                sha256: sha256.to_owned(),
            })
            .collect(),
    };
    let json_path = PathBuf::from(native.json_path());
    Ok(BoundTarget {
        record: RuntimeValidationRecord {
            role,
            analysis_json: AnalysisJsonIdentity {
                role,
                sha256: expected.sha256().to_owned(),
            },
            workspace_files: target.bindings,
            native,
        },
        assets,
        json_path,
    })
}

#[cfg(test)]
mod tests;

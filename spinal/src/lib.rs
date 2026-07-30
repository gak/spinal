#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod asset;
mod diagnostic;
mod id;
mod math;
mod skeleton;

pub use asset::{AnimationRef, BoneRef, IkConstraintRef, SkeletonAsset, SkinRef, SlotRef};
pub use diagnostic::{Diagnostic, DiagnosticCode, DiagnosticScope, DiagnosticSeverity};
pub use glam;
pub use id::{
    AnimationId, AtlasPageId, AtlasRegionId, AttachmentId, BoneId, ConstraintId, IdError,
    IdErrorKind, IkConstraintId, SkinId, SlotId,
};
pub use math::{Angle, BoneTransform, InvalidAngle, InvalidBoneTransform, InvalidMix, Mix, Shear};
pub use skeleton::{BonePoseRef, Skeleton};

/// The Spine major version targeted by the first Spinal wire-format loader.
pub const TARGET_SPINE_MAJOR: u16 = 4;

/// The Spine minor version targeted by the first Spinal wire-format loader.
pub const TARGET_SPINE_MINOR: u16 = 3;

/// The exact Spine editor version targeted by Spinal's initial conformance
/// suite.
///
/// This is a target, not a compatibility claim. The suite remains provisional
/// until editor-generated fixtures from this exact version are recorded and
/// pass.
pub const TARGET_SPINE_VERSION: &str = "4.3.23";

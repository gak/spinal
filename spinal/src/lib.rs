#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod animation;
mod asset;
mod atlas;
mod diagnostic;
mod geometry;
mod id;
mod json;
mod load;
mod math;
mod skeleton;

pub use animation::PlaybackMode;
pub use asset::{
    AnimationRef, AtlasPageRef, AtlasPropertyRef, AtlasRegionRef, AttachmentKind, AttachmentRef,
    BendDirection, BoneRef, ConstraintRef, EventDefinitionRef, IkConstraintRef,
    RegionAttachmentRef, SkeletonAsset, SkinRef, SlotBlendMode, SlotRef,
};
pub use diagnostic::{Diagnostic, DiagnosticCode, DiagnosticScope, DiagnosticSeverity};
pub use geometry::{
    AlphaEncoding, AtlasRotation, InvalidRgba, PixelRect, PixelSize, Rgba, Rgba8, TextureFilter,
    TextureFormat, Trim, WrapMode,
};
pub use glam;
pub use id::{
    AnimationId, AtlasPageId, AtlasRegionId, AttachmentId, BoneId, ConstraintId, EventId, IdError,
    IdErrorKind, IkConstraintId, SkinId, SlotId,
};
pub use load::{LoadDocument, LoadError, LoadErrorKind, LoadReport, SourceLocation, load_json};
pub use math::{Angle, BoneTransform, InvalidAngle, InvalidBoneTransform, InvalidMix, Mix, Shear};
pub use skeleton::{BonePoseRef, IkConstraintPoseRef, Skeleton, SlotPoseRef};

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

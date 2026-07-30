#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod animation;
mod asset;
mod atlas;
mod diagnostic;
mod draw;
mod frame;
mod geometry;
mod id;
mod json;
mod load;
mod math;
mod player;
mod pose;
mod skeleton;
mod world;

pub use animation::PlaybackMode;
pub use asset::{
    AnimationRef, AtlasPageRef, AtlasPropertyRef, AtlasRegionRef, AttachmentKind, AttachmentRef,
    BendDirection, BoneRef, ConstraintRef, EventDefinitionRef, IkConstraintRef,
    RegionAttachmentRef, SkeletonAsset, SkinRef, SlotBlendMode, SlotRef,
};
pub use diagnostic::{Diagnostic, DiagnosticCode, DiagnosticScope, DiagnosticSeverity};
pub use draw::{DrawItemRef, RegionDrawItemRef};
pub use frame::{
    EditablePose, IkSolveIssue, IkSolveStatus, IkTargetReach, PoseEditor, SolvedBoneRef,
    SolvedFrame,
};
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
pub use player::{
    AnimationEvent, AnimationPlayer, Crossfade, DiscreteSwitches, EventSink, MixCurve, PlayOptions,
    PlayOutcome, PlaybackId, PlayerError, PlayerStatus, Transition, UpdateReport,
};
pub use skeleton::{BonePoseRef, IkConstraintPoseRef, Skeleton, SlotPoseRef};
pub use world::{InvalidWorldTransform, WorldTransform};

/// The Spine major version targeted by the first Spinal wire-format loader.
pub const TARGET_SPINE_MAJOR: u16 = 4;

/// The Spine minor version targeted by the first Spinal wire-format loader.
pub const TARGET_SPINE_MINOR: u16 = 3;

/// The exact Spine editor version targeted by Spinal's initial conformance
/// suite.
///
/// External exact-version Spineboy fixtures pass, but complete
/// supported-profile conformance remains provisional until project-owned
/// fixtures cover every supported feature and export setting.
pub const TARGET_SPINE_VERSION: &str = "4.3.23";

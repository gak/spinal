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
mod mesh;
mod mixer;
mod player;
mod pose;
mod runtime_bundle;
mod semantic_frame;
mod skeleton;
mod world;

pub use animation::{OverrideSupport, PlaybackMode, PropertyKey, TransformMixChannel};
pub use asset::{
    AnimationRef, AtlasPageRef, AtlasPropertyRef, AtlasRegionRef, AttachmentKind, AttachmentRef,
    BendDirection, BoneRef, ConstraintRef, EventDefinitionRef, IkConstraintRef,
    OverrideCompatibility, RegionAttachmentRef, SkeletonAsset, SkinRef, SlotBlendMode, SlotRef,
    TransformConstraintRef, TransformConstraintSetupPose,
};
pub use diagnostic::{Diagnostic, DiagnosticCode, DiagnosticScope, DiagnosticSeverity};
pub use draw::{DrawItemRef, MeshDrawItemRef, MeshUvIter, RegionDrawItemRef};
pub use frame::{
    ControlTargetError, EditablePose, IkSolveIssue, IkSolveStatus, IkTargetReach, PoseEditor,
    PoseTargets, SolvedBoneRef, SolvedFrame, TransformConstraintSolveStatus, TransformSolveIssue,
};
pub use geometry::{
    AlphaEncoding, AtlasRotation, InvalidRgba, PixelRect, PixelSize, Rgba, Rgba8, TextureFilter,
    TextureFormat, Trim, WrapMode,
};
pub use glam;
pub use id::{
    AnimationId, AtlasPageId, AtlasRegionId, AttachmentId, BoneId, ConstraintId, EventId, IdError,
    IdErrorKind, IkConstraintId, SkinId, SlotId, TransformConstraintId,
};
pub use load::{LoadDocument, LoadError, LoadErrorKind, LoadReport, SourceLocation, load_json};
pub use math::{
    Angle, BoneTransform, InvalidAngle, InvalidBoneTransform, InvalidMix, InvalidTransformMix, Mix,
    Shear, TransformMix,
};
pub use mesh::{MeshAttachmentRef, MeshInfluenceRef, MeshVertexRef};
pub use mixer::{
    AnimationMixer, BaseTrackMut, BaseTrackRef, InvalidPlaybackSpeed, TrackAnimationEvent,
    TrackError, TrackErrorKind, TrackEventSink, TrackId, TrackMut, TrackOptions,
    TrackPropertyIssue, TrackRef, TrackUpdateReport, WeightFade,
};
pub use player::{
    AnimationEvent, AnimationPlayer, Crossfade, DiscreteSwitches, EventSink, MixCurve, PlayOptions,
    PlayOutcome, PlaybackId, PlayerError, PlayerStatus, RotationPath, Transition, UpdateReport,
};
pub use runtime_bundle::{
    MAX_RUNTIME_BUNDLE_BYTES, MAX_RUNTIME_DECODED_TEXTURE_BYTES, MAX_RUNTIME_FILE_COUNT,
    MAX_RUNTIME_MANIFEST_BYTES, RuntimeBundleError, RuntimeBundleFile, RuntimeBundleManifest,
    ValidatedRuntimeBundle, parse_runtime_bundle_sha256,
    validate_runtime_bundle_location_reference,
};
pub use semantic_frame::{
    SEMANTIC_FRAME_FORMAT_VERSION, SemanticAtlasRegion, SemanticAttachment, SemanticBlendMode,
    SemanticBone, SemanticDiagnostic, SemanticDiagnosticCode, SemanticDiagnosticScope,
    SemanticDiagnosticSeverity, SemanticDraw, SemanticDrawKind, SemanticFrame, SemanticFrameError,
    SemanticIkConstraint, SemanticIkSolveIssue, SemanticIkTargetReach, SemanticLocalTransform,
    SemanticSlot, SemanticTransformConstraint, SemanticTransformSolveIssue, SemanticWorldTransform,
};
pub use skeleton::{
    BonePoseRef, IkConstraintPoseRef, Skeleton, SlotPoseRef, TransformConstraintPoseRef,
};
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

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod asset;
mod components;
mod plugin;
mod runtime;

#[cfg(feature = "render")]
mod render;

/// The renderer-independent runtime core used by this adapter.
///
/// Re-exporting the exact core dependency lets Bevy-only applications use
/// types appearing in adapter signatures without declaring a second,
/// potentially incompatible `spinal` dependency.
pub use spinal;

pub use asset::{
    SpinalAsset, SpinalAssetLoader, SpinalAssetLoaderError, SpinalAssetLoaderSettings,
    SpinalAtlasPage,
};
pub use components::{
    BoneOverride, InvalidControlTargetPosition, InvalidPlaybackSpeed, SpinalAnimationTracks,
    SpinalAnimator, SpinalAppearance, SpinalControlTargets, SpinalInstance, SpinalInstanceState,
    SpinalPlaybackState, SpinalPoseOverrides, SpinalSemanticCapture, SpinalSkinLayers,
    SpinalTrackIntentRef, SpinalTrackState, SpinalTrackStates, TrackReorderError,
    WorldToSkeletonPositionError,
};
pub use plugin::{SpinalPlugin, SpinalSet};
pub use runtime::{SpinalAnimationEvent, SpinalIssue, SpinalIssueKind, SpinalRuntimeConfig};

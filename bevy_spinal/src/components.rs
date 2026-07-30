use std::{fmt, time::Duration};

use bevy::{
    asset::Handle,
    camera::visibility::{self, Visibility, VisibilityClass},
    color::Color,
    ecs::component::Component,
    transform::components::Transform,
};
use spinal::{BoneTransform, PlaybackMode, Transition};
use thiserror::Error;

use crate::SpinalAsset;

/// Selects one loaded Spinal asset for an ECS entity.
///
/// Adding this component also inserts the public control and observation
/// components needed by [`crate::SpinalPlugin`].
#[derive(Clone, Component, Debug)]
#[require(
    Transform,
    Visibility,
    VisibilityClass,
    SpinalAnimator,
    SpinalAppearance,
    SpinalSkinLayers,
    SpinalPoseOverrides,
    SpinalInstanceState,
    SpinalPlaybackState
)]
#[component(on_add = visibility::add_visibility_class::<SpinalInstance>)]
pub struct SpinalInstance {
    asset: Handle<SpinalAsset>,
}

impl SpinalInstance {
    /// Creates an instance backed by `asset`.
    #[must_use]
    pub const fn new(asset: Handle<SpinalAsset>) -> Self {
        Self { asset }
    }

    /// Returns the selected Bevy asset handle.
    #[must_use]
    pub const fn asset(&self) -> &Handle<SpinalAsset> {
        &self.asset
    }

    /// Replaces the selected asset.
    ///
    /// The plugin rebuilds the private runtime on its next update and
    /// reapplies animation, skin, and pose intent by stable name.
    pub fn set_asset(&mut self, asset: Handle<SpinalAsset>) {
        self.asset = asset;
    }
}

/// Per-instance visual modulation and local-space facing.
///
/// Facing is applied to solved skeleton geometry before the entity's
/// [`Transform`]. This avoids using a negative parent scale, which is useful
/// when the parent also owns gameplay rotation, squash, or children.
///
/// The modulation is additional to colors authored in Spine. Its declared
/// Bevy color space is respected and both colors are composed in linear space.
#[derive(Clone, Component, Debug, PartialEq)]
pub struct SpinalAppearance {
    modulation: Color,
    flip_x: bool,
    flip_y: bool,
}

impl SpinalAppearance {
    /// Returns the additional per-instance color modulation.
    #[must_use]
    pub const fn modulation(&self) -> Color {
        self.modulation
    }

    /// Replaces the additional per-instance color modulation.
    pub fn set_modulation(&mut self, modulation: Color) {
        self.modulation = modulation;
    }

    /// Returns a copy with a new color modulation.
    #[must_use]
    pub const fn with_modulation(mut self, modulation: Color) -> Self {
        self.modulation = modulation;
        self
    }

    /// Returns whether solved geometry is mirrored across its local Y axis.
    #[must_use]
    pub const fn flip_x(&self) -> bool {
        self.flip_x
    }

    /// Sets whether solved geometry is mirrored across its local Y axis.
    pub fn set_flip_x(&mut self, flip_x: bool) {
        self.flip_x = flip_x;
    }

    /// Returns a copy with horizontal local-space facing selected.
    #[must_use]
    pub const fn with_flip_x(mut self, flip_x: bool) -> Self {
        self.flip_x = flip_x;
        self
    }

    /// Returns whether solved geometry is mirrored across its local X axis.
    #[must_use]
    pub const fn flip_y(&self) -> bool {
        self.flip_y
    }

    /// Sets whether solved geometry is mirrored across its local X axis.
    pub fn set_flip_y(&mut self, flip_y: bool) {
        self.flip_y = flip_y;
    }

    /// Returns a copy with vertical local-space facing selected.
    #[must_use]
    pub const fn with_flip_y(mut self, flip_y: bool) -> Self {
        self.flip_y = flip_y;
        self
    }
}

impl Default for SpinalAppearance {
    fn default() -> Self {
        Self {
            modulation: Color::WHITE,
            flip_x: false,
            flip_y: false,
        }
    }
}

/// Declarative one-track playback intent.
///
/// Methods bump an internal generation, so requesting the same animation
/// again is an explicit restart rather than an ambiguous no-op.
#[derive(Clone, Component, Debug, PartialEq)]
pub struct SpinalAnimator {
    desired: Option<DesiredPlayback>,
    stop_transition: Transition,
    speed: f32,
    paused: bool,
    revision: u64,
}

#[derive(Clone, Debug, PartialEq)]
struct DesiredPlayback {
    animation: Box<str>,
    mode: PlaybackMode,
    transition: Transition,
}

impl SpinalAnimator {
    /// Creates an animator that loops `animation` immediately.
    #[must_use]
    pub fn looping(animation: impl Into<Box<str>>) -> Self {
        Self::playing(animation, PlaybackMode::Loop)
    }

    /// Creates an animator that plays `animation` once immediately.
    #[must_use]
    pub fn once(animation: impl Into<Box<str>>) -> Self {
        Self::playing(animation, PlaybackMode::Once)
    }

    /// Creates an animator with an immediate initial playback.
    #[must_use]
    pub fn playing(animation: impl Into<Box<str>>, mode: PlaybackMode) -> Self {
        Self {
            desired: Some(DesiredPlayback {
                animation: animation.into(),
                mode,
                transition: Transition::Immediate,
            }),
            stop_transition: Transition::Immediate,
            speed: 1.0,
            paused: false,
            revision: 1,
        }
    }

    /// Requests a playback, including the transition from the current pose.
    pub fn play(
        &mut self,
        animation: impl Into<Box<str>>,
        mode: PlaybackMode,
        transition: Transition,
    ) {
        self.desired = Some(DesiredPlayback {
            animation: animation.into(),
            mode,
            transition,
        });
        self.bump_revision();
    }

    /// Requests a transition to setup pose.
    pub fn stop(&mut self, transition: Transition) {
        self.desired = None;
        self.bump_revision();
        self.stop_transition = transition;
    }

    /// Restarts the current desired animation, if any.
    pub fn restart(&mut self) {
        if self.desired.is_some() {
            self.bump_revision();
        }
    }

    /// Pauses or resumes clock advancement without changing playback intent.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Sets the nonnegative finite playback speed.
    pub fn set_speed(&mut self, speed: f32) -> Result<(), InvalidPlaybackSpeed> {
        if !speed.is_finite() || speed < 0.0 {
            return Err(InvalidPlaybackSpeed { speed });
        }
        self.speed = speed;
        Ok(())
    }

    /// Returns the desired animation name, or `None` for setup pose.
    #[must_use]
    pub fn animation(&self) -> Option<&str> {
        self.desired
            .as_ref()
            .map(|playback| playback.animation.as_ref())
    }

    /// Returns the desired playback mode.
    #[must_use]
    pub fn mode(&self) -> Option<PlaybackMode> {
        self.desired.as_ref().map(|playback| playback.mode)
    }

    /// Returns the requested transition.
    #[must_use]
    pub fn transition(&self) -> Transition {
        self.desired
            .as_ref()
            .map_or(self.stop_transition, |playback| playback.transition)
    }

    /// Returns whether clock advancement is paused.
    #[must_use]
    pub const fn is_paused(&self) -> bool {
        self.paused
    }

    /// Returns the playback speed multiplier.
    #[must_use]
    pub const fn speed(&self) -> f32 {
        self.speed
    }

    /// Returns the request generation.
    ///
    /// Integrations normally do not need this; it is exposed for deterministic
    /// diagnostics and tests.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
}

impl Default for SpinalAnimator {
    fn default() -> Self {
        Self {
            desired: None,
            speed: 1.0,
            paused: false,
            revision: 0,
            stop_transition: Transition::Immediate,
        }
    }
}

/// An invalid animation speed.
#[derive(Clone, Copy, Debug, Error, PartialEq)]
#[error("playback speed must be finite and nonnegative, got {speed}")]
pub struct InvalidPlaybackSpeed {
    speed: f32,
}

impl InvalidPlaybackSpeed {
    /// Returns the rejected speed.
    #[must_use]
    pub const fn speed(self) -> f32 {
        self.speed
    }
}

/// Ordered attachment-only skin composition, from low to high priority.
#[derive(Clone, Component, Debug, Default, Eq, PartialEq)]
pub struct SpinalSkinLayers {
    names: Vec<Box<str>>,
}

impl SpinalSkinLayers {
    /// Creates an ordered skin composition.
    #[must_use]
    pub fn new<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<Box<str>>,
    {
        Self {
            names: names.into_iter().map(Into::into).collect(),
        }
    }

    /// Replaces all layers while preserving caller order.
    pub fn set<I, S>(&mut self, names: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<Box<str>>,
    {
        self.names = names.into_iter().map(Into::into).collect();
    }

    /// Iterates layer names from low to high priority.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &str> + ExactSizeIterator {
        self.names.iter().map(AsRef::as_ref)
    }

    /// Returns whether no additional skin layer is active.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

/// One stable-name procedural replacement applied before constraint solving.
#[derive(Clone, Debug, PartialEq)]
pub struct BoneOverride {
    bone: Box<str>,
    transform: BoneTransform,
}

impl BoneOverride {
    /// Creates a full local-transform replacement for `bone`.
    #[must_use]
    pub fn new(bone: impl Into<Box<str>>, transform: BoneTransform) -> Self {
        Self {
            bone: bone.into(),
            transform,
        }
    }

    /// Returns the stable bone name.
    #[must_use]
    pub fn bone(&self) -> &str {
        &self.bone
    }

    /// Returns the replacement local transform.
    #[must_use]
    pub const fn transform(&self) -> BoneTransform {
        self.transform
    }
}

/// Procedural local-bone replacements applied after animation and before IK.
#[derive(Clone, Component, Debug, Default, PartialEq)]
pub struct SpinalPoseOverrides {
    overrides: Vec<BoneOverride>,
}

impl SpinalPoseOverrides {
    /// Inserts or replaces one override by stable bone name.
    pub fn set(&mut self, replacement: BoneOverride) {
        if let Some(existing) = self
            .overrides
            .iter_mut()
            .find(|existing| existing.bone == replacement.bone)
        {
            *existing = replacement;
        } else {
            self.overrides.push(replacement);
        }
    }

    /// Removes and returns one override.
    pub fn remove(&mut self, bone: &str) -> Option<BoneOverride> {
        let index = self
            .overrides
            .iter()
            .position(|replacement| replacement.bone() == bone)?;
        Some(self.overrides.remove(index))
    }

    /// Returns one override by stable bone name.
    #[must_use]
    pub fn get(&self, bone: &str) -> Option<&BoneOverride> {
        self.overrides
            .iter()
            .find(|replacement| replacement.bone() == bone)
    }

    /// Iterates overrides in deterministic insertion order.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &BoneOverride> + ExactSizeIterator {
        self.overrides.iter()
    }

    /// Removes every override.
    pub fn clear(&mut self) {
        self.overrides.clear();
    }
}

/// Public lifecycle state for one ECS skeleton instance.
#[derive(Clone, Component, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum SpinalInstanceState {
    /// The compound asset has not produced a usable value yet.
    #[default]
    Loading,
    /// The current frame uses the supported profile without active fallbacks.
    Ready,
    /// The current frame is visible but has at least one active degradation.
    Degraded,
    /// The compound asset failed before a usable runtime could be created.
    Failed,
}

/// Public one-track playback observation.
#[derive(Clone, Component, Debug, Default, PartialEq)]
#[non_exhaustive]
pub enum SpinalPlaybackState {
    /// No animation or setup transition is active.
    #[default]
    Idle,
    /// An animation is active or holding its final pose.
    Playing {
        /// Player-local playback identifier.
        playback: u64,
        /// Stable animation name.
        animation: Box<str>,
        /// Requested end behavior.
        mode: PlaybackMode,
        /// Current animation-local position.
        position: Duration,
        /// Current zero-based loop index.
        loop_index: u128,
        /// Whether a once playback is holding its endpoint.
        complete: bool,
        /// Current eased crossfade influence, when transitioning.
        transition_mix: Option<spinal::Mix>,
    },
    /// The player is transitioning back to setup pose.
    Stopping {
        /// Current eased crossfade influence.
        transition_mix: Option<spinal::Mix>,
    },
}

impl SpinalPlaybackState {
    /// Returns whether the player has no active animation or transition.
    #[must_use]
    pub const fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }

    /// Returns whether the player is transitioning back to setup pose.
    #[must_use]
    pub const fn is_stopping(&self) -> bool {
        matches!(self, Self::Stopping { .. })
    }

    /// Returns the player-local playback identifier.
    #[must_use]
    pub const fn playback(&self) -> Option<u64> {
        match self {
            Self::Playing { playback, .. } => Some(*playback),
            Self::Idle | Self::Stopping { .. } => None,
        }
    }

    /// Returns the active animation name.
    #[must_use]
    pub fn animation(&self) -> Option<&str> {
        match self {
            Self::Playing { animation, .. } => Some(animation),
            Self::Idle | Self::Stopping { .. } => None,
        }
    }

    /// Returns the active playback mode.
    #[must_use]
    pub const fn mode(&self) -> Option<PlaybackMode> {
        match self {
            Self::Playing { mode, .. } => Some(*mode),
            Self::Idle | Self::Stopping { .. } => None,
        }
    }

    /// Returns the animation-local position.
    #[must_use]
    pub const fn position(&self) -> Option<Duration> {
        match self {
            Self::Playing { position, .. } => Some(*position),
            Self::Idle | Self::Stopping { .. } => None,
        }
    }

    /// Returns the zero-based loop index.
    #[must_use]
    pub const fn loop_index(&self) -> Option<u128> {
        match self {
            Self::Playing { loop_index, .. } => Some(*loop_index),
            Self::Idle | Self::Stopping { .. } => None,
        }
    }

    /// Returns the current eased transition influence.
    #[must_use]
    pub const fn transition_mix(&self) -> Option<spinal::Mix> {
        match self {
            Self::Playing { transition_mix, .. } | Self::Stopping { transition_mix } => {
                *transition_mix
            }
            Self::Idle => None,
        }
    }

    /// Returns whether a once playback is holding its endpoint.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        matches!(self, Self::Playing { complete: true, .. })
    }
}

impl fmt::Display for SpinalInstanceState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Loading => formatter.write_str("loading"),
            Self::Ready => formatter.write_str("ready"),
            Self::Degraded => formatter.write_str("degraded"),
            Self::Failed => formatter.write_str("failed"),
        }
    }
}

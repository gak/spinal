use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use bevy::{
    asset::Handle,
    camera::visibility::{self, Visibility, VisibilityClass},
    color::Color,
    ecs::component::Component,
    math::Vec2,
    transform::components::{GlobalTransform, Transform},
};
use spinal::{BoneTransform, Mix, PlaybackMode, Transition, WeightFade};
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
    SpinalAnimationTracks,
    SpinalAppearance,
    SpinalSkinLayers,
    SpinalPoseOverrides,
    SpinalControlTargets,
    SpinalInstanceState,
    SpinalPlaybackState,
    SpinalTrackStates
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

    /// Converts a Bevy world-space point into unflipped skeleton space.
    ///
    /// This inverts the entity's two-dimensional [`GlobalTransform`] and then
    /// removes this appearance's local facing. The result can be passed
    /// directly to [`SpinalControlTargets::set_skeleton_position`].
    ///
    /// The point is interpreted on the skeleton's world Z plane. Nonfinite
    /// input and nonfinite or singular entity transforms are rejected.
    pub fn world_to_skeleton_position(
        &self,
        transform: &GlobalTransform,
        world_position: Vec2,
    ) -> Result<Vec2, WorldToSkeletonPositionError> {
        if !world_position.is_finite() {
            return Err(WorldToSkeletonPositionError::NonFiniteWorldPosition);
        }
        let matrix = transform.to_matrix();
        let determinant = matrix.determinant();
        if !matrix.is_finite() || !determinant.is_finite() || determinant.abs() <= f32::EPSILON {
            return Err(WorldToSkeletonPositionError::InvalidEntityTransform);
        }
        let point = matrix
            .inverse()
            .transform_point3(world_position.extend(transform.translation().z))
            .truncate();
        if !point.is_finite() {
            return Err(WorldToSkeletonPositionError::InvalidEntityTransform);
        }
        Ok(Vec2::new(
            if self.flip_x { -point.x } else { point.x },
            if self.flip_y { -point.y } else { point.y },
        ))
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

/// Failure to convert a Bevy world point into Spinal skeleton space.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum WorldToSkeletonPositionError {
    /// The requested world-space point contains NaN or infinity.
    #[error("world position must be finite")]
    NonFiniteWorldPosition,
    /// The entity transform contains nonfinite values or cannot be inverted.
    #[error("entity transform must be finite and invertible")]
    InvalidEntityTransform,
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
    seek_position: Option<Duration>,
    seek_revision: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DesiredPlayback {
    pub(crate) animation: Box<str>,
    pub(crate) mode: PlaybackMode,
    pub(crate) transition: Transition,
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
            seek_position: None,
            seek_revision: 0,
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
        self.clear_seek();
        self.bump_revision();
    }

    /// Requests a transition to setup pose.
    pub fn stop(&mut self, transition: Transition) {
        self.desired = None;
        self.clear_seek();
        self.bump_revision();
        self.stop_transition = transition;
    }

    /// Restarts the current desired animation, if any.
    pub fn restart(&mut self) {
        if self.desired.is_some() {
            self.clear_seek();
            self.bump_revision();
        }
    }

    /// Requests an absolute seek within the current desired playback.
    ///
    /// Seeking is independent from play and restart intent: applying it does
    /// not create a new playback identity. Repeating the same position is
    /// still a fresh command. The runtime suppresses crossed authored events
    /// and presents the requested position before resuming clock advancement.
    pub fn seek_to(&mut self, elapsed: Duration) {
        self.seek_position = Some(elapsed);
        self.bump_seek_revision();
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

    /// Returns the latest requested absolute seek position, when retained.
    #[must_use]
    pub const fn seek_position(&self) -> Option<Duration> {
        self.seek_position
    }

    /// Returns the seek-command generation independently from play intent.
    ///
    /// Integrations normally do not need this; it is exposed for deterministic
    /// diagnostics and tests.
    #[must_use]
    pub const fn seek_revision(&self) -> u64 {
        self.seek_revision
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    fn clear_seek(&mut self) {
        self.seek_position = None;
        self.bump_seek_revision();
    }

    fn bump_seek_revision(&mut self) {
        self.seek_revision = self.seek_revision.wrapping_add(1);
    }
}

impl Default for SpinalAnimator {
    fn default() -> Self {
        Self {
            desired: None,
            speed: 1.0,
            paused: false,
            revision: 0,
            seek_position: None,
            seek_revision: 0,
            stop_transition: Transition::Immediate,
        }
    }
}

/// Declarative ordered override-track intent keyed by stable application names.
///
/// `play`, `restart`, `stop`, and `fade_weight` are commands. Paused, speed,
/// and constant-weight setters are idempotent state changes.
///
/// Mutating an existing component preserves live track continuity. Inserting
/// an independently constructed component declares fresh tracks; cloning a
/// component preserves its declaration lineage. Equality includes that
/// lineage, so equality-based Bevy setters do not suppress a fresh
/// declaration.
#[derive(Clone, Component, Debug, Default)]
pub struct SpinalAnimationTracks {
    tracks: Vec<TrackIntent>,
    // Clones preserve one declaration lineage. An independently constructed
    // component receives a distinct allocation-backed namespace.
    namespace: Arc<TrackNamespace>,
}

#[derive(Debug, Default)]
pub(crate) struct TrackNamespace(AtomicU64);

impl TrackNamespace {
    fn issue_generation(&self) -> u64 {
        self.0
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map(|previous| previous + 1)
            .expect("track declaration generation capacity exhausted")
    }
}

impl PartialEq for SpinalAnimationTracks {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.namespace, &other.namespace) && self.tracks == other.tracks
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TrackIntent {
    pub(crate) key: Box<str>,
    // Stable for this declaration, but replaced when a key is removed and recreated.
    pub(crate) incarnation: u64,
    pub(crate) desired: Option<DesiredPlayback>,
    pub(crate) stop_transition: Transition,
    pub(crate) speed: f32,
    pub(crate) paused: bool,
    pub(crate) weight: Mix,
    pub(crate) weight_fade: Option<WeightFade>,
    // Reconstruction fallback when no prior runtime can provide its presented weight.
    pub(crate) weight_fade_source: Option<Mix>,
    pub(crate) play_revision: u64,
    pub(crate) weight_revision: u64,
}

impl SpinalAnimationTracks {
    /// Requests an animation on a stable named override track.
    pub fn play(
        &mut self,
        track: impl AsRef<str>,
        animation: impl Into<Box<str>>,
        mode: PlaybackMode,
        transition: Transition,
    ) {
        let index = self.ensure_index(track.as_ref());
        let revision = self.issue_generation();
        let intent = &mut self.tracks[index];
        intent.desired = Some(DesiredPlayback {
            animation: animation.into(),
            mode,
            transition,
        });
        intent.play_revision = revision;
    }

    /// Requests a transition from one named track to no contribution.
    pub fn stop(&mut self, track: impl AsRef<str>, transition: Transition) {
        let index = self.ensure_index(track.as_ref());
        let revision = self.issue_generation();
        let intent = &mut self.tracks[index];
        intent.desired = None;
        intent.stop_transition = transition;
        intent.play_revision = revision;
    }

    /// Restarts one named track's desired animation, if present.
    pub fn restart(&mut self, track: &str) {
        if let Some(index) = self.find_index(track)
            && self.tracks[index].desired.is_some()
        {
            let revision = self.issue_generation();
            self.tracks[index].play_revision = revision;
        }
    }

    /// Removes one named override track and its intent.
    ///
    /// Reusing the key later creates a fresh track incarnation, even when the
    /// removal and recreation occur before the next Bevy update.
    pub fn remove(&mut self, track: &str) -> bool {
        let Some(index) = self
            .tracks
            .iter()
            .position(|intent| intent.key.as_ref() == track)
        else {
            return false;
        };
        self.tracks.remove(index);
        true
    }

    /// Moves one existing named track to a zero-based low-to-high priority
    /// index without restarting its playback.
    pub fn move_to(&mut self, track: &str, index: usize) -> Result<(), TrackReorderError> {
        if index >= self.tracks.len() {
            return Err(TrackReorderError::IndexOutOfBounds {
                index,
                len: self.tracks.len(),
            });
        }
        let current = self
            .tracks
            .iter()
            .position(|intent| intent.key.as_ref() == track)
            .ok_or(TrackReorderError::UnknownTrack)?;
        if current != index {
            let intent = self.tracks.remove(current);
            self.tracks.insert(index, intent);
        }
        Ok(())
    }

    /// Pauses or resumes one named animation clock.
    pub fn set_paused(&mut self, track: impl AsRef<str>, paused: bool) {
        let index = self.ensure_index(track.as_ref());
        self.tracks[index].paused = paused;
    }

    /// Sets one named track's nonnegative finite animation speed.
    pub fn set_speed(
        &mut self,
        track: impl AsRef<str>,
        speed: f32,
    ) -> Result<(), InvalidPlaybackSpeed> {
        if !speed.is_finite() || speed < 0.0 {
            return Err(InvalidPlaybackSpeed { speed });
        }
        let index = self.ensure_index(track.as_ref());
        self.tracks[index].speed = speed;
        Ok(())
    }

    /// Sets one named track's constant weight and cancels its pending fade.
    pub fn set_weight(&mut self, track: impl AsRef<str>, weight: Mix) {
        let index = self.ensure_index(track.as_ref());
        let intent = &mut self.tracks[index];
        intent.weight = weight;
        intent.weight_fade = None;
        intent.weight_fade_source = None;
    }

    /// Requests a wall-clock fade to one named track's target weight.
    pub fn fade_weight(&mut self, track: impl AsRef<str>, target: Mix, fade: WeightFade) {
        let index = self.ensure_index(track.as_ref());
        let revision = self.issue_generation();
        let intent = &mut self.tracks[index];
        let source = intent.weight_fade_source.unwrap_or(intent.weight);
        intent.weight = target;
        intent.weight_fade = Some(fade);
        intent.weight_fade_source = Some(source);
        intent.weight_revision = revision;
    }

    /// Iterates stable track keys from low to high priority.
    pub fn keys(&self) -> impl DoubleEndedIterator<Item = &str> + ExactSizeIterator {
        self.tracks.iter().map(|intent| intent.key.as_ref())
    }

    /// Iterates immutable declared intent from low to high track priority.
    pub fn iter(
        &self,
    ) -> impl DoubleEndedIterator<Item = SpinalTrackIntentRef<'_>> + ExactSizeIterator {
        self.tracks
            .iter()
            .map(|intent| SpinalTrackIntentRef { intent })
    }

    /// Looks up immutable declared intent by stable application key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<SpinalTrackIntentRef<'_>> {
        self.tracks
            .iter()
            .find(|intent| intent.key.as_ref() == key)
            .map(|intent| SpinalTrackIntentRef { intent })
    }

    /// Returns the number of declared override tracks.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.tracks.len()
    }

    /// Returns whether no override tracks are declared.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    /// Removes every named override track and its intent.
    ///
    /// Reusing any cleared key later creates a fresh track incarnation.
    pub fn clear(&mut self) {
        self.tracks.clear();
    }

    pub(crate) fn intents(&self) -> &[TrackIntent] {
        &self.tracks
    }

    pub(crate) fn namespace(&self) -> &Arc<TrackNamespace> {
        &self.namespace
    }

    fn ensure_index(&mut self, key: &str) -> usize {
        if let Some(index) = self.find_index(key) {
            return index;
        }
        let incarnation = self.issue_generation();
        self.tracks.push(TrackIntent {
            key: key.into(),
            incarnation,
            desired: None,
            stop_transition: Transition::Immediate,
            speed: 1.0,
            paused: false,
            weight: Mix::ONE,
            weight_fade: None,
            weight_fade_source: None,
            play_revision: 0,
            weight_revision: 0,
        });
        self.tracks.len() - 1
    }

    fn find_index(&self, key: &str) -> Option<usize> {
        self.tracks
            .iter()
            .position(|intent| intent.key.as_ref() == key)
    }

    fn issue_generation(&mut self) -> u64 {
        self.namespace.issue_generation()
    }
}

/// Borrowed immutable declarative intent for one named override track.
#[derive(Clone, Copy, Debug)]
pub struct SpinalTrackIntentRef<'a> {
    intent: &'a TrackIntent,
}

impl<'a> SpinalTrackIntentRef<'a> {
    /// Returns the stable application key.
    #[must_use]
    pub fn key(self) -> &'a str {
        &self.intent.key
    }

    /// Returns the requested animation name, or `None` when stopped.
    #[must_use]
    pub fn animation(self) -> Option<&'a str> {
        self.intent
            .desired
            .as_ref()
            .map(|desired| desired.animation.as_ref())
    }

    /// Returns the requested playback mode, or `None` when stopped.
    #[must_use]
    pub fn mode(self) -> Option<PlaybackMode> {
        self.intent.desired.as_ref().map(|desired| desired.mode)
    }

    /// Returns the transition requested by the latest play command.
    #[must_use]
    pub fn play_transition(self) -> Option<Transition> {
        self.intent
            .desired
            .as_ref()
            .map(|desired| desired.transition)
    }

    /// Returns the transition requested by the latest stop command.
    #[must_use]
    pub const fn stop_transition(self) -> Transition {
        self.intent.stop_transition
    }

    /// Returns whether the animation clock is declared paused.
    #[must_use]
    pub const fn is_paused(self) -> bool {
        self.intent.paused
    }

    /// Returns the declared animation-clock speed.
    #[must_use]
    pub const fn speed(self) -> f32 {
        self.intent.speed
    }

    /// Returns the declared target weight.
    #[must_use]
    pub const fn target_weight(self) -> Mix {
        self.intent.weight
    }

    /// Returns the latest requested weight fade, if any.
    #[must_use]
    pub const fn weight_fade(self) -> Option<WeightFade> {
        self.intent.weight_fade
    }
}

/// A failure to reorder a declarative named override track.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum TrackReorderError {
    /// The stable application key is not declared.
    #[error("the named override track does not exist")]
    UnknownTrack,
    /// The requested priority index is outside the declared track set.
    #[error("track priority index {index} is out of bounds for {len} tracks")]
    IndexOutOfBounds {
        /// The rejected zero-based priority index.
        index: usize,
        /// The current number of named tracks.
        len: usize,
    },
}

/// Public observation for one named Bevy override track.
#[derive(Clone, Debug, PartialEq)]
pub struct SpinalTrackState {
    pub(crate) key: Box<str>,
    pub(crate) playback: SpinalPlaybackState,
    pub(crate) weight: Mix,
    pub(crate) target_weight: Mix,
    pub(crate) weight_fading: bool,
    pub(crate) paused: bool,
    pub(crate) speed: f32,
}

impl SpinalTrackState {
    /// Returns the stable application track key.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Returns current playback observation.
    #[must_use]
    pub const fn playback(&self) -> &SpinalPlaybackState {
        &self.playback
    }

    /// Returns the presented track weight.
    #[must_use]
    pub const fn weight(&self) -> Mix {
        self.weight
    }

    /// Returns the destination of an active weight fade, or the presented
    /// weight when no fade is active.
    #[must_use]
    pub const fn target_weight(&self) -> Mix {
        self.target_weight
    }

    /// Returns whether the presented weight is moving toward another value.
    #[must_use]
    pub const fn is_weight_fading(&self) -> bool {
        self.weight_fading
    }

    /// Returns whether the animation clock is paused.
    #[must_use]
    pub const fn is_paused(&self) -> bool {
        self.paused
    }

    /// Returns the animation-clock speed.
    #[must_use]
    pub const fn speed(&self) -> f32 {
        self.speed
    }
}

/// Ordered public observation of all declared override tracks.
#[derive(Clone, Component, Debug, Default, PartialEq)]
pub struct SpinalTrackStates {
    pub(crate) states: Vec<SpinalTrackState>,
}

impl SpinalTrackStates {
    /// Iterates observations from low to high track priority.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &SpinalTrackState> + ExactSizeIterator {
        self.states.iter()
    }

    /// Looks up one observation by stable application key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&SpinalTrackState> {
        self.states.iter().find(|state| state.key.as_ref() == key)
    }

    /// Returns the number of observed named tracks.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.states.len()
    }

    /// Returns whether no named tracks are observed.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.states.is_empty()
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

/// Procedural local-bone replacements applied after animation and before
/// ordered constraint solving.
#[derive(Clone, Component, Debug, Default, PartialEq)]
pub struct SpinalPoseOverrides {
    overrides: Vec<BoneOverride>,
}

/// Named skeleton-space bone targets applied after animation mixing.
#[derive(Clone, Component, Debug, Default, PartialEq)]
pub struct SpinalControlTargets {
    targets: Vec<ControlTarget>,
}

#[derive(Clone, Debug, PartialEq)]
struct ControlTarget {
    bone: Box<str>,
    position: Vec2,
}

impl SpinalControlTargets {
    /// Inserts or replaces one finite skeleton-space target by bone name.
    pub fn set_skeleton_position(
        &mut self,
        bone: impl AsRef<str>,
        position: Vec2,
    ) -> Result<(), InvalidControlTargetPosition> {
        if !position.is_finite() {
            return Err(InvalidControlTargetPosition);
        }
        let bone = bone.as_ref();
        if let Some(target) = self
            .targets
            .iter_mut()
            .find(|target| target.bone.as_ref() == bone)
        {
            target.position = position;
        } else {
            self.targets.push(ControlTarget {
                bone: bone.into(),
                position,
            });
        }
        Ok(())
    }

    /// Removes and returns one named target position.
    pub fn remove(&mut self, bone: &str) -> Option<Vec2> {
        let index = self
            .targets
            .iter()
            .position(|target| target.bone.as_ref() == bone)?;
        Some(self.targets.remove(index).position)
    }

    /// Returns one named skeleton-space target position.
    #[must_use]
    pub fn get(&self, bone: &str) -> Option<Vec2> {
        self.targets
            .iter()
            .find(|target| target.bone.as_ref() == bone)
            .map(|target| target.position)
    }

    /// Iterates targets in deterministic insertion order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, Vec2)> {
        self.targets
            .iter()
            .map(|target| (target.bone.as_ref(), target.position))
    }

    /// Returns the number of named control targets.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.targets.len()
    }

    /// Returns whether no control targets are declared.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    /// Removes every named control target.
    pub fn clear(&mut self) {
        self.targets.clear();
    }
}

/// Returned when a Bevy control-target position contains NaN or infinity.
#[derive(Clone, Copy, Debug, Error, PartialEq)]
#[error("control target position must be finite")]
pub struct InvalidControlTargetPosition;

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
    /// The current frame has drawable output and uses the supported profile
    /// without active fallbacks.
    Ready,
    /// The current frame uses the supported profile without active fallbacks,
    /// but produced no drawable items.
    ReadyNoDraws,
    /// The current frame has drawable output and at least one active
    /// degradation.
    Degraded,
    /// The asset is usable and diagnostics are visible, but the current frame
    /// produced no drawable items.
    DegradedNoDraws,
    /// The compound asset failed before a usable runtime could be created.
    Failed,
}

impl SpinalInstanceState {
    /// Returns whether the frame has no active degradation.
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self, Self::Ready | Self::ReadyNoDraws)
    }

    /// Returns whether the frame has at least one active degradation.
    #[must_use]
    pub const fn is_degraded(&self) -> bool {
        matches!(self, Self::Degraded | Self::DegradedNoDraws)
    }

    /// Returns whether the compound asset has a live runtime, including
    /// degraded states.
    #[must_use]
    pub const fn is_usable(&self) -> bool {
        matches!(
            self,
            Self::Ready | Self::ReadyNoDraws | Self::Degraded | Self::DegradedNoDraws
        )
    }

    /// Returns whether the current frame produced at least one drawable item.
    ///
    /// This does not promise visual completeness. An application may require
    /// [`Self::Ready`] before replacing a known-complete fallback.
    #[must_use]
    pub const fn has_drawable_output(&self) -> bool {
        matches!(self, Self::Ready | Self::Degraded)
    }
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
            Self::ReadyNoDraws => formatter.write_str("ready (no draws)"),
            Self::Degraded => formatter.write_str("degraded"),
            Self::DegradedNoDraws => formatter.write_str("degraded (no draws)"),
            Self::Failed => formatter.write_str("failed"),
        }
    }
}

use std::{
    num::NonZeroU64,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use thiserror::Error;

use crate::{
    AnimationEvent, AnimationId, AnimationPlayer, Crossfade, EventSink, Mix, MixCurve, PlayOptions,
    PlayOutcome, PlaybackId, PlayerError, PlayerStatus, Skeleton, SkeletonAsset, Transition,
    UpdateReport,
    frame::EditablePose,
    player::{Advance, Playback},
    pose::{AngleBranches, ContributionPose},
    skeleton::SkeletonInstanceKey,
};

static NEXT_MIXER_KEY: AtomicU64 = AtomicU64::new(1);

/// Identifies one permanent base or inserted override track.
///
/// Track IDs are scoped to the mixer that created them. They remain stale
/// after removal and are not stable across processes, hot reload, or mixer
/// reconstruction.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TrackId {
    mixer: NonZeroU64,
    serial: NonZeroU64,
}

impl TrackId {
    /// Returns the nonzero mixer-local serial number.
    #[must_use]
    pub const fn get(self) -> NonZeroU64 {
        self.serial
    }
}

/// One borrowed authored event occurrence with its mixer track identity.
#[derive(Clone, Copy, Debug)]
pub struct TrackAnimationEvent<'a> {
    track: TrackId,
    event: AnimationEvent<'a>,
}

impl<'a> TrackAnimationEvent<'a> {
    /// Returns the track whose playback emitted the occurrence.
    #[must_use]
    pub const fn track(self) -> TrackId {
        self.track
    }

    /// Returns the underlying borrowed animation occurrence.
    #[must_use]
    pub const fn event(self) -> AnimationEvent<'a> {
        self.event
    }
}

/// Receives borrowed mixer events without requiring frame-by-frame storage.
///
/// Events follow playback clocks and are delivered from the base track
/// through the highest override track. Track weight does not suppress them.
/// Replacing or stopping a playback immediately ends event delivery from the
/// outgoing transition source.
pub trait TrackEventSink {
    /// Handles one occurrence in deterministic track and playback order.
    fn event(&mut self, event: TrackAnimationEvent<'_>);
}

impl<F> TrackEventSink for F
where
    F: for<'a> FnMut(TrackAnimationEvent<'a>),
{
    fn event(&mut self, event: TrackAnimationEvent<'_>) {
        self(event);
    }
}

impl TrackEventSink for () {
    fn event(&mut self, _event: TrackAnimationEvent<'_>) {}
}

/// Lifecycle facts produced by one mixer track during the latest update.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackUpdateReport {
    track: TrackId,
    playback: UpdateReport,
    weight_fade_completed: bool,
}

/// One active authored property ignored by an override track.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrackPropertyIssue {
    track: TrackId,
    animation: AnimationId,
    property: crate::PropertyKey,
}

impl TrackPropertyIssue {
    /// Returns the affected override track.
    #[must_use]
    pub const fn track(self) -> TrackId {
        self.track
    }

    /// Returns the active animation containing the property.
    #[must_use]
    pub const fn animation(self) -> AnimationId {
        self.animation
    }

    /// Returns the deferred property ignored by the track.
    #[must_use]
    pub const fn property(self) -> crate::PropertyKey {
        self.property
    }
}

impl TrackUpdateReport {
    /// Returns the track described by this report.
    #[must_use]
    pub const fn track(self) -> TrackId {
        self.track
    }

    /// Returns playback and within-track transition lifecycle facts.
    #[must_use]
    pub const fn playback(self) -> UpdateReport {
        self.playback
    }

    /// Returns whether a nonzero weight fade reached its target.
    #[must_use]
    pub const fn weight_fade_completed(self) -> bool {
        self.weight_fade_completed
    }
}

struct TrackEventAdapter<'a, S: ?Sized> {
    track: TrackId,
    sink: &'a mut S,
}

impl<S: TrackEventSink + ?Sized> EventSink for TrackEventAdapter<'_, S> {
    fn event(&mut self, event: AnimationEvent<'_>) {
        self.sink.event(TrackAnimationEvent {
            track: self.track,
            event,
        });
    }
}

/// Construction settings for one ordered override track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackOptions {
    weight: Mix,
}

impl TrackOptions {
    /// Creates a full-weight replacement-style override track.
    #[must_use]
    pub const fn override_track() -> Self {
        Self { weight: Mix::ONE }
    }

    /// Replaces the initial constant track weight.
    #[must_use]
    pub const fn with_weight(mut self, weight: Mix) -> Self {
        self.weight = weight;
        self
    }

    /// Returns the initial constant track weight.
    #[must_use]
    pub const fn weight(self) -> Mix {
        self.weight
    }
}

impl Default for TrackOptions {
    fn default() -> Self {
        Self::override_track()
    }
}

/// Settings for a wall-clock fade between normalized track weights.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeightFade {
    duration: Duration,
    curve: MixCurve,
}

impl WeightFade {
    /// Creates a linear weight fade.
    #[must_use]
    pub const fn new(duration: Duration) -> Self {
        Self {
            duration,
            curve: MixCurve::Linear,
        }
    }

    /// Replaces the interpolation applied to normalized fade time.
    #[must_use]
    pub const fn with_curve(mut self, curve: MixCurve) -> Self {
        self.curve = curve;
        self
    }

    /// Returns the wall-clock duration.
    #[must_use]
    pub const fn duration(self) -> Duration {
        self.duration
    }

    /// Returns the fade-time interpolation.
    #[must_use]
    pub const fn curve(self) -> MixCurve {
        self.curve
    }
}

/// A rejected playback speed.
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

/// Stable category of track identity failure.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum TrackErrorKind {
    /// The track ID belongs to another mixer.
    #[error("the track identifier belongs to a different mixer")]
    ForeignMixer,
    /// The track was removed or was never created by this mixer.
    #[error("the track identifier is no longer present")]
    Removed,
    /// The mixer exhausted its nonzero track identity space.
    #[error("the mixer exhausted its track identity space")]
    IdentityExhausted,
    /// The requested priority index is outside the current override-track set.
    #[error("the requested track priority index is out of bounds")]
    OrderOutOfBounds,
}

/// A failure to resolve or create a mixer track.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("{kind}")]
pub struct TrackError {
    kind: TrackErrorKind,
}

impl TrackError {
    /// Returns the stable failure category.
    #[must_use]
    pub const fn kind(self) -> TrackErrorKind {
        self.kind
    }

    const fn new(kind: TrackErrorKind) -> Self {
        Self { kind }
    }
}

/// Immutable observation of the permanent base track.
#[derive(Clone, Copy, Debug)]
pub struct BaseTrackRef<'a> {
    player: &'a AnimationPlayer,
    paused: bool,
    speed: f32,
}

impl BaseTrackRef<'_> {
    /// Returns a copyable snapshot of base playback state.
    #[must_use]
    pub fn status(self) -> PlayerStatus {
        self.player.status()
    }

    /// Returns whether animation-clock advancement is paused.
    #[must_use]
    pub const fn is_paused(self) -> bool {
        self.paused
    }

    /// Returns the animation-clock speed.
    #[must_use]
    pub const fn speed(self) -> f32 {
        self.speed
    }
}

/// Mutable command access to the permanent base track.
#[derive(Debug)]
pub struct BaseTrackMut<'a> {
    player: &'a mut AnimationPlayer,
    paused: &'a mut bool,
    speed: &'a mut f32,
}

impl BaseTrackMut<'_> {
    /// Replaces the base animation and starts it at time zero.
    pub fn play(
        &mut self,
        animation: AnimationId,
        options: PlayOptions,
    ) -> Result<PlayOutcome, PlayerError> {
        self.player.play(animation, options)
    }

    /// Restarts the current base animation immediately.
    pub fn restart(&mut self) -> Result<Option<PlayOutcome>, PlayerError> {
        self.player.restart()
    }

    /// Moves the active base playback clock to an absolute elapsed time.
    ///
    /// This preserves playback identity, mode, and any active crossfade. It
    /// emits no events or lifecycle pulses; the next mixer update continues
    /// from the sought event baseline.
    pub fn seek_to(&mut self, elapsed: Duration) -> Option<PlaybackId> {
        self.player.seek_to(elapsed)
    }

    /// Stops the base playback and optionally crossfades to setup pose.
    pub fn stop(&mut self, transition: Transition) -> Option<PlaybackId> {
        self.player.stop(transition)
    }

    /// Returns a copyable snapshot of base playback state.
    #[must_use]
    pub fn status(&self) -> PlayerStatus {
        self.player.status()
    }

    /// Pauses or resumes the animation clock without pausing crossfades.
    pub fn set_paused(&mut self, paused: bool) {
        *self.paused = paused;
    }

    /// Returns whether animation-clock advancement is paused.
    #[must_use]
    pub const fn is_paused(&self) -> bool {
        *self.paused
    }

    /// Sets the nonnegative finite animation-clock speed.
    pub fn set_speed(&mut self, speed: f32) -> Result<(), InvalidPlaybackSpeed> {
        validate_speed(speed)?;
        *self.speed = speed;
        Ok(())
    }

    /// Returns the animation-clock speed.
    #[must_use]
    pub const fn speed(&self) -> f32 {
        *self.speed
    }
}

/// Immutable observation of one ordered override track.
#[derive(Clone, Copy, Debug)]
pub struct TrackRef<'a> {
    track: &'a OverrideTrack,
}

impl TrackRef<'_> {
    /// Returns the stable mixer-scoped track identifier.
    #[must_use]
    pub const fn id(self) -> TrackId {
        self.track.id
    }

    /// Returns a copyable playback snapshot.
    #[must_use]
    pub fn status(self) -> PlayerStatus {
        self.track.status()
    }

    /// Returns the currently presented normalized track weight.
    #[must_use]
    pub const fn weight(self) -> Mix {
        self.track.weight
    }

    /// Returns the destination weight of an active fade, or the presented
    /// weight when no fade is active.
    #[must_use]
    pub fn target_weight(self) -> Mix {
        self.track
            .weight_fade
            .map_or(self.track.weight, |fade| fade.target)
    }

    /// Returns whether the presented weight is moving toward another value.
    #[must_use]
    pub const fn is_weight_fading(self) -> bool {
        self.track.weight_fade.is_some()
    }

    /// Returns whether animation-clock advancement is paused.
    #[must_use]
    pub const fn is_paused(self) -> bool {
        self.track.paused
    }

    /// Returns the animation-clock speed.
    #[must_use]
    pub const fn speed(self) -> f32 {
        self.track.speed
    }
}

/// Mutable command access to one ordered override track.
#[derive(Debug)]
pub struct TrackMut<'a> {
    track: &'a mut OverrideTrack,
}

impl TrackMut<'_> {
    /// Replaces this track's animation and starts it at time zero.
    pub fn play(
        &mut self,
        animation: AnimationId,
        options: PlayOptions,
    ) -> Result<PlayOutcome, PlayerError> {
        self.track.play(animation, options)
    }

    /// Restarts the current override animation immediately.
    pub fn restart(&mut self) -> Result<Option<PlayOutcome>, PlayerError> {
        self.track.restart()
    }

    /// Stops this track and optionally fades out its sparse contribution.
    pub fn stop(&mut self, transition: Transition) -> Option<PlaybackId> {
        self.track.stop(transition)
    }

    /// Returns a copyable playback snapshot.
    #[must_use]
    pub fn status(&self) -> PlayerStatus {
        self.track.status()
    }

    /// Sets the constant normalized track weight.
    pub fn set_weight(&mut self, weight: Mix) {
        self.track.weight = weight;
        self.track.weight_fade = None;
    }

    /// Fades from the presented weight to `target` in wall-clock time.
    pub fn fade_weight(&mut self, target: Mix, fade: WeightFade) {
        self.track.fade_weight(target, fade);
    }

    /// Returns the constant normalized track weight.
    #[must_use]
    pub const fn weight(&self) -> Mix {
        self.track.weight
    }

    /// Pauses or resumes the animation clock without pausing fades.
    pub fn set_paused(&mut self, paused: bool) {
        self.track.paused = paused;
    }

    /// Returns whether animation-clock advancement is paused.
    #[must_use]
    pub const fn is_paused(&self) -> bool {
        self.track.paused
    }

    /// Sets the nonnegative finite animation-clock speed.
    pub fn set_speed(&mut self, speed: f32) -> Result<(), InvalidPlaybackSpeed> {
        validate_speed(speed)?;
        self.track.speed = speed;
        Ok(())
    }

    /// Returns the animation-clock speed.
    #[must_use]
    pub const fn speed(&self) -> f32 {
        self.track.speed
    }
}

/// A renderer-independent ordered animation-track mixer.
///
/// The permanent base track reconstructs a complete pose. Inserted override
/// tracks then change only supported continuous properties authored by their
/// current animation. The returned pose remains editable before constraints
/// are solved once.
#[derive(Debug)]
pub struct AnimationMixer {
    key: NonZeroU64,
    instance_key: SkeletonInstanceKey,
    asset: Arc<SkeletonAsset>,
    base_id: TrackId,
    base: AnimationPlayer,
    base_paused: bool,
    base_speed: f32,
    base_report: UpdateReport,
    tracks: Vec<OverrideTrack>,
    next_track_serial: u64,
}

impl AnimationMixer {
    /// Creates an idle mixer permanently bound to one skeleton instance.
    #[must_use]
    pub fn new(skeleton: &Skeleton) -> Self {
        let key = next_mixer_key();
        let base_id = TrackId {
            mixer: key,
            serial: NonZeroU64::MIN,
        };
        Self {
            key,
            instance_key: skeleton.instance_key(),
            asset: Arc::clone(skeleton.asset_handle()),
            base_id,
            base: AnimationPlayer::new(skeleton),
            base_paused: false,
            base_speed: 1.0,
            base_report: UpdateReport::default(),
            tracks: Vec::new(),
            next_track_serial: 2,
        }
    }

    /// Returns the permanent base track ID.
    #[must_use]
    pub const fn base_track_id(&self) -> TrackId {
        self.base_id
    }

    /// Borrows immutable observation of the permanent base track.
    #[must_use]
    pub fn base_track(&self) -> BaseTrackRef<'_> {
        BaseTrackRef {
            player: &self.base,
            paused: self.base_paused,
            speed: self.base_speed,
        }
    }

    /// Borrows mutable command access to the permanent base track.
    pub fn base_track_mut(&mut self) -> BaseTrackMut<'_> {
        BaseTrackMut {
            player: &mut self.base,
            paused: &mut self.base_paused,
            speed: &mut self.base_speed,
        }
    }

    /// Inserts an override track above every existing track.
    pub fn insert_track(&mut self, options: TrackOptions) -> Result<TrackId, TrackError> {
        let serial = NonZeroU64::new(self.next_track_serial)
            .ok_or_else(|| TrackError::new(TrackErrorKind::IdentityExhausted))?;
        self.next_track_serial = self.next_track_serial.checked_add(1).unwrap_or(0);
        let id = TrackId {
            mixer: self.key,
            serial,
        };
        self.tracks
            .push(OverrideTrack::new(id, options, &self.asset));
        Ok(id)
    }

    /// Returns the number of inserted override tracks.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.tracks.len()
    }

    /// Returns whether no override tracks are present.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    /// Iterates override-track IDs from low to high priority.
    pub fn tracks(&self) -> impl DoubleEndedIterator<Item = TrackId> + ExactSizeIterator + '_ {
        self.tracks.iter().map(|track| track.id)
    }

    /// Resolves immutable observation of one inserted override track.
    pub fn track(&self, id: TrackId) -> Result<TrackRef<'_>, TrackError> {
        self.validate_track_scope(id)?;
        let track = self
            .tracks
            .iter()
            .find(|track| track.id == id)
            .ok_or_else(|| TrackError::new(TrackErrorKind::Removed))?;
        Ok(TrackRef { track })
    }

    /// Resolves mutable command access to one inserted override track.
    pub fn track_mut(&mut self, id: TrackId) -> Result<TrackMut<'_>, TrackError> {
        self.validate_track_scope(id)?;
        let track = self
            .tracks
            .iter_mut()
            .find(|track| track.id == id)
            .ok_or_else(|| TrackError::new(TrackErrorKind::Removed))?;
        Ok(TrackMut { track })
    }

    /// Removes one inserted override track.
    pub fn remove_track(&mut self, id: TrackId) -> Result<(), TrackError> {
        self.validate_track_scope(id)?;
        let index = self
            .tracks
            .iter()
            .position(|track| track.id == id)
            .ok_or_else(|| TrackError::new(TrackErrorKind::Removed))?;
        self.tracks.remove(index);
        Ok(())
    }

    /// Moves one override track to a zero-based low-to-high priority index.
    ///
    /// Playback, transitions, and weight fades remain attached to the track.
    /// Invalid IDs or indices leave ordering unchanged.
    pub fn move_track(&mut self, id: TrackId, index: usize) -> Result<(), TrackError> {
        self.validate_track_scope(id)?;
        if index >= self.tracks.len() {
            return Err(TrackError::new(TrackErrorKind::OrderOutOfBounds));
        }
        let current = self
            .tracks
            .iter()
            .position(|track| track.id == id)
            .ok_or_else(|| TrackError::new(TrackErrorKind::Removed))?;
        if current != index {
            let track = self.tracks.remove(current);
            self.tracks.insert(index, track);
            for track in &mut self.tracks {
                track.apply_branches.reset();
            }
        }
        Ok(())
    }

    /// Iterates latest lifecycle reports from base to highest override track.
    pub fn reports(&self) -> impl Iterator<Item = TrackUpdateReport> + '_ {
        std::iter::once(TrackUpdateReport {
            track: self.base_id,
            playback: self.base_report,
            weight_fade_completed: false,
        })
        .chain(self.tracks.iter().map(|track| TrackUpdateReport {
            track: track.id,
            playback: track.last_report,
            weight_fade_completed: track.last_weight_fade_completed,
        }))
    }

    /// Iterates active deferred properties in track and authored order.
    pub fn active_deferred_properties(&self) -> impl Iterator<Item = TrackPropertyIssue> + '_ {
        let asset = &self.asset;
        self.tracks.iter().flat_map(move |track| {
            (track.weight != Mix::ZERO)
                .then_some(track)
                .into_iter()
                .flat_map(move |track| {
                    track.presented.active_animations.iter().copied().flat_map(
                        move |animation_index| {
                            let animation_index = animation_index as usize;
                            let animation = AnimationId::new(asset.key(), animation_index as u32);
                            asset
                                .animation_data(animation_index)
                                .deferred_override_properties
                                .iter()
                                .copied()
                                .map(move |property| {
                                    let property = property.to_key(asset.key());
                                    TrackPropertyIssue {
                                        track: track.id,
                                        animation,
                                        property,
                                    }
                                })
                        },
                    )
                })
        })
    }

    /// Returns whether an active override track ignores any authored property.
    #[must_use]
    pub fn has_degraded_overrides(&self) -> bool {
        self.active_deferred_properties().next().is_some()
    }

    /// Advances every track and produces one procedurally editable local pose.
    ///
    /// Track clocks are validated before the base pose or any mixer state is
    /// changed. Override tracks are evaluated in their observable low-to-high
    /// priority order. Playback clocks use scaled time; crossfades and weight
    /// fades use the unscaled `delta`.
    pub fn update<'s, S: TrackEventSink + ?Sized>(
        &mut self,
        skeleton: &'s mut Skeleton,
        delta: Duration,
        events: &mut S,
    ) -> Result<EditablePose<'s>, PlayerError> {
        if skeleton.instance_key() != self.instance_key {
            return Err(PlayerError::ForeignSkeleton);
        }
        let base_delta = if self.base.status().playback().is_some() {
            playback_delta(delta, self.base_paused, self.base_speed)?
        } else {
            Duration::ZERO
        };
        self.base.validate_update_with_time(skeleton, base_delta)?;
        for track in &self.tracks {
            track.validate_update(delta)?;
        }

        let base_id = self.base_id;
        let report = {
            let mut base_events = TrackEventAdapter {
                track: base_id,
                sink: events,
            };
            self.base
                .update_pose_with_time(skeleton, base_delta, delta, &mut base_events)?
        };
        self.base_report = report;
        for track in &mut self.tracks {
            track.update(skeleton, delta, events);
        }
        Ok(EditablePose::new(skeleton, report))
    }

    fn validate_track_scope(&self, id: TrackId) -> Result<(), TrackError> {
        if id.mixer != self.key {
            return Err(TrackError::new(TrackErrorKind::ForeignMixer));
        }
        Ok(())
    }
}

#[derive(Debug)]
struct OverrideTrack {
    id: TrackId,
    asset: Arc<SkeletonAsset>,
    active: Option<Playback>,
    transition: Option<ContributionTransition>,
    next_playback_id: u64,
    weight: Mix,
    weight_fade: Option<ActiveWeightFade>,
    paused: bool,
    speed: f32,
    sampled: ContributionPose,
    presented: ContributionPose,
    transition_source: ContributionPose,
    transition_branches: AngleBranches,
    apply_branches: AngleBranches,
    last_report: UpdateReport,
    last_weight_fade_completed: bool,
}

impl OverrideTrack {
    fn new(id: TrackId, options: TrackOptions, asset: &Arc<SkeletonAsset>) -> Self {
        Self {
            id,
            asset: Arc::clone(asset),
            active: None,
            transition: None,
            next_playback_id: 1,
            weight: options.weight,
            weight_fade: None,
            paused: false,
            speed: 1.0,
            sampled: ContributionPose::new(asset),
            presented: ContributionPose::new(asset),
            transition_source: ContributionPose::new(asset),
            transition_branches: AngleBranches::new(asset.bones().len()),
            apply_branches: AngleBranches::new(asset.bones().len()),
            last_report: UpdateReport::default(),
            last_weight_fade_completed: false,
        }
    }

    fn play(
        &mut self,
        animation: AnimationId,
        options: PlayOptions,
    ) -> Result<PlayOutcome, PlayerError> {
        let animation_index = self
            .asset
            .animation_index(animation)
            .map_err(PlayerError::InvalidAnimation)?;
        let duration_ticks = self.asset.animation_data(animation_index).duration.ticks;
        let playback = self.issue_playback_id();
        let interrupted = self.active.map(|active| active.id);
        self.begin_transition(options.transition());
        self.active = Some(Playback {
            id: playback,
            animation,
            animation_index,
            mode: options.mode(),
            duration_ticks,
            local_ticks: 0,
            loop_index: 0,
            pending_start: true,
            complete: false,
        });
        Ok(PlayOutcome::new(playback, interrupted))
    }

    fn restart(&mut self) -> Result<Option<PlayOutcome>, PlayerError> {
        let Some(active) = self.active else {
            return Ok(None);
        };
        self.play(
            active.animation,
            match active.mode {
                crate::PlaybackMode::Once => PlayOptions::once(),
                crate::PlaybackMode::Loop => PlayOptions::looping(),
            },
        )
        .map(Some)
    }

    fn stop(&mut self, transition: Transition) -> Option<PlaybackId> {
        let stopped = self.active.map(|active| active.id)?;
        self.begin_transition(transition);
        self.active = None;
        Some(stopped)
    }

    fn status(&self) -> PlayerStatus {
        let transition_mix = self.transition.map(ContributionTransition::amount);
        match self.active {
            Some(active) => PlayerStatus::from_playback(active, transition_mix),
            None => PlayerStatus::from_transition(transition_mix, self.transition.is_some()),
        }
    }

    fn validate_update(&self, delta: Duration) -> Result<(), PlayerError> {
        let Some(active) = self.active else {
            return Ok(());
        };
        let delta = playback_delta(delta, self.paused, self.speed)?;
        let advance = active.advance(delta)?;
        AnimationPlayer::validate_event_budget(&self.asset, advance)?;
        Ok(())
    }

    fn update<S: TrackEventSink + ?Sized>(
        &mut self,
        skeleton: &mut Skeleton,
        delta: Duration,
        events: &mut S,
    ) {
        let advance = self.active.map(|active| {
            let playback_delta = playback_delta(delta, self.paused, self.speed)
                .expect("scaled track time was validated before mixer mutation");
            active
                .advance(playback_delta)
                .expect("all track clocks were validated before mixer mutation")
        });
        let next_weight_fade = self.weight_fade.map(|fade| fade.advance(delta));
        self.last_weight_fade_completed =
            next_weight_fade.is_some_and(ActiveWeightFade::is_complete);
        if let Some(fade) = next_weight_fade {
            self.weight = fade.weight();
        }
        self.weight_fade = next_weight_fade.filter(|fade| !fade.is_complete());
        let next_transition = self.transition.map(|transition| transition.advance(delta));
        if let Some(advance) = advance {
            skeleton
                .sample_animation_contribution(
                    advance.next.animation,
                    Duration::from_nanos(advance.next.local_ticks),
                    advance.next.mode,
                    &mut self.sampled,
                )
                .expect("an active track retains an asset-local animation");
        } else {
            self.sampled.clear();
        }
        if let Some(transition) = next_transition {
            self.presented.mix_from(
                &self.transition_source,
                &self.sampled,
                transition.amount().get(),
                transition.crossfade.rotation_path(),
                &mut self.transition_branches,
            );
        } else {
            self.presented.copy_from(&self.sampled);
        }
        self.presented
            .apply_to(&mut skeleton.pose, self.weight, &mut self.apply_branches);
        if let Some(advance) = advance {
            self.emit_events(advance, events);
        }
        self.active = advance.map(|advance| advance.next);
        let transition_completed = next_transition.is_some_and(ContributionTransition::is_complete);
        self.transition = next_transition.filter(|transition| !transition.is_complete());
        self.last_report = UpdateReport::new(
            self.active.map(|active| active.id),
            advance.and_then(|advance| advance.completed.then_some(advance.next.id)),
            advance.map_or(0, |advance| advance.loops_completed),
            transition_completed,
        );
    }

    fn issue_playback_id(&mut self) -> PlaybackId {
        let value = NonZeroU64::new(self.next_playback_id)
            .expect("the track always skips the zero playback ID");
        self.next_playback_id = self.next_playback_id.wrapping_add(1);
        if self.next_playback_id == 0 {
            self.next_playback_id = 1;
        }
        PlaybackId::new(value)
    }

    fn emit_events<S: TrackEventSink + ?Sized>(&self, advance: Advance, events: &mut S) {
        let mut adapter = TrackEventAdapter {
            track: self.id,
            sink: events,
        };
        AnimationPlayer::emit_events(&self.asset, advance, &mut adapter);
    }

    fn fade_weight(&mut self, target: Mix, fade: WeightFade) {
        if fade.duration().is_zero() || target == self.weight {
            self.weight = target;
            self.weight_fade = None;
            return;
        }
        self.weight_fade = Some(ActiveWeightFade {
            source: self.weight,
            target,
            fade,
            elapsed: Duration::ZERO,
        });
    }

    fn begin_transition(&mut self, transition: Transition) {
        self.transition = match transition {
            Transition::Immediate => {
                self.transition_branches.reset();
                self.apply_branches.reset();
                None
            }
            Transition::Crossfade(crossfade) if crossfade.duration().is_zero() => {
                self.transition_branches.reset();
                self.apply_branches.reset();
                None
            }
            Transition::Crossfade(crossfade) => {
                self.transition_source.copy_from(&self.presented);
                self.transition_branches.reset();
                self.apply_branches.reset();
                Some(ContributionTransition {
                    crossfade,
                    elapsed: Duration::ZERO,
                })
            }
        };
    }
}

#[derive(Clone, Copy, Debug)]
struct ActiveWeightFade {
    source: Mix,
    target: Mix,
    fade: WeightFade,
    elapsed: Duration,
}

impl ActiveWeightFade {
    fn advance(mut self, delta: Duration) -> Self {
        self.elapsed = self.elapsed.saturating_add(delta).min(self.fade.duration());
        self
    }

    fn weight(self) -> Mix {
        let linear = if self.fade.duration().is_zero() {
            1.0
        } else {
            (self.elapsed.as_secs_f64() / self.fade.duration().as_secs_f64()) as f32
        };
        let amount = self.fade.curve().apply(linear);
        Mix::clamped(f64::from(self.source.get()).mul_add(
            1.0 - f64::from(amount),
            f64::from(self.target.get()) * f64::from(amount),
        ) as f32)
        .expect("normalized endpoints and fade time produce a normalized weight")
    }

    fn is_complete(self) -> bool {
        self.elapsed >= self.fade.duration()
    }
}

#[derive(Clone, Copy, Debug)]
struct ContributionTransition {
    crossfade: Crossfade,
    elapsed: Duration,
}

impl ContributionTransition {
    fn advance(mut self, delta: Duration) -> Self {
        self.elapsed = self
            .elapsed
            .saturating_add(delta)
            .min(self.crossfade.duration());
        self
    }

    fn amount(self) -> Mix {
        let linear = if self.crossfade.duration().is_zero() {
            1.0
        } else {
            (self.elapsed.as_secs_f64() / self.crossfade.duration().as_secs_f64()) as f32
        };
        Mix::clamped(self.crossfade.curve().apply(linear))
            .expect("finite durations produce a finite normalized contribution mix")
    }

    fn is_complete(self) -> bool {
        self.elapsed >= self.crossfade.duration()
    }
}

fn next_mixer_key() -> NonZeroU64 {
    let value = NEXT_MIXER_KEY
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .expect("the process exhausted its mixer identity space");
    NonZeroU64::new(value).expect("the mixer identity counter starts at one")
}

fn validate_speed(speed: f32) -> Result<(), InvalidPlaybackSpeed> {
    if !speed.is_finite() || speed < 0.0 {
        return Err(InvalidPlaybackSpeed { speed });
    }
    Ok(())
}

fn playback_delta(wall_delta: Duration, paused: bool, speed: f32) -> Result<Duration, PlayerError> {
    if paused {
        Ok(Duration::ZERO)
    } else {
        scale_duration(wall_delta, speed)
    }
}

fn scale_duration(duration: Duration, scale: f32) -> Result<Duration, PlayerError> {
    if duration.is_zero() || scale == 0.0 {
        return Ok(Duration::ZERO);
    }
    if scale == 1.0 {
        return Ok(duration);
    }

    debug_assert!(scale.is_finite() && scale.is_sign_positive());
    let bits = scale.to_bits();
    let encoded_exponent = ((bits >> 23) & 0xff) as i32;
    let fraction = bits & 0x7f_ffff;
    let (significand, exponent) = if encoded_exponent == 0 {
        (u128::from(fraction), -149)
    } else {
        (
            u128::from((1 << 23) | fraction),
            encoded_exponent - 127 - 23,
        )
    };
    let product = duration
        .as_nanos()
        .checked_mul(significand)
        .ok_or(PlayerError::TimeOverflow)?;
    let maximum = Duration::MAX.as_nanos();
    let scaled = if exponent >= 0 {
        let shift = exponent as u32;
        if shift >= u128::BITS || product > maximum >> shift {
            return Err(PlayerError::TimeOverflow);
        }
        product << shift
    } else {
        let shift = exponent.unsigned_abs();
        if shift >= u128::BITS {
            0
        } else {
            let divisor = 1_u128 << shift;
            let quotient = product / divisor;
            let remainder = product % divisor;
            quotient + u128::from(remainder >= divisor / 2)
        }
    };
    if scaled > maximum {
        return Err(PlayerError::TimeOverflow);
    }

    const NANOS_PER_SECOND: u128 = 1_000_000_000;
    Ok(Duration::new(
        (scaled / NANOS_PER_SECOND) as u64,
        (scaled % NANOS_PER_SECOND) as u32,
    ))
}

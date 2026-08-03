use std::{num::NonZeroU64, sync::Arc, time::Duration};

use thiserror::Error;

use crate::{
    AnimationId, Diagnostic, DiagnosticScope, EventDefinitionRef, EventId, IdError, Mix,
    PlaybackMode, Skeleton, SkeletonAsset,
    animation::{AnimationData, EventFrame, TimelineData},
    frame::EditablePose,
    pose::{AngleBranches, BlendSwitches, PoseBuffers},
    skeleton::SkeletonInstanceKey,
};

/// Identifies one invocation of [`AnimationPlayer::play`].
///
/// A new ID is issued even when an animation restarts itself. IDs are local
/// to one player and are intended for correlating events and update reports,
/// not for persistence.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PlaybackId(NonZeroU64);

impl PlaybackId {
    pub(crate) fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    /// Returns the nonzero integer representation.
    #[must_use]
    pub const fn get(self) -> NonZeroU64 {
        self.0
    }
}

/// Options for replacing the player's current animation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayOptions {
    mode: PlaybackMode,
    transition: Transition,
}

impl PlayOptions {
    /// Plays once and holds the final pose.
    #[must_use]
    pub const fn once() -> Self {
        Self {
            mode: PlaybackMode::Once,
            transition: Transition::Immediate,
        }
    }

    /// Loops from the animation duration back to time zero.
    #[must_use]
    pub const fn looping() -> Self {
        Self {
            mode: PlaybackMode::Loop,
            transition: Transition::Immediate,
        }
    }

    /// Replaces the transition used to enter this playback.
    #[must_use]
    pub const fn with_transition(mut self, transition: Transition) -> Self {
        self.transition = transition;
        self
    }

    /// Returns the requested end behavior.
    #[must_use]
    pub const fn mode(self) -> PlaybackMode {
        self.mode
    }

    /// Returns the requested transition.
    #[must_use]
    pub const fn transition(self) -> Transition {
        self.transition
    }
}

impl Default for PlayOptions {
    fn default() -> Self {
        Self::once()
    }
}

/// How a newly requested pose replaces the currently presented base pose.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub enum Transition {
    /// Uses the new target pose on the next player update.
    #[default]
    Immediate,
    /// Interpolates a frozen presentation snapshot into the new target.
    Crossfade(Crossfade),
}

/// Settings for one interruption-safe crossfade.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crossfade {
    duration: Duration,
    curve: MixCurve,
    discrete: DiscreteSwitches,
    rotation_path: RotationPath,
}

impl Crossfade {
    /// Creates a linear crossfade whose discrete properties switch at start.
    #[must_use]
    pub const fn new(duration: Duration) -> Self {
        Self {
            duration,
            curve: MixCurve::Linear,
            discrete: DiscreteSwitches::TARGET_AT_START,
            rotation_path: RotationPath::Shortest,
        }
    }

    /// Replaces the interpolation applied to normalized transition time.
    #[must_use]
    pub const fn with_curve(mut self, curve: MixCurve) -> Self {
        self.curve = curve;
        self
    }

    /// Replaces the switch points for non-interpolated properties.
    #[must_use]
    pub const fn with_discrete(mut self, discrete: DiscreteSwitches) -> Self {
        self.discrete = discrete;
        self
    }

    /// Replaces how angular values choose their path during the crossfade.
    #[must_use]
    pub const fn with_rotation_path(mut self, rotation_path: RotationPath) -> Self {
        self.rotation_path = rotation_path;
        self
    }

    /// Returns the wall-clock transition duration.
    #[must_use]
    pub const fn duration(self) -> Duration {
        self.duration
    }

    /// Returns the transition-time interpolation.
    #[must_use]
    pub const fn curve(self) -> MixCurve {
        self.curve
    }

    /// Returns the switch points for non-interpolated properties.
    #[must_use]
    pub const fn discrete(self) -> DiscreteSwitches {
        self.discrete
    }

    /// Returns how angular values choose their path during the crossfade.
    #[must_use]
    pub const fn rotation_path(self) -> RotationPath {
        self.rotation_path
    }
}

/// How angular values choose a path during a crossfade.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum RotationPath {
    /// Re-evaluates the shortest angular path on every update.
    ///
    /// This avoids unintended full turns when a moving target crosses the
    /// source angle, but the chosen direction can change during a crossfade.
    #[default]
    Shortest,
    /// Keeps the first nonzero rotation direction for the whole crossfade.
    ///
    /// This prevents direction reversals, but can intentionally take the long
    /// way around when a moving target crosses the source angle.
    PreserveDirection,
}

/// Interpolation applied to a crossfade's normalized elapsed time.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum MixCurve {
    /// Uses normalized elapsed time directly.
    #[default]
    Linear,
    /// Uses a cubic smoothstep with zero slope at both ends.
    SmoothStep,
}

impl MixCurve {
    pub(crate) fn apply(self, amount: f32) -> f32 {
        let amount = amount.clamp(0.0, 1.0);
        match self {
            Self::Linear => amount,
            Self::SmoothStep => amount * amount * (3.0 - 2.0 * amount),
        }
    }
}

/// Crossfade switch points for properties that cannot be interpolated.
///
/// The target value is selected when the eased crossfade amount is greater
/// than or equal to the corresponding switch point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiscreteSwitches {
    attachment: Mix,
    draw_order: Mix,
    ik_bend: Mix,
    scale_sign: Mix,
}

impl DiscreteSwitches {
    /// Selects all target discrete values as soon as a crossfade is applied.
    pub const TARGET_AT_START: Self = Self {
        attachment: Mix::ZERO,
        draw_order: Mix::ZERO,
        ik_bend: Mix::ZERO,
        scale_sign: Mix::ZERO,
    };

    /// Keeps all source discrete values until a crossfade reaches its end.
    pub const TARGET_AT_END: Self = Self {
        attachment: Mix::ONE,
        draw_order: Mix::ONE,
        ik_bend: Mix::ONE,
        scale_sign: Mix::ONE,
    };

    /// Uses one switch point for every discrete property.
    #[must_use]
    pub const fn uniform(at: Mix) -> Self {
        Self {
            attachment: at,
            draw_order: at,
            ik_bend: at,
            scale_sign: at,
        }
    }

    /// Creates independently configurable switch points.
    #[must_use]
    pub const fn new(attachment: Mix, draw_order: Mix, ik_bend: Mix, scale_sign: Mix) -> Self {
        Self {
            attachment,
            draw_order,
            ik_bend,
            scale_sign,
        }
    }

    /// Returns the attachment switch point.
    #[must_use]
    pub const fn attachment(self) -> Mix {
        self.attachment
    }

    /// Returns the draw-order switch point.
    #[must_use]
    pub const fn draw_order(self) -> Mix {
        self.draw_order
    }

    /// Returns the IK bend-direction switch point.
    #[must_use]
    pub const fn ik_bend(self) -> Mix {
        self.ik_bend
    }

    /// Returns the signed-scale sign switch point.
    #[must_use]
    pub const fn scale_sign(self) -> Mix {
        self.scale_sign
    }

    fn as_blend_switches(self) -> BlendSwitches {
        BlendSwitches {
            attachment: self.attachment.get(),
            draw_order: self.draw_order.get(),
            ik_bend: self.ik_bend.get(),
            scale_sign: self.scale_sign.get(),
        }
    }
}

impl Default for DiscreteSwitches {
    fn default() -> Self {
        Self::TARGET_AT_START
    }
}

/// The result of replacing the current playback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlayOutcome {
    playback: PlaybackId,
    interrupted: Option<PlaybackId>,
}

impl PlayOutcome {
    pub(crate) const fn new(playback: PlaybackId, interrupted: Option<PlaybackId>) -> Self {
        Self {
            playback,
            interrupted,
        }
    }

    /// Returns the newly issued playback ID.
    #[must_use]
    pub const fn playback(self) -> PlaybackId {
        self.playback
    }

    /// Returns the playback that was replaced, if one existed.
    #[must_use]
    pub const fn interrupted(self) -> Option<PlaybackId> {
        self.interrupted
    }
}

/// A snapshot of the one-track player's observable state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerStatus {
    playback: Option<PlaybackId>,
    animation: Option<AnimationId>,
    mode: Option<PlaybackMode>,
    position: Option<Duration>,
    loop_index: Option<u128>,
    complete: bool,
    transition_mix: Option<Mix>,
    stopping: bool,
}

impl PlayerStatus {
    pub(crate) const fn from_playback(playback: Playback, transition_mix: Option<Mix>) -> Self {
        Self {
            playback: Some(playback.id),
            animation: Some(playback.animation),
            mode: Some(playback.mode),
            position: Some(Duration::from_nanos(playback.local_ticks)),
            loop_index: Some(playback.loop_index),
            complete: playback.complete,
            transition_mix,
            stopping: false,
        }
    }

    pub(crate) const fn from_transition(transition_mix: Option<Mix>, stopping: bool) -> Self {
        Self {
            playback: None,
            animation: None,
            mode: None,
            position: None,
            loop_index: None,
            complete: false,
            transition_mix,
            stopping,
        }
    }

    /// Returns whether no animation or setup-pose transition is active.
    #[must_use]
    pub const fn is_idle(self) -> bool {
        self.playback.is_none() && !self.stopping
    }

    /// Returns the current playback ID.
    #[must_use]
    pub const fn playback(self) -> Option<PlaybackId> {
        self.playback
    }

    /// Returns the current animation.
    #[must_use]
    pub const fn animation(self) -> Option<AnimationId> {
        self.animation
    }

    /// Returns the current playback mode.
    #[must_use]
    pub const fn mode(self) -> Option<PlaybackMode> {
        self.mode
    }

    /// Returns the current animation-local position.
    #[must_use]
    pub const fn position(self) -> Option<Duration> {
        self.position
    }

    /// Returns the current zero-based loop index.
    #[must_use]
    pub const fn loop_index(self) -> Option<u128> {
        self.loop_index
    }

    /// Returns whether a once playback is holding its endpoint.
    #[must_use]
    pub const fn is_complete(self) -> bool {
        self.complete
    }

    /// Returns the eased transition influence most recently applied.
    ///
    /// A newly requested crossfade reports zero until the next update.
    #[must_use]
    pub const fn transition_mix(self) -> Option<Mix> {
        self.transition_mix
    }

    /// Returns whether the player is transitioning to setup pose.
    #[must_use]
    pub const fn is_stopping(self) -> bool {
        self.stopping
    }
}

/// Small lifecycle facts produced by one player update.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UpdateReport {
    current: Option<PlaybackId>,
    completed: Option<PlaybackId>,
    loops_completed: u128,
    transition_completed: bool,
}

impl UpdateReport {
    pub(crate) const fn new(
        current: Option<PlaybackId>,
        completed: Option<PlaybackId>,
        loops_completed: u128,
        transition_completed: bool,
    ) -> Self {
        Self {
            current,
            completed,
            loops_completed,
            transition_completed,
        }
    }

    /// Returns the playback sampled by this update.
    #[must_use]
    pub const fn current(self) -> Option<PlaybackId> {
        self.current
    }

    /// Returns a once playback that first reached its endpoint this update.
    #[must_use]
    pub const fn completed(self) -> Option<PlaybackId> {
        self.completed
    }

    /// Returns how many loop boundaries the current playback crossed.
    #[must_use]
    pub const fn loops_completed(self) -> u128 {
        self.loops_completed
    }

    /// Returns whether a nonzero crossfade reached full influence.
    #[must_use]
    pub const fn transition_completed(self) -> bool {
        self.transition_completed
    }
}

/// One borrowed authored event occurrence emitted by a player update.
#[derive(Clone, Copy, Debug)]
pub struct AnimationEvent<'a> {
    playback: PlaybackId,
    animation: AnimationId,
    loop_index: u128,
    local_time: Duration,
    definition: EventDefinitionRef<'a>,
    integer: i32,
    float: f32,
    string: Option<&'a str>,
    volume: f32,
    balance: f32,
    diagnostics: &'a [Diagnostic],
}

impl<'a> AnimationEvent<'a> {
    /// Returns the playback that emitted this occurrence.
    #[must_use]
    pub const fn playback(self) -> PlaybackId {
        self.playback
    }

    /// Returns the animation containing the event key.
    #[must_use]
    pub const fn animation(self) -> AnimationId {
        self.animation
    }

    /// Returns the zero-based loop containing this occurrence.
    #[must_use]
    pub const fn loop_index(self) -> u128 {
        self.loop_index
    }

    /// Returns the event key's animation-local time.
    #[must_use]
    pub const fn local_time(self) -> Duration {
        self.local_time
    }

    /// Returns the immutable event definition.
    #[must_use]
    pub const fn definition(self) -> EventDefinitionRef<'a> {
        self.definition
    }

    /// Returns the resolved integer payload for this key.
    #[must_use]
    pub const fn integer(self) -> i32 {
        self.integer
    }

    /// Returns the resolved floating-point payload for this key.
    #[must_use]
    pub const fn float(self) -> f32 {
        self.float
    }

    /// Returns the resolved borrowed string payload for this key.
    #[must_use]
    pub const fn string(self) -> Option<&'a str> {
        self.string
    }

    /// Returns the resolved audio volume for this key.
    #[must_use]
    pub const fn volume(self) -> f32 {
        self.volume
    }

    /// Returns the resolved audio balance for this key.
    #[must_use]
    pub const fn balance(self) -> f32 {
        self.balance
    }

    /// Iterates retained diagnostics scoped to this emitted event definition.
    pub fn diagnostics(self) -> impl Iterator<Item = &'a Diagnostic> + 'a {
        let event = self.definition.id();
        self.diagnostics
            .iter()
            .filter(move |diagnostic| diagnostic.scope() == DiagnosticScope::Event(event))
    }

    /// Returns whether unsupported data changes this emitted event.
    #[must_use]
    pub fn has_degradations(self) -> bool {
        self.diagnostics().any(Diagnostic::is_degraded)
    }
}

/// Receives borrowed authored event occurrences without requiring storage.
///
/// Implementations must handle an occurrence before returning because its
/// borrowed payload cannot outlive the update call.
pub trait EventSink {
    /// Handles one event in chronological playback order.
    fn event(&mut self, event: AnimationEvent<'_>);
}

impl<F> EventSink for F
where
    F: for<'a> FnMut(AnimationEvent<'a>),
{
    fn event(&mut self, event: AnimationEvent<'_>) {
        self(event);
    }
}

impl EventSink for () {
    fn event(&mut self, _event: AnimationEvent<'_>) {}
}

// Publicly configurable resource limits remain a post-demo concern. This
// fixed ceiling is an internal frame-safety invariant: it prevents one valid
// but extreme delta from turning authored event delivery into an effectively
// unbounded loop.
const MAX_EVENTS_PER_UPDATE: u128 = 65_536;

/// A stateful, renderer-independent one-track animation player.
///
/// A player is permanently bound to the [`Skeleton`] instance passed to
/// [`AnimationPlayer::new`]. Construction allocates all pose and angular
/// branch storage needed by later playback, interruption, and crossfading.
#[derive(Debug)]
pub struct AnimationPlayer {
    instance_key: SkeletonInstanceKey,
    asset: Arc<SkeletonAsset>,
    active: Option<Playback>,
    transition: Option<ActiveTransition>,
    reset_to_setup: bool,
    next_playback_id: u64,
    presented_pose: PoseBuffers,
    transition_source: PoseBuffers,
    angle_branches: AngleBranches,
    skin_revision: u64,
}

impl AnimationPlayer {
    /// Creates an idle player bound to the skeleton's current local pose.
    #[must_use]
    pub fn new(skeleton: &Skeleton) -> Self {
        let mut presented_pose = skeleton.new_pose_buffers();
        skeleton.copy_pose_into(&mut presented_pose);
        let mut transition_source = skeleton.new_pose_buffers();
        transition_source.copy_from(&presented_pose);
        Self {
            instance_key: skeleton.instance_key(),
            asset: Arc::clone(skeleton.asset_handle()),
            active: None,
            transition: None,
            reset_to_setup: false,
            next_playback_id: 1,
            presented_pose,
            transition_source,
            angle_branches: AngleBranches::new(skeleton.asset().bones().len()),
            skin_revision: skeleton.skin_revision(),
        }
    }

    /// Returns the immutable asset used by this player.
    #[must_use]
    pub fn asset(&self) -> &SkeletonAsset {
        &self.asset
    }

    /// Replaces the current animation and starts the new playback at time zero.
    ///
    /// Outgoing authored events stop immediately. The next [`Self::update`]
    /// emits the new target's time-zero events and advances its clock by the
    /// supplied delta. Repeated calls before an update retain only the last
    /// requested target and do not emit events for skipped targets.
    pub fn play(
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

        self.begin_transition(options.transition);
        self.active = Some(Playback {
            id: playback,
            animation,
            animation_index,
            mode: options.mode,
            duration_ticks,
            local_ticks: 0,
            loop_index: 0,
            pending_start: true,
            complete: false,
        });
        self.reset_to_setup = false;

        Ok(PlayOutcome {
            playback,
            interrupted,
        })
    }

    /// Restarts the current animation immediately, if one is active.
    pub fn restart(&mut self) -> Result<Option<PlayOutcome>, PlayerError> {
        let Some(active) = self.active else {
            return Ok(None);
        };
        self.play(
            active.animation,
            PlayOptions {
                mode: active.mode,
                transition: Transition::Immediate,
            },
        )
        .map(Some)
    }

    /// Moves the active playback clock to an absolute elapsed time.
    ///
    /// Once playback clamps at its endpoint. Nonzero looping playback derives
    /// its loop index and local time from the elapsed duration; a zero-duration
    /// loop remains at time zero in loop zero. Seeking preserves playback
    /// identity, mode, and any active crossfade.
    ///
    /// Seeking emits no authored events or lifecycle pulses. The next update
    /// samples from the sought clock and emits only events strictly after that
    /// baseline. Returns `None` while idle or transitioning to setup pose.
    pub fn seek_to(&mut self, elapsed: Duration) -> Option<PlaybackId> {
        let active = self.active.as_mut()?;
        active.seek_to(elapsed);
        Some(active.id)
    }

    /// Stops the current playback and returns toward setup pose.
    ///
    /// The returned ID identifies the playback that was stopped. Pose changes
    /// are applied by the next [`Self::update`]. Calling this while idle or
    /// already stopping is an idempotent no-op.
    pub fn stop(&mut self, transition: Transition) -> Option<PlaybackId> {
        let stopped = self.active.map(|active| active.id)?;
        self.begin_transition(transition);
        self.active = None;
        self.reset_to_setup = true;
        Some(stopped)
    }

    /// Returns a copyable snapshot of current playback state.
    #[must_use]
    pub fn status(&self) -> PlayerStatus {
        let transition_mix = self.transition.map(ActiveTransition::amount);
        match self.active {
            Some(active) => PlayerStatus {
                playback: Some(active.id),
                animation: Some(active.animation),
                mode: Some(active.mode),
                position: Some(Duration::from_nanos(active.local_ticks)),
                loop_index: Some(active.loop_index),
                complete: active.complete,
                transition_mix,
                stopping: false,
            },
            None => PlayerStatus {
                playback: None,
                animation: None,
                mode: None,
                position: None,
                loop_index: None,
                complete: false,
                transition_mix,
                stopping: self.reset_to_setup,
            },
        }
    }

    /// Advances, samples, and crossfades into an editable local pose.
    ///
    /// The skeleton instance is validated before any clock, event, player
    /// buffer, or skeleton pose changes. Authored target events are streamed
    /// using the exact interval `(previous, current]`; a fresh playback also
    /// includes time zero once. Call [`EditablePose::solve`] after scoped
    /// procedural edits to obtain renderer output.
    pub fn update<'s, S: EventSink + ?Sized>(
        &mut self,
        skeleton: &'s mut Skeleton,
        delta: Duration,
        events: &mut S,
    ) -> Result<EditablePose<'s>, PlayerError> {
        let report = self.update_pose_with_time(skeleton, delta, delta, events)?;
        Ok(EditablePose::new(skeleton, report))
    }

    pub(crate) fn validate_update_with_time(
        &self,
        skeleton: &Skeleton,
        playback_delta: Duration,
    ) -> Result<(), PlayerError> {
        if skeleton.instance_key() != self.instance_key {
            return Err(PlayerError::ForeignSkeleton);
        }
        let advance = self
            .active
            .map(|active| active.advance(playback_delta))
            .transpose()?;
        if let Some(advance) = advance {
            Self::validate_event_budget(&self.asset, advance)?;
        }
        Ok(())
    }

    pub(crate) fn update_pose_with_time<S: EventSink + ?Sized>(
        &mut self,
        skeleton: &mut Skeleton,
        playback_delta: Duration,
        transition_delta: Duration,
        events: &mut S,
    ) -> Result<UpdateReport, PlayerError> {
        self.validate_update_with_time(skeleton, playback_delta)?;

        let advance = self
            .active
            .map(|active| active.advance(playback_delta))
            .transpose()?;
        let skin_revision = skeleton.skin_revision();
        if skin_revision != self.skin_revision {
            skeleton.remap_pose_attachments(&mut self.presented_pose);
            skeleton.remap_pose_attachments(&mut self.transition_source);
            self.skin_revision = skin_revision;
        }
        let next_active = advance.map(|advance| advance.next);
        let next_transition = self
            .transition
            .map(|transition| transition.advance(transition_delta));

        match next_active {
            Some(active) => skeleton
                .sample_animation(
                    active.animation,
                    Duration::from_nanos(active.local_ticks),
                    active.mode,
                )
                .map_err(PlayerError::InvalidAnimation)?,
            None if self.reset_to_setup => skeleton.reset_to_setup_pose(),
            None => skeleton.replace_pose_from(&self.presented_pose),
        }

        let mut transition_completed = false;
        if let Some(transition) = next_transition {
            skeleton.blend_pose_from(
                &self.transition_source,
                transition.amount().get(),
                transition.crossfade.discrete.as_blend_switches(),
                transition.crossfade.rotation_path,
                &mut self.angle_branches,
            );
            transition_completed = transition.is_complete();
        }

        skeleton.copy_pose_into(&mut self.presented_pose);

        if let Some(advance) = advance {
            Self::emit_events(&self.asset, advance, events);
        }

        self.active = next_active;
        self.transition = match next_transition {
            Some(transition) if !transition.is_complete() => Some(transition),
            Some(_transition) => None,
            None => None,
        };
        if self.active.is_none() && self.reset_to_setup && self.transition.is_none() {
            self.reset_to_setup = false;
        }

        let report = UpdateReport::new(
            self.active.map(|active| active.id),
            advance.and_then(|advance| advance.completed.then_some(advance.next.id)),
            advance.map_or(0, |advance| advance.loops_completed),
            transition_completed,
        );
        Ok(report)
    }

    fn issue_playback_id(&mut self) -> PlaybackId {
        let value = NonZeroU64::new(self.next_playback_id)
            .expect("the player always skips the zero playback ID");
        self.next_playback_id = self.next_playback_id.wrapping_add(1);
        if self.next_playback_id == 0 {
            self.next_playback_id = 1;
        }
        PlaybackId(value)
    }

    fn begin_transition(&mut self, transition: Transition) {
        self.transition = match transition {
            Transition::Immediate => None,
            Transition::Crossfade(crossfade) if crossfade.duration.is_zero() => None,
            Transition::Crossfade(crossfade) => {
                self.transition_source.copy_from(&self.presented_pose);
                self.angle_branches.reset();
                Some(ActiveTransition {
                    crossfade,
                    elapsed: Duration::ZERO,
                })
            }
        };
    }

    pub(crate) fn emit_events<S: EventSink + ?Sized>(
        asset: &SkeletonAsset,
        advance: Advance,
        events: &mut S,
    ) {
        let animation = asset.animation_data(advance.next.animation_index);
        let Some(frames) = event_frames(animation) else {
            return;
        };
        if frames.is_empty() {
            return;
        }

        if advance.previous.pending_start {
            Self::emit_zero_events(asset, advance.next, frames, 0, events);
        }

        match advance.next.mode {
            PlaybackMode::Once => Self::emit_exclusive_inclusive(
                asset,
                advance.next,
                frames,
                advance.previous.local_ticks,
                advance.next.local_ticks,
                0,
                events,
            ),
            PlaybackMode::Loop if advance.next.duration_ticks == 0 => {}
            PlaybackMode::Loop if advance.loops_completed == 0 => {
                Self::emit_exclusive_inclusive(
                    asset,
                    advance.next,
                    frames,
                    advance.previous.local_ticks,
                    advance.next.local_ticks,
                    advance.previous.loop_index,
                    events,
                );
            }
            PlaybackMode::Loop => {
                let mut cycle = advance.previous.loop_index;
                Self::emit_exclusive_inclusive(
                    asset,
                    advance.next,
                    frames,
                    advance.previous.local_ticks,
                    advance.next.duration_ticks,
                    cycle,
                    events,
                );

                let mut boundaries = advance.loops_completed;
                while boundaries > 0 {
                    cycle += 1;
                    Self::emit_zero_events(asset, advance.next, frames, cycle, events);
                    boundaries -= 1;
                    let upper = if boundaries == 0 {
                        advance.next.local_ticks
                    } else {
                        advance.next.duration_ticks
                    };
                    Self::emit_exclusive_inclusive(
                        asset,
                        advance.next,
                        frames,
                        0,
                        upper,
                        cycle,
                        events,
                    );
                }
            }
        }
    }

    pub(crate) fn validate_event_budget(
        asset: &SkeletonAsset,
        advance: Advance,
    ) -> Result<(), PlayerError> {
        let animation = asset.animation_data(advance.next.animation_index);
        let Some(frames) = event_frames(animation) else {
            return Ok(());
        };
        let count = Self::event_count(advance, frames);
        if count > MAX_EVENTS_PER_UPDATE {
            return Err(PlayerError::EventLimitExceeded {
                limit: MAX_EVENTS_PER_UPDATE as u64,
            });
        }
        Ok(())
    }

    fn event_count(advance: Advance, frames: &[EventFrame]) -> u128 {
        if frames.is_empty() {
            return 0;
        }
        let zero = frames.partition_point(|frame| frame.time.ticks == 0) as u128;
        let mut count = if advance.previous.pending_start {
            zero
        } else {
            0
        };

        match advance.next.mode {
            PlaybackMode::Once => count.saturating_add(Self::event_range_count(
                frames,
                advance.previous.local_ticks,
                advance.next.local_ticks,
            )),
            PlaybackMode::Loop if advance.next.duration_ticks == 0 => count,
            PlaybackMode::Loop if advance.loops_completed == 0 => {
                count.saturating_add(Self::event_range_count(
                    frames,
                    advance.previous.local_ticks,
                    advance.next.local_ticks,
                ))
            }
            PlaybackMode::Loop => {
                count = count.saturating_add(Self::event_range_count(
                    frames,
                    advance.previous.local_ticks,
                    advance.next.duration_ticks,
                ));
                count = count.saturating_add(zero.saturating_mul(advance.loops_completed));
                let complete_nonzero_cycles = advance.loops_completed.saturating_sub(1);
                let full_nonzero = Self::event_range_count(frames, 0, advance.next.duration_ticks);
                count = count.saturating_add(full_nonzero.saturating_mul(complete_nonzero_cycles));
                count.saturating_add(Self::event_range_count(frames, 0, advance.next.local_ticks))
            }
        }
    }

    fn event_range_count(frames: &[EventFrame], lower: u64, upper: u64) -> u128 {
        if upper <= lower {
            return 0;
        }
        let start = frames.partition_point(|frame| frame.time.ticks <= lower);
        let end = frames.partition_point(|frame| frame.time.ticks <= upper);
        (end - start) as u128
    }

    fn emit_zero_events<S: EventSink + ?Sized>(
        asset: &SkeletonAsset,
        playback: Playback,
        frames: &[EventFrame],
        loop_index: u128,
        events: &mut S,
    ) {
        let end = frames.partition_point(|frame| frame.time.ticks == 0);
        for frame in &frames[..end] {
            Self::emit_event(asset, playback, frame, loop_index, events);
        }
    }

    fn emit_exclusive_inclusive<S: EventSink + ?Sized>(
        asset: &SkeletonAsset,
        playback: Playback,
        frames: &[EventFrame],
        lower: u64,
        upper: u64,
        loop_index: u128,
        events: &mut S,
    ) {
        if upper <= lower {
            return;
        }
        let start = frames.partition_point(|frame| frame.time.ticks <= lower);
        let end = frames.partition_point(|frame| frame.time.ticks <= upper);
        for frame in &frames[start..end] {
            Self::emit_event(asset, playback, frame, loop_index, events);
        }
    }

    fn emit_event<S: EventSink + ?Sized>(
        asset: &SkeletonAsset,
        playback: Playback,
        frame: &EventFrame,
        loop_index: u128,
        events: &mut S,
    ) {
        let definition = asset
            .event_definition(EventId::new(asset.key(), frame.event))
            .expect("loaded animation events reference their own asset");
        events.event(AnimationEvent {
            playback: playback.id,
            animation: playback.animation,
            loop_index,
            local_time: frame.time.as_duration(),
            definition,
            integer: frame.payload.integer,
            float: frame.payload.float,
            string: frame.payload.string.as_deref(),
            volume: frame.payload.volume,
            balance: frame.payload.balance,
            diagnostics: asset.diagnostics(),
        });
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Playback {
    pub(crate) id: PlaybackId,
    pub(crate) animation: AnimationId,
    pub(crate) animation_index: usize,
    pub(crate) mode: PlaybackMode,
    pub(crate) duration_ticks: u64,
    pub(crate) local_ticks: u64,
    pub(crate) loop_index: u128,
    pub(crate) pending_start: bool,
    pub(crate) complete: bool,
}

impl Playback {
    fn seek_to(&mut self, elapsed: Duration) {
        self.pending_start = false;
        self.loop_index = 0;
        self.complete = false;

        let elapsed_ticks = elapsed.as_nanos();
        match self.mode {
            PlaybackMode::Once => {
                self.local_ticks =
                    u64::try_from(elapsed_ticks.min(u128::from(self.duration_ticks)))
                        .expect("a clamped animation tick fits in u64");
                self.complete = self.local_ticks == self.duration_ticks;
            }
            PlaybackMode::Loop if self.duration_ticks == 0 => {
                self.local_ticks = 0;
            }
            PlaybackMode::Loop => {
                let duration = u128::from(self.duration_ticks);
                self.local_ticks = u64::try_from(elapsed_ticks % duration)
                    .expect("a wrapped animation tick fits in u64");
                self.loop_index = elapsed_ticks / duration;
            }
        }
    }

    pub(crate) fn advance(self, delta: Duration) -> Result<Advance, PlayerError> {
        let mut next = self;
        next.pending_start = false;

        let (loops_completed, completed) = match self.mode {
            PlaybackMode::Once => {
                let total = u128::from(self.local_ticks) + delta.as_nanos();
                next.local_ticks = u64::try_from(total.min(u128::from(self.duration_ticks)))
                    .expect("a clamped animation tick fits in u64");
                let completed = !self.complete && next.local_ticks == self.duration_ticks;
                next.complete |= completed;
                (0, completed)
            }
            PlaybackMode::Loop if self.duration_ticks == 0 => (0, false),
            PlaybackMode::Loop => {
                let total = u128::from(self.local_ticks) + delta.as_nanos();
                let duration = u128::from(self.duration_ticks);
                let loops_completed = total / duration;
                next.local_ticks =
                    u64::try_from(total % duration).expect("a wrapped animation tick fits in u64");
                next.loop_index = self
                    .loop_index
                    .checked_add(loops_completed)
                    .ok_or(PlayerError::TimeOverflow)?;
                (loops_completed, false)
            }
        };

        Ok(Advance {
            previous: self,
            next,
            loops_completed,
            completed,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Advance {
    pub(crate) previous: Playback,
    pub(crate) next: Playback,
    pub(crate) loops_completed: u128,
    pub(crate) completed: bool,
}

#[derive(Clone, Copy, Debug)]
struct ActiveTransition {
    crossfade: Crossfade,
    elapsed: Duration,
}

impl ActiveTransition {
    fn advance(mut self, delta: Duration) -> Self {
        self.elapsed = self
            .elapsed
            .saturating_add(delta)
            .min(self.crossfade.duration);
        self
    }

    fn amount(self) -> Mix {
        let linear = if self.crossfade.duration.is_zero() {
            1.0
        } else {
            (self.elapsed.as_secs_f64() / self.crossfade.duration.as_secs_f64()) as f32
        };
        Mix::clamped(self.crossfade.curve.apply(linear))
            .expect("finite durations produce a finite normalized mix")
    }

    fn is_complete(self) -> bool {
        self.elapsed >= self.crossfade.duration
    }
}

fn event_frames(animation: &AnimationData) -> Option<&[EventFrame]> {
    animation
        .timelines
        .iter()
        .find_map(|timeline| match timeline {
            TimelineData::Events { frames } => Some(frames.as_ref()),
            _ => None,
        })
}

/// A failure to apply a player command or update.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum PlayerError {
    /// The requested animation belongs to another loaded asset.
    #[error("the requested animation is invalid for this player: {0}")]
    InvalidAnimation(
        #[doc = "The underlying asset-scoped identifier error."]
        #[source]
        IdError,
    ),
    /// The player was updated with a different skeleton instance.
    #[error("the animation player is bound to a different skeleton instance")]
    ForeignSkeleton,
    /// A scaled delta or accumulated loop index could not be represented
    /// exactly.
    #[error("animation time overflowed its exact runtime representation")]
    TimeOverflow,
    /// One update would emit more authored events than the fixed frame-safety
    /// ceiling.
    #[error("one animation update exceeds the authored event limit of {limit}")]
    EventLimitExceeded {
        /// Maximum authored event occurrences accepted by one update.
        limit: u64,
    },
}

impl PlayerError {
    /// Returns the underlying identifier error, when present.
    #[must_use]
    pub const fn id_error(self) -> Option<IdError> {
        match self {
            Self::InvalidAnimation(error) => Some(error),
            Self::ForeignSkeleton | Self::TimeOverflow | Self::EventLimitExceeded { .. } => None,
        }
    }
}

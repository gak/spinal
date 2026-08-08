//! Pure preview state and commands transported toward the future Bevy app.

use std::{error::Error, fmt, num::NonZeroU32, time::Duration};

use crate::{
    clock::{AdvanceBoundary, PlaybackSpeed, ReviewClock},
    command::{PlaybackCommand, StepDirection, ViewerCommand},
};

const NANOS_PER_SECOND: u128 = 1_000_000_000;
const DEFAULT_PREVIEW_FPS: u32 = 30;
const MAX_PREVIEW_FPS: u32 = 1_000_000_000;

/// A positive integer rate for the viewer's preview-time grid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PreviewRate(NonZeroU32);

impl PreviewRate {
    pub(crate) fn from_override(override_fps: Option<u32>) -> Result<Self, InvalidPreviewRate> {
        let fps = override_fps.unwrap_or(DEFAULT_PREVIEW_FPS);
        let fps = NonZeroU32::new(fps).ok_or(InvalidPreviewRate::Zero)?;
        if fps.get() > MAX_PREVIEW_FPS {
            return Err(InvalidPreviewRate::ExceedsNanosecondResolution { fps: fps.get() });
        }
        Ok(Self(fps))
    }

    pub(crate) const fn fps(self) -> u32 {
        self.0.get()
    }

    pub(crate) fn frame_index(self, position: Duration) -> u128 {
        let scaled = position.as_nanos() * u128::from(self.fps());
        if scaled == 0 {
            0
        } else {
            (scaled - 1) / NANOS_PER_SECOND + 1
        }
    }

    /// Derives a timestamp from an absolute frame index.
    ///
    /// Every call evaluates `frame_index / fps` independently with checked
    /// `u128` arithmetic. No rounded `Duration` step is accumulated.
    pub(crate) fn timestamp(self, frame_index: u128) -> Result<Duration, PreviewTimeError> {
        let total_nanos = frame_index
            .checked_mul(NANOS_PER_SECOND)
            .ok_or(PreviewTimeError::Overflow)?
            / u128::from(self.fps());
        duration_from_nanos(total_nanos)
    }

    /// Counts grid points whose exact rational timestamp is below `duration`.
    pub(crate) fn loop_point_count(self, duration: Duration) -> Result<u128, PreviewTimeError> {
        if duration.is_zero() {
            return Ok(0);
        }
        let scaled_duration = duration
            .as_nanos()
            .checked_mul(u128::from(self.fps()))
            .ok_or(PreviewTimeError::Overflow)?;
        (scaled_duration - 1)
            .checked_div(NANOS_PER_SECOND)
            .and_then(|last| last.checked_add(1))
            .ok_or(PreviewTimeError::Overflow)
    }

    pub(crate) fn next_index(
        self,
        position: Duration,
        point_count: u128,
    ) -> Result<u128, PreviewTimeError> {
        let after_position = position
            .as_nanos()
            .checked_add(1)
            .ok_or(PreviewTimeError::Overflow)?;
        let numerator = after_position
            .checked_mul(u128::from(self.fps()))
            .ok_or(PreviewTimeError::Overflow)?;
        let next = ceil_div(numerator, NANOS_PER_SECOND)?;
        Ok(if next >= point_count { 0 } else { next })
    }

    pub(crate) fn previous_index(
        self,
        position: Duration,
        point_count: u128,
    ) -> Result<u128, PreviewTimeError> {
        if position.is_zero() {
            return point_count.checked_sub(1).ok_or(PreviewTimeError::Overflow);
        }
        let numerator = position
            .as_nanos()
            .checked_mul(u128::from(self.fps()))
            .and_then(|value| value.checked_sub(1))
            .ok_or(PreviewTimeError::Overflow)?;
        Ok((numerator / NANOS_PER_SECOND).min(point_count - 1))
    }
}

impl Default for PreviewRate {
    fn default() -> Self {
        Self(NonZeroU32::new(DEFAULT_PREVIEW_FPS).expect("the default preview rate is positive"))
    }
}

/// Returned when `--fps` resolves to zero.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InvalidPreviewRate {
    Zero,
    ExceedsNanosecondResolution { fps: u32 },
}

impl fmt::Display for InvalidPreviewRate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zero => formatter.write_str("preview FPS must be a positive integer"),
            Self::ExceedsNanosecondResolution { fps } => write!(
                formatter,
                "preview FPS {fps} exceeds Duration's 1,000,000,000 FPS nanosecond resolution"
            ),
        }
    }
}

impl Error for InvalidPreviewRate {}

/// A checked preview-grid calculation exceeded the representable range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PreviewTimeError {
    Overflow,
}

impl fmt::Display for PreviewTimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Overflow => formatter.write_str("preview timestamp exceeds Duration range"),
        }
    }
}

impl Error for PreviewTimeError {}

/// The only playback mode used by animation selections in the viewer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SelectionMode {
    Loop,
}

/// The only transition used by animation selections in the viewer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SelectionTransition {
    Immediate,
}

/// A complete request to select or reselect one named animation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SelectionRequest {
    pub(crate) animation_name: Box<str>,
    pub(crate) mode: SelectionMode,
    pub(crate) transition: SelectionTransition,
    pub(crate) start_at: Duration,
    pub(crate) paused: bool,
    /// Authoritative shared-clock mode for the future renderer bridge.
    pub(crate) looping: bool,
    /// Authoritative shared-clock speed for the future renderer bridge.
    pub(crate) playback_speed: PlaybackSpeed,
}

/// A request that atomically pauses and moves to one preview-grid point.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SeekAndPauseRequest {
    pub(crate) animation_name: Box<str>,
    pub(crate) frame_index: u128,
    pub(crate) position: Duration,
}

/// A complete source-independent playback state after a pure model update.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "consumed by the compare renderer in the next slice"
    )
)]
pub(crate) struct PlaybackUpdate {
    pub(crate) animation_name: Box<str>,
    pub(crate) position: Duration,
    pub(crate) paused: bool,
    pub(crate) looping: bool,
    pub(crate) playback_speed: PlaybackSpeed,
}

/// One pure shared-clock effect for both renderer instances to consume.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "consumed by the compare renderer in the next slice"
    )
)]
pub(crate) struct PlaybackEffect {
    pub(crate) update: PlaybackUpdate,
    pub(crate) boundary: AdvanceBoundary,
}

/// A state change for the future Bevy/Spinal integration layer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PreviewEffect {
    Select(SelectionRequest),
    SetPaused {
        paused: bool,
        /// The position that must remain authoritative across the toggle.
        position: Duration,
    },
    SeekAndPause(SeekAndPauseRequest),
    Refit,
}

/// Private, dependency-free playback intent for the read-only viewer.
#[derive(Debug)]
pub(crate) struct PreviewTransport {
    ready: bool,
    animations: Vec<(Box<str>, Duration)>,
    selected: Option<Box<str>>,
    clock: ReviewClock,
}

impl PreviewTransport {
    pub(crate) fn new(rate: PreviewRate) -> Self {
        Self {
            ready: false,
            animations: Vec::new(),
            selected: None,
            clock: ReviewClock::new(rate),
        }
    }

    /// Installs source-order durations and selects the first animation.
    ///
    /// An empty, successfully loaded catalog is ready but has no active
    /// animation. Replacing a catalog establishes a fresh paused preview at
    /// zero so merely loading a source never starts motion.
    pub(crate) fn replace_catalog(
        &mut self,
        animations: impl IntoIterator<Item = (Box<str>, Duration)>,
    ) -> Option<PreviewEffect> {
        self.ready = true;
        self.animations = animations.into_iter().collect();
        self.selected = self
            .animations
            .first()
            .map(|(name, _duration)| name.clone());
        self.clock.reset();
        self.selected
            .as_deref()
            .map(|name| self.selection_effect(name))
    }

    pub(crate) fn mark_unready(&mut self) {
        self.ready = false;
        self.animations.clear();
        self.selected = None;
        self.clock.reset();
    }

    /// Observes the runtime's latest loop-local position while it is playing.
    #[cfg(test)]
    pub(crate) fn observe_position(&mut self, position: Duration) {
        let Some(duration) = self.selected_duration() else {
            return;
        };
        self.clock
            .seek_absolute(position, duration)
            .expect("a Duration constrained by another Duration remains representable");
    }

    pub(crate) fn handle(
        &mut self,
        command: ViewerCommand,
    ) -> Result<Option<PreviewEffect>, PreviewTimeError> {
        match command {
            ViewerCommand::SelectAnimation(name) => Ok(self.select(name)),
            ViewerCommand::TogglePause => Ok(self.toggle_pause()),
            ViewerCommand::Step(direction) => self.step(direction),
            ViewerCommand::Restart => Ok(self.restart()),
            ViewerCommand::Refit => Ok(self.ready.then_some(PreviewEffect::Refit)),
        }
    }

    /// Applies a shared-clock command without involving Bevy input or UI.
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "consumed by the compare renderer in the next slice"
        )
    )]
    pub(crate) fn handle_playback(
        &mut self,
        command: PlaybackCommand,
    ) -> Result<Option<PlaybackEffect>, PreviewTimeError> {
        match command {
            PlaybackCommand::SetPaused(paused) => {
                if !self.apply_paused(paused) {
                    return Ok(None);
                }
                Ok(self.playback_effect(AdvanceBoundary::None))
            }
            PlaybackCommand::SetLooping(looping) => {
                let Some(duration) = self.selected_duration().filter(|_duration| self.ready) else {
                    return Ok(None);
                };
                self.clock.set_looping(looping, duration)?;
                Ok(self.playback_effect(AdvanceBoundary::None))
            }
            PlaybackCommand::SetPlaybackSpeed(speed) => {
                self.clock.set_playback_speed(speed);
                Ok(self.playback_effect(AdvanceBoundary::None))
            }
            PlaybackCommand::SeekAbsolute(position) => {
                let Some(duration) = self.selected_duration().filter(|_duration| self.ready) else {
                    return Ok(None);
                };
                self.clock.seek_absolute(position, duration)?;
                Ok(self.playback_effect(AdvanceBoundary::None))
            }
            PlaybackCommand::Advance(delta) => {
                let Some(duration) = self.selected_duration().filter(|_duration| self.ready) else {
                    return Ok(None);
                };
                let advance = self.clock.advance(delta, duration)?;
                Ok(self.playback_effect(advance.boundary))
            }
        }
    }

    pub(crate) const fn is_ready(&self) -> bool {
        self.ready
    }

    pub(crate) fn selected_animation(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    pub(crate) const fn position(&self) -> Duration {
        self.clock.position()
    }

    pub(crate) const fn is_paused(&self) -> bool {
        self.clock.is_paused()
    }

    pub(crate) const fn rate(&self) -> PreviewRate {
        self.clock.rate()
    }

    pub(crate) fn frame_index(&self) -> u128 {
        self.clock.frame_index()
    }

    /// Projects shared time into one source-local animation duration.
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "consumed by the compare renderer in the next slice"
        )
    )]
    pub(crate) fn projected_position(
        &self,
        duration: Duration,
    ) -> Result<Duration, PreviewTimeError> {
        self.clock.projected_position(duration)
    }

    fn select(&mut self, name: Box<str>) -> Option<PreviewEffect> {
        if !self.ready || self.animation_duration(&name).is_none() {
            return None;
        }
        self.selected = Some(name.clone());
        self.clock.restart();
        Some(self.selection_effect(&name))
    }

    fn restart(&mut self) -> Option<PreviewEffect> {
        let name = self.selected.clone()?;
        if !self.ready {
            return None;
        }
        self.clock.restart();
        Some(self.selection_effect(&name))
    }

    fn toggle_pause(&mut self) -> Option<PreviewEffect> {
        let paused = !self.clock.is_paused();
        if !self.apply_paused(paused) {
            return None;
        }
        Some(PreviewEffect::SetPaused {
            paused: self.clock.is_paused(),
            position: self.clock.position(),
        })
    }

    fn step(
        &mut self,
        direction: StepDirection,
    ) -> Result<Option<PreviewEffect>, PreviewTimeError> {
        let Some(animation_name) = self.selected.clone().filter(|_name| self.ready) else {
            return Ok(None);
        };
        let duration = self
            .animation_duration(&animation_name)
            .expect("a selected animation belongs to the current catalog");
        let Some(step) = self.clock.step(direction, duration)? else {
            return Ok(Some(PreviewEffect::SetPaused {
                paused: true,
                position: Duration::ZERO,
            }));
        };
        Ok(Some(PreviewEffect::SeekAndPause(SeekAndPauseRequest {
            animation_name,
            frame_index: step.frame_index,
            position: step.position,
        })))
    }

    fn selection_effect(&self, animation_name: &str) -> PreviewEffect {
        PreviewEffect::Select(SelectionRequest {
            animation_name: animation_name.into(),
            mode: SelectionMode::Loop,
            transition: SelectionTransition::Immediate,
            start_at: Duration::ZERO,
            paused: self.clock.is_paused(),
            looping: self.clock.is_looping(),
            playback_speed: self.clock.playback_speed(),
        })
    }

    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "consumed by the compare renderer in the next slice"
        )
    )]
    fn playback_update(&self) -> Option<PlaybackUpdate> {
        Some(PlaybackUpdate {
            animation_name: self.selected.clone()?,
            position: self.clock.position(),
            paused: self.clock.is_paused(),
            looping: self.clock.is_looping(),
            playback_speed: self.clock.playback_speed(),
        })
    }

    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "consumed by the compare renderer in the next slice"
        )
    )]
    fn playback_effect(&self, boundary: AdvanceBoundary) -> Option<PlaybackEffect> {
        self.playback_update()
            .map(|update| PlaybackEffect { update, boundary })
    }

    fn apply_paused(&mut self, paused: bool) -> bool {
        let Some(duration) = self.selected_duration().filter(|_duration| self.ready) else {
            return false;
        };
        let paused = paused || duration.is_zero();
        if !paused && !self.clock.is_looping() && self.clock.position() >= duration {
            self.clock.restart();
        }
        self.clock.set_paused(paused);
        true
    }

    fn selected_duration(&self) -> Option<Duration> {
        self.selected
            .as_deref()
            .and_then(|name| self.animation_duration(name))
    }

    fn animation_duration(&self, name: &str) -> Option<Duration> {
        self.animations
            .iter()
            .find_map(|(candidate, duration)| (candidate.as_ref() == name).then_some(*duration))
    }
}

fn ceil_div(numerator: u128, denominator: u128) -> Result<u128, PreviewTimeError> {
    if numerator == 0 {
        return Ok(0);
    }
    numerator
        .checked_sub(1)
        .and_then(|value| value.checked_div(denominator))
        .and_then(|value| value.checked_add(1))
        .ok_or(PreviewTimeError::Overflow)
}

pub(crate) fn duration_from_nanos(total_nanos: u128) -> Result<Duration, PreviewTimeError> {
    let seconds = total_nanos / NANOS_PER_SECOND;
    let subsecond_nanos = total_nanos % NANOS_PER_SECOND;
    let seconds = u64::try_from(seconds).map_err(|_error| PreviewTimeError::Overflow)?;
    let subsecond_nanos =
        u32::try_from(subsecond_nanos).map_err(|_error| PreviewTimeError::Overflow)?;
    Ok(Duration::new(seconds, subsecond_nanos))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{StepDirection, ViewerCommand};

    fn duration_ms(milliseconds: u64) -> Duration {
        Duration::from_millis(milliseconds)
    }

    fn ready_transport(durations: impl IntoIterator<Item = Duration>) -> PreviewTransport {
        let mut transport = PreviewTransport::new(PreviewRate::default());
        transport.replace_catalog(
            durations
                .into_iter()
                .enumerate()
                .map(|(index, duration)| (format!("animation-{index}").into(), duration)),
        );
        transport
    }

    fn expect_selection(effect: Option<PreviewEffect>) -> SelectionRequest {
        match effect {
            Some(PreviewEffect::Select(request)) => request,
            other => panic!("expected selection, got {other:?}"),
        }
    }

    fn expect_seek(effect: Option<PreviewEffect>) -> SeekAndPauseRequest {
        match effect {
            Some(PreviewEffect::SeekAndPause(request)) => request,
            other => panic!("expected seek-and-pause, got {other:?}"),
        }
    }

    fn expect_playback_effect(effect: Option<PlaybackEffect>) -> PlaybackEffect {
        effect.unwrap_or_else(|| panic!("expected playback effect"))
    }

    #[test]
    fn preview_rate_defaults_to_thirty_and_accepts_positive_override() {
        assert_eq!(PreviewRate::from_override(None).unwrap().fps(), 30);
        assert_eq!(PreviewRate::from_override(Some(60)).unwrap().fps(), 60);
        assert_eq!(
            PreviewRate::from_override(Some(0)),
            Err(InvalidPreviewRate::Zero)
        );
    }

    #[test]
    fn preview_rate_rejects_values_finer_than_duration_nanoseconds() {
        assert_eq!(
            PreviewRate::from_override(Some(MAX_PREVIEW_FPS))
                .unwrap()
                .fps(),
            MAX_PREVIEW_FPS
        );
        assert_eq!(
            PreviewRate::from_override(Some(MAX_PREVIEW_FPS + 1)),
            Err(InvalidPreviewRate::ExceedsNanosecondResolution {
                fps: MAX_PREVIEW_FPS + 1,
            })
        );
    }

    #[test]
    fn displayed_frame_index_uses_the_same_absolute_rational_grid() {
        let rate = PreviewRate::default();
        assert_eq!(rate.frame_index(Duration::from_nanos(66_666_666)), 2);
        assert_eq!(rate.frame_index(Duration::from_secs(1)), 30);
    }

    #[test]
    fn timestamps_come_from_absolute_integer_indices_without_drift() {
        let rate = PreviewRate::default();

        assert_eq!(rate.timestamp(1).unwrap(), Duration::from_nanos(33_333_333));
        assert_eq!(rate.timestamp(2).unwrap(), Duration::from_nanos(66_666_666));
        assert_eq!(rate.timestamp(30).unwrap(), Duration::from_secs(1));
        assert_eq!(rate.timestamp(u128::MAX), Err(PreviewTimeError::Overflow));
    }

    #[test]
    fn selections_reselections_and_restart_are_looping_immediate_and_keep_pause() {
        let mut transport = ready_transport([duration_ms(100), duration_ms(250)]);

        for command in [
            ViewerCommand::SelectAnimation("animation-1".into()),
            ViewerCommand::SelectAnimation("animation-1".into()),
            ViewerCommand::Restart,
        ] {
            let request = expect_selection(transport.handle(command).unwrap());
            assert_eq!(request.animation_name.as_ref(), "animation-1");
            assert_eq!(request.mode, SelectionMode::Loop);
            assert_eq!(request.transition, SelectionTransition::Immediate);
            assert_eq!(request.start_at, Duration::ZERO);
            assert!(request.paused);
            assert!(request.looping);
            assert_eq!(request.playback_speed, PlaybackSpeed::NORMAL);
            assert!(transport.is_paused());
        }
    }

    #[test]
    fn explicit_pause_and_play_are_idempotent_and_keep_position() {
        let mut transport = ready_transport([duration_ms(100)]);
        transport.observe_position(duration_ms(40));

        for paused in [true, true, false, false] {
            let effect = expect_playback_effect(
                transport
                    .handle_playback(PlaybackCommand::SetPaused(paused))
                    .unwrap(),
            );
            assert_eq!(effect.update.position, duration_ms(40));
            assert_eq!(effect.update.paused, paused);
            assert_eq!(effect.boundary, AdvanceBoundary::None);
        }
    }

    #[test]
    fn zero_duration_transport_cannot_be_put_into_playing_state() {
        let mut transport = ready_transport([Duration::ZERO]);

        for _attempt in 0..2 {
            let effect = expect_playback_effect(
                transport
                    .handle_playback(PlaybackCommand::SetPaused(false))
                    .unwrap(),
            );
            assert_eq!(effect.update.position, Duration::ZERO);
            assert!(effect.update.paused);
        }
    }

    #[test]
    fn speed_loop_seek_and_advance_form_one_authoritative_update() {
        let mut transport = ready_transport([duration_ms(100)]);

        let speed = expect_playback_effect(
            transport
                .handle_playback(PlaybackCommand::set_playback_speed(2.0).unwrap())
                .unwrap(),
        );
        assert_eq!(speed.update.playback_speed.multiplier(), 2.0);
        assert!(speed.update.looping);

        let sought = expect_playback_effect(
            transport
                .handle_playback(PlaybackCommand::SeekAbsolute(duration_ms(90)))
                .unwrap(),
        );
        assert_eq!(sought.update.position, duration_ms(90));
        transport
            .handle_playback(PlaybackCommand::SetPaused(false))
            .unwrap()
            .expect("play effect");
        let wrapped = expect_playback_effect(
            transport
                .handle_playback(PlaybackCommand::Advance(duration_ms(10)))
                .unwrap(),
        );
        assert_eq!(wrapped.update.position, duration_ms(10));
        assert!(!wrapped.update.paused);
        assert_eq!(wrapped.boundary, AdvanceBoundary::Wrapped);

        let bounded = expect_playback_effect(
            transport
                .handle_playback(PlaybackCommand::SetLooping(false))
                .unwrap(),
        );
        assert!(!bounded.update.looping);
        let end = expect_playback_effect(
            transport
                .handle_playback(PlaybackCommand::SeekAbsolute(duration_ms(150)))
                .unwrap(),
        );
        assert_eq!(end.update.position, duration_ms(100));
        assert!(end.update.paused);

        let replay = expect_playback_effect(
            transport
                .handle_playback(PlaybackCommand::SetPaused(false))
                .unwrap(),
        );
        assert_eq!(replay.update.position, Duration::ZERO);
        assert!(!replay.update.paused);
        let ended = expect_playback_effect(
            transport
                .handle_playback(PlaybackCommand::Advance(duration_ms(60)))
                .unwrap(),
        );
        assert_eq!(ended.update.position, duration_ms(100));
        assert!(ended.update.paused);
        assert_eq!(ended.boundary, AdvanceBoundary::Completed);
    }

    #[test]
    fn clock_operations_are_inert_without_a_ready_selected_animation() {
        let mut transport = PreviewTransport::new(PreviewRate::default());

        for command in [
            PlaybackCommand::SeekAbsolute(duration_ms(50)),
            PlaybackCommand::Advance(duration_ms(50)),
            PlaybackCommand::SetLooping(false),
            PlaybackCommand::SetPaused(false),
            PlaybackCommand::set_playback_speed(2.0).unwrap(),
        ] {
            assert_eq!(transport.handle_playback(command).unwrap(), None);
        }
    }

    #[test]
    fn animation_selection_is_independent_of_catalog_source_order() {
        for catalog in [
            [("walk", duration_ms(100)), ("idle", duration_ms(250))],
            [("idle", duration_ms(250)), ("walk", duration_ms(100))],
        ] {
            let mut transport = PreviewTransport::new(PreviewRate::default());
            transport.replace_catalog(
                catalog
                    .into_iter()
                    .map(|(name, duration)| (name.into(), duration)),
            );

            let request = expect_selection(
                transport
                    .handle(ViewerCommand::SelectAnimation("idle".into()))
                    .unwrap(),
            );

            assert_eq!(request.animation_name.as_ref(), "idle");
            assert_eq!(transport.selected_animation(), Some("idle"));
        }
    }

    #[test]
    fn arrows_pause_and_pick_adjacent_grid_points_without_duration_addition() {
        let mut transport = ready_transport([duration_ms(100)]);
        transport.observe_position(duration_ms(50));

        let backward = expect_seek(
            transport
                .handle(ViewerCommand::Step(StepDirection::Backward))
                .unwrap(),
        );
        assert_eq!(backward.animation_name.as_ref(), "animation-0");
        assert_eq!(backward.frame_index, 1);
        assert_eq!(backward.position, Duration::from_nanos(33_333_333));
        assert!(transport.is_paused());

        transport.observe_position(duration_ms(50));
        let forward = expect_seek(
            transport
                .handle(ViewerCommand::Step(StepDirection::Forward))
                .unwrap(),
        );
        assert_eq!(forward.frame_index, 2);
        assert_eq!(forward.position, Duration::from_nanos(66_666_666));
        assert!(transport.is_paused());
    }

    #[test]
    fn loop_grid_points_are_strictly_below_duration_and_wrap_both_ways() {
        let mut transport = ready_transport([duration_ms(100)]);
        transport.observe_position(Duration::from_nanos(66_666_666));

        let wrapped_forward = expect_seek(
            transport
                .handle(ViewerCommand::Step(StepDirection::Forward))
                .unwrap(),
        );
        assert_eq!(wrapped_forward.frame_index, 0);
        assert_eq!(wrapped_forward.position, Duration::ZERO);

        let wrapped_backward = expect_seek(
            transport
                .handle(ViewerCommand::Step(StepDirection::Backward))
                .unwrap(),
        );
        assert_eq!(wrapped_backward.frame_index, 2);
        assert_eq!(wrapped_backward.position, Duration::from_nanos(66_666_666));
        assert!(wrapped_backward.position < duration_ms(100));
    }

    #[test]
    fn zero_duration_pauses_at_zero_without_inventing_a_loop_point() {
        let mut transport = ready_transport([Duration::ZERO]);

        let effect = transport
            .handle(ViewerCommand::Step(StepDirection::Forward))
            .unwrap();

        assert_eq!(
            effect,
            Some(PreviewEffect::SetPaused {
                paused: true,
                position: Duration::ZERO,
            })
        );
        assert!(transport.is_paused());
        assert_eq!(transport.position(), Duration::ZERO);
    }

    #[test]
    fn sub_frame_loop_has_one_zero_time_point_for_both_directions() {
        let mut transport = ready_transport([duration_ms(1)]);

        for direction in [StepDirection::Forward, StepDirection::Backward] {
            let request = expect_seek(transport.handle(ViewerCommand::Step(direction)).unwrap());
            assert_eq!(request.frame_index, 0);
            assert_eq!(request.position, Duration::ZERO);
        }
    }

    #[test]
    fn space_resumes_from_the_last_sought_timestamp() {
        let mut transport = ready_transport([duration_ms(100)]);
        let sought = expect_seek(
            transport
                .handle(ViewerCommand::Step(StepDirection::Forward))
                .unwrap(),
        );

        let resumed = transport
            .handle(ViewerCommand::TogglePause)
            .unwrap()
            .expect("resume effect");

        assert_eq!(
            resumed,
            PreviewEffect::SetPaused {
                paused: false,
                position: sought.position,
            }
        );
        assert_eq!(transport.position(), sought.position);
        assert!(!transport.is_paused());
    }

    #[test]
    fn steps_are_ignored_until_ready_and_when_catalog_has_no_animations() {
        let mut transport = PreviewTransport::new(PreviewRate::default());
        assert_eq!(
            transport
                .handle(ViewerCommand::Step(StepDirection::Forward))
                .unwrap(),
            None
        );

        assert_eq!(transport.replace_catalog([]), None);
        assert!(transport.is_ready());
        assert_eq!(
            transport
                .handle(ViewerCommand::Step(StepDirection::Backward))
                .unwrap(),
            None
        );
    }

    #[test]
    fn refit_is_forwarded_only_after_load_readiness() {
        let mut transport = PreviewTransport::new(PreviewRate::default());
        assert_eq!(transport.rate().fps(), 30);
        assert_eq!(transport.handle(ViewerCommand::Refit).unwrap(), None);

        transport.replace_catalog([]);
        assert_eq!(
            transport.handle(ViewerCommand::Refit).unwrap(),
            Some(PreviewEffect::Refit)
        );
    }

    #[test]
    fn unknown_or_unready_selection_does_not_change_transport_state() {
        let mut transport = ready_transport([duration_ms(100)]);
        transport.observe_position(duration_ms(50));
        assert_eq!(
            transport
                .handle(ViewerCommand::SelectAnimation("missing".into()))
                .unwrap(),
            None
        );
        assert_eq!(transport.selected_animation(), Some("animation-0"));
        assert_eq!(transport.position(), duration_ms(50));

        transport.mark_unready();
        assert_eq!(
            transport
                .handle(ViewerCommand::SelectAnimation("animation-0".into()))
                .unwrap(),
            None
        );
    }
}

//! Pure preview state and commands transported toward the future Bevy app.

use std::{error::Error, fmt, num::NonZeroU32, time::Duration};

use crate::{
    clock::ReviewClock,
    command::{StepDirection, ViewerCommand},
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

/// A complete request to select or reselect one source-order animation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SelectionRequest {
    pub(crate) animation_index: usize,
    pub(crate) mode: SelectionMode,
    pub(crate) transition: SelectionTransition,
    pub(crate) start_at: Duration,
    pub(crate) paused: bool,
}

/// A request that atomically pauses and moves to one preview-grid point.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SeekAndPauseRequest {
    pub(crate) animation_index: usize,
    pub(crate) frame_index: u128,
    pub(crate) position: Duration,
}

/// A state change for the future Bevy/Spinal integration layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
    animation_durations: Vec<Duration>,
    selected: Option<usize>,
    clock: ReviewClock,
}

impl PreviewTransport {
    pub(crate) fn new(rate: PreviewRate) -> Self {
        Self {
            ready: false,
            animation_durations: Vec::new(),
            selected: None,
            clock: ReviewClock::new(rate),
        }
    }

    /// Installs source-order durations and selects the first animation.
    ///
    /// An empty, successfully loaded catalog is ready but has no active
    /// animation. Replacing a catalog establishes a fresh running preview.
    pub(crate) fn replace_catalog(
        &mut self,
        durations: impl IntoIterator<Item = Duration>,
    ) -> Option<PreviewEffect> {
        self.ready = true;
        self.animation_durations = durations.into_iter().collect();
        self.selected = (!self.animation_durations.is_empty()).then_some(0);
        self.clock.reset();
        self.selected.map(|index| self.selection_effect(index))
    }

    pub(crate) fn mark_unready(&mut self) {
        self.ready = false;
        self.animation_durations.clear();
        self.selected = None;
        self.clock.reset();
    }

    /// Observes the runtime's latest loop-local position while it is playing.
    pub(crate) fn observe_position(&mut self, position: Duration) {
        let Some(duration) = self.selected_duration() else {
            return;
        };
        self.clock.observe_position(position);
        self.clock
            .normalize_loop_position(duration)
            .expect("a Duration normalized by another Duration remains representable");
    }

    pub(crate) fn handle(
        &mut self,
        command: ViewerCommand,
    ) -> Result<Option<PreviewEffect>, PreviewTimeError> {
        match command {
            ViewerCommand::SelectAnimation(index) => Ok(self.select(index)),
            ViewerCommand::TogglePause => Ok(self.toggle_pause()),
            ViewerCommand::Step(direction) => self.step(direction),
            ViewerCommand::Restart => Ok(self.restart()),
            ViewerCommand::Refit => Ok(self.ready.then_some(PreviewEffect::Refit)),
        }
    }

    pub(crate) const fn is_ready(&self) -> bool {
        self.ready
    }

    pub(crate) const fn selected_animation(&self) -> Option<usize> {
        self.selected
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

    fn select(&mut self, index: usize) -> Option<PreviewEffect> {
        if !self.ready || index >= self.animation_durations.len() {
            return None;
        }
        self.selected = Some(index);
        self.clock.restart();
        Some(self.selection_effect(index))
    }

    fn restart(&mut self) -> Option<PreviewEffect> {
        let index = self.selected?;
        if !self.ready {
            return None;
        }
        self.clock.restart();
        Some(self.selection_effect(index))
    }

    fn toggle_pause(&mut self) -> Option<PreviewEffect> {
        self.selected?;
        if !self.ready {
            return None;
        }
        let paused = self.clock.toggle_paused();
        Some(PreviewEffect::SetPaused {
            paused,
            position: self.clock.position(),
        })
    }

    fn step(
        &mut self,
        direction: StepDirection,
    ) -> Result<Option<PreviewEffect>, PreviewTimeError> {
        let Some(animation_index) = self.selected.filter(|_index| self.ready) else {
            return Ok(None);
        };
        let duration = self.animation_durations[animation_index];
        let Some(step) = self.clock.step_looping(direction, duration)? else {
            return Ok(Some(PreviewEffect::SetPaused {
                paused: true,
                position: Duration::ZERO,
            }));
        };
        Ok(Some(PreviewEffect::SeekAndPause(SeekAndPauseRequest {
            animation_index,
            frame_index: step.frame_index,
            position: step.position,
        })))
    }

    fn selection_effect(&self, animation_index: usize) -> PreviewEffect {
        PreviewEffect::Select(SelectionRequest {
            animation_index,
            mode: SelectionMode::Loop,
            transition: SelectionTransition::Immediate,
            start_at: Duration::ZERO,
            paused: self.clock.is_paused(),
        })
    }

    fn selected_duration(&self) -> Option<Duration> {
        self.selected
            .and_then(|index| self.animation_durations.get(index))
            .copied()
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
    use crate::command::{StepDirection, ViewerCommand, command_for_digit};

    fn duration_ms(milliseconds: u64) -> Duration {
        Duration::from_millis(milliseconds)
    }

    fn ready_transport(durations: impl IntoIterator<Item = Duration>) -> PreviewTransport {
        let mut transport = PreviewTransport::new(PreviewRate::default());
        transport.replace_catalog(durations);
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
        transport
            .handle(ViewerCommand::TogglePause)
            .unwrap()
            .expect("pause effect");

        for command in [
            ViewerCommand::SelectAnimation(1),
            ViewerCommand::SelectAnimation(1),
            ViewerCommand::Restart,
        ] {
            let request = expect_selection(transport.handle(command).unwrap());
            assert_eq!(request.animation_index, 1);
            assert_eq!(request.mode, SelectionMode::Loop);
            assert_eq!(request.transition, SelectionTransition::Immediate);
            assert_eq!(request.start_at, Duration::ZERO);
            assert!(request.paused);
            assert!(transport.is_paused());
        }
    }

    #[test]
    fn zero_digit_selects_tenth_animation_in_source_order() {
        let mut transport = ready_transport((1..=12).map(Duration::from_secs));
        let command = command_for_digit(0).expect("zero has a stable binding");

        let request = expect_selection(transport.handle(command).unwrap());

        assert_eq!(request.animation_index, 9);
        assert_eq!(transport.selected_animation(), Some(9));
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
    fn invalid_or_unready_selection_does_not_change_transport_state() {
        let mut transport = ready_transport([duration_ms(100)]);
        assert_eq!(
            transport.handle(ViewerCommand::SelectAnimation(9)).unwrap(),
            None
        );
        assert_eq!(transport.selected_animation(), Some(0));

        transport.mark_unready();
        assert_eq!(
            transport.handle(ViewerCommand::SelectAnimation(0)).unwrap(),
            None
        );
    }
}

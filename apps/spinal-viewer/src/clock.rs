//! Pure review-clock state shared by preview and future comparison sessions.

use std::time::Duration;

use crate::{
    command::StepDirection,
    preview::{PreviewRate, PreviewTimeError, duration_from_nanos},
};

/// One exact point on the configured preview-time grid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ClockStep {
    pub(crate) frame_index: u128,
    pub(crate) position: Duration,
}

/// Dependency-free playback clock for a viewer or synchronized review session.
///
/// Source selection and animation catalogs deliberately remain outside this
/// type. That lets one clock drive one or two sources without giving either
/// source ownership of review time.
#[derive(Debug)]
pub(crate) struct ReviewClock {
    rate: PreviewRate,
    position: Duration,
    paused: bool,
}

impl ReviewClock {
    pub(crate) const fn new(rate: PreviewRate) -> Self {
        Self {
            rate,
            position: Duration::ZERO,
            paused: false,
        }
    }

    /// Establishes a fresh running review at time zero.
    pub(crate) fn reset(&mut self) {
        self.position = Duration::ZERO;
        self.paused = false;
    }

    /// Records an observed absolute position without assigning it to a source.
    pub(crate) fn observe_position(&mut self, position: Duration) {
        self.position = position;
    }

    /// Normalizes the observed position for today's single looping source.
    ///
    /// Compare mode will instead advance this clock directly and project its
    /// absolute position into each source independently.
    pub(crate) fn normalize_loop_position(
        &mut self,
        duration: Duration,
    ) -> Result<(), PreviewTimeError> {
        self.position = loop_position(self.position, duration)?;
        Ok(())
    }

    /// Moves to the beginning without changing the pause state.
    pub(crate) fn restart(&mut self) {
        self.position = Duration::ZERO;
    }

    /// Toggles playback and returns the resulting pause state.
    pub(crate) fn toggle_paused(&mut self) -> bool {
        self.paused = !self.paused;
        self.paused
    }

    /// Pauses and selects the adjacent exact grid point in a looping extent.
    ///
    /// A zero-duration extent has no grid point and therefore returns `None`
    /// while still establishing the safe paused-at-zero state.
    pub(crate) fn step_looping(
        &mut self,
        direction: StepDirection,
        duration: Duration,
    ) -> Result<Option<ClockStep>, PreviewTimeError> {
        self.paused = true;
        let point_count = self.rate.loop_point_count(duration)?;
        if point_count == 0 {
            self.position = Duration::ZERO;
            return Ok(None);
        }

        let frame_index = match direction {
            StepDirection::Backward => self.rate.previous_index(self.position, point_count)?,
            StepDirection::Forward => self.rate.next_index(self.position, point_count)?,
        };
        let position = self.rate.timestamp(frame_index)?;
        debug_assert!(position < duration);
        self.position = position;
        Ok(Some(ClockStep {
            frame_index,
            position,
        }))
    }

    pub(crate) const fn position(&self) -> Duration {
        self.position
    }

    pub(crate) const fn is_paused(&self) -> bool {
        self.paused
    }

    pub(crate) const fn rate(&self) -> PreviewRate {
        self.rate
    }

    pub(crate) fn frame_index(&self) -> u128 {
        self.rate.frame_index(self.position)
    }
}

fn loop_position(position: Duration, duration: Duration) -> Result<Duration, PreviewTimeError> {
    if duration.is_zero() {
        return Ok(Duration::ZERO);
    }
    duration_from_nanos(position.as_nanos() % duration.as_nanos())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_clock_is_running_at_zero_on_the_shared_rate() {
        let clock = ReviewClock::new(PreviewRate::from_override(Some(60)).unwrap());

        assert_eq!(clock.position(), Duration::ZERO);
        assert_eq!(clock.frame_index(), 0);
        assert_eq!(clock.rate().fps(), 60);
        assert!(!clock.is_paused());
    }

    #[test]
    fn observation_and_restart_preserve_pause_intent() {
        let mut clock = ReviewClock::new(PreviewRate::default());
        clock.observe_position(Duration::from_millis(75));
        assert!(clock.toggle_paused());

        clock.restart();

        assert_eq!(clock.position(), Duration::ZERO);
        assert!(clock.is_paused());
    }

    #[test]
    fn single_source_compatibility_normalizes_loop_local_time() {
        let mut clock = ReviewClock::new(PreviewRate::default());
        clock.observe_position(Duration::from_millis(275));

        clock
            .normalize_loop_position(Duration::from_millis(100))
            .unwrap();

        assert_eq!(clock.position(), Duration::from_millis(75));
    }

    #[test]
    fn exact_steps_wrap_without_accumulating_rounded_durations() {
        let mut clock = ReviewClock::new(PreviewRate::default());
        clock.observe_position(Duration::from_nanos(66_666_666));

        let forward = clock
            .step_looping(StepDirection::Forward, Duration::from_millis(100))
            .unwrap()
            .unwrap();
        assert_eq!(forward.frame_index, 0);
        assert_eq!(forward.position, Duration::ZERO);

        let backward = clock
            .step_looping(StepDirection::Backward, Duration::from_millis(100))
            .unwrap()
            .unwrap();
        assert_eq!(backward.frame_index, 2);
        assert_eq!(backward.position, Duration::from_nanos(66_666_666));
        assert!(clock.is_paused());
    }

    #[test]
    fn zero_duration_has_no_invented_frame_and_is_paused_at_zero() {
        let mut clock = ReviewClock::new(PreviewRate::default());
        clock.observe_position(Duration::from_secs(1));

        let step = clock
            .step_looping(StepDirection::Forward, Duration::ZERO)
            .unwrap();

        assert_eq!(step, None);
        assert_eq!(clock.position(), Duration::ZERO);
        assert!(clock.is_paused());
    }

    #[test]
    fn reset_establishes_a_fresh_running_review() {
        let mut clock = ReviewClock::new(PreviewRate::default());
        clock.observe_position(Duration::from_secs(2));
        clock.toggle_paused();

        clock.reset();

        assert_eq!(clock.position(), Duration::ZERO);
        assert!(!clock.is_paused());
    }
}

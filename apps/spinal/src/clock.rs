//! Pure review-clock state shared by preview and future comparison sessions.

use std::{error::Error, fmt, time::Duration};

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

/// A positive, finite multiplier for animation-clock advancement.
///
/// The exact `f32` bits are retained because Spinal's animation mixer uses
/// those bits to scale wall-clock deltas without floating-point drift.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PlaybackSpeed(u32);

#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "consumed by the compare renderer in the next slice"
    )
)]
impl PlaybackSpeed {
    pub(crate) const NORMAL: Self = Self(1.0_f32.to_bits());

    pub(crate) fn new(multiplier: f32) -> Result<Self, InvalidPlaybackSpeed> {
        if !multiplier.is_finite() {
            return Err(InvalidPlaybackSpeed::NonFinite);
        }
        if multiplier <= 0.0 {
            return Err(InvalidPlaybackSpeed::NotPositive);
        }
        Ok(Self(multiplier.to_bits()))
    }

    pub(crate) const fn multiplier(self) -> f32 {
        f32::from_bits(self.0)
    }
}

/// Stable reason a requested review speed was rejected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "consumed by the compare renderer in the next slice"
    )
)]
pub(crate) enum InvalidPlaybackSpeed {
    NonFinite,
    NotPositive,
}

impl fmt::Display for InvalidPlaybackSpeed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite => formatter.write_str("playback speed must be finite"),
            Self::NotPositive => formatter.write_str("playback speed must be greater than zero"),
        }
    }
}

impl Error for InvalidPlaybackSpeed {}

/// One complete authoritative clock state after an operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ClockState {
    pub(crate) position: Duration,
    pub(crate) paused: bool,
}

/// Boundary crossed by one authoritative clock advance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "consumed by the compare renderer in the next slice"
    )
)]
pub(crate) enum AdvanceBoundary {
    None,
    Wrapped,
    Completed,
    Empty,
}

/// State and boundary facts produced by exactly one host delta.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "consumed by the compare renderer in the next slice"
    )
)]
pub(crate) struct ClockAdvance {
    pub(crate) state: ClockState,
    pub(crate) boundary: AdvanceBoundary,
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
    looping: bool,
    playback_speed: PlaybackSpeed,
}

impl ReviewClock {
    pub(crate) const fn new(rate: PreviewRate) -> Self {
        Self {
            rate,
            position: Duration::ZERO,
            paused: true,
            looping: true,
            playback_speed: PlaybackSpeed::NORMAL,
        }
    }

    /// Establishes a fresh, non-autoplaying review at time zero.
    pub(crate) fn reset(&mut self) {
        self.position = Duration::ZERO;
        self.paused = true;
    }

    /// Moves to the beginning without changing the pause state.
    pub(crate) fn restart(&mut self) {
        self.position = Duration::ZERO;
    }

    /// Establishes an explicit playback state and returns the effective state.
    ///
    /// Calling this repeatedly with the same value is intentionally
    /// idempotent. Extent-specific constraints, such as a zero-duration
    /// animation being unable to play, are applied by the transport.
    pub(crate) fn set_paused(&mut self, paused: bool) -> bool {
        self.paused = paused;
        paused
    }

    /// Changes the review boundary policy and normalizes the current point.
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "consumed by the compare renderer in the next slice"
        )
    )]
    pub(crate) fn set_looping(
        &mut self,
        looping: bool,
        duration: Duration,
    ) -> Result<ClockState, PreviewTimeError> {
        self.looping = looping;
        self.constrain_to_extent(duration)?;
        Ok(self.state())
    }

    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "consumed by the compare renderer in the next slice"
        )
    )]
    pub(crate) fn set_playback_speed(&mut self, speed: PlaybackSpeed) {
        self.playback_speed = speed;
    }

    /// Seeks to an absolute point in the shared review extent.
    ///
    /// Looping seeks wrap into `[0, duration)`. Non-looping seeks clamp into
    /// `[0, duration]`, and seeking to the end pauses. A zero-duration extent
    /// always becomes safely paused at zero.
    pub(crate) fn seek_absolute(
        &mut self,
        position: Duration,
        duration: Duration,
    ) -> Result<ClockState, PreviewTimeError> {
        self.position = position;
        self.constrain_to_extent(duration)?;
        Ok(self.state())
    }

    /// Advances once from one authoritative wall-clock delta.
    ///
    /// Scaling mirrors `spinal::AnimationMixer` exactly: the positive finite
    /// `f32` speed is decomposed into an integer significand and exponent and
    /// rounded to the nearest nanosecond. Looping wraps; non-looping playback
    /// clamps and pauses at the end. Errors never partially advance the clock.
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "consumed by the compare renderer in the next slice"
        )
    )]
    pub(crate) fn advance(
        &mut self,
        wall_delta: Duration,
        duration: Duration,
    ) -> Result<ClockAdvance, PreviewTimeError> {
        if duration.is_zero() {
            self.position = Duration::ZERO;
            self.paused = true;
            return Ok(ClockAdvance {
                state: self.state(),
                boundary: AdvanceBoundary::Empty,
            });
        }
        if self.paused {
            return Ok(ClockAdvance {
                state: self.state(),
                boundary: AdvanceBoundary::None,
            });
        }

        let scaled_delta = scale_duration(wall_delta, self.playback_speed)?;
        let boundary = if self.looping {
            let started_outside_extent = self.position >= duration;
            let start = loop_position(self.position, duration)?;
            let delta = loop_position(scaled_delta, duration)?;
            let combined_nanos = start
                .as_nanos()
                .checked_add(delta.as_nanos())
                .ok_or(PreviewTimeError::Overflow)?;
            self.position = duration_from_nanos(combined_nanos % duration.as_nanos())?;
            if started_outside_extent
                || scaled_delta >= duration
                || combined_nanos >= duration.as_nanos()
            {
                AdvanceBoundary::Wrapped
            } else {
                AdvanceBoundary::None
            }
        } else {
            let start = self.position.min(duration);
            let remaining = duration
                .checked_sub(start)
                .ok_or(PreviewTimeError::Overflow)?;
            if scaled_delta >= remaining {
                self.position = duration;
                self.paused = true;
                AdvanceBoundary::Completed
            } else {
                self.position = start
                    .checked_add(scaled_delta)
                    .ok_or(PreviewTimeError::Overflow)?;
                AdvanceBoundary::None
            }
        };
        Ok(ClockAdvance {
            state: self.state(),
            boundary,
        })
    }

    /// Pauses and selects the adjacent exact grid point in the review extent.
    ///
    /// A zero-duration extent has no grid point and therefore returns `None`
    /// while still establishing the safe paused-at-zero state. Looping wraps;
    /// non-looping stepping clamps at its first and last authored grid points.
    pub(crate) fn step(
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

        let frame_index = match (self.looping, direction) {
            (true, StepDirection::Backward) => {
                self.rate.previous_index(self.position, point_count)?
            }
            (true, StepDirection::Forward) => self.rate.next_index(self.position, point_count)?,
            (false, StepDirection::Backward) if self.position.is_zero() => 0,
            (false, StepDirection::Backward) => {
                self.rate.previous_index(self.position, point_count)?
            }
            (false, StepDirection::Forward) => {
                let next = self.rate.next_index(self.position, point_count)?;
                if next == 0 && !self.position.is_zero() {
                    point_count - 1
                } else {
                    next
                }
            }
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

    pub(crate) const fn is_looping(&self) -> bool {
        self.looping
    }

    pub(crate) const fn playback_speed(&self) -> PlaybackSpeed {
        self.playback_speed
    }

    pub(crate) const fn rate(&self) -> PreviewRate {
        self.rate
    }

    pub(crate) fn frame_index(&self) -> u128 {
        self.rate.frame_index(self.position)
    }

    /// Projects shared review time into one source's own animation extent.
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
        if self.looping {
            loop_position(self.position, duration)
        } else {
            Ok(self.position.min(duration))
        }
    }

    const fn state(&self) -> ClockState {
        ClockState {
            position: self.position,
            paused: self.paused,
        }
    }

    fn constrain_to_extent(&mut self, duration: Duration) -> Result<(), PreviewTimeError> {
        if duration.is_zero() {
            self.position = Duration::ZERO;
            self.paused = true;
        } else if self.looping {
            self.position = loop_position(self.position, duration)?;
        } else if self.position >= duration {
            self.position = duration;
            self.paused = true;
        }
        Ok(())
    }
}

fn loop_position(position: Duration, duration: Duration) -> Result<Duration, PreviewTimeError> {
    if duration.is_zero() {
        return Ok(Duration::ZERO);
    }
    duration_from_nanos(position.as_nanos() % duration.as_nanos())
}

/// Scales a wall delta with the same exact f32-bit algorithm as Spinal's
/// animation mixer. Keep the parity tests below when changing this routine.
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "consumed by the compare renderer in the next slice"
    )
)]
fn scale_duration(duration: Duration, speed: PlaybackSpeed) -> Result<Duration, PreviewTimeError> {
    let scale = speed.multiplier();
    if duration.is_zero() {
        return Ok(Duration::ZERO);
    }
    if scale == 1.0 {
        return Ok(duration);
    }

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
        .ok_or(PreviewTimeError::Overflow)?;
    let maximum = Duration::MAX.as_nanos();
    let scaled = if exponent >= 0 {
        let shift = exponent as u32;
        if shift >= u128::BITS || product > maximum >> shift {
            return Err(PreviewTimeError::Overflow);
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
        return Err(PreviewTimeError::Overflow);
    }
    duration_from_nanos(scaled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_clock_is_paused_at_zero_on_the_shared_rate() {
        let clock = ReviewClock::new(PreviewRate::from_override(Some(60)).unwrap());

        assert_eq!(clock.position(), Duration::ZERO);
        assert_eq!(clock.frame_index(), 0);
        assert_eq!(clock.rate().fps(), 60);
        assert!(clock.is_paused());
        assert!(clock.is_looping());
        assert_eq!(clock.playback_speed(), PlaybackSpeed::NORMAL);
    }

    #[test]
    fn playback_speed_accepts_only_positive_finite_f32_values() {
        assert_eq!(
            PlaybackSpeed::new(f32::NAN),
            Err(InvalidPlaybackSpeed::NonFinite)
        );
        assert_eq!(
            PlaybackSpeed::new(f32::INFINITY),
            Err(InvalidPlaybackSpeed::NonFinite)
        );
        assert_eq!(
            PlaybackSpeed::new(f32::NEG_INFINITY),
            Err(InvalidPlaybackSpeed::NonFinite)
        );
        assert_eq!(
            PlaybackSpeed::new(0.0),
            Err(InvalidPlaybackSpeed::NotPositive)
        );
        assert_eq!(
            PlaybackSpeed::new(-1.0),
            Err(InvalidPlaybackSpeed::NotPositive)
        );
        assert_eq!(PlaybackSpeed::new(1.5).unwrap().multiplier(), 1.5);
    }

    #[test]
    fn explicit_pause_and_play_are_idempotent() {
        let mut clock = ReviewClock::new(PreviewRate::default());

        assert!(clock.set_paused(true));
        assert!(clock.set_paused(true));
        assert!(clock.is_paused());
        assert!(!clock.set_paused(false));
        assert!(!clock.set_paused(false));
        assert!(!clock.is_paused());
    }

    #[test]
    fn jittered_deltas_advance_once_each_at_the_exact_speed() {
        let mut clock = ReviewClock::new(PreviewRate::default());
        clock.set_playback_speed(PlaybackSpeed::new(1.5).unwrap());
        clock.set_paused(false);

        for milliseconds in [7, 13, 2, 19, 59] {
            clock
                .advance(Duration::from_millis(milliseconds), Duration::from_secs(1))
                .unwrap();
        }

        assert_eq!(clock.position(), Duration::from_millis(150));
        assert!(!clock.is_paused());
    }

    #[test]
    fn speed_scaling_matches_spinal_nanosecond_rounding() {
        let half = PlaybackSpeed::new(0.5).unwrap();

        // Spinal rounds exact half-nanoseconds upward.
        assert_eq!(
            scale_duration(Duration::from_nanos(1), half).unwrap(),
            Duration::from_nanos(1)
        );
        assert_eq!(
            scale_duration(Duration::from_nanos(3), half).unwrap(),
            Duration::from_nanos(2)
        );
        assert_eq!(
            scale_duration(Duration::from_millis(20), PlaybackSpeed::new(2.0).unwrap()).unwrap(),
            Duration::from_millis(40)
        );
    }

    #[test]
    fn looping_advance_wraps_and_absolute_seek_normalizes() {
        let mut clock = ReviewClock::new(PreviewRate::default());
        let duration = Duration::from_millis(100);

        clock
            .seek_absolute(Duration::from_millis(290), duration)
            .unwrap();
        clock.set_paused(false);
        assert_eq!(clock.position(), Duration::from_millis(90));

        let advance = clock.advance(Duration::from_millis(25), duration).unwrap();
        assert_eq!(advance.state.position, Duration::from_millis(15));
        assert!(!advance.state.paused);
        assert_eq!(advance.boundary, AdvanceBoundary::Wrapped);
    }

    #[test]
    fn non_looping_advance_clamps_and_pauses_at_the_end() {
        let mut clock = ReviewClock::new(PreviewRate::default());
        let duration = Duration::from_millis(100);
        clock.set_looping(false, duration).unwrap();
        clock
            .seek_absolute(Duration::from_millis(90), duration)
            .unwrap();
        clock.set_paused(false);

        let advance = clock.advance(Duration::from_millis(25), duration).unwrap();

        assert_eq!(advance.state.position, duration);
        assert!(advance.state.paused);
        assert_eq!(advance.boundary, AdvanceBoundary::Completed);
        let paused = clock.advance(Duration::from_secs(1), duration).unwrap();
        assert_eq!(paused.state, advance.state);
        assert_eq!(paused.boundary, AdvanceBoundary::None);
    }

    #[test]
    fn non_looping_seek_clamps_and_looping_toggle_wraps_without_resuming() {
        let mut clock = ReviewClock::new(PreviewRate::default());
        let duration = Duration::from_millis(100);
        clock.set_looping(false, duration).unwrap();

        let end = clock
            .seek_absolute(Duration::from_millis(275), duration)
            .unwrap();
        assert_eq!(end.position, duration);
        assert!(end.paused);

        let wrapped = clock.set_looping(true, duration).unwrap();
        assert_eq!(wrapped.position, Duration::ZERO);
        assert!(wrapped.paused);
    }

    #[test]
    fn zero_duration_advance_and_seek_are_safely_paused_at_zero() {
        let mut clock = ReviewClock::new(PreviewRate::default());
        clock.set_paused(false);

        assert_eq!(
            clock
                .advance(Duration::from_secs(1), Duration::ZERO)
                .unwrap(),
            ClockAdvance {
                state: ClockState {
                    position: Duration::ZERO,
                    paused: true,
                },
                boundary: AdvanceBoundary::Empty,
            }
        );
        clock.set_paused(false);
        assert_eq!(
            clock
                .seek_absolute(Duration::from_secs(1), Duration::ZERO)
                .unwrap(),
            ClockState {
                position: Duration::ZERO,
                paused: true,
            }
        );
    }

    #[test]
    fn scaling_overflow_is_reported_without_partial_clock_mutation() {
        let mut clock = ReviewClock::new(PreviewRate::default());
        clock
            .seek_absolute(Duration::from_nanos(7), Duration::MAX)
            .unwrap();
        clock.set_paused(false);
        clock.set_playback_speed(PlaybackSpeed::new(f32::MAX).unwrap());

        assert_eq!(
            clock.advance(Duration::MAX, Duration::MAX),
            Err(PreviewTimeError::Overflow)
        );
        assert_eq!(clock.position(), Duration::from_nanos(7));
        assert!(!clock.is_paused());
    }

    #[test]
    fn reset_preserves_review_configuration_but_starts_from_zero_paused() {
        let mut clock = ReviewClock::new(PreviewRate::default());
        clock.set_looping(false, Duration::from_secs(1)).unwrap();
        clock.set_playback_speed(PlaybackSpeed::new(0.25).unwrap());
        clock
            .seek_absolute(Duration::from_millis(500), Duration::from_secs(1))
            .unwrap();
        clock.set_paused(true);

        clock.reset();

        assert_eq!(clock.position(), Duration::ZERO);
        assert!(clock.is_paused());
        assert!(!clock.is_looping());
        assert_eq!(clock.playback_speed().multiplier(), 0.25);
    }

    #[test]
    fn seek_and_restart_preserve_pause_intent() {
        let mut clock = ReviewClock::new(PreviewRate::default());
        clock
            .seek_absolute(Duration::from_millis(75), Duration::from_secs(1))
            .unwrap();

        clock.restart();

        assert_eq!(clock.position(), Duration::ZERO);
        assert!(clock.is_paused());
    }

    #[test]
    fn looping_absolute_seek_normalizes_time() {
        let mut clock = ReviewClock::new(PreviewRate::default());
        clock
            .seek_absolute(Duration::from_millis(275), Duration::from_millis(100))
            .unwrap();

        assert_eq!(clock.position(), Duration::from_millis(75));
    }

    #[test]
    fn exact_steps_wrap_without_accumulating_rounded_durations() {
        let mut clock = ReviewClock::new(PreviewRate::default());
        clock
            .seek_absolute(Duration::from_nanos(66_666_666), Duration::from_millis(100))
            .unwrap();

        let forward = clock
            .step(StepDirection::Forward, Duration::from_millis(100))
            .unwrap()
            .unwrap();
        assert_eq!(forward.frame_index, 0);
        assert_eq!(forward.position, Duration::ZERO);

        let backward = clock
            .step(StepDirection::Backward, Duration::from_millis(100))
            .unwrap()
            .unwrap();
        assert_eq!(backward.frame_index, 2);
        assert_eq!(backward.position, Duration::from_nanos(66_666_666));
        assert!(clock.is_paused());
    }

    #[test]
    fn zero_duration_has_no_invented_frame_and_is_paused_at_zero() {
        let mut clock = ReviewClock::new(PreviewRate::default());

        let step = clock.step(StepDirection::Forward, Duration::ZERO).unwrap();

        assert_eq!(step, None);
        assert_eq!(clock.position(), Duration::ZERO);
        assert!(clock.is_paused());
    }

    #[test]
    fn reset_establishes_a_fresh_paused_review() {
        let mut clock = ReviewClock::new(PreviewRate::default());
        clock
            .seek_absolute(Duration::from_secs(2), Duration::from_secs(3))
            .unwrap();
        clock.set_paused(false);

        clock.reset();

        assert_eq!(clock.position(), Duration::ZERO);
        assert!(clock.is_paused());
    }
}

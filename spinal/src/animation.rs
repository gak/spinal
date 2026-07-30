use std::time::Duration;

use crate::{BendDirection, Mix, Rgba, Rgba8};

pub(crate) const NANOS_PER_SECOND: u64 = 1_000_000_000;

/// How an absolute animation sample behaves at the end of a clip.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum PlaybackMode {
    /// Clamp at the final key and hold the final pose.
    #[default]
    Once,
    /// Wrap to time zero at the exact animation duration.
    Loop,
}

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct TimelineTime {
    pub(crate) ticks: u64,
}

impl TimelineTime {
    pub(crate) const ZERO: Self = Self { ticks: 0 };

    pub(crate) fn from_seconds_f64(seconds: f64) -> Option<Self> {
        if !seconds.is_finite() || seconds < 0.0 {
            return None;
        }
        let ticks = (seconds * NANOS_PER_SECOND as f64).round();
        if ticks > u64::MAX as f64 {
            return None;
        }
        Some(Self {
            ticks: ticks as u64,
        })
    }

    pub(crate) const fn as_duration(self) -> Duration {
        Duration::from_nanos(self.ticks)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FrameCurve<const CHANNELS: usize> {
    Linear,
    Stepped,
    Bezier([[f32; 4]; CHANNELS]),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ScalarFrame {
    pub(crate) time: TimelineTime,
    pub(crate) value: f32,
    pub(crate) curve: FrameCurve<1>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Vec2Frame {
    pub(crate) time: TimelineTime,
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) curve: FrameCurve<2>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ColourFrame {
    pub(crate) time: TimelineTime,
    pub(crate) colour: Rgba8,
    pub(crate) curve: FrameCurve<4>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AttachmentFrame {
    pub(crate) time: TimelineTime,
    pub(crate) placeholder_name: Option<Box<str>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct IkFrame {
    pub(crate) time: TimelineTime,
    pub(crate) mix: Mix,
    pub(crate) bend_direction: BendDirection,
    pub(crate) curve: FrameCurve<2>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DrawOrderOffset {
    pub(crate) slot: u32,
    pub(crate) offset: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DrawOrderFrame {
    pub(crate) time: TimelineTime,
    pub(crate) offsets: Box<[DrawOrderOffset]>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EventPayload {
    pub(crate) integer: i32,
    pub(crate) float: f32,
    pub(crate) string: Option<Box<str>>,
    pub(crate) volume: f32,
    pub(crate) balance: f32,
}

impl Default for EventPayload {
    fn default() -> Self {
        Self {
            integer: 0,
            float: 0.0,
            string: None,
            volume: 1.0,
            balance: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EventFrame {
    pub(crate) time: TimelineTime,
    pub(crate) event: u32,
    pub(crate) payload: EventPayload,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TimelineData {
    BoneRotate {
        bone: u32,
        frames: Box<[ScalarFrame]>,
    },
    BoneTranslate {
        bone: u32,
        frames: Box<[Vec2Frame]>,
    },
    BoneScale {
        bone: u32,
        frames: Box<[Vec2Frame]>,
    },
    BoneShear {
        bone: u32,
        frames: Box<[Vec2Frame]>,
    },
    SlotAttachment {
        slot: u32,
        frames: Box<[AttachmentFrame]>,
    },
    SlotColour {
        slot: u32,
        frames: Box<[ColourFrame]>,
    },
    Ik {
        constraint: u32,
        frames: Box<[IkFrame]>,
    },
    DrawOrder {
        frames: Box<[DrawOrderFrame]>,
    },
    Events {
        frames: Box<[EventFrame]>,
    },
    Unsupported {
        name: Box<str>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AnimationData {
    pub(crate) name: Box<str>,
    pub(crate) duration: TimelineTime,
    pub(crate) timelines: Box<[TimelineData]>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EventDefinitionData {
    pub(crate) name: Box<str>,
    pub(crate) payload: EventPayload,
    pub(crate) audio: Option<Box<str>>,
}

pub(crate) fn resolve_sample_time(
    position: Duration,
    duration: TimelineTime,
    playback: PlaybackMode,
) -> TimelineTime {
    let position = position.as_nanos();
    match playback {
        PlaybackMode::Once => TimelineTime {
            ticks: u64::try_from(position.min(u128::from(duration.ticks)))
                .expect("a clamped timeline position fits in u64 ticks"),
        },
        PlaybackMode::Loop if duration == TimelineTime::ZERO => TimelineTime::ZERO,
        PlaybackMode::Loop => TimelineTime {
            ticks: u64::try_from(position % u128::from(duration.ticks))
                .expect("a wrapped timeline position fits in u64 ticks"),
        },
    }
}

pub(crate) fn sample_scalar(frames: &[ScalarFrame], time: TimelineTime) -> Option<f32> {
    let span = frame_span(frames, time, |frame| frame.time)?;
    let start = frames[span.start].value;
    Some(match span.end {
        None => start,
        Some(end) => {
            let amount = curve_amount(&frames[span.start].curve, 0, span.linear);
            interpolate_finite(start, frames[end].value, amount)
        }
    })
}

pub(crate) fn sample_vec2(frames: &[Vec2Frame], time: TimelineTime) -> Option<[f32; 2]> {
    let span = frame_span(frames, time, |frame| frame.time)?;
    let start = [frames[span.start].x, frames[span.start].y];
    Some(match span.end {
        None => start,
        Some(end) => [
            interpolate_finite(
                start[0],
                frames[end].x,
                curve_amount(&frames[span.start].curve, 0, span.linear),
            ),
            interpolate_finite(
                start[1],
                frames[end].y,
                curve_amount(&frames[span.start].curve, 1, span.linear),
            ),
        ],
    })
}

pub(crate) fn sample_colour(frames: &[ColourFrame], time: TimelineTime) -> Option<Rgba> {
    let span = frame_span(frames, time, |frame| frame.time)?;
    let start = Rgba::from_rgba8(frames[span.start].colour);
    Some(match span.end {
        None => start,
        Some(end) => {
            let amounts = core::array::from_fn(|channel| {
                curve_amount(&frames[span.start].curve, channel, span.linear)
            });
            start.lerp(Rgba::from_rgba8(frames[end].colour), amounts)
        }
    })
}

pub(crate) fn sample_attachment(
    frames: &[AttachmentFrame],
    time: TimelineTime,
) -> Option<Option<&str>> {
    let span = frame_span(frames, time, |frame| frame.time)?;
    Some(frames[span.start].placeholder_name.as_deref())
}

pub(crate) fn sample_ik(frames: &[IkFrame], time: TimelineTime) -> Option<(Mix, BendDirection)> {
    let span = frame_span(frames, time, |frame| frame.time)?;
    let start = frames[span.start].mix.get();
    let mix = match span.end {
        None => start,
        Some(end) => {
            let amount = curve_amount(&frames[span.start].curve, 0, span.linear);
            interpolate_finite(start, frames[end].mix.get(), amount)
        }
    };
    Some((
        Mix::clamped(mix).expect("loaded curves and IK values are finite"),
        frames[span.start].bend_direction,
    ))
}

pub(crate) fn sample_draw_order(
    frames: &[DrawOrderFrame],
    time: TimelineTime,
) -> Option<&[DrawOrderOffset]> {
    let span = frame_span(frames, time, |frame| frame.time)?;
    Some(&frames[span.start].offsets)
}

#[derive(Clone, Copy)]
struct FrameSpan {
    start: usize,
    end: Option<usize>,
    linear: f32,
}

fn frame_span<T>(
    frames: &[T],
    time: TimelineTime,
    frame_time: impl Fn(&T) -> TimelineTime,
) -> Option<FrameSpan> {
    let end = frames.partition_point(|frame| frame_time(frame) <= time);
    if end == 0 {
        return None;
    }
    let start = end - 1;
    if end == frames.len() {
        return Some(FrameSpan {
            start,
            end: None,
            linear: 0.0,
        });
    }
    let start_time = frame_time(&frames[start]).ticks;
    let end_time = frame_time(&frames[end]).ticks;
    let linear = (time.ticks - start_time) as f64 / (end_time - start_time) as f64;
    Some(FrameSpan {
        start,
        end: Some(end),
        linear: linear as f32,
    })
}

fn curve_amount<const CHANNELS: usize>(
    curve: &FrameCurve<CHANNELS>,
    channel: usize,
    linear: f32,
) -> f32 {
    match curve {
        FrameCurve::Linear => linear,
        FrameCurve::Stepped => 0.0,
        FrameCurve::Bezier(curves) => segmented_bezier_y_for_x(linear, curves[channel]),
    }
}

fn segmented_bezier_y_for_x(x: f32, [x1, y1, x2, y2]: [f32; 4]) -> f32 {
    let x = f64::from(x.clamp(0.0, 1.0));
    if x == 0.0 {
        return 0.0;
    }
    if x == 1.0 {
        return 1.0;
    }

    let mut previous_x = 0.0;
    let mut previous_y = 0.0;
    for segment in 1..10 {
        let parameter = f64::from(segment) / 10.0;
        let next_x = cubic_bezier(parameter, f64::from(x1), f64::from(x2));
        let next_y = cubic_bezier(parameter, f64::from(y1), f64::from(y2));
        if x <= next_x {
            return interpolate_segment(previous_x, previous_y, next_x, next_y, x);
        }
        previous_x = next_x;
        previous_y = next_y;
    }
    interpolate_segment(previous_x, previous_y, 1.0, 1.0, x)
}

fn cubic_bezier(parameter: f64, control1: f64, control2: f64) -> f64 {
    let inverse = 1.0 - parameter;
    3.0 * inverse * inverse * parameter * control1
        + 3.0 * inverse * parameter * parameter * control2
        + parameter * parameter * parameter
}

fn interpolate_segment(start_x: f64, start_y: f64, end_x: f64, end_y: f64, x: f64) -> f32 {
    let width = end_x - start_x;
    let amount = if width > 0.0 {
        (x - start_x) / width
    } else {
        0.0
    };
    saturating_f32(start_y + (end_y - start_y) * amount)
}

fn interpolate_finite(start: f32, end: f32, amount: f32) -> f32 {
    saturating_f32(f64::from(start) + (f64::from(end) - f64::from(start)) * f64::from(amount))
}

fn saturating_f32(value: f64) -> f32 {
    value.clamp(-(f32::MAX as f64), f32::MAX as f64) as f32
}

#[cfg(test)]
mod sampling_tests {
    use super::*;

    #[test]
    fn exact_decimal_times_keep_integer_boundaries() {
        assert_eq!(
            TimelineTime::from_seconds_f64(0.1)
                .expect("representable time")
                .ticks,
            100_000_000
        );
    }

    #[test]
    fn identity_bezier_is_linear_under_segment_approximation() {
        let amount = segmented_bezier_y_for_x(0.5, [0.0, 0.0, 1.0, 1.0]);
        assert!((amount - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn bezier_uses_the_documented_ten_segment_polyline() {
        let sharp = segmented_bezier_y_for_x(0.0005, [0.0, 1.0, 0.0, 1.0]);
        let asymmetric = segmented_bezier_y_for_x(0.361_75, [0.8, -0.4, 0.1, 1.6]);

        assert!((sharp - 0.1355).abs() < 1.0e-6);
        assert!((asymmetric - 0.0805).abs() < 1.0e-6);
    }

    #[test]
    fn bezier_key_endpoints_are_exact_even_with_extreme_handles() {
        let curve = [0.0, f32::MAX, 1.0, -f32::MAX];
        assert_eq!(segmented_bezier_y_for_x(0.0, curve), 0.0);
        assert_eq!(segmented_bezier_y_for_x(1.0, curve), 1.0);
    }

    #[test]
    fn folded_x_handles_remain_finite_and_monotone_in_time() {
        let curve = [1.0, -2.0, 0.0, 3.0];
        let before = segmented_bezier_y_for_x(0.499, curve);
        let middle = segmented_bezier_y_for_x(0.5, curve);
        let after = segmented_bezier_y_for_x(0.501, curve);

        assert!(before.is_finite());
        assert!(middle.is_finite());
        assert!(after.is_finite());
        assert!(before <= middle);
        assert!(middle <= after);
    }
}

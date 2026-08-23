//! Closed version-one policy shared by every Phase 0B rehearsal host.
//!
//! These values are deliberately not configurable. Changing any value is a
//! contract-version change and invalidates observations made under version one.

use std::time::Duration;

/// Spine Editor/runtime export version required by this rehearsal.
pub const TARGET_SPINE_VERSION: &str = "4.3.23";

/// The only animation exercised by the version-one rehearsal.
pub const ANIMATION_NAME: &str = "sway";

/// The exact authored duration required for [`ANIMATION_NAME`], in nanoseconds.
pub const ANIMATION_DURATION_NS: u64 = 1_000_000_000;

/// The exact authored duration required for [`ANIMATION_NAME`].
pub const ANIMATION_DURATION: Duration = Duration::from_nanos(ANIMATION_DURATION_NS);

/// The attachment-only skin layer required by the alternate-skin sample.
pub const ALTERNATE_SKIN_NAME: &str = "alternate";

/// The exact number of samples captured for each source.
pub const SAMPLE_COUNT: usize = 4;

const DEFAULT_SKIN_LAYERS: &[&str] = &[];
const ALTERNATE_SKIN_LAYERS: &[&str] = &[ALTERNATE_SKIN_NAME];

/// One entry in the closed version-one semantic sample schedule.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Sample {
    /// `sway` at zero seconds with no additional skin layer.
    SwayStart,
    /// `sway` at 0.5 seconds with no additional skin layer.
    SwayMiddle,
    /// `sway` at 0.75 seconds with only the `alternate` layer.
    SwayAlternateSkin,
    /// `sway` at one second with no additional skin layer.
    SwayEnd,
}

impl Sample {
    /// Returns the stable sample identifier used by the Phase 0B case.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::SwayStart => "sway-start",
            Self::SwayMiddle => "sway-middle",
            Self::SwayAlternateSkin => "sway-alternate-skin",
            Self::SwayEnd => "sway-end",
        }
    }

    /// Returns the exact absolute animation time in nanoseconds.
    #[must_use]
    pub const fn time_ns(self) -> u64 {
        match self {
            Self::SwayStart => 0,
            Self::SwayMiddle => 500_000_000,
            Self::SwayAlternateSkin => 750_000_000,
            Self::SwayEnd => ANIMATION_DURATION_NS,
        }
    }

    /// Returns the exact absolute animation time.
    #[must_use]
    pub const fn time(self) -> Duration {
        Duration::from_nanos(self.time_ns())
    }

    /// Returns the complete ordered attachment-only skin selection.
    #[must_use]
    pub const fn skin_layers(self) -> &'static [&'static str] {
        match self {
            Self::SwayAlternateSkin => ALTERNATE_SKIN_LAYERS,
            Self::SwayStart | Self::SwayMiddle | Self::SwayEnd => DEFAULT_SKIN_LAYERS,
        }
    }
}

/// The literal ordered version-one semantic sample schedule.
pub const SAMPLE_SCHEDULE: [Sample; SAMPLE_COUNT] = [
    Sample::SwayStart,
    Sample::SwayMiddle,
    Sample::SwayAlternateSkin,
    Sample::SwayEnd,
];

/// The only version-one authored-event observation window.
pub const EVENT_WINDOW_ID: &str = "sway-events";

/// Inclusive start of the version-one event window.
pub const EVENT_WINDOW_START_NS: u64 = 0;

/// Inclusive end of the version-one event window.
pub const EVENT_WINDOW_END_NS: u64 = ANIMATION_DURATION_NS;

/// Absolute tolerance for positions.
pub const POSITION_ABS: f64 = 0.0001;

/// Absolute tolerance for world-axis components.
pub const AXIS_ABS: f64 = 0.0001;

/// Absolute tolerance for angles measured in radians.
pub const ANGLE_RADIANS_ABS: f64 = 0.0001;

/// Absolute tolerance for unitless values such as scale.
pub const UNITLESS_ABS: f64 = 0.00001;

/// Absolute tolerance for texture coordinates.
pub const UV_ABS: f64 = 0.00001;

/// Absolute tolerance for normalized color channels.
pub const COLOR_ABS: f64 = 0.00001;

/// Absolute tolerance for authored floating-point event values.
pub const EVENT_FLOAT_ABS: f64 = 0.00001;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_animation_and_schedule_policy_cannot_drift_silently() {
        assert_eq!(TARGET_SPINE_VERSION, "4.3.23");
        assert_eq!(ANIMATION_NAME, "sway");
        assert_eq!(ANIMATION_DURATION_NS, 1_000_000_000);
        assert_eq!(ANIMATION_DURATION, Duration::from_secs(1));
        assert_eq!(ALTERNATE_SKIN_NAME, "alternate");
        assert_eq!(SAMPLE_COUNT, 4);

        let literal = [
            ("sway-start", 0, &[][..]),
            ("sway-middle", 500_000_000, &[][..]),
            ("sway-alternate-skin", 750_000_000, &["alternate"][..]),
            ("sway-end", 1_000_000_000, &[][..]),
        ];
        for (sample, (id, time_ns, skin_layers)) in SAMPLE_SCHEDULE.into_iter().zip(literal) {
            assert_eq!(sample.id(), id);
            assert_eq!(sample.time_ns(), time_ns);
            assert_eq!(sample.time(), Duration::from_nanos(time_ns));
            assert_eq!(sample.skin_layers(), skin_layers);
        }

        assert_eq!(EVENT_WINDOW_ID, "sway-events");
        assert_eq!(EVENT_WINDOW_START_NS, 0);
        assert_eq!(EVENT_WINDOW_END_NS, 1_000_000_000);
    }

    #[test]
    fn literal_semantic_tolerance_policy_cannot_drift_silently() {
        for (actual, literal) in [
            (POSITION_ABS, 0.0001_f64),
            (AXIS_ABS, 0.0001_f64),
            (ANGLE_RADIANS_ABS, 0.0001_f64),
            (UNITLESS_ABS, 0.00001_f64),
            (UV_ABS, 0.00001_f64),
            (COLOR_ABS, 0.00001_f64),
            (EVENT_FLOAT_ABS, 0.00001_f64),
        ] {
            assert_eq!(actual.to_bits(), literal.to_bits());
        }
    }
}

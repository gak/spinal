//! Input commands understood by the viewer's private preview transport.

use std::time::Duration;

use crate::clock::{InvalidPlaybackSpeed, PlaybackSpeed};

/// A direction on the fixed preview-time grid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StepDirection {
    Backward,
    Forward,
}

/// A bounded, screen-relative pan direction for keyboard controls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PanDirection {
    Left,
    Right,
    Up,
    Down,
}

/// A bounded zoom step around the center of the shared review view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ZoomDirection {
    In,
    Out,
}

/// Discrete camera navigation shared by native and browser controls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CameraNavigationCommand {
    Pan(PanDirection),
    Zoom(ZoomDirection),
}

/// One synchronized skin choice shared by every source in a review.
///
/// `Default` means no additional skin layers. The runtime's ordinary default
/// skin fallback remains active in that state.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "named skin selection is exposed by the native and browser UI slices"
    )
)]
pub(crate) enum SkinSelection {
    #[default]
    Default,
    Named(Box<str>),
}

impl SkinSelection {
    pub(crate) fn name(&self) -> Option<&str> {
        match self {
            Self::Default => None,
            Self::Named(name) => Some(name),
        }
    }
}

/// A semantic viewer command, independent of Bevy input types.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ViewerCommand {
    SelectAnimation(Box<str>),
    SelectSkin(SkinSelection),
    SetLooping(bool),
    SetPlaybackSpeed(PlaybackSpeed),
    SeekAbsolute(Duration),
    TogglePause,
    Step(StepDirection),
    Restart,
    Refit,
    Navigate(CameraNavigationCommand),
}

impl ViewerCommand {
    pub(crate) fn set_playback_speed(multiplier: f32) -> Result<Self, InvalidPlaybackSpeed> {
        Ok(Self::SetPlaybackSpeed(PlaybackSpeed::new(multiplier)?))
    }
}

/// Host-independent shared-clock commands not yet exposed by the Bevy UI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "consumed by the compare renderer in the next slice"
    )
)]
pub(crate) enum PlaybackCommand {
    SetPaused(bool),
    SetLooping(bool),
    SetPlaybackSpeed(PlaybackSpeed),
    SeekAbsolute(Duration),
    Advance(Duration),
}

#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "consumed by the compare renderer in the next slice"
    )
)]
impl PlaybackCommand {
    pub(crate) fn set_playback_speed(multiplier: f32) -> Result<Self, InvalidPlaybackSpeed> {
        Ok(Self::SetPlaybackSpeed(PlaybackSpeed::new(multiplier)?))
    }
}

/// Maps a number-row digit to its stable source-order animation index.
///
/// `1` through `9` select the first nine animations and `0` selects the
/// tenth. Digits never page or remap when an export contains more clips.
pub(crate) const fn source_animation_index(digit: u8) -> Option<usize> {
    match digit {
        1..=9 => Some((digit - 1) as usize),
        0 => Some(9),
        _other => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_row_maps_first_ten_source_animations_without_paging() {
        let actual = [1, 2, 3, 4, 5, 6, 7, 8, 9, 0].map(source_animation_index);

        assert_eq!(
            actual,
            [
                Some(0),
                Some(1),
                Some(2),
                Some(3),
                Some(4),
                Some(5),
                Some(6),
                Some(7),
                Some(8),
                Some(9),
            ]
        );
        assert_eq!(source_animation_index(10), None);
    }

    #[test]
    fn playback_speed_command_validates_before_entering_the_transport() {
        assert_eq!(
            PlaybackCommand::set_playback_speed(0.0),
            Err(InvalidPlaybackSpeed::NotPositive)
        );
        assert_eq!(
            PlaybackCommand::set_playback_speed(f32::NAN),
            Err(InvalidPlaybackSpeed::NonFinite)
        );
        assert_eq!(
            PlaybackCommand::set_playback_speed(1.5),
            Ok(PlaybackCommand::SetPlaybackSpeed(
                PlaybackSpeed::new(1.5).unwrap()
            ))
        );
        assert_eq!(
            ViewerCommand::set_playback_speed(1.5),
            Ok(ViewerCommand::SetPlaybackSpeed(
                PlaybackSpeed::new(1.5).unwrap()
            ))
        );
    }
}

//! Input commands understood by the viewer's private preview transport.

/// A direction on the fixed preview-time grid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StepDirection {
    Backward,
    Forward,
}

/// A semantic viewer command, independent of Bevy input types.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ViewerCommand {
    SelectAnimation(Box<str>),
    TogglePause,
    Step(StepDirection),
    Restart,
    Refit,
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
}

//! Pure native-window layout for Preview and Compare viewports.

use bevy::camera::Viewport;
use bevy::prelude::UVec2;

use crate::ui::SIDEBAR_WIDTH;

/// Physical-pixel camera viewports plus the logical sidebar boundary.
#[derive(Clone, Debug)]
pub(crate) struct ReviewLayout {
    pub(crate) primary: Viewport,
    pub(crate) comparison: Option<Viewport>,
}

impl ReviewLayout {
    pub(crate) fn new(physical_size: UVec2, scale_factor: f32, has_comparison: bool) -> Self {
        let width = physical_size.x.max(1);
        let height = physical_size.y.max(1);
        let scale_factor = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        };
        let requested_sidebar = (SIDEBAR_WIDTH * scale_factor).round() as u32;
        let sidebar_width = requested_sidebar.min(width.saturating_sub(1));
        let preview_width = width - sidebar_width;

        let viewport = |x: u32, viewport_width: u32| Viewport {
            physical_position: UVec2::new(x, 0),
            physical_size: UVec2::new(viewport_width.max(1), height),
            ..Default::default()
        };

        if has_comparison && preview_width >= 2 {
            let primary_width = preview_width / 2;
            let comparison_width = preview_width - primary_width;
            Self {
                primary: viewport(0, primary_width),
                comparison: Some(viewport(primary_width, comparison_width)),
            }
        } else {
            Self {
                primary: viewport(0, preview_width),
                comparison: None,
            }
        }
    }

    pub(crate) fn viewport(&self, comparison: bool) -> &Viewport {
        if comparison {
            self.comparison.as_ref().unwrap_or(&self.primary)
        } else {
            &self.primary
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_viewports_cover_every_preview_pixel_without_overlap() {
        let layout = ReviewLayout::new(UVec2::new(1_121, 720), 1.0, true);
        let comparison = layout.comparison.expect("compare viewport");

        assert_eq!(layout.primary.physical_position, UVec2::ZERO);
        assert_eq!(
            comparison.physical_position.x,
            layout.primary.physical_size.x
        );
        assert_eq!(
            layout.primary.physical_size.x + comparison.physical_size.x,
            1_121 - 360
        );
        assert_eq!(comparison.physical_size.y, 720);
    }

    #[test]
    fn hidpi_sidebar_is_converted_once_to_physical_pixels() {
        let layout = ReviewLayout::new(UVec2::new(2_240, 1_440), 2.0, false);

        assert_eq!(layout.primary.physical_size, UVec2::new(1_520, 1_440));
        assert!(layout.comparison.is_none());
    }

    #[test]
    fn tiny_and_invalid_windows_keep_a_nonempty_primary_viewport() {
        for scale in [0.0, f32::NAN, 1.0] {
            let layout = ReviewLayout::new(UVec2::ZERO, scale, true);
            assert_eq!(layout.primary.physical_size, UVec2::ONE);
            assert!(layout.comparison.is_none());
        }
    }
}

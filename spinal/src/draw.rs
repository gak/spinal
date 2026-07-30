//! Renderer-neutral draw data derived from a solved skeleton pose.

use glam::Vec2;

use crate::{
    AtlasPageId, AtlasPageRef, AtlasRegionId, AtlasRegionRef, AtlasRotation, AttachmentId, IdError,
    PixelRect, PixelSize, RegionAttachmentRef, Rgba, SkeletonAsset, SlotBlendMode, SlotId, SlotRef,
    Trim,
    world::{WorldTransform, normal_local_to_world},
};

/// A borrowed renderer-neutral item in back-to-front draw order.
///
/// Consumers should match supported variants and retain a wildcard arm so
/// future attachment profiles can add draw item kinds without breaking them.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum DrawItemRef<'a> {
    /// A rigid textured quadrilateral.
    Region(RegionDrawItemRef<'a>),
}

impl<'a> From<RegionDrawItemRef<'a>> for DrawItemRef<'a> {
    fn from(region: RegionDrawItemRef<'a>) -> Self {
        Self::Region(region)
    }
}

/// Borrowed renderer-neutral geometry for one rigid region attachment.
///
/// Positions are in skeleton space. UV coordinates use the atlas page's
/// top-left origin, with both axes increasing toward the page's right and
/// bottom edges.
#[derive(Clone, Copy, Debug)]
pub struct RegionDrawItemRef<'a> {
    slot: SlotRef<'a>,
    attachment: RegionAttachmentRef<'a>,
    atlas_page: AtlasPageRef<'a>,
    atlas_region: AtlasRegionRef<'a>,
    positions: [Vec2; 4],
    uvs: Option<[Vec2; 4]>,
    color: Rgba,
}

impl<'a> RegionDrawItemRef<'a> {
    /// Returns the slot that contributes draw order, colour, and blend mode.
    #[must_use]
    pub fn slot(self) -> SlotId {
        self.slot.id()
    }

    /// Returns the concrete region attachment being drawn.
    #[must_use]
    pub fn attachment(self) -> AttachmentId {
        self.attachment.attachment().id()
    }

    /// Returns the texture-atlas page containing this region.
    #[must_use]
    pub fn atlas_page(self) -> AtlasPageId {
        self.atlas_page.id()
    }

    /// Returns the texture-atlas region mapped onto this quadrilateral.
    #[must_use]
    pub fn atlas_region(self) -> AtlasRegionId {
        self.atlas_region.id()
    }

    /// Returns the four skeleton-space positions.
    ///
    /// Vertex order is bottom-left, top-left, top-right, bottom-right before
    /// attachment and bone transforms. The same order is used by [`Self::uvs`].
    #[must_use]
    pub const fn positions(self) -> [Vec2; 4] {
        self.positions
    }

    /// Returns normalized texture coordinates in position order.
    ///
    /// `None` means normalization is not possible because the atlas omitted
    /// its page size, or the region uses an unsupported non-quarter rotation.
    /// This preserves a drawable-sized item for diagnostics without silently
    /// sampling the wrong pixels.
    #[must_use]
    pub const fn uvs(self) -> Option<[Vec2; 4]> {
        self.uvs
    }

    /// Returns the final normalized modulation colour.
    ///
    /// This is the component-wise product of the evaluated slot colour and
    /// the attachment's authored colour.
    #[must_use]
    pub const fn color(self) -> Rgba {
        self.color
    }

    /// Returns the slot's authored blend mode.
    #[must_use]
    pub fn blend_mode(self) -> SlotBlendMode {
        self.slot.blend_mode()
    }

    pub(crate) fn from_asset(
        asset: &'a SkeletonAsset,
        slot: SlotRef<'_>,
        attachment: RegionAttachmentRef<'_>,
        bone_world: WorldTransform,
        slot_color: Rgba,
    ) -> Result<Self, IdError> {
        let slot = asset.slot(slot.id())?;
        let attachment = asset
            .attachment(attachment.attachment().id())?
            .as_region()
            .expect("a RegionAttachmentRef always identifies a region attachment");
        debug_assert_eq!(
            attachment.attachment().slot(),
            slot.id(),
            "a draw attachment must belong to its slot"
        );
        let atlas_region = asset.atlas_region(attachment.atlas_region())?;
        let atlas_page = asset.atlas_page(atlas_region.page())?;
        Ok(Self::from_linked(
            slot,
            attachment,
            atlas_region,
            atlas_page,
            bone_world,
            slot_color,
        ))
    }

    pub(crate) fn from_linked(
        slot: SlotRef<'a>,
        attachment: RegionAttachmentRef<'a>,
        atlas_region: AtlasRegionRef<'a>,
        atlas_page: AtlasPageRef<'a>,
        bone_world: WorldTransform,
        slot_color: Rgba,
    ) -> Self {
        debug_assert_eq!(
            attachment.attachment().slot(),
            slot.id(),
            "a draw attachment must belong to its slot"
        );
        debug_assert_eq!(
            attachment.atlas_region(),
            atlas_region.id(),
            "a draw attachment must use its linked atlas region"
        );
        debug_assert_eq!(
            atlas_region.page(),
            atlas_page.id(),
            "a draw region must use its containing atlas page"
        );

        let positions = transform_region_positions(
            bone_world,
            attachment.local_transform(),
            region_local_corners(
                attachment.size(),
                atlas_region.bounds(),
                atlas_region.trim(),
            ),
        );
        let uvs = normalized_uvs(
            atlas_page.size(),
            atlas_region.bounds(),
            atlas_region.rotation(),
        );
        let color = modulate_color(slot_color, Rgba::from_rgba8(attachment.color()));

        Self {
            slot,
            attachment,
            atlas_page,
            atlas_region,
            positions,
            uvs,
            color,
        }
    }
}

fn region_local_corners(size: PixelSize, bounds: PixelRect, trim: Trim) -> [Vec2; 4] {
    let original = trim.original_size();
    let (left, right) = trimmed_axis(size.width(), trim.left(), bounds.width(), original.width());
    let (bottom, top) = trimmed_axis(
        size.height(),
        trim.bottom(),
        bounds.height(),
        original.height(),
    );
    [
        Vec2::new(left, bottom),
        Vec2::new(left, top),
        Vec2::new(right, top),
        Vec2::new(right, bottom),
    ]
}

fn trimmed_axis(
    attachment_extent: u32,
    stripped_before: u32,
    packed_extent: u32,
    original_extent: u32,
) -> (f32, f32) {
    let attachment_extent = f64::from(attachment_extent);
    if original_extent == 0 {
        let half = attachment_extent * 0.5;
        return (-half as f32, half as f32);
    }

    let scale = attachment_extent / f64::from(original_extent);
    let start = -attachment_extent * 0.5 + f64::from(stripped_before) * scale;
    let end = start + f64::from(packed_extent) * scale;
    (start as f32, end as f32)
}

fn transform_region_positions(
    bone_world: WorldTransform,
    attachment_local: crate::BoneTransform,
    positions: [Vec2; 4],
) -> [Vec2; 4] {
    let attachment_world = normal_local_to_world(Some(bone_world), attachment_local);
    positions.map(|position| attachment_world.transform_point(position))
}

fn normalized_uvs(
    page: PixelSize,
    bounds: PixelRect,
    rotation: AtlasRotation,
) -> Option<[Vec2; 4]> {
    if page.width() == 0 || page.height() == 0 {
        return None;
    }

    let degrees = rotation.as_degrees();
    let (packed_width, packed_height) = if matches!(degrees, 90.0 | 270.0) {
        (bounds.height(), bounds.width())
    } else {
        (bounds.width(), bounds.height())
    };
    let left = bounds.x() as f64 / f64::from(page.width());
    let top = bounds.y() as f64 / f64::from(page.height());
    let right = (f64::from(bounds.x()) + f64::from(packed_width)) / f64::from(page.width());
    let bottom = (f64::from(bounds.y()) + f64::from(packed_height)) / f64::from(page.height());
    let top_left = Vec2::new(left as f32, top as f32);
    let top_right = Vec2::new(right as f32, top as f32);
    let bottom_right = Vec2::new(right as f32, bottom as f32);
    let bottom_left = Vec2::new(left as f32, bottom as f32);

    match degrees {
        0.0 | 360.0 => Some([bottom_left, top_left, top_right, bottom_right]),
        90.0 => Some([bottom_right, bottom_left, top_left, top_right]),
        180.0 => Some([top_right, bottom_right, bottom_left, top_left]),
        270.0 => Some([top_left, top_right, bottom_right, bottom_left]),
        _ => None,
    }
}

fn modulate_color(slot: Rgba, attachment: Rgba) -> Rgba {
    Rgba::new(
        slot.red() * attachment.red(),
        slot.green() * attachment.green(),
        slot.blue() * attachment.blue(),
        slot.alpha() * attachment.alpha(),
    )
    .expect("products of normalized finite colour channels remain normalized and finite")
}

#[cfg(test)]
mod tests {
    use glam::Vec2;

    use super::*;
    use crate::{
        Angle, AtlasRotation, BoneTransform, PixelRect, PixelSize, Rgba, Shear, SlotBlendMode,
        Trim, WorldTransform, load_json,
    };

    #[test]
    fn trim_is_scaled_into_the_authored_attachment_extent() {
        let positions = region_local_corners(
            PixelSize::new(100, 120),
            PixelRect::new(10, 20, 30, 40),
            Trim::new(5, 7, 50, 60),
        );

        assert_vec2_array(
            positions,
            [
                Vec2::new(-40.0, -46.0),
                Vec2::new(-40.0, 34.0),
                Vec2::new(20.0, 34.0),
                Vec2::new(20.0, -46.0),
            ],
        );
    }

    #[test]
    fn quarter_turn_uvs_compensate_for_counter_clockwise_packing() {
        let page = PixelSize::new(100, 100);
        let bounds = PixelRect::new(10, 20, 30, 40);

        let cases = [
            (
                0.0,
                [
                    Vec2::new(0.1, 0.6),
                    Vec2::new(0.1, 0.2),
                    Vec2::new(0.4, 0.2),
                    Vec2::new(0.4, 0.6),
                ],
            ),
            (
                90.0,
                [
                    Vec2::new(0.5, 0.5),
                    Vec2::new(0.1, 0.5),
                    Vec2::new(0.1, 0.2),
                    Vec2::new(0.5, 0.2),
                ],
            ),
            (
                180.0,
                [
                    Vec2::new(0.4, 0.2),
                    Vec2::new(0.4, 0.6),
                    Vec2::new(0.1, 0.6),
                    Vec2::new(0.1, 0.2),
                ],
            ),
            (
                270.0,
                [
                    Vec2::new(0.1, 0.2),
                    Vec2::new(0.5, 0.2),
                    Vec2::new(0.5, 0.5),
                    Vec2::new(0.1, 0.5),
                ],
            ),
        ];

        for (degrees, expected) in cases {
            let rotation = AtlasRotation::new(degrees).expect("test rotation is valid");
            let actual = normalized_uvs(page, bounds, rotation)
                .expect("a sized page and quarter turn have normalized UVs");
            assert_vec2_array(actual, expected);
        }
    }

    #[test]
    fn omitted_page_size_and_unsupported_rotation_have_no_normalized_uvs() {
        let bounds = PixelRect::new(10, 20, 30, 40);
        let unsupported = AtlasRotation::new(45.0).expect("test rotation is valid");

        assert_eq!(
            normalized_uvs(PixelSize::new(0, 0), bounds, AtlasRotation::ZERO),
            None
        );
        assert_eq!(
            normalized_uvs(PixelSize::new(100, 100), bounds, unsupported),
            None
        );
    }

    #[test]
    fn attachment_srt_is_applied_before_the_bone_world_transform() {
        let attachment_local = BoneTransform::new(
            Vec2::new(10.0, -5.0),
            Angle::from_degrees(90.0).expect("fixture angle is finite"),
            Vec2::new(2.0, 0.5),
            Shear::ZERO,
        )
        .expect("fixture transform is finite");
        let bone_world = WorldTransform::new(
            Vec2::new(100.0, 200.0),
            Vec2::new(3.0, 0.0),
            Vec2::new(0.0, 4.0),
        )
        .expect("fixture world transform is finite");
        let positions = transform_region_positions(
            bone_world,
            attachment_local,
            region_local_corners(
                PixelSize::new(2, 4),
                PixelRect::new(0, 0, 2, 4),
                Trim::new(0, 0, 2, 4),
            ),
        );

        assert_vec2_array(
            positions,
            [
                Vec2::new(133.0, 172.0),
                Vec2::new(127.0, 172.0),
                Vec2::new(127.0, 188.0),
                Vec2::new(133.0, 188.0),
            ],
        );
    }

    #[test]
    fn linked_region_builds_skeleton_space_geometry_and_multi_page_identity() {
        const ATLAS: &str = "\
unused.png
\tsize: 16, 16
unused
\tbounds: 0, 0, 16, 16

cat.png
\tsize: 100, 80
body
\tbounds: 10, 20, 30, 40
\toffsets: 5, 7, 50, 60
";
        const JSON: &str = r#"{
  "skeleton":{"spine":"4.3.23"},
  "bones":[{"name":"root"}],
  "slots":[{
    "name":"body-slot",
    "bone":"root",
    "attachment":"body",
    "blend":"additive"
  }],
  "skins":[{
    "name":"default",
    "attachments":{
      "body-slot":{
        "body":{
          "path":"body",
          "width":100,
          "height":120,
          "color":"80402080"
        }
      }
    }
  }]
}"#;

        let asset = load_json(JSON.as_bytes(), ATLAS.as_bytes())
            .expect("draw fixture should load")
            .into_asset();
        let slot = asset.slots().next().expect("fixture has one slot");
        let attachment = asset
            .attachments()
            .find(|candidate| candidate.name() == "body")
            .and_then(|candidate| candidate.as_region())
            .expect("fixture has one region attachment");
        let world = WorldTransform::new(
            Vec2::new(100.0, 200.0),
            Vec2::new(2.0, 0.0),
            Vec2::new(0.0, 3.0),
        )
        .expect("fixture transform is finite");
        let slot_color = Rgba::new(0.5, 0.25, 1.0, 0.75).expect("fixture colour is normalized");

        let item = RegionDrawItemRef::from_asset(&asset, slot, attachment, world, slot_color)
            .expect("all linked refs belong to the fixture");

        assert_eq!(item.slot(), slot.id());
        assert_eq!(item.attachment(), attachment.attachment().id());
        assert_eq!(
            item.atlas_page(),
            asset.atlas_page_id("cat.png").expect("second page exists")
        );
        assert_eq!(item.atlas_region(), attachment.atlas_region());
        assert_eq!(item.blend_mode(), SlotBlendMode::Additive);
        assert_vec2_array(
            item.positions(),
            [
                Vec2::new(20.0, 62.0),
                Vec2::new(20.0, 302.0),
                Vec2::new(140.0, 302.0),
                Vec2::new(140.0, 62.0),
            ],
        );
        assert_vec2_array(
            item.uvs().expect("fixture page declares its size"),
            [
                Vec2::new(0.1, 0.75),
                Vec2::new(0.1, 0.25),
                Vec2::new(0.4, 0.25),
                Vec2::new(0.4, 0.75),
            ],
        );
        let color = item.color();
        assert_near(color.red(), 0.5 * (128.0 / 255.0));
        assert_near(color.green(), 0.25 * (64.0 / 255.0));
        assert_near(color.blue(), 32.0 / 255.0);
        assert_near(color.alpha(), 0.75 * (128.0 / 255.0));

        let DrawItemRef::Region(region) = DrawItemRef::from(item);
        assert_eq!(region.attachment(), item.attachment());
    }

    fn assert_vec2_array(actual: [Vec2; 4], expected: [Vec2; 4]) {
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert_near(actual.x, expected.x);
            assert_near(actual.y, expected.y);
        }
    }

    fn assert_near(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= 1.0e-5,
            "expected {expected}, got {actual}"
        );
    }
}

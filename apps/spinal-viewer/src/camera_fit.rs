//! Host-neutral sampled-bounds fitting for native and browser preview cameras.

use std::{sync::Arc, time::Duration};

use bevy::{asset::Assets, prelude::*, window::PrimaryWindow};
use bevy_spinal::{
    SpinalAsset, SpinalInstance,
    spinal::{AnimationPlayer, DrawItemRef, PlayOptions, Skeleton},
};

use crate::{
    runtime::{ViewerRuntime, ViewerRuntimeSet},
    session::SourceSlot,
};

const DEFAULT_PREVIEW_SIZE: Vec2 = Vec2::new(1120.0, 720.0);
const DEFAULT_PREVIEW_PADDING: f32 = 36.0;

/// Associates one preview camera with exactly one isolated runtime source.
#[derive(Clone, Copy, Component, Debug, Eq, PartialEq)]
pub(crate) struct PreviewCamera(pub(crate) SourceSlot);

/// Ordering point after a host has assigned its camera viewports.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, SystemSet)]
pub(crate) struct ViewerCameraFitSet;

/// Fits runtime instances to their host-provided preview camera viewports.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ViewerCameraFitPlugin {
    padding: f32,
}

impl ViewerCameraFitPlugin {
    pub(crate) const fn new(padding: f32) -> Self {
        Self { padding }
    }
}

impl Default for ViewerCameraFitPlugin {
    fn default() -> Self {
        Self::new(DEFAULT_PREVIEW_PADDING)
    }
}

impl Plugin for ViewerCameraFitPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(CameraFitSettings {
            padding: self.padding,
        })
        .init_resource::<CameraFitState>()
        .add_systems(
            Update,
            fit_preview_cameras
                .in_set(ViewerCameraFitSet)
                .after(ViewerRuntimeSet::Observe),
        );
    }
}

#[derive(Resource)]
struct CameraFitSettings {
    padding: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct CameraSignature {
    entity: Entity,
    logical_size: Vec2,
}

#[derive(Default, Resource)]
struct CameraFitState {
    revisions: Option<(u64, u64)>,
    cameras: [Option<CameraSignature>; 2],
}

impl CameraFitState {
    const fn camera_mut(&mut self, slot: SourceSlot) -> &mut Option<CameraSignature> {
        match slot {
            SourceSlot::Primary => &mut self.cameras[0],
            SourceSlot::Comparison => &mut self.cameras[1],
        }
    }
}

fn fit_preview_cameras(
    windows: Query<'_, '_, &Window, With<PrimaryWindow>>,
    runtime: Res<'_, ViewerRuntime>,
    assets: Res<'_, Assets<SpinalAsset>>,
    cameras: Query<'_, '_, (Entity, &PreviewCamera, &Camera)>,
    mut instances: Query<'_, '_, &mut Transform, With<SpinalInstance>>,
    settings: Res<'_, CameraFitSettings>,
    mut state: ResMut<'_, CameraFitState>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let revisions = (runtime.catalog_revision(), runtime.refit_revision());
    let revisions_changed = state.revisions != Some(revisions);
    let mut observed = [false; 2];

    for (camera_entity, marker, camera) in &cameras {
        let index = match marker.0 {
            SourceSlot::Primary => 0,
            SourceSlot::Comparison => 1,
        };
        observed[index] = true;
        let logical_size = logical_preview_size(camera, window);
        let signature = CameraSignature {
            entity: camera_entity,
            logical_size,
        };
        let camera_changed = *state.camera_mut(marker.0) != Some(signature);
        *state.camera_mut(marker.0) = Some(signature);
        if !revisions_changed && !camera_changed {
            continue;
        }

        let Some(source) = runtime.source(marker.0) else {
            continue;
        };
        let Some(asset) = assets.get(source.asset()) else {
            continue;
        };
        if let Ok(mut transform) = instances.get_mut(source.entity()) {
            *transform =
                fitted_transform(asset, &runtime, marker.0, logical_size, settings.padding);
        }
    }

    for (index, was_observed) in observed.into_iter().enumerate() {
        if !was_observed {
            state.cameras[index] = None;
        }
    }
    state.revisions = Some(revisions);
}

fn logical_preview_size(camera: &Camera, window: &Window) -> Vec2 {
    let physical_size = camera.viewport.as_ref().map_or_else(
        || UVec2::new(window.physical_width(), window.physical_height()),
        |viewport| viewport.physical_size,
    );
    let scale_factor = window.scale_factor();
    let scale_factor = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    physical_size.as_vec2() / scale_factor
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct GeometryBounds {
    min: Vec2,
    max: Vec2,
}

impl GeometryBounds {
    fn from_points(points: impl IntoIterator<Item = Vec2>) -> Option<Self> {
        let mut bounds: Option<Self> = None;
        for point in points.into_iter().filter(|point| point.is_finite()) {
            bounds = Some(match bounds {
                Some(bounds) => Self {
                    min: bounds.min.min(point),
                    max: bounds.max.max(point),
                },
                None => Self {
                    min: point,
                    max: point,
                },
            });
        }
        bounds
    }

    fn center(self) -> Vec2 {
        self.min + (self.max - self.min) * 0.5
    }

    fn size(self) -> Vec2 {
        self.max - self.min
    }
}

fn fitted_transform(
    asset: &SpinalAsset,
    runtime: &ViewerRuntime,
    slot: SourceSlot,
    preview_size: Vec2,
    padding: f32,
) -> Transform {
    let selected_name = runtime
        .selected_name()
        .filter(|name| runtime.model().duration(slot, name).is_some());
    let projected = runtime
        .model()
        .projected_position(slot)
        .ok()
        .flatten()
        .unwrap_or(Duration::ZERO);
    fit_transform(
        sampled_bounds(asset, selected_name, projected),
        preview_size,
        padding,
    )
}

fn sampled_bounds(
    asset: &SpinalAsset,
    animation_name: Option<&str>,
    position: Duration,
) -> Option<GeometryBounds> {
    let mut skeleton = Skeleton::new(Arc::clone(asset.skeleton()));
    let frame = if let Some(animation_name) = animation_name {
        let animation = asset.skeleton().animation_id(animation_name)?;
        let mut player = AnimationPlayer::new(&skeleton);
        player.play(animation, PlayOptions::looping()).ok()?;
        player.seek_to(position);
        player
            .update(&mut skeleton, Duration::ZERO, &mut ())
            .ok()?
            .solve()
    } else {
        skeleton.editable_pose().solve()
    };
    GeometryBounds::from_points(frame.draw_items().flat_map(|item| match item {
        DrawItemRef::Region(region) => region.positions().into_iter().collect::<Vec<_>>(),
        DrawItemRef::Mesh(mesh) => mesh.positions().to_vec(),
        _other => Vec::new(),
    }))
}

fn fit_transform(bounds: Option<GeometryBounds>, preview_size: Vec2, padding: f32) -> Transform {
    let preview_size = if preview_size.is_finite() && preview_size.min_element() > 0.0 {
        preview_size
    } else {
        DEFAULT_PREVIEW_SIZE
    }
    .max(Vec2::ONE);
    let padding = if padding.is_finite() && padding >= 0.0 {
        padding
    } else {
        DEFAULT_PREVIEW_PADDING
    };
    let available = (preview_size - Vec2::splat(padding * 2.0)).max(Vec2::ONE);

    let Some(bounds) = bounds else {
        return Transform::default();
    };
    let size = bounds.size();
    if !size.is_finite() || size.min_element() < 0.0 {
        return Transform::default();
    }
    let scale_x = (size.x > f32::EPSILON).then_some(available.x / size.x);
    let scale_y = (size.y > f32::EPSILON).then_some(available.y / size.y);
    let scale = match (scale_x, scale_y) {
        (Some(x), Some(y)) => x.min(y),
        (Some(x), None) => x,
        (None, Some(y)) => y,
        (None, None) => 1.0,
    };
    let translation = -bounds.center() * scale;
    if !scale.is_finite() || scale <= 0.0 || !translation.is_finite() {
        return Transform::default();
    }
    Transform::from_translation(translation.extend(0.0)).with_scale(Vec3::splat(scale))
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        path::{Path, PathBuf},
        thread,
    };

    use bevy::asset::AssetPlugin;

    use crate::{
        bundle::SourceBundle,
        command::ViewerCommand,
        preview::PreviewRate,
        runtime::{self, CommandInbox, ViewerRuntimePlugin},
    };

    use super::*;

    const FIXTURE_JSON: &[u8] = br#"{
      "skeleton":{"spine":"4.3.23","width":180,"height":120},
      "bones":[{"name":"root"}],
      "slots":[{"name":"shape-slot","bone":"root","attachment":"shape"}],
      "skins":[{"name":"default","attachments":{"shape-slot":{"shape":{"width":180,"height":120}}}}],
      "animations":{"sway":{"bones":{"root":{"rotate":[{"value":-8},{"time":1,"value":8}]}}}}
    }"#;
    const FIXTURE_ATLAS: &[u8] = b"viewer.png\n\tsize: 1, 1\n\tformat: RGBA8888\n\tfilter: Linear, Linear\n\trepeat: none\n\tpma: false\nshape\n\tbounds: 0, 0, 1, 1\n";
    const BLUE_PIXEL_PNG: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 96, 96, 248, 255, 31,
        0, 3, 2, 1, 255, 230, 119, 11, 174, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];

    fn fixture_bundle() -> SourceBundle {
        let files = BTreeMap::from([
            (PathBuf::from("viewer.json"), FIXTURE_JSON.to_vec()),
            (PathBuf::from("viewer.atlas"), FIXTURE_ATLAS.to_vec()),
            (PathBuf::from("viewer.png"), BLUE_PIXEL_PNG.to_vec()),
        ]);
        SourceBundle::from_test_files(
            "Camera-fit fixture",
            Path::new("viewer.json"),
            Path::new("viewer.atlas"),
            files,
        )
    }

    fn update_until_ready(app: &mut App) {
        for _attempt in 0..5_000 {
            app.update();
            if app.world().resource::<ViewerRuntime>().controls_ready() {
                return;
            }
            thread::sleep(Duration::from_millis(1));
        }
        panic!("fit fixture did not become ready before the test timeout");
    }

    fn fitted_geometry(app: &App) -> (GeometryBounds, Transform) {
        let runtime = app.world().resource::<ViewerRuntime>();
        let source = runtime
            .source(SourceSlot::Primary)
            .expect("primary runtime source");
        let assets = app.world().resource::<Assets<SpinalAsset>>();
        let asset = assets.get(source.asset()).expect("loaded fixture asset");
        let animation = runtime.selected_name();
        let position = runtime
            .model()
            .projected_position(SourceSlot::Primary)
            .expect("valid projected time")
            .unwrap_or(Duration::ZERO);
        let bounds = sampled_bounds(asset, animation, position).expect("visible fixture bounds");
        let transform = app
            .world()
            .entity(source.entity())
            .get::<Transform>()
            .expect("runtime transform")
            .to_owned();
        (bounds, transform)
    }

    fn assert_visible(bounds: GeometryBounds, transform: &Transform, preview_size: Vec2) {
        let scale = transform.scale.truncate();
        let translation = transform.translation.truncate();
        let fitted_min = bounds.min * scale + translation;
        let fitted_max = bounds.max * scale + translation;
        let inset_half_size = preview_size * 0.5 - Vec2::splat(DEFAULT_PREVIEW_PADDING);
        let tolerance = Vec2::splat(0.001);
        assert!(fitted_min.cmpge(-inset_half_size - tolerance).all());
        assert!(fitted_max.cmple(inset_half_size + tolerance).all());
    }

    #[test]
    fn shared_plugin_fits_known_bounds_and_refits_after_resize_or_command() {
        let config = runtime::LaunchConfig::single(fixture_bundle(), PreviewRate::default());
        let mut app = App::new();
        runtime::prepare_runtime(&mut app, config);
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ViewerRuntimePlugin,
            ViewerCameraFitPlugin::default(),
        ));
        let window = app
            .world_mut()
            .spawn((
                Window {
                    resolution: (400, 300).into(),
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        app.world_mut()
            .spawn((Camera::default(), PreviewCamera(SourceSlot::Primary)));

        update_until_ready(&mut app);
        let (bounds, initial) = fitted_geometry(&app);
        assert_visible(bounds, &initial, Vec2::new(400.0, 300.0));

        app.world_mut()
            .entity_mut(window)
            .get_mut::<Window>()
            .expect("primary window")
            .resolution
            .set_physical_resolution(800, 600);
        app.update();

        let (resized_bounds, resized) = fitted_geometry(&app);
        assert_visible(resized_bounds, &resized, Vec2::new(800.0, 600.0));
        assert!(resized.scale.x > initial.scale.x);
        assert_eq!(resized.scale.x, resized.scale.y);
        assert_eq!(resized.scale.y, resized.scale.z);

        let source_entity = app
            .world()
            .resource::<ViewerRuntime>()
            .source(SourceSlot::Primary)
            .expect("primary runtime source")
            .entity();
        app.world_mut()
            .entity_mut(source_entity)
            .insert(Transform::default());
        app.world_mut()
            .resource_mut::<CommandInbox>()
            .push(ViewerCommand::Refit);
        app.update();

        let (refitted_bounds, refitted) = fitted_geometry(&app);
        assert_visible(refitted_bounds, &refitted, Vec2::new(800.0, 600.0));
        assert_eq!(refitted, resized);
    }

    #[test]
    fn empty_nonfinite_and_degenerate_geometry_have_safe_fallbacks() {
        let empty = fit_transform(None, Vec2::splat(f32::NAN), f32::NAN);
        assert!(empty.translation.is_finite());
        assert_eq!(empty.scale, Vec3::ONE);

        let points = GeometryBounds::from_points([
            Vec2::splat(f32::NAN),
            Vec2::new(2.0, 3.0),
            Vec2::splat(f32::INFINITY),
        ])
        .expect("one finite point remains usable");
        let degenerate = fit_transform(Some(points), DEFAULT_PREVIEW_SIZE, DEFAULT_PREVIEW_PADDING);
        assert!(degenerate.translation.is_finite());
        assert!(degenerate.scale.is_finite());
        assert!(degenerate.scale.x > 0.0);
    }
}

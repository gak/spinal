//! Shared source-camera creation and viewport assignment for every host.

use bevy::{
    camera::{ClearColorConfig, visibility::RenderLayers},
    prelude::*,
    window::PrimaryWindow,
};

use crate::{
    camera_fit::{PreviewCamera, ViewerCameraFitSet},
    layout::ReviewLayout,
    runtime::{ViewerRuntime, ViewerRuntimeSet, source_camera_order, source_render_layer},
    session::SourceSlot,
};

/// Creates one isolated preview camera per runtime source.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ViewerViewportPlugin {
    logical_right_inset: f32,
}

impl ViewerViewportPlugin {
    /// Leaves this much logical width on the right for host UI.
    pub(crate) const fn new(logical_right_inset: f32) -> Self {
        Self {
            logical_right_inset,
        }
    }

    /// Uses the entire browser canvas for source viewports.
    #[cfg(any(feature = "web", test))]
    pub(crate) const fn browser() -> Self {
        Self::new(0.0)
    }
}

impl Plugin for ViewerViewportPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ViewportSettings {
            logical_right_inset: self.logical_right_inset,
        })
        .add_systems(Startup, spawn_source_cameras.after(ViewerRuntimeSet::Setup))
        .add_systems(
            Update,
            update_source_viewports
                .after(ViewerRuntimeSet::Observe)
                .before(ViewerCameraFitSet),
        );
    }
}

#[derive(Resource)]
struct ViewportSettings {
    logical_right_inset: f32,
}

fn spawn_source_cameras(
    mut commands: Commands<'_, '_>,
    windows: Query<'_, '_, &Window, With<PrimaryWindow>>,
    runtime: Res<'_, ViewerRuntime>,
    settings: Res<'_, ViewportSettings>,
) {
    let layout = windows.single().map_or_else(
        |_error| {
            ReviewLayout::new(
                UVec2::new(1120, 720),
                1.0,
                runtime.has_comparison(),
                settings.logical_right_inset,
            )
        },
        |window| review_layout(window, &runtime, &settings),
    );

    for source in runtime.sources() {
        let slot = source.slot();
        commands.spawn((
            Camera2d,
            Camera {
                order: source_camera_order(slot),
                viewport: Some(layout.viewport(slot == SourceSlot::Comparison).clone()),
                clear_color: ClearColorConfig::Custom(Color::srgb(0.025, 0.030, 0.041)),
                ..default()
            },
            RenderLayers::layer(source_render_layer(slot)),
            PreviewCamera(slot),
        ));
    }
}

fn update_source_viewports(
    windows: Query<'_, '_, Ref<'_, Window>, With<PrimaryWindow>>,
    runtime: Res<'_, ViewerRuntime>,
    settings: Res<'_, ViewportSettings>,
    mut cameras: Query<'_, '_, (&PreviewCamera, &mut Camera)>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    if !window.is_changed() && !settings.is_changed() {
        return;
    }
    let layout = review_layout(&window, &runtime, &settings);
    for (marker, mut camera) in &mut cameras {
        camera.viewport = Some(layout.viewport(marker.0 == SourceSlot::Comparison).clone());
    }
}

fn review_layout(
    window: &Window,
    runtime: &ViewerRuntime,
    settings: &ViewportSettings,
) -> ReviewLayout {
    ReviewLayout::new(
        UVec2::new(window.physical_width(), window.physical_height()),
        window.scale_factor(),
        runtime.has_comparison(),
        settings.logical_right_inset,
    )
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        path::{Path, PathBuf},
    };

    use bevy::asset::AssetPlugin;

    use super::*;
    use crate::{
        bundle::{SourceBundle, TEST_BLUE_PIXEL_PNG, TEST_RED_PIXEL_PNG},
        preview::PreviewRate,
        runtime::{self, LaunchConfig, ViewerRuntimePlugin},
    };

    const JSON: &[u8] = br#"{"skeleton":{"spine":"4.3.23"},"bones":[{"name":"root"}]}"#;

    fn bundle(label: &str, page: &[u8]) -> SourceBundle {
        let files = BTreeMap::from([
            (PathBuf::from("fixture.json"), JSON.to_vec()),
            (
                PathBuf::from("fixture.atlas"),
                b"fixture.png\n\tsize: 1, 1\n\tformat: RGBA8888\n\tfilter: Linear, Linear\n\trepeat: none\n\tpma: false\n".to_vec(),
            ),
            (PathBuf::from("fixture.png"), page.to_vec()),
        ]);
        SourceBundle::from_test_files(
            label,
            Path::new("fixture.json"),
            Path::new("fixture.atlas"),
            files,
        )
    }

    fn comparison_app(logical_right_inset: f32) -> App {
        let config = LaunchConfig::from_bundles(
            bundle("Primary", TEST_RED_PIXEL_PNG),
            Some(bundle("Comparison", TEST_BLUE_PIXEL_PNG)),
            PreviewRate::default(),
        );
        let mut app = App::new();
        runtime::prepare_runtime(&mut app, config);
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ViewerRuntimePlugin,
            ViewerViewportPlugin::new(logical_right_inset),
        ));
        app.world_mut().spawn((
            Window {
                resolution: (1121, 720).into(),
                ..default()
            },
            PrimaryWindow,
        ));
        app
    }

    #[derive(Default, Resource)]
    struct FitSetObservation(Vec<(SourceSlot, UVec2, UVec2)>);

    fn observe_viewports_at_fit(
        cameras: Query<'_, '_, (&PreviewCamera, &Camera)>,
        mut observation: ResMut<'_, FitSetObservation>,
    ) {
        observation.0 = cameras
            .iter()
            .map(|(marker, camera)| {
                let viewport = camera.viewport.as_ref().expect("assigned viewport");
                (marker.0, viewport.physical_position, viewport.physical_size)
            })
            .collect();
        observation
            .0
            .sort_by_key(|(slot, _position, _size)| *slot == SourceSlot::Comparison);
    }

    #[test]
    fn shared_plugin_spawns_exactly_two_isolated_source_cameras() {
        let mut app = comparison_app(360.0);
        let preserved_camera = app
            .world_mut()
            .spawn((Camera2d, RenderLayers::layer(3)))
            .id();

        app.update();

        let mut cameras = app
            .world_mut()
            .query::<(&PreviewCamera, &Camera, &RenderLayers)>()
            .iter(app.world())
            .map(|(marker, camera, layers)| (marker.0, camera.clone(), layers.clone()))
            .collect::<Vec<_>>();
        cameras.sort_by_key(|(slot, _camera, _layers)| *slot == SourceSlot::Comparison);
        assert_eq!(cameras.len(), 2);
        assert!(app.world().entity(preserved_camera).contains::<Camera>());

        let runtime = app.world().resource::<ViewerRuntime>();
        for (slot, camera, camera_layers) in &cameras {
            let source = runtime.source(*slot).expect("matching runtime source");
            let source_layers = app
                .world()
                .entity(source.entity())
                .get::<RenderLayers>()
                .expect("source render layer");
            assert_eq!(camera.order, source_camera_order(*slot));
            assert!(camera_layers.intersects(source_layers));

            let other_slot = if *slot == SourceSlot::Primary {
                SourceSlot::Comparison
            } else {
                SourceSlot::Primary
            };
            let other_source = runtime.source(other_slot).expect("other runtime source");
            let other_layers = app
                .world()
                .entity(other_source.entity())
                .get::<RenderLayers>()
                .expect("other source render layer");
            assert!(!camera_layers.intersects(other_layers));
        }
    }

    #[test]
    fn resize_reassigns_odd_width_viewports_before_camera_fit() {
        let mut app = comparison_app(0.0);
        app.init_resource::<FitSetObservation>()
            .add_systems(Update, observe_viewports_at_fit.in_set(ViewerCameraFitSet));
        app.update();

        let window = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(app.world())
            .expect("primary window");
        app.world_mut()
            .entity_mut(window)
            .get_mut::<Window>()
            .expect("window")
            .resolution
            .set_physical_resolution(1123, 720);
        app.update();

        assert_eq!(
            app.world().resource::<FitSetObservation>().0,
            [
                (SourceSlot::Primary, UVec2::new(0, 0), UVec2::new(561, 720)),
                (
                    SourceSlot::Comparison,
                    UVec2::new(561, 0),
                    UVec2::new(562, 720),
                ),
            ],
            "the fit set must observe the resized viewports in the same frame"
        );

        let mut viewports = app
            .world_mut()
            .query::<(&PreviewCamera, &Camera)>()
            .iter(app.world())
            .map(|(marker, camera)| {
                (
                    marker.0,
                    camera.viewport.clone().expect("assigned viewport"),
                )
            })
            .collect::<Vec<_>>();
        viewports.sort_by_key(|(slot, _viewport)| *slot == SourceSlot::Comparison);
        let primary = &viewports[0].1;
        let comparison = &viewports[1].1;
        assert_eq!(primary.physical_position.x, 0);
        assert_eq!(comparison.physical_position.x, primary.physical_size.x);
        assert_eq!(
            comparison.physical_position.x + comparison.physical_size.x,
            1123
        );
    }

    #[test]
    fn browser_configuration_has_no_reserved_inset() {
        let plugin = ViewerViewportPlugin::browser();
        assert_eq!(plugin.logical_right_inset, 0.0);
    }
}

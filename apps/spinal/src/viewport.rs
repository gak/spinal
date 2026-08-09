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

#[cfg(feature = "phase0b-rehearsal")]
const PHASE0B_CAPTURE_SIZE: UVec2 = UVec2::new(640, 480);

/// Orders the opt-in browser capture's viewport enforcement before its hold
/// validation. Ordinary builds do not contain this set or its control resource.
#[cfg(feature = "phase0b-rehearsal")]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, SystemSet)]
pub(crate) struct Phase0bViewportSet;

/// Exact presentation state owned by the opt-in browser capture harness.
#[cfg(feature = "phase0b-rehearsal")]
#[derive(Resource, Debug)]
pub(crate) struct Phase0bViewportControl {
    requested: Option<SourceSlot>,
    applied: bool,
    restore_requested: bool,
    normal_applied: bool,
    violation: Option<Box<str>>,
}

#[cfg(feature = "phase0b-rehearsal")]
impl Default for Phase0bViewportControl {
    fn default() -> Self {
        Self {
            requested: None,
            applied: false,
            restore_requested: false,
            normal_applied: true,
            violation: None,
        }
    }
}

#[cfg(feature = "phase0b-rehearsal")]
impl Phase0bViewportControl {
    pub(crate) fn request(&mut self, source: SourceSlot) -> Result<(), &'static str> {
        if self.requested.is_some() || self.restore_requested {
            return Err("a source presentation is already active");
        }
        self.requested = Some(source);
        self.applied = false;
        self.normal_applied = false;
        self.violation = None;
        Ok(())
    }

    pub(crate) fn restore(&mut self) {
        self.requested = None;
        self.applied = false;
        self.restore_requested = true;
        self.normal_applied = false;
        self.violation = None;
    }

    pub(crate) fn release(&mut self) {
        self.requested = None;
        self.applied = false;
        self.normal_applied = false;
        self.violation = None;
    }

    pub(crate) fn is_applied(&self, source: SourceSlot) -> bool {
        self.applied && matches!(self.requested, Some(requested) if requested == source)
    }

    pub(crate) const fn is_normal(&self) -> bool {
        self.normal_applied && self.requested.is_none() && !self.restore_requested
    }

    pub(crate) fn violation(&self) -> Option<&str> {
        self.violation.as_deref()
    }

    pub(crate) const fn capture_size() -> UVec2 {
        PHASE0B_CAPTURE_SIZE
    }
}

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
        .add_systems(Startup, spawn_source_cameras.after(ViewerRuntimeSet::Setup));
        #[cfg(feature = "phase0b-rehearsal")]
        app.init_resource::<Phase0bViewportControl>().add_systems(
            Update,
            update_source_viewports
                .after(ViewerRuntimeSet::Observe)
                .in_set(Phase0bViewportSet)
                .before(ViewerCameraFitSet),
        );
        #[cfg(not(feature = "phase0b-rehearsal"))]
        app.add_systems(
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
    #[cfg(feature = "phase0b-rehearsal")] mut capture: ResMut<'_, Phase0bViewportControl>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    #[cfg(feature = "phase0b-rehearsal")]
    let restore_requested = capture.restore_requested;
    #[cfg(feature = "phase0b-rehearsal")]
    if enforce_phase0b_presentation(&window, &mut cameras, &mut capture) {
        return;
    }
    #[cfg(not(feature = "phase0b-rehearsal"))]
    if !window.is_changed() && !settings.is_changed() {
        return;
    }
    #[cfg(feature = "phase0b-rehearsal")]
    if !window.is_changed() && !settings.is_changed() && !restore_requested {
        return;
    }
    let layout = review_layout(&window, &runtime, &settings);
    for (marker, mut camera) in &mut cameras {
        camera.viewport = Some(layout.viewport(marker.0 == SourceSlot::Comparison).clone());
    }
}

#[cfg(feature = "phase0b-rehearsal")]
fn enforce_phase0b_presentation(
    window: &Window,
    cameras: &mut Query<'_, '_, (&PreviewCamera, &mut Camera)>,
    control: &mut Phase0bViewportControl,
) -> bool {
    if control.restore_requested {
        for (_marker, mut camera) in cameras.iter_mut() {
            camera.is_active = true;
        }
        control.restore_requested = false;
        control.normal_applied = true;
        return false;
    }
    let Some(requested) = control.requested else {
        return false;
    };
    if window.physical_width() != PHASE0B_CAPTURE_SIZE.x
        || window.physical_height() != PHASE0B_CAPTURE_SIZE.y
    {
        control.violation.get_or_insert_with(|| {
            format!(
                "the capture window changed to {}x{}; expected {}x{}",
                window.physical_width(),
                window.physical_height(),
                PHASE0B_CAPTURE_SIZE.x,
                PHASE0B_CAPTURE_SIZE.y
            )
            .into()
        });
        return true;
    }

    let full_viewport = bevy::camera::Viewport {
        physical_position: UVec2::ZERO,
        physical_size: PHASE0B_CAPTURE_SIZE,
        ..default()
    };
    let mut seen = [false; 2];
    let mut count = 0_usize;
    for (marker, mut camera) in cameras.iter_mut() {
        count = count.saturating_add(1);
        let index = match marker.0 {
            SourceSlot::Primary => 0,
            SourceSlot::Comparison => 1,
        };
        if seen[index] {
            control
                .violation
                .get_or_insert_with(|| "duplicate source camera detected".into());
            continue;
        }
        seen[index] = true;
        let expected_active = marker.0 == requested;
        let viewport_matches = camera.viewport.as_ref().is_some_and(|viewport| {
            viewport.physical_position == full_viewport.physical_position
                && viewport.physical_size == full_viewport.physical_size
                && viewport.depth.start.to_bits() == full_viewport.depth.start.to_bits()
                && viewport.depth.end.to_bits() == full_viewport.depth.end.to_bits()
        });
        if control.applied && (camera.is_active != expected_active || !viewport_matches) {
            control.violation.get_or_insert_with(|| {
                format!("the {:?} source camera or viewport changed", marker.0).into()
            });
            continue;
        }
        if !control.applied {
            camera.is_active = expected_active;
            camera.viewport = Some(full_viewport.clone());
        }
    }
    if count != 2 || seen != [true, true] {
        control.violation.get_or_insert_with(|| {
            format!("expected exactly two distinct source cameras, observed {count}").into()
        });
    }
    if control.violation.is_none() {
        control.applied = true;
    }
    true
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

    #[cfg(feature = "phase0b-rehearsal")]
    #[test]
    fn phase0b_presents_one_source_full_canvas_and_detects_camera_mutation() {
        let mut app = comparison_app(0.0);
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
            .set_physical_resolution(640, 480);
        app.update();
        app.world_mut()
            .resource_mut::<Phase0bViewportControl>()
            .request(SourceSlot::Primary)
            .expect("first presentation");
        app.update();

        let mut cameras = app
            .world_mut()
            .query::<(&PreviewCamera, &Camera)>()
            .iter(app.world())
            .map(|(marker, camera)| (marker.0, camera.clone()))
            .collect::<Vec<_>>();
        cameras.sort_by_key(|(slot, _camera)| *slot == SourceSlot::Comparison);
        assert_eq!(cameras.len(), 2);
        assert!(cameras[0].1.is_active);
        assert!(!cameras[1].1.is_active);
        for (_slot, camera) in &cameras {
            let viewport = camera.viewport.as_ref().expect("full viewport");
            assert_eq!(viewport.physical_position, UVec2::ZERO);
            assert_eq!(viewport.physical_size, UVec2::new(640, 480));
        }

        let comparison = app
            .world_mut()
            .query::<(Entity, &PreviewCamera)>()
            .iter(app.world())
            .find_map(|(entity, marker)| (marker.0 == SourceSlot::Comparison).then_some(entity))
            .expect("comparison camera");
        app.world_mut()
            .entity_mut(comparison)
            .get_mut::<Camera>()
            .expect("camera")
            .is_active = true;
        app.update();
        assert!(
            app.world()
                .resource::<Phase0bViewportControl>()
                .violation()
                .is_some()
        );
    }

    #[cfg(feature = "phase0b-rehearsal")]
    #[test]
    fn phase0b_terminal_restore_reenables_normal_split_cameras() {
        let mut app = comparison_app(0.0);
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
            .set_physical_resolution(640, 480);
        app.update();
        app.world_mut()
            .resource_mut::<Phase0bViewportControl>()
            .request(SourceSlot::Comparison)
            .expect("presentation");
        app.update();
        app.world_mut()
            .resource_mut::<Phase0bViewportControl>()
            .restore();
        app.update();

        let cameras = app
            .world_mut()
            .query::<&Camera>()
            .iter(app.world())
            .collect::<Vec<_>>();
        assert_eq!(cameras.len(), 2);
        assert!(cameras.iter().all(|camera| camera.is_active));
        assert_eq!(cameras[0].viewport.as_ref().unwrap().physical_size.y, 480);
        assert_eq!(
            cameras
                .iter()
                .map(|camera| camera.viewport.as_ref().unwrap().physical_size.x)
                .sum::<u32>(),
            640
        );
    }
}

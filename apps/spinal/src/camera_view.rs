//! Shared, bounded pan and zoom state for native and browser viewer cameras.

use bevy::{camera::Projection, ecs::schedule::SystemSet, prelude::*};
#[cfg(any(test, not(all(target_arch = "wasm32", feature = "phase0b-rehearsal"))))]
use bevy::{
    ecs::{message::MessageReader, system::SystemParam},
    input::{
        mouse::{MouseScrollUnit, MouseWheel},
        touch::TouchInput,
    },
    window::{CursorMoved, PrimaryWindow},
};

#[cfg(any(test, not(all(target_arch = "wasm32", feature = "phase0b-rehearsal"))))]
use crate::session::SourceSlot;
use crate::{
    camera_fit::{PreviewCamera, ViewerCameraFitSet},
    command::{CameraNavigationCommand, PanDirection, ZoomDirection},
    runtime::{ViewerRuntime, ViewerRuntimeSet},
};

const ZOOM_STEP: f32 = 1.25;
const MIN_PROJECTION_SCALE: f32 = 1.0 / 16.0;
const MAX_PROJECTION_SCALE: f32 = 10.0;
const PAN_STEP_LOGICAL_PIXELS: f32 = 48.0;
#[cfg(any(test, not(all(target_arch = "wasm32", feature = "phase0b-rehearsal"))))]
const PIXELS_PER_WHEEL_STEP: f32 = 100.0;
#[cfg(any(test, not(all(target_arch = "wasm32", feature = "phase0b-rehearsal"))))]
const MAX_WHEEL_STEPS_PER_FRAME: f32 = 4.0;
const MAX_CAMERA_CENTER: f32 = 1.0e9;

/// One linked camera state applied identically to every viewer pane.
#[derive(Clone, Copy, Debug, PartialEq, Resource)]
pub(crate) struct CameraViewState {
    center: Vec2,
    projection_scale: f32,
    revision: u64,
    observed_catalog_revision: Option<u64>,
    observed_refit_revision: u64,
}

impl Default for CameraViewState {
    fn default() -> Self {
        Self {
            center: Vec2::ZERO,
            projection_scale: 1.0,
            revision: 0,
            observed_catalog_revision: None,
            observed_refit_revision: 0,
        }
    }
}

impl CameraViewState {
    #[cfg_attr(
        not(any(test, target_arch = "wasm32")),
        allow(
            dead_code,
            reason = "browser parity attributes are published only on wasm32"
        )
    )]
    pub(crate) const fn center(&self) -> Vec2 {
        self.center
    }

    #[cfg_attr(
        not(any(test, target_arch = "wasm32")),
        allow(
            dead_code,
            reason = "browser parity attributes are published only on wasm32"
        )
    )]
    pub(crate) const fn projection_scale(&self) -> f32 {
        self.projection_scale
    }

    #[cfg_attr(
        not(any(test, target_arch = "wasm32")),
        allow(
            dead_code,
            reason = "browser parity attributes are published only on wasm32"
        )
    )]
    pub(crate) const fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn zoom_percent(&self) -> u32 {
        (100.0 / self.projection_scale).round().clamp(10.0, 1600.0) as u32
    }

    pub(crate) fn is_panned(&self) -> bool {
        self.center.length_squared() > f32::EPSILON
    }

    pub(crate) fn summary(&self, linked: bool) -> String {
        let mode = if linked { "Linked view" } else { "View" };
        let panned = if self.is_panned() { " · panned" } else { "" };
        format!("{mode} · {}% zoom{panned}", self.zoom_percent())
    }

    fn observe_runtime(&mut self, catalog_revision: u64, refit_revision: u64) {
        let catalog_changed = self
            .observed_catalog_revision
            .is_some_and(|observed| observed != catalog_revision);
        let refit_changed = self.observed_refit_revision != refit_revision;
        self.observed_catalog_revision = Some(catalog_revision);
        self.observed_refit_revision = refit_revision;
        if catalog_changed || refit_changed {
            self.reset();
        }
    }

    fn reset(&mut self) {
        if self.center != Vec2::ZERO || self.projection_scale != 1.0 {
            self.center = Vec2::ZERO;
            self.projection_scale = 1.0;
            self.bump_revision();
        }
    }

    fn apply_navigation(&mut self, command: CameraNavigationCommand) {
        match command {
            CameraNavigationCommand::Pan(direction) => {
                let delta = match direction {
                    PanDirection::Left => Vec2::new(-PAN_STEP_LOGICAL_PIXELS, 0.0),
                    PanDirection::Right => Vec2::new(PAN_STEP_LOGICAL_PIXELS, 0.0),
                    PanDirection::Up => Vec2::new(0.0, -PAN_STEP_LOGICAL_PIXELS),
                    PanDirection::Down => Vec2::new(0.0, PAN_STEP_LOGICAL_PIXELS),
                };
                self.pan_screen(delta);
            }
            CameraNavigationCommand::Zoom(ZoomDirection::In) => {
                self.zoom_by(1.0 / ZOOM_STEP, Vec2::ZERO);
            }
            CameraNavigationCommand::Zoom(ZoomDirection::Out) => {
                self.zoom_by(ZOOM_STEP, Vec2::ZERO);
            }
        }
    }

    /// Moves the artwork by a logical-pixel delta in top-left screen axes.
    fn pan_screen(&mut self, delta: Vec2) {
        if !delta.is_finite() {
            return;
        }
        let next = self.center + Vec2::new(-delta.x, delta.y) * self.projection_scale;
        if !next.is_finite() {
            return;
        }
        let next = next.clamp(
            Vec2::splat(-MAX_CAMERA_CENTER),
            Vec2::splat(MAX_CAMERA_CENTER),
        );
        if next != self.center {
            self.center = next;
            self.bump_revision();
        }
    }

    /// Multiplies projection scale while preserving the world point at "anchor".
    ///
    /// The anchor is measured in logical pixels from the pane center with Y up.
    fn zoom_by(&mut self, factor: f32, anchor: Vec2) {
        if !factor.is_finite() || factor <= 0.0 || !anchor.is_finite() {
            return;
        }
        let previous = self.projection_scale;
        let next = (previous * factor).clamp(MIN_PROJECTION_SCALE, MAX_PROJECTION_SCALE);
        if next == previous {
            return;
        }
        let next_center = self.center + anchor * (previous - next);
        if !next_center.is_finite() {
            return;
        }
        self.center = next_center.clamp(
            Vec2::splat(-MAX_CAMERA_CENTER),
            Vec2::splat(MAX_CAMERA_CENTER),
        );
        self.projection_scale = next;
        self.bump_revision();
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
}

/// Stable ordering for lifecycle reset, host input, and camera application.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, SystemSet)]
pub(crate) enum ViewerCameraViewSet {
    Lifecycle,
    Input,
    Apply,
}

/// Owns the one linked camera state and applies it to every source camera.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ViewerCameraViewPlugin;

impl Plugin for ViewerCameraViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraViewState>()
            .configure_sets(
                Update,
                (
                    ViewerCameraViewSet::Lifecycle.after(ViewerRuntimeSet::Commands),
                    ViewerCameraViewSet::Input
                        .after(ViewerCameraViewSet::Lifecycle)
                        .after(ViewerCameraFitSet),
                    ViewerCameraViewSet::Apply
                        .after(ViewerCameraViewSet::Input)
                        .after(ViewerCameraFitSet),
                ),
            )
            .add_systems(
                Update,
                (synchronize_camera_lifecycle, consume_navigation_commands)
                    .chain()
                    .in_set(ViewerCameraViewSet::Lifecycle),
            )
            .add_systems(
                Update,
                apply_view_to_cameras.in_set(ViewerCameraViewSet::Apply),
            );
    }
}

/// Adds pointer, wheel, and touch gestures to the shared camera state.
#[cfg(any(test, not(all(target_arch = "wasm32", feature = "phase0b-rehearsal"))))]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ViewerCameraInputPlugin;

#[cfg(any(test, not(all(target_arch = "wasm32", feature = "phase0b-rehearsal"))))]
impl Plugin for ViewerCameraInputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraGestureState>().add_systems(
            Update,
            (handle_mouse_camera_input, handle_touch_camera_input)
                .chain()
                .in_set(ViewerCameraViewSet::Input),
        );
    }
}

#[cfg(any(test, not(all(target_arch = "wasm32", feature = "phase0b-rehearsal"))))]
#[derive(Default, Resource)]
struct CameraGestureState {
    mouse_dragging: bool,
}

#[cfg(any(test, not(all(target_arch = "wasm32", feature = "phase0b-rehearsal"))))]
#[derive(SystemParam)]
struct MouseCameraInput<'w, 's> {
    buttons: Res<'w, ButtonInput<MouseButton>>,
    touches: Res<'w, Touches>,
    windows: Query<'w, 's, (Entity, &'static Window), With<PrimaryWindow>>,
    cameras: Query<'w, 's, (&'static PreviewCamera, &'static Camera)>,
    runtime: Res<'w, ViewerRuntime>,
}

fn synchronize_camera_lifecycle(
    runtime: Res<'_, ViewerRuntime>,
    mut view: ResMut<'_, CameraViewState>,
) {
    view.observe_runtime(runtime.catalog_revision(), runtime.refit_revision());
}

fn consume_navigation_commands(
    mut runtime: ResMut<'_, ViewerRuntime>,
    mut view: ResMut<'_, CameraViewState>,
) {
    for command in runtime.take_camera_navigation() {
        view.apply_navigation(command);
    }
}

fn apply_view_to_cameras(
    view: Res<'_, CameraViewState>,
    mut cameras: Query<'_, '_, (&mut Transform, &mut Projection), With<PreviewCamera>>,
) {
    for (mut transform, mut projection) in &mut cameras {
        transform.translation.x = view.center.x;
        transform.translation.y = view.center.y;
        if let Projection::Orthographic(orthographic) = &mut *projection {
            orthographic.scale = view.projection_scale;
        }
    }
}

#[cfg(any(test, not(all(target_arch = "wasm32", feature = "phase0b-rehearsal"))))]
fn handle_mouse_camera_input(
    mut cursor_events: MessageReader<'_, '_, CursorMoved>,
    mut wheel_events: MessageReader<'_, '_, MouseWheel>,
    input: MouseCameraInput<'_, '_>,
    mut gestures: ResMut<'_, CameraGestureState>,
    mut view: ResMut<'_, CameraViewState>,
) {
    let Ok((window_entity, window)) = input.windows.single() else {
        return;
    };
    if !input.runtime.controls_ready() || input.touches.iter().next().is_some() {
        gestures.mouse_dragging = false;
        cursor_events.clear();
        wheel_events.clear();
        return;
    }

    let cursor = window.cursor_position();
    if (input.buttons.just_pressed(MouseButton::Left)
        || input.buttons.just_pressed(MouseButton::Middle))
        && cursor.is_some_and(|position| pane_anchor(position, window, &input.cameras).is_some())
    {
        gestures.mouse_dragging = true;
    }
    if input.buttons.just_released(MouseButton::Left)
        || input.buttons.just_released(MouseButton::Middle)
        || (!input.buttons.pressed(MouseButton::Left)
            && !input.buttons.pressed(MouseButton::Middle))
    {
        gestures.mouse_dragging = false;
    }

    if gestures.mouse_dragging {
        let delta = cursor_events
            .read()
            .filter(|event| event.window == window_entity)
            .filter_map(|event| event.delta)
            .filter(|delta| delta.is_finite())
            .fold(Vec2::ZERO, |sum, delta| sum + delta);
        view.pan_screen(delta);
    } else {
        cursor_events.clear();
    }

    let Some((_, anchor)) =
        cursor.and_then(|position| pane_anchor(position, window, &input.cameras))
    else {
        wheel_events.clear();
        return;
    };
    let wheel_steps = wheel_events
        .read()
        .filter(|event| event.window == window_entity)
        .filter_map(|event| {
            event.y.is_finite().then_some(match event.unit {
                MouseScrollUnit::Line => event.y,
                MouseScrollUnit::Pixel => event.y / PIXELS_PER_WHEEL_STEP,
            })
        })
        .sum::<f32>()
        .clamp(-MAX_WHEEL_STEPS_PER_FRAME, MAX_WHEEL_STEPS_PER_FRAME);
    if wheel_steps != 0.0 {
        view.zoom_by(ZOOM_STEP.powf(-wheel_steps), anchor);
    }
}

#[cfg(any(test, not(all(target_arch = "wasm32", feature = "phase0b-rehearsal"))))]
fn handle_touch_camera_input(
    mut touch_events: MessageReader<'_, '_, TouchInput>,
    touches: Res<'_, Touches>,
    windows: Query<'_, '_, &Window, With<PrimaryWindow>>,
    cameras: Query<'_, '_, (&PreviewCamera, &Camera)>,
    runtime: Res<'_, ViewerRuntime>,
    mut view: ResMut<'_, CameraViewState>,
) {
    if !drain_touch_input(&mut touch_events) {
        return;
    }
    if !runtime.controls_ready() {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let active = touches
        .iter()
        .filter_map(|touch| {
            let (slot, _anchor) = pane_anchor(touch.start_position(), window, &cameras)?;
            Some((slot, touch))
        })
        .take(3)
        .collect::<Vec<_>>();
    match active.as_slice() {
        [(_slot, touch)] => view.pan_screen(touch.delta()),
        [(first_slot, first), (second_slot, second)] if first_slot == second_slot => {
            let previous_midpoint = (first.previous_position() + second.previous_position()) * 0.5;
            let midpoint = (first.position() + second.position()) * 0.5;
            view.pan_screen(midpoint - previous_midpoint);

            let previous_distance = first
                .previous_position()
                .distance(second.previous_position());
            let distance = first.position().distance(second.position());
            if previous_distance > f32::EPSILON
                && distance > f32::EPSILON
                && let Some(anchor) =
                    pane_anchor_for_slot(midpoint, window, cameras.iter(), *first_slot)
            {
                view.zoom_by(previous_distance / distance, anchor);
            }
        }
        _other => {}
    }
}

#[cfg(any(test, not(all(target_arch = "wasm32", feature = "phase0b-rehearsal"))))]
fn drain_touch_input(touch_events: &mut MessageReader<'_, '_, TouchInput>) -> bool {
    touch_events.read().count() != 0
}

#[cfg(any(test, not(all(target_arch = "wasm32", feature = "phase0b-rehearsal"))))]
fn pane_anchor(
    cursor: Vec2,
    window: &Window,
    cameras: &Query<'_, '_, (&PreviewCamera, &Camera)>,
) -> Option<(SourceSlot, Vec2)> {
    cameras.iter().find_map(|(marker, camera)| {
        let rect = logical_viewport_rect(camera, window);
        rect.contains(cursor)
            .then(|| (marker.0, anchor_in_rect(cursor, rect)))
    })
}

#[cfg(any(test, not(all(target_arch = "wasm32", feature = "phase0b-rehearsal"))))]
fn pane_anchor_for_slot<'a>(
    cursor: Vec2,
    window: &Window,
    cameras: impl IntoIterator<Item = (&'a PreviewCamera, &'a Camera)>,
    slot: SourceSlot,
) -> Option<Vec2> {
    cameras
        .into_iter()
        .find(|(marker, _camera)| marker.0 == slot)
        .map(|(_marker, camera)| anchor_in_rect(cursor, logical_viewport_rect(camera, window)))
}

#[cfg(any(test, not(all(target_arch = "wasm32", feature = "phase0b-rehearsal"))))]
fn anchor_in_rect(cursor: Vec2, rect: Rect) -> Vec2 {
    let local = cursor - rect.min;
    let center = rect.size() * 0.5;
    Vec2::new(local.x - center.x, center.y - local.y)
}

#[cfg(any(test, not(all(target_arch = "wasm32", feature = "phase0b-rehearsal"))))]
fn logical_viewport_rect(camera: &Camera, window: &Window) -> Rect {
    let scale_factor = if window.scale_factor().is_finite() && window.scale_factor() > 0.0 {
        window.scale_factor()
    } else {
        1.0
    };
    camera.viewport.as_ref().map_or_else(
        || Rect::from_corners(Vec2::ZERO, window.resolution.size()),
        |viewport| {
            let min = viewport.physical_position.as_vec2() / scale_factor;
            Rect::from_corners(min, min + viewport.physical_size.as_vec2() / scale_factor)
        },
    )
}

#[cfg(test)]
mod tests {
    use bevy::camera::Viewport;

    use super::*;

    #[derive(Default, Resource)]
    struct TouchInputObservations(Vec<bool>);

    fn observe_touch_input(
        mut touch_events: MessageReader<'_, '_, TouchInput>,
        mut observations: ResMut<'_, TouchInputObservations>,
    ) {
        observations.0.push(drain_touch_input(&mut touch_events));
    }

    #[test]
    fn pointer_anchored_zoom_keeps_the_anchor_world_point_stable() {
        let mut view = CameraViewState {
            center: Vec2::new(20.0, -10.0),
            ..default()
        };
        let anchor = Vec2::new(120.0, 35.0);
        let before = view.center + anchor * view.projection_scale;

        view.zoom_by(0.5, anchor);

        let after = view.center + anchor * view.projection_scale;
        assert_eq!(after, before);
        assert_eq!(view.projection_scale, 0.5);
    }

    #[test]
    fn semantic_navigation_is_bounded_and_screen_relative() {
        let mut view = CameraViewState::default();
        view.apply_navigation(CameraNavigationCommand::Pan(PanDirection::Left));
        assert_eq!(view.center, Vec2::new(PAN_STEP_LOGICAL_PIXELS, 0.0));

        for _step in 0..100 {
            view.apply_navigation(CameraNavigationCommand::Zoom(ZoomDirection::In));
        }
        assert_eq!(view.projection_scale, MIN_PROJECTION_SCALE);
        assert_eq!(view.zoom_percent(), 1600);

        for _step in 0..200 {
            view.apply_navigation(CameraNavigationCommand::Zoom(ZoomDirection::Out));
        }
        assert_eq!(view.projection_scale, MAX_PROJECTION_SCALE);
        assert_eq!(view.zoom_percent(), 10);
    }

    #[test]
    fn invalid_gestures_do_not_poison_camera_state() {
        let mut view = CameraViewState::default();
        view.pan_screen(Vec2::new(f32::NAN, 1.0));
        view.zoom_by(f32::INFINITY, Vec2::ZERO);
        view.zoom_by(0.5, Vec2::splat(f32::NAN));

        assert_eq!(view, CameraViewState::default());
    }

    #[test]
    fn fit_revision_clears_manual_navigation_but_resize_does_not_enter_the_model() {
        let mut view = CameraViewState::default();
        view.observe_runtime(1, 0);
        view.apply_navigation(CameraNavigationCommand::Pan(PanDirection::Right));
        view.apply_navigation(CameraNavigationCommand::Zoom(ZoomDirection::In));
        let revision = view.revision();

        view.observe_runtime(1, 0);
        assert_eq!(view.revision(), revision);
        assert!(view.is_panned());

        view.observe_runtime(1, 1);
        assert_eq!(view.center(), Vec2::ZERO);
        assert_eq!(view.projection_scale(), 1.0);
        assert!(view.revision() > revision);
    }

    #[test]
    fn summary_is_non_color_and_names_linked_compare_state() {
        let mut view = CameraViewState::default();
        assert_eq!(view.summary(true), "Linked view · 100% zoom");
        view.apply_navigation(CameraNavigationCommand::Pan(PanDirection::Down));
        assert_eq!(view.summary(true), "Linked view · 100% zoom · panned");
    }

    #[test]
    fn one_state_is_applied_identically_to_both_compare_cameras() {
        let mut app = App::new();
        app.init_resource::<CameraViewState>()
            .add_systems(Update, apply_view_to_cameras);
        for slot in [SourceSlot::Primary, SourceSlot::Comparison] {
            app.world_mut().spawn((Camera2d, PreviewCamera(slot)));
        }
        {
            let mut view = app.world_mut().resource_mut::<CameraViewState>();
            view.apply_navigation(CameraNavigationCommand::Pan(PanDirection::Left));
            view.apply_navigation(CameraNavigationCommand::Zoom(ZoomDirection::In));
        }

        app.update();

        let mut query = app
            .world_mut()
            .query_filtered::<(&Transform, &Projection), With<PreviewCamera>>();
        let actual = query
            .iter(app.world())
            .map(|(transform, projection)| {
                let Projection::Orthographic(orthographic) = projection else {
                    panic!("2D viewer camera must use an orthographic projection");
                };
                (transform.translation.truncate(), orthographic.scale)
            })
            .collect::<Vec<_>>();
        assert_eq!(actual.len(), 2);
        assert_eq!(actual[0], actual[1]);
        assert_eq!(
            actual[0],
            (Vec2::new(PAN_STEP_LOGICAL_PIXELS, 0.0), 1.0 / ZOOM_STEP)
        );
    }

    #[test]
    fn physical_viewports_are_hit_tested_in_logical_pixels_once() {
        let mut window = Window::default();
        window.resolution.set_physical_resolution(800, 600);
        window.resolution.set_scale_factor_override(Some(2.0));
        let camera = Camera {
            viewport: Some(Viewport {
                physical_position: UVec2::new(100, 40),
                physical_size: UVec2::new(400, 300),
                ..default()
            }),
            ..default()
        };

        assert_eq!(
            logical_viewport_rect(&camera, &window),
            Rect::from_corners(Vec2::new(50.0, 20.0), Vec2::new(250.0, 170.0))
        );
    }

    #[test]
    fn pinch_anchor_stays_in_its_starting_pane_across_the_compare_seam() {
        let mut app = App::new();
        let mut window = Window::default();
        window.resolution.set_physical_resolution(800, 600);
        for (slot, position) in [
            (SourceSlot::Primary, UVec2::ZERO),
            (SourceSlot::Comparison, UVec2::new(400, 0)),
        ] {
            app.world_mut().spawn((
                PreviewCamera(slot),
                Camera {
                    viewport: Some(Viewport {
                        physical_position: position,
                        physical_size: UVec2::new(400, 600),
                        ..default()
                    }),
                    ..default()
                },
            ));
        }

        let mut cameras = app.world_mut().query::<(&PreviewCamera, &Camera)>();
        let midpoint = Vec2::new(410.0, 300.0);

        let comparison_rect = Rect::from_corners(Vec2::new(400.0, 0.0), Vec2::new(800.0, 600.0));
        assert!(comparison_rect.contains(midpoint));
        assert_eq!(
            pane_anchor_for_slot(
                midpoint,
                &window,
                cameras.iter(app.world()),
                SourceSlot::Primary,
            ),
            Some(Vec2::new(210.0, 0.0)),
            "a pinch that began on Primary keeps Primary's coordinate system"
        );
    }

    #[test]
    fn multiple_touch_messages_are_fully_drained_in_one_frame() {
        let mut app = App::new();
        app.add_message::<TouchInput>()
            .init_resource::<TouchInputObservations>()
            .add_systems(Update, observe_touch_input);
        let event = TouchInput {
            phase: bevy::input::touch::TouchPhase::Moved,
            position: Vec2::new(10.0, 20.0),
            window: Entity::PLACEHOLDER,
            force: None,
            id: 7,
        };
        app.world_mut()
            .resource_mut::<Messages<TouchInput>>()
            .write_batch([event, event]);

        app.update();
        app.update();

        assert_eq!(
            app.world().resource::<TouchInputObservations>().0,
            [true, false],
            "a second frame without new touch input must not replay stale deltas"
        );
    }
}

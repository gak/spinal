//! Native Bevy host for the shared read-only Spinal viewer runtime.

use std::time::Duration;

use accesskit::Action;
use bevy::{
    a11y::{AccessibilityNode, ActionRequest},
    asset::AssetPlugin,
    camera::{ClearColorConfig, visibility::RenderLayers},
    ecs::message::MessageReader,
    input::mouse::{MouseScrollUnit, MouseWheel},
    input_focus::{InputDispatchPlugin, InputFocus, tab_navigation::TabNavigationPlugin},
    prelude::*,
    ui::InteractionDisabled,
    window::{PrimaryWindow, WindowResizeConstraints},
};
#[cfg(test)]
use bevy_spinal::SpinalInstance;
use bevy_spinal::SpinalInstanceState;

use crate::{
    camera_fit::{PreviewCamera, ViewerCameraFitPlugin, ViewerCameraFitSet},
    command::{StepDirection, ViewerCommand, source_animation_index},
    layout::ReviewLayout,
    runtime::{
        self, CommandInbox, ViewerLoadState, ViewerRuntime, ViewerRuntimeSet, source_camera_order,
        source_render_layer, source_slot_label,
    },
    session::SourceSlot,
    ui::{
        self, AnimationList, PauseButtonLabel, SourceStatusLabel, ViewerAction, ViewerButton,
        ViewerLabel,
    },
};

const UI_RENDER_LAYER: usize = 3;

pub(crate) use crate::runtime::{LaunchConfig, LaunchSource};

pub(crate) fn run(config: LaunchConfig) -> AppExit {
    let mut app = App::new();
    runtime::prepare_runtime(&mut app, config);
    app.insert_resource(ClearColor(Color::srgb(0.025, 0.030, 0.041)))
        .init_resource::<InputFocus>()
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    watch_for_changes_override: Some(false),
                    use_asset_processor_override: Some(false),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Spinal — Preview".into(),
                        resolution: (1120, 720).into(),
                        resizable: true,
                        resize_constraints: WindowResizeConstraints {
                            min_width: 820.0,
                            min_height: 560.0,
                            ..default()
                        },
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins((
            runtime::ViewerRuntimePlugin,
            ViewerCameraFitPlugin::new(ui::PREVIEW_PADDING),
            InputDispatchPlugin,
            TabNavigationPlugin,
        ))
        .add_systems(Startup, setup.after(ViewerRuntimeSet::Setup))
        .add_systems(
            Update,
            (
                handle_buttons,
                handle_accessibility_actions,
                handle_shortcuts,
            )
                .after(ViewerRuntimeSet::Poll)
                .before(ViewerRuntimeSet::Commands),
        )
        .add_systems(
            Update,
            sync_runtime_changes
                .after(ViewerRuntimeSet::Commands)
                .before(ViewerRuntimeSet::Clock),
        )
        .add_systems(
            Update,
            update_viewports
                .after(ViewerRuntimeSet::Observe)
                .before(ViewerCameraFitSet),
        )
        .add_systems(
            Update,
            (
                sync_button_availability,
                update_button_visuals,
                update_focus_outline,
                update_labels,
                scroll_animation_list,
            )
                .chain()
                .after(ViewerRuntimeSet::Observe),
        )
        .run()
}

#[derive(Component)]
struct ViewerUiCamera;

#[derive(Resource)]
struct NativeRuntimeRevisions {
    catalog: u64,
}

fn setup(
    mut commands: Commands<'_, '_>,
    runtime: Res<'_, ViewerRuntime>,
    windows: Query<'_, '_, &Window, With<PrimaryWindow>>,
) {
    let has_comparison = runtime.has_comparison();
    let layout = windows.single().map_or_else(
        |_error| ReviewLayout::new(UVec2::new(1120, 720), 1.0, has_comparison),
        |window| review_layout(window, has_comparison),
    );
    let ui_camera = commands
        .spawn((
            Camera2d,
            Camera {
                order: 2,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            RenderLayers::layer(UI_RENDER_LAYER),
            ViewerUiCamera,
        ))
        .id();

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
    commands.insert_resource(NativeRuntimeRevisions {
        catalog: runtime.catalog_revision(),
    });
    ui::spawn(&mut commands, ui_camera, has_comparison);
}

type ChangedButtonInteractions<'world, 'state> = Query<
    'world,
    'state,
    (Entity, &'static Interaction, &'static ViewerAction),
    (Changed<Interaction>, Without<InteractionDisabled>),
>;

fn handle_buttons(
    interactions: ChangedButtonInteractions<'_, '_>,
    mut focus: ResMut<'_, InputFocus>,
    mut inbox: ResMut<'_, CommandInbox>,
) {
    for (entity, interaction, action) in &interactions {
        if *interaction == Interaction::Pressed {
            focus.0 = Some(entity);
            inbox.push(action.0.clone());
        }
    }
}

fn handle_accessibility_actions(
    mut requests: MessageReader<'_, '_, ActionRequest>,
    actions: Query<'_, '_, &ViewerAction, Without<InteractionDisabled>>,
    mut focus: ResMut<'_, InputFocus>,
    mut inbox: ResMut<'_, CommandInbox>,
) {
    for request in requests.read() {
        if request.action != Action::Click {
            continue;
        }
        let Some(entity) = Entity::try_from_bits(request.target.0) else {
            continue;
        };
        let Ok(action) = actions.get(entity) else {
            continue;
        };
        focus.0 = Some(entity);
        inbox.push(action.0.clone());
    }
}

fn handle_shortcuts(
    keys: Res<'_, ButtonInput<KeyCode>>,
    focus: Res<'_, InputFocus>,
    runtime: Res<'_, ViewerRuntime>,
    actions: Query<'_, '_, (&ViewerAction, Option<&InteractionDisabled>)>,
    mut inbox: ResMut<'_, CommandInbox>,
) {
    let focused = focus.0.and_then(|entity| {
        actions
            .get(entity)
            .ok()
            .map(|(action, disabled)| (action.0.clone(), disabled.is_none()))
    });
    if let Some(command) = keyboard_activation_command(
        focused,
        keys.just_pressed(KeyCode::Enter),
        keys.just_pressed(KeyCode::Space),
    ) {
        inbox.push(command);
    }

    const DIGITS: [(KeyCode, u8); 10] = [
        (KeyCode::Digit1, 1),
        (KeyCode::Digit2, 2),
        (KeyCode::Digit3, 3),
        (KeyCode::Digit4, 4),
        (KeyCode::Digit5, 5),
        (KeyCode::Digit6, 6),
        (KeyCode::Digit7, 7),
        (KeyCode::Digit8, 8),
        (KeyCode::Digit9, 9),
        (KeyCode::Digit0, 0),
    ];
    for (key, digit) in DIGITS {
        if keys.just_pressed(key)
            && let Some(command) = selection_command_for_digit(digit, runtime.model().animations())
        {
            inbox.push(command);
        }
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        inbox.push(ViewerCommand::Step(StepDirection::Backward));
    }
    if keys.just_pressed(KeyCode::ArrowRight) {
        inbox.push(ViewerCommand::Step(StepDirection::Forward));
    }
    if keys.just_pressed(KeyCode::KeyR) {
        inbox.push(ViewerCommand::Restart);
    }
    if keys.just_pressed(KeyCode::KeyF) {
        inbox.push(ViewerCommand::Refit);
    }
}

fn selection_command_for_digit(digit: u8, catalog: &[Box<str>]) -> Option<ViewerCommand> {
    let index = source_animation_index(digit)?;
    let name = catalog.get(index)?;
    Some(ViewerCommand::SelectAnimation(name.clone()))
}

fn keyboard_activation_command(
    focused: Option<(ViewerCommand, bool)>,
    enter_pressed: bool,
    space_pressed: bool,
) -> Option<ViewerCommand> {
    if enter_pressed || (focused.is_some() && space_pressed) {
        return match focused {
            Some((command, true)) => Some(command),
            Some((_command, false)) => None,
            None => None,
        };
    }
    if space_pressed {
        Some(ViewerCommand::TogglePause)
    } else {
        None
    }
}

fn sync_runtime_changes(
    mut commands: Commands<'_, '_>,
    runtime: Res<'_, ViewerRuntime>,
    mut revisions: ResMut<'_, NativeRuntimeRevisions>,
    lists: Query<'_, '_, Entity, With<AnimationList>>,
) {
    let catalog_changed = revisions.catalog != runtime.catalog_revision();
    if !catalog_changed {
        return;
    }
    revisions.catalog = runtime.catalog_revision();

    if let Ok(list) = lists.single() {
        ui::rebuild_animation_list(&mut commands, list, runtime.model().animations());
    }
}

fn update_viewports(
    windows: Query<'_, '_, Ref<'_, Window>, With<PrimaryWindow>>,
    runtime: Res<'_, ViewerRuntime>,
    mut cameras: Query<'_, '_, (&PreviewCamera, &mut Camera)>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    if !window.is_changed() {
        return;
    }
    let layout = review_layout(&window, runtime.has_comparison());
    for (marker, mut camera) in &mut cameras {
        camera.viewport = Some(layout.viewport(marker.0 == SourceSlot::Comparison).clone());
    }
}

fn sync_button_availability(
    mut commands: Commands<'_, '_>,
    runtime: Res<'_, ViewerRuntime>,
    buttons: Query<'_, '_, (Entity, &ViewerAction, Option<&InteractionDisabled>)>,
) {
    for (entity, action, disabled) in &buttons {
        let enabled = ui::command_is_available(
            &action.0,
            runtime.controls_ready(),
            runtime.model().animations().iter().map(AsRef::as_ref),
        );
        match (enabled, disabled.is_some()) {
            (true, true) => {
                commands.entity(entity).remove::<InteractionDisabled>();
            }
            (false, false) => {
                commands.entity(entity).insert(InteractionDisabled);
            }
            (true, false) | (false, true) => {}
        }
    }
}

fn update_button_visuals(
    runtime: Res<'_, ViewerRuntime>,
    mut buttons: Query<
        '_,
        '_,
        (
            &ViewerAction,
            &Interaction,
            Option<&InteractionDisabled>,
            &mut BackgroundColor,
        ),
        With<ViewerButton>,
    >,
    mut pause_labels: Query<'_, '_, &mut Text, With<PauseButtonLabel>>,
) {
    for (action, interaction, disabled, mut color) in &mut buttons {
        let selected = matches!(
            &action.0,
            ViewerCommand::SelectAnimation(name)
                if runtime.model().transport().selected_animation() == Some(name.as_ref())
        );
        *color = if disabled.is_some() {
            ui::DISABLED_BUTTON
        } else {
            match interaction {
                Interaction::Pressed => ui::PRESSED_BUTTON,
                Interaction::Hovered => ui::HOVERED_BUTTON,
                Interaction::None if selected => ui::SELECTED_BUTTON,
                Interaction::None => ui::NORMAL_BUTTON,
            }
        }
        .into();
    }
    for mut text in &mut pause_labels {
        **text = if runtime.model().transport().is_paused() {
            "Resume".to_owned()
        } else {
            "Pause".to_owned()
        };
    }
}

fn update_focus_outline(
    focus: Res<'_, InputFocus>,
    mut buttons: Query<'_, '_, (Entity, &mut Outline), With<ViewerButton>>,
) {
    if !focus.is_changed() {
        return;
    }
    for (entity, mut outline) in &mut buttons {
        outline.color = if focus.0 == Some(entity) {
            Color::WHITE
        } else {
            Color::NONE
        };
    }
}

fn update_labels(
    runtime: Res<'_, ViewerRuntime>,
    windows: Query<'_, '_, &Window, With<PrimaryWindow>>,
    mut labels: Query<'_, '_, (&ViewerLabel, &mut Text, &mut TextColor), With<ViewerLabel>>,
    mut source_labels: Query<
        '_,
        '_,
        (
            &SourceStatusLabel,
            &mut Text,
            &mut TextColor,
            &mut Node,
            &mut AccessibilityNode,
        ),
        Without<ViewerLabel>,
    >,
) {
    let selected = runtime.selected_entry();
    let position = runtime.model().transport().position();
    let has_comparison = runtime.has_comparison();
    for (marker, mut text, mut color) in &mut labels {
        let (value, value_color) = match marker {
            ViewerLabel::Source => (
                format!(
                    "Files: {}",
                    runtime
                        .sources()
                        .iter()
                        .map(|source| format!(
                            "{}: {}",
                            source_slot_label(source.slot(), has_comparison),
                            source.display_path()
                        ))
                        .collect::<Vec<_>>()
                        .join(" | ")
                ),
                ui::MUTED_TEXT,
            ),
            ViewerLabel::Version => {
                let versions = runtime
                    .sources()
                    .iter()
                    .map(|source| {
                        format!(
                            "{}: {}",
                            source_slot_label(source.slot(), has_comparison),
                            source.spine_version().unwrap_or("-")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" | ");
                (format!("Spine version: {versions}"), ui::MUTED_TEXT)
            }
            ViewerLabel::Current => (
                selected.map_or_else(
                    || "Animation: -".to_owned(),
                    |(index, name, _duration)| {
                        format!(
                            "Animation {}/{}: {name}",
                            index + 1,
                            runtime.model().animations().len()
                        )
                    },
                ),
                ui::TEXT,
            ),
            ViewerLabel::Time => (
                format!(
                    "Time: {:.3} / {:.3} s",
                    position.as_secs_f64(),
                    selected.map_or(0.0, |(_index, _name, duration)| duration.as_secs_f64())
                ),
                ui::TEXT,
            ),
            ViewerLabel::Frame => (
                format!(
                    "Frame: {} @ {} FPS",
                    runtime.model().transport().frame_index(),
                    runtime.model().transport().rate().fps()
                ),
                ui::TEXT,
            ),
            ViewerLabel::RuntimeState => {
                let states = runtime
                    .sources()
                    .iter()
                    .map(|source| {
                        format!(
                            "{}: {}",
                            source_slot_label(source.slot(), has_comparison),
                            source.runtime_state()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" | ");
                let color = runtime
                    .sources()
                    .iter()
                    .map(|source| runtime_state_color(source.runtime_state()))
                    .find(|color| *color == ui::ERROR)
                    .or_else(|| {
                        runtime
                            .sources()
                            .iter()
                            .map(|source| runtime_state_color(source.runtime_state()))
                            .find(|color| *color == ui::WARNING)
                    })
                    .unwrap_or(ui::SUCCESS);
                (format!("Runtime state: {states}"), color)
            }
            ViewerLabel::LoadStatus => {
                let statuses = runtime
                    .sources()
                    .iter()
                    .map(|source| match source.load_state() {
                        ViewerLoadState::Loading => format!(
                            "{}: loading",
                            source_slot_label(source.slot(), has_comparison)
                        ),
                        ViewerLoadState::Ready => format!(
                            "{}: linked ({}, {} page(s))",
                            source_slot_label(source.slot(), has_comparison),
                            source.atlas_display_path(),
                            source.atlas_page_count()
                        ),
                        ViewerLoadState::Failed(error) => format!(
                            "{}: failed ({error})",
                            source_slot_label(source.slot(), has_comparison)
                        ),
                    })
                    .collect::<Vec<_>>()
                    .join(" | ");
                let color = if runtime
                    .sources()
                    .iter()
                    .any(|source| matches!(source.load_state(), ViewerLoadState::Failed(_)))
                {
                    ui::ERROR
                } else if runtime
                    .sources()
                    .iter()
                    .all(|source| source.load_state() == &ViewerLoadState::Ready)
                {
                    ui::SUCCESS
                } else {
                    ui::MUTED_TEXT
                };
                (format!("Load status: {statuses}"), color)
            }
            ViewerLabel::Compatibility => {
                let warnings = runtime
                    .sources()
                    .iter()
                    .filter_map(|source| {
                        source.compatibility_warning().map(|warning| {
                            format!(
                                "{}: {warning}",
                                source_slot_label(source.slot(), has_comparison)
                            )
                        })
                    })
                    .collect::<Vec<_>>();
                if warnings.is_empty() {
                    ("Source compatibility: ready".to_owned(), ui::SUCCESS)
                } else {
                    (
                        format!("Source compatibility: {}", warnings.join(" | ")),
                        ui::ERROR,
                    )
                }
            }
            ViewerLabel::LatestIssue => (
                format!(
                    "Latest runtime issue (history, not active status): {}",
                    runtime.latest_issue().unwrap_or("none")
                ),
                if runtime.latest_issue().is_some() {
                    ui::WARNING
                } else {
                    ui::MUTED_TEXT
                },
            ),
            ViewerLabel::IssueHistory => (
                if runtime.issue_history().is_empty() {
                    "Issue history (observations, newest first): none".to_owned()
                } else {
                    format!(
                        "Issue history (observations, newest first; not active-state list):\n{}",
                        runtime
                            .issue_history()
                            .iter()
                            .enumerate()
                            .map(|(index, issue)| format!("{}. {issue}", index + 1))
                            .collect::<Vec<_>>()
                            .join("\n")
                    )
                },
                ui::MUTED_TEXT,
            ),
        };
        **text = value;
        color.0 = value_color;
    }

    let layout = windows
        .single()
        .ok()
        .map(|window| review_layout(window, has_comparison));
    let scale_factor = windows
        .single()
        .ok()
        .map_or(1.0, |window| window.scale_factor());
    for (marker, mut text, mut color, mut node, mut accessibility) in &mut source_labels {
        let Some(source) = runtime.source(marker.0) else {
            continue;
        };
        let title = source_slot_label(marker.0, has_comparison);
        let selected_name = runtime.model().transport().selected_animation();
        let (value, value_color) = match source.load_state() {
            ViewerLoadState::Loading => (format!("{title} — loading"), ui::MUTED_TEXT),
            ViewerLoadState::Failed(error) => (format!("{title} — failed: {error}"), ui::ERROR),
            ViewerLoadState::Ready if !source.selected_present() => (
                format!(
                    "{title} — “{}” not present • setup pose",
                    selected_name.unwrap_or("-")
                ),
                ui::WARNING,
            ),
            ViewerLoadState::Ready => {
                let projected = runtime
                    .model()
                    .projected_position(marker.0)
                    .ok()
                    .flatten()
                    .unwrap_or(Duration::ZERO);
                let duration = selected_name
                    .and_then(|name| runtime.model().duration(marker.0, name))
                    .unwrap_or(Duration::ZERO);
                (
                    format!(
                        "{title} — {} • {:.3} / {:.3} s",
                        selected_name.unwrap_or("-"),
                        projected.as_secs_f64(),
                        duration.as_secs_f64()
                    ),
                    ui::TEXT,
                )
            }
        };
        **text = value;
        color.0 = value_color;
        let accessibility_summary = source_accessibility_summary(
            title,
            source.load_state(),
            selected_name,
            source.selected_present(),
        );
        update_accessibility_summary(&mut accessibility, accessibility_summary);
        if let Some(layout) = &layout {
            let viewport = layout.viewport(marker.0 == SourceSlot::Comparison);
            node.left = px(viewport.physical_position.x as f32 / scale_factor + 12.0);
            node.max_width = px((viewport.physical_size.x as f32 / scale_factor - 24.0).max(1.0));
        }
    }
}

fn source_accessibility_summary(
    title: &str,
    load_state: &ViewerLoadState,
    selected_animation: Option<&str>,
    selected_present: bool,
) -> String {
    match load_state {
        ViewerLoadState::Loading => format!("{title} status: loading"),
        ViewerLoadState::Failed(error) => format!("{title} status: failed: {error}"),
        ViewerLoadState::Ready if selected_animation.is_none() => {
            format!("{title} status: ready; no animation selected")
        }
        ViewerLoadState::Ready if !selected_present => format!(
            "{title} status: animation {} is not present; showing setup pose",
            selected_animation.unwrap_or("-")
        ),
        ViewerLoadState::Ready => format!(
            "{title} status: animation {} is present",
            selected_animation.unwrap_or("-")
        ),
    }
}

fn update_accessibility_summary(
    accessibility: &mut Mut<'_, AccessibilityNode>,
    summary: String,
) -> bool {
    let changed = {
        let accessibility = accessibility.bypass_change_detection();
        if accessibility.label() == Some(summary.as_str()) {
            false
        } else {
            accessibility.set_label(summary);
            true
        }
    };
    if changed {
        accessibility.set_changed();
    }
    changed
}

const fn runtime_state_color(state: &SpinalInstanceState) -> Color {
    match state {
        SpinalInstanceState::Ready => ui::SUCCESS,
        SpinalInstanceState::Degraded => ui::WARNING,
        SpinalInstanceState::ReadyNoDraws
        | SpinalInstanceState::DegradedNoDraws
        | SpinalInstanceState::Failed => ui::ERROR,
        SpinalInstanceState::Loading => ui::MUTED_TEXT,
        _other => ui::ERROR,
    }
}

fn scroll_animation_list(
    mut wheel: MessageReader<'_, '_, MouseWheel>,
    mut lists: Query<'_, '_, &mut ScrollPosition, With<AnimationList>>,
) {
    let delta = wheel
        .read()
        .map(|event| match event.unit {
            MouseScrollUnit::Line => event.y * 24.0,
            MouseScrollUnit::Pixel => event.y,
        })
        .sum::<f32>();
    if delta == 0.0 {
        return;
    }
    for mut scroll in &mut lists {
        scroll.0.y = (scroll.0.y - delta).max(0.0);
    }
}

fn review_layout(window: &Window, has_comparison: bool) -> ReviewLayout {
    ReviewLayout::new(
        UVec2::new(window.physical_width(), window.physical_height()),
        window.scale_factor(),
        has_comparison,
    )
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;

    use bevy::{
        asset::AssetPlugin,
        camera::{CameraPlugin, visibility::VisibleEntities},
        mesh::MeshPlugin,
        transform::TransformPlugin,
    };
    use bevy_spinal::SpinalPlugin;

    use super::*;

    fn spinal_entities_visible_to(app: &App, camera: Entity) -> Vec<Entity> {
        app.world()
            .entity(camera)
            .get::<VisibleEntities>()
            .expect("camera visibility results")
            .get(TypeId::of::<SpinalInstance>())
            .to_vec()
    }

    #[test]
    fn preview_camera_layers_isolate_spinal_instances_headlessly() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            AssetPlugin::default(),
            MeshPlugin,
            CameraPlugin,
            SpinalPlugin,
        ));

        let primary_camera = app
            .world_mut()
            .spawn((
                Camera2d,
                RenderLayers::layer(source_render_layer(SourceSlot::Primary)),
            ))
            .id();
        let comparison_camera = app
            .world_mut()
            .spawn((
                Camera2d,
                RenderLayers::layer(source_render_layer(SourceSlot::Comparison)),
            ))
            .id();
        let ui_camera = app
            .world_mut()
            .spawn((Camera2d, RenderLayers::layer(UI_RENDER_LAYER)))
            .id();
        let primary_instance = app
            .world_mut()
            .spawn((
                SpinalInstance::new(Handle::default()),
                RenderLayers::layer(source_render_layer(SourceSlot::Primary)),
            ))
            .id();
        let comparison_instance = app
            .world_mut()
            .spawn((
                SpinalInstance::new(Handle::default()),
                RenderLayers::layer(source_render_layer(SourceSlot::Comparison)),
            ))
            .id();

        app.update();
        assert_eq!(
            spinal_entities_visible_to(&app, primary_camera),
            [primary_instance]
        );
        assert_eq!(
            spinal_entities_visible_to(&app, comparison_camera),
            [comparison_instance]
        );
        assert!(spinal_entities_visible_to(&app, ui_camera).is_empty());

        app.world_mut()
            .entity_mut(primary_instance)
            .insert(RenderLayers::layer(source_render_layer(
                SourceSlot::Comparison,
            )));
        app.world_mut()
            .entity_mut(comparison_instance)
            .insert(RenderLayers::layer(source_render_layer(
                SourceSlot::Primary,
            )));
        app.update();

        assert_eq!(
            spinal_entities_visible_to(&app, primary_camera),
            [comparison_instance]
        );
        assert_eq!(
            spinal_entities_visible_to(&app, comparison_camera),
            [primary_instance]
        );
        assert!(spinal_entities_visible_to(&app, ui_camera).is_empty());
    }

    #[test]
    fn accessible_source_status_changes_only_with_semantic_review_state() {
        let present =
            source_accessibility_summary("Current", &ViewerLoadState::Ready, Some("walk"), true);
        assert_eq!(present, "Current status: animation walk is present");
        assert!(!present.contains("0.000"));

        let mut node = accesskit::Node::new(accesskit::Role::Status);
        node.set_label(present.clone());
        let mut world = World::new();
        let entity = world.spawn(AccessibilityNode(node)).id();
        world.clear_trackers();

        {
            let mut entity_mut = world.entity_mut(entity);
            let mut accessibility = entity_mut
                .get_mut::<AccessibilityNode>()
                .expect("source status accessibility node");
            assert!(!update_accessibility_summary(&mut accessibility, present));
        }
        assert!(
            !world
                .entity(entity)
                .get_ref::<AccessibilityNode>()
                .expect("source status accessibility node")
                .is_changed(),
            "an unchanged semantic summary must not trigger an announcement"
        );

        let setup_pose =
            source_accessibility_summary("Current", &ViewerLoadState::Ready, Some("jump"), false);
        {
            let mut entity_mut = world.entity_mut(entity);
            let mut accessibility = entity_mut
                .get_mut::<AccessibilityNode>()
                .expect("source status accessibility node");
            assert!(update_accessibility_summary(&mut accessibility, setup_pose));
        }
        let accessibility = world
            .entity(entity)
            .get_ref::<AccessibilityNode>()
            .expect("source status accessibility node");
        assert!(accessibility.is_changed());
        assert_eq!(
            accessibility.label(),
            Some("Current status: animation jump is not present; showing setup pose")
        );
    }

    #[test]
    fn space_activates_focus_without_also_toggling_transport() {
        let selection = ViewerCommand::SelectAnimation("walk".into());
        assert_eq!(
            keyboard_activation_command(Some((selection.clone(), true)), false, true),
            Some(selection.clone())
        );
        assert_eq!(
            keyboard_activation_command(Some((selection, false)), false, true),
            None
        );
        assert_eq!(
            keyboard_activation_command(None, false, true),
            Some(ViewerCommand::TogglePause)
        );
    }

    #[test]
    fn number_shortcut_resolves_the_current_source_order_to_a_name() {
        let catalog = (0..12)
            .map(|index| format!("animation-{index}").into_boxed_str())
            .collect::<Vec<_>>();

        assert_eq!(
            selection_command_for_digit(0, &catalog),
            Some(ViewerCommand::SelectAnimation("animation-9".into()))
        );
    }

    #[test]
    fn runtime_status_colors_distinguish_drawable_and_empty_states() {
        assert_eq!(
            runtime_state_color(&SpinalInstanceState::Ready),
            ui::SUCCESS
        );
        assert_eq!(
            runtime_state_color(&SpinalInstanceState::Degraded),
            ui::WARNING
        );
        for state in [
            SpinalInstanceState::ReadyNoDraws,
            SpinalInstanceState::DegradedNoDraws,
            SpinalInstanceState::Failed,
        ] {
            assert_eq!(runtime_state_color(&state), ui::ERROR);
        }
    }
}

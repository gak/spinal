//! Native Bevy host for the shared read-only Spinal viewer runtime.

use std::time::Duration;

#[cfg(test)]
use crate::runtime::source_render_layer;
use accesskit::{Action, Toggled};
use bevy::{
    a11y::{AccessibilityNode, ActionRequest},
    asset::AssetPlugin,
    camera::{ClearColorConfig, visibility::RenderLayers},
    ecs::message::MessageReader,
    input::mouse::{MouseScrollUnit, MouseWheel},
    input_focus::{FocusCause, InputFocus, tab_navigation::TabNavigationPlugin},
    prelude::*,
    ui::{InteractionDisabled, RelativeCursorPosition},
    window::{PrimaryWindow, WindowResizeConstraints},
};
#[cfg(test)]
use bevy_spinal::SpinalInstance;
use bevy_spinal::SpinalInstanceState;

use crate::{
    camera_fit::ViewerCameraFitPlugin,
    camera_view::{
        CameraViewState, ViewerCameraInputPlugin, ViewerCameraViewPlugin, ViewerCameraViewSet,
    },
    command::{
        CameraNavigationCommand, PanDirection, SkinSelection, StepDirection, ViewerCommand,
        ZoomDirection, source_animation_index,
    },
    layout::ReviewLayout,
    runtime::{
        self, CommandInbox, ViewerLoadState, ViewerRuntime, ViewerRuntimeSet, source_slot_label,
    },
    session::SourceSlot,
    ui::{
        self, AnimationButtonLabel, AnimationList, PauseButtonLabel, SidebarScroll,
        SkinButtonLabel, SkinList, SourceStatusLabel, ViewerAction, ViewerButton, ViewerLabel,
        ViewerViewportFocus,
    },
    viewport::ViewerViewportPlugin,
};

const UI_RENDER_LAYER: usize = 3;

pub(crate) use crate::runtime::{LaunchConfig, LaunchSource};

pub(crate) fn run(config: LaunchConfig) -> AppExit {
    let mut app = App::new();
    let mode_copy = ui::native_mode_copy(config.comparison.is_some());
    runtime::prepare_runtime(&mut app, config);
    app.insert_resource(ClearColor(Color::srgb(0.025, 0.030, 0.041)))
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    watch_for_changes_override: Some(false),
                    use_asset_processor_override: Some(false),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: mode_copy.window_title.into(),
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
            ViewerViewportPlugin::new(ui::SIDEBAR_WIDTH),
            ViewerCameraFitPlugin::new(ui::PREVIEW_PADDING),
            ViewerCameraViewPlugin,
            ViewerCameraInputPlugin,
            TabNavigationPlugin,
        ))
        .add_systems(Startup, setup.after(ViewerRuntimeSet::Setup))
        .add_systems(
            Update,
            (
                handle_buttons,
                handle_accessibility_actions,
                focus_viewport_on_pointer,
                handle_shortcuts,
            )
                .chain()
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
            (
                sync_button_availability,
                update_button_visuals,
                update_focus_outline,
                update_viewport_focus_outline,
                update_viewport_accessibility,
                reveal_focused_sidebar_control,
                update_labels,
                scroll_catalog_lists,
            )
                .chain()
                .after(ViewerRuntimeSet::Observe)
                .after(ViewerCameraViewSet::Input),
        )
        .run()
}

#[derive(Component)]
struct ViewerUiCamera;

#[derive(Resource)]
struct NativeRuntimeRevisions {
    catalog: u64,
}

fn setup(mut commands: Commands<'_, '_>, runtime: Res<'_, ViewerRuntime>) {
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

    commands.insert_resource(NativeRuntimeRevisions {
        catalog: runtime.catalog_revision(),
    });
    ui::spawn(&mut commands, ui_camera, &runtime);
}

type ChangedButtonInteractions<'world, 'state> = Query<
    'world,
    'state,
    (Entity, &'static Interaction, &'static ViewerAction),
    (Changed<Interaction>, Without<InteractionDisabled>),
>;

type ViewerButtonVisuals<'world, 'state> = Query<
    'world,
    'state,
    (
        &'static ViewerAction,
        &'static Interaction,
        Option<&'static InteractionDisabled>,
        &'static mut BackgroundColor,
        &'static mut AccessibilityNode,
    ),
    With<ViewerButton>,
>;

fn handle_buttons(
    interactions: ChangedButtonInteractions<'_, '_>,
    mut focus: ResMut<'_, InputFocus>,
    mut inbox: ResMut<'_, CommandInbox>,
) {
    for (entity, interaction, action) in &interactions {
        if *interaction == Interaction::Pressed {
            focus.set(entity, FocusCause::Pressed);
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
        let Some(entity) = Entity::try_from_bits(request.target_node.0) else {
            continue;
        };
        let Ok(action) = actions.get(entity) else {
            continue;
        };
        focus.set(entity, FocusCause::Navigated);
        inbox.push(action.0.clone());
    }
}

fn handle_shortcuts(
    keys: Res<'_, ButtonInput<KeyCode>>,
    focus: Res<'_, InputFocus>,
    runtime: Res<'_, ViewerRuntime>,
    actions: Query<'_, '_, (&ViewerAction, Option<&InteractionDisabled>)>,
    viewport_focus: Query<'_, '_, Entity, With<ViewerViewportFocus>>,
    mut inbox: ResMut<'_, CommandInbox>,
) {
    let focused = focus.get().and_then(|entity| {
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
    let viewport_focused = viewport_focus
        .single()
        .is_ok_and(|entity| focus.get() == Some(entity));
    if viewport_focused {
        for (key, direction) in [
            (KeyCode::ArrowLeft, PanDirection::Left),
            (KeyCode::ArrowRight, PanDirection::Right),
            (KeyCode::ArrowUp, PanDirection::Up),
            (KeyCode::ArrowDown, PanDirection::Down),
        ] {
            if keys.just_pressed(key) {
                inbox.push(ViewerCommand::Navigate(CameraNavigationCommand::Pan(
                    direction,
                )));
            }
        }
        if keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadAdd) {
            inbox.push(ViewerCommand::Navigate(CameraNavigationCommand::Zoom(
                ZoomDirection::In,
            )));
        }
        if keys.just_pressed(KeyCode::Minus) || keys.just_pressed(KeyCode::NumpadSubtract) {
            inbox.push(ViewerCommand::Navigate(CameraNavigationCommand::Zoom(
                ZoomDirection::Out,
            )));
        }
    } else {
        if keys.just_pressed(KeyCode::ArrowLeft) {
            inbox.push(ViewerCommand::Step(StepDirection::Backward));
        }
        if keys.just_pressed(KeyCode::ArrowRight) {
            inbox.push(ViewerCommand::Step(StepDirection::Forward));
        }
    }
    if keys.just_pressed(KeyCode::KeyR) {
        inbox.push(ViewerCommand::Restart);
    }
    if keys.just_pressed(KeyCode::KeyF) {
        inbox.push(ViewerCommand::Refit);
    }
}

fn focus_viewport_on_pointer(
    buttons: Res<'_, ButtonInput<MouseButton>>,
    windows: Query<'_, '_, &Window, With<PrimaryWindow>>,
    runtime: Res<'_, ViewerRuntime>,
    viewport: Query<'_, '_, Entity, With<ViewerViewportFocus>>,
    mut focus: ResMut<'_, InputFocus>,
) {
    if !buttons.just_pressed(MouseButton::Left) && !buttons.just_pressed(MouseButton::Middle) {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let layout = review_layout(window, runtime.has_comparison());
    let preview_right = layout.comparison.as_ref().map_or_else(
        || layout.primary.physical_position.x + layout.primary.physical_size.x,
        |comparison| comparison.physical_position.x + comparison.physical_size.x,
    ) as f32
        / window.scale_factor().max(f32::EPSILON);
    if cursor.x < preview_right
        && let Ok(entity) = viewport.single()
    {
        focus.set(entity, FocusCause::Pressed);
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
    animation_lists: Query<'_, '_, Entity, With<AnimationList>>,
    skin_lists: Query<'_, '_, Entity, With<SkinList>>,
) {
    let catalog_changed = revisions.catalog != runtime.catalog_revision();
    if !catalog_changed {
        return;
    }
    revisions.catalog = runtime.catalog_revision();

    if let Ok(list) = animation_lists.single() {
        ui::rebuild_animation_list(&mut commands, list, runtime.model().animations());
    }
    if let Ok(list) = skin_lists.single() {
        ui::rebuild_skin_list(&mut commands, list, runtime.model().skins());
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
            runtime.model().skins().iter().map(AsRef::as_ref),
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
    mut buttons: ViewerButtonVisuals<'_, '_>,
    mut pause_labels: Query<'_, '_, &mut Text, With<PauseButtonLabel>>,
    mut animation_labels: Query<'_, '_, (&AnimationButtonLabel, &mut Text)>,
    mut skin_labels: Query<'_, '_, (&SkinButtonLabel, &mut Text)>,
) {
    let selected_animation = runtime.model().transport().selected_animation();
    let pause_copy = ui::pause_action_copy(runtime.model().transport().is_paused());
    for (action, interaction, disabled, mut color, mut accessibility) in &mut buttons {
        let selected = ui::command_is_selected(
            &action.0,
            selected_animation,
            runtime.model().selected_skin(),
        );
        if matches!(
            &action.0,
            ViewerCommand::SelectAnimation(_) | ViewerCommand::SelectSkin(_)
        ) {
            update_accessibility_toggle(&mut accessibility, selected);
        }
        if matches!(&action.0, ViewerCommand::TogglePause) {
            update_accessibility_summary(
                &mut accessibility,
                pause_copy.accessible_label.to_owned(),
            );
        }
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
        if text.as_str() != pause_copy.visible_label {
            **text = pause_copy.visible_label.to_owned();
        }
    }
    for (label, mut text) in &mut animation_labels {
        let value = label.text(selected_animation);
        if text.as_str() != value.as_str() {
            **text = value;
        }
    }
    for (label, mut text) in &mut skin_labels {
        let value = label.text(runtime.model().selected_skin());
        if text.as_str() != value.as_str() {
            **text = value;
        }
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
        outline.color = if focus.get() == Some(entity) {
            Color::WHITE
        } else {
            Color::NONE
        };
    }
}

fn update_viewport_focus_outline(
    focus: Res<'_, InputFocus>,
    mut viewport: Query<'_, '_, (Entity, &mut Outline), With<ViewerViewportFocus>>,
) {
    if !focus.is_changed() {
        return;
    }
    let Ok((entity, mut outline)) = viewport.single_mut() else {
        return;
    };
    outline.color = if focus.get() == Some(entity) {
        Color::WHITE
    } else {
        Color::NONE
    };
}

fn update_viewport_accessibility(
    runtime: Res<'_, ViewerRuntime>,
    camera_view: Res<'_, CameraViewState>,
    mut viewport: Query<'_, '_, &mut AccessibilityNode, With<ViewerViewportFocus>>,
) {
    let Ok(mut accessibility) = viewport.single_mut() else {
        return;
    };
    let summary = ui::viewport_accessibility_label(&camera_view.summary(runtime.has_comparison()));
    update_accessibility_summary(&mut accessibility, summary);
}

type ScrollableUi<'world, 'state, Filter> = Query<
    'world,
    'state,
    (
        &'static mut ScrollPosition,
        &'static ComputedNode,
        &'static UiGlobalTransform,
    ),
    Filter,
>;

#[allow(
    clippy::type_complexity,
    reason = "the three disjoint scroll queries must share one ParamSet for Bevy's system-param borrow rules"
)]
fn reveal_focused_sidebar_control(
    focus: Res<'_, InputFocus>,
    buttons: Query<'_, '_, (&ViewerAction, &ComputedNode, &UiGlobalTransform), With<ViewerButton>>,
    mut scrollables: ParamSet<
        '_,
        '_,
        (
            ScrollableUi<'_, '_, With<SkinList>>,
            ScrollableUi<'_, '_, With<AnimationList>>,
            ScrollableUi<'_, '_, With<SidebarScroll>>,
        ),
    >,
) {
    if !focus.is_changed() {
        return;
    }
    let Some(focused) = focus.get() else {
        return;
    };
    let Ok((action, button_node, button_transform)) = buttons.get(focused) else {
        return;
    };
    let margin = 4.0;
    let mut outer_target = vertical_bounds(button_node, button_transform);

    if matches!(&action.0, ViewerCommand::SelectSkin(_)) {
        let mut skin_lists = scrollables.p0();
        let Ok((mut scroll, list_node, list_transform)) = skin_lists.single_mut() else {
            return;
        };
        let (list_left, list_right) = horizontal_bounds(list_node, list_transform);
        let (button_left, button_right) = horizontal_bounds(button_node, button_transform);
        let effective_scroll = effective_logical_scroll(list_node);
        scroll.0.x = revealed_scroll_x(
            effective_scroll.x,
            list_left + margin,
            list_right - margin,
            button_left,
            button_right,
            list_node.inverse_scale_factor(),
        );
        outer_target = vertical_bounds(list_node, list_transform);
    } else if matches!(&action.0, ViewerCommand::SelectAnimation(_)) {
        let mut animation_lists = scrollables.p1();
        let Ok((mut scroll, list_node, list_transform)) = animation_lists.single_mut() else {
            return;
        };
        let (list_top, list_bottom) = vertical_bounds(list_node, list_transform);
        let (button_top, button_bottom) = vertical_bounds(button_node, button_transform);
        let effective_scroll = effective_logical_scroll(list_node);
        scroll.0.y = revealed_scroll_y(
            effective_scroll.y,
            list_top + margin,
            list_bottom - margin,
            button_top,
            button_bottom,
            list_node.inverse_scale_factor(),
        );
        outer_target = (list_top, list_bottom);
    }

    let mut sidebars = scrollables.p2();
    let Ok((mut scroll, sidebar_node, sidebar_transform)) = sidebars.single_mut() else {
        return;
    };
    let (sidebar_top, sidebar_bottom) = vertical_bounds(sidebar_node, sidebar_transform);
    let effective_scroll = effective_logical_scroll(sidebar_node);
    scroll.0.y = revealed_scroll_y(
        effective_scroll.y,
        sidebar_top + margin,
        sidebar_bottom - margin,
        outer_target.0,
        outer_target.1,
        sidebar_node.inverse_scale_factor(),
    );
}

fn horizontal_bounds(node: &ComputedNode, transform: &UiGlobalTransform) -> (f32, f32) {
    let center = transform.to_scale_angle_translation().2.x;
    let half_width = node.size().x * 0.5;
    (center - half_width, center + half_width)
}

fn vertical_bounds(node: &ComputedNode, transform: &UiGlobalTransform) -> (f32, f32) {
    let center = transform.to_scale_angle_translation().2.y;
    let half_height = node.size().y * 0.5;
    (center - half_height, center + half_height)
}

fn effective_logical_scroll(node: &ComputedNode) -> Vec2 {
    node.scroll_position * node.inverse_scale_factor()
}

fn revealed_scroll_x(
    current: f32,
    viewport_left: f32,
    viewport_right: f32,
    item_left: f32,
    item_right: f32,
    logical_per_physical: f32,
) -> f32 {
    revealed_scroll_axis(
        current,
        viewport_left,
        viewport_right,
        item_left,
        item_right,
        logical_per_physical,
    )
}

fn revealed_scroll_y(
    current: f32,
    viewport_top: f32,
    viewport_bottom: f32,
    item_top: f32,
    item_bottom: f32,
    logical_per_physical: f32,
) -> f32 {
    revealed_scroll_axis(
        current,
        viewport_top,
        viewport_bottom,
        item_top,
        item_bottom,
        logical_per_physical,
    )
}

fn revealed_scroll_axis(
    current: f32,
    viewport_start: f32,
    viewport_end: f32,
    item_start: f32,
    item_end: f32,
    logical_per_physical: f32,
) -> f32 {
    let current = current.max(0.0);
    if ![
        current,
        viewport_start,
        viewport_end,
        item_start,
        item_end,
        logical_per_physical,
    ]
    .into_iter()
    .all(f32::is_finite)
        || viewport_end <= viewport_start
        || item_end < item_start
        || logical_per_physical <= 0.0
    {
        return current;
    }
    if item_start < viewport_start {
        (current - (viewport_start - item_start) * logical_per_physical).max(0.0)
    } else if item_end > viewport_end {
        current + (item_end - viewport_end) * logical_per_physical
    } else {
        current
    }
}

fn update_labels(
    runtime: Res<'_, ViewerRuntime>,
    camera_view: Res<'_, CameraViewState>,
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
            ViewerLabel::CameraView => (
                format!("Camera: {}", camera_view.summary(runtime.has_comparison())),
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
        let selected_skin = runtime.model().selected_skin();
        let skin_present = source.selected_skin_present();
        let skin_status = visible_skin_status(selected_skin, skin_present);
        let missing_skin = selected_skin.name().is_some() && !skin_present;
        let (value, value_color) = match source.load_state() {
            ViewerLoadState::Loading => (format!("{title} — loading"), ui::MUTED_TEXT),
            ViewerLoadState::Failed(error) => (format!("{title} — failed: {error}"), ui::ERROR),
            ViewerLoadState::Ready if !source.selected_present() => (
                format!(
                    "{title} — “{}” not present • setup pose • {skin_status}",
                    selected_name.unwrap_or("-")
                ),
                ui::WARNING,
            ),
            ViewerLoadState::Ready if selected_name.is_none() => (
                format!("{title} — no animation • {skin_status}"),
                if missing_skin { ui::WARNING } else { ui::TEXT },
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
                        "{title} — {} • {:.3} / {:.3} s • {skin_status}",
                        selected_name.unwrap_or("-"),
                        projected.as_secs_f64(),
                        duration.as_secs_f64()
                    ),
                    if missing_skin { ui::WARNING } else { ui::TEXT },
                )
            }
        };
        **text = value;
        color.0 = value_color;
        let accessibility_summary = source_accessibility_summary(SourceAccessibilityContext {
            title,
            load_state: source.load_state(),
            has_runtime_issue: source.latest_issue().is_some(),
            selected_animation: selected_name,
            selected_present: source.selected_present(),
            selected_skin,
            selected_skin_present: skin_present,
        });
        update_accessibility_summary(&mut accessibility, accessibility_summary);
        if let Some(layout) = &layout {
            let viewport = layout.viewport(marker.0 == SourceSlot::Comparison);
            node.left = px(viewport.physical_position.x as f32 / scale_factor + 12.0);
            node.max_width = px((viewport.physical_size.x as f32 / scale_factor - 24.0).max(1.0));
        }
    }
}

#[derive(Clone, Copy)]
struct SourceAccessibilityContext<'a> {
    title: &'a str,
    load_state: &'a ViewerLoadState,
    has_runtime_issue: bool,
    selected_animation: Option<&'a str>,
    selected_present: bool,
    selected_skin: &'a SkinSelection,
    selected_skin_present: bool,
}

fn source_accessibility_summary(context: SourceAccessibilityContext<'_>) -> String {
    let SourceAccessibilityContext {
        title,
        load_state,
        has_runtime_issue,
        selected_animation,
        selected_present,
        selected_skin,
        selected_skin_present,
    } = context;
    let animation_status = match load_state {
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
    };
    let selection_status = match load_state {
        ViewerLoadState::Loading | ViewerLoadState::Failed(_) => animation_status,
        ViewerLoadState::Ready => format!(
            "{animation_status}; {}",
            accessible_skin_status(selected_skin, selected_skin_present)
        ),
    };
    if has_runtime_issue {
        format!("{selection_status}; runtime findings are present; see Diagnostics")
    } else {
        format!("{selection_status}; runtime findings: none")
    }
}

fn visible_skin_status(selection: &SkinSelection, present: bool) -> String {
    match selection {
        SkinSelection::Default => "skin Default".to_owned(),
        SkinSelection::Named(name) if present => format!("skin “{name}”"),
        SkinSelection::Named(name) => {
            format!("skin “{name}” not present • Default fallback")
        }
    }
}

fn accessible_skin_status(selection: &SkinSelection, present: bool) -> String {
    match selection {
        SkinSelection::Default => "skin Default is selected".to_owned(),
        SkinSelection::Named(name) if present => format!("skin {name} is present"),
        SkinSelection::Named(name) => {
            format!("skin {name} is not present; showing Default fallback")
        }
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

fn update_accessibility_toggle(
    accessibility: &mut Mut<'_, AccessibilityNode>,
    selected: bool,
) -> bool {
    let toggled = Toggled::from(selected);
    let changed = {
        let accessibility = accessibility.bypass_change_detection();
        if accessibility.toggled() == Some(toggled) {
            false
        } else {
            accessibility.set_toggled(toggled);
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

fn scroll_catalog_lists(
    mut wheel: MessageReader<'_, '_, MouseWheel>,
    mut animation_lists: Query<
        '_,
        '_,
        (&mut ScrollPosition, &RelativeCursorPosition),
        With<AnimationList>,
    >,
    mut skin_lists: Query<'_, '_, (&mut ScrollPosition, &RelativeCursorPosition), With<SkinList>>,
    mut sidebars: Query<
        '_,
        '_,
        (&mut ScrollPosition, &RelativeCursorPosition),
        With<SidebarScroll>,
    >,
) {
    let delta = wheel.read().fold(Vec2::ZERO, |total, event| {
        let scale = match event.unit {
            MouseScrollUnit::Line => 24.0,
            MouseScrollUnit::Pixel => 1.0,
        };
        total + Vec2::new(event.x, event.y) * scale
    });
    if delta == Vec2::ZERO {
        return;
    }
    for (mut scroll, cursor) in &mut skin_lists {
        if cursor.cursor_over() {
            let horizontal = if delta.x == 0.0 { delta.y } else { delta.x };
            scroll.0.x = (scroll.0.x - horizontal).max(0.0);
            return;
        }
    }
    for (mut scroll, cursor) in &mut animation_lists {
        if cursor.cursor_over() {
            scroll.0.y = (scroll.0.y - delta.y).max(0.0);
            return;
        }
    }
    for (mut scroll, cursor) in &mut sidebars {
        if cursor.cursor_over() {
            scroll.0.y = (scroll.0.y - delta.y).max(0.0);
            return;
        }
    }
}

fn review_layout(window: &Window, has_comparison: bool) -> ReviewLayout {
    ReviewLayout::new(
        UVec2::new(window.physical_width(), window.physical_height()),
        window.scale_factor(),
        has_comparison,
        ui::SIDEBAR_WIDTH,
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
        let present_context = SourceAccessibilityContext {
            title: "Current",
            load_state: &ViewerLoadState::Ready,
            has_runtime_issue: false,
            selected_animation: Some("walk"),
            selected_present: true,
            selected_skin: &SkinSelection::Default,
            selected_skin_present: true,
        };
        let present = source_accessibility_summary(present_context);
        assert_eq!(
            present,
            "Current status: animation walk is present; skin Default is selected; runtime findings: none"
        );
        assert!(!present.contains("0.000"));
        for transient_state in [
            SpinalInstanceState::Loading,
            SpinalInstanceState::Ready,
            SpinalInstanceState::ReadyNoDraws,
            SpinalInstanceState::Degraded,
            SpinalInstanceState::DegradedNoDraws,
            SpinalInstanceState::Failed,
        ] {
            assert_eq!(
                source_accessibility_summary(present_context),
                present,
                "transient visual runtime state {transient_state} must not enter the live status"
            );
        }

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

        let setup_pose = source_accessibility_summary(SourceAccessibilityContext {
            title: "Current",
            load_state: &ViewerLoadState::Ready,
            has_runtime_issue: true,
            selected_animation: Some("jump"),
            selected_present: false,
            selected_skin: &SkinSelection::Named("hat".into()),
            selected_skin_present: false,
        });
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
            Some(
                "Current status: animation jump is not present; showing setup pose; skin hat is not present; showing Default fallback; runtime findings are present; see Diagnostics"
            )
        );

        let failed = source_accessibility_summary(SourceAccessibilityContext {
            title: "Current",
            load_state: &ViewerLoadState::Failed("atlas missing".into()),
            has_runtime_issue: true,
            selected_animation: None,
            selected_present: false,
            selected_skin: &SkinSelection::Default,
            selected_skin_present: true,
        });
        assert_eq!(
            failed,
            "Current status: failed: atlas missing; runtime findings are present; see Diagnostics"
        );
    }

    #[test]
    fn pane_skin_status_explicitly_names_default_fallback() {
        assert_eq!(
            visible_skin_status(&SkinSelection::Named("hat".into()), false),
            "skin “hat” not present • Default fallback"
        );
        assert_eq!(
            accessible_skin_status(&SkinSelection::Named("hat".into()), false),
            "skin hat is not present; showing Default fallback"
        );
        assert_eq!(
            visible_skin_status(&SkinSelection::Default, true),
            "skin Default"
        );
    }

    #[test]
    fn selection_button_toggle_accessibility_changes_only_with_selection() {
        let mut node = accesskit::Node::new(accesskit::Role::Button);
        node.set_toggled(Toggled::False);
        let mut world = World::new();
        let entity = world.spawn(AccessibilityNode(node)).id();
        world.clear_trackers();

        {
            let mut entity_mut = world.entity_mut(entity);
            let mut accessibility = entity_mut
                .get_mut::<AccessibilityNode>()
                .expect("skin button accessibility node");
            assert!(update_accessibility_toggle(&mut accessibility, true));
        }
        let accessibility = world
            .entity(entity)
            .get_ref::<AccessibilityNode>()
            .expect("skin button accessibility node");
        assert!(accessibility.is_changed());
        assert_eq!(accessibility.role(), accesskit::Role::Button);
        assert_eq!(accessibility.toggled(), Some(Toggled::True));

        world.clear_trackers();
        {
            let mut entity_mut = world.entity_mut(entity);
            let mut accessibility = entity_mut
                .get_mut::<AccessibilityNode>()
                .expect("skin button accessibility node");
            assert!(!update_accessibility_toggle(&mut accessibility, true));
        }
        assert!(
            !world
                .entity(entity)
                .get_ref::<AccessibilityNode>()
                .expect("skin button accessibility node")
                .is_changed()
        );
    }

    #[test]
    fn pause_action_accessible_label_changes_only_when_action_changes() {
        let resume = ui::pause_action_copy(true).accessible_label.to_owned();
        let pause = ui::pause_action_copy(false).accessible_label.to_owned();
        let mut node = accesskit::Node::new(accesskit::Role::Button);
        node.set_label(resume.clone());
        let mut world = World::new();
        let entity = world.spawn(AccessibilityNode(node)).id();
        world.clear_trackers();

        {
            let mut entity_mut = world.entity_mut(entity);
            let mut accessibility = entity_mut
                .get_mut::<AccessibilityNode>()
                .expect("pause button accessibility node");
            assert!(!update_accessibility_summary(&mut accessibility, resume));
        }
        assert!(
            !world
                .entity(entity)
                .get_ref::<AccessibilityNode>()
                .expect("pause button accessibility node")
                .is_changed()
        );

        {
            let mut entity_mut = world.entity_mut(entity);
            let mut accessibility = entity_mut
                .get_mut::<AccessibilityNode>()
                .expect("pause button accessibility node");
            assert!(update_accessibility_summary(&mut accessibility, pause));
        }
        assert_eq!(
            world
                .entity(entity)
                .get::<AccessibilityNode>()
                .and_then(|node| node.label()),
            Some("Pause animation")
        );
    }

    #[test]
    fn focus_reveal_scrolls_only_when_an_item_crosses_a_viewport_edge() {
        assert_eq!(revealed_scroll_x(40.0, 10.0, 310.0, 40.0, 120.0, 1.0), 40.0);
        assert_eq!(
            revealed_scroll_x(40.0, 10.0, 310.0, 300.0, 350.0, 1.0),
            80.0
        );
        assert_eq!(
            revealed_scroll_x(100.0, 10.0, 310.0, -30.0, 20.0, 1.0),
            60.0
        );
    }

    #[test]
    fn focus_reveal_converts_physical_delta_and_never_scrolls_before_default() {
        assert_eq!(revealed_scroll_x(20.0, 0.0, 300.0, 320.0, 380.0, 0.5), 60.0);
        assert_eq!(revealed_scroll_x(10.0, 0.0, 300.0, -30.0, 20.0, 0.5), 0.0);
        assert_eq!(revealed_scroll_x(25.0, 300.0, 0.0, 20.0, 40.0, 1.0), 25.0);
    }

    #[test]
    fn focus_reveal_rebases_an_overscrolled_request_on_effective_layout_scroll() {
        let raw_requested_scroll = 1_000.0;
        let computed = ComputedNode {
            scroll_position: Vec2::new(0.0, 100.0),
            inverse_scale_factor: 0.5,
            ..default()
        };
        let effective_scroll = effective_logical_scroll(&computed).y;

        assert_eq!(effective_scroll, 50.0);
        assert_eq!(
            revealed_scroll_y(effective_scroll, 10.0, 310.0, -30.0, 20.0, 0.5),
            30.0
        );
        assert_eq!(
            revealed_scroll_y(raw_requested_scroll, 10.0, 310.0, -30.0, 20.0, 0.5),
            980.0,
            "the raw request is intentionally far beyond Bevy's applied scroll"
        );
    }

    #[test]
    fn focus_reveal_scrolls_animation_and_sidebar_axes_vertically() {
        assert_eq!(revealed_scroll_y(40.0, 10.0, 310.0, 40.0, 120.0, 1.0), 40.0);
        assert_eq!(
            revealed_scroll_y(40.0, 10.0, 310.0, 300.0, 350.0, 1.0),
            80.0
        );
        assert_eq!(
            revealed_scroll_y(100.0, 10.0, 310.0, -30.0, 20.0, 1.0),
            60.0
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

//! Private Bevy integration for the read-only Spinal viewer.

use std::{collections::VecDeque, path::PathBuf, sync::Arc, time::Duration};

use accesskit::Action;
use bevy::{
    a11y::ActionRequest,
    asset::{AssetPlugin, AssetServer, Assets, LoadState},
    ecs::message::MessageReader,
    input::mouse::{MouseScrollUnit, MouseWheel},
    input_focus::{InputDispatchPlugin, InputFocus, tab_navigation::TabNavigationPlugin},
    prelude::*,
    ui::InteractionDisabled,
    window::{PrimaryWindow, WindowResizeConstraints},
};
use bevy_spinal::{
    SpinalAnimator, SpinalAsset, SpinalAssetLoaderSettings, SpinalInstance, SpinalInstanceState,
    SpinalIssue, SpinalPlaybackState, SpinalPlugin, SpinalRuntimeConfig, SpinalSet,
    spinal::{
        AnimationPlayer, DrawItemRef, PlayOptions, PlaybackMode, Skeleton, SkeletonAsset,
        Transition,
    },
};

use crate::{
    command::{StepDirection, ViewerCommand},
    preview::{PreviewEffect, PreviewRate, PreviewTransport, SelectionMode, SelectionTransition},
    ui::{self, AnimationList, PauseButtonLabel, ViewerAction, ViewerButton, ViewerLabel},
};

const MAX_ISSUE_HISTORY: usize = 8;
const DEFAULT_WINDOW_SIZE: Vec2 = Vec2::new(1120.0, 720.0);

#[derive(Clone, Debug)]
pub(crate) struct LaunchConfig {
    pub(crate) asset_root: PathBuf,
    pub(crate) asset_path: String,
    pub(crate) atlas_path: Option<String>,
    pub(crate) display_path: String,
    pub(crate) atlas_display_path: String,
    pub(crate) atlas_page_count: usize,
    pub(crate) premultiplied_pages: Vec<Box<str>>,
    pub(crate) preflight_skeleton: Arc<SkeletonAsset>,
    pub(crate) preview_rate: PreviewRate,
}

pub(crate) fn run(config: LaunchConfig) -> AppExit {
    let asset_root = config.asset_root.to_string_lossy().into_owned();
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.025, 0.030, 0.041)))
        .insert_resource(viewer_runtime_config())
        .insert_resource(ViewerLaunch(config))
        .init_resource::<InputFocus>()
        .init_resource::<CommandInbox>()
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: asset_root,
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Spinal animation viewer".into(),
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
        .add_plugins((SpinalPlugin, InputDispatchPlugin, TabNavigationPlugin))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            poll_asset
                .after(SpinalSet::Prepare)
                .before(SpinalSet::Animate),
        )
        .add_systems(
            Update,
            (
                handle_buttons,
                handle_accessibility_actions,
                handle_shortcuts,
            )
                .after(poll_asset)
                .before(SpinalSet::Animate),
        )
        .add_systems(
            Update,
            apply_commands
                .after(handle_buttons)
                .after(handle_accessibility_actions)
                .after(handle_shortcuts)
                .before(SpinalSet::Animate),
        )
        .add_systems(
            Update,
            (
                observe_runtime,
                observe_issues,
                sync_button_availability,
                update_button_visuals,
                update_focus_outline,
                update_labels,
                scroll_animation_list,
            )
                .chain()
                .after(SpinalSet::Animate),
        )
        .run()
}

fn viewer_runtime_config() -> SpinalRuntimeConfig {
    let mut config = SpinalRuntimeConfig::default();
    // The dedicated viewer reports degradation in its status panel. Keeping
    // world-space diagnostic crosses off prevents them obscuring the artwork.
    config.set_diagnostic_markers(false);
    config
}

#[derive(Resource)]
struct ViewerLaunch(LaunchConfig);

#[derive(Clone, Debug, Eq, PartialEq)]
enum ViewerLoadState {
    Loading,
    Ready,
    Failed(Box<str>),
}

#[derive(Resource)]
struct ViewerSession {
    entity: Entity,
    asset: Handle<SpinalAsset>,
    load_state: ViewerLoadState,
    runtime_state: SpinalInstanceState,
    catalog: Vec<(Box<str>, Duration)>,
    spine_version: Option<Box<str>>,
    transport: PreviewTransport,
    compatibility_warning: Option<Box<str>>,
    latest_issue: Option<Box<str>>,
    issue_history: VecDeque<Box<str>>,
}

impl ViewerSession {
    fn controls_ready(&self) -> bool {
        self.load_state == ViewerLoadState::Ready
            && self.transport.is_ready()
            && self.runtime_state.is_usable()
    }

    fn selected_entry(&self) -> Option<(usize, &str, Duration)> {
        let index = self.transport.selected_animation()?;
        let (name, duration) = self.catalog.get(index)?;
        Some((index, name, *duration))
    }

    fn selected_name(&self) -> Option<&str> {
        self.selected_entry().map(|(_index, name, _duration)| name)
    }
}

#[derive(Default, Resource)]
struct CommandInbox(Vec<ViewerCommand>);

fn setup(
    mut commands: Commands<'_, '_>,
    launch: Res<'_, ViewerLaunch>,
    asset_server: Res<'_, AssetServer>,
) {
    commands.spawn(Camera2d);
    let asset = load_prepared_asset(&asset_server, &launch.0);
    let entity = commands
        .spawn((SpinalInstance::new(asset.clone()), Transform::default()))
        .id();
    let catalog = launch
        .0
        .preflight_skeleton
        .animations()
        .map(|animation| (animation.name().into(), animation.duration()))
        .collect();
    let session = ViewerSession {
        entity,
        asset,
        load_state: ViewerLoadState::Loading,
        runtime_state: SpinalInstanceState::Loading,
        catalog,
        spine_version: Some(launch.0.preflight_skeleton.spine_version().into()),
        transport: PreviewTransport::new(launch.0.preview_rate),
        compatibility_warning: premultiplied_alpha_issue(&launch.0.premultiplied_pages)
            .map(Into::into),
        latest_issue: None,
        issue_history: VecDeque::new(),
    };
    commands.insert_resource(session);
    ui::spawn(&mut commands);
}

fn premultiplied_alpha_issue(pages: &[Box<str>]) -> Option<String> {
    (!pages.is_empty()).then(|| {
        format!(
            "Unsupported premultiplied alpha on {}; re-export with Premultiply alpha off and Bleed on",
            pages
                .iter()
                .map(AsRef::as_ref)
                .collect::<Vec<_>>()
                .join(", ")
        )
    })
}

/// The only bridge between source preparation and Bevy's compound loader.
fn load_prepared_asset(asset_server: &AssetServer, config: &LaunchConfig) -> Handle<SpinalAsset> {
    let atlas_path = config.atlas_path.clone();
    asset_server.load_with_settings::<SpinalAsset, SpinalAssetLoaderSettings>(
        config.asset_path.clone(),
        move |settings| settings.atlas_path.clone_from(&atlas_path),
    )
}

fn poll_asset(
    mut commands: Commands<'_, '_>,
    asset_server: Res<'_, AssetServer>,
    assets: Res<'_, Assets<SpinalAsset>>,
    windows: Query<'_, '_, &Window, With<PrimaryWindow>>,
    lists: Query<'_, '_, Entity, With<AnimationList>>,
    mut session: ResMut<'_, ViewerSession>,
    mut instance: Query<'_, '_, (&mut SpinalAnimator, &mut Transform)>,
) {
    if session.load_state != ViewerLoadState::Loading {
        return;
    }
    match asset_server.load_state(&session.asset) {
        LoadState::NotLoaded | LoadState::Loading => {}
        LoadState::Failed(error) => {
            session.transport.mark_unready();
            session.load_state = ViewerLoadState::Failed(error.to_string().into());
        }
        LoadState::Loaded => {
            let Some(asset) = assets.get(&session.asset) else {
                return;
            };
            session.catalog = asset
                .skeleton()
                .animations()
                .map(|animation| (animation.name().into(), animation.duration()))
                .collect();
            session.spine_version = Some(asset.skeleton().spine_version().into());
            session.load_state = ViewerLoadState::Ready;
            let durations = session
                .catalog
                .iter()
                .map(|(_name, duration)| *duration)
                .collect::<Vec<_>>();
            let initial = session.transport.replace_catalog(durations);

            if let Ok(list) = lists.single() {
                ui::rebuild_animation_list(&mut commands, list, &session.catalog);
            }
            if let Ok((mut animator, mut transform)) = instance.get_mut(session.entity) {
                if let Some(effect) = initial {
                    apply_playback_effect(effect, &session, &mut animator);
                }
                if let Ok(window) = windows.single() {
                    *transform = fitted_transform(asset, &session, window_size(window));
                }
            }
        }
    }
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
            inbox.0.push(action.0);
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
        inbox.0.push(action.0);
    }
}

fn handle_shortcuts(
    keys: Res<'_, ButtonInput<KeyCode>>,
    focus: Res<'_, InputFocus>,
    actions: Query<'_, '_, (&ViewerAction, Option<&InteractionDisabled>)>,
    mut inbox: ResMut<'_, CommandInbox>,
) {
    let focused = focus.0.and_then(|entity| {
        actions
            .get(entity)
            .ok()
            .map(|(action, disabled)| (action.0, disabled.is_none()))
    });
    if let Some(command) = keyboard_activation_command(
        focused,
        keys.just_pressed(KeyCode::Enter),
        keys.just_pressed(KeyCode::Space),
    ) {
        inbox.0.push(command);
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
            && let Some(command) = crate::command::command_for_digit(digit)
        {
            inbox.0.push(command);
        }
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        inbox.0.push(ViewerCommand::Step(StepDirection::Backward));
    }
    if keys.just_pressed(KeyCode::ArrowRight) {
        inbox.0.push(ViewerCommand::Step(StepDirection::Forward));
    }
    if keys.just_pressed(KeyCode::KeyR) {
        inbox.0.push(ViewerCommand::Restart);
    }
    if keys.just_pressed(KeyCode::KeyF) {
        inbox.0.push(ViewerCommand::Refit);
    }
}

const fn keyboard_activation_command(
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

fn apply_commands(
    mut inbox: ResMut<'_, CommandInbox>,
    mut session: ResMut<'_, ViewerSession>,
    assets: Res<'_, Assets<SpinalAsset>>,
    windows: Query<'_, '_, &Window, With<PrimaryWindow>>,
    mut instance: Query<'_, '_, (&mut SpinalAnimator, &mut Transform)>,
) {
    let queued = std::mem::take(&mut inbox.0);
    if queued.is_empty() {
        return;
    }
    let Ok((mut animator, mut transform)) = instance.get_mut(session.entity) else {
        return;
    };
    for command in queued {
        if !ui::command_is_available(command, session.controls_ready(), session.catalog.len()) {
            continue;
        }
        let effect = match session.transport.handle(command) {
            Ok(effect) => effect,
            Err(error) => {
                record_local_issue(&mut session, format!("preview command failed: {error}"));
                None
            }
        };
        let Some(effect) = effect else {
            continue;
        };
        if effect == PreviewEffect::Refit {
            if let (Some(asset), Ok(window)) = (assets.get(&session.asset), windows.single()) {
                *transform = fitted_transform(asset, &session, window_size(window));
            }
        } else {
            apply_playback_effect(effect, &session, &mut animator);
        }
    }
}

fn apply_playback_effect(
    effect: PreviewEffect,
    session: &ViewerSession,
    animator: &mut SpinalAnimator,
) {
    match effect {
        PreviewEffect::Select(request) => {
            let Some((name, _duration)) = session.catalog.get(request.animation_index) else {
                return;
            };
            let mode = match request.mode {
                SelectionMode::Loop => PlaybackMode::Loop,
            };
            let transition = match request.transition {
                SelectionTransition::Immediate => Transition::Immediate,
            };
            animator.play(name.clone(), mode, transition);
            animator.seek_to(request.start_at);
            animator.set_paused(request.paused);
        }
        PreviewEffect::SetPaused { paused, position } => {
            animator.seek_to(position);
            animator.set_paused(paused);
        }
        PreviewEffect::SeekAndPause(request) => {
            if session.transport.selected_animation() == Some(request.animation_index) {
                animator.set_paused(true);
                animator.seek_to(request.position);
            }
        }
        PreviewEffect::Refit => {}
    }
}

fn observe_runtime(
    mut session: ResMut<'_, ViewerSession>,
    runtime: Query<'_, '_, (&SpinalInstanceState, &SpinalPlaybackState)>,
) {
    let Ok((state, playback)) = runtime.get(session.entity) else {
        return;
    };
    session.runtime_state = state.clone();
    if let Some(position) = playback.position() {
        session.transport.observe_position(position);
    }
}

fn observe_issues(
    mut issues: MessageReader<'_, '_, SpinalIssue>,
    mut session: ResMut<'_, ViewerSession>,
) {
    let entity = session.entity;
    for issue in issues.read().filter(|issue| issue.entity() == entity) {
        let track = issue
            .track()
            .map(|track| format!(" track `{track}`"))
            .unwrap_or_default();
        record_local_issue(
            &mut session,
            format!("{:?}{track}: {}", issue.kind(), issue.message()),
        );
    }
}

fn record_local_issue(session: &mut ViewerSession, detail: String) {
    let detail: Box<str> = detail.into();
    session.latest_issue = Some(detail.clone());
    session.issue_history.push_front(detail);
    session.issue_history.truncate(MAX_ISSUE_HISTORY);
}

fn sync_button_availability(
    mut commands: Commands<'_, '_>,
    session: Res<'_, ViewerSession>,
    buttons: Query<'_, '_, (Entity, &ViewerAction, Option<&InteractionDisabled>)>,
) {
    for (entity, action, disabled) in &buttons {
        let enabled =
            ui::command_is_available(action.0, session.controls_ready(), session.catalog.len());
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
    session: Res<'_, ViewerSession>,
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
            action.0,
            ViewerCommand::SelectAnimation(index)
                if session.transport.selected_animation() == Some(index)
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
        **text = if session.transport.is_paused() {
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
    launch: Res<'_, ViewerLaunch>,
    session: Res<'_, ViewerSession>,
    mut labels: Query<'_, '_, (&ViewerLabel, &mut Text, &mut TextColor)>,
) {
    let selected = session.selected_entry();
    let position = session.transport.position();
    for (marker, mut text, mut color) in &mut labels {
        let (value, value_color) = match marker {
            ViewerLabel::Source => (format!("File: {}", launch.0.display_path), ui::MUTED_TEXT),
            ViewerLabel::Version => (
                format!(
                    "Spine version: {}",
                    session.spine_version.as_deref().unwrap_or("-")
                ),
                ui::MUTED_TEXT,
            ),
            ViewerLabel::Current => (
                selected.map_or_else(
                    || "Animation: -".to_owned(),
                    |(index, name, _duration)| {
                        format!("Animation {}/{}: {name}", index + 1, session.catalog.len())
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
                    session.transport.frame_index(),
                    session.transport.rate().fps()
                ),
                ui::TEXT,
            ),
            ViewerLabel::RuntimeState => {
                let color = runtime_state_color(&session.runtime_state);
                (format!("Runtime state: {}", session.runtime_state), color)
            }
            ViewerLabel::LoadStatus => match &session.load_state {
                ViewerLoadState::Loading => ("Load status: loading".to_owned(), ui::MUTED_TEXT),
                ViewerLoadState::Ready => (
                    format!(
                        "Load status: asset linked | {} | {} page(s)",
                        launch.0.atlas_display_path, launch.0.atlas_page_count
                    ),
                    ui::SUCCESS,
                ),
                ViewerLoadState::Failed(error) => (format!("Load failed: {error}"), ui::ERROR),
            },
            ViewerLabel::Compatibility => match &session.compatibility_warning {
                Some(warning) => (format!("Source compatibility: {warning}"), ui::ERROR),
                None => ("Source compatibility: ready".to_owned(), ui::SUCCESS),
            },
            ViewerLabel::LatestIssue => (
                format!(
                    "Latest runtime issue (history, not active status): {}",
                    session.latest_issue.as_deref().unwrap_or("none")
                ),
                if session.latest_issue.is_some() {
                    ui::WARNING
                } else {
                    ui::MUTED_TEXT
                },
            ),
            ViewerLabel::IssueHistory => (
                if session.issue_history.is_empty() {
                    "Issue history (observations, newest first): none".to_owned()
                } else {
                    format!(
                        "Issue history (observations, newest first; not active-state list):\n{}",
                        session
                            .issue_history
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

fn fitted_transform(asset: &SpinalAsset, session: &ViewerSession, window_size: Vec2) -> Transform {
    let bounds = sampled_bounds(asset, session.selected_name(), session.transport.position());
    fit_transform(bounds, window_size)
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

fn fit_transform(bounds: Option<GeometryBounds>, window_size: Vec2) -> Transform {
    let window_size = if window_size.is_finite() && window_size.min_element() > 0.0 {
        window_size
    } else {
        DEFAULT_WINDOW_SIZE
    };
    let sidebar = ui::SIDEBAR_WIDTH.min(window_size.x.max(0.0));
    let preview_size = Vec2::new((window_size.x - sidebar).max(1.0), window_size.y.max(1.0));
    let preview_center = Vec2::new(-sidebar * 0.5, 0.0);
    let available = (preview_size - Vec2::splat(ui::PREVIEW_PADDING * 2.0)).max(Vec2::ONE);

    let Some(bounds) = bounds else {
        return Transform::from_translation(preview_center.extend(0.0));
    };
    let size = bounds.size();
    if !size.is_finite() || size.min_element() < 0.0 {
        return Transform::from_translation(preview_center.extend(0.0));
    }
    let scale_x = (size.x > f32::EPSILON).then_some(available.x / size.x);
    let scale_y = (size.y > f32::EPSILON).then_some(available.y / size.y);
    let scale = match (scale_x, scale_y) {
        (Some(x), Some(y)) => x.min(y),
        (Some(x), None) => x,
        (None, Some(y)) => y,
        (None, None) => 1.0,
    };
    let center = bounds.center();
    let translation = preview_center - center * scale;
    if !scale.is_finite() || scale <= 0.0 || !translation.is_finite() {
        return Transform::from_translation(preview_center.extend(0.0));
    }
    Transform::from_translation(translation.extend(0.0)).with_scale(Vec3::splat(scale))
}

fn window_size(window: &Window) -> Vec2 {
    Vec2::new(window.width(), window.height())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_is_uniform_centered_and_excludes_the_sidebar() {
        let transform = fit_transform(
            Some(GeometryBounds {
                min: Vec2::new(-50.0, -100.0),
                max: Vec2::new(50.0, 100.0),
            }),
            DEFAULT_WINDOW_SIZE,
        );

        assert_eq!(transform.scale.x, transform.scale.y);
        assert_eq!(transform.scale.y, transform.scale.z);
        assert_eq!(transform.translation.x, -ui::SIDEBAR_WIDTH * 0.5);
        assert_eq!(transform.translation.y, 0.0);
        let fitted_height = 200.0 * transform.scale.y;
        assert!(fitted_height <= DEFAULT_WINDOW_SIZE.y - ui::PREVIEW_PADDING * 2.0);
    }

    #[test]
    fn empty_nonfinite_and_degenerate_geometry_have_safe_fallbacks() {
        let empty = fit_transform(None, Vec2::splat(f32::NAN));
        assert!(empty.translation.is_finite());
        assert_eq!(empty.scale, Vec3::ONE);

        let points = GeometryBounds::from_points([
            Vec2::splat(f32::NAN),
            Vec2::new(2.0, 3.0),
            Vec2::splat(f32::INFINITY),
        ])
        .expect("one finite point remains usable");
        let degenerate = fit_transform(Some(points), DEFAULT_WINDOW_SIZE);
        assert!(degenerate.translation.is_finite());
        assert!(degenerate.scale.is_finite());
        assert!(degenerate.scale.x > 0.0);
    }

    #[test]
    fn issue_history_is_bounded_without_claiming_current_activity() {
        let mut history = VecDeque::new();
        for index in 0..MAX_ISSUE_HISTORY + 3 {
            history.push_front(Box::<str>::from(format!("issue {index}")));
            history.truncate(MAX_ISSUE_HISTORY);
        }
        assert_eq!(history.len(), MAX_ISSUE_HISTORY);
        assert_eq!(history.front().map(AsRef::as_ref), Some("issue 10"));
    }

    #[test]
    fn viewer_runtime_policy_disables_world_space_diagnostic_markers() {
        assert!(!viewer_runtime_config().diagnostic_markers());
    }

    #[test]
    fn preview_time_error_remains_displayable_for_ui_history() {
        assert!(
            !crate::preview::PreviewTimeError::Overflow
                .to_string()
                .is_empty()
        );
    }

    #[test]
    fn space_activates_focus_without_also_toggling_transport() {
        let selection = ViewerCommand::SelectAnimation(4);
        assert_eq!(
            keyboard_activation_command(Some((selection, true)), false, true),
            Some(selection)
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

    #[test]
    fn premultiplied_alpha_warning_names_pages_and_the_export_fix() {
        assert_eq!(premultiplied_alpha_issue(&[]), None);
        let issue = premultiplied_alpha_issue(&["cat.png".into(), "eyes.png".into()])
            .expect("PMA pages need an actionable issue");
        assert!(issue.contains("cat.png, eyes.png"));
        assert!(issue.contains("Premultiply alpha off"));
        assert!(issue.contains("Bleed on"));
    }
}

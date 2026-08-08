//! Private Bevy integration for the read-only Spinal viewer.

use std::{collections::VecDeque, sync::Arc, time::Duration};

use accesskit::Action;
use bevy::{
    a11y::{AccessibilityNode, ActionRequest},
    asset::{
        AssetApp, AssetPath, AssetPlugin, AssetServer, Assets, LoadState, io::AssetSourceBuilder,
    },
    camera::{ClearColorConfig, Viewport, visibility::RenderLayers},
    ecs::message::MessageReader,
    input::mouse::{MouseScrollUnit, MouseWheel},
    input_focus::{InputDispatchPlugin, InputFocus, tab_navigation::TabNavigationPlugin},
    prelude::*,
    ui::InteractionDisabled,
    window::{PrimaryWindow, WindowResizeConstraints},
};
use bevy_spinal::{
    SpinalAnimator, SpinalAsset, SpinalAssetLoaderSettings, SpinalInstance, SpinalInstanceState,
    SpinalIssue, SpinalPlugin, SpinalRuntimeConfig, SpinalSet,
    spinal::{
        AnimationPlayer, DrawItemRef, PlayOptions, PlaybackMode, Skeleton, SkeletonAsset,
        Transition,
    },
};

use crate::{
    bundle::SourceBundle,
    clock::AdvanceBoundary,
    command::{PlaybackCommand, StepDirection, ViewerCommand, source_animation_index},
    layout::ReviewLayout,
    preview::{PreviewEffect, PreviewRate, SelectionMode, SelectionTransition},
    session::{SourceReadiness, SourceSlot, ViewerSession},
    ui::{
        self, AnimationList, PauseButtonLabel, SourceStatusLabel, ViewerAction, ViewerButton,
        ViewerLabel,
    },
};

const MAX_ISSUE_HISTORY: usize = 8;
const DEFAULT_WINDOW_SIZE: Vec2 = Vec2::new(1120.0, 720.0);
const PRIMARY_ASSET_SOURCE: &str = "spinal-primary";
const COMPARISON_ASSET_SOURCE: &str = "spinal-comparison";
const PRIMARY_RENDER_LAYER: usize = 1;
const COMPARISON_RENDER_LAYER: usize = 2;
const UI_RENDER_LAYER: usize = 3;

#[derive(Clone, Debug)]
pub(crate) struct LaunchSource {
    pub(crate) bundle: SourceBundle,
    pub(crate) display_path: String,
    pub(crate) atlas_display_path: String,
    pub(crate) atlas_page_count: usize,
    pub(crate) premultiplied_pages: Vec<Box<str>>,
    pub(crate) preflight_skeleton: Arc<SkeletonAsset>,
}

#[derive(Clone, Debug)]
pub(crate) struct LaunchConfig {
    pub(crate) primary: LaunchSource,
    pub(crate) comparison: Option<LaunchSource>,
    pub(crate) preview_rate: PreviewRate,
}

pub(crate) fn run(config: LaunchConfig) -> AppExit {
    let primary_reader = config.primary.bundle.memory_reader();
    let mut app = App::new();
    app.register_asset_source(
        PRIMARY_ASSET_SOURCE,
        AssetSourceBuilder::new(move || Box::new(primary_reader.clone())),
    );
    if let Some(comparison) = &config.comparison {
        let comparison_reader = comparison.bundle.memory_reader();
        app.register_asset_source(
            COMPARISON_ASSET_SOURCE,
            AssetSourceBuilder::new(move || Box::new(comparison_reader.clone())),
        );
    }
    app.insert_resource(ClearColor(Color::srgb(0.025, 0.030, 0.041)))
        .insert_resource(viewer_runtime_config())
        .insert_resource(ViewerLaunch(config))
        .init_resource::<InputFocus>()
        .init_resource::<CommandInbox>()
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    watch_for_changes_override: Some(false),
                    use_asset_processor_override: Some(false),
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
            advance_review_clock
                .after(apply_commands)
                .before(SpinalSet::Animate),
        )
        .add_systems(
            Update,
            (
                release_deferred_playback,
                observe_runtime,
                observe_issues,
                update_viewports_and_refit,
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

#[derive(Component)]
struct PreviewCamera;

#[derive(Component)]
struct ViewerUiCamera;

struct RuntimeSource {
    slot: SourceSlot,
    entity: Entity,
    camera: Entity,
    asset: Handle<SpinalAsset>,
    load_state: ViewerLoadState,
    runtime_state: SpinalInstanceState,
    display_path: Box<str>,
    atlas_display_path: Box<str>,
    atlas_page_count: usize,
    spine_version: Option<Box<str>>,
    compatibility_warning: Option<Box<str>>,
    selected_present: bool,
}

#[derive(Resource)]
struct AppSession {
    sources: Vec<RuntimeSource>,
    model: ViewerSession,
    latest_issue: Option<Box<str>>,
    issue_history: VecDeque<Box<str>>,
    suppress_clock_advance: bool,
    resume_after_animate: bool,
}

impl AppSession {
    fn controls_ready(&self) -> bool {
        self.sources
            .iter()
            .all(|source| source.load_state == ViewerLoadState::Ready)
            && self.model.all_present_sources_ready()
            && self.model.transport().is_ready()
            && self
                .sources
                .iter()
                .all(|source| source.runtime_state.is_usable())
    }

    fn selected_entry(&self) -> Option<(usize, &str, Duration)> {
        let selected = self.model.transport().selected_animation()?;
        self.model
            .animations()
            .iter()
            .enumerate()
            .find_map(|(index, name)| {
                (name.as_ref() == selected).then(|| {
                    (
                        index,
                        name.as_ref(),
                        self.model
                            .review_duration(selected)
                            .unwrap_or(Duration::ZERO),
                    )
                })
            })
    }

    fn selected_name(&self) -> Option<&str> {
        self.selected_entry().map(|(_index, name, _duration)| name)
    }

    fn source(&self, slot: SourceSlot) -> Option<&RuntimeSource> {
        self.sources.iter().find(|source| source.slot == slot)
    }

    fn has_comparison(&self) -> bool {
        self.source(SourceSlot::Comparison).is_some()
    }
}

#[derive(Default, Resource)]
struct CommandInbox(Vec<ViewerCommand>);

fn setup(
    mut commands: Commands<'_, '_>,
    launch: Res<'_, ViewerLaunch>,
    asset_server: Res<'_, AssetServer>,
    windows: Query<'_, '_, &Window, With<PrimaryWindow>>,
) {
    let has_comparison = launch.0.comparison.is_some();
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

    let mut model = ViewerSession::new(launch.0.preview_rate);
    let mut sources = Vec::with_capacity(usize::from(has_comparison) + 1);
    for (slot, source) in [
        (SourceSlot::Primary, Some(&launch.0.primary)),
        (SourceSlot::Comparison, launch.0.comparison.as_ref()),
    ] {
        let Some(source) = source else {
            continue;
        };
        let catalog = source
            .preflight_skeleton
            .animations()
            .map(|animation| (animation.name().into(), animation.duration()))
            .collect::<Vec<_>>();
        model.set_source(slot, SourceReadiness::Loading, catalog);
        sources.push(spawn_runtime_source(
            &mut commands,
            &asset_server,
            slot,
            source,
            layout.viewport(slot == SourceSlot::Comparison).clone(),
        ));
    }
    debug_assert!((1..=2).contains(&sources.len()));
    let session = AppSession {
        sources,
        model,
        latest_issue: None,
        issue_history: VecDeque::new(),
        suppress_clock_advance: false,
        resume_after_animate: false,
    };
    commands.insert_resource(session);
    ui::spawn(&mut commands, ui_camera, has_comparison);
}

fn spawn_runtime_source(
    commands: &mut Commands<'_, '_>,
    asset_server: &AssetServer,
    slot: SourceSlot,
    launch: &LaunchSource,
    viewport: Viewport,
) -> RuntimeSource {
    let (asset_source, render_layer, camera_order) = source_render_spec(slot);
    let camera = commands
        .spawn((
            Camera2d,
            Camera {
                order: camera_order,
                viewport: Some(viewport),
                clear_color: ClearColorConfig::Custom(Color::srgb(0.025, 0.030, 0.041)),
                ..default()
            },
            RenderLayers::layer(render_layer),
            PreviewCamera,
        ))
        .id();
    let asset = load_prepared_asset(asset_server, launch, asset_source);
    let entity = commands
        .spawn((
            SpinalInstance::new(asset.clone()),
            Transform::default(),
            RenderLayers::layer(render_layer),
        ))
        .id();
    RuntimeSource {
        slot,
        entity,
        camera,
        asset,
        load_state: ViewerLoadState::Loading,
        runtime_state: SpinalInstanceState::Loading,
        display_path: launch.display_path.clone().into(),
        atlas_display_path: launch.atlas_display_path.clone().into(),
        atlas_page_count: launch.atlas_page_count,
        spine_version: Some(launch.preflight_skeleton.spine_version().into()),
        compatibility_warning: premultiplied_alpha_issue(&launch.premultiplied_pages)
            .map(Into::into),
        selected_present: true,
    }
}

const fn source_render_spec(slot: SourceSlot) -> (&'static str, usize, isize) {
    match slot {
        SourceSlot::Primary => (PRIMARY_ASSET_SOURCE, PRIMARY_RENDER_LAYER, 0),
        SourceSlot::Comparison => (COMPARISON_ASSET_SOURCE, COMPARISON_RENDER_LAYER, 1),
    }
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
fn load_prepared_asset(
    asset_server: &AssetServer,
    config: &LaunchSource,
    asset_source: &'static str,
) -> Handle<SpinalAsset> {
    let atlas_path = Some(config.bundle.atlas_reference().to_owned());
    let asset_path = AssetPath::from_path_buf(config.bundle.json_asset_path().to_owned())
        .with_source(asset_source);
    asset_server
        .load_with_settings::<SpinalAsset, SpinalAssetLoaderSettings>(asset_path, move |settings| {
            settings.atlas_path.clone_from(&atlas_path)
        })
}

fn poll_asset(
    mut commands: Commands<'_, '_>,
    asset_server: Res<'_, AssetServer>,
    assets: Res<'_, Assets<SpinalAsset>>,
    windows: Query<'_, '_, &Window, With<PrimaryWindow>>,
    lists: Query<'_, '_, Entity, With<AnimationList>>,
    mut session: ResMut<'_, AppSession>,
    mut instances: Query<'_, '_, (&mut SpinalAnimator, &mut Transform)>,
) {
    let mut initial = None;
    let mut transitioned_to_ready = false;
    for index in 0..session.sources.len() {
        if session.sources[index].load_state != ViewerLoadState::Loading {
            continue;
        }
        let asset_handle = session.sources[index].asset.clone();
        let slot = session.sources[index].slot;
        match asset_server.load_state(&asset_handle) {
            LoadState::NotLoaded | LoadState::Loading => {}
            LoadState::Failed(error) => {
                session.sources[index].load_state =
                    ViewerLoadState::Failed(error.to_string().into());
                session.model.set_readiness(slot, SourceReadiness::Failed);
            }
            LoadState::Loaded => {
                let Some(asset) = assets.get(&asset_handle) else {
                    continue;
                };
                let catalog = asset
                    .skeleton()
                    .animations()
                    .map(|animation| (animation.name().into(), animation.duration()))
                    .collect::<Vec<_>>();
                session.sources[index].spine_version =
                    Some(asset.skeleton().spine_version().into());
                session.sources[index].load_state = ViewerLoadState::Ready;
                transitioned_to_ready = true;
                if let Some(effect) =
                    session
                        .model
                        .set_source(slot, SourceReadiness::Ready, catalog)
                {
                    initial = Some(effect);
                }
            }
        }
    }

    if should_finalize_ready_transition(
        transitioned_to_ready,
        session.model.all_present_sources_ready(),
    ) {
        if let Ok(list) = lists.single() {
            ui::rebuild_animation_list(&mut commands, list, session.model.animations());
        }
        if let Some(effect) = initial {
            apply_preview_effect_to_all(effect, &mut session, &mut instances, true);
        }
        if let Ok(window) = windows.single() {
            for source in &session.sources {
                let Some(asset) = assets.get(&source.asset) else {
                    continue;
                };
                if let Ok((_animator, mut transform)) = instances.get_mut(source.entity) {
                    *transform = fitted_transform(asset, &session, source.slot, window);
                }
            }
        }
    }
}

const fn should_finalize_ready_transition(
    transitioned_to_ready: bool,
    all_present_sources_ready: bool,
) -> bool {
    transitioned_to_ready && all_present_sources_ready
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
            inbox.0.push(action.0.clone());
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
        inbox.0.push(action.0.clone());
    }
}

fn handle_shortcuts(
    keys: Res<'_, ButtonInput<KeyCode>>,
    focus: Res<'_, InputFocus>,
    session: Res<'_, AppSession>,
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
            && let Some(command) = selection_command_for_digit(digit, session.model.animations())
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

fn apply_commands(
    mut inbox: ResMut<'_, CommandInbox>,
    mut session: ResMut<'_, AppSession>,
    assets: Res<'_, Assets<SpinalAsset>>,
    windows: Query<'_, '_, &Window, With<PrimaryWindow>>,
    mut instances: Query<'_, '_, (&mut SpinalAnimator, &mut Transform)>,
) {
    let queued = std::mem::take(&mut inbox.0);
    if queued.is_empty() {
        return;
    }
    for command in queued {
        if !ui::command_is_available(
            &command,
            session.controls_ready(),
            session.model.animations().iter().map(AsRef::as_ref),
        ) {
            continue;
        }
        let effect = match session.model.handle(command) {
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
            if let Ok(window) = windows.single() {
                for source in &session.sources {
                    let Some(asset) = assets.get(&source.asset) else {
                        continue;
                    };
                    if let Ok((_animator, mut transform)) = instances.get_mut(source.entity) {
                        *transform = fitted_transform(asset, &session, source.slot, window);
                    }
                }
            }
        } else {
            session.suppress_clock_advance = true;
            apply_preview_effect_to_all(effect, &mut session, &mut instances, true);
        }
    }
}

fn apply_preview_effect_to_all(
    effect: PreviewEffect,
    session: &mut AppSession,
    instances: &mut Query<'_, '_, (&mut SpinalAnimator, &mut Transform)>,
    hold_resume_for_frame: bool,
) {
    let desired_paused = match &effect {
        PreviewEffect::Select(request) => Some(request.paused),
        PreviewEffect::SetPaused { paused, .. } => Some(*paused),
        PreviewEffect::SeekAndPause(_request) => Some(true),
        PreviewEffect::Refit => None,
    };
    session.resume_after_animate = desired_paused == Some(false) && hold_resume_for_frame;

    for index in 0..session.sources.len() {
        let slot = session.sources[index].slot;
        let entity = session.sources[index].entity;
        let selected = session.model.transport().selected_animation();
        let present = selected.is_some_and(|name| session.model.duration(slot, name).is_some());
        session.sources[index].selected_present = present;
        let Ok((mut animator, _transform)) = instances.get_mut(entity) else {
            continue;
        };
        if !present {
            animator.stop(Transition::Immediate);
            continue;
        }
        let projected = session
            .model
            .projected_position(slot)
            .ok()
            .flatten()
            .unwrap_or(Duration::ZERO);

        match &effect {
            PreviewEffect::Select(request) => {
                let mode = match request.mode {
                    SelectionMode::Loop => PlaybackMode::Loop,
                };
                let transition = match request.transition {
                    SelectionTransition::Immediate => Transition::Immediate,
                };
                animator.play(request.animation_name.clone(), mode, transition);
                animator.seek_to(projected);
                animator
                    .set_speed(request.playback_speed.multiplier())
                    .expect("the transport only retains valid playback speeds");
                animator.set_paused(request.paused || session.resume_after_animate);
            }
            PreviewEffect::SetPaused { paused, .. } => {
                animator.seek_to(projected);
                animator.set_paused(*paused || session.resume_after_animate);
            }
            PreviewEffect::SeekAndPause(request) => {
                debug_assert_eq!(
                    session.model.transport().selected_animation(),
                    Some(request.animation_name.as_ref())
                );
                animator.set_paused(true);
                animator.seek_to(projected);
            }
            PreviewEffect::Refit => {}
        }
    }
}

fn advance_review_clock(
    time: Res<'_, Time>,
    mut session: ResMut<'_, AppSession>,
    mut animators: Query<'_, '_, &mut SpinalAnimator>,
) {
    if std::mem::take(&mut session.suppress_clock_advance) {
        return;
    }
    let effect = match session
        .model
        .handle_playback(PlaybackCommand::Advance(time.delta()))
    {
        Ok(effect) => effect,
        Err(error) => {
            record_local_issue(&mut session, format!("preview clock failed: {error}"));
            return;
        }
    };
    let Some(effect) = effect else {
        return;
    };
    if effect.boundary == AdvanceBoundary::Wrapped {
        let present_sources = session
            .sources
            .iter()
            .filter(|source| source.selected_present)
            .map(|source| (source.entity, source.slot))
            .collect::<Vec<_>>();
        for (entity, slot) in present_sources {
            match wrap_rebase_position(&session.model, slot, effect.boundary) {
                Ok(Some(position)) => {
                    if let Ok(mut animator) = animators.get_mut(entity) {
                        // A fresh seek makes Spinal sample this correction with
                        // zero delta, then normal advancement resumes next frame.
                        animator.seek_to(position);
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    record_local_issue(
                        &mut session,
                        format!("preview wrap rebase failed: {error}"),
                    );
                    return;
                }
            }
        }
    }
    if matches!(
        effect.boundary,
        AdvanceBoundary::Completed | AdvanceBoundary::Empty
    ) || effect.update.paused
    {
        for source in &session.sources {
            if let Ok(mut animator) = animators.get_mut(source.entity) {
                animator.set_paused(true);
            }
        }
    }
}

fn wrap_rebase_position(
    model: &ViewerSession,
    slot: SourceSlot,
    boundary: AdvanceBoundary,
) -> Result<Option<Duration>, crate::preview::PreviewTimeError> {
    if boundary != AdvanceBoundary::Wrapped {
        Ok(None)
    } else {
        let Some(animation) = model.transport().selected_animation() else {
            return Ok(None);
        };
        let Some(review_duration) = model.review_duration(animation) else {
            return Ok(None);
        };
        let Some(source_duration) = model.duration(slot, animation) else {
            return Ok(None);
        };
        if source_duration.is_zero() || review_duration.as_nanos() % source_duration.as_nanos() == 0
        {
            return Ok(None);
        }
        model.projected_position(slot)
    }
}

fn release_deferred_playback(
    mut session: ResMut<'_, AppSession>,
    mut animators: Query<'_, '_, &mut SpinalAnimator>,
) {
    if !std::mem::take(&mut session.resume_after_animate) {
        return;
    }
    for source in &session.sources {
        if source.selected_present
            && let Ok(mut animator) = animators.get_mut(source.entity)
        {
            animator.set_paused(false);
        }
    }
}

fn observe_runtime(
    mut session: ResMut<'_, AppSession>,
    runtime: Query<'_, '_, &SpinalInstanceState>,
) {
    for source in &mut session.sources {
        if let Ok(state) = runtime.get(source.entity) {
            source.runtime_state = state.clone();
        }
    }
}

fn observe_issues(
    mut issues: MessageReader<'_, '_, SpinalIssue>,
    mut session: ResMut<'_, AppSession>,
) {
    for issue in issues.read() {
        let Some(source) = session
            .sources
            .iter()
            .find(|source| issue.entity() == source.entity)
        else {
            continue;
        };
        let source_name = source_slot_label(source.slot, session.has_comparison());
        let track = issue
            .track()
            .map(|track| format!(" track `{track}`"))
            .unwrap_or_default();
        record_local_issue(
            &mut session,
            format!(
                "{source_name} {:?}{track}: {}",
                issue.kind(),
                issue.message()
            ),
        );
    }
}

fn record_local_issue(session: &mut AppSession, detail: String) {
    let detail: Box<str> = detail.into();
    session.latest_issue = Some(detail.clone());
    session.issue_history.push_front(detail);
    session.issue_history.truncate(MAX_ISSUE_HISTORY);
}

fn update_viewports_and_refit(
    windows: Query<'_, '_, Ref<'_, Window>, With<PrimaryWindow>>,
    session: Res<'_, AppSession>,
    assets: Res<'_, Assets<SpinalAsset>>,
    mut cameras: Query<'_, '_, &mut Camera, With<PreviewCamera>>,
    mut instances: Query<'_, '_, &mut Transform, With<SpinalInstance>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    if !window.is_changed() {
        return;
    }
    let layout = review_layout(&window, session.has_comparison());
    for source in &session.sources {
        if let Ok(mut camera) = cameras.get_mut(source.camera) {
            camera.viewport = Some(
                layout
                    .viewport(source.slot == SourceSlot::Comparison)
                    .clone(),
            );
        }
        let Some(asset) = assets.get(&source.asset) else {
            continue;
        };
        if let Ok(mut transform) = instances.get_mut(source.entity) {
            *transform = fitted_transform(asset, &session, source.slot, &window);
        }
    }
}

fn sync_button_availability(
    mut commands: Commands<'_, '_>,
    session: Res<'_, AppSession>,
    buttons: Query<'_, '_, (Entity, &ViewerAction, Option<&InteractionDisabled>)>,
) {
    for (entity, action, disabled) in &buttons {
        let enabled = ui::command_is_available(
            &action.0,
            session.controls_ready(),
            session.model.animations().iter().map(AsRef::as_ref),
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
    session: Res<'_, AppSession>,
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
                if session.model.transport().selected_animation() == Some(name.as_ref())
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
        **text = if session.model.transport().is_paused() {
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
    session: Res<'_, AppSession>,
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
    let selected = session.selected_entry();
    let position = session.model.transport().position();
    let has_comparison = session.has_comparison();
    for (marker, mut text, mut color) in &mut labels {
        let (value, value_color) = match marker {
            ViewerLabel::Source => (
                format!(
                    "Files: {}",
                    session
                        .sources
                        .iter()
                        .map(|source| format!(
                            "{}: {}",
                            source_slot_label(source.slot, has_comparison),
                            source.display_path
                        ))
                        .collect::<Vec<_>>()
                        .join(" | ")
                ),
                ui::MUTED_TEXT,
            ),
            ViewerLabel::Version => {
                let versions = session
                    .sources
                    .iter()
                    .map(|source| {
                        format!(
                            "{}: {}",
                            source_slot_label(source.slot, has_comparison),
                            source.spine_version.as_deref().unwrap_or("-")
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
                            session.model.animations().len()
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
                    session.model.transport().frame_index(),
                    session.model.transport().rate().fps()
                ),
                ui::TEXT,
            ),
            ViewerLabel::RuntimeState => {
                let states = session
                    .sources
                    .iter()
                    .map(|source| {
                        format!(
                            "{}: {}",
                            source_slot_label(source.slot, has_comparison),
                            source.runtime_state
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" | ");
                let color = session
                    .sources
                    .iter()
                    .map(|source| runtime_state_color(&source.runtime_state))
                    .find(|color| *color == ui::ERROR)
                    .or_else(|| {
                        session
                            .sources
                            .iter()
                            .map(|source| runtime_state_color(&source.runtime_state))
                            .find(|color| *color == ui::WARNING)
                    })
                    .unwrap_or(ui::SUCCESS);
                (format!("Runtime state: {states}"), color)
            }
            ViewerLabel::LoadStatus => {
                let statuses = session
                    .sources
                    .iter()
                    .map(|source| match &source.load_state {
                        ViewerLoadState::Loading => format!(
                            "{}: loading",
                            source_slot_label(source.slot, has_comparison)
                        ),
                        ViewerLoadState::Ready => format!(
                            "{}: linked ({}, {} page(s))",
                            source_slot_label(source.slot, has_comparison),
                            source.atlas_display_path,
                            source.atlas_page_count
                        ),
                        ViewerLoadState::Failed(error) => format!(
                            "{}: failed ({error})",
                            source_slot_label(source.slot, has_comparison)
                        ),
                    })
                    .collect::<Vec<_>>()
                    .join(" | ");
                let color = if session
                    .sources
                    .iter()
                    .any(|source| matches!(source.load_state, ViewerLoadState::Failed(_)))
                {
                    ui::ERROR
                } else if session
                    .sources
                    .iter()
                    .all(|source| source.load_state == ViewerLoadState::Ready)
                {
                    ui::SUCCESS
                } else {
                    ui::MUTED_TEXT
                };
                (format!("Load status: {statuses}"), color)
            }
            ViewerLabel::Compatibility => {
                let warnings = session
                    .sources
                    .iter()
                    .filter_map(|source| {
                        source.compatibility_warning.as_deref().map(|warning| {
                            format!(
                                "{}: {warning}",
                                source_slot_label(source.slot, has_comparison)
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

    let layout = windows
        .single()
        .ok()
        .map(|window| review_layout(window, has_comparison));
    let scale_factor = windows
        .single()
        .ok()
        .map_or(1.0, |window| window.scale_factor());
    for (marker, mut text, mut color, mut node, mut accessibility) in &mut source_labels {
        let Some(source) = session.source(marker.0) else {
            continue;
        };
        let title = source_slot_label(marker.0, has_comparison);
        let selected_name = session.model.transport().selected_animation();
        let (value, value_color) = match &source.load_state {
            ViewerLoadState::Loading => (format!("{title} — loading"), ui::MUTED_TEXT),
            ViewerLoadState::Failed(error) => (format!("{title} — failed: {error}"), ui::ERROR),
            ViewerLoadState::Ready if !source.selected_present => (
                format!(
                    "{title} — “{}” not present • setup pose",
                    selected_name.unwrap_or("-")
                ),
                ui::WARNING,
            ),
            ViewerLoadState::Ready => {
                let projected = session
                    .model
                    .projected_position(marker.0)
                    .ok()
                    .flatten()
                    .unwrap_or(Duration::ZERO);
                let duration = selected_name
                    .and_then(|name| session.model.duration(marker.0, name))
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
            &source.load_state,
            selected_name,
            source.selected_present,
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

const fn source_slot_label(slot: SourceSlot, has_comparison: bool) -> &'static str {
    match (slot, has_comparison) {
        (SourceSlot::Primary, true) => "Current",
        (SourceSlot::Comparison, true) => "Comparison",
        (SourceSlot::Primary, false) => "Preview",
        (SourceSlot::Comparison, false) => "Comparison",
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

fn fitted_transform(
    asset: &SpinalAsset,
    session: &AppSession,
    slot: SourceSlot,
    window: &Window,
) -> Transform {
    let selected_name = session
        .selected_name()
        .filter(|name| session.model.duration(slot, name).is_some());
    let projected = session
        .model
        .projected_position(slot)
        .ok()
        .flatten()
        .unwrap_or(Duration::ZERO);
    let bounds = sampled_bounds(asset, selected_name, projected);
    let layout = review_layout(window, session.has_comparison());
    let viewport = layout.viewport(slot == SourceSlot::Comparison);
    let scale_factor = window.scale_factor().max(f32::EPSILON);
    let preview_size = viewport.physical_size.as_vec2() / scale_factor;
    fit_transform(bounds, preview_size)
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
    let preview_size = window_size.max(Vec2::ONE);
    let preview_center = Vec2::ZERO;
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
            .spawn((Camera2d, RenderLayers::layer(PRIMARY_RENDER_LAYER)))
            .id();
        let comparison_camera = app
            .world_mut()
            .spawn((Camera2d, RenderLayers::layer(COMPARISON_RENDER_LAYER)))
            .id();
        let ui_camera = app
            .world_mut()
            .spawn((Camera2d, RenderLayers::layer(UI_RENDER_LAYER)))
            .id();
        let primary_instance = app
            .world_mut()
            .spawn((
                SpinalInstance::new(Handle::default()),
                RenderLayers::layer(PRIMARY_RENDER_LAYER),
            ))
            .id();
        let comparison_instance = app
            .world_mut()
            .spawn((
                SpinalInstance::new(Handle::default()),
                RenderLayers::layer(COMPARISON_RENDER_LAYER),
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
            .insert(RenderLayers::layer(COMPARISON_RENDER_LAYER));
        app.world_mut()
            .entity_mut(comparison_instance)
            .insert(RenderLayers::layer(PRIMARY_RENDER_LAYER));
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
    fn shared_three_second_wrap_rebases_two_second_comparison_to_remainder() {
        let mut model = ViewerSession::new(PreviewRate::default());
        model.set_source(
            SourceSlot::Primary,
            SourceReadiness::Ready,
            [(Box::<str>::from("walk"), Duration::from_secs(3))],
        );
        model.set_source(
            SourceSlot::Comparison,
            SourceReadiness::Ready,
            [(Box::<str>::from("walk"), Duration::from_secs(2))],
        );
        model
            .handle_playback(PlaybackCommand::SeekAbsolute(Duration::from_millis(2_900)))
            .expect("seek shared clock");
        model
            .handle_playback(PlaybackCommand::SetPaused(false))
            .expect("resume shared clock");

        let advance = model
            .handle_playback(PlaybackCommand::Advance(Duration::from_millis(200)))
            .expect("advance shared clock")
            .expect("selected animation produces an advance");

        assert_eq!(advance.boundary, AdvanceBoundary::Wrapped);
        assert_eq!(advance.update.position, Duration::from_millis(100));
        assert_eq!(
            wrap_rebase_position(&model, SourceSlot::Primary, advance.boundary)
                .expect("check primary alignment"),
            None
        );
        assert_eq!(
            wrap_rebase_position(&model, SourceSlot::Comparison, advance.boundary)
                .expect("project comparison rebase"),
            Some(Duration::from_millis(100))
        );
    }

    #[test]
    fn exact_divisor_sources_need_no_seek_when_shared_four_second_extent_wraps() {
        let mut model = ViewerSession::new(PreviewRate::default());
        model.set_source(
            SourceSlot::Primary,
            SourceReadiness::Ready,
            [(Box::<str>::from("walk"), Duration::from_secs(4))],
        );
        model.set_source(
            SourceSlot::Comparison,
            SourceReadiness::Ready,
            [(Box::<str>::from("walk"), Duration::from_secs(2))],
        );
        model
            .handle_playback(PlaybackCommand::SeekAbsolute(Duration::from_millis(3_900)))
            .expect("seek shared clock");
        model
            .handle_playback(PlaybackCommand::SetPaused(false))
            .expect("resume shared clock");
        let advance = model
            .handle_playback(PlaybackCommand::Advance(Duration::from_millis(200)))
            .expect("advance shared clock")
            .expect("selected animation produces an advance");

        assert_eq!(advance.boundary, AdvanceBoundary::Wrapped);
        for slot in [SourceSlot::Primary, SourceSlot::Comparison] {
            assert_eq!(
                wrap_rebase_position(&model, slot, advance.boundary)
                    .expect("check exact-divisor alignment"),
                None
            );
        }
    }

    #[test]
    fn ready_transition_finalization_is_edge_triggered_even_for_empty_catalogs() {
        assert!(should_finalize_ready_transition(true, true));
        assert!(!should_finalize_ready_transition(false, true));
        assert!(!should_finalize_ready_transition(true, false));
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
    fn fit_is_uniform_and_centered_inside_its_camera_viewport() {
        let transform = fit_transform(
            Some(GeometryBounds {
                min: Vec2::new(-50.0, -100.0),
                max: Vec2::new(50.0, 100.0),
            }),
            DEFAULT_WINDOW_SIZE,
        );

        assert_eq!(transform.scale.x, transform.scale.y);
        assert_eq!(transform.scale.y, transform.scale.z);
        assert_eq!(transform.translation.x, 0.0);
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

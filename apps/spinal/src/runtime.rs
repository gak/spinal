//! Host-neutral Bevy runtime for immutable Spinal review bundles.

use std::{collections::VecDeque, time::Duration};

use bevy::{
    asset::{AssetApp, AssetPath, AssetServer, Assets, LoadState, io::AssetSourceBuilder},
    camera::visibility::RenderLayers,
    ecs::{message::MessageReader, schedule::SystemSet},
    prelude::*,
};
use bevy_spinal::{
    SpinalAnimator, SpinalAsset, SpinalAssetLoaderSettings, SpinalInstance, SpinalInstanceState,
    SpinalIssue, SpinalPlugin, SpinalRuntimeConfig, SpinalSet, SpinalSkinLayers,
    spinal::{AlphaEncoding, PlaybackMode, SkeletonAsset, Transition},
};

use crate::{
    bundle::SourceBundle,
    clock::AdvanceBoundary,
    command::{PlaybackCommand, SkinSelection, ViewerCommand},
    preview::{PreviewEffect, PreviewRate, SelectionMode, SelectionTransition},
    session::{SourceReadiness, SourceSlot, ViewerSession},
};

const MAX_ISSUE_HISTORY: usize = 8;
const PRIMARY_ASSET_SOURCE: &str = "spinal-primary";
const COMPARISON_ASSET_SOURCE: &str = "spinal-comparison";
const PRIMARY_RENDER_LAYER: usize = 1;
const COMPARISON_RENDER_LAYER: usize = 2;

/// One immutable, already-preflighted source prepared by a host adapter.
#[derive(Clone, Debug)]
pub(crate) struct LaunchSource {
    pub(crate) bundle: SourceBundle,
    pub(crate) display_path: String,
    pub(crate) atlas_display_path: String,
}

impl LaunchSource {
    pub(crate) fn new(
        bundle: SourceBundle,
        display_path: impl Into<String>,
        atlas_display_path: impl Into<String>,
    ) -> Self {
        Self {
            bundle,
            display_path: display_path.into(),
            atlas_display_path: atlas_display_path.into(),
        }
    }
}

/// Complete input for one preview or synchronized two-source comparison.
#[derive(Clone, Debug)]
pub(crate) struct LaunchConfig {
    pub(crate) primary: LaunchSource,
    pub(crate) comparison: Option<LaunchSource>,
    pub(crate) preview_rate: PreviewRate,
}

impl LaunchConfig {
    /// Creates a launch from one required and one optional immutable bundle.
    #[cfg_attr(
        not(feature = "web"),
        allow(
            dead_code,
            reason = "browser manifests supply immutable bundles directly"
        )
    )]
    pub(crate) fn from_bundles(
        primary: SourceBundle,
        comparison: Option<SourceBundle>,
        preview_rate: PreviewRate,
    ) -> Self {
        fn launch_source(bundle: SourceBundle) -> LaunchSource {
            let display_path = bundle.json_asset_path().display().to_string();
            let atlas_display_path = bundle.atlas_reference().to_owned();
            LaunchSource::new(bundle, display_path, atlas_display_path)
        }

        Self {
            primary: launch_source(primary),
            comparison: comparison.map(launch_source),
            preview_rate,
        }
    }

    /// Creates the smallest test launch: one immutable bundle.
    #[cfg(test)]
    pub(crate) fn single(bundle: SourceBundle, preview_rate: PreviewRate) -> Self {
        Self::from_bundles(bundle, None, preview_rate)
    }
}

/// Stable ordering points shared by native and browser hosts.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, SystemSet)]
pub(crate) enum ViewerRuntimeSet {
    Setup,
    Poll,
    Commands,
    Clock,
    Observe,
}

/// Registers immutable memory sources before Bevy constructs its asset server.
///
/// Bevy requires custom asset sources to exist before `AssetPlugin`. Hosts call
/// this first, add their chosen `DefaultPlugins`, and finally add
/// [`ViewerRuntimePlugin`].
pub(crate) fn prepare_runtime(app: &mut App, config: LaunchConfig) {
    assert!(
        !app.world().contains_resource::<AssetServer>(),
        "Spinal viewer bundles must be registered before Bevy's AssetPlugin"
    );

    let primary_reader = config.primary.bundle.memory_reader();
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
    app.insert_resource(ViewerLaunch(config));
}

/// Installs the shared loader, renderer instances, session, and review clock.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ViewerRuntimePlugin;

impl Plugin for ViewerRuntimePlugin {
    fn build(&self, app: &mut App) {
        assert!(
            app.world().contains_resource::<ViewerLaunch>(),
            "prepare_runtime must be called before ViewerRuntimePlugin"
        );
        assert!(
            app.world().contains_resource::<AssetServer>(),
            "ViewerRuntimePlugin requires Bevy's AssetPlugin first"
        );

        app.insert_resource(viewer_runtime_config())
            .init_resource::<CommandInbox>()
            .add_plugins(SpinalPlugin)
            .add_systems(Startup, setup_runtime.in_set(ViewerRuntimeSet::Setup))
            .add_systems(
                Update,
                poll_asset
                    .after(SpinalSet::Prepare)
                    .before(SpinalSet::Animate)
                    .in_set(ViewerRuntimeSet::Poll),
            )
            .add_systems(
                Update,
                apply_commands
                    .after(ViewerRuntimeSet::Poll)
                    .before(SpinalSet::Animate)
                    .in_set(ViewerRuntimeSet::Commands),
            )
            .add_systems(
                Update,
                advance_review_clock
                    .after(ViewerRuntimeSet::Commands)
                    .before(SpinalSet::Animate)
                    .in_set(ViewerRuntimeSet::Clock),
            )
            .add_systems(
                Update,
                (release_deferred_playback, observe_runtime, observe_issues)
                    .chain()
                    .after(SpinalSet::Animate)
                    .in_set(ViewerRuntimeSet::Observe),
            );
    }
}

fn viewer_runtime_config() -> SpinalRuntimeConfig {
    let mut config = SpinalRuntimeConfig::default();
    // Review hosts surface degradation in their own status UI. World-space
    // crosses would obscure the artwork and differ between hosts.
    config.set_diagnostic_markers(false);
    config
}

#[derive(Resource)]
struct ViewerLaunch(LaunchConfig);

/// Source-level compound asset loading, independent of runtime usability.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ViewerLoadState {
    Loading,
    Ready,
    Failed(Box<str>),
}

pub(crate) struct RuntimeSource {
    slot: SourceSlot,
    entity: Entity,
    asset: Handle<SpinalAsset>,
    load_state: ViewerLoadState,
    runtime_state: SpinalInstanceState,
    display_path: Box<str>,
    atlas_display_path: Box<str>,
    atlas_page_count: usize,
    spine_version: Option<Box<str>>,
    compatibility_warning: Option<Box<str>>,
    selected_present: bool,
    selected_skin_present: bool,
    latest_issue: Option<Box<str>>,
}

impl RuntimeSource {
    pub(crate) const fn slot(&self) -> SourceSlot {
        self.slot
    }

    pub(crate) const fn entity(&self) -> Entity {
        self.entity
    }

    pub(crate) const fn asset(&self) -> &Handle<SpinalAsset> {
        &self.asset
    }

    pub(crate) const fn load_state(&self) -> &ViewerLoadState {
        &self.load_state
    }

    pub(crate) const fn runtime_state(&self) -> &SpinalInstanceState {
        &self.runtime_state
    }

    pub(crate) fn display_path(&self) -> &str {
        &self.display_path
    }

    pub(crate) fn atlas_display_path(&self) -> &str {
        &self.atlas_display_path
    }

    pub(crate) const fn atlas_page_count(&self) -> usize {
        self.atlas_page_count
    }

    pub(crate) fn spine_version(&self) -> Option<&str> {
        self.spine_version.as_deref()
    }

    pub(crate) fn compatibility_warning(&self) -> Option<&str> {
        self.compatibility_warning.as_deref()
    }

    pub(crate) const fn selected_present(&self) -> bool {
        self.selected_present
    }

    #[allow(
        dead_code,
        reason = "source-level skin status is exposed by the native and browser UI slices"
    )]
    pub(crate) const fn selected_skin_present(&self) -> bool {
        self.selected_skin_present
    }
}

/// Shared state owned by exactly one Bevy app, regardless of its host.
#[derive(Resource)]
pub(crate) struct ViewerRuntime {
    sources: Vec<RuntimeSource>,
    model: ViewerSession,
    latest_issue: Option<Box<str>>,
    issue_history: VecDeque<Box<str>>,
    suppress_clock_advance: bool,
    resume_after_animate: bool,
    catalog_revision: u64,
    refit_revision: u64,
}

impl ViewerRuntime {
    pub(crate) fn sources(&self) -> &[RuntimeSource] {
        &self.sources
    }

    pub(crate) const fn model(&self) -> &ViewerSession {
        &self.model
    }

    pub(crate) fn controls_ready(&self) -> bool {
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

    pub(crate) fn selected_entry(&self) -> Option<(usize, &str, Duration)> {
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

    pub(crate) fn selected_name(&self) -> Option<&str> {
        self.selected_entry().map(|(_index, name, _duration)| name)
    }

    pub(crate) fn source(&self, slot: SourceSlot) -> Option<&RuntimeSource> {
        self.sources.iter().find(|source| source.slot == slot)
    }

    pub(crate) fn has_comparison(&self) -> bool {
        self.source(SourceSlot::Comparison).is_some()
    }

    pub(crate) fn latest_issue(&self) -> Option<&str> {
        self.latest_issue.as_deref()
    }

    pub(crate) fn issue_history(&self) -> &VecDeque<Box<str>> {
        &self.issue_history
    }

    pub(crate) const fn catalog_revision(&self) -> u64 {
        self.catalog_revision
    }

    pub(crate) const fn refit_revision(&self) -> u64 {
        self.refit_revision
    }

    /// Produces the stable, allocation-bounded observation seam for web hosts.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "used by the browser host in the next slice")
    )]
    pub(crate) fn snapshot(&self) -> RuntimeSnapshot {
        RuntimeSnapshot {
            sources: self
                .sources
                .iter()
                .map(|source| RuntimeSourceSnapshot {
                    slot: source.slot,
                    load_state: source.load_state.clone(),
                    runtime_state: source.runtime_state.clone(),
                    selected_present: source.selected_present,
                    selected_skin_present: source.selected_skin_present,
                    latest_issue: source.latest_issue.clone(),
                })
                .collect(),
            selected_animation: self.model.transport().selected_animation().map(Into::into),
            selected_skin: self.model.selected_skin().clone(),
            paused: self.model.transport().is_paused(),
            controls_ready: self.controls_ready(),
            latest_issue: self.latest_issue.clone(),
        }
    }
}

/// Read-only source state suitable for native labels or a browser bridge.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "used by the browser host in the next slice")
)]
pub(crate) struct RuntimeSourceSnapshot {
    slot: SourceSlot,
    load_state: ViewerLoadState,
    runtime_state: SpinalInstanceState,
    selected_present: bool,
    selected_skin_present: bool,
    latest_issue: Option<Box<str>>,
}

#[cfg_attr(
    not(test),
    allow(dead_code, reason = "used by the browser host in the next slice")
)]
impl RuntimeSourceSnapshot {
    pub(crate) const fn slot(&self) -> SourceSlot {
        self.slot
    }

    pub(crate) const fn load_state(&self) -> &ViewerLoadState {
        &self.load_state
    }

    pub(crate) const fn runtime_state(&self) -> &SpinalInstanceState {
        &self.runtime_state
    }

    pub(crate) const fn runtime_usable(&self) -> bool {
        self.runtime_state.is_usable()
    }

    pub(crate) const fn selected_present(&self) -> bool {
        self.selected_present
    }

    pub(crate) const fn selected_skin_present(&self) -> bool {
        self.selected_skin_present
    }

    #[cfg_attr(
        not(feature = "web"),
        allow(
            dead_code,
            reason = "browser status attributes diagnostics to one source"
        )
    )]
    pub(crate) fn latest_issue(&self) -> Option<&str> {
        self.latest_issue.as_deref()
    }
}

/// One immutable observation of loading, selection, and runtime usability.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "used by the browser host in the next slice")
)]
pub(crate) struct RuntimeSnapshot {
    sources: Vec<RuntimeSourceSnapshot>,
    selected_animation: Option<Box<str>>,
    selected_skin: SkinSelection,
    paused: bool,
    controls_ready: bool,
    latest_issue: Option<Box<str>>,
}

#[cfg_attr(
    not(test),
    allow(dead_code, reason = "used by the browser host in the next slice")
)]
impl RuntimeSnapshot {
    #[cfg_attr(
        not(feature = "web"),
        allow(
            dead_code,
            reason = "all-source status aggregation belongs to the browser review host"
        )
    )]
    pub(crate) fn sources(&self) -> &[RuntimeSourceSnapshot] {
        &self.sources
    }

    pub(crate) fn source(&self, slot: SourceSlot) -> Option<&RuntimeSourceSnapshot> {
        self.sources.iter().find(|source| source.slot == slot)
    }

    pub(crate) fn selected_animation(&self) -> Option<&str> {
        self.selected_animation.as_deref()
    }

    pub(crate) const fn selected_skin(&self) -> &SkinSelection {
        &self.selected_skin
    }

    pub(crate) const fn is_paused(&self) -> bool {
        self.paused
    }

    pub(crate) const fn controls_ready(&self) -> bool {
        self.controls_ready
    }

    pub(crate) fn latest_issue(&self) -> Option<&str> {
        self.latest_issue.as_deref()
    }

    pub(crate) fn runtime_usable(&self) -> bool {
        !self.sources.is_empty()
            && self
                .sources
                .iter()
                .all(RuntimeSourceSnapshot::runtime_usable)
    }
}

/// Commands are semantic inputs shared by keyboard, native UI, and future DOM.
#[derive(Default, Resource)]
pub(crate) struct CommandInbox(Vec<ViewerCommand>);

impl CommandInbox {
    pub(crate) fn push(&mut self, command: ViewerCommand) {
        self.0.push(command);
    }
}

fn setup_runtime(
    mut commands: Commands<'_, '_>,
    launch: Res<'_, ViewerLaunch>,
    asset_server: Res<'_, AssetServer>,
) {
    let has_comparison = launch.0.comparison.is_some();
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
            .bundle
            .skeleton()
            .animations()
            .map(|animation| (animation.name().into(), animation.duration()))
            .collect::<Vec<_>>();
        let skins = selectable_skin_names(source.bundle.skeleton());
        model.set_source_with_skins(slot, SourceReadiness::Loading, catalog, skins);
        sources.push(spawn_runtime_source(
            &mut commands,
            &asset_server,
            slot,
            source,
        ));
    }
    debug_assert!((1..=2).contains(&sources.len()));
    commands.insert_resource(ViewerRuntime {
        sources,
        model,
        latest_issue: None,
        issue_history: VecDeque::new(),
        suppress_clock_advance: false,
        resume_after_animate: false,
        catalog_revision: 0,
        refit_revision: 0,
    });
}

fn spawn_runtime_source(
    commands: &mut Commands<'_, '_>,
    asset_server: &AssetServer,
    slot: SourceSlot,
    launch: &LaunchSource,
) -> RuntimeSource {
    let (asset_source, render_layer) = source_render_spec(slot);
    let asset = load_prepared_asset(asset_server, launch, asset_source);
    let skeleton = launch.bundle.skeleton();
    let premultiplied_pages = skeleton
        .atlas_pages()
        .filter(|page| page.alpha_encoding() == AlphaEncoding::Premultiplied)
        .map(|page| Box::<str>::from(page.name()))
        .collect::<Vec<_>>();
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
        asset,
        load_state: ViewerLoadState::Loading,
        runtime_state: SpinalInstanceState::Loading,
        display_path: launch.display_path.clone().into(),
        atlas_display_path: launch.atlas_display_path.clone().into(),
        atlas_page_count: skeleton.atlas_pages().len(),
        spine_version: Some(skeleton.spine_version().into()),
        compatibility_warning: premultiplied_alpha_issue(&premultiplied_pages).map(Into::into),
        selected_present: true,
        selected_skin_present: true,
        latest_issue: None,
    }
}

fn selectable_skin_names(skeleton: &SkeletonAsset) -> Vec<Box<str>> {
    let default = skeleton.default_skin().map(|skin| skin.id());
    skeleton
        .skins()
        .filter(|skin| Some(skin.id()) != default)
        .map(|skin| skin.name().into())
        .collect()
}

const fn source_render_spec(slot: SourceSlot) -> (&'static str, usize) {
    match slot {
        SourceSlot::Primary => (PRIMARY_ASSET_SOURCE, PRIMARY_RENDER_LAYER),
        SourceSlot::Comparison => (COMPARISON_ASSET_SOURCE, COMPARISON_RENDER_LAYER),
    }
}

pub(crate) const fn source_render_layer(slot: SourceSlot) -> usize {
    source_render_spec(slot).1
}

pub(crate) const fn source_camera_order(slot: SourceSlot) -> isize {
    match slot {
        SourceSlot::Primary => 0,
        SourceSlot::Comparison => 1,
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
    asset_server: Res<'_, AssetServer>,
    assets: Res<'_, Assets<SpinalAsset>>,
    mut runtime: ResMut<'_, ViewerRuntime>,
    mut animators: Query<'_, '_, &mut SpinalAnimator>,
    mut skin_layers: Query<'_, '_, &mut SpinalSkinLayers>,
) {
    let mut initial = None;
    let mut transitioned_to_ready = false;
    let mut catalog_transitioned = false;
    for index in 0..runtime.sources.len() {
        if runtime.sources[index].load_state != ViewerLoadState::Loading {
            continue;
        }
        let asset_handle = runtime.sources[index].asset.clone();
        let slot = runtime.sources[index].slot;
        match asset_server.load_state(&asset_handle) {
            LoadState::NotLoaded | LoadState::Loading => {}
            LoadState::Failed(error) => {
                runtime.sources[index].load_state =
                    ViewerLoadState::Failed(error.to_string().into());
                runtime.model.set_readiness(slot, SourceReadiness::Failed);
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
                let skins = selectable_skin_names(asset.skeleton());
                runtime.sources[index].spine_version =
                    Some(asset.skeleton().spine_version().into());
                runtime.sources[index].load_state = ViewerLoadState::Ready;
                transitioned_to_ready = true;
                catalog_transitioned = true;
                if let Some(effect) = runtime.model.set_source_with_skins(
                    slot,
                    SourceReadiness::Ready,
                    catalog,
                    skins,
                ) {
                    initial = Some(effect);
                }
            }
        }
    }

    if catalog_transitioned {
        // A reload can remove the synchronized named skin and reset the
        // session to Default. Project the resulting choice before Animate so
        // the adapter never observes a now-missing layer.
        apply_skin_selection_to_all(&mut runtime, &mut skin_layers);
    }

    if should_finalize_ready_transition(
        transitioned_to_ready,
        runtime.model.all_present_sources_ready(),
    ) {
        runtime.catalog_revision = runtime.catalog_revision.wrapping_add(1);
        if let Some(effect) = initial {
            apply_preview_effect_to_all(effect, &mut runtime, &mut animators, true);
        }
    }
}

const fn should_finalize_ready_transition(
    transitioned_to_ready: bool,
    all_present_sources_ready: bool,
) -> bool {
    transitioned_to_ready && all_present_sources_ready
}

fn apply_commands(
    mut inbox: ResMut<'_, CommandInbox>,
    mut runtime: ResMut<'_, ViewerRuntime>,
    mut animators: Query<'_, '_, &mut SpinalAnimator>,
    mut skin_layers: Query<'_, '_, &mut SpinalSkinLayers>,
) {
    let queued = std::mem::take(&mut inbox.0);
    if queued.is_empty() {
        return;
    }
    for command in queued {
        if !command_is_available(&runtime, &command) {
            continue;
        }
        let previous_skin = runtime.model.selected_skin().clone();
        let effect = match runtime.model.handle(command) {
            Ok(effect) => effect,
            Err(error) => {
                record_local_issue(&mut runtime, format!("preview command failed: {error}"));
                None
            }
        };
        if runtime.model.selected_skin() != &previous_skin {
            apply_skin_selection_to_all(&mut runtime, &mut skin_layers);
        }
        let Some(effect) = effect else {
            continue;
        };
        if effect == PreviewEffect::Refit {
            runtime.refit_revision = runtime.refit_revision.wrapping_add(1);
        } else {
            runtime.suppress_clock_advance = true;
            apply_preview_effect_to_all(effect, &mut runtime, &mut animators, true);
        }
    }
}

fn command_is_available(runtime: &ViewerRuntime, command: &ViewerCommand) -> bool {
    if !runtime.controls_ready() {
        return false;
    }
    match command {
        ViewerCommand::Refit => true,
        ViewerCommand::SelectAnimation(name) => runtime
            .model
            .animations()
            .iter()
            .any(|candidate| candidate == name),
        ViewerCommand::SelectSkin(SkinSelection::Default) => true,
        ViewerCommand::SelectSkin(SkinSelection::Named(name)) => runtime
            .model
            .skins()
            .iter()
            .any(|candidate| candidate == name),
        ViewerCommand::SetLooping(_)
        | ViewerCommand::SetPlaybackSpeed(_)
        | ViewerCommand::SeekAbsolute(_)
        | ViewerCommand::TogglePause
        | ViewerCommand::Restart
        | ViewerCommand::Step(_) => !runtime.model.animations().is_empty(),
    }
}

fn apply_skin_selection_to_all(
    runtime: &mut ViewerRuntime,
    skin_layers: &mut Query<'_, '_, &mut SpinalSkinLayers>,
) {
    let selection = runtime.model.selected_skin().clone();
    for index in 0..runtime.sources.len() {
        let slot = runtime.sources[index].slot;
        let entity = runtime.sources[index].entity;
        let present = runtime.model.skin_present(slot, &selection);
        runtime.sources[index].selected_skin_present = present;
        let Ok(mut layers) = skin_layers.get_mut(entity) else {
            continue;
        };
        match (&selection, present) {
            (SkinSelection::Named(name), true) => layers.set([name.as_ref()]),
            (SkinSelection::Default | SkinSelection::Named(_), false)
            | (SkinSelection::Default, true) => layers.set(std::iter::empty::<&str>()),
        }
    }
}

fn apply_preview_effect_to_all(
    effect: PreviewEffect,
    runtime: &mut ViewerRuntime,
    animators: &mut Query<'_, '_, &mut SpinalAnimator>,
    hold_resume_for_frame: bool,
) {
    let desired_paused = match &effect {
        PreviewEffect::Select(request) => Some(request.paused),
        PreviewEffect::SetPaused { paused, .. } => Some(*paused),
        PreviewEffect::SeekAndPause(_request) => Some(true),
        PreviewEffect::Playback(effect) => Some(effect.update.paused),
        PreviewEffect::Refit => None,
    };
    runtime.resume_after_animate = desired_paused == Some(false) && hold_resume_for_frame;

    for index in 0..runtime.sources.len() {
        let slot = runtime.sources[index].slot;
        let entity = runtime.sources[index].entity;
        let selected = runtime.model.transport().selected_animation();
        let present = selected.is_some_and(|name| runtime.model.duration(slot, name).is_some());
        runtime.sources[index].selected_present = present;
        let Ok(mut animator) = animators.get_mut(entity) else {
            continue;
        };
        if !present {
            animator.stop(Transition::Immediate);
            continue;
        }
        let projected = runtime
            .model
            .projected_position(slot)
            .ok()
            .flatten()
            .unwrap_or(Duration::ZERO);

        match &effect {
            PreviewEffect::Select(request) => {
                let mode = match request.mode {
                    SelectionMode::Loop => PlaybackMode::Loop,
                    SelectionMode::Once => PlaybackMode::Once,
                };
                let transition = match request.transition {
                    SelectionTransition::Immediate => Transition::Immediate,
                };
                animator.play(request.animation_name.clone(), mode, transition);
                animator.seek_to(projected);
                animator
                    .set_speed(request.playback_speed.multiplier())
                    .expect("the transport only retains valid playback speeds");
                animator.set_paused(request.paused || runtime.resume_after_animate);
            }
            PreviewEffect::SetPaused { paused, .. } => {
                animator.seek_to(projected);
                animator.set_paused(*paused || runtime.resume_after_animate);
            }
            PreviewEffect::SeekAndPause(request) => {
                debug_assert_eq!(
                    runtime.model.transport().selected_animation(),
                    Some(request.animation_name.as_ref())
                );
                animator.set_paused(true);
                animator.seek_to(projected);
            }
            PreviewEffect::Playback(effect) => {
                debug_assert_eq!(
                    runtime.model.transport().selected_animation(),
                    Some(effect.update.animation_name.as_ref())
                );
                let mode = if effect.update.looping {
                    PlaybackMode::Loop
                } else {
                    PlaybackMode::Once
                };
                if animator.animation() != Some(effect.update.animation_name.as_ref())
                    || animator.mode() != Some(mode)
                {
                    animator.play(
                        effect.update.animation_name.clone(),
                        mode,
                        Transition::Immediate,
                    );
                }
                animator.seek_to(projected);
                animator
                    .set_speed(effect.update.playback_speed.multiplier())
                    .expect("the transport only retains valid playback speeds");
                animator.set_paused(effect.update.paused || runtime.resume_after_animate);
            }
            PreviewEffect::Refit => {}
        }
    }
}

fn advance_review_clock(
    time: Res<'_, Time>,
    mut runtime: ResMut<'_, ViewerRuntime>,
    mut animators: Query<'_, '_, &mut SpinalAnimator>,
) {
    if std::mem::take(&mut runtime.suppress_clock_advance) {
        return;
    }
    let effect = match runtime
        .model
        .handle_playback(PlaybackCommand::Advance(time.delta()))
    {
        Ok(effect) => effect,
        Err(error) => {
            record_local_issue(&mut runtime, format!("preview clock failed: {error}"));
            return;
        }
    };
    let Some(effect) = effect else {
        return;
    };
    if effect.boundary == AdvanceBoundary::Wrapped {
        let present_sources = runtime
            .sources
            .iter()
            .filter(|source| source.selected_present)
            .map(|source| (source.entity, source.slot))
            .collect::<Vec<_>>();
        for (entity, slot) in present_sources {
            match wrap_rebase_position(&runtime.model, slot, effect.boundary) {
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
                        &mut runtime,
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
        for source in &runtime.sources {
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
    mut runtime: ResMut<'_, ViewerRuntime>,
    mut animators: Query<'_, '_, &mut SpinalAnimator>,
) {
    if !std::mem::take(&mut runtime.resume_after_animate) {
        return;
    }
    for source in &runtime.sources {
        if source.selected_present
            && let Ok(mut animator) = animators.get_mut(source.entity)
        {
            animator.set_paused(false);
        }
    }
}

fn observe_runtime(
    mut runtime: ResMut<'_, ViewerRuntime>,
    states: Query<'_, '_, &SpinalInstanceState>,
) {
    for source in &mut runtime.sources {
        if let Ok(state) = states.get(source.entity) {
            source.runtime_state = state.clone();
        }
    }
}

fn observe_issues(
    mut issues: MessageReader<'_, '_, SpinalIssue>,
    mut runtime: ResMut<'_, ViewerRuntime>,
) {
    for issue in issues.read() {
        let Some(source_index) = runtime
            .sources
            .iter()
            .position(|source| issue.entity() == source.entity)
        else {
            continue;
        };
        let source_name =
            source_slot_label(runtime.sources[source_index].slot, runtime.has_comparison());
        let track = issue
            .track()
            .map(|track| format!(" track `{track}`"))
            .unwrap_or_default();
        let detail = format!(
            "{source_name} {:?}{track}: {}",
            issue.kind(),
            issue.message()
        );
        runtime.sources[source_index].latest_issue = Some(detail.clone().into());
        record_local_issue(&mut runtime, detail);
    }
}

fn record_local_issue(runtime: &mut ViewerRuntime, detail: String) {
    let detail: Box<str> = detail.into();
    runtime.latest_issue = Some(detail.clone());
    runtime.issue_history.push_front(detail);
    runtime.issue_history.truncate(MAX_ISSUE_HISTORY);
}

pub(crate) const fn source_slot_label(slot: SourceSlot, has_comparison: bool) -> &'static str {
    match (slot, has_comparison) {
        (SourceSlot::Primary, true) => "Current",
        (SourceSlot::Comparison, true) => "Proposed",
        (SourceSlot::Primary, false) => "Preview",
        (SourceSlot::Comparison, false) => "Proposed",
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        path::{Path, PathBuf},
        thread,
    };

    use bevy::{asset::AssetPlugin, ecs::message::Messages, time::TimeUpdateStrategy};
    use bevy_spinal::{
        SpinalAnimationEvent, SpinalAtlasPage, SpinalIssueKind, SpinalPlaybackState,
        SpinalSkinLayers,
    };

    use super::*;

    const REVIEW_ATLAS: &[u8] = b"shared.png\n\tsize: 1, 1\n\tformat: RGBA8888\n\tfilter: Linear, Linear\n\trepeat: none\n\tpma: false\n";
    const PRIMARY_REVIEW_JSON: &[u8] = br#"{
      "skeleton":{"spine":"4.3.23"},
      "bones":[{"name":"root"}],
      "skins":[{"name":"default"},{"name":"shared"},{"name":"primary-only"}],
      "events":{"tick":{}},
      "animations":{
        "shared":{
          "bones":{"root":{"translate":[{"x":0,"y":0},{"time":1,"x":1,"y":0}]}},
          "events":[{"time":0.05,"name":"tick"}]
        },
        "primary-only":{}
      }
    }"#;
    const COMPARISON_REVIEW_JSON: &[u8] = br#"{
      "skeleton":{"spine":"4.3.23"},
      "bones":[{"name":"root"}],
      "skins":[{"name":"default"},{"name":"shared"},{"name":"comparison-only"}],
      "events":{"tick":{}},
      "animations":{
        "shared":{
          "bones":{"root":{"translate":[{"x":0,"y":0},{"time":1,"x":2,"y":0}]}},
          "events":[{"time":0.05,"name":"tick"}]
        },
        "comparison-only":{}
      }
    }"#;
    const PRIMARY_WITHOUT_PRIMARY_ONLY_SKIN_JSON: &[u8] = br#"{
      "skeleton":{"spine":"4.3.23"},
      "bones":[{"name":"root"}],
      "skins":[{"name":"default"},{"name":"shared"}],
      "events":{"tick":{}},
      "animations":{
        "shared":{
          "bones":{"root":{"translate":[{"x":0,"y":0},{"time":1,"x":1,"y":0}]}},
          "events":[{"time":0.05,"name":"tick"}]
        },
        "primary-only":{}
      }
    }"#;
    const SKIN_ONLY_REVIEW_JSON: &[u8] = br#"{
      "skeleton":{"spine":"4.3.23"},
      "bones":[{"name":"root"}],
      "skins":[{"name":"default"},{"name":"hat"}]
    }"#;
    const RED_PIXEL_PNG: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240,
        31, 0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];
    const BLUE_PIXEL_PNG: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 96, 96, 248, 255, 31,
        0, 3, 2, 1, 255, 230, 119, 11, 174, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];

    fn launch_bundle() -> SourceBundle {
        let json = br#"{"skeleton":{"spine":"4.3.23"},"bones":[{"name":"root"}]}"#;
        let atlas = b"fixture.png\n\tsize: 1, 1\n\tformat: RGBA8888\n\tfilter: Linear, Linear\n\trepeat: none\n\tpma: false\n";
        let mut files = BTreeMap::new();
        files.insert(PathBuf::from("fixture.json"), json.to_vec());
        files.insert(PathBuf::from("fixture.atlas"), atlas.to_vec());
        files.insert(PathBuf::from("fixture.png"), BLUE_PIXEL_PNG.to_vec());
        SourceBundle::from_test_files(
            "Runtime fixture",
            Path::new("fixture.json"),
            Path::new("fixture.atlas"),
            files,
        )
    }

    fn review_bundle(json: &[u8], page: &[u8]) -> SourceBundle {
        let files = BTreeMap::from([
            (PathBuf::from("shared.json"), json.to_vec()),
            (PathBuf::from("shared.atlas"), REVIEW_ATLAS.to_vec()),
            (PathBuf::from("shared.png"), page.to_vec()),
        ]);
        SourceBundle::from_test_files(
            "Review fixture",
            Path::new("shared.json"),
            Path::new("shared.atlas"),
            files,
        )
    }

    fn update_until_ready(app: &mut App) {
        for _attempt in 0..5_000 {
            app.update();
            if app
                .world()
                .get_resource::<ViewerRuntime>()
                .is_some_and(ViewerRuntime::controls_ready)
            {
                return;
            }
            thread::sleep(Duration::from_millis(1));
        }
        panic!("the two-source runtime did not become ready before the test timeout");
    }

    fn two_source_review_app() -> App {
        let config = LaunchConfig {
            primary: LaunchSource::new(
                review_bundle(PRIMARY_REVIEW_JSON, RED_PIXEL_PNG),
                "primary",
                "primary atlas",
            ),
            comparison: Some(LaunchSource::new(
                review_bundle(COMPARISON_REVIEW_JSON, BLUE_PIXEL_PNG),
                "comparison",
                "comparison atlas",
            )),
            preview_rate: PreviewRate::default(),
        };
        let mut app = App::new();
        prepare_runtime(&mut app, config);
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin {
                watch_for_changes_override: Some(false),
                use_asset_processor_override: Some(false),
                ..default()
            },
            ViewerRuntimePlugin,
        ));
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            100,
        )));
        update_until_ready(&mut app);
        app
    }

    fn replace_review_asset(app: &mut App, handle: &Handle<SpinalAsset>, json: &[u8]) {
        let image = app
            .world()
            .resource::<Assets<SpinalAsset>>()
            .get(handle)
            .and_then(|asset| asset.page(0))
            .expect("loaded review page")
            .image()
            .clone();
        let skeleton = bevy_spinal::spinal::load_json(json, REVIEW_ATLAS)
            .expect("replacement fixture is supported")
            .into_asset();
        let replacement =
            SpinalAsset::new(skeleton, vec![SpinalAtlasPage::new("shared.png", image)])
                .expect("replacement page matches the linked atlas");
        app.world_mut()
            .resource_mut::<Assets<SpinalAsset>>()
            .insert(handle.id(), replacement)
            .expect("the live asset handle remains valid");
    }

    fn source(
        slot: SourceSlot,
        load_state: ViewerLoadState,
        state: SpinalInstanceState,
    ) -> RuntimeSource {
        RuntimeSource {
            slot,
            entity: Entity::PLACEHOLDER,
            asset: Handle::default(),
            load_state,
            runtime_state: state,
            display_path: "fixture.json".into(),
            atlas_display_path: "fixture.atlas".into(),
            atlas_page_count: 1,
            spine_version: Some("4.3.23".into()),
            compatibility_warning: None,
            selected_present: true,
            selected_skin_present: true,
            latest_issue: None,
        }
    }

    fn runtime_with(
        load_state: ViewerLoadState,
        state: SpinalInstanceState,
        readiness: SourceReadiness,
    ) -> ViewerRuntime {
        let mut model = ViewerSession::new(PreviewRate::default());
        model.set_source(
            SourceSlot::Primary,
            readiness,
            [(Box::<str>::from("walk"), Duration::from_secs(1))],
        );
        ViewerRuntime {
            sources: vec![source(SourceSlot::Primary, load_state, state)],
            model,
            latest_issue: None,
            issue_history: VecDeque::new(),
            suppress_clock_advance: false,
            resume_after_animate: false,
            catalog_revision: 0,
            refit_revision: 0,
        }
    }

    #[test]
    fn single_bundle_config_is_paused_by_the_shared_runtime_contract() {
        let config = LaunchConfig::single(launch_bundle(), PreviewRate::default());
        assert!(config.comparison.is_none());
        assert_eq!(config.primary.display_path, "fixture.json");
        assert_eq!(config.primary.atlas_display_path, "fixture.atlas");
        let runtime = runtime_with(
            ViewerLoadState::Loading,
            SpinalInstanceState::Loading,
            SourceReadiness::Loading,
        );
        assert!(runtime.snapshot().is_paused());
    }

    #[test]
    fn snapshot_exposes_loading_ready_failed_selection_and_usability() {
        let loading = runtime_with(
            ViewerLoadState::Loading,
            SpinalInstanceState::Loading,
            SourceReadiness::Loading,
        )
        .snapshot();
        assert_eq!(loading.sources().len(), 1);
        assert_eq!(
            loading
                .source(SourceSlot::Primary)
                .expect("primary source")
                .load_state(),
            &ViewerLoadState::Loading
        );
        assert_eq!(loading.selected_animation(), None);
        assert_eq!(loading.selected_skin(), &SkinSelection::Default);
        assert!(loading.is_paused());
        assert!(!loading.controls_ready());
        assert!(!loading.runtime_usable());

        let ready = runtime_with(
            ViewerLoadState::Ready,
            SpinalInstanceState::Ready,
            SourceReadiness::Ready,
        )
        .snapshot();
        let primary = ready.source(SourceSlot::Primary).expect("primary source");
        assert_eq!(primary.slot(), SourceSlot::Primary);
        assert_eq!(primary.load_state(), &ViewerLoadState::Ready);
        assert_eq!(primary.runtime_state(), &SpinalInstanceState::Ready);
        assert!(primary.runtime_usable());
        assert!(primary.selected_present());
        assert!(primary.selected_skin_present());
        assert_eq!(ready.selected_animation(), Some("walk"));
        assert_eq!(ready.selected_skin(), &SkinSelection::Default);
        assert!(ready.is_paused(), "loading a bundle must never autoplay");
        assert!(ready.controls_ready());
        assert!(ready.runtime_usable());
        assert_eq!(ready.latest_issue(), None);

        let mut degraded_runtime = runtime_with(
            ViewerLoadState::Ready,
            SpinalInstanceState::Degraded,
            SourceReadiness::Ready,
        );
        degraded_runtime.latest_issue = Some("unsupported blend mode".into());
        degraded_runtime.sources[0].latest_issue = Some("Preview BlendMode: multiply".into());
        let degraded = degraded_runtime.snapshot();
        assert_eq!(degraded.latest_issue(), Some("unsupported blend mode"));
        assert_eq!(
            degraded
                .source(SourceSlot::Primary)
                .expect("primary source")
                .latest_issue(),
            Some("Preview BlendMode: multiply")
        );

        let failed = runtime_with(
            ViewerLoadState::Failed("bad atlas".into()),
            SpinalInstanceState::Failed,
            SourceReadiness::Failed,
        )
        .snapshot();
        assert!(matches!(
            failed
                .source(SourceSlot::Primary)
                .expect("primary source")
                .load_state(),
            ViewerLoadState::Failed(error) if error.as_ref() == "bad atlas"
        ));
        assert!(!failed.controls_ready());
        assert!(!failed.runtime_usable());
    }

    #[test]
    fn comparison_only_skin_uses_default_fallback_without_missing_skin_issue() {
        let mut app = two_source_review_app();
        let (primary_entity, comparison_entity, before) = {
            let runtime = app.world().resource::<ViewerRuntime>();
            assert_eq!(
                runtime
                    .model()
                    .skins()
                    .iter()
                    .map(AsRef::as_ref)
                    .collect::<Vec<_>>(),
                ["shared", "primary-only", "comparison-only"]
            );
            let transport = runtime.model().transport();
            (
                runtime
                    .source(SourceSlot::Primary)
                    .expect("primary source")
                    .entity(),
                runtime
                    .source(SourceSlot::Comparison)
                    .expect("comparison source")
                    .entity(),
                (
                    transport.selected_animation().map(str::to_owned),
                    transport.position(),
                    transport.is_paused(),
                    transport.is_looping(),
                    transport.playback_speed(),
                ),
            )
        };
        let mut issue_cursor = app
            .world()
            .resource::<Messages<SpinalIssue>>()
            .get_cursor_current();

        app.world_mut()
            .resource_mut::<CommandInbox>()
            .push(ViewerCommand::SelectSkin(SkinSelection::Named(
                "comparison-only".into(),
            )));
        app.update();

        assert!(
            app.world()
                .entity(primary_entity)
                .get::<SpinalSkinLayers>()
                .expect("primary skin layers")
                .is_empty(),
            "a missing synchronized skin projects to Default/setup"
        );
        assert_eq!(
            app.world()
                .entity(comparison_entity)
                .get::<SpinalSkinLayers>()
                .expect("comparison skin layers")
                .iter()
                .collect::<Vec<_>>(),
            ["comparison-only"]
        );

        let runtime = app.world().resource::<ViewerRuntime>();
        assert_eq!(
            runtime.model().selected_skin(),
            &SkinSelection::Named("comparison-only".into())
        );
        assert!(
            !runtime
                .source(SourceSlot::Primary)
                .expect("primary source")
                .selected_skin_present()
        );
        assert!(
            runtime
                .source(SourceSlot::Comparison)
                .expect("comparison source")
                .selected_skin_present()
        );
        let snapshot = runtime.snapshot();
        assert_eq!(snapshot.selected_skin(), runtime.model().selected_skin());
        assert!(
            !snapshot
                .source(SourceSlot::Primary)
                .expect("primary snapshot")
                .selected_skin_present()
        );
        assert!(
            snapshot
                .source(SourceSlot::Comparison)
                .expect("comparison snapshot")
                .selected_skin_present()
        );
        let transport = runtime.model().transport();
        assert_eq!(transport.selected_animation(), before.0.as_deref());
        assert_eq!(transport.position(), before.1);
        assert_eq!(transport.is_paused(), before.2);
        assert_eq!(transport.is_looping(), before.3);
        assert_eq!(transport.playback_speed(), before.4);
        assert_eq!(
            issue_cursor
                .read(app.world().resource::<Messages<SpinalIssue>>())
                .filter(|issue| issue.kind() == SpinalIssueKind::MissingSkin)
                .count(),
            0
        );
    }

    #[test]
    fn skin_selection_remains_usable_without_animations() {
        let config = LaunchConfig::single(
            review_bundle(SKIN_ONLY_REVIEW_JSON, RED_PIXEL_PNG),
            PreviewRate::default(),
        );
        let mut app = App::new();
        prepare_runtime(&mut app, config);
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), ViewerRuntimePlugin));
        update_until_ready(&mut app);

        let entity = {
            let runtime = app.world().resource::<ViewerRuntime>();
            assert!(runtime.controls_ready());
            assert!(runtime.model().animations().is_empty());
            assert_eq!(
                runtime
                    .model()
                    .skins()
                    .iter()
                    .map(AsRef::as_ref)
                    .collect::<Vec<_>>(),
                ["hat"]
            );
            runtime
                .source(SourceSlot::Primary)
                .expect("primary source")
                .entity()
        };

        app.world_mut()
            .resource_mut::<CommandInbox>()
            .push(ViewerCommand::SelectSkin(SkinSelection::Named(
                "hat".into(),
            )));
        app.update();

        assert_eq!(
            app.world()
                .entity(entity)
                .get::<SpinalSkinLayers>()
                .expect("skin layers")
                .iter()
                .collect::<Vec<_>>(),
            ["hat"]
        );
        assert_eq!(
            app.world()
                .resource::<ViewerRuntime>()
                .model()
                .selected_skin(),
            &SkinSelection::Named("hat".into())
        );
    }

    #[test]
    fn catalog_reload_removing_selected_skin_resets_layers_and_presence() {
        let mut app = two_source_review_app();
        let (primary_entity, comparison_entity, primary_asset) = {
            let runtime = app.world().resource::<ViewerRuntime>();
            (
                runtime
                    .source(SourceSlot::Primary)
                    .expect("primary source")
                    .entity(),
                runtime
                    .source(SourceSlot::Comparison)
                    .expect("comparison source")
                    .entity(),
                runtime
                    .source(SourceSlot::Primary)
                    .expect("primary source")
                    .asset()
                    .clone(),
            )
        };
        app.world_mut()
            .resource_mut::<CommandInbox>()
            .push(ViewerCommand::SelectSkin(SkinSelection::Named(
                "primary-only".into(),
            )));
        app.update();
        assert_eq!(
            app.world()
                .entity(primary_entity)
                .get::<SpinalSkinLayers>()
                .expect("primary skin layers")
                .iter()
                .collect::<Vec<_>>(),
            ["primary-only"]
        );

        let mut issue_cursor = app
            .world()
            .resource::<Messages<SpinalIssue>>()
            .get_cursor_current();
        replace_review_asset(
            &mut app,
            &primary_asset,
            PRIMARY_WITHOUT_PRIMARY_ONLY_SKIN_JSON,
        );
        {
            let mut runtime = app.world_mut().resource_mut::<ViewerRuntime>();
            runtime
                .sources
                .iter_mut()
                .find(|source| source.slot == SourceSlot::Primary)
                .expect("primary source")
                .load_state = ViewerLoadState::Loading;
            runtime
                .model
                .set_readiness(SourceSlot::Primary, SourceReadiness::Loading);
        }

        app.update();

        let runtime = app.world().resource::<ViewerRuntime>();
        assert_eq!(runtime.model().selected_skin(), &SkinSelection::Default);
        assert_eq!(
            runtime
                .model()
                .skins()
                .iter()
                .map(AsRef::as_ref)
                .collect::<Vec<_>>(),
            ["shared", "comparison-only"]
        );
        for slot in [SourceSlot::Primary, SourceSlot::Comparison] {
            assert!(
                runtime
                    .source(slot)
                    .expect("runtime source")
                    .selected_skin_present()
            );
            assert!(
                runtime
                    .snapshot()
                    .source(slot)
                    .expect("source snapshot")
                    .selected_skin_present()
            );
        }
        for entity in [primary_entity, comparison_entity] {
            assert!(
                app.world()
                    .entity(entity)
                    .get::<SpinalSkinLayers>()
                    .expect("skin layers")
                    .is_empty()
            );
        }
        assert_eq!(
            issue_cursor
                .read(app.world().resource::<Messages<SpinalIssue>>())
                .filter(|issue| issue.kind() == SpinalIssueKind::MissingSkin)
                .count(),
            0,
            "the removed layer is cleared before the replacement asset animates"
        );
    }

    #[test]
    fn two_bundle_headless_runtime_preserves_barrier_isolation_clock_and_events() {
        let primary_bundle = review_bundle(PRIMARY_REVIEW_JSON, RED_PIXEL_PNG);
        let comparison_bundle = review_bundle(COMPARISON_REVIEW_JSON, BLUE_PIXEL_PNG);
        assert_ne!(
            primary_bundle.file(Path::new("shared.json")),
            comparison_bundle.file(Path::new("shared.json"))
        );
        assert_ne!(
            primary_bundle.file(Path::new("shared.png")),
            comparison_bundle.file(Path::new("shared.png"))
        );

        let config = LaunchConfig {
            primary: LaunchSource::new(primary_bundle, "primary", "primary atlas"),
            comparison: Some(LaunchSource::new(
                comparison_bundle,
                "comparison",
                "comparison atlas",
            )),
            preview_rate: PreviewRate::default(),
        };
        let mut app = App::new();
        prepare_runtime(&mut app, config);
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin {
                watch_for_changes_override: Some(false),
                use_asset_processor_override: Some(false),
                ..default()
            },
            ViewerRuntimePlugin,
        ));
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            100,
        )));
        update_until_ready(&mut app);

        let (primary_entity, comparison_entity, primary_asset, comparison_asset) = {
            let runtime = app.world().resource::<ViewerRuntime>();
            assert!(runtime.has_comparison());
            assert!(runtime.controls_ready());
            assert_eq!(runtime.selected_name(), Some("shared"));
            assert!(runtime.model().transport().is_paused());
            let primary = runtime.source(SourceSlot::Primary).expect("primary source");
            let comparison = runtime
                .source(SourceSlot::Comparison)
                .expect("comparison source");
            (
                primary.entity(),
                comparison.entity(),
                primary.asset().clone(),
                comparison.asset().clone(),
            )
        };
        assert_ne!(primary_asset.id(), comparison_asset.id());

        let (primary_image, comparison_image) = {
            let assets = app.world().resource::<Assets<SpinalAsset>>();
            let primary = assets.get(&primary_asset).expect("primary asset retained");
            let comparison = assets
                .get(&comparison_asset)
                .expect("comparison asset retained");
            assert_eq!(
                primary
                    .skeleton()
                    .animations()
                    .map(|animation| animation.name())
                    .collect::<Vec<_>>(),
                ["shared", "primary-only"]
            );
            assert_eq!(
                comparison
                    .skeleton()
                    .animations()
                    .map(|animation| animation.name())
                    .collect::<Vec<_>>(),
                ["shared", "comparison-only"]
            );
            let primary_page = primary.page(0).expect("primary page");
            let comparison_page = comparison.page(0).expect("comparison page");
            assert_eq!(
                primary_page
                    .source_path()
                    .map(ToString::to_string)
                    .as_deref(),
                Some("spinal-primary://shared.png")
            );
            assert_eq!(
                comparison_page
                    .source_path()
                    .map(ToString::to_string)
                    .as_deref(),
                Some("spinal-comparison://shared.png")
            );
            (
                primary_page.image().clone(),
                comparison_page.image().clone(),
            )
        };
        assert_ne!(primary_image.id(), comparison_image.id());
        let images = app.world().resource::<Assets<Image>>();
        assert_ne!(
            images
                .get(&primary_image)
                .expect("decoded primary image")
                .data,
            images
                .get(&comparison_image)
                .expect("decoded comparison image")
                .data
        );

        // Re-enter the live readiness barrier with only the primary ready. The
        // next poll observes the already loaded comparison asset and releases
        // the barrier deterministically, without relying on task timing.
        {
            let mut runtime = app.world_mut().resource_mut::<ViewerRuntime>();
            let comparison = runtime
                .sources
                .iter_mut()
                .find(|source| source.slot == SourceSlot::Comparison)
                .expect("comparison source");
            comparison.load_state = ViewerLoadState::Loading;
            comparison.runtime_state = SpinalInstanceState::Loading;
            runtime
                .model
                .set_readiness(SourceSlot::Comparison, SourceReadiness::Loading);
        }
        {
            let runtime = app.world().resource::<ViewerRuntime>();
            assert_eq!(
                runtime.model().readiness(SourceSlot::Primary),
                Some(SourceReadiness::Ready)
            );
            assert_eq!(
                runtime.model().readiness(SourceSlot::Comparison),
                Some(SourceReadiness::Loading)
            );
            assert!(!runtime.model().all_present_sources_ready());
            assert!(!runtime.controls_ready());
            assert_eq!(runtime.selected_name(), None);
        }
        app.update();
        {
            let runtime = app.world().resource::<ViewerRuntime>();
            assert!(runtime.controls_ready());
            assert_eq!(runtime.selected_name(), Some("shared"));
            assert!(runtime.model().transport().is_paused());
        }

        let mut event_cursor = app
            .world()
            .resource::<Messages<SpinalAnimationEvent>>()
            .get_cursor_current();
        app.world_mut()
            .resource_mut::<CommandInbox>()
            .push(ViewerCommand::TogglePause);
        app.update();
        assert_eq!(
            event_cursor
                .read(app.world().resource::<Messages<SpinalAnimationEvent>>())
                .count(),
            0,
            "the resume command is sampled at zero delta"
        );
        let seek_revisions = [primary_entity, comparison_entity].map(|entity| {
            app.world()
                .entity(entity)
                .get::<SpinalAnimator>()
                .expect("source animator")
                .seek_revision()
        });
        assert!(
            !app.world()
                .resource::<ViewerRuntime>()
                .model()
                .transport()
                .is_paused()
        );

        app.update();
        assert_eq!(
            app.world()
                .resource::<ViewerRuntime>()
                .model()
                .transport()
                .position(),
            Duration::from_millis(100)
        );
        for (index, entity) in [primary_entity, comparison_entity].into_iter().enumerate() {
            let animator = app
                .world()
                .entity(entity)
                .get::<SpinalAnimator>()
                .expect("source animator");
            assert_eq!(
                animator.seek_revision(),
                seek_revisions[index],
                "ordinary playback must not issue a per-frame seek"
            );
            let playback = app
                .world()
                .entity(entity)
                .get::<SpinalPlaybackState>()
                .expect("source playback state");
            assert_eq!(playback.animation(), Some("shared"));
            assert_eq!(playback.position(), Some(Duration::from_millis(100)));
        }
        let mut event_entities = event_cursor
            .read(app.world().resource::<Messages<SpinalAnimationEvent>>())
            .filter(|event| event.animation() == "shared" && event.event() == "tick")
            .map(SpinalAnimationEvent::entity)
            .collect::<Vec<_>>();
        event_entities.sort_by_key(|entity| entity.to_bits());
        let mut expected_entities = vec![primary_entity, comparison_entity];
        expected_entities.sort_by_key(|entity| entity.to_bits());
        assert_eq!(
            event_entities, expected_entities,
            "one ordinary crossed event is retained independently for each source"
        );

        app.world_mut()
            .resource_mut::<CommandInbox>()
            .push(ViewerCommand::SetLooping(false));
        app.world_mut()
            .resource_mut::<CommandInbox>()
            .push(ViewerCommand::set_playback_speed(1.5).expect("valid browser speed"));
        app.world_mut()
            .resource_mut::<CommandInbox>()
            .push(ViewerCommand::SeekAbsolute(Duration::from_millis(350)));
        app.update();

        let runtime = app.world().resource::<ViewerRuntime>();
        assert_eq!(
            runtime.model().transport().position(),
            Duration::from_millis(350)
        );
        assert!(!runtime.model().transport().is_looping());
        assert_eq!(
            runtime.model().transport().playback_speed().multiplier(),
            1.5
        );
        for entity in [primary_entity, comparison_entity] {
            let animator = app
                .world()
                .entity(entity)
                .get::<SpinalAnimator>()
                .expect("source animator");
            assert_eq!(animator.mode(), Some(PlaybackMode::Once));
            assert_eq!(animator.speed(), 1.5);
            assert_eq!(animator.seek_position(), Some(Duration::from_millis(350)));
            assert!(!animator.is_paused());
        }
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
    fn non_divisor_rebase_is_reserved_for_the_shared_wrap_discontinuity() {
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
            .handle_playback(PlaybackCommand::SeekAbsolute(Duration::from_millis(1_900)))
            .expect("seek before comparison-local wrap");
        model
            .handle_playback(PlaybackCommand::SetPaused(false))
            .expect("resume shared clock");

        let ordinary = model
            .handle_playback(PlaybackCommand::Advance(Duration::from_millis(200)))
            .expect("cross comparison-local wrap")
            .expect("selected animation advances");
        assert_eq!(ordinary.boundary, AdvanceBoundary::None);
        assert_eq!(
            model
                .projected_position(SourceSlot::Comparison)
                .expect("comparison projection"),
            Some(Duration::from_millis(100))
        );
        assert_eq!(
            wrap_rebase_position(&model, SourceSlot::Comparison, ordinary.boundary)
                .expect("ordinary crossing needs no correction"),
            None,
            "a source-local loop crosses naturally so authored events remain observable"
        );

        model
            .handle_playback(PlaybackCommand::SeekAbsolute(Duration::from_millis(2_900)))
            .expect("seek before shared wrap");
        let discontinuity = model
            .handle_playback(PlaybackCommand::Advance(Duration::from_millis(200)))
            .expect("cross shared wrap")
            .expect("selected animation advances");
        assert_eq!(discontinuity.boundary, AdvanceBoundary::Wrapped);
        assert_eq!(
            wrap_rebase_position(&model, SourceSlot::Comparison, discontinuity.boundary,)
                .expect("non-divisor source requires shared-wrap correction"),
            Some(Duration::from_millis(100)),
            "this explicit discontinuity is the only non-divisor rebase point"
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
    fn issue_history_is_bounded_without_claiming_current_activity() {
        let mut runtime = runtime_with(
            ViewerLoadState::Ready,
            SpinalInstanceState::Ready,
            SourceReadiness::Ready,
        );
        for index in 0..MAX_ISSUE_HISTORY + 3 {
            record_local_issue(&mut runtime, format!("issue {index}"));
        }
        assert_eq!(runtime.issue_history.len(), MAX_ISSUE_HISTORY);
        assert_eq!(
            runtime.issue_history.front().map(AsRef::as_ref),
            Some("issue 10")
        );
    }

    #[test]
    fn viewer_runtime_policy_disables_world_space_diagnostic_markers() {
        assert!(!viewer_runtime_config().diagnostic_markers());
    }

    #[test]
    fn preview_time_error_remains_displayable_for_issue_history() {
        assert!(
            !crate::preview::PreviewTimeError::Overflow
                .to_string()
                .is_empty()
        );
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

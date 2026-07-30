use std::{sync::Arc, time::Duration};

use bevy::{
    asset::{AssetId, AssetServer, Assets, LoadState},
    ecs::{
        component::Component,
        entity::Entity,
        lifecycle::RemovedComponents,
        message::{Message, MessageWriter},
        resource::Resource,
        system::{Commands, Query, Res},
    },
    image::Image,
    math::Vec2,
    time::Time,
};
use spinal::{
    AnimationEvent, AnimationPlayer, Diagnostic, DiagnosticCode, DiagnosticScope, DrawItemRef,
    IkSolveIssue, PlayOptions, PlaybackMode, Skeleton, SlotBlendMode,
};

use crate::{
    SpinalAnimator, SpinalAsset, SpinalInstance, SpinalInstanceState, SpinalPlaybackState,
    SpinalPoseOverrides, SpinalSkinLayers,
};

/// Runtime policy shared by all Spinal ECS instances.
#[derive(Clone, Debug, Resource)]
pub struct SpinalRuntimeConfig {
    diagnostic_markers: bool,
    diagnostic_marker_size: f32,
    diagnostic_marker_thickness: f32,
}

impl SpinalRuntimeConfig {
    /// Enables or disables persistent red-cross markers for active
    /// degradations.
    pub fn set_diagnostic_markers(&mut self, enabled: bool) {
        self.diagnostic_markers = enabled;
    }

    /// Returns whether active degradations should be marked in-world.
    #[must_use]
    pub const fn diagnostic_markers(&self) -> bool {
        self.diagnostic_markers
    }

    /// Returns the full local-space size of a diagnostic cross.
    #[must_use]
    pub const fn diagnostic_marker_size(&self) -> f32 {
        self.diagnostic_marker_size
    }

    /// Returns the screen-space stroke thickness of a diagnostic cross in
    /// pixels.
    #[must_use]
    pub const fn diagnostic_marker_thickness(&self) -> f32 {
        self.diagnostic_marker_thickness
    }
}

impl Default for SpinalRuntimeConfig {
    fn default() -> Self {
        Self {
            diagnostic_markers: true,
            diagnostic_marker_size: 24.0,
            diagnostic_marker_thickness: 3.0,
        }
    }
}

/// A stable category for an owned [`SpinalIssue`] message.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum SpinalIssueKind {
    /// The compound Bevy asset failed before a usable value was available.
    AssetLoadFailed,
    /// The requested animation name does not exist in the current asset.
    MissingAnimation,
    /// A requested skin-layer name does not exist in the current asset.
    MissingSkin,
    /// A requested procedural bone name does not exist in the current asset.
    MissingBone,
    /// A retained core diagnostic became active.
    AssetDiagnostic(DiagnosticCode),
    /// IK preserved the finite FK pose because solving was unsafe.
    RuntimeIk(IkSolveIssue),
    /// A referenced atlas page image is not ready.
    MissingAtlasPage,
    /// The adapter omitted a slot using a blend mode outside the profile.
    UnsupportedBlendMode(SlotBlendMode),
    /// The standalone player rejected an otherwise internal update.
    Player,
}

/// An owned, entity-scoped adapter issue.
///
/// Messages are emitted when an issue becomes active. The public
/// [`SpinalInstanceState`] and red-cross marker remain active until it clears.
#[derive(Clone, Debug, Message)]
pub struct SpinalIssue {
    entity: Entity,
    kind: SpinalIssueKind,
    message: Box<str>,
}

impl SpinalIssue {
    /// Returns the affected ECS entity.
    #[must_use]
    pub const fn entity(&self) -> Entity {
        self.entity
    }

    /// Returns the stable issue category.
    #[must_use]
    pub const fn kind(&self) -> SpinalIssueKind {
        self.kind
    }

    /// Returns the human-readable detail.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// An owned authored animation event emitted by one ECS instance.
#[derive(Clone, Debug, Message)]
pub struct SpinalAnimationEvent {
    entity: Entity,
    playback: u64,
    animation: Box<str>,
    event: Box<str>,
    loop_index: u128,
    local_time: Duration,
    integer: i32,
    float: f32,
    string: Option<Box<str>>,
    volume: f32,
    balance: f32,
    degraded: bool,
}

impl SpinalAnimationEvent {
    /// Returns the ECS skeleton entity.
    #[must_use]
    pub const fn entity(&self) -> Entity {
        self.entity
    }

    /// Returns the player-local playback identifier.
    #[must_use]
    pub const fn playback(&self) -> u64 {
        self.playback
    }

    /// Returns the stable animation name.
    #[must_use]
    pub fn animation(&self) -> &str {
        &self.animation
    }

    /// Returns the stable event-definition name.
    #[must_use]
    pub fn event(&self) -> &str {
        &self.event
    }

    /// Returns the zero-based loop containing the occurrence.
    #[must_use]
    pub const fn loop_index(&self) -> u128 {
        self.loop_index
    }

    /// Returns the animation-local event time.
    #[must_use]
    pub const fn local_time(&self) -> Duration {
        self.local_time
    }

    /// Returns the resolved integer payload.
    #[must_use]
    pub const fn integer(&self) -> i32 {
        self.integer
    }

    /// Returns the resolved floating-point payload.
    #[must_use]
    pub const fn float(&self) -> f32 {
        self.float
    }

    /// Returns the resolved optional string payload.
    #[must_use]
    pub fn string(&self) -> Option<&str> {
        self.string.as_deref()
    }

    /// Returns the resolved audio volume payload.
    #[must_use]
    pub const fn volume(&self) -> f32 {
        self.volume
    }

    /// Returns the resolved audio balance payload.
    #[must_use]
    pub const fn balance(&self) -> f32 {
        self.balance
    }

    /// Returns whether unsupported authored event data changed this
    /// occurrence.
    #[must_use]
    pub const fn is_degraded(&self) -> bool {
        self.degraded
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct IssueFingerprint {
    kind: SpinalIssueKind,
    message: Box<str>,
}

#[derive(Clone, Debug)]
struct ActiveIssue {
    fingerprint: IssueFingerprint,
    point: Vec2,
}

impl ActiveIssue {
    fn new(kind: SpinalIssueKind, message: impl Into<Box<str>>, point: Vec2) -> Self {
        Self {
            fingerprint: IssueFingerprint {
                kind,
                message: message.into(),
            },
            point,
        }
    }
}

#[derive(Component, Debug)]
pub(crate) struct SpinalRuntime {
    skeleton: Skeleton,
    player: AnimationPlayer,
    animation_revision: Option<u64>,
    skin_request: Vec<Box<str>>,
    override_request: Vec<(Box<str>, spinal::BoneTransform)>,
    resolved_overrides: Vec<(spinal::BoneId, spinal::BoneTransform)>,
    active_issues: Vec<IssueFingerprint>,
}

#[derive(Component, Debug)]
pub(crate) struct SpinalSelection(AssetId<SpinalAsset>);

impl SpinalRuntime {
    fn new(asset: Arc<spinal::SkeletonAsset>) -> Self {
        let skeleton = Skeleton::new(asset);
        let player = AnimationPlayer::new(&skeleton);
        Self {
            skeleton,
            player,
            animation_revision: None,
            skin_request: Vec::new(),
            override_request: Vec::new(),
            resolved_overrides: Vec::new(),
            active_issues: Vec::new(),
        }
    }

    fn uses(&self, asset: &SpinalAsset) -> bool {
        Arc::ptr_eq(self.skeleton.asset_handle(), asset.skeleton())
    }
}

#[derive(Clone, Debug)]
#[cfg_attr(not(feature = "render"), allow(dead_code))]
pub(crate) struct SpinalDraw {
    pub(crate) page_ordinal: usize,
    pub(crate) positions: [Vec2; 4],
    pub(crate) uvs: [Vec2; 4],
    pub(crate) color: [f32; 4],
}

#[derive(Component, Debug, Default)]
pub(crate) struct SpinalFrame {
    pub(crate) revision: u64,
    pub(crate) draws: Vec<SpinalDraw>,
    pub(crate) issue_points: Vec<Vec2>,
    pub(crate) ready: bool,
}

#[allow(clippy::type_complexity)]
pub(crate) fn prepare_instances(
    mut commands: Commands<'_, '_>,
    assets: Res<'_, Assets<SpinalAsset>>,
    asset_server: Res<'_, AssetServer>,
    mut issues: MessageWriter<'_, SpinalIssue>,
    mut instances: Query<
        '_,
        '_,
        (
            Entity,
            &SpinalInstance,
            &mut SpinalInstanceState,
            &mut SpinalPlaybackState,
            Option<&SpinalSelection>,
            Option<&SpinalRuntime>,
        ),
    >,
) {
    for (entity, instance, mut state, mut playback, selection, runtime) in &mut instances {
        let selected_id = instance.asset().id();
        let selection_changed = selection.is_some_and(|selection| selection.0 != selected_id);
        if selection.is_none() || selection_changed {
            commands.entity(entity).insert(SpinalSelection(selected_id));
        }
        if selection_changed {
            commands
                .entity(entity)
                .remove::<(SpinalRuntime, SpinalFrame)>();
            replace_if_changed(&mut *state, SpinalInstanceState::Loading);
            replace_if_changed(&mut *playback, SpinalPlaybackState::Idle);
        }

        if let Some(asset) = assets.get(instance.asset()) {
            if selection_changed || runtime.is_none_or(|runtime| !runtime.uses(asset)) {
                commands.entity(entity).insert((
                    SpinalRuntime::new(Arc::clone(asset.skeleton())),
                    SpinalFrame::default(),
                ));
                replace_if_changed(&mut *state, SpinalInstanceState::Loading);
            }
            continue;
        }

        if runtime.is_some() && !selection_changed {
            continue;
        }

        if let Some(LoadState::Failed(error)) = asset_server.get_load_state(instance.asset().id()) {
            let newly_failed = *state != SpinalInstanceState::Failed;
            replace_if_changed(&mut *state, SpinalInstanceState::Failed);
            if newly_failed {
                issues.write(SpinalIssue {
                    entity,
                    kind: SpinalIssueKind::AssetLoadFailed,
                    message: error.to_string().into(),
                });
            }
        } else {
            replace_if_changed(&mut *state, SpinalInstanceState::Loading);
        }
    }
}

pub(crate) fn cleanup_removed_instances(
    mut commands: Commands<'_, '_>,
    mut removed: RemovedComponents<'_, '_, SpinalInstance>,
) {
    for entity in removed.read() {
        if let Ok(mut entity) = commands.get_entity(entity) {
            entity.remove::<(SpinalSelection, SpinalRuntime, SpinalFrame)>();
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn update_instances(
    assets: Res<'_, Assets<SpinalAsset>>,
    images: Res<'_, Assets<Image>>,
    time: Option<Res<'_, Time>>,
    config: Res<'_, SpinalRuntimeConfig>,
    mut event_messages: MessageWriter<'_, SpinalAnimationEvent>,
    mut issue_messages: MessageWriter<'_, SpinalIssue>,
    mut instances: Query<
        '_,
        '_,
        (
            Entity,
            &SpinalInstance,
            &SpinalAnimator,
            &SpinalSkinLayers,
            &SpinalPoseOverrides,
            &mut SpinalInstanceState,
            &mut SpinalPlaybackState,
            &mut SpinalRuntime,
            &mut SpinalFrame,
        ),
    >,
) {
    let wall_delta = time.as_deref().map_or(Duration::ZERO, Time::delta);

    for (
        entity,
        instance,
        animator,
        skin_layers,
        pose_overrides,
        mut instance_state,
        mut playback_state,
        mut runtime,
        mut output,
    ) in &mut instances
    {
        let Some(asset) = assets.get(instance.asset()) else {
            continue;
        };
        if !runtime.uses(asset) {
            continue;
        }

        let root_point = root_point_from_unsolved(&runtime.skeleton);
        let mut active_issues = Vec::new();
        apply_skin_intent(&mut runtime, skin_layers, root_point, &mut active_issues);
        apply_override_intent(&mut runtime, pose_overrides, root_point, &mut active_issues);
        apply_animation_intent(&mut runtime, animator, root_point, &mut active_issues);

        let delta = if animator.is_paused() {
            Duration::ZERO
        } else {
            scale_duration(wall_delta, animator.speed())
        };

        let SpinalRuntime {
            skeleton,
            player,
            resolved_overrides,
            active_issues: previous_issues,
            ..
        } = &mut *runtime;
        let mut authored_events = Vec::new();
        let mut event_issues = Vec::new();
        let core_asset = Arc::clone(asset.skeleton());
        let update_result = player.update(skeleton, delta, &mut |event: AnimationEvent<'_>| {
            let animation_name = core_asset
                .animation(event.animation())
                .map_or("<invalid>", |animation| animation.name());
            let owned = owned_event(entity, animation_name, event);
            for diagnostic in event
                .diagnostics()
                .filter(|diagnostic| diagnostic.is_degraded())
            {
                event_issues.push(ActiveIssue::new(
                    SpinalIssueKind::AssetDiagnostic(diagnostic.code()),
                    diagnostic.message(),
                    root_point,
                ));
            }
            authored_events.push(owned);
        });

        let mut editable = match update_result {
            Ok(editable) => editable,
            Err(error) => {
                active_issues.push(ActiveIssue::new(
                    SpinalIssueKind::Player,
                    error.to_string(),
                    root_point,
                ));
                output.draws.clear();
                output.issue_points.clear();
                output.ready = false;
                replace_if_changed(&mut *instance_state, SpinalInstanceState::Failed);
                emit_new_issues(entity, previous_issues, &active_issues, &mut issue_messages);
                continue;
            }
        };

        {
            let mut editor = editable.edit();
            for (bone, transform) in resolved_overrides.iter().copied() {
                editor
                    .set_bone_local(bone, transform)
                    .expect("cached override IDs belong to the active skeleton");
            }
        }
        let solved = editable.solve();

        active_issues.extend(event_issues);
        append_frame_issues(&solved, &mut active_issues);
        let frame_ready = write_draws(asset, &images, &solved, &mut output, &mut active_issues);

        output.issue_points.clear();
        if config.diagnostic_markers() {
            for issue in &active_issues {
                if !output.issue_points.contains(&issue.point) {
                    output.issue_points.push(issue.point);
                }
            }
        }
        output.ready = frame_ready;
        output.revision = output.revision.wrapping_add(1);

        emit_new_issues(entity, previous_issues, &active_issues, &mut issue_messages);
        for authored_event in authored_events {
            event_messages.write(authored_event);
        }

        let state = if !frame_ready {
            SpinalInstanceState::Loading
        } else if active_issues.is_empty() {
            SpinalInstanceState::Ready
        } else {
            SpinalInstanceState::Degraded
        };
        replace_if_changed(&mut *instance_state, state);
        replace_if_changed(&mut *playback_state, playback_observation(skeleton, player));
    }
}

fn apply_skin_intent(
    runtime: &mut SpinalRuntime,
    skin_layers: &SpinalSkinLayers,
    root_point: Vec2,
    issues: &mut Vec<ActiveIssue>,
) {
    let requested = skin_layers.iter().map(Box::<str>::from).collect::<Vec<_>>();
    let mut resolved = Vec::with_capacity(requested.len());
    for name in &requested {
        if let Some(id) = runtime.skeleton.asset().skin_id(name) {
            resolved.push(id);
        } else {
            issues.push(ActiveIssue::new(
                SpinalIssueKind::MissingSkin,
                format!("skin layer `{name}` does not exist"),
                root_point,
            ));
        }
    }
    if requested != runtime.skin_request {
        runtime
            .skeleton
            .set_skin_layers(&resolved)
            .expect("resolved skin IDs belong to the active skeleton");
        runtime.skin_request = requested;
    }
}

fn apply_override_intent(
    runtime: &mut SpinalRuntime,
    pose_overrides: &SpinalPoseOverrides,
    root_point: Vec2,
    issues: &mut Vec<ActiveIssue>,
) {
    let requested = pose_overrides
        .iter()
        .map(|replacement| {
            (
                Box::<str>::from(replacement.bone()),
                replacement.transform(),
            )
        })
        .collect::<Vec<_>>();
    if requested != runtime.override_request {
        runtime.resolved_overrides.clear();
        for (name, transform) in &requested {
            if let Some(id) = runtime.skeleton.asset().bone_id(name) {
                runtime.resolved_overrides.push((id, *transform));
            }
        }
        runtime.override_request = requested.clone();
    }
    for (name, _transform) in requested {
        if runtime.skeleton.asset().bone_id(&name).is_none() {
            issues.push(ActiveIssue::new(
                SpinalIssueKind::MissingBone,
                format!("procedural bone `{name}` does not exist"),
                root_point,
            ));
        }
    }
}

fn apply_animation_intent(
    runtime: &mut SpinalRuntime,
    animator: &SpinalAnimator,
    root_point: Vec2,
    issues: &mut Vec<ActiveIssue>,
) {
    let desired_id = animator
        .animation()
        .and_then(|name| runtime.skeleton.asset().animation_id(name));
    if let Some(name) = animator.animation()
        && desired_id.is_none()
    {
        issues.push(ActiveIssue::new(
            SpinalIssueKind::MissingAnimation,
            format!("animation `{name}` does not exist"),
            root_point,
        ));
    }

    if runtime.animation_revision == Some(animator.revision()) {
        return;
    }

    match (desired_id, animator.mode()) {
        (Some(animation), Some(mode)) => {
            let options = match mode {
                PlaybackMode::Once => PlayOptions::once(),
                PlaybackMode::Loop => PlayOptions::looping(),
                _other => PlayOptions::once(),
            }
            .with_transition(animator.transition());
            runtime
                .player
                .play(animation, options)
                .expect("resolved animation IDs belong to the active player");
        }
        (None, None) => {
            runtime.player.stop(animator.transition());
        }
        (None, Some(_mode)) => {
            runtime.player.stop(spinal::Transition::Immediate);
        }
        (Some(_animation), None) => {
            runtime.player.stop(spinal::Transition::Immediate);
        }
    }
    runtime.animation_revision = Some(animator.revision());
}

fn append_frame_issues(frame: &spinal::SolvedFrame<'_>, issues: &mut Vec<ActiveIssue>) {
    for diagnostic in frame
        .active_diagnostics()
        .filter(|diagnostic| diagnostic.is_degraded())
    {
        issues.push(ActiveIssue::new(
            SpinalIssueKind::AssetDiagnostic(diagnostic.code()),
            diagnostic.message(),
            diagnostic_point(frame, diagnostic),
        ));
    }
    for (constraint, status) in frame.ik_statuses() {
        if let Some(issue) = status.issue() {
            let point = frame
                .asset()
                .ik_constraint(constraint)
                .ok()
                .and_then(|constraint| frame.bone(constraint.target()).ok())
                .map_or(Vec2::ZERO, |bone| bone.world_transform().translation());
            issues.push(ActiveIssue::new(
                SpinalIssueKind::RuntimeIk(issue),
                "IK preserved the finite FK pose because target geometry was unsafe",
                point,
            ));
        }
    }
}

fn write_draws(
    asset: &SpinalAsset,
    images: &Assets<Image>,
    solved: &spinal::SolvedFrame<'_>,
    output: &mut SpinalFrame,
    issues: &mut Vec<ActiveIssue>,
) -> bool {
    output.draws.clear();
    let mut ready = true;
    for draw in solved.draw_items() {
        let region = match draw {
            DrawItemRef::Region(region) => region,
            _other => continue,
        };
        let page = solved
            .asset()
            .atlas_page(region.atlas_page())
            .expect("draw page IDs belong to the solved asset");
        let Some(bevy_page) = asset.page(page.ordinal()) else {
            ready = false;
            issues.push(ActiveIssue::new(
                SpinalIssueKind::MissingAtlasPage,
                format!("atlas page {} is not linked to a Bevy image", page.name()),
                Vec2::ZERO,
            ));
            continue;
        };
        let Some(image) = images.get(bevy_page.image()) else {
            ready = false;
            continue;
        };
        let atlas_region = solved
            .asset()
            .atlas_region(region.atlas_region())
            .expect("draw region IDs belong to the solved asset");
        if page.alpha_encoding() != spinal::AlphaEncoding::Straight {
            continue;
        }
        let Some(uvs) = region.uvs().or_else(|| {
            normalized_uvs(
                image.width(),
                image.height(),
                atlas_region.bounds(),
                atlas_region.rotation().as_degrees(),
            )
        }) else {
            continue;
        };
        if region.blend_mode() != SlotBlendMode::Normal {
            let slot_point = solved
                .asset()
                .slot(region.slot())
                .ok()
                .and_then(|slot| solved.bone(slot.bone()).ok())
                .map_or(Vec2::ZERO, |bone| bone.world_transform().translation());
            issues.push(ActiveIssue::new(
                SpinalIssueKind::UnsupportedBlendMode(region.blend_mode()),
                format!(
                    "slot blend mode `{}` is outside the renderer profile; the slot was omitted",
                    solved
                        .asset()
                        .slot(region.slot())
                        .map_or("unknown", |slot| slot.blend_token())
                ),
                slot_point,
            ));
            continue;
        }
        output.draws.push(SpinalDraw {
            page_ordinal: page.ordinal(),
            positions: region.positions(),
            uvs,
            color: region.color().to_array(),
        });
    }
    if !ready {
        output.draws.clear();
    }
    ready
}

fn normalized_uvs(
    page_width: u32,
    page_height: u32,
    bounds: spinal::PixelRect,
    rotation_degrees: f32,
) -> Option<[Vec2; 4]> {
    if page_width == 0 || page_height == 0 {
        return None;
    }
    let degrees = rotation_degrees;
    let (packed_width, packed_height) = if matches!(degrees, 90.0 | 270.0) {
        (bounds.height(), bounds.width())
    } else {
        (bounds.width(), bounds.height())
    };
    let left = bounds.x() as f32 / page_width as f32;
    let top = bounds.y() as f32 / page_height as f32;
    let right = bounds.x().saturating_add(packed_width) as f32 / page_width as f32;
    let bottom = bounds.y().saturating_add(packed_height) as f32 / page_height as f32;
    let top_left = Vec2::new(left, top);
    let top_right = Vec2::new(right, top);
    let bottom_right = Vec2::new(right, bottom);
    let bottom_left = Vec2::new(left, bottom);
    match degrees {
        0.0 | 360.0 => Some([bottom_left, top_left, top_right, bottom_right]),
        90.0 => Some([bottom_right, bottom_left, top_left, top_right]),
        180.0 => Some([top_right, bottom_right, bottom_left, top_left]),
        270.0 => Some([top_left, top_right, bottom_right, bottom_left]),
        _other => None,
    }
}

fn diagnostic_point(frame: &spinal::SolvedFrame<'_>, diagnostic: &Diagnostic) -> Vec2 {
    let asset = frame.asset();
    let bone = match diagnostic.scope() {
        DiagnosticScope::Bone(bone) => Some(bone),
        DiagnosticScope::Slot(slot) => asset.slot(slot).ok().map(|slot| slot.bone()),
        DiagnosticScope::Attachment(attachment) => asset
            .attachment(attachment)
            .ok()
            .and_then(|attachment| asset.slot(attachment.slot()).ok())
            .map(|slot| slot.bone()),
        DiagnosticScope::IkConstraint(constraint) => asset
            .ik_constraint(constraint)
            .ok()
            .map(|constraint| constraint.target()),
        DiagnosticScope::Constraint(constraint) => asset
            .constraint(constraint)
            .ok()
            .and_then(|constraint| constraint.as_ik())
            .map(|constraint| constraint.target()),
        DiagnosticScope::Asset => None,
        DiagnosticScope::Skin(_skin) => None,
        DiagnosticScope::Animation(_animation) => None,
        DiagnosticScope::Event(_event) => None,
        DiagnosticScope::AtlasPage(_page) => None,
        DiagnosticScope::AtlasRegion(_region) => None,
        _other => None,
    };
    bone.and_then(|bone| frame.bone(bone).ok()).map_or_else(
        || root_point(frame),
        |bone| bone.world_transform().translation(),
    )
}

fn root_point(frame: &spinal::SolvedFrame<'_>) -> Vec2 {
    frame
        .bones()
        .next()
        .map_or(Vec2::ZERO, |bone| bone.world_transform().translation())
}

fn root_point_from_unsolved(skeleton: &Skeleton) -> Vec2 {
    skeleton
        .asset()
        .bones()
        .next()
        .map_or(Vec2::ZERO, |bone| bone.setup_transform().translation())
}

fn playback_observation(skeleton: &Skeleton, player: &AnimationPlayer) -> SpinalPlaybackState {
    let status = player.status();
    if let Some(animation) = status.animation() {
        let name = skeleton
            .asset()
            .animation(animation)
            .map_or("<invalid>", |animation| animation.name());
        SpinalPlaybackState::Playing {
            playback: status
                .playback()
                .expect("an active animation has a playback ID")
                .get()
                .get(),
            animation: name.into(),
            mode: status
                .mode()
                .expect("an active animation has a playback mode"),
            position: status
                .position()
                .expect("an active animation has a local position"),
            loop_index: status
                .loop_index()
                .expect("an active animation has a loop index"),
            complete: status.is_complete(),
            transition_mix: status.transition_mix(),
        }
    } else if status.is_stopping() {
        SpinalPlaybackState::Stopping {
            transition_mix: status.transition_mix(),
        }
    } else {
        SpinalPlaybackState::Idle
    }
}

fn owned_event(
    entity: Entity,
    animation_name: &str,
    event: AnimationEvent<'_>,
) -> SpinalAnimationEvent {
    SpinalAnimationEvent {
        entity,
        playback: event.playback().get().get(),
        animation: animation_name.into(),
        event: event.definition().name().into(),
        loop_index: event.loop_index(),
        local_time: event.local_time(),
        integer: event.integer(),
        float: event.float(),
        string: event.string().map(Into::into),
        volume: event.volume(),
        balance: event.balance(),
        degraded: event.has_degradations(),
    }
}

fn scale_duration(duration: Duration, speed: f32) -> Duration {
    Duration::try_from_secs_f64(duration.as_secs_f64() * f64::from(speed)).unwrap_or(Duration::MAX)
}

fn emit_new_issues(
    entity: Entity,
    previous: &mut Vec<IssueFingerprint>,
    current: &[ActiveIssue],
    messages: &mut MessageWriter<'_, SpinalIssue>,
) {
    for issue in current {
        if !previous.contains(&issue.fingerprint) {
            messages.write(SpinalIssue {
                entity,
                kind: issue.fingerprint.kind,
                message: issue.fingerprint.message.clone(),
            });
        }
    }
    previous.clear();
    previous.extend(current.iter().map(|issue| issue.fingerprint.clone()));
}

fn replace_if_changed<T: PartialEq>(current: &mut T, replacement: T) {
    if *current != replacement {
        *current = replacement;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omitted_page_size_uses_actual_image_dimensions() {
        let bounds = spinal::PixelRect::new(10, 20, 30, 40);
        let uvs = normalized_uvs(100, 200, bounds, 0.0).expect("page has dimensions");
        assert_eq!(
            uvs,
            [
                Vec2::new(0.1, 0.3),
                Vec2::new(0.1, 0.1),
                Vec2::new(0.4, 0.1),
                Vec2::new(0.4, 0.3),
            ]
        );
    }

    #[test]
    fn duration_scaling_never_panics_for_a_finite_speed() {
        assert_eq!(scale_duration(Duration::from_secs(1), 0.0), Duration::ZERO);
        assert_eq!(
            scale_duration(Duration::from_millis(500), 2.0),
            Duration::from_secs(1)
        );
        assert_eq!(scale_duration(Duration::MAX, f32::MAX), Duration::MAX);
    }
}

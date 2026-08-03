use std::{ops::Range, sync::Arc, time::Duration};

use bevy::{
    asset::{AssetId, AssetServer, Assets, LoadState},
    ecs::{
        change_detection::DetectChangesMut,
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
    AnimationEvent, AnimationMixer, Diagnostic, DiagnosticCode, DiagnosticScope, DrawItemRef,
    IkSolveIssue, Mix, PlayOptions, PlaybackMode, Skeleton, SlotBlendMode, TrackAnimationEvent,
    TrackId, TrackOptions,
};

use crate::{
    SpinalAnimationTracks, SpinalAnimator, SpinalAsset, SpinalControlTargets, SpinalInstance,
    SpinalInstanceState, SpinalPlaybackState, SpinalPoseOverrides, SpinalSkinLayers,
    SpinalTrackState, SpinalTrackStates,
    components::{DesiredPlayback, TrackNamespace},
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
    /// A transform constraint preserved its finite unconstrained rotation
    /// because solving was unsafe.
    RuntimeTransform(spinal::TransformSolveIssue),
    /// A referenced atlas page image is not ready.
    MissingAtlasPage,
    /// The adapter omitted a slot using a blend mode outside the profile.
    UnsupportedBlendMode(SlotBlendMode),
    /// The standalone player rejected an otherwise internal update.
    Player,
    /// A named skeleton-space control target could not be applied.
    ControlTarget,
    /// An active override track contains a deferred property.
    UnsupportedOverrideProperty(spinal::PropertyKey),
}

/// An owned, entity-scoped adapter issue.
///
/// Messages are emitted when an issue becomes active. The public
/// [`SpinalInstanceState`] and red-cross marker remain active until it clears.
#[derive(Clone, Debug, Message)]
pub struct SpinalIssue {
    entity: Entity,
    track: Option<Box<str>>,
    kind: SpinalIssueKind,
    message: Box<str>,
}

impl SpinalIssue {
    /// Returns the affected ECS entity.
    #[must_use]
    pub const fn entity(&self) -> Entity {
        self.entity
    }

    /// Returns the stable override-track key, or `None` for an entity-wide
    /// or base-track issue.
    #[must_use]
    pub fn track(&self) -> Option<&str> {
        self.track.as_deref()
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
    track: Option<Box<str>>,
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

    /// Returns the stable override-track key, or `None` for the base track.
    #[must_use]
    pub fn track(&self) -> Option<&str> {
        self.track.as_deref()
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
    track: Option<Box<str>>,
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
                track: None,
                kind,
                message: message.into(),
            },
            point,
        }
    }

    fn with_track(mut self, track: Option<&str>) -> Self {
        self.fingerprint.track = track.map(Into::into);
        self
    }
}

#[derive(Component, Debug)]
pub(crate) struct SpinalRuntime {
    skeleton: Skeleton,
    mixer: AnimationMixer,
    animation_intent: Option<CachedAnimationIntent>,
    animation_seek: Option<CachedAnimationSeek>,
    track_intents: Vec<CachedTrackIntent>,
    track_weight_seeds: Vec<CachedTrackWeight>,
    skin_request: Vec<Box<str>>,
    override_request: Vec<(Box<str>, spinal::BoneTransform)>,
    resolved_overrides: Vec<(spinal::BoneId, spinal::BoneTransform)>,
    target_request: Vec<(Box<str>, Vec2)>,
    resolved_targets: Vec<ResolvedControlTarget>,
    active_issues: Vec<IssueFingerprint>,
}

#[derive(Debug)]
struct CachedTrackIntent {
    key: Box<str>,
    namespace: Arc<TrackNamespace>,
    incarnation: u64,
    track: TrackId,
    play_revision: Option<u64>,
    desired: Option<DesiredPlayback>,
    stop_transition: spinal::Transition,
    weight_revision: Option<u64>,
    declared_weight: spinal::Mix,
    weight_fade: Option<spinal::WeightFade>,
}

#[derive(Debug)]
struct CachedTrackWeight {
    key: Box<str>,
    namespace: Arc<TrackNamespace>,
    incarnation: u64,
    weight: spinal::Mix,
}

#[derive(Clone, Copy, Debug)]
struct ResolvedControlTarget {
    request_index: usize,
    bone: spinal::BoneId,
}

#[derive(Debug)]
struct CachedAnimationIntent {
    revision: u64,
    animation: Option<Box<str>>,
    mode: Option<PlaybackMode>,
    transition: spinal::Transition,
}

#[derive(Clone, Copy, Debug)]
struct CachedAnimationSeek {
    revision: u64,
    position: Option<Duration>,
}

impl CachedAnimationIntent {
    fn from_animator(animator: &SpinalAnimator) -> Self {
        Self {
            revision: animator.revision(),
            animation: animator.animation().map(Box::<str>::from),
            mode: animator.mode(),
            transition: animator.transition(),
        }
    }

    fn matches(&self, animator: &SpinalAnimator) -> bool {
        self.revision == animator.revision()
            && self.animation.as_deref() == animator.animation()
            && self.mode == animator.mode()
            && self.transition == animator.transition()
    }
}

impl CachedAnimationSeek {
    fn from_animator(animator: &SpinalAnimator) -> Self {
        Self {
            revision: animator.seek_revision(),
            position: animator.seek_position(),
        }
    }

    fn matches(self, animator: &SpinalAnimator) -> bool {
        self.revision == animator.seek_revision() && self.position == animator.seek_position()
    }
}

#[derive(Component, Debug)]
pub(crate) struct SpinalSelection(AssetId<SpinalAsset>);

impl SpinalRuntime {
    fn new(asset: Arc<spinal::SkeletonAsset>, previous: Option<&Self>) -> Self {
        let track_weight_seeds = previous.map_or_else(Vec::new, |previous| {
            previous
                .track_intents
                .iter()
                .filter_map(|intent| {
                    previous
                        .mixer
                        .track(intent.track)
                        .ok()
                        .map(|track| CachedTrackWeight {
                            key: intent.key.clone(),
                            namespace: Arc::clone(&intent.namespace),
                            incarnation: intent.incarnation,
                            weight: track.weight(),
                        })
                })
                .collect()
        });
        let skeleton = Skeleton::new(asset);
        let mixer = AnimationMixer::new(&skeleton);
        Self {
            skeleton,
            mixer,
            animation_intent: None,
            animation_seek: None,
            track_intents: Vec::new(),
            track_weight_seeds,
            skin_request: Vec::new(),
            override_request: Vec::new(),
            resolved_overrides: Vec::new(),
            target_request: Vec::new(),
            resolved_targets: Vec::new(),
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
    pub(crate) vertices: Range<usize>,
    pub(crate) indices: Range<usize>,
    pub(crate) color: [f32; 4],
}

#[derive(Clone, Copy, Debug)]
#[cfg_attr(not(feature = "render"), allow(dead_code))]
pub(crate) struct SpinalVertex {
    pub(crate) position: Vec2,
    pub(crate) uv: Vec2,
}

#[derive(Component, Debug, Default)]
pub(crate) struct SpinalFrame {
    pub(crate) revision: u64,
    pub(crate) draws: Vec<SpinalDraw>,
    pub(crate) vertices: Vec<SpinalVertex>,
    pub(crate) indices: Vec<u32>,
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
            &mut SpinalTrackStates,
            Option<&SpinalSelection>,
            Option<&SpinalRuntime>,
        ),
    >,
) {
    for (entity, instance, mut state, mut playback, mut tracks, selection, runtime) in
        &mut instances
    {
        let selected_id = instance.asset().id();
        let selection_changed = selection.is_some_and(|selection| selection.0 != selected_id);
        if selection.is_none() || selection_changed {
            commands.entity(entity).insert(SpinalSelection(selected_id));
        }
        if selection_changed {
            commands
                .entity(entity)
                .remove::<(SpinalRuntime, SpinalFrame)>();
            state.set_if_neq(SpinalInstanceState::Loading);
            playback.set_if_neq(SpinalPlaybackState::Idle);
            tracks.set_if_neq(SpinalTrackStates::default());
        }

        if let Some(asset) = assets.get(instance.asset()) {
            if selection_changed || runtime.is_none_or(|runtime| !runtime.uses(asset)) {
                let previous = if selection_changed { None } else { runtime };
                commands.entity(entity).insert((
                    SpinalRuntime::new(Arc::clone(asset.skeleton()), previous),
                    SpinalFrame::default(),
                ));
                state.set_if_neq(SpinalInstanceState::Loading);
            }
            continue;
        }

        if runtime.is_some() && !selection_changed {
            continue;
        }

        if let Some(LoadState::Failed(error)) = asset_server.get_load_state(instance.asset().id()) {
            let newly_failed = *state != SpinalInstanceState::Failed;
            state.set_if_neq(SpinalInstanceState::Failed);
            if newly_failed {
                issues.write(SpinalIssue {
                    entity,
                    track: None,
                    kind: SpinalIssueKind::AssetLoadFailed,
                    message: error.to_string().into(),
                });
            }
        } else {
            state.set_if_neq(SpinalInstanceState::Loading);
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
            &SpinalAnimationTracks,
            &SpinalSkinLayers,
            &SpinalPoseOverrides,
            &SpinalControlTargets,
            &mut SpinalInstanceState,
            &mut SpinalPlaybackState,
            &mut SpinalTrackStates,
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
        animation_tracks,
        skin_layers,
        pose_overrides,
        control_targets,
        mut instance_state,
        mut playback_state,
        mut track_states,
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
        apply_target_intent(
            &mut runtime,
            control_targets,
            root_point,
            &mut active_issues,
        );
        let base_seek_applied =
            apply_animation_intent(&mut runtime, animator, root_point, &mut active_issues);
        apply_track_intent(
            &mut runtime,
            animation_tracks,
            root_point,
            &mut active_issues,
        );

        let SpinalRuntime {
            skeleton,
            mixer,
            track_intents,
            resolved_overrides,
            target_request,
            resolved_targets,
            animation_seek,
            active_issues: previous_issues,
            ..
        } = &mut *runtime;
        let mut authored_events = Vec::new();
        let mut event_issues = Vec::new();
        let core_asset = Arc::clone(asset.skeleton());
        let base_track = mixer.base_track_id();
        let update_result = mixer.update(
            skeleton,
            wall_delta,
            &mut |track_event: TrackAnimationEvent<'_>| {
                let event = track_event.event();
                let animation_name = core_asset
                    .animation(event.animation())
                    .map_or("<invalid>", |animation| animation.name());
                let track = (track_event.track() != base_track).then(|| {
                    track_intents
                        .iter()
                        .find(|intent| intent.track == track_event.track())
                        .map_or("<removed>", |intent| intent.key.as_ref())
                });
                let owned = owned_event(entity, track, animation_name, event);
                for diagnostic in event
                    .diagnostics()
                    .filter(|diagnostic| diagnostic.is_degraded())
                {
                    event_issues.push(
                        ActiveIssue::new(
                            SpinalIssueKind::AssetDiagnostic(diagnostic.code()),
                            diagnostic.message(),
                            root_point,
                        )
                        .with_track(track),
                    );
                }
                authored_events.push(owned);
            },
        );
        if base_seek_applied {
            mixer.base_track_mut().set_paused(animator.is_paused());
        }

        let mut editable = match update_result {
            Ok(editable) => editable,
            Err(error) => {
                if base_seek_applied {
                    // No sought pose was published, so the one-shot command
                    // must remain pending for the first successful frame.
                    *animation_seek = None;
                }
                active_issues.push(ActiveIssue::new(
                    SpinalIssueKind::Player,
                    error.to_string(),
                    root_point,
                ));
                output.draws.clear();
                output.issue_points.clear();
                output.ready = false;
                instance_state.set_if_neq(SpinalInstanceState::Failed);
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
        {
            let mut targets = editable.targets();
            for resolved in resolved_targets.iter().copied() {
                let (name, position) = &target_request[resolved.request_index];
                if let Err(error) = targets.set_skeleton_position(resolved.bone, *position) {
                    active_issues.push(ActiveIssue::new(
                        SpinalIssueKind::ControlTarget,
                        format!("control target bone `{name}` could not be placed: {error}"),
                        root_point,
                    ));
                }
            }
        }
        let solved = editable.solve();

        active_issues.extend(event_issues);
        append_frame_issues(&solved, &mut active_issues);
        for issue in mixer.active_deferred_properties() {
            let track = track_intents
                .iter()
                .find(|intent| intent.track == issue.track())
                .map_or("<removed>", |intent| intent.key.as_ref());
            active_issues.push(
                ActiveIssue::new(
                    SpinalIssueKind::UnsupportedOverrideProperty(issue.property()),
                    format!(
                        "override track `{track}` property {:?} is deferred and was ignored",
                        issue.property()
                    ),
                    root_point,
                )
                .with_track(Some(track)),
            );
        }
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
        } else if active_issues.is_empty() && output.draws.is_empty() {
            SpinalInstanceState::ReadyNoDraws
        } else if active_issues.is_empty() {
            SpinalInstanceState::Ready
        } else if output.draws.is_empty() {
            SpinalInstanceState::DegradedNoDraws
        } else {
            SpinalInstanceState::Degraded
        };
        instance_state.set_if_neq(state);
        let base_status = mixer.base_track().status();
        let playback_changed = write_playback_observation(
            playback_state.bypass_change_detection(),
            skeleton.asset(),
            base_status,
        );
        if playback_changed {
            playback_state.set_changed();
        }
        let tracks_changed = write_track_observations(
            track_states.bypass_change_detection(),
            skeleton.asset(),
            mixer,
            track_intents,
        );
        if tracks_changed {
            track_states.set_changed();
        }
    }
}

fn apply_skin_intent(
    runtime: &mut SpinalRuntime,
    skin_layers: &SpinalSkinLayers,
    root_point: Vec2,
    issues: &mut Vec<ActiveIssue>,
) {
    let request_changed = runtime.skin_request.len() != skin_layers.iter().len()
        || runtime
            .skin_request
            .iter()
            .map(AsRef::as_ref)
            .zip(skin_layers.iter())
            .any(|(cached, requested)| cached != requested);
    if request_changed {
        runtime.skin_request.clear();
        runtime
            .skin_request
            .extend(skin_layers.iter().map(Box::<str>::from));
        let resolved = runtime
            .skin_request
            .iter()
            .filter_map(|name| runtime.skeleton.asset().skin_id(name))
            .collect::<Vec<_>>();
        runtime
            .skeleton
            .set_skin_layers(&resolved)
            .expect("resolved skin IDs belong to the active skeleton");
    }
    for name in &runtime.skin_request {
        if runtime.skeleton.asset().skin_id(name).is_none() {
            issues.push(ActiveIssue::new(
                SpinalIssueKind::MissingSkin,
                format!("skin layer `{name}` does not exist"),
                root_point,
            ));
        }
    }
}

fn apply_override_intent(
    runtime: &mut SpinalRuntime,
    pose_overrides: &SpinalPoseOverrides,
    root_point: Vec2,
    issues: &mut Vec<ActiveIssue>,
) {
    let request_changed = runtime.override_request.len() != pose_overrides.iter().len()
        || runtime
            .override_request
            .iter()
            .zip(pose_overrides.iter())
            .any(|((cached_name, cached_transform), requested)| {
                cached_name.as_ref() != requested.bone()
                    || *cached_transform != requested.transform()
            });
    if request_changed {
        runtime.override_request.clear();
        runtime
            .override_request
            .extend(pose_overrides.iter().map(|replacement| {
                (
                    Box::<str>::from(replacement.bone()),
                    replacement.transform(),
                )
            }));
        runtime.resolved_overrides.clear();
        for (name, transform) in &runtime.override_request {
            if let Some(id) = runtime.skeleton.asset().bone_id(name) {
                runtime.resolved_overrides.push((id, *transform));
            }
        }
    }
    for (name, _transform) in &runtime.override_request {
        if runtime.skeleton.asset().bone_id(name).is_none() {
            issues.push(ActiveIssue::new(
                SpinalIssueKind::MissingBone,
                format!("procedural bone `{name}` does not exist"),
                root_point,
            ));
        }
    }
}

fn apply_target_intent(
    runtime: &mut SpinalRuntime,
    targets: &SpinalControlTargets,
    root_point: Vec2,
    issues: &mut Vec<ActiveIssue>,
) {
    let names_changed = runtime.target_request.len() != targets.iter().len()
        || runtime.target_request.iter().zip(targets.iter()).any(
            |((cached_name, _cached_position), (name, _position))| cached_name.as_ref() != name,
        );
    let positions_changed = !names_changed
        && runtime.target_request.iter().zip(targets.iter()).any(
            |((_cached_name, cached_position), (_name, position))| *cached_position != position,
        );
    if names_changed {
        runtime.target_request.clear();
        runtime.target_request.extend(
            targets
                .iter()
                .map(|(name, position)| (Box::<str>::from(name), position)),
        );
    } else if positions_changed {
        for ((_, cached_position), (_name, position)) in
            runtime.target_request.iter_mut().zip(targets.iter())
        {
            *cached_position = position;
        }
    }
    if names_changed {
        runtime.resolved_targets.clear();
        for (request_index, (name, _position)) in runtime.target_request.iter().enumerate() {
            if let Some(id) = runtime.skeleton.asset().bone_id(name) {
                runtime.resolved_targets.push(ResolvedControlTarget {
                    request_index,
                    bone: id,
                });
            }
        }
        runtime.resolved_targets.sort_unstable_by_key(|resolved| {
            runtime
                .skeleton
                .asset()
                .bone(resolved.bone)
                .expect("a name-resolved control target belongs to the active asset")
                .ordinal()
        });
    }
    for (name, _position) in &runtime.target_request {
        if runtime.skeleton.asset().bone_id(name).is_none() {
            issues.push(ActiveIssue::new(
                SpinalIssueKind::MissingBone,
                format!("control target bone `{name}` does not exist"),
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
) -> bool {
    {
        let mut base = runtime.mixer.base_track_mut();
        base.set_paused(animator.is_paused());
        base.set_speed(animator.speed())
            .expect("SpinalAnimator validates playback speed");
    }
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

    let playback_intent_matches = runtime
        .animation_intent
        .as_ref()
        .is_some_and(|cached| cached.matches(animator));
    if !playback_intent_matches {
        // A seek cache belongs to the playback declaration it was applied to.
        // Independently constructed animator components can legitimately use
        // the same local seek generation for a different animation.
        runtime.animation_seek = None;
        match (desired_id, animator.mode()) {
            (Some(animation), Some(mode)) => {
                let options = match mode {
                    PlaybackMode::Once => PlayOptions::once(),
                    PlaybackMode::Loop => PlayOptions::looping(),
                    _other => PlayOptions::once(),
                }
                .with_transition(animator.transition());
                runtime
                    .mixer
                    .base_track_mut()
                    .play(animation, options)
                    .expect("resolved animation IDs belong to the active player");
            }
            (None, None) => {
                runtime.mixer.base_track_mut().stop(animator.transition());
            }
            (None, Some(_mode)) => {
                runtime
                    .mixer
                    .base_track_mut()
                    .stop(spinal::Transition::Immediate);
            }
            (Some(_animation), None) => {
                runtime
                    .mixer
                    .base_track_mut()
                    .stop(spinal::Transition::Immediate);
            }
        }
        runtime.animation_intent = Some(CachedAnimationIntent::from_animator(animator));
    }

    if runtime
        .animation_seek
        .is_some_and(|cached| cached.matches(animator))
    {
        return false;
    }

    let seek_applied = animator
        .seek_position()
        .is_some_and(|position| runtime.mixer.base_track_mut().seek_to(position).is_some());
    runtime.animation_seek = Some(CachedAnimationSeek::from_animator(animator));
    if seek_applied {
        // A seek is sampled with a zero playback delta. This guarantees the
        // requested clock position is observable for one frame even when the
        // declarative animator is otherwise running. Crossfade wall time is
        // intentionally unaffected by the temporary pause.
        runtime.mixer.base_track_mut().set_paused(true);
    }
    seek_applied
}

fn apply_track_intent(
    runtime: &mut SpinalRuntime,
    tracks: &SpinalAnimationTracks,
    root_point: Vec2,
    issues: &mut Vec<ActiveIssue>,
) {
    let SpinalRuntime {
        skeleton,
        mixer,
        track_intents,
        track_weight_seeds,
        ..
    } = runtime;

    let mut index = 0;
    while index < track_intents.len() {
        if tracks.intents().iter().any(|intent| {
            Arc::ptr_eq(tracks.namespace(), &track_intents[index].namespace)
                && intent.key == track_intents[index].key
                && intent.incarnation == track_intents[index].incarnation
        }) {
            index += 1;
            continue;
        }
        let removed = track_intents.remove(index);
        mixer
            .remove_track(removed.track)
            .expect("cached track IDs belong to the active mixer");
    }

    for (intent_index, intent) in tracks.intents().iter().enumerate() {
        let cache_index = track_intents
            .iter()
            .position(|cached| {
                Arc::ptr_eq(tracks.namespace(), &cached.namespace)
                    && cached.key == intent.key
                    && cached.incarnation == intent.incarnation
            })
            .unwrap_or_else(|| {
                let initial_weight = intent.weight_fade.map_or(intent.weight, |_fade| {
                    track_weight_seeds
                        .iter()
                        .find(|seed| {
                            Arc::ptr_eq(tracks.namespace(), &seed.namespace)
                                && seed.key == intent.key
                                && seed.incarnation == intent.incarnation
                        })
                        .map_or_else(
                            || intent.weight_fade_source.unwrap_or(intent.weight),
                            |seed| seed.weight,
                        )
                });
                let track = mixer
                    .insert_track(TrackOptions::override_track().with_weight(initial_weight))
                    .expect("a Bevy instance cannot exhaust core track identities");
                track_intents.push(CachedTrackIntent {
                    key: intent.key.clone(),
                    namespace: Arc::clone(tracks.namespace()),
                    incarnation: intent.incarnation,
                    track,
                    play_revision: None,
                    desired: None,
                    stop_transition: spinal::Transition::Immediate,
                    weight_revision: None,
                    declared_weight: initial_weight,
                    weight_fade: None,
                });
                track_intents.len() - 1
            });
        if cache_index != intent_index {
            let cached = track_intents.remove(cache_index);
            track_intents.insert(intent_index, cached);
        }
        let cached = &mut track_intents[intent_index];
        mixer
            .move_track(cached.track, intent_index)
            .expect("cached track ordering belongs to the active mixer");
        let desired_id = intent
            .desired
            .as_ref()
            .and_then(|desired| skeleton.asset().animation_id(&desired.animation));
        if let Some(desired) = &intent.desired
            && desired_id.is_none()
        {
            issues.push(
                ActiveIssue::new(
                    SpinalIssueKind::MissingAnimation,
                    format!(
                        "animation `{}` for override track `{}` does not exist",
                        desired.animation, intent.key
                    ),
                    root_point,
                )
                .with_track(Some(&intent.key)),
            );
        }

        let mut track = mixer
            .track_mut(cached.track)
            .expect("cached track IDs belong to the active mixer");
        track.set_paused(intent.paused);
        track
            .set_speed(intent.speed)
            .expect("SpinalAnimationTracks validates playback speed");

        let weight_revision_changed = cached.weight_revision != Some(intent.weight_revision);
        let fade_command_changed = weight_revision_changed
            || cached.declared_weight != intent.weight
            || cached.weight_fade != intent.weight_fade;
        if weight_revision_changed
            || cached.declared_weight != intent.weight
            || cached.weight_fade != intent.weight_fade
        {
            if fade_command_changed && let Some(fade) = intent.weight_fade {
                track.fade_weight(intent.weight, fade);
            } else {
                track.set_weight(intent.weight);
            }
            cached.weight_revision = Some(intent.weight_revision);
            cached.declared_weight = intent.weight;
            cached.weight_fade = intent.weight_fade;
        }

        if cached.play_revision == Some(intent.play_revision)
            && cached.desired == intent.desired
            && cached.stop_transition == intent.stop_transition
        {
            continue;
        }
        match (&intent.desired, desired_id) {
            (Some(desired), Some(animation)) => {
                let options = match desired.mode {
                    PlaybackMode::Once => PlayOptions::once(),
                    PlaybackMode::Loop => PlayOptions::looping(),
                    _other => PlayOptions::once(),
                }
                .with_transition(desired.transition);
                track
                    .play(animation, options)
                    .expect("resolved animation IDs belong to the active mixer");
            }
            (None, None) => {
                track.stop(intent.stop_transition);
            }
            (Some(_desired), None) => {
                track.stop(spinal::Transition::Immediate);
            }
            (None, Some(_animation)) => {
                track.stop(spinal::Transition::Immediate);
            }
        }
        cached.play_revision = Some(intent.play_revision);
        cached.desired.clone_from(&intent.desired);
        cached.stop_transition = intent.stop_transition;
    }
    track_weight_seeds.clear();
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
    for (constraint, status) in frame.transform_statuses() {
        if let Some(issue) = status.issue() {
            let point = frame
                .asset()
                .transform_constraint(constraint)
                .ok()
                .and_then(|constraint| frame.bone(constraint.source()).ok())
                .map_or(Vec2::ZERO, |bone| bone.world_transform().translation());
            issues.push(ActiveIssue::new(
                SpinalIssueKind::RuntimeTransform(issue),
                "transform constraint preserved the finite unconstrained rotation because source geometry was unsafe",
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
    output.vertices.clear();
    output.indices.clear();
    let mut ready = true;
    for draw in solved.draw_items() {
        let (page_id, region_id, slot_id, blend_mode, color) = match draw {
            DrawItemRef::Region(region) => (
                region.atlas_page(),
                region.atlas_region(),
                region.slot(),
                region.blend_mode(),
                region.color(),
            ),
            DrawItemRef::Mesh(mesh) => (
                mesh.atlas_page(),
                mesh.atlas_region(),
                mesh.slot(),
                mesh.blend_mode(),
                mesh.color(),
            ),
            _future => continue,
        };
        let page = solved
            .asset()
            .atlas_page(page_id)
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
            .atlas_region(region_id)
            .expect("draw region IDs belong to the solved asset");
        if page.alpha_encoding() != spinal::AlphaEncoding::Straight {
            continue;
        }
        if blend_mode != SlotBlendMode::Normal {
            let slot_point = solved
                .asset()
                .slot(slot_id)
                .ok()
                .and_then(|slot| solved.bone(slot.bone()).ok())
                .map_or(Vec2::ZERO, |bone| bone.world_transform().translation());
            issues.push(ActiveIssue::new(
                SpinalIssueKind::UnsupportedBlendMode(blend_mode),
                format!(
                    "slot blend mode `{}` is outside the renderer profile; the slot was omitted",
                    solved
                        .asset()
                        .slot(slot_id)
                        .map_or("unknown", |slot| slot.blend_token())
                ),
                slot_point,
            ));
            continue;
        }

        let vertex_start = output.vertices.len();
        match draw {
            DrawItemRef::Region(region) => {
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
                output.vertices.extend(
                    region
                        .positions()
                        .into_iter()
                        .zip(uvs)
                        .map(|(position, uv)| SpinalVertex { position, uv }),
                );
            }
            DrawItemRef::Mesh(mesh) => {
                if let Some(uvs) = mesh.uvs() {
                    output.vertices.extend(
                        mesh.positions()
                            .iter()
                            .copied()
                            .zip(uvs)
                            .map(|(position, uv)| SpinalVertex { position, uv }),
                    );
                } else {
                    let Some(corners) = normalized_uvs(
                        image.width(),
                        image.height(),
                        atlas_region.bounds(),
                        atlas_region.rotation().as_degrees(),
                    ) else {
                        continue;
                    };
                    output.vertices.extend(
                        mesh.positions()
                            .iter()
                            .copied()
                            .zip(mesh.source_uvs().iter().copied())
                            .map(|(position, uv)| SpinalVertex {
                                position,
                                uv: map_mesh_uv(
                                    uv,
                                    corners,
                                    atlas_region.bounds(),
                                    atlas_region.trim(),
                                ),
                            }),
                    );
                }
            }
            _future => continue,
        }
        let vertex_end = output.vertices.len();
        let base_vertex = u32::try_from(vertex_start)
            .expect("one Spinal frame cannot contain more than u32::MAX vertices");
        let index_start = output.indices.len();
        match draw {
            DrawItemRef::Region(_region) => {
                output.indices.extend(
                    [0_u32, 1, 2, 0, 2, 3]
                        .into_iter()
                        .map(|index| base_vertex + index),
                );
            }
            DrawItemRef::Mesh(mesh) => output.indices.extend(
                mesh.triangles()
                    .iter()
                    .copied()
                    .map(|index| base_vertex + index),
            ),
            _future => continue,
        }
        let index_end = output.indices.len();
        output.draws.push(SpinalDraw {
            page_ordinal: page.ordinal(),
            vertices: vertex_start..vertex_end,
            indices: index_start..index_end,
            color: color.to_array(),
        });
    }
    if !ready {
        output.draws.clear();
        output.vertices.clear();
        output.indices.clear();
    }
    ready
}

fn map_mesh_uv(
    source: Vec2,
    corners: [Vec2; 4],
    bounds: spinal::PixelRect,
    trim: spinal::Trim,
) -> Vec2 {
    let original = trim.original_size();
    let trimmed_top = original.height() - trim.bottom() - bounds.height();
    let packed = Vec2::new(
        (source.x * original.width() as f32 - trim.left() as f32) / bounds.width() as f32,
        (source.y * original.height() as f32 - trimmed_top as f32) / bounds.height() as f32,
    );
    let [bottom_left, top_left, top_right, bottom_right] = corners;
    let bottom = bottom_left.lerp(bottom_right, packed.x);
    let top = top_left.lerp(top_right, packed.x);
    top.lerp(bottom, packed.y)
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
        DiagnosticScope::Constraint(constraint) => {
            asset.constraint(constraint).ok().and_then(|constraint| {
                constraint
                    .as_ik()
                    .map(|constraint| constraint.target())
                    .or_else(|| {
                        constraint
                            .as_transform()
                            .map(|constraint| constraint.source())
                    })
            })
        }
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

fn write_playback_observation(
    current: &mut SpinalPlaybackState,
    asset: &spinal::SkeletonAsset,
    status: spinal::PlayerStatus,
) -> bool {
    if let Some(animation) = status.animation() {
        let name = asset
            .animation(animation)
            .map_or("<invalid>", |animation| animation.name());
        let next_playback = status
            .playback()
            .expect("an active animation has a playback ID")
            .get()
            .get();
        let next_mode = status
            .mode()
            .expect("an active animation has a playback mode");
        let next_position = status
            .position()
            .expect("an active animation has a local position");
        let next_loop_index = status
            .loop_index()
            .expect("an active animation has a loop index");
        let next_complete = status.is_complete();
        let next_transition_mix = status.transition_mix();
        match &mut *current {
            SpinalPlaybackState::Playing {
                playback,
                animation,
                mode,
                position,
                loop_index,
                complete,
                transition_mix,
            } if animation.as_ref() == name => {
                let changed = *playback != next_playback
                    || *mode != next_mode
                    || *position != next_position
                    || *loop_index != next_loop_index
                    || *complete != next_complete
                    || *transition_mix != next_transition_mix;
                if changed {
                    *playback = next_playback;
                    *mode = next_mode;
                    *position = next_position;
                    *loop_index = next_loop_index;
                    *complete = next_complete;
                    *transition_mix = next_transition_mix;
                }
                changed
            }
            _other => {
                *current = SpinalPlaybackState::Playing {
                    playback: next_playback,
                    animation: name.into(),
                    mode: next_mode,
                    position: next_position,
                    loop_index: next_loop_index,
                    complete: next_complete,
                    transition_mix: next_transition_mix,
                };
                true
            }
        }
    } else if status.is_stopping() {
        let next_transition_mix = status.transition_mix();
        match &mut *current {
            SpinalPlaybackState::Stopping { transition_mix } => {
                if *transition_mix == next_transition_mix {
                    false
                } else {
                    *transition_mix = next_transition_mix;
                    true
                }
            }
            _other => {
                *current = SpinalPlaybackState::Stopping {
                    transition_mix: next_transition_mix,
                };
                true
            }
        }
    } else {
        if matches!(current, SpinalPlaybackState::Idle) {
            false
        } else {
            *current = SpinalPlaybackState::Idle;
            true
        }
    }
}

fn write_track_observations(
    current: &mut SpinalTrackStates,
    asset: &spinal::SkeletonAsset,
    mixer: &AnimationMixer,
    intents: &[CachedTrackIntent],
) -> bool {
    let keys_changed = current.states.len() != intents.len()
        || current
            .states
            .iter()
            .zip(intents)
            .any(|(state, intent)| state.key != intent.key);
    if keys_changed {
        current.states.clear();
        current
            .states
            .extend(intents.iter().map(|intent| SpinalTrackState {
                key: intent.key.clone(),
                playback: SpinalPlaybackState::Idle,
                weight: Mix::ONE,
                target_weight: Mix::ONE,
                weight_fading: false,
                paused: false,
                speed: 1.0,
            }));
    }

    let mut changed = keys_changed;
    for (state, intent) in current.states.iter_mut().zip(intents) {
        let track = mixer
            .track(intent.track)
            .expect("cached track IDs belong to the active mixer");
        changed |= write_playback_observation(&mut state.playback, asset, track.status());
        let weight = track.weight();
        let target_weight = track.target_weight();
        let weight_fading = track.is_weight_fading();
        let paused = track.is_paused();
        let speed = track.speed();
        if state.weight != weight
            || state.target_weight != target_weight
            || state.weight_fading != weight_fading
            || state.paused != paused
            || state.speed != speed
        {
            state.weight = weight;
            state.target_weight = target_weight;
            state.weight_fading = weight_fading;
            state.paused = paused;
            state.speed = speed;
            changed = true;
        }
    }
    changed
}

fn owned_event(
    entity: Entity,
    track: Option<&str>,
    animation_name: &str,
    event: AnimationEvent<'_>,
) -> SpinalAnimationEvent {
    SpinalAnimationEvent {
        entity,
        track: track.map(Into::into),
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

#[cfg(test)]
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
                track: issue.fingerprint.track.clone(),
                kind: issue.fingerprint.kind,
                message: issue.fingerprint.message.clone(),
            });
        }
    }
    previous.clear();
    previous.extend(current.iter().map(|issue| issue.fingerprint.clone()));
}

#[cfg(test)]
mod tests {
    use bevy::{
        asset::{AssetPlugin, Assets},
        ecs::system::{IntoSystem, System},
        image::Image,
        prelude::{App, MinimalPlugins},
    };
    use spinal::{Angle, BoneTransform, Shear, load_json};

    use crate::{
        BoneOverride, SpinalAnimationTracks, SpinalAnimator, SpinalAsset, SpinalAtlasPage,
        SpinalControlTargets, SpinalInstance, SpinalInstanceState, SpinalPlugin,
        SpinalPoseOverrides, SpinalSkinLayers,
    };

    use super::*;

    #[test]
    fn related_control_targets_apply_parent_before_child_regardless_of_insertion_order() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), SpinalPlugin));
        let skeleton = load_json(
            br#"{
              "skeleton":{"spine":"4.3.23"},
              "bones":[
                {"name":"root"},
                {"name":"child","parent":"root"}
              ],
              "slots":[{"name":"body","bone":"child","attachment":"body"}],
              "skins":[{
                "name":"default",
                "attachments":{"body":{"body":{"width":2,"height":2}}}
              }]
            }"#,
            b"cat.png\n\tsize:1,1\nbody\n\tbounds:0,0,1,1\n",
        )
        .expect("the related-target fixture is valid")
        .into_asset();
        let image = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::default());
        let asset = SpinalAsset::new(skeleton, vec![SpinalAtlasPage::new("cat.png", image)])
            .expect("the manual image matches the atlas");
        let handle = app
            .world_mut()
            .resource_mut::<Assets<SpinalAsset>>()
            .add(asset);
        let mut targets = SpinalControlTargets::default();
        targets
            .set_skeleton_position("child", Vec2::new(10.0, 0.0))
            .expect("the child target is finite");
        targets
            .set_skeleton_position("root", Vec2::new(5.0, 0.0))
            .expect("the root target is finite");
        let entity = app
            .world_mut()
            .spawn((SpinalInstance::new(handle), targets))
            .id();

        app.update();
        app.update();

        let frame = app
            .world()
            .entity(entity)
            .get::<SpinalFrame>()
            .expect("the adapter produced an owned frame");
        assert_eq!(frame.draws.len(), 1);
        let draw = &frame.draws[0];
        assert_eq!(
            frame.vertices[draw.vertices.clone()]
                .iter()
                .map(|vertex| vertex.position.x)
                .collect::<Vec<_>>(),
            [9.0, 9.0, 11.0, 11.0],
            "both final bone origins must match their skeleton-space destinations"
        );
    }

    #[test]
    fn ecs_frame_preserves_arbitrary_weighted_mesh_vertices_and_indices() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), SpinalPlugin));
        let skeleton = load_json(
            br#"{
              "skeleton":{"spine":"4.3.23"},
              "bones":[
                {"name":"root"},
                {"name":"left","parent":"root","x":10},
                {"name":"right","parent":"root","x":30}
              ],
              "slots":[{"name":"body-slot","bone":"root","attachment":"body"}],
              "skins":[{"name":"default","attachments":{"body-slot":{"body":{
                "type":"mesh",
                "uvs":[0,0,0.5,0,1,1,0,1,0.5,1],
                "triangles":[0,1,4,0,4,3,1,2,4],
                "vertices":[
                  1,1,-10,0,1,
                  2,1,0,0,0.5,2,-20,0,0.5,
                  1,2,-20,10,1,
                  1,1,-10,10,1,
                  2,1,0,10,0.5,2,-20,10,0.5
                ],
                "hull":4
              }}}}]
            }"#,
            b"cat.png\n\tsize: 100, 100\nbody\n\tbounds: 10, 20, 40, 20\n",
        )
        .expect("the weighted adapter fixture is valid")
        .into_asset();
        let image = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::default());
        let asset = SpinalAsset::new(skeleton, vec![SpinalAtlasPage::new("cat.png", image)])
            .expect("the manual image matches the atlas");
        let handle = app
            .world_mut()
            .resource_mut::<Assets<SpinalAsset>>()
            .add(asset);
        let mut targets = SpinalControlTargets::default();
        targets
            .set_skeleton_position("right", Vec2::new(40.0, 0.0))
            .expect("the weighted bone target is finite");
        let entity = app
            .world_mut()
            .spawn((SpinalInstance::new(handle), targets))
            .id();

        app.update();
        app.update();

        assert_eq!(
            app.world().entity(entity).get::<SpinalInstanceState>(),
            Some(&SpinalInstanceState::Ready)
        );
        let frame = app
            .world()
            .entity(entity)
            .get::<SpinalFrame>()
            .expect("the adapter produced an indexed frame");
        assert!(frame.ready);
        assert_eq!(frame.draws.len(), 1);
        assert_eq!(frame.draws[0].vertices, 0..5);
        assert_eq!(frame.draws[0].indices, 0..9);
        for (actual, expected) in frame.vertices.iter().map(|vertex| vertex.position).zip([
            Vec2::new(0.0, 0.0),
            Vec2::new(15.0, 0.0),
            Vec2::new(20.0, 10.0),
            Vec2::new(0.0, 10.0),
            Vec2::new(15.0, 10.0),
        ]) {
            assert!((actual - expected).length() < 1.0e-5);
        }
        assert_eq!(frame.indices, [0, 1, 4, 0, 4, 3, 1, 2, 4]);

        let mut animate = IntoSystem::into_system(update_instances);
        animate.initialize(app.world_mut());
        animate
            .run((), app.world_mut())
            .expect("the weighted adapter path warms successfully");
        let allocations = allocation_counter::measure(|| {
            app.world_mut()
                .entity_mut(entity)
                .get_mut::<SpinalControlTargets>()
                .expect("the weighted target remains present")
                .set_skeleton_position("right", Vec2::new(41.0, 1.0))
                .expect("the moving weighted target remains finite");
            animate
                .run((), app.world_mut())
                .expect("the weighted adapter path remains valid");
        });
        assert_eq!(allocations.count_total, 0);
        assert_eq!(allocations.bytes_total, 0);
    }

    #[test]
    fn mesh_uvs_use_top_left_image_origin_with_trim_and_rotation() {
        let bounds = spinal::PixelRect::new(10, 20, 40, 20);
        let trim = spinal::Trim::new(10, 5, 80, 40);
        let corners = normalized_uvs(200, 100, bounds, 90.0)
            .expect("the page dimensions and quarter-turn are supported");

        assert_eq!(
            map_mesh_uv(Vec2::new(10.0 / 80.0, 15.0 / 40.0), corners, bounds, trim),
            Vec2::new(0.05, 0.6)
        );
        assert_eq!(
            map_mesh_uv(Vec2::new(50.0 / 80.0, 35.0 / 40.0), corners, bounds, trim),
            Vec2::new(0.15, 0.2)
        );
    }

    #[test]
    fn ecs_skin_override_and_observation_are_allocation_free_after_warmup() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), SpinalPlugin));
        let skeleton = load_json(
            br#"{
              "skeleton":{"spine":"4.3.23"},
              "bones":[
                {"name":"root"},
                {"name":"crosshair","parent":"root"}
              ],
              "slots":[{"name":"body","bone":"root","attachment":"body"}],
              "skins":[
                {
                  "name":"default",
                  "attachments":{
                    "body":{"body":{"width":2,"height":2}}
                  }
                },
                {
                  "name":"item/shift",
                  "attachments":{
                    "body":{"body":{"x":4,"width":2,"height":2}}
                  }
                }
              ],
              "animations":{
                "idle":{"bones":{"root":{"rotate":[{"value":0},{"time":1,"value":5}]}}},
                "aim":{"bones":{"root":{"translate":[{"x":0,"y":0},{"time":1,"x":2,"y":0}]}}}
              }
            }"#,
            b"cat.png\n\tsize:1,1\nbody\n\tbounds:0,0,1,1\n",
        )
        .expect("the adapter intent fixture is valid")
        .into_asset();
        let image = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::default());
        let asset = SpinalAsset::new(skeleton, vec![SpinalAtlasPage::new("cat.png", image)])
            .expect("the manual image matches the atlas");
        let handle = app
            .world_mut()
            .resource_mut::<Assets<SpinalAsset>>()
            .add(asset);
        let mut overrides = SpinalPoseOverrides::default();
        overrides.set(BoneOverride::new(
            "root",
            BoneTransform::new(Vec2::new(3.0, 0.0), Angle::ZERO, Vec2::ONE, Shear::ZERO)
                .expect("the test override is finite"),
        ));
        let mut tracks = SpinalAnimationTracks::default();
        tracks.play(
            "aim",
            "aim",
            PlaybackMode::Loop,
            spinal::Transition::Immediate,
        );
        let mut targets = SpinalControlTargets::default();
        targets
            .set_skeleton_position("crosshair", Vec2::new(8.0, 2.0))
            .expect("the warmup target is finite");
        let entity = app
            .world_mut()
            .spawn((
                SpinalInstance::new(handle),
                SpinalAnimator::looping("idle"),
                tracks,
                SpinalSkinLayers::new(["item/shift"]),
                overrides,
                targets,
            ))
            .id();

        app.update();
        app.update();

        assert_eq!(
            app.world().entity(entity).get::<SpinalInstanceState>(),
            Some(&SpinalInstanceState::Ready)
        );
        let frame = app
            .world()
            .entity(entity)
            .get::<SpinalFrame>()
            .expect("the adapter produced an owned frame");
        assert!(frame.ready);
        assert_eq!(frame.draws.len(), 1);
        let xs = frame.vertices[frame.draws[0].vertices.clone()]
            .iter()
            .map(|vertex| vertex.position.x)
            .collect::<Vec<_>>();
        assert_eq!(
            xs,
            [6.0, 6.0, 8.0, 8.0],
            "skin attachment x=4 and root override x=3 must both reach the solved draw"
        );

        let mut animate = IntoSystem::into_system(update_instances);
        animate.initialize(app.world_mut());
        animate
            .run((), app.world_mut())
            .expect("the isolated adapter animation system warms successfully");
        let allocations = allocation_counter::measure(|| {
            app.world_mut()
                .entity_mut(entity)
                .get_mut::<SpinalControlTargets>()
                .expect("the target component remains present")
                .set_skeleton_position("crosshair", Vec2::new(9.0, 3.0))
                .expect("the moving target remains finite");
            animate
                .run((), app.world_mut())
                .expect("the isolated adapter animation system remains valid");
        });
        assert_eq!(
            allocations.count_total, 0,
            "an unchanged adapter instance must not allocate in steady state"
        );
        assert_eq!(allocations.bytes_total, 0);
    }

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

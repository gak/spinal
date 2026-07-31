//! Public API contract tests for the Bevy adapter.

use std::time::Duration;

use bevy::{
    asset::uuid::Uuid,
    color::Color,
    prelude::{GlobalTransform, Handle, Quat, Transform, Vec2, Vec3, World},
};
use bevy_spinal::{
    BoneOverride, SpinalAnimationTracks, SpinalAnimator, SpinalAppearance, SpinalAsset,
    SpinalControlTargets, SpinalInstance, SpinalPoseOverrides, SpinalSkinLayers, SpinalTrackStates,
    TrackReorderError, WorldToSkeletonPositionError,
    spinal::{BoneTransform, Mix, PlaybackMode, Transition, WeightFade},
};

#[test]
fn animation_intent_is_declarative_and_restartable() {
    let mut animator = SpinalAnimator::looping("idle");
    assert_eq!(animator.animation(), Some("idle"));
    assert_eq!(animator.mode(), Some(PlaybackMode::Loop));
    assert!(!animator.is_paused());
    assert_eq!(animator.speed(), 1.0);

    let original_revision = animator.revision();
    animator.play(
        "eat",
        PlaybackMode::Once,
        Transition::Crossfade(bevy_spinal::spinal::Crossfade::new(Duration::from_millis(
            120,
        ))),
    );
    assert_eq!(animator.animation(), Some("eat"));
    assert_eq!(animator.mode(), Some(PlaybackMode::Once));
    assert!(animator.revision() > original_revision);

    let play_revision = animator.revision();
    animator.restart();
    assert!(animator.revision() > play_revision);

    animator.stop(Transition::Immediate);
    assert_eq!(animator.animation(), None);
}

#[test]
fn named_override_track_intent_separates_commands_from_idempotent_state() {
    let mut tracks = SpinalAnimationTracks::default();
    tracks.play("aim", "look", PlaybackMode::Loop, Transition::Immediate);
    tracks.set_paused("aim", true);
    tracks.set_speed("aim", 0.5).expect("half speed is valid");
    tracks.set_weight("aim", Mix::ZERO);
    tracks.fade_weight("aim", Mix::ONE, WeightFade::new(Duration::from_millis(120)));
    tracks.restart("aim");
    tracks.play(
        "expression",
        "blink",
        PlaybackMode::Loop,
        Transition::Immediate,
    );
    assert_eq!(tracks.keys().collect::<Vec<_>>(), ["aim", "expression"]);
    tracks
        .move_to("expression", 0)
        .expect("a declared track can move without replay");
    assert_eq!(tracks.keys().collect::<Vec<_>>(), ["expression", "aim"]);
    let aim = tracks.get("aim").expect("aim intent remains inspectable");
    assert_eq!(aim.key(), "aim");
    assert_eq!(aim.animation(), Some("look"));
    assert_eq!(aim.mode(), Some(PlaybackMode::Loop));
    assert!(aim.is_paused());
    assert_eq!(aim.speed(), 0.5);
    assert_eq!(aim.target_weight(), Mix::ONE);
    assert_eq!(
        aim.weight_fade().map(|fade| fade.duration()),
        Some(Duration::from_millis(120))
    );
    assert_eq!(tracks.len(), 2);
    assert!(!tracks.is_empty());
    assert_eq!(
        tracks
            .move_to("missing", 0)
            .expect_err("unknown stable names are rejected"),
        TrackReorderError::UnknownTrack
    );
    assert!(matches!(
        tracks
            .move_to("aim", 2)
            .expect_err("the end-exclusive index is invalid"),
        TrackReorderError::IndexOutOfBounds { index: 2, len: 2 }
    ));
    assert!(tracks.remove("aim"));
    assert_eq!(tracks.keys().collect::<Vec<_>>(), ["expression"]);
    tracks.clear();
    assert!(tracks.is_empty());
}

#[test]
fn named_track_equality_distinguishes_lineages_and_divergent_clones() {
    let mut original = SpinalAnimationTracks::default();
    original.play("aim", "look", PlaybackMode::Loop, Transition::Immediate);
    let clone = original.clone();
    assert_eq!(original, clone);

    let mut independent = SpinalAnimationTracks::default();
    independent.play("aim", "look", PlaybackMode::Loop, Transition::Immediate);
    assert_ne!(original, independent);

    let mut left = original.clone();
    let mut right = original.clone();
    assert!(left.remove("aim"));
    left.play("aim", "look", PlaybackMode::Loop, Transition::Immediate);
    assert!(right.remove("aim"));
    right.play("aim", "look", PlaybackMode::Loop, Transition::Immediate);
    assert_ne!(left, right);
}

#[test]
fn skeleton_space_control_targets_replace_by_stable_name_and_reject_nonfinite_input() {
    let mut targets = SpinalControlTargets::default();
    targets
        .set_skeleton_position("crosshair", bevy::math::Vec2::new(2.0, 3.0))
        .expect("finite positions are accepted");
    targets
        .set_skeleton_position("crosshair", bevy::math::Vec2::new(5.0, 7.0))
        .expect("the same stable name replaces in place");
    assert_eq!(
        targets.iter().collect::<Vec<_>>(),
        [("crosshair", bevy::math::Vec2::new(5.0, 7.0))]
    );
    assert!(
        targets
            .set_skeleton_position("crosshair", bevy::math::Vec2::new(f32::NAN, 0.0))
            .is_err()
    );
    assert_eq!(
        targets.get("crosshair"),
        Some(bevy::math::Vec2::new(5.0, 7.0)),
        "invalid replacement is failure-atomic"
    );
}

#[test]
fn skin_layers_preserve_caller_priority_order() {
    let mut skins = SpinalSkinLayers::new(["breed/tuxedo", "item/hat/beret"]);
    assert_eq!(
        skins.iter().collect::<Vec<_>>(),
        ["breed/tuxedo", "item/hat/beret"]
    );

    skins.set(["breed/calico", "item/collar/bow"]);
    assert_eq!(
        skins.iter().collect::<Vec<_>>(),
        ["breed/calico", "item/collar/bow"]
    );
}

#[test]
fn bone_overrides_replace_by_stable_name() {
    let mut overrides = SpinalPoseOverrides::default();
    overrides.set(BoneOverride::new("look_target", BoneTransform::IDENTITY));
    overrides.set(BoneOverride::new("look_target", BoneTransform::IDENTITY));

    assert_eq!(overrides.iter().count(), 1);
    assert_eq!(
        overrides.get("look_target").map(BoneOverride::transform),
        Some(BoneTransform::IDENTITY)
    );
    assert!(overrides.remove("look_target").is_some());
}

#[test]
fn instance_accepts_a_typed_asset_handle() {
    let handle = Handle::<SpinalAsset>::from(Uuid::from_u128(7));
    let instance = SpinalInstance::new(handle.clone());
    assert_eq!(instance.asset(), &handle);
}

#[test]
fn appearance_is_required_and_supports_modulation_and_local_facing() {
    let handle = Handle::<SpinalAsset>::from(Uuid::from_u128(8));
    let mut world = World::new();
    let entity = world.spawn(SpinalInstance::new(handle)).id();

    let required = world
        .entity(entity)
        .get::<SpinalAppearance>()
        .expect("SpinalInstance inserts its appearance controls");
    assert_eq!(required.modulation(), Color::WHITE);
    assert!(!required.flip_x());
    assert!(!required.flip_y());
    assert!(
        world
            .entity(entity)
            .get::<SpinalAnimationTracks>()
            .is_some()
    );
    assert!(world.entity(entity).get::<SpinalControlTargets>().is_some());
    assert!(world.entity(entity).get::<SpinalTrackStates>().is_some());

    let appearance = SpinalAppearance::default()
        .with_modulation(Color::srgba(0.8, 0.7, 0.6, 0.5))
        .with_flip_x(true)
        .with_flip_y(true);
    assert_eq!(appearance.modulation(), Color::srgba(0.8, 0.7, 0.6, 0.5));
    assert!(appearance.flip_x());
    assert!(appearance.flip_y());
}

#[test]
fn appearance_converts_world_positions_through_entity_transform_and_local_facing() {
    let transform = GlobalTransform::from(
        Transform::from_xyz(11.0, -7.0, 3.0)
            .with_rotation(Quat::from_rotation_z(0.63))
            .with_scale(Vec3::new(2.5, 0.75, 1.0)),
    );
    let skeleton_position = Vec2::new(4.0, -6.0);

    for (appearance, facing) in [
        (
            SpinalAppearance::default().with_flip_x(true),
            Vec2::new(-1.0, 1.0),
        ),
        (
            SpinalAppearance::default().with_flip_y(true),
            Vec2::new(1.0, -1.0),
        ),
    ] {
        let world = transform
            .transform_point((skeleton_position * facing).extend(0.0))
            .truncate();
        let recovered = appearance
            .world_to_skeleton_position(&transform, world)
            .expect("the finite nonsingular transform is invertible");
        assert!((recovered.x - skeleton_position.x).abs() < 1.0e-5);
        assert!((recovered.y - skeleton_position.y).abs() < 1.0e-5);
    }
}

#[test]
fn appearance_rejects_singular_or_nonfinite_world_position_conversions() {
    let appearance = SpinalAppearance::default();
    let singular = GlobalTransform::from(Transform::from_scale(Vec3::new(0.0, 2.0, 1.0)));
    assert_eq!(
        appearance.world_to_skeleton_position(&singular, Vec2::ZERO),
        Err(WorldToSkeletonPositionError::InvalidEntityTransform)
    );
    let nonfinite_position = Vec2::new(f32::NAN, 0.0);
    assert_eq!(
        appearance.world_to_skeleton_position(&GlobalTransform::IDENTITY, nonfinite_position),
        Err(WorldToSkeletonPositionError::NonFiniteWorldPosition)
    );
    let nonfinite = GlobalTransform::from(Transform::from_translation(Vec3::new(
        f32::INFINITY,
        0.0,
        0.0,
    )));
    assert_eq!(
        appearance.world_to_skeleton_position(&nonfinite, Vec2::ZERO),
        Err(WorldToSkeletonPositionError::InvalidEntityTransform)
    );
}

//! Public API contract tests for the Bevy adapter.

use std::time::Duration;

use bevy::{
    asset::uuid::Uuid,
    color::Color,
    prelude::{Handle, World},
};
use bevy_spinal::{
    BoneOverride, SpinalAnimator, SpinalAppearance, SpinalAsset, SpinalInstance,
    SpinalPoseOverrides, SpinalSkinLayers,
    spinal::{BoneTransform, PlaybackMode, Transition},
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

    let appearance = SpinalAppearance::default()
        .with_modulation(Color::srgba(0.8, 0.7, 0.6, 0.5))
        .with_flip_x(true)
        .with_flip_y(true);
    assert_eq!(appearance.modulation(), Color::srgba(0.8, 0.7, 0.6, 0.5));
    assert!(appearance.flip_x());
    assert!(appearance.flip_y());
}

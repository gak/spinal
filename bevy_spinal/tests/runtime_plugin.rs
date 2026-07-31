//! Headless ECS integration tests for playback, intent, diagnostics, and hot reload.

use std::{sync::Arc, time::Duration};

use bevy::{
    asset::{AssetPlugin, Assets, Handle, uuid::Uuid},
    ecs::message::Messages,
    image::Image,
    prelude::{App, MinimalPlugins},
    time::TimeUpdateStrategy,
};
use bevy_spinal::{
    BoneOverride, SpinalAnimationEvent, SpinalAnimationTracks, SpinalAnimator, SpinalAsset,
    SpinalAtlasPage, SpinalControlTargets, SpinalInstance, SpinalInstanceState, SpinalIssue,
    SpinalIssueKind, SpinalPlaybackState, SpinalPlugin, SpinalPoseOverrides, SpinalSkinLayers,
    SpinalTrackStates,
    spinal::{DiagnosticCode, Mix, SlotBlendMode, WeightFade, glam::Vec2},
};
use spinal::{Angle, BoneTransform, Crossfade, PlaybackMode, Shear, Transition, load_json};

const ATLAS: &[u8] = b"cat.png\n\tsize: 1, 1\nbody\n\tbounds: 0, 0, 1, 1\n";
const PREMULTIPLIED_ATLAS: &[u8] =
    b"cat.png\n\tsize: 1, 1\n\tpma: true\nbody\n\tbounds: 0, 0, 1, 1\n";

const JSON: &[u8] = br#"{
  "skeleton": { "spine": "4.3.23" },
  "bones": [
    { "name": "root" },
    { "name": "look", "parent": "root" }
  ],
  "slots": [{ "name": "body", "bone": "root", "attachment": "body" }],
  "skins": [
    {
      "name": "default",
      "attachments": {
        "body": {
          "body": { "width": 32, "height": 32 }
        }
      }
    },
    {
      "name": "item/hat/beret",
      "attachments": {}
    }
  ],
  "events": {
    "bite": {
      "int": 1,
      "float": 0.75,
      "string": "nom",
      "volume": 0.4,
      "balance": -0.2
    }
  },
  "animations": {
    "idle": {
      "bones": {
        "root": {
          "translate": [
            { "x": 0, "y": 0 },
            { "time": 1, "x": 1, "y": 0 }
          ]
        }
      }
    },
    "eat": {
      "bones": {
        "root": {
          "rotate": [
            { "value": 0 },
            { "time": 1, "value": 10 }
          ]
        }
      },
      "events": [
        { "time": 0.25, "name": "bite", "int": 2, "string": "crunch" }
      ]
    }
  }
}"#;

const REPLACEMENT_WITHOUT_ITEM_INTENT: &[u8] = br#"{
  "skeleton": { "spine": "4.3.23" },
  "bones": [{ "name": "root" }],
  "slots": [{ "name": "body", "bone": "root", "attachment": "body" }],
  "skins": [{
    "name": "default",
    "attachments": {
      "body": {
        "body": { "width": 32, "height": 32 }
      }
    }
  }],
  "animations": {
    "idle": {},
    "eat": {}
  }
}"#;

const ADDITIVE_JSON: &[u8] = br#"{
  "skeleton": { "spine": "4.3.23" },
  "bones": [{ "name": "root" }],
  "slots": [
    {
      "name": "body",
      "bone": "root",
      "attachment": "body",
      "blend": "additive"
    }
  ],
  "skins": [{
    "name": "default",
    "attachments": {
      "body": {
        "body": { "width": 32, "height": 32 }
      }
    }
  }]
}"#;

#[test]
fn supported_empty_pose_is_usable_but_has_no_drawable_output() {
    let mut app = headless_app();
    let asset_handle = add_asset(
        &mut app,
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[{"name":"root"}]
        }"#,
    );
    let entity = app
        .world_mut()
        .spawn(SpinalInstance::new(asset_handle))
        .id();

    app.update();
    app.update();

    let state = app
        .world()
        .entity(entity)
        .get::<SpinalInstanceState>()
        .expect("state exists");
    assert_eq!(state, &SpinalInstanceState::ReadyNoDraws);
    assert!(state.is_ready());
    assert!(!state.is_degraded());
    assert!(state.is_usable());
    assert!(!state.has_drawable_output());
}

#[test]
fn plugin_applies_name_based_intent_and_recovers_across_asset_replacement() {
    let mut app = headless_app();
    let asset_handle = add_asset(&mut app, JSON);
    let mut overrides = SpinalPoseOverrides::default();
    overrides.set(BoneOverride::new("look", BoneTransform::IDENTITY));
    let mut tracks = SpinalAnimationTracks::default();
    tracks.play("aim", "eat", PlaybackMode::Loop, Transition::Immediate);
    let mut targets = SpinalControlTargets::default();
    targets
        .set_skeleton_position("look", Vec2::new(4.0, 3.0))
        .expect("the target position is finite");
    let entity = app
        .world_mut()
        .spawn((
            SpinalInstance::new(asset_handle.clone()),
            SpinalAnimator::looping("idle"),
            tracks,
            SpinalSkinLayers::new(["item/hat/beret"]),
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

    app.world_mut()
        .entity_mut(entity)
        .get_mut::<SpinalAnimator>()
        .expect("required animator exists")
        .play("eat", PlaybackMode::Once, Transition::Immediate);
    app.update();
    assert_eq!(
        app.world()
            .entity(entity)
            .get::<bevy_spinal::SpinalPlaybackState>()
            .and_then(bevy_spinal::SpinalPlaybackState::animation),
        Some("eat")
    );

    let mut issue_cursor = app
        .world()
        .resource::<Messages<SpinalIssue>>()
        .get_cursor_current();
    let replacement = manual_asset(app.world_mut(), REPLACEMENT_WITHOUT_ITEM_INTENT);
    app.world_mut()
        .resource_mut::<Assets<SpinalAsset>>()
        .insert(asset_handle.id(), replacement)
        .expect("the live asset ID accepts a replacement");
    app.update();
    app.update();

    assert_eq!(
        app.world()
            .entity(entity)
            .get::<bevy_spinal::SpinalPlaybackState>()
            .and_then(bevy_spinal::SpinalPlaybackState::animation),
        Some("eat"),
        "hot reload reapplies declarative playback by name"
    );
    assert_eq!(
        app.world()
            .entity(entity)
            .get::<SpinalTrackStates>()
            .and_then(|tracks| tracks.get("aim"))
            .and_then(|track| track.playback().animation()),
        Some("eat"),
        "hot reload also rebuilds named override tracks by stable key"
    );
    assert_eq!(
        app.world().entity(entity).get::<SpinalInstanceState>(),
        Some(&SpinalInstanceState::Degraded),
        "hot reload must re-resolve every name-based intent against the replacement"
    );
    let messages = app.world().resource::<Messages<SpinalIssue>>();
    let issue_kinds = issue_cursor
        .read(messages)
        .filter(|issue| issue.entity() == entity)
        .map(SpinalIssue::kind)
        .collect::<Vec<_>>();
    assert!(issue_kinds.contains(&SpinalIssueKind::MissingSkin));
    assert!(issue_kinds.contains(&SpinalIssueKind::MissingBone));

    let replacement = manual_asset(app.world_mut(), JSON);
    app.world_mut()
        .resource_mut::<Assets<SpinalAsset>>()
        .insert(asset_handle.id(), replacement)
        .expect("the live asset ID accepts a second replacement");
    app.update();
    app.update();
    assert_eq!(
        app.world().entity(entity).get::<SpinalInstanceState>(),
        Some(&SpinalInstanceState::Ready),
        "restoring the requested skin and bone clears replacement diagnostics"
    );
}

#[test]
fn replacing_animator_component_reapplies_same_revision_with_different_intent() {
    let mut app = headless_app();
    let asset_handle = add_asset(&mut app, JSON);
    let entity = app
        .world_mut()
        .spawn((
            SpinalInstance::new(asset_handle),
            SpinalAnimator::looping("idle"),
        ))
        .id();

    app.update();
    app.update();
    assert_eq!(
        app.world()
            .entity(entity)
            .get::<SpinalPlaybackState>()
            .and_then(SpinalPlaybackState::animation),
        Some("idle")
    );

    app.world_mut()
        .entity_mut(entity)
        .insert(SpinalAnimator::looping("eat"));
    app.update();

    assert_eq!(
        app.world()
            .entity(entity)
            .get::<SpinalPlaybackState>()
            .and_then(SpinalPlaybackState::animation),
        Some("eat"),
        "component replacement must compare intent, not only its local revision"
    );
}

#[test]
fn ecs_crossfade_observation_and_owned_events_compose_with_skin_and_override_intent() {
    let mut app = headless_app();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        100,
    )));
    let mut event_cursor = app
        .world()
        .resource::<Messages<SpinalAnimationEvent>>()
        .get_cursor_current();
    let asset_handle = add_asset(&mut app, JSON);
    let mut overrides = SpinalPoseOverrides::default();
    overrides.set(BoneOverride::new("look", BoneTransform::IDENTITY));
    let entity = app
        .world_mut()
        .spawn((
            SpinalInstance::new(asset_handle),
            SpinalAnimator::looping("idle"),
            SpinalSkinLayers::new(["item/hat/beret"]),
            overrides,
        ))
        .id();

    app.update();
    app.update();
    app.world_mut()
        .entity_mut(entity)
        .get_mut::<SpinalAnimator>()
        .expect("required animator exists")
        .play(
            "eat",
            PlaybackMode::Once,
            Transition::Crossfade(Crossfade::new(Duration::from_millis(400))),
        );
    app.update();

    let playback = app
        .world()
        .entity(entity)
        .get::<SpinalPlaybackState>()
        .expect("required playback observation exists");
    assert_eq!(playback.animation(), Some("eat"));
    assert_eq!(playback.position(), Some(Duration::from_millis(100)));
    assert!(
        (playback
            .transition_mix()
            .expect("the ECS facade exposes the live crossfade")
            .get()
            - 0.25)
            .abs()
            < 1.0e-6
    );

    app.update();
    app.update();
    let messages = app.world().resource::<Messages<SpinalAnimationEvent>>();
    let event = event_cursor
        .read(messages)
        .find(|event| event.entity() == entity && event.event() == "bite")
        .expect("the authored target event crosses the ECS boundary");
    assert_eq!(event.track(), None);
    assert_eq!(event.animation(), "eat");
    assert_eq!(event.local_time(), Duration::from_millis(250));
    assert_eq!(event.integer(), 2);
    assert_eq!(event.float(), 0.75);
    assert_eq!(event.string(), Some("crunch"));
    assert_eq!(event.volume(), 0.4);
    assert_eq!(event.balance(), -0.2);
    assert!(!event.is_degraded());
    assert_eq!(
        app.world().entity(entity).get::<SpinalInstanceState>(),
        Some(&SpinalInstanceState::Ready),
        "valid skin and override intent remain compatible with playback"
    );
}

#[test]
fn named_override_tracks_mix_emit_identity_and_publish_observation() {
    let mut app = headless_app();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        100,
    )));
    let mut event_cursor = app
        .world()
        .resource::<Messages<SpinalAnimationEvent>>()
        .get_cursor_current();
    let asset_handle = add_asset(&mut app, JSON);
    let mut tracks = SpinalAnimationTracks::default();
    tracks.play("aim", "eat", PlaybackMode::Loop, Transition::Immediate);
    tracks.set_weight("aim", Mix::ZERO);
    let entity = app
        .world_mut()
        .spawn((
            SpinalInstance::new(asset_handle),
            SpinalAnimator::looping("idle"),
            tracks,
        ))
        .id();

    app.update();
    app.update();
    app.world_mut()
        .entity_mut(entity)
        .get_mut::<SpinalAnimationTracks>()
        .expect("required named-track intent exists")
        .fade_weight("aim", Mix::ONE, WeightFade::new(Duration::from_millis(200)));
    app.update();

    let observations = app
        .world()
        .entity(entity)
        .get::<SpinalTrackStates>()
        .expect("required named-track observation exists");
    let aim = observations.get("aim").expect("aim observation exists");
    assert_eq!(aim.playback().animation(), Some("eat"));
    assert!((aim.weight().get() - 0.5).abs() < 1.0e-6);
    assert_eq!(aim.target_weight(), Mix::ONE);
    assert!(aim.is_weight_fading());

    app.update();
    let observations = app
        .world()
        .entity(entity)
        .get::<SpinalTrackStates>()
        .expect("required named-track observation exists");
    assert!(
        !observations
            .get("aim")
            .expect("aim observation exists")
            .is_weight_fading()
    );
    let messages = app.world().resource::<Messages<SpinalAnimationEvent>>();
    let event = event_cursor
        .read(messages)
        .find(|event| event.entity() == entity && event.event() == "bite")
        .expect("the override-track event crosses the ECS boundary");
    assert_eq!(event.track(), Some("aim"));
    assert_eq!(event.animation(), "eat");
}

#[test]
fn named_track_fade_declared_before_spawn_starts_from_the_declared_source() {
    let mut app = headless_app();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        100,
    )));
    let asset_handle = add_asset(&mut app, JSON);
    let mut tracks = SpinalAnimationTracks::default();
    tracks.set_weight("aim", Mix::ZERO);
    tracks.play("aim", "eat", PlaybackMode::Loop, Transition::Immediate);
    tracks.fade_weight("aim", Mix::ONE, WeightFade::new(Duration::from_millis(400)));
    let entity = app
        .world_mut()
        .spawn((SpinalInstance::new(asset_handle), tracks))
        .id();

    app.update();

    let aim = app
        .world()
        .entity(entity)
        .get::<SpinalTrackStates>()
        .and_then(|tracks| tracks.get("aim"))
        .expect("the named track is observed on its first animated frame");
    assert!(
        aim.weight() < Mix::ONE,
        "the first frame must preserve the declared zero source instead of snapping"
    );
    assert_eq!(aim.target_weight(), Mix::ONE);
    assert!(aim.is_weight_fading());
}

#[test]
fn hot_reload_preserves_the_presented_source_of_an_active_named_track_fade() {
    let mut app = headless_app();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        100,
    )));
    let asset_handle = add_asset(&mut app, JSON);
    let mut tracks = SpinalAnimationTracks::default();
    tracks.set_weight("aim", Mix::ZERO);
    tracks.play("aim", "eat", PlaybackMode::Loop, Transition::Immediate);
    tracks.fade_weight("aim", Mix::ONE, WeightFade::new(Duration::from_secs(1)));
    let entity = app
        .world_mut()
        .spawn((SpinalInstance::new(asset_handle.clone()), tracks))
        .id();
    app.update();
    app.update();

    let before_reload = app
        .world()
        .entity(entity)
        .get::<SpinalTrackStates>()
        .and_then(|tracks| tracks.get("aim"))
        .map(|track| track.weight())
        .expect("the active fade is observed before reload");
    let replacement = manual_asset(app.world_mut(), JSON);
    app.world_mut()
        .resource_mut::<Assets<SpinalAsset>>()
        .insert(asset_handle.id(), replacement)
        .expect("the live asset ID accepts a replacement");
    app.update();

    let after_reload = app
        .world()
        .entity(entity)
        .get::<SpinalTrackStates>()
        .and_then(|tracks| tracks.get("aim"))
        .expect("the named track is rebuilt after reload");
    assert!(
        after_reload.weight() > before_reload && after_reload.weight() < Mix::ONE,
        "hot reload must continue fading from the last presented weight instead of snapping"
    );
    assert!(after_reload.is_weight_fading());
}

#[test]
fn hot_reload_preserves_a_cloned_track_components_declaration_lineage() {
    let mut app = headless_app();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        100,
    )));
    let asset_handle = add_asset(&mut app, JSON);
    let mut tracks = SpinalAnimationTracks::default();
    tracks.set_weight("aim", Mix::ZERO);
    tracks.play("aim", "eat", PlaybackMode::Loop, Transition::Immediate);
    tracks.fade_weight("aim", Mix::ONE, WeightFade::new(Duration::from_secs(1)));
    let entity = app
        .world_mut()
        .spawn((SpinalInstance::new(asset_handle.clone()), tracks))
        .id();
    app.update();
    app.update();

    let before_reload = app
        .world()
        .entity(entity)
        .get::<SpinalTrackStates>()
        .and_then(|tracks| tracks.get("aim"))
        .map(|track| track.weight())
        .expect("the active fade is observed before reload");
    let cloned_tracks = app
        .world()
        .entity(entity)
        .get::<SpinalAnimationTracks>()
        .expect("track intent exists")
        .clone();
    let replacement = manual_asset(app.world_mut(), JSON);
    app.world_mut()
        .resource_mut::<Assets<SpinalAsset>>()
        .insert(asset_handle.id(), replacement)
        .expect("the live asset ID accepts a replacement");
    app.world_mut().entity_mut(entity).insert(cloned_tracks);
    app.update();

    let after_reload = app
        .world()
        .entity(entity)
        .get::<SpinalTrackStates>()
        .and_then(|tracks| tracks.get("aim"))
        .expect("the cloned declaration is rebuilt after reload");
    assert!(after_reload.weight() > before_reload && after_reload.weight() < Mix::ONE);
    assert!(after_reload.is_weight_fading());
}

#[test]
fn divergent_clones_cannot_share_a_recreated_tracks_hot_reload_seed() {
    let mut app = headless_app();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        100,
    )));
    let asset_handle = add_asset(&mut app, JSON);
    let mut tracks = SpinalAnimationTracks::default();
    tracks.set_weight("aim", Mix::ZERO);
    tracks.play("aim", "eat", PlaybackMode::Loop, Transition::Immediate);
    tracks.fade_weight("aim", Mix::ONE, WeightFade::new(Duration::from_secs(1)));
    let mut left = tracks.clone();
    let mut right = tracks.clone();
    let entity = app
        .world_mut()
        .spawn((SpinalInstance::new(asset_handle.clone()), tracks))
        .id();
    app.update();
    app.update();

    assert!(left.remove("aim"));
    left.set_weight("aim", Mix::ZERO);
    left.play("aim", "eat", PlaybackMode::Loop, Transition::Immediate);
    left.fade_weight("aim", Mix::ONE, WeightFade::new(Duration::from_secs(1)));
    app.world_mut().entity_mut(entity).insert(left);
    app.update();

    assert!(right.remove("aim"));
    right.set_weight("aim", Mix::ZERO);
    right.play("aim", "eat", PlaybackMode::Loop, Transition::Immediate);
    right.fade_weight("aim", Mix::ONE, WeightFade::new(Duration::from_secs(1)));
    let replacement = manual_asset(app.world_mut(), JSON);
    app.world_mut()
        .resource_mut::<Assets<SpinalAsset>>()
        .insert(asset_handle.id(), replacement)
        .expect("the live asset ID accepts a replacement");
    app.world_mut().entity_mut(entity).insert(right);
    app.update();

    let recreated = app
        .world()
        .entity(entity)
        .get::<SpinalTrackStates>()
        .and_then(|tracks| tracks.get("aim"))
        .expect("the divergent clone creates a fresh track");
    assert!(
        (recreated.weight().get() - 0.1).abs() < 1.0e-6,
        "a divergent clone must not consume its sibling's hot-reload seed"
    );
    assert!(recreated.is_weight_fading());
}

#[test]
fn hot_reload_weight_seed_is_not_reused_after_track_removal() {
    let mut app = headless_app();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        100,
    )));
    let asset_handle = add_asset(&mut app, JSON);
    let mut tracks = SpinalAnimationTracks::default();
    tracks.set_weight("aim", Mix::ZERO);
    tracks.play("aim", "eat", PlaybackMode::Loop, Transition::Immediate);
    tracks.fade_weight("aim", Mix::ONE, WeightFade::new(Duration::from_secs(1)));
    let entity = app
        .world_mut()
        .spawn((SpinalInstance::new(asset_handle.clone()), tracks))
        .id();
    app.update();
    app.update();

    let replacement = manual_asset(app.world_mut(), JSON);
    app.world_mut()
        .resource_mut::<Assets<SpinalAsset>>()
        .insert(asset_handle.id(), replacement)
        .expect("the live asset ID accepts a replacement");
    app.update();
    let weight_after_reload = app
        .world()
        .entity(entity)
        .get::<SpinalTrackStates>()
        .and_then(|tracks| tracks.get("aim"))
        .map(|track| track.weight())
        .expect("the reloaded track continues its fade");

    app.world_mut()
        .entity_mut(entity)
        .get_mut::<SpinalAnimationTracks>()
        .expect("track intent exists")
        .remove("aim");
    app.update();
    assert!(
        app.world()
            .entity(entity)
            .get::<SpinalTrackStates>()
            .is_some_and(|tracks| tracks.get("aim").is_none()),
        "the runtime observes removal before the stable key is reused"
    );

    {
        let mut entity_mut = app.world_mut().entity_mut(entity);
        let mut tracks = entity_mut
            .get_mut::<SpinalAnimationTracks>()
            .expect("track intent exists");
        tracks.set_weight("aim", Mix::ZERO);
        tracks.play("aim", "eat", PlaybackMode::Loop, Transition::Immediate);
        tracks.fade_weight("aim", Mix::ONE, WeightFade::new(Duration::from_secs(1)));
    }
    app.update();

    let readded = app
        .world()
        .entity(entity)
        .get::<SpinalTrackStates>()
        .and_then(|tracks| tracks.get("aim"))
        .expect("the reused key creates a new runtime track");
    assert!(
        readded.weight() < weight_after_reload,
        "the later track must fade from its new zero source, not the stale reload seed"
    );
    assert_eq!(readded.target_weight(), Mix::ONE);
    assert!(readded.is_weight_fading());
}

#[test]
fn hot_reload_does_not_seed_a_same_frame_recreated_track() {
    let mut app = headless_app();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        100,
    )));
    let asset_handle = add_asset(&mut app, JSON);
    let mut tracks = SpinalAnimationTracks::default();
    tracks.set_weight("aim", Mix::ZERO);
    tracks.play("aim", "eat", PlaybackMode::Loop, Transition::Immediate);
    tracks.fade_weight("aim", Mix::ONE, WeightFade::new(Duration::from_secs(1)));
    let entity = app
        .world_mut()
        .spawn((SpinalInstance::new(asset_handle.clone()), tracks))
        .id();
    app.update();
    app.update();

    let replacement = manual_asset(app.world_mut(), JSON);
    app.world_mut()
        .resource_mut::<Assets<SpinalAsset>>()
        .insert(asset_handle.id(), replacement)
        .expect("the live asset ID accepts a replacement");
    {
        let mut entity_mut = app.world_mut().entity_mut(entity);
        let mut tracks = entity_mut
            .get_mut::<SpinalAnimationTracks>()
            .expect("track intent exists");
        assert!(tracks.remove("aim"));
        tracks.set_weight("aim", Mix::ZERO);
        tracks.play("aim", "eat", PlaybackMode::Loop, Transition::Immediate);
        tracks.fade_weight("aim", Mix::ONE, WeightFade::new(Duration::from_secs(1)));
    }
    app.update();

    let readded = app
        .world()
        .entity(entity)
        .get::<SpinalTrackStates>()
        .and_then(|tracks| tracks.get("aim"))
        .expect("the same-frame replacement creates a fresh track");
    assert!(
        (readded.weight().get() - 0.1).abs() < 1.0e-6,
        "the fresh track must fade from its newly declared zero source"
    );
    assert_eq!(readded.target_weight(), Mix::ONE);
    assert!(readded.is_weight_fading());
}

#[test]
fn hot_reload_does_not_seed_an_independently_replaced_track_component() {
    let mut app = headless_app();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        100,
    )));
    let asset_handle = add_asset(&mut app, JSON);
    let mut tracks = SpinalAnimationTracks::default();
    tracks.set_weight("aim", Mix::ZERO);
    tracks.play("aim", "eat", PlaybackMode::Loop, Transition::Immediate);
    tracks.fade_weight("aim", Mix::ONE, WeightFade::new(Duration::from_secs(1)));
    let entity = app
        .world_mut()
        .spawn((SpinalInstance::new(asset_handle.clone()), tracks))
        .id();
    app.update();
    app.update();

    let replacement_asset = manual_asset(app.world_mut(), JSON);
    app.world_mut()
        .resource_mut::<Assets<SpinalAsset>>()
        .insert(asset_handle.id(), replacement_asset)
        .expect("the live asset ID accepts a replacement");
    let mut replacement_tracks = SpinalAnimationTracks::default();
    replacement_tracks.set_weight("aim", Mix::ZERO);
    replacement_tracks.play("aim", "eat", PlaybackMode::Loop, Transition::Immediate);
    replacement_tracks.fade_weight("aim", Mix::ONE, WeightFade::new(Duration::from_secs(1)));
    app.world_mut()
        .entity_mut(entity)
        .insert(replacement_tracks);
    app.update();

    let replaced = app
        .world()
        .entity(entity)
        .get::<SpinalTrackStates>()
        .and_then(|tracks| tracks.get("aim"))
        .expect("the replacement component creates a fresh track");
    assert!(
        (replaced.weight().get() - 0.1).abs() < 1.0e-6,
        "an independently constructed component must use its declared fade source"
    );
    assert_eq!(replaced.target_weight(), Mix::ONE);
    assert!(replaced.is_weight_fading());
}

#[test]
fn replacing_named_track_component_compares_full_intent_not_only_local_revisions() {
    let mut app = headless_app();
    let asset_handle = add_asset(&mut app, JSON);
    let mut initial = SpinalAnimationTracks::default();
    initial.play("aim", "idle", PlaybackMode::Loop, Transition::Immediate);
    let entity = app
        .world_mut()
        .spawn((SpinalInstance::new(asset_handle), initial))
        .id();
    app.update();
    app.update();
    assert_eq!(
        app.world()
            .entity(entity)
            .get::<SpinalTrackStates>()
            .and_then(|tracks| tracks.get("aim"))
            .and_then(|track| track.playback().animation()),
        Some("idle")
    );

    let mut replacement = SpinalAnimationTracks::default();
    replacement.play("aim", "eat", PlaybackMode::Loop, Transition::Immediate);
    app.world_mut().entity_mut(entity).insert(replacement);
    app.update();

    assert_eq!(
        app.world()
            .entity(entity)
            .get::<SpinalTrackStates>()
            .and_then(|tracks| tracks.get("aim"))
            .and_then(|track| track.playback().animation()),
        Some("eat"),
        "same-revision component replacement must compare its complete desired playback"
    );
}

#[test]
fn independently_replacing_named_track_component_starts_its_declared_fade() {
    let mut app = headless_app();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        100,
    )));
    let asset_handle = add_asset(&mut app, JSON);
    let mut initial = SpinalAnimationTracks::default();
    initial.play("aim", "eat", PlaybackMode::Loop, Transition::Immediate);
    initial.set_weight("aim", Mix::ZERO);
    initial.fade_weight("aim", Mix::ONE, WeightFade::new(Duration::from_secs(1)));
    let entity = app
        .world_mut()
        .spawn((SpinalInstance::new(asset_handle), initial))
        .id();
    app.update();
    app.update();
    let before_replacement = app
        .world()
        .entity(entity)
        .get::<SpinalTrackStates>()
        .and_then(|tracks| tracks.get("aim"))
        .map(|track| track.weight().get())
        .expect("the first fade is active");
    assert!(before_replacement > 0.0 && before_replacement < 0.5);

    let mut replacement = SpinalAnimationTracks::default();
    replacement.play("aim", "eat", PlaybackMode::Loop, Transition::Immediate);
    replacement.set_weight("aim", Mix::ONE);
    replacement.fade_weight("aim", Mix::ZERO, WeightFade::new(Duration::from_secs(1)));
    app.world_mut().entity_mut(entity).insert(replacement);
    app.update();

    let after_replacement = app
        .world()
        .entity(entity)
        .get::<SpinalTrackStates>()
        .and_then(|tracks| tracks.get("aim"))
        .expect("the replacement fade is observed");
    assert!((after_replacement.weight().get() - 0.9).abs() < 1.0e-6);
    assert_eq!(after_replacement.target_weight(), Mix::ZERO);
    assert!(after_replacement.is_weight_fading());
}

#[test]
fn remove_or_clear_then_same_frame_play_recreates_the_named_track() {
    let mut app = headless_app();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        100,
    )));
    let asset_handle = add_asset(&mut app, JSON);
    let mut tracks = SpinalAnimationTracks::default();
    tracks.play("aim", "eat", PlaybackMode::Loop, Transition::Immediate);
    let entity = app
        .world_mut()
        .spawn((SpinalInstance::new(asset_handle), tracks))
        .id();
    app.update();
    app.update();
    let initial_position = app
        .world()
        .entity(entity)
        .get::<SpinalTrackStates>()
        .and_then(|tracks| tracks.get("aim"))
        .and_then(|track| track.playback().position())
        .expect("the initial named playback exists");

    {
        let mut entity_mut = app.world_mut().entity_mut(entity);
        let mut tracks = entity_mut
            .get_mut::<SpinalAnimationTracks>()
            .expect("track intent exists");
        assert!(tracks.remove("aim"));
        tracks.play("aim", "eat", PlaybackMode::Loop, Transition::Immediate);
    }
    app.update();
    let after_remove = app
        .world()
        .entity(entity)
        .get::<SpinalTrackStates>()
        .and_then(|tracks| tracks.get("aim"))
        .and_then(|track| track.playback().position())
        .expect("remove followed by play recreates playback");
    assert_eq!(
        after_remove, initial_position,
        "remove followed by play must reset the same-name track clock"
    );

    {
        let mut entity_mut = app.world_mut().entity_mut(entity);
        let mut tracks = entity_mut
            .get_mut::<SpinalAnimationTracks>()
            .expect("track intent exists");
        tracks.clear();
        tracks.play("aim", "eat", PlaybackMode::Loop, Transition::Immediate);
    }
    app.update();
    let after_clear = app
        .world()
        .entity(entity)
        .get::<SpinalTrackStates>()
        .and_then(|tracks| tracks.get("aim"))
        .and_then(|track| track.playback().position())
        .expect("clear followed by play recreates playback");
    assert_eq!(
        after_clear, initial_position,
        "clear followed by play must also reset the same-name track clock"
    );
}

#[test]
fn named_track_reordering_preserves_playback_identity_and_clock() {
    let mut app = headless_app();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        100,
    )));
    let asset_handle = add_asset(&mut app, JSON);
    let mut tracks = SpinalAnimationTracks::default();
    tracks.play("look", "idle", PlaybackMode::Loop, Transition::Immediate);
    tracks.play("aim", "eat", PlaybackMode::Loop, Transition::Immediate);
    let entity = app
        .world_mut()
        .spawn((SpinalInstance::new(asset_handle), tracks))
        .id();
    app.update();
    app.update();

    let before = app
        .world()
        .entity(entity)
        .get::<SpinalTrackStates>()
        .expect("track observations exist");
    let look_playback = before
        .get("look")
        .and_then(|track| track.playback().playback())
        .expect("look is playing");
    let aim_playback = before
        .get("aim")
        .and_then(|track| track.playback().playback())
        .expect("aim is playing");
    let look_position = before
        .get("look")
        .and_then(|track| track.playback().position())
        .expect("look has a clock");

    app.world_mut()
        .entity_mut(entity)
        .get_mut::<SpinalAnimationTracks>()
        .expect("track intent exists")
        .move_to("aim", 0)
        .expect("aim can become the lower override");
    app.update();

    let after = app
        .world()
        .entity(entity)
        .get::<SpinalTrackStates>()
        .expect("track observations remain");
    assert_eq!(
        after.iter().map(|track| track.key()).collect::<Vec<_>>(),
        ["aim", "look"]
    );
    assert_eq!(
        after
            .get("look")
            .and_then(|track| track.playback().playback()),
        Some(look_playback)
    );
    assert_eq!(
        after
            .get("aim")
            .and_then(|track| track.playback().playback()),
        Some(aim_playback)
    );
    assert!(
        after
            .get("look")
            .and_then(|track| track.playback().position())
            .is_some_and(|position| position > look_position),
        "priority changes do not restart or pause the track clock"
    );
}

#[test]
fn deferred_override_property_degrades_and_emits_an_issue_in_the_same_frame() {
    let mut app = headless_app();
    let mut issue_cursor = app
        .world()
        .resource::<Messages<SpinalIssue>>()
        .get_cursor_current();
    let asset_handle = add_asset(
        &mut app,
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[{"name":"root"}],
          "slots":[{"name":"body","bone":"root","attachment":"body"}],
          "skins":[{
            "name":"default",
            "attachments":{"body":{"body":{"width":32,"height":32}}}
          }],
          "animations":{
            "wear":{"slots":{"body":{"attachment":[{"name":"body"}]}}}
          }
        }"#,
    );
    let mut tracks = SpinalAnimationTracks::default();
    tracks.play(
        "cosmetic",
        "wear",
        PlaybackMode::Loop,
        Transition::Immediate,
    );
    let entity = app
        .world_mut()
        .spawn((SpinalInstance::new(asset_handle), tracks))
        .id();

    app.update();
    app.update();

    assert_eq!(
        app.world().entity(entity).get::<SpinalInstanceState>(),
        Some(&SpinalInstanceState::Degraded),
        "the unsupported active contribution affects the frame that discovers it"
    );
    let messages = app.world().resource::<Messages<SpinalIssue>>();
    assert!(issue_cursor.read(messages).any(|issue| {
        issue.track() == Some("cosmetic")
            && matches!(
                issue.kind(),
                SpinalIssueKind::UnsupportedOverrideProperty(
                    bevy_spinal::spinal::PropertyKey::SlotAttachment(_)
                )
            )
    }));
}

#[test]
fn explicit_asset_selection_reports_loading_until_the_new_asset_is_ready() {
    let mut app = headless_app();
    let initial_handle = add_asset(&mut app, JSON);
    let entity = app
        .world_mut()
        .spawn((
            SpinalInstance::new(initial_handle),
            SpinalAnimator::looping("idle"),
        ))
        .id();
    app.update();
    app.update();
    assert_eq!(
        app.world().entity(entity).get::<SpinalInstanceState>(),
        Some(&SpinalInstanceState::Ready)
    );

    let pending_handle = Handle::<SpinalAsset>::from(Uuid::from_u128(77));
    app.world_mut()
        .entity_mut(entity)
        .get_mut::<SpinalInstance>()
        .expect("instance exists")
        .set_asset(pending_handle.clone());
    app.update();
    assert_eq!(
        app.world().entity(entity).get::<SpinalInstanceState>(),
        Some(&SpinalInstanceState::Loading),
        "the old runtime cannot leave a newly selected missing asset looking ready"
    );
    assert!(
        app.world()
            .entity(entity)
            .get::<bevy_spinal::SpinalPlaybackState>()
            .is_some_and(bevy_spinal::SpinalPlaybackState::is_idle),
        "the destroyed old player cannot remain publicly observable"
    );

    let replacement = manual_asset(app.world_mut(), JSON);
    app.world_mut()
        .resource_mut::<Assets<SpinalAsset>>()
        .insert(pending_handle.id(), replacement)
        .expect("the selected asset ID accepts its completed value");
    app.update();
    app.update();
    assert_eq!(
        app.world().entity(entity).get::<SpinalInstanceState>(),
        Some(&SpinalInstanceState::Ready)
    );
}

#[test]
fn unresolved_public_intent_degrades_without_preventing_a_frame() {
    let mut app = headless_app();
    let asset_handle = add_asset(&mut app, JSON);
    let entity = app
        .world_mut()
        .spawn(SpinalInstance::new(asset_handle))
        .id();
    app.world_mut()
        .entity_mut(entity)
        .insert(SpinalSkinLayers::new(["missing/coat"]));
    let mut overrides = SpinalPoseOverrides::default();
    overrides.set(BoneOverride::new(
        "missing/look_target",
        BoneTransform::IDENTITY,
    ));
    app.world_mut().entity_mut(entity).insert(overrides);

    app.update();
    app.update();
    assert_eq!(
        app.world().entity(entity).get::<SpinalInstanceState>(),
        Some(&SpinalInstanceState::Degraded)
    );
    assert!(
        app.world()
            .entity(entity)
            .get::<SpinalInstanceState>()
            .expect("state exists")
            .has_drawable_output()
    );

    app.world_mut()
        .entity_mut(entity)
        .insert(SpinalSkinLayers::new(["item/hat/beret"]));
    app.world_mut()
        .entity_mut(entity)
        .insert(SpinalPoseOverrides::default());
    app.update();
    assert_eq!(
        app.world().entity(entity).get::<SpinalInstanceState>(),
        Some(&SpinalInstanceState::Ready)
    );
}

#[test]
fn failed_control_target_reports_its_stable_bone_name() {
    let mut app = headless_app();
    let mut issue_cursor = app
        .world()
        .resource::<Messages<SpinalIssue>>()
        .get_cursor_current();
    let asset_handle = add_asset(&mut app, JSON);
    let mut overrides = SpinalPoseOverrides::default();
    overrides.set(BoneOverride::new(
        "root",
        BoneTransform::new(Vec2::ZERO, Angle::ZERO, Vec2::ZERO, Shear::ZERO)
            .expect("a zero-scale finite transform is valid"),
    ));
    let mut targets = SpinalControlTargets::default();
    targets
        .set_skeleton_position("look", Vec2::new(4.0, 3.0))
        .expect("the target position is finite");
    let entity = app
        .world_mut()
        .spawn((SpinalInstance::new(asset_handle), overrides, targets))
        .id();

    app.update();
    app.update();

    let messages = app.world().resource::<Messages<SpinalIssue>>();
    let issue = issue_cursor
        .read(messages)
        .find(|issue| issue.entity() == entity && issue.kind() == SpinalIssueKind::ControlTarget)
        .expect("the singular control target emits a specific issue");
    assert!(
        issue.message().contains("`look`"),
        "the diagnostic identifies the public stable bone name"
    );
}

#[test]
fn known_unsupported_alpha_encoding_loads_with_an_obvious_degraded_state() {
    let mut app = headless_app();
    let mut issue_cursor = app
        .world()
        .resource::<Messages<SpinalIssue>>()
        .get_cursor_current();
    let asset = manual_asset_with(app.world_mut(), JSON, PREMULTIPLIED_ATLAS);
    let asset_handle = app
        .world_mut()
        .resource_mut::<Assets<SpinalAsset>>()
        .add(asset);
    let entity = app
        .world_mut()
        .spawn(SpinalInstance::new(asset_handle))
        .id();

    app.update();
    app.update();

    assert_eq!(
        app.world().entity(entity).get::<SpinalInstanceState>(),
        Some(&SpinalInstanceState::DegradedNoDraws)
    );
    let state = app
        .world()
        .entity(entity)
        .get::<SpinalInstanceState>()
        .expect("state exists");
    assert!(state.is_usable());
    assert!(state.is_degraded());
    assert!(!state.has_drawable_output());
    let messages = app.world().resource::<Messages<SpinalIssue>>();
    assert!(issue_cursor.read(messages).any(|issue| {
        issue.kind() == SpinalIssueKind::AssetDiagnostic(DiagnosticCode::AlphaEncodingMismatch)
    }));
}

#[test]
fn known_unsupported_blend_mode_is_omitted_and_reported_as_degraded() {
    let mut app = headless_app();
    let mut issue_cursor = app
        .world()
        .resource::<Messages<SpinalIssue>>()
        .get_cursor_current();
    let asset_handle = add_asset(&mut app, ADDITIVE_JSON);
    let entity = app
        .world_mut()
        .spawn(SpinalInstance::new(asset_handle))
        .id();

    app.update();
    app.update();

    assert_eq!(
        app.world().entity(entity).get::<SpinalInstanceState>(),
        Some(&SpinalInstanceState::DegradedNoDraws)
    );
    let messages = app.world().resource::<Messages<SpinalIssue>>();
    assert!(issue_cursor.read(messages).any(|issue| {
        issue.kind() == SpinalIssueKind::UnsupportedBlendMode(SlotBlendMode::Additive)
    }));
}

#[test]
fn render_enabled_plugin_remains_safe_without_a_render_sub_app() {
    let mut app = headless_app();
    let asset_handle = add_asset(&mut app, JSON);
    let entity = app
        .world_mut()
        .spawn(SpinalInstance::new(asset_handle))
        .id();

    app.update();
    app.world_mut()
        .entity_mut(entity)
        .remove::<SpinalInstance>();
    app.update();

    assert!(
        app.world().entity(entity).get::<SpinalInstance>().is_none(),
        "headless applications can remove render-facing instances safely"
    );
}

fn headless_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), SpinalPlugin));
    app
}

fn add_asset(app: &mut App, json: &[u8]) -> bevy::asset::Handle<SpinalAsset> {
    let asset = manual_asset(app.world_mut(), json);
    app.world_mut()
        .resource_mut::<Assets<SpinalAsset>>()
        .add(asset)
}

fn manual_asset(world: &mut bevy::prelude::World, json: &[u8]) -> SpinalAsset {
    manual_asset_with(world, json, ATLAS)
}

fn manual_asset_with(world: &mut bevy::prelude::World, json: &[u8], atlas: &[u8]) -> SpinalAsset {
    let skeleton = load_json(json, atlas)
        .expect("fixture is a supported export")
        .into_asset();
    let image = world.resource_mut::<Assets<Image>>().add(Image::default());
    SpinalAsset::new(
        Arc::clone(&skeleton),
        vec![SpinalAtlasPage::new("cat.png", image)],
    )
    .expect("manual page matches the linked atlas")
}

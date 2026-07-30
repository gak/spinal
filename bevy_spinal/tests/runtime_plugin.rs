//! Headless ECS integration tests for playback, diagnostics, and hot reload.

use std::sync::Arc;

use bevy::{
    asset::{AssetPlugin, Assets, Handle, uuid::Uuid},
    ecs::message::Messages,
    image::Image,
    prelude::{App, MinimalPlugins},
};
use bevy_spinal::{
    BoneOverride, SpinalAnimator, SpinalAsset, SpinalAtlasPage, SpinalInstance,
    SpinalInstanceState, SpinalIssue, SpinalIssueKind, SpinalPlugin, SpinalPoseOverrides,
    SpinalSkinLayers,
    spinal::{DiagnosticCode, SlotBlendMode},
};
use spinal::{BoneTransform, PlaybackMode, Transition, load_json};

const ATLAS: &[u8] = b"cat.png\n\tsize: 1, 1\nbody\n\tbounds: 0, 0, 1, 1\n";
const PREMULTIPLIED_ATLAS: &[u8] =
    b"cat.png\n\tsize: 1, 1\n\tpma: true\nbody\n\tbounds: 0, 0, 1, 1\n";

const JSON: &[u8] = br#"{
  "skeleton": { "spine": "4.3.23" },
  "bones": [{ "name": "root" }],
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
      }
    }
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
fn plugin_applies_name_based_intent_and_recovers_across_asset_replacement() {
    let mut app = headless_app();
    let asset_handle = add_asset(&mut app, JSON);
    let entity = app
        .world_mut()
        .spawn((
            SpinalInstance::new(asset_handle.clone()),
            SpinalAnimator::looping("idle"),
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

    let replacement = manual_asset(app.world_mut(), JSON);
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
        app.world().entity(entity).get::<SpinalInstanceState>(),
        Some(&SpinalInstanceState::Ready)
    );
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
        Some(&SpinalInstanceState::Degraded)
    );
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
        Some(&SpinalInstanceState::Degraded)
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

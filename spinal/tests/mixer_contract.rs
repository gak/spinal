//! Public contract tests for the v0.4 layered animation mixer.

use std::{sync::Arc, time::Duration};

use spinal::{
    AnimationMixer, Crossfade, Mix, OverrideSupport, PlayOptions, PlayerError, PropertyKey,
    Skeleton, TrackAnimationEvent, TrackErrorKind, TrackOptions, TransformMixChannel, Transition,
    WeightFade, glam::Vec2, load_json,
};

const MIXER_JSON: &[u8] = br#"{
  "skeleton":{"spine":"4.3.23"},
  "bones":[
    {"name":"root"},
    {"name":"body","parent":"root","x":2,"rotation":10},
    {"name":"aim","parent":"body","rotation":5}
  ],
  "events":{"step":{},"aimed":{}},
  "animations":{
    "walk":{
      "bones":{
        "root":{
          "translate":[
            {"x":0,"y":0},
            {"time":1,"x":10,"y":0}
          ]
        },
        "body":{
          "rotate":[
            {"value":0},
            {"time":1,"value":20}
          ]
        },
        "aim":{
          "rotate":[
            {"value":0},
            {"time":1,"value":20}
          ],
          "translate":[
            {"x":0,"y":0},
            {"time":1,"x":20,"y":0}
          ]
        }
      },
      "events":[{"time":0.25,"name":"step"}]
    },
    "fall":{
      "bones":{
        "root":{
          "rotate":[
            {"value":0},
            {"time":1,"value":90}
          ]
        }
      }
    },
    "look":{
      "bones":{
        "aim":{
          "rotate":[
            {"value":60},
            {"time":1,"value":60}
          ],
          "translate":[
            {"x":60,"y":0},
            {"time":1,"x":60,"y":0}
          ]
        }
      },
      "events":[{"time":0.25,"name":"aimed"}]
    },
    "body-only":{
      "bones":{
        "body":{
          "rotate":[
            {"value":0},
            {"time":1,"value":0}
          ]
        }
      }
    },
    "look-back":{
      "bones":{
        "aim":{
          "rotate":[
            {"value":-60},
            {"time":1,"value":-60}
          ],
          "translate":[
            {"x":-60,"y":0},
            {"time":1,"x":-60,"y":0}
          ]
        }
      }
    }
  }
}"#;

fn mixer_fixture() -> (Arc<spinal::SkeletonAsset>, Skeleton) {
    let asset = load_json(MIXER_JSON, b"page.png\n")
        .expect("the mixer fixture loads")
        .into_asset();
    let skeleton = Skeleton::new(Arc::clone(&asset));
    (asset, skeleton)
}

#[test]
fn animations_expose_ordered_unique_authored_properties_and_override_support() {
    let asset = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[
            {"name":"root"},
            {"name":"animated","parent":"root"},
            {"name":"target","parent":"root"}
          ],
          "slots":[{"name":"body","bone":"animated"}],
          "constraints":[
            {
              "type":"ik",
              "name":"aim",
              "bones":["animated"],
              "target":"target"
            },
            {
              "type":"transform",
              "name":"follow",
              "source":"target",
              "bones":["animated"],
              "properties":{"rotate":{"to":{"rotate":{"max":100}}}}
            }
          ],
          "animations":{
            "everything":{
              "bones":{
                "animated":{
                  "translate":[{"x":1,"y":2}],
                  "rotate":[{"value":3}],
                  "scale":[{"x":1.25,"y":0.75}],
                  "shear":[{"x":4,"y":5}]
                }
              },
              "slots":{
                "body":{
                  "rgba":[{"color":"FFFFFFFF"}],
                  "attachment":[{"name":null}]
                }
              },
              "ik":{"aim":[{"mix":0.5,"bendPositive":false}]},
              "transform":{"follow":[{"mixRotate":0.5}]},
              "drawOrder":[{}]
            },
            "flip":{
              "bones":{"animated":{"scale":[{"x":-1,"y":1}]}}
            }
          }
        }"#,
        b"page.png\n",
    )
    .expect("the property fixture loads")
    .into_asset();
    let animated = asset.bone_id("animated").expect("bone exists");
    let body = asset.slot_id("body").expect("slot exists");
    let aim = asset.ik_constraint_id("aim").expect("IK exists");
    let follow = asset
        .transform_constraint_id("follow")
        .expect("transform constraint exists");
    let animation = asset
        .animation(asset.animation_id("everything").expect("animation exists"))
        .expect("animation belongs to the asset");

    let properties = animation.properties().collect::<Vec<_>>();
    assert_eq!(
        properties,
        [
            PropertyKey::BoneTranslation(animated),
            PropertyKey::BoneRotation(animated),
            PropertyKey::BoneScaleMagnitude(animated),
            PropertyKey::BoneScaleSign(animated),
            PropertyKey::BoneShear(animated),
            PropertyKey::SlotColor(body),
            PropertyKey::SlotAttachment(body),
            PropertyKey::IkMix(aim),
            PropertyKey::IkBendDirection(aim),
            PropertyKey::TransformMix(follow, TransformMixChannel::Rotate),
            PropertyKey::TransformMix(follow, TransformMixChannel::X),
            PropertyKey::TransformMix(follow, TransformMixChannel::Y),
            PropertyKey::TransformMix(follow, TransformMixChannel::ScaleX),
            PropertyKey::TransformMix(follow, TransformMixChannel::ScaleY),
            PropertyKey::TransformMix(follow, TransformMixChannel::ShearY),
            PropertyKey::DrawOrder,
        ]
    );
    assert_eq!(animation.properties().len(), properties.len());
    assert_eq!(
        properties
            .iter()
            .copied()
            .filter(|property| property.override_support() == OverrideSupport::Deferred)
            .collect::<Vec<_>>(),
        [
            PropertyKey::BoneScaleSign(animated),
            PropertyKey::SlotAttachment(body),
            PropertyKey::IkBendDirection(aim),
            PropertyKey::DrawOrder,
        ]
    );

    let compatibility = animation.override_compatibility();
    assert!(!compatibility.is_supported());
    assert_eq!(
        compatibility.deferred_properties().collect::<Vec<_>>(),
        [
            PropertyKey::SlotAttachment(body),
            PropertyKey::IkBendDirection(aim),
            PropertyKey::DrawOrder,
        ]
    );
    let flip = asset
        .animation(asset.animation_id("flip").expect("flip exists"))
        .expect("flip belongs to the asset");
    assert_eq!(
        flip.override_compatibility()
            .deferred_properties()
            .collect::<Vec<_>>(),
        [PropertyKey::BoneScaleSign(animated)]
    );

    let mut skeleton = Skeleton::new(Arc::clone(&asset));
    let mut mixer = AnimationMixer::new(&skeleton);
    let track = mixer
        .insert_track(TrackOptions::override_track())
        .expect("the mixer has track identity capacity");
    mixer
        .track_mut(track)
        .expect("track exists")
        .play(animation.id(), PlayOptions::once())
        .expect("animation belongs to the mixer");
    let _frame = mixer
        .update(&mut skeleton, Duration::ZERO, &mut ())
        .expect("the degraded override still updates")
        .solve();
    let issues = mixer.active_deferred_properties().collect::<Vec<_>>();
    assert!(mixer.has_degraded_overrides());
    assert!(issues.iter().all(|issue| issue.track() == track));
    assert_eq!(
        issues
            .iter()
            .map(|issue| issue.property())
            .collect::<Vec<_>>(),
        [
            PropertyKey::SlotAttachment(body),
            PropertyKey::IkBendDirection(aim),
            PropertyKey::DrawOrder,
        ]
    );
}

#[test]
fn ordered_override_tracks_change_only_their_authored_continuous_properties() {
    let (asset, mut skeleton) = mixer_fixture();
    let walk = asset.animation_id("walk").expect("walk exists");
    let look = asset.animation_id("look").expect("look exists");
    let root = asset.bone_id("root").expect("root exists");
    let body = asset.bone_id("body").expect("body exists");
    let aim_bone = asset.bone_id("aim").expect("aim bone exists");
    let mut mixer = AnimationMixer::new(&skeleton);

    mixer
        .base_track_mut()
        .play(walk, PlayOptions::once())
        .expect("walk belongs to the mixer");
    let aim = mixer
        .insert_track(TrackOptions::override_track())
        .expect("the mixer has track identity capacity");
    mixer
        .track_mut(aim)
        .expect("new track exists")
        .play(look, PlayOptions::looping())
        .expect("look belongs to the mixer");
    mixer
        .track_mut(aim)
        .expect("new track exists")
        .set_weight(Mix::ONE);

    let frame = mixer
        .update(&mut skeleton, Duration::from_millis(500), &mut ())
        .expect("mixer remains bound to the skeleton")
        .solve();
    let root_pose = frame
        .bone(root)
        .expect("root belongs to the skeleton")
        .local_transform();
    let body_pose = frame
        .bone(body)
        .expect("body belongs to the skeleton")
        .local_transform();
    let aim_pose = frame
        .bone(aim_bone)
        .expect("aim belongs to the skeleton")
        .local_transform();

    assert!((root_pose.translation().x - 5.0).abs() < 1.0e-4);
    assert!((body_pose.rotation().as_degrees() - 20.0).abs() < 1.0e-4);
    assert!((aim_pose.rotation().as_degrees() - 65.0).abs() < 1.0e-4);
}

#[test]
fn every_supported_continuous_property_blends_over_its_live_lower_value() {
    let asset = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[
            {"name":"root"},
            {
              "name":"free",
              "parent":"root",
              "x":2,
              "y":4,
              "rotation":10,
              "scaleX":-2,
              "scaleY":4,
              "shearX":5,
              "shearY":10
            },
            {"name":"ik-bone","parent":"root"},
            {"name":"ik-target","parent":"root","x":10,"y":10},
            {"name":"source","parent":"root","rotation":90},
            {"name":"copy-bone","parent":"root"}
          ],
          "slots":[{"name":"tint","bone":"free","color":"000000FF"}],
          "constraints":[
            {
              "type":"ik",
              "name":"aim",
              "bones":["ik-bone"],
              "target":"ik-target",
              "mix":0
            },
            {
              "type":"transform",
              "name":"copy",
              "source":"source",
              "bones":["copy-bone"],
              "properties":{"rotate":{"to":{"rotate":{"max":100}}}},
              "mixRotate":0,
              "mixX":0,
              "mixY":0,
              "mixScaleX":0,
              "mixScaleY":0,
              "mixShearY":0
            }
          ],
          "animations":{
            "all":{
              "bones":{
                "free":{
                  "translate":[{"x":10,"y":20}],
                  "rotate":[{"value":30}],
                  "scale":[{"x":2,"y":0.5}],
                  "shear":[{"x":10,"y":-10}]
                }
              },
              "slots":{"tint":{"rgba":[{"color":"FFFFFFFF"}]}},
              "ik":{"aim":[{"mix":1}]},
              "transform":{"copy":[{
                "mixRotate":1,
                "mixX":1,
                "mixY":1,
                "mixScaleX":1,
                "mixScaleY":1,
                "mixShearY":1
              }]}
            }
          }
        }"#,
        b"page.png\n",
    )
    .expect("the continuous-property fixture loads")
    .into_asset();
    let animation = asset.animation_id("all").expect("animation exists");
    let free = asset.bone_id("free").expect("free bone exists");
    let tint = asset.slot_id("tint").expect("tint slot exists");
    let aim = asset.ik_constraint_id("aim").expect("IK exists");
    let copy = asset
        .transform_constraint_id("copy")
        .expect("transform constraint exists");
    let mut skeleton = Skeleton::new(Arc::clone(&asset));
    let mut mixer = AnimationMixer::new(&skeleton);
    let track = mixer
        .insert_track(
            TrackOptions::override_track()
                .with_weight(Mix::new(0.5).expect("one half is normalized")),
        )
        .expect("track identity remains available");
    mixer
        .track_mut(track)
        .expect("track exists")
        .play(animation, PlayOptions::looping())
        .expect("animation belongs to the mixer");

    let frame = mixer
        .update(&mut skeleton, Duration::ZERO, &mut ())
        .expect("every continuous contribution samples")
        .solve();
    assert!(
        !mixer.has_degraded_overrides(),
        "positive scale magnitudes and unchanged IK bend direction need no false diagnostic"
    );
    let transform = frame
        .bone(free)
        .expect("free bone belongs to the skeleton")
        .local_transform();
    assert!((transform.translation().x - 7.0).abs() < 1.0e-4);
    assert!((transform.translation().y - 14.0).abs() < 1.0e-4);
    assert!((transform.rotation().as_degrees() - 25.0).abs() < 1.0e-4);
    assert!((transform.scale().x + 3.0).abs() < 1.0e-4);
    assert!((transform.scale().y - 3.0).abs() < 1.0e-4);
    assert!((transform.shear().x().as_degrees() - 10.0).abs() < 1.0e-4);
    assert!((transform.shear().y().as_degrees() - 5.0).abs() < 1.0e-4);
    let color = frame
        .slot(tint)
        .expect("tint slot belongs to the skeleton")
        .color()
        .to_array();
    assert!(
        color[..3]
            .iter()
            .all(|channel| (*channel - 0.5).abs() < 1.0e-4)
    );
    assert!((color[3] - 1.0).abs() < 1.0e-4);
    drop(frame);

    assert!(
        (skeleton
            .ik_constraint_pose(aim)
            .expect("IK pose belongs to the skeleton")
            .mix()
            .get()
            - 0.5)
            .abs()
            < 1.0e-4
    );
    let transform = skeleton
        .transform_constraint_pose(copy)
        .expect("transform pose belongs to the skeleton");
    for channel in [
        transform.mix_rotate(),
        transform.mix_x(),
        transform.mix_y(),
        transform.mix_scale_x(),
        transform.mix_scale_y(),
        transform.mix_shear_y(),
    ] {
        assert!((channel.get() - 0.5).abs() < 1.0e-4);
    }
}

#[test]
fn a_sparse_property_is_absent_before_its_first_key() {
    let asset = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[{"name":"root","x":3}],
          "animations":{
            "base":{"bones":{"root":{"translate":[{"x":10},{"time":1,"x":10}]}}},
            "late":{"bones":{"root":{"translate":[{"time":0.5,"x":100}]}}}
          }
        }"#,
        b"page.png\n",
    )
    .expect("the before-first-key fixture loads")
    .into_asset();
    let base = asset.animation_id("base").expect("base exists");
    let late = asset.animation_id("late").expect("late exists");
    let root = asset.bone_id("root").expect("root exists");
    let mut skeleton = Skeleton::new(Arc::clone(&asset));
    let mut mixer = AnimationMixer::new(&skeleton);
    mixer
        .base_track_mut()
        .play(base, PlayOptions::once())
        .expect("base belongs to the mixer");
    let track = mixer
        .insert_track(TrackOptions::override_track())
        .expect("track identity remains available");
    mixer
        .track_mut(track)
        .expect("track exists")
        .play(late, PlayOptions::once())
        .expect("late animation belongs to the mixer");

    let before = mixer
        .update(&mut skeleton, Duration::from_millis(250), &mut ())
        .expect("before-first-key sampling succeeds")
        .solve();
    assert!(
        (before
            .bone(root)
            .expect("root belongs to the skeleton")
            .local_transform()
            .translation()
            .x
            - 13.0)
            .abs()
            < 1.0e-4,
        "the late timeline must not contribute setup or first-key data early"
    );
    drop(before);

    let at_key = mixer
        .update(&mut skeleton, Duration::from_millis(250), &mut ())
        .expect("exact first-key sampling succeeds")
        .solve();
    assert!(
        (at_key
            .bone(root)
            .expect("root belongs to the skeleton")
            .local_transform()
            .translation()
            .x
            - 103.0)
            .abs()
            < 1.0e-4
    );
}

#[test]
fn immediate_play_replacement_starts_fresh_rotation_and_shear_branches() {
    let asset = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[{"name":"root"}],
          "animations":{
            "clockwise":{
              "bones":{"root":{
                "rotate":[{"value":170}],
                "shear":[{"x":170,"y":170}]
              }}
            },
            "counter-clockwise":{
              "bones":{"root":{
                "rotate":[{"value":-170}],
                "shear":[{"x":-170,"y":-170}]
              }}
            }
          }
        }"#,
        b"page.png\n",
    )
    .expect("the angular replacement fixture loads")
    .into_asset();
    let clockwise = asset
        .animation_id("clockwise")
        .expect("clockwise animation exists");
    let counter_clockwise = asset
        .animation_id("counter-clockwise")
        .expect("counter-clockwise animation exists");
    let root = asset.bone_id("root").expect("root exists");
    let mut skeleton = Skeleton::new(Arc::clone(&asset));
    let mut mixer = AnimationMixer::new(&skeleton);
    let half = Mix::new(0.5).expect("one half is normalized");
    let track = mixer
        .insert_track(TrackOptions::override_track().with_weight(half))
        .expect("track identity remains available");
    mixer
        .track_mut(track)
        .expect("track exists")
        .play(clockwise, PlayOptions::looping())
        .expect("clockwise belongs to the mixer");
    let first = mixer
        .update(&mut skeleton, Duration::ZERO, &mut ())
        .expect("the first branch initializes")
        .solve();
    let first_transform = first
        .bone(root)
        .expect("root belongs to the skeleton")
        .local_transform();
    assert!((first_transform.rotation().as_degrees() - 85.0).abs() < 1.0e-4);
    assert!((first_transform.shear().x().as_degrees() - 85.0).abs() < 1.0e-4);
    assert!((first_transform.shear().y().as_degrees() - 85.0).abs() < 1.0e-4);
    drop(first);

    mixer
        .track_mut(track)
        .expect("track exists")
        .play(counter_clockwise, PlayOptions::looping())
        .expect("counter-clockwise belongs to the mixer");
    let replaced = mixer
        .update(&mut skeleton, Duration::ZERO, &mut ())
        .expect("the replacement samples")
        .solve();
    let replaced_transform = replaced
        .bone(root)
        .expect("root belongs to the skeleton")
        .local_transform();
    assert!(
        (replaced_transform.rotation().as_degrees() + 85.0).abs() < 1.0e-4,
        "an immediate play command starts a fresh shortest rotation branch"
    );
    assert!(
        (replaced_transform.shear().x().as_degrees() + 85.0).abs() < 1.0e-4,
        "an immediate play command starts a fresh shortest shear-x branch"
    );
    assert!(
        (replaced_transform.shear().y().as_degrees() + 85.0).abs() < 1.0e-4,
        "an immediate play command starts a fresh shortest shear-y branch"
    );
}

#[test]
fn sparse_angular_contributions_start_fresh_branches_after_absence() {
    let asset = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[{"name":"root"}],
          "animations":{
            "turn":{
              "bones":{"root":{
                "rotate":[
                  {"value":0,"curve":"stepped"},
                  {"time":1,"value":190},
                  {"time":2,"value":190}
                ],
                "shear":[
                  {"x":0,"y":0,"curve":"stepped"},
                  {"time":1,"x":190,"y":190},
                  {"time":2,"x":190,"y":190}
                ]
              }}
            },
            "late":{
              "bones":{"root":{
                "rotate":[
                  {"time":0.5,"value":170},
                  {"time":1,"value":170}
                ],
                "shear":[
                  {"time":0.5,"x":170,"y":170},
                  {"time":1,"x":170,"y":170}
                ]
              }}
            }
          }
        }"#,
        b"page.png\n",
    )
    .expect("the sparse angular fixture loads")
    .into_asset();
    let turn = asset.animation_id("turn").expect("turn animation exists");
    let late = asset.animation_id("late").expect("late animation exists");
    let root = asset.bone_id("root").expect("root exists");
    let mut skeleton = Skeleton::new(Arc::clone(&asset));
    let mut mixer = AnimationMixer::new(&skeleton);
    mixer
        .base_track_mut()
        .play(turn, PlayOptions::once())
        .expect("turn belongs to the mixer");
    let track = mixer
        .insert_track(
            TrackOptions::override_track()
                .with_weight(Mix::new(0.5).expect("one half is normalized")),
        )
        .expect("track identity remains available");
    mixer
        .track_mut(track)
        .expect("track exists")
        .play(late, PlayOptions::looping())
        .expect("late belongs to the mixer");

    let first = mixer
        .update(&mut skeleton, Duration::from_millis(500), &mut ())
        .expect("the first contribution samples")
        .solve();
    assert!(
        (first
            .bone(root)
            .expect("root belongs to the skeleton")
            .local_transform()
            .rotation()
            .as_degrees()
            - 85.0)
            .abs()
            < 1.0e-4
    );
    drop(first);

    let absent = mixer
        .update(&mut skeleton, Duration::from_millis(500), &mut ())
        .expect("the looping contribution becomes absent")
        .solve();
    assert!(
        (absent
            .bone(root)
            .expect("root belongs to the skeleton")
            .local_transform()
            .rotation()
            .as_degrees()
            - 190.0)
            .abs()
            < 1.0e-4
    );
    drop(absent);

    let returned = mixer
        .update(&mut skeleton, Duration::from_millis(500), &mut ())
        .expect("the looping contribution returns")
        .solve();
    let returned_transform = returned
        .bone(root)
        .expect("root belongs to the skeleton")
        .local_transform();
    for angle in [
        returned_transform.rotation(),
        returned_transform.shear().x(),
        returned_transform.shear().y(),
    ] {
        assert!(
            (angle.as_degrees() - 180.0).abs() < 1.0e-4,
            "a returned sparse contribution must choose a fresh shortest branch, got {}",
            angle.as_degrees()
        );
    }
}

#[test]
fn interrupted_crossfades_remain_sparse_over_the_live_lower_track_pose() {
    let (asset, mut skeleton) = mixer_fixture();
    let walk = asset.animation_id("walk").expect("walk exists");
    let look = asset.animation_id("look").expect("look exists");
    let body_only = asset.animation_id("body-only").expect("body-only exists");
    let look_back = asset.animation_id("look-back").expect("look-back exists");
    let aim_bone = asset.bone_id("aim").expect("aim bone exists");
    let transition = Transition::Crossfade(Crossfade::new(Duration::from_secs(1)));
    let mut mixer = AnimationMixer::new(&skeleton);

    mixer
        .base_track_mut()
        .play(walk, PlayOptions::once())
        .expect("walk belongs to the mixer");
    let aim = mixer
        .insert_track(TrackOptions::override_track())
        .expect("the mixer has track identity capacity");
    mixer
        .track_mut(aim)
        .expect("aim track exists")
        .play(look, PlayOptions::once())
        .expect("look belongs to the mixer");
    let initial = mixer
        .update(&mut skeleton, Duration::from_millis(250), &mut ())
        .expect("initial update succeeds")
        .solve();
    assert!(
        (initial
            .bone(aim_bone)
            .expect("aim bone belongs to the skeleton")
            .local_transform()
            .rotation()
            .as_degrees()
            - 65.0)
            .abs()
            < 1.0e-4
    );
    drop(initial);

    mixer
        .track_mut(aim)
        .expect("aim track exists")
        .play(body_only, PlayOptions::once().with_transition(transition))
        .expect("body-only belongs to the mixer");
    let fading_out = mixer
        .update(&mut skeleton, Duration::from_millis(500), &mut ())
        .expect("fade-out update succeeds")
        .solve();
    assert!(
        (fading_out
            .bone(aim_bone)
            .expect("aim bone belongs to the skeleton")
            .local_transform()
            .rotation()
            .as_degrees()
            - 42.5)
            .abs()
            < 1.0e-4,
        "a disappearing aim property fades toward the current walk pose"
    );
    drop(fading_out);

    mixer
        .track_mut(aim)
        .expect("aim track exists")
        .play(look_back, PlayOptions::once().with_transition(transition))
        .expect("look-back belongs to the mixer");
    let interrupted = mixer
        .update(&mut skeleton, Duration::from_millis(500), &mut ())
        .expect("interrupted transition succeeds")
        .solve();
    assert!(
        (interrupted
            .bone(aim_bone)
            .expect("aim bone belongs to the skeleton")
            .local_transform()
            .rotation()
            .as_degrees()
            + 5.0)
            .abs()
            < 1.0e-4,
        "the interrupted sparse contribution is recomposed over the new live walk pose"
    );
}

#[test]
fn animation_clocks_pause_independently_while_weight_fades_use_wall_time() {
    let (asset, mut skeleton) = mixer_fixture();
    let walk = asset.animation_id("walk").expect("walk exists");
    let look = asset.animation_id("look").expect("look exists");
    let root = asset.bone_id("root").expect("root exists");
    let aim_bone = asset.bone_id("aim").expect("aim bone exists");
    let mut mixer = AnimationMixer::new(&skeleton);

    {
        let mut base = mixer.base_track_mut();
        base.play(walk, PlayOptions::once())
            .expect("walk belongs to the mixer");
        base.set_paused(true);
    }
    let aim = mixer
        .insert_track(TrackOptions::override_track().with_weight(Mix::ZERO))
        .expect("the mixer has track identity capacity");
    {
        let mut track = mixer.track_mut(aim).expect("aim track exists");
        track
            .play(look, PlayOptions::once())
            .expect("look belongs to the mixer");
        track.set_paused(true);
        track.fade_weight(Mix::ONE, WeightFade::new(Duration::from_secs(1)));
    }

    let halfway = mixer
        .update(&mut skeleton, Duration::from_millis(500), &mut ())
        .expect("paused update succeeds")
        .solve();
    assert!(
        (halfway
            .bone(root)
            .expect("root belongs to the skeleton")
            .local_transform()
            .translation()
            .x)
            .abs()
            < 1.0e-4,
        "the paused base clock remains at time zero"
    );
    assert!(
        (halfway
            .bone(aim_bone)
            .expect("aim bone belongs to the skeleton")
            .local_transform()
            .rotation()
            .as_degrees()
            - 35.0)
            .abs()
            < 1.0e-4,
        "the wall-clock weight fade reaches one half over the paused setup pose"
    );
    drop(halfway);

    {
        let mut base = mixer.base_track_mut();
        base.set_paused(false);
        base.set_speed(2.0).expect("two-times speed is valid");
    }
    {
        let mut track = mixer.track_mut(aim).expect("aim track exists");
        assert_eq!(track.weight(), Mix::new(0.5).expect("half is normalized"));
        assert!(track.set_speed(f32::NAN).is_err());
        assert_eq!(track.speed(), 1.0, "a rejected speed is failure-atomic");
    }
    let complete = mixer
        .update(&mut skeleton, Duration::from_millis(500), &mut ())
        .expect("unpaused update succeeds")
        .solve();
    assert!(
        (complete
            .bone(root)
            .expect("root belongs to the skeleton")
            .local_transform()
            .translation()
            .x
            - 10.0)
            .abs()
            < 1.0e-4
    );
    assert!(
        (complete
            .bone(aim_bone)
            .expect("aim belongs to the skeleton")
            .local_transform()
            .rotation()
            .as_degrees()
            - 65.0)
            .abs()
            < 1.0e-4
    );
}

#[test]
fn authored_events_are_track_aware_and_follow_deterministic_track_order() {
    let (asset, mut skeleton) = mixer_fixture();
    let walk = asset.animation_id("walk").expect("walk exists");
    let look = asset.animation_id("look").expect("look exists");
    let mut mixer = AnimationMixer::new(&skeleton);
    let base = mixer.base_track_id();
    mixer
        .base_track_mut()
        .play(walk, PlayOptions::once())
        .expect("walk belongs to the mixer");
    let aim = mixer
        .insert_track(TrackOptions::override_track())
        .expect("the mixer has track identity capacity");
    mixer
        .track_mut(aim)
        .expect("aim track exists")
        .play(look, PlayOptions::once())
        .expect("look belongs to the mixer");

    let mut events = Vec::new();
    let _frame = mixer
        .update(
            &mut skeleton,
            Duration::from_millis(250),
            &mut |event: TrackAnimationEvent<'_>| {
                events.push((event.track(), event.event().definition().name().to_owned()));
            },
        )
        .expect("eventful update succeeds")
        .solve();
    assert_eq!(
        events,
        [(base, "step".to_owned()), (aim, "aimed".to_owned())]
    );
}

#[test]
fn one_large_update_matches_split_updates_with_accumulated_lifecycle_pulses() {
    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    struct LifecycleTotals {
        playbacks_completed: u32,
        loops_completed: u128,
        transitions_completed: u32,
        weight_fades_completed: u32,
    }

    let accumulate_reports = |totals: &mut [LifecycleTotals], mixer: &AnimationMixer| {
        for (total, report) in totals.iter_mut().zip(mixer.reports()) {
            total.playbacks_completed += u32::from(report.playback().completed().is_some());
            total.loops_completed += report.playback().loops_completed();
            total.transitions_completed += u32::from(report.playback().transition_completed());
            total.weight_fades_completed += u32::from(report.weight_fade_completed());
        }
    };

    let (asset, mut single_skeleton) = mixer_fixture();
    let mut split_skeleton = Skeleton::new(Arc::clone(&asset));
    let walk = asset.animation_id("walk").expect("walk exists");
    let look = asset.animation_id("look").expect("look exists");
    let root = asset.bone_id("root").expect("root exists");
    let aim_bone = asset.bone_id("aim").expect("aim bone exists");
    let transition = Transition::Crossfade(Crossfade::new(Duration::from_millis(500)));
    let mut single = AnimationMixer::new(&single_skeleton);
    let mut split = AnimationMixer::new(&split_skeleton);

    let configure = |mixer: &mut AnimationMixer| {
        mixer
            .base_track_mut()
            .play(walk, PlayOptions::looping())
            .expect("walk belongs to the mixer");
        let track = mixer
            .insert_track(TrackOptions::override_track().with_weight(Mix::ZERO))
            .expect("track identity remains available");
        let mut track_mut = mixer.track_mut(track).expect("track exists");
        track_mut
            .play(look, PlayOptions::looping().with_transition(transition))
            .expect("look belongs to the mixer");
        track_mut.fade_weight(Mix::ONE, WeightFade::new(Duration::from_millis(500)));
        track
    };
    let single_track = configure(&mut single);
    let split_track = configure(&mut split);

    let mut single_events = Vec::new();
    let single_frame = single
        .update(
            &mut single_skeleton,
            Duration::from_secs(1),
            &mut |event: TrackAnimationEvent<'_>| {
                single_events.push((
                    event.track() == single_track,
                    event.event().definition().name().to_owned(),
                    event.event().loop_index(),
                    event.event().local_time(),
                ));
            },
        )
        .expect("single update succeeds")
        .solve();
    let single_pose = (
        single_frame
            .bone(root)
            .expect("root belongs to the skeleton")
            .local_transform(),
        single_frame
            .bone(aim_bone)
            .expect("aim belongs to the skeleton")
            .local_transform(),
    );
    drop(single_frame);
    let mut single_lifecycle = vec![LifecycleTotals::default(); single.reports().count()];
    accumulate_reports(&mut single_lifecycle, &single);

    let mut split_events = Vec::new();
    let mut split_lifecycle = vec![LifecycleTotals::default(); split.reports().count()];
    for delta in [Duration::from_millis(600), Duration::from_millis(400)] {
        let _frame = split
            .update(
                &mut split_skeleton,
                delta,
                &mut |event: TrackAnimationEvent<'_>| {
                    split_events.push((
                        event.track() == split_track,
                        event.event().definition().name().to_owned(),
                        event.event().loop_index(),
                        event.event().local_time(),
                    ));
                },
            )
            .expect("split update succeeds")
            .solve();
        accumulate_reports(&mut split_lifecycle, &split);
    }
    let split_frame = split_skeleton.editable_pose().solve();
    let split_pose = (
        split_frame
            .bone(root)
            .expect("root belongs to the skeleton")
            .local_transform(),
        split_frame
            .bone(aim_bone)
            .expect("aim belongs to the skeleton")
            .local_transform(),
    );
    drop(split_frame);

    assert_eq!(single_pose, split_pose);
    assert_eq!(single_events, split_events);
    assert_eq!(
        single.base_track().status().position(),
        split.base_track().status().position()
    );
    assert_eq!(
        single
            .track(single_track)
            .expect("single track exists")
            .status()
            .position(),
        split
            .track(split_track)
            .expect("split track exists")
            .status()
            .position()
    );
    assert_eq!(
        single
            .track(single_track)
            .expect("single track exists")
            .weight(),
        split
            .track(split_track)
            .expect("split track exists")
            .weight()
    );
    assert_eq!(single_lifecycle, split_lifecycle);
}

#[test]
fn unit_speed_preserves_huge_wall_deltas_exactly_across_tiny_loops() {
    let asset = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[{"name":"root"}],
          "animations":{
            "tick":{
              "bones":{"root":{"translate":[
                {"x":0},
                {"time":0.000000001,"x":1}
              ]}}
            }
          }
        }"#,
        b"page.png\n",
    )
    .expect("the nanosecond loop fixture loads")
    .into_asset();
    let tick = asset.animation_id("tick").expect("tick exists");
    let mut single_skeleton = Skeleton::new(Arc::clone(&asset));
    let mut split_skeleton = Skeleton::new(Arc::clone(&asset));
    let mut single = AnimationMixer::new(&single_skeleton);
    let mut split = AnimationMixer::new(&split_skeleton);
    let configure = |mixer: &mut AnimationMixer| {
        mixer
            .base_track_mut()
            .play(tick, PlayOptions::looping())
            .expect("tick belongs to the mixer");
        let track = mixer
            .insert_track(TrackOptions::override_track().with_weight(Mix::ZERO))
            .expect("track identity remains available");
        mixer
            .track_mut(track)
            .expect("track exists")
            .play(tick, PlayOptions::looping())
            .expect("tick belongs to the mixer");
        track
    };
    let single_track = configure(&mut single);
    let split_track = configure(&mut split);
    let huge_seconds = 1_u64 << 53;
    let whole = Duration::new(huge_seconds, 1);

    let _single_frame = single
        .update(&mut single_skeleton, whole, &mut ())
        .expect("one huge update remains representable")
        .solve();
    for delta in [Duration::from_secs(huge_seconds), Duration::from_nanos(1)] {
        let _split_frame = split
            .update(&mut split_skeleton, delta, &mut ())
            .expect("split huge updates remain representable")
            .solve();
    }

    assert_eq!(
        single.base_track().status().loop_index(),
        split.base_track().status().loop_index(),
        "unit speed must not round-trip exact Duration values through f64"
    );
    assert_eq!(
        single
            .track(single_track)
            .expect("single track exists")
            .status()
            .loop_index(),
        split
            .track(split_track)
            .expect("split track exists")
            .status()
            .loop_index(),
        "override tracks use the same exact unit-speed clock"
    );
}

#[test]
fn scaled_duration_overflow_is_rejected_before_base_mixer_mutation() {
    let (asset, mut skeleton) = mixer_fixture();
    let fall = asset.animation_id("fall").expect("fall exists");
    let look = asset.animation_id("look").expect("look exists");
    let root = asset.bone_id("root").expect("root exists");
    let mut mixer = AnimationMixer::new(&skeleton);
    {
        let mut base = mixer.base_track_mut();
        base.play(fall, PlayOptions::looping())
            .expect("fall belongs to the mixer");
        base.set_speed(2.0).expect("two-times speed is valid");
    }
    let track = mixer
        .insert_track(TrackOptions::override_track().with_weight(Mix::ZERO))
        .expect("track identity remains available");
    {
        let mut override_track = mixer.track_mut(track).expect("track exists");
        override_track
            .play(look, PlayOptions::once())
            .expect("look belongs to the mixer");
        override_track.fade_weight(Mix::ONE, WeightFade::new(Duration::from_secs(1)));
    }
    let base_before = mixer.base_track().status();
    let override_before = mixer.track(track).expect("track exists").status();
    let weight_before = mixer.track(track).expect("track exists").weight();
    let reports_before = mixer.reports().collect::<Vec<_>>();
    let pose_before = skeleton
        .bone_pose(root)
        .expect("root belongs to the skeleton")
        .local_transform();
    let mut events = Vec::new();

    let error = mixer
        .update(
            &mut skeleton,
            Duration::MAX,
            &mut |event: TrackAnimationEvent<'_>| events.push(event.track()),
        )
        .expect_err("scaled playback time does not fit Duration");

    assert_eq!(error, PlayerError::TimeOverflow);
    assert!(events.is_empty());
    assert_eq!(mixer.base_track().status(), base_before);
    assert_eq!(
        mixer.track(track).expect("track remains present").status(),
        override_before
    );
    assert_eq!(
        mixer.track(track).expect("track remains present").weight(),
        weight_before
    );
    assert_eq!(mixer.reports().collect::<Vec<_>>(), reports_before);
    assert_eq!(
        skeleton
            .bone_pose(root)
            .expect("root remains available")
            .local_transform(),
        pose_before
    );
}

#[test]
fn later_override_scaled_duration_overflow_is_atomic_for_the_whole_mixer() {
    let (asset, mut skeleton) = mixer_fixture();
    let walk = asset.animation_id("walk").expect("walk exists");
    let look = asset.animation_id("look").expect("look exists");
    let look_back = asset.animation_id("look-back").expect("look-back exists");
    let root = asset.bone_id("root").expect("root exists");
    let aim = asset.bone_id("aim").expect("aim exists");
    let mut mixer = AnimationMixer::new(&skeleton);
    mixer
        .base_track_mut()
        .play(walk, PlayOptions::once())
        .expect("walk belongs to the mixer");
    let earlier = mixer
        .insert_track(TrackOptions::override_track().with_weight(Mix::ZERO))
        .expect("earlier track identity remains available");
    {
        let mut track = mixer.track_mut(earlier).expect("earlier track exists");
        track
            .play(look, PlayOptions::once())
            .expect("look belongs to the mixer");
        track.fade_weight(Mix::ONE, WeightFade::new(Duration::from_secs(1)));
    }
    let later = mixer
        .insert_track(TrackOptions::override_track().with_weight(Mix::ZERO))
        .expect("later track identity remains available");
    {
        let mut track = mixer.track_mut(later).expect("later track exists");
        track
            .play(look_back, PlayOptions::looping())
            .expect("look-back belongs to the mixer");
        track.set_speed(2.0).expect("two-times speed is valid");
        track.fade_weight(Mix::ONE, WeightFade::new(Duration::from_secs(1)));
    }
    let base_before = mixer.base_track().status();
    let earlier_before = mixer.track(earlier).expect("earlier track exists").status();
    let later_before = mixer.track(later).expect("later track exists").status();
    let weights_before = [
        mixer.track(earlier).expect("earlier track exists").weight(),
        mixer.track(later).expect("later track exists").weight(),
    ];
    let reports_before = mixer.reports().collect::<Vec<_>>();
    let pose_before = [
        skeleton
            .bone_pose(root)
            .expect("root belongs to the skeleton")
            .local_transform(),
        skeleton
            .bone_pose(aim)
            .expect("aim belongs to the skeleton")
            .local_transform(),
    ];
    let mut events = Vec::new();

    let error = mixer
        .update(
            &mut skeleton,
            Duration::MAX,
            &mut |event: TrackAnimationEvent<'_>| events.push(event.track()),
        )
        .expect_err("the later override's scaled playback time does not fit Duration");

    assert_eq!(error, PlayerError::TimeOverflow);
    assert!(
        events.is_empty(),
        "late preflight failure must not emit earlier-track events"
    );
    assert_eq!(mixer.base_track().status(), base_before);
    assert_eq!(
        mixer
            .track(earlier)
            .expect("earlier track remains present")
            .status(),
        earlier_before
    );
    assert_eq!(
        mixer
            .track(later)
            .expect("later track remains present")
            .status(),
        later_before
    );
    assert_eq!(
        [
            mixer
                .track(earlier)
                .expect("earlier track remains present")
                .weight(),
            mixer
                .track(later)
                .expect("later track remains present")
                .weight(),
        ],
        weights_before
    );
    assert_eq!(mixer.reports().collect::<Vec<_>>(), reports_before);
    assert_eq!(
        [
            skeleton
                .bone_pose(root)
                .expect("root remains available")
                .local_transform(),
            skeleton
                .bone_pose(aim)
                .expect("aim remains available")
                .local_transform(),
        ],
        pose_before
    );
}

#[test]
fn scaled_duration_overflow_is_irrelevant_to_idle_tracks() {
    let (_asset, mut skeleton) = mixer_fixture();
    let mut mixer = AnimationMixer::new(&skeleton);
    mixer
        .base_track_mut()
        .set_speed(2.0)
        .expect("two-times speed is valid");
    let track = mixer
        .insert_track(TrackOptions::override_track().with_weight(Mix::ZERO))
        .expect("track identity remains available");
    {
        let mut override_track = mixer.track_mut(track).expect("track exists");
        override_track
            .set_speed(2.0)
            .expect("two-times speed is valid");
        override_track.fade_weight(Mix::ONE, WeightFade::new(Duration::from_secs(1)));
    }

    let _frame = mixer
        .update(&mut skeleton, Duration::MAX, &mut ())
        .expect("idle tracks have no scaled animation clock to overflow")
        .solve();

    assert!(mixer.base_track().status().is_idle());
    assert!(
        mixer
            .track(track)
            .expect("track remains present")
            .status()
            .is_idle()
    );
    assert_eq!(
        mixer.track(track).expect("track remains present").weight(),
        Mix::ONE,
        "wall-clock weight fades remain independent of playback time"
    );
}

#[test]
fn failed_whole_mixer_update_changes_no_track_clock_weight_pose_or_event_output() {
    let (asset, skeleton) = mixer_fixture();
    let mut foreign = Skeleton::new(Arc::clone(&asset));
    let walk = asset.animation_id("walk").expect("walk exists");
    let look = asset.animation_id("look").expect("look exists");
    let root = asset.bone_id("root").expect("root exists");
    let mut mixer = AnimationMixer::new(&skeleton);
    mixer
        .base_track_mut()
        .play(walk, PlayOptions::looping())
        .expect("walk belongs to the mixer");
    let track = mixer
        .insert_track(TrackOptions::override_track().with_weight(Mix::ZERO))
        .expect("track identity remains available");
    {
        let mut track_mut = mixer.track_mut(track).expect("track exists");
        track_mut
            .play(look, PlayOptions::looping())
            .expect("look belongs to the mixer");
        track_mut.fade_weight(Mix::ONE, WeightFade::new(Duration::from_secs(1)));
    }
    let base_before = mixer.base_track().status();
    let track_before = mixer.track(track).expect("track exists").status();
    let weight_before = mixer.track(track).expect("track exists").weight();
    let foreign_pose_before = foreign
        .bone_pose(root)
        .expect("root belongs to the foreign instance")
        .local_transform();
    let mut events = Vec::new();

    assert_eq!(
        mixer
            .update(
                &mut foreign,
                Duration::from_millis(500),
                &mut |event: TrackAnimationEvent<'_>| events.push(event.track()),
            )
            .expect_err("a mixer is bound to exactly one skeleton instance"),
        PlayerError::ForeignSkeleton
    );
    assert!(events.is_empty());
    assert_eq!(mixer.base_track().status(), base_before);
    assert_eq!(
        mixer.track(track).expect("track remains present").status(),
        track_before
    );
    assert_eq!(
        mixer.track(track).expect("track remains present").weight(),
        weight_before
    );
    assert_eq!(
        foreign
            .bone_pose(root)
            .expect("root remains present")
            .local_transform(),
        foreign_pose_before
    );
}

#[test]
fn late_override_event_preflight_failure_is_atomic_for_the_whole_mixer() {
    let asset = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[{"name":"root"}],
          "events":{"pulse":{}},
          "animations":{
            "quiet":{
              "bones":{"root":{"translate":[
                {"x":0},
                {"time":1,"x":1}
              ]}}
            },
            "noisy":{
              "bones":{"root":{"rotate":[
                {"value":0},
                {"time":0.001,"value":1}
              ]}},
              "events":[{"time":0.0005,"name":"pulse"}]
            }
          }
        }"#,
        b"page.png\n",
    )
    .expect("the event-budget mixer fixture loads")
    .into_asset();
    let quiet = asset.animation_id("quiet").expect("quiet exists");
    let noisy = asset.animation_id("noisy").expect("noisy exists");
    let root = asset.bone_id("root").expect("root exists");
    let mut skeleton = Skeleton::new(Arc::clone(&asset));
    let mut mixer = AnimationMixer::new(&skeleton);
    mixer
        .base_track_mut()
        .play(quiet, PlayOptions::looping())
        .expect("quiet belongs to the mixer");
    let track = mixer
        .insert_track(TrackOptions::override_track().with_weight(Mix::ZERO))
        .expect("track identity remains available");
    {
        let mut override_track = mixer.track_mut(track).expect("track exists");
        override_track
            .play(noisy, PlayOptions::looping())
            .expect("noisy belongs to the mixer");
        override_track.fade_weight(Mix::ONE, WeightFade::new(Duration::from_secs(1)));
    }
    let base_before = mixer.base_track().status();
    let override_before = mixer.track(track).expect("track exists").status();
    let weight_before = mixer.track(track).expect("track exists").weight();
    let reports_before = mixer.reports().collect::<Vec<_>>();
    let pose_before = skeleton
        .bone_pose(root)
        .expect("root belongs to the skeleton")
        .local_transform();
    let mut events = Vec::new();

    let error = mixer
        .update(
            &mut skeleton,
            Duration::from_secs(66),
            &mut |event: TrackAnimationEvent<'_>| events.push(event.track()),
        )
        .expect_err("the late override exceeds the fixed event budget");

    assert!(matches!(error, PlayerError::EventLimitExceeded { .. }));
    assert!(
        events.is_empty(),
        "preflight must not partially fill the sink"
    );
    assert_eq!(mixer.base_track().status(), base_before);
    assert_eq!(
        mixer.track(track).expect("track remains present").status(),
        override_before
    );
    assert_eq!(
        mixer.track(track).expect("track remains present").weight(),
        weight_before
    );
    assert_eq!(mixer.reports().collect::<Vec<_>>(), reports_before);
    assert_eq!(
        skeleton
            .bone_pose(root)
            .expect("root remains available")
            .local_transform(),
        pose_before
    );
}

#[test]
fn skeleton_space_targets_use_the_current_rotated_and_reflected_parent_pose() {
    let asset = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[
            {
              "name":"root",
              "x":7,
              "y":-3,
              "rotation":90,
              "scaleX":-2,
              "scaleY":0.5
            },
            {"name":"crosshair","parent":"root","x":1,"y":2}
          ],
          "animations":{
            "turn":{
              "bones":{
                "root":{
                  "rotate":[
                    {"value":0},
                    {"time":1,"value":90}
                  ]
                }
              }
            }
          }
        }"#,
        b"page.png\n",
    )
    .expect("the reflected target fixture loads")
    .into_asset();
    let turn = asset.animation_id("turn").expect("turn exists");
    let crosshair = asset.bone_id("crosshair").expect("crosshair exists");
    let mut skeleton = Skeleton::new(Arc::clone(&asset));
    let mut mixer = AnimationMixer::new(&skeleton);
    mixer
        .base_track_mut()
        .play(turn, PlayOptions::once())
        .expect("turn belongs to the mixer");

    let destination = Vec2::new(41.0, -17.0);
    let mut pose = mixer
        .update(&mut skeleton, Duration::from_millis(500), &mut ())
        .expect("the target frame mixes");
    pose.targets()
        .set_skeleton_position(crosshair, destination)
        .expect("the reflected parent remains invertible");
    let frame = pose.solve();
    let actual = frame
        .bone(crosshair)
        .expect("crosshair belongs to the skeleton")
        .world_transform()
        .translation();
    assert!((actual.x - destination.x).abs() < 1.0e-4);
    assert!((actual.y - destination.y).abs() < 1.0e-4);
}

#[test]
fn track_ids_reject_foreign_mixers_and_remain_stale_after_removal() {
    let (_asset_a, skeleton_a) = mixer_fixture();
    let (_asset_b, skeleton_b) = mixer_fixture();
    let mut first = AnimationMixer::new(&skeleton_a);
    let mut second = AnimationMixer::new(&skeleton_b);
    let track = first
        .insert_track(TrackOptions::override_track())
        .expect("the mixer has track identity capacity");

    assert_eq!(
        second
            .track_mut(track)
            .expect_err("track is mixer-scoped")
            .kind(),
        TrackErrorKind::ForeignMixer
    );
    first
        .remove_track(track)
        .expect("the inserted override track can be removed");
    assert_eq!(
        first
            .track_mut(track)
            .expect_err("removed IDs stay stale")
            .kind(),
        TrackErrorKind::Removed
    );
}

#[test]
fn immutable_track_observation_and_reordering_preserve_live_playback() {
    let (asset, skeleton) = mixer_fixture();
    let look = asset.animation_id("look").expect("look exists");
    let look_back = asset.animation_id("look-back").expect("look-back exists");
    let mut mixer = AnimationMixer::new(&skeleton);
    let lower = mixer
        .insert_track(TrackOptions::override_track())
        .expect("track identity remains available");
    let higher = mixer
        .insert_track(TrackOptions::override_track())
        .expect("track identity remains available");
    let lower_playback = mixer
        .track_mut(lower)
        .expect("lower track exists")
        .play(look, PlayOptions::looping())
        .expect("look belongs to the mixer")
        .playback();
    let higher_playback = mixer
        .track_mut(higher)
        .expect("higher track exists")
        .play(look_back, PlayOptions::looping())
        .expect("look-back belongs to the mixer")
        .playback();

    assert_eq!(mixer.len(), 2);
    assert!(!mixer.is_empty());
    assert_eq!(mixer.tracks().collect::<Vec<_>>(), [lower, higher]);
    assert_eq!(
        mixer
            .track(lower)
            .expect("lower track remains observable")
            .status()
            .playback(),
        Some(lower_playback)
    );

    mixer
        .move_track(higher, 0)
        .expect("an existing track can move to a valid priority");
    assert_eq!(mixer.tracks().collect::<Vec<_>>(), [higher, lower]);
    assert_eq!(
        mixer
            .track(higher)
            .expect("moved track remains observable")
            .status()
            .playback(),
        Some(higher_playback),
        "reordering must not restart playback"
    );
    assert_eq!(
        mixer
            .move_track(lower, 2)
            .expect_err("the end-exclusive index is invalid")
            .kind(),
        TrackErrorKind::OrderOutOfBounds
    );
    assert_eq!(
        mixer.tracks().collect::<Vec<_>>(),
        [higher, lower],
        "an invalid reorder is failure-atomic"
    );
}

#[test]
fn deferred_diagnostics_follow_presented_crossfade_sources_and_visible_weight() {
    let asset = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[{"name":"root"}],
          "slots":[{"name":"body","bone":"root"}],
          "animations":{
            "attachment":{"slots":{"body":{"attachment":[{"name":null}]}}},
            "continuous":{"bones":{"root":{"rotate":[{"value":10}]}}}
          }
        }"#,
        b"page.png\n",
    )
    .expect("the deferred-property fixture loads")
    .into_asset();
    let attachment = asset
        .animation_id("attachment")
        .expect("attachment animation exists");
    let continuous = asset
        .animation_id("continuous")
        .expect("continuous animation exists");
    let mut skeleton = Skeleton::new(Arc::clone(&asset));
    let mut mixer = AnimationMixer::new(&skeleton);
    let track = mixer
        .insert_track(TrackOptions::override_track())
        .expect("track identity remains available");
    mixer
        .track_mut(track)
        .expect("track exists")
        .play(attachment, PlayOptions::looping())
        .expect("attachment animation belongs to the mixer");
    let _frame = mixer
        .update(&mut skeleton, Duration::ZERO, &mut ())
        .expect("initial contribution samples")
        .solve();
    assert!(mixer.has_degraded_overrides());

    mixer
        .track_mut(track)
        .expect("track exists")
        .play(
            continuous,
            PlayOptions::looping().with_transition(Transition::Crossfade(Crossfade::new(
                Duration::from_secs(1),
            ))),
        )
        .expect("continuous animation belongs to the mixer");
    let _frame = mixer
        .update(&mut skeleton, Duration::from_millis(500), &mut ())
        .expect("outgoing contribution remains presented")
        .solve();
    assert!(
        mixer
            .active_deferred_properties()
            .any(|issue| issue.animation() == attachment),
        "an outgoing animation remains diagnosable while it contributes"
    );

    mixer
        .track_mut(track)
        .expect("track exists")
        .set_weight(Mix::ZERO);
    assert!(
        !mixer.has_degraded_overrides(),
        "a zero-weight track has no visible unsupported contribution"
    );
}

#[derive(Clone, Copy, Debug)]
enum ReferenceClip {
    Look,
    LookBack,
    Empty,
}

impl ReferenceClip {
    const fn contribution(self) -> ReferenceContribution {
        match self {
            Self::Look => ReferenceContribution {
                value: 60.0,
                influence: 1.0,
            },
            Self::LookBack => ReferenceContribution {
                value: -60.0,
                influence: 1.0,
            },
            Self::Empty => ReferenceContribution {
                value: 0.0,
                influence: 0.0,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct ReferenceContribution {
    value: f32,
    influence: f32,
}

impl ReferenceContribution {
    fn mix(source: Self, target: Self, amount: f32) -> Self {
        let source_weight = (1.0 - amount) * source.influence;
        let target_weight = amount * target.influence;
        let influence = source_weight + target_weight;
        if influence == 0.0 {
            return Self::default();
        }
        let target_share = target_weight / influence;
        let source_value = if source.influence == 0.0 {
            target.value
        } else {
            source.value
        };
        let target_value = if target.influence == 0.0 {
            source.value
        } else {
            target.value
        };
        Self {
            value: source_value + (target_value - source_value) * target_share,
            influence,
        }
    }

    fn apply(self, lower: f32, weight: f32) -> f32 {
        lower + (self.value - lower) * self.influence * weight
    }
}

#[derive(Clone, Copy, Debug)]
struct ReferenceTrack {
    active: Option<ReferenceClip>,
    presented: ReferenceContribution,
    transition_source: ReferenceContribution,
    transition_elapsed: Duration,
    transition_duration: Duration,
    weight: f32,
    weight_fade_source: f32,
    weight_fade_target: f32,
    weight_fade_elapsed: Duration,
    weight_fade_duration: Duration,
}

impl ReferenceTrack {
    fn new(active: ReferenceClip) -> Self {
        Self {
            active: Some(active),
            presented: active.contribution(),
            transition_source: ReferenceContribution::default(),
            transition_elapsed: Duration::ZERO,
            transition_duration: Duration::ZERO,
            weight: 1.0,
            weight_fade_source: 1.0,
            weight_fade_target: 1.0,
            weight_fade_elapsed: Duration::ZERO,
            weight_fade_duration: Duration::ZERO,
        }
    }

    fn play(&mut self, active: ReferenceClip, transition_duration: Duration) {
        self.transition_source = self.presented;
        self.active = Some(active);
        self.transition_elapsed = Duration::ZERO;
        self.transition_duration = transition_duration;
    }

    fn restart(&mut self) {
        if let Some(active) = self.active {
            self.play(active, Duration::ZERO);
        }
    }

    fn stop(&mut self, transition_duration: Duration) {
        if self.active.is_none() {
            return;
        }
        self.transition_source = self.presented;
        self.active = None;
        self.transition_elapsed = Duration::ZERO;
        self.transition_duration = transition_duration;
    }

    fn set_weight(&mut self, weight: f32) {
        self.weight = weight;
        self.weight_fade_duration = Duration::ZERO;
    }

    fn fade_weight(&mut self, target: f32, duration: Duration) {
        if duration.is_zero() || self.weight == target {
            self.set_weight(target);
            return;
        }
        self.weight_fade_source = self.weight;
        self.weight_fade_target = target;
        self.weight_fade_elapsed = Duration::ZERO;
        self.weight_fade_duration = duration;
    }

    fn update(&mut self, delta: Duration) {
        if !self.weight_fade_duration.is_zero() {
            self.weight_fade_elapsed = self
                .weight_fade_elapsed
                .saturating_add(delta)
                .min(self.weight_fade_duration);
            let amount =
                self.weight_fade_elapsed.as_secs_f32() / self.weight_fade_duration.as_secs_f32();
            self.weight = self.weight_fade_source
                + (self.weight_fade_target - self.weight_fade_source) * amount;
            if self.weight_fade_elapsed == self.weight_fade_duration {
                self.weight_fade_duration = Duration::ZERO;
            }
        }
        let sampled = self
            .active
            .map_or_else(ReferenceContribution::default, ReferenceClip::contribution);
        if self.transition_duration.is_zero() {
            self.presented = sampled;
            return;
        }
        self.transition_elapsed = self
            .transition_elapsed
            .saturating_add(delta)
            .min(self.transition_duration);
        let amount = self.transition_elapsed.as_secs_f32() / self.transition_duration.as_secs_f32();
        self.presented = ReferenceContribution::mix(self.transition_source, sampled, amount);
        if self.transition_elapsed == self.transition_duration {
            self.transition_duration = Duration::ZERO;
        }
    }
}

#[test]
fn generated_action_trace_matches_a_slow_dense_reference_compositor() {
    let (asset, mut skeleton) = mixer_fixture();
    let walk = asset.animation_id("walk").expect("walk exists");
    let look = asset.animation_id("look").expect("look exists");
    let look_back = asset.animation_id("look-back").expect("look-back exists");
    let empty = asset.animation_id("body-only").expect("body-only exists");
    let aim_bone = asset.bone_id("aim").expect("aim bone exists");
    let mut mixer = AnimationMixer::new(&skeleton);
    mixer
        .base_track_mut()
        .play(walk, PlayOptions::once())
        .expect("walk belongs to the mixer");
    let first = mixer
        .insert_track(TrackOptions::override_track())
        .expect("first track exists");
    let second = mixer
        .insert_track(TrackOptions::override_track())
        .expect("second track exists");
    mixer
        .track_mut(first)
        .expect("first track exists")
        .play(look, PlayOptions::looping())
        .expect("look belongs to the mixer");
    mixer
        .track_mut(second)
        .expect("second track exists")
        .play(look_back, PlayOptions::looping())
        .expect("look-back belongs to the mixer");
    let _frame = mixer
        .update(&mut skeleton, Duration::ZERO, &mut ())
        .expect("initial sparse values sample")
        .solve();

    let mut reference = [
        ReferenceTrack::new(ReferenceClip::Look),
        ReferenceTrack::new(ReferenceClip::LookBack),
    ];
    let mut order = [0_usize, 1_usize];
    let mut base_elapsed = Duration::ZERO;
    let mut random = 0xD1CE_BA5E_u64;
    let clips = [
        (look, ReferenceClip::Look),
        (look_back, ReferenceClip::LookBack),
        (empty, ReferenceClip::Empty),
    ];

    for step in 0..256 {
        random = random
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let track_index = ((random >> 8) & 1) as usize;
        match (random >> 16) % 7 {
            0 => {
                let weight = f32::from(((random >> 24) & 0xff) as u8) / 255.0;
                let weight = Mix::clamped(weight).expect("generated weight is normalized");
                mixer
                    .track_mut([first, second][track_index])
                    .expect("generated track exists")
                    .set_weight(weight);
                reference[track_index].set_weight(weight.get());
            }
            1 => {
                let clip = clips[((random >> 32) % clips.len() as u64) as usize];
                let duration = Duration::from_millis(200);
                mixer
                    .track_mut([first, second][track_index])
                    .expect("generated track exists")
                    .play(
                        clip.0,
                        PlayOptions::looping()
                            .with_transition(Transition::Crossfade(Crossfade::new(duration))),
                    )
                    .expect("generated animation belongs to the mixer");
                reference[track_index].play(clip.1, duration);
            }
            2 => {
                order.swap(0, 1);
                mixer
                    .move_track([first, second][order[0]], 0)
                    .expect("generated priority is valid");
            }
            3 => {
                let target = f32::from(((random >> 24) & 0xff) as u8) / 255.0;
                let target = Mix::clamped(target).expect("generated weight is normalized");
                let duration = Duration::from_millis(200);
                mixer
                    .track_mut([first, second][track_index])
                    .expect("generated track exists")
                    .fade_weight(target, WeightFade::new(duration));
                reference[track_index].fade_weight(target.get(), duration);
            }
            4 => {
                let duration = Duration::from_millis(200);
                mixer
                    .track_mut([first, second][track_index])
                    .expect("generated track exists")
                    .stop(Transition::Crossfade(Crossfade::new(duration)));
                reference[track_index].stop(duration);
            }
            5 => {
                mixer
                    .track_mut([first, second][track_index])
                    .expect("generated track exists")
                    .restart()
                    .expect("generated restart remains asset-local");
                reference[track_index].restart();
            }
            _other => {}
        }

        let delta = Duration::from_millis([0_u64, 17, 50, 93][((random >> 40) & 3) as usize]);
        base_elapsed = base_elapsed
            .saturating_add(delta)
            .min(Duration::from_secs(1));
        for track in &mut reference {
            track.update(delta);
        }
        let frame = mixer
            .update(&mut skeleton, delta, &mut ())
            .expect("generated mixer update succeeds")
            .solve();

        let mut expected = 20.0 * base_elapsed.as_secs_f32();
        for index in order {
            expected = reference[index]
                .presented
                .apply(expected, reference[index].weight);
        }
        let actual = frame
            .bone(aim_bone)
            .expect("aim belongs to the skeleton")
            .local_transform()
            .translation()
            .x;
        assert!(
            (actual - expected).abs() < 2.0e-3,
            "generated step {step} expected {expected}, got {actual}; \
             order {order:?}, reference {reference:?}"
        );
        for (id, reference) in [first, second].into_iter().zip(&reference) {
            let actual_weight = mixer
                .track(id)
                .expect("generated track remains present")
                .weight()
                .get();
            assert!(
                (actual_weight - reference.weight).abs() < 1.0e-5,
                "generated step {step} track {id:?} expected weight {}, got {actual_weight}",
                reference.weight
            );
        }
    }
}

#[test]
fn active_base_and_overrides_allocate_nothing_after_warmup() {
    let (asset, mut skeleton) = mixer_fixture();
    let walk = asset.animation_id("walk").expect("walk exists");
    let look = asset.animation_id("look").expect("look exists");
    let look_back = asset.animation_id("look-back").expect("look-back exists");
    let mut mixer = AnimationMixer::new(&skeleton);
    mixer
        .base_track_mut()
        .play(walk, PlayOptions::looping())
        .expect("walk belongs to the mixer");
    for animation in [look, look_back] {
        let track = mixer
            .insert_track(TrackOptions::override_track())
            .expect("track identity remains available");
        mixer
            .track_mut(track)
            .expect("track exists")
            .play(animation, PlayOptions::looping())
            .expect("animation belongs to the mixer");
    }
    for _frame in 0..4 {
        let _solved = mixer
            .update(&mut skeleton, Duration::from_millis(16), &mut ())
            .expect("warmup succeeds")
            .solve();
    }

    let allocations = allocation_counter::measure(|| {
        for _frame in 0..120 {
            let _solved = mixer
                .update(&mut skeleton, Duration::from_millis(16), &mut ())
                .expect("steady-state update succeeds")
                .solve();
        }
    });
    assert_eq!(allocations.count_total, 0);
    assert_eq!(allocations.bytes_total, 0);
}

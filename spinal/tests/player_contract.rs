//! Public contract tests for the Stage 4 stateful one-track player.

use std::{sync::Arc, time::Duration};

use spinal::{
    Angle, AnimationEvent, AnimationPlayer, BoneTransform, Crossfade, DiscreteSwitches, Mix,
    PlayOptions, PlayerError, Skeleton, TransformMix, Transition, load_json,
};

const ATLAS: &str = "\
cat.png
	size: 16, 8
body
	bounds: 0, 0, 8, 8
alternate
	bounds: 8, 0, 8, 8
";

const JSON: &str = r#"{
  "skeleton":{"spine":"4.3.23"},
  "bones":[
    {"name":"root"},
    {"name":"cat","parent":"root","length":10},
    {"name":"target","parent":"root","x":10}
  ],
  "slots":[{"name":"body-slot","bone":"cat","attachment":"body"}],
  "skins":[
    {
      "name":"default",
      "attachments":{
        "body-slot":{
          "body":{"path":"body","width":8,"height":8},
          "alternate":{"path":"alternate","width":8,"height":8}
        }
      }
    },
    {
      "name":"alternate-skin",
      "attachments":{
        "body-slot":{
          "body":{"path":"alternate","width":8,"height":8}
        }
      }
    }
  ],
  "events":{
    "start":{},
    "middle":{"int":7,"futurePayload":true},
    "end":{},
    "same-a":{},
    "same-b":{},
    "target-start":{},
    "target-middle":{},
    "target-end":{},
    "interrupt-start":{},
    "interrupt-middle":{},
    "interrupt-end":{}
  },
  "animations":{
    "idle":{
      "bones":{"cat":{"rotate":[{"value":0},{"time":1,"value":0}]}},
      "events":[
        {"name":"start"},
        {"time":0.5,"name":"middle"},
        {"time":1,"name":"end"}
      ]
    },
    "fall":{
      "bones":{"cat":{"rotate":[{"value":90},{"time":1,"value":90}]}}
    },
    "jump":{
      "bones":{"cat":{"rotate":[{"value":120},{"time":1,"value":120}]}}
    },
    "alternate":{
      "slots":{"body-slot":{"attachment":[{"name":"alternate"},{"time":1,"name":"alternate"}]}}
    },
    "mirror":{
      "bones":{"cat":{"scale":[{"x":-1,"y":1},{"time":1,"x":-1,"y":1}]}}
    },
    "zero":{
      "events":[{"name":"middle","int":9,"float":1.5,"string":"override"}]
    },
    "same-time":{
      "events":[
        {"time":0.5,"name":"same-a"},
        {"time":0.5,"name":"same-b"}
      ]
    },
    "event-target":{
      "events":[
        {"name":"target-start"},
        {"time":0.4,"name":"target-middle"},
        {"time":0.8,"name":"target-end"}
      ]
    },
    "event-interrupt":{
      "events":[
        {"name":"interrupt-start"},
        {"time":0.25,"name":"interrupt-middle"},
        {"time":0.75,"name":"interrupt-end"}
      ]
    }
  }
}"#;

fn fixture() -> (Arc<spinal::SkeletonAsset>, Skeleton) {
    let asset = load_json(JSON.as_bytes(), ATLAS.as_bytes())
        .expect("the player fixture should load")
        .into_asset();
    let skeleton = Skeleton::new(Arc::clone(&asset));
    (asset, skeleton)
}

#[test]
fn player_crossfades_into_an_editable_then_solved_pose() {
    let (asset, mut skeleton) = fixture();
    let idle = asset.animation_id("idle").expect("animation exists");
    let fall = asset.animation_id("fall").expect("animation exists");
    let cat = asset.bone_id("cat").expect("bone exists");
    let mut player = AnimationPlayer::new(&skeleton);

    player
        .play(idle, PlayOptions::looping())
        .expect("animation belongs to the bound skeleton");
    let frame = player
        .update(&mut skeleton, Duration::ZERO, &mut ())
        .expect("the player remains bound to this skeleton")
        .solve();
    assert!(
        frame
            .bone(cat)
            .expect("asset-local bone")
            .local_transform()
            .rotation()
            .as_degrees()
            .abs()
            < 1.0e-4
    );
    drop(frame);

    player
        .play(
            fall,
            PlayOptions::once().with_transition(Transition::Crossfade(Crossfade::new(
                Duration::from_secs(1),
            ))),
        )
        .expect("animation belongs to the bound skeleton");
    let frame = player
        .update(&mut skeleton, Duration::from_millis(500), &mut ())
        .expect("the player remains bound to this skeleton")
        .solve();
    assert!(
        (frame
            .bone(cat)
            .expect("asset-local bone")
            .local_transform()
            .rotation()
            .as_degrees()
            - 45.0)
            .abs()
            < 1.0e-4
    );
}

#[test]
fn transform_constraint_mix_crossfades_with_the_rest_of_the_pose() {
    let asset = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[
            {"name":"root"},
            {"name":"source","parent":"root","rotation":90},
            {"name":"constrained","parent":"root"}
          ],
          "constraints":[{
            "type":"transform",
            "name":"copy",
            "source":"source",
            "bones":["constrained"],
            "properties":{"rotate":{"to":{"rotate":{"max":100}}}},
            "mixRotate":0
          }],
          "animations":{
            "off":{"transform":{"copy":[{"mixRotate":0}]}},
            "on":{"transform":{"copy":[{"mixRotate":1}]}}
          }
        }"#,
        b"page.png\n",
    )
    .expect("rotation-transform crossfade fixture loads")
    .into_asset();
    let off = asset.animation_id("off").unwrap();
    let on = asset.animation_id("on").unwrap();
    let constrained = asset.bone_id("constrained").unwrap();
    let copy = asset.transform_constraint_id("copy").unwrap();
    let mut skeleton = Skeleton::new(Arc::clone(&asset));
    let mut player = AnimationPlayer::new(&skeleton);

    player.play(off, PlayOptions::looping()).unwrap();
    let _frame = player
        .update(&mut skeleton, Duration::ZERO, &mut ())
        .unwrap()
        .solve();
    player
        .play(
            on,
            PlayOptions::looping().with_transition(Transition::Crossfade(Crossfade::new(
                Duration::from_secs(1),
            ))),
        )
        .unwrap();
    let frame = player
        .update(&mut skeleton, Duration::from_millis(500), &mut ())
        .unwrap()
        .solve();

    let axis = frame.bone(constrained).unwrap().world_transform().x_axis();
    assert!((axis.y.atan2(axis.x).to_degrees() - 45.0).abs() < 1.0e-4);
    drop(frame);
    assert_eq!(
        skeleton
            .transform_constraint_pose(copy)
            .unwrap()
            .mix_rotate(),
        TransformMix::new(0.5).unwrap()
    );
}

#[test]
fn event_boundaries_are_exact_and_loop_end_precedes_the_next_zero_key() {
    let (asset, mut skeleton) = fixture();
    let idle = asset.animation_id("idle").expect("animation exists");
    let mut player = AnimationPlayer::new(&skeleton);
    let mut seen = Vec::new();
    let mut record = |event: AnimationEvent<'_>| {
        seen.push((
            event.definition().name().to_owned(),
            event.loop_index(),
            event.local_time(),
        ));
    };

    player
        .play(idle, PlayOptions::looping())
        .expect("animation belongs to the bound skeleton");
    let _frame = player
        .update(&mut skeleton, Duration::ZERO, &mut record)
        .expect("bound skeleton")
        .solve();
    let _frame = player
        .update(&mut skeleton, Duration::from_millis(500), &mut record)
        .expect("bound skeleton")
        .solve();
    let _frame = player
        .update(&mut skeleton, Duration::from_millis(500), &mut record)
        .expect("bound skeleton")
        .solve();

    assert_eq!(
        seen,
        [
            ("start".to_owned(), 0, Duration::ZERO),
            ("middle".to_owned(), 0, Duration::from_millis(500)),
            ("end".to_owned(), 0, Duration::from_secs(1)),
            ("start".to_owned(), 1, Duration::ZERO),
        ]
    );
}

#[test]
fn one_large_update_emits_every_crossed_loop_in_exact_order() {
    let (asset, mut skeleton) = fixture();
    let idle = asset.animation_id("idle").expect("animation exists");
    let mut player = AnimationPlayer::new(&skeleton);
    let mut seen = Vec::new();

    player
        .play(idle, PlayOptions::looping())
        .expect("animation belongs to this player");
    let frame = player
        .update(
            &mut skeleton,
            Duration::from_millis(2_500),
            &mut |event: AnimationEvent<'_>| {
                seen.push((event.definition().name().to_owned(), event.loop_index()));
            },
        )
        .expect("player remains bound")
        .solve();

    assert_eq!(frame.report().loops_completed(), 2);
    assert_eq!(
        seen,
        [
            ("start".to_owned(), 0),
            ("middle".to_owned(), 0),
            ("end".to_owned(), 0),
            ("start".to_owned(), 1),
            ("middle".to_owned(), 1),
            ("end".to_owned(), 1),
            ("start".to_owned(), 2),
            ("middle".to_owned(), 2),
        ]
    );
}

#[test]
fn once_completion_and_endpoint_events_are_reported_once() {
    let (asset, mut skeleton) = fixture();
    let idle = asset.animation_id("idle").expect("animation exists");
    let mut player = AnimationPlayer::new(&skeleton);
    let outcome = player
        .play(idle, PlayOptions::once())
        .expect("animation belongs to this player");
    let mut end_count = 0;

    let first = player
        .update(
            &mut skeleton,
            Duration::from_secs(1),
            &mut |event: AnimationEvent<'_>| {
                end_count += usize::from(event.definition().name() == "end");
            },
        )
        .expect("player remains bound")
        .solve();
    assert_eq!(first.report().completed(), Some(outcome.playback()));
    drop(first);

    let held = player
        .update(
            &mut skeleton,
            Duration::from_secs(5),
            &mut |event: AnimationEvent<'_>| {
                end_count += usize::from(event.definition().name() == "end");
            },
        )
        .expect("player remains bound")
        .solve();
    assert_eq!(held.report().completed(), None);
    assert!(player.status().is_complete());
    assert_eq!(end_count, 1);
}

#[test]
fn zero_duration_playback_emits_its_zero_event_and_payload_once() {
    let (asset, mut skeleton) = fixture();
    let zero = asset.animation_id("zero").expect("animation exists");
    let mut player = AnimationPlayer::new(&skeleton);
    let outcome = player
        .play(zero, PlayOptions::once())
        .expect("animation belongs to this player");
    let mut seen = Vec::new();

    let first = player
        .update(
            &mut skeleton,
            Duration::ZERO,
            &mut |event: AnimationEvent<'_>| {
                seen.push((
                    event.integer(),
                    event.float(),
                    event.string().map(str::to_owned),
                    event.local_time(),
                    event.has_degradations(),
                ));
            },
        )
        .expect("player remains bound")
        .solve();
    assert_eq!(first.report().completed(), Some(outcome.playback()));
    drop(first);
    let held = player
        .update(
            &mut skeleton,
            Duration::from_secs(1),
            &mut |event: AnimationEvent<'_>| {
                seen.push((
                    event.integer(),
                    event.float(),
                    event.string().map(str::to_owned),
                    event.local_time(),
                    event.has_degradations(),
                ));
            },
        )
        .expect("player remains bound")
        .solve();
    assert_eq!(held.report().completed(), None);
    assert_eq!(
        seen,
        [(9, 1.5, Some("override".to_owned()), Duration::ZERO, true)]
    );
}

#[test]
fn equal_time_events_keep_source_order_and_interrupted_sources_stop_immediately() {
    let (asset, mut skeleton) = fixture();
    let same_time = asset.animation_id("same-time").expect("animation exists");
    let idle = asset.animation_id("idle").expect("animation exists");
    let fall = asset.animation_id("fall").expect("animation exists");
    let mut player = AnimationPlayer::new(&skeleton);
    let mut seen = Vec::new();

    player
        .play(same_time, PlayOptions::once())
        .expect("animation is asset-local");
    let _frame = player
        .update(
            &mut skeleton,
            Duration::from_millis(500),
            &mut |event: AnimationEvent<'_>| {
                seen.push(event.definition().name().to_owned());
            },
        )
        .expect("player remains bound")
        .solve();
    assert_eq!(seen, ["same-a", "same-b"]);

    player
        .play(idle, PlayOptions::looping())
        .expect("animation is asset-local");
    let _frame = player
        .update(&mut skeleton, Duration::ZERO, &mut ())
        .expect("player remains bound")
        .solve();
    player
        .play(fall, PlayOptions::looping())
        .expect("animation is asset-local");
    let _frame = player
        .update(
            &mut skeleton,
            Duration::from_millis(500),
            &mut |event: AnimationEvent<'_>| {
                seen.push(event.definition().name().to_owned());
            },
        )
        .expect("player remains bound")
        .solve();
    assert_eq!(seen, ["same-a", "same-b"]);
}

#[test]
fn live_crossfade_events_belong_only_to_each_current_target_and_never_replay() {
    let (asset, mut skeleton) = fixture();
    let idle = asset.animation_id("idle").expect("animation exists");
    let target = asset
        .animation_id("event-target")
        .expect("animation exists");
    let interrupt = asset
        .animation_id("event-interrupt")
        .expect("animation exists");
    let mut player = AnimationPlayer::new(&skeleton);
    let mut seen = Vec::new();
    let transition = Transition::Crossfade(Crossfade::new(Duration::from_secs(1)));

    player
        .play(idle, PlayOptions::looping())
        .expect("animation is asset-local");
    let _frame = player
        .update(
            &mut skeleton,
            Duration::from_millis(100),
            &mut |event: AnimationEvent<'_>| {
                seen.push(event.definition().name().to_owned());
            },
        )
        .expect("player remains bound")
        .solve();

    player
        .play(target, PlayOptions::once().with_transition(transition))
        .expect("animation is asset-local");
    for delta in [
        Duration::ZERO,
        Duration::from_millis(300),
        Duration::from_millis(200),
    ] {
        let _frame = player
            .update(&mut skeleton, delta, &mut |event: AnimationEvent<'_>| {
                seen.push(event.definition().name().to_owned());
            })
            .expect("player remains bound")
            .solve();
    }
    assert!(
        player.status().transition_mix().is_some(),
        "the target remains in a live crossfade before rapid interruption"
    );

    player
        .play(interrupt, PlayOptions::once().with_transition(transition))
        .expect("animation is asset-local");
    for delta in [
        Duration::ZERO,
        Duration::from_millis(600),
        Duration::from_millis(400),
        Duration::from_secs(1),
    ] {
        let _frame = player
            .update(&mut skeleton, delta, &mut |event: AnimationEvent<'_>| {
                seen.push(event.definition().name().to_owned());
            })
            .expect("player remains bound")
            .solve();
    }

    assert_eq!(
        seen,
        [
            "start",
            "target-start",
            "target-middle",
            "interrupt-start",
            "interrupt-middle",
            "interrupt-end",
        ],
        "source and superseded-target events stop immediately, while each current target emits every crossed key exactly once"
    );
}

#[test]
fn repeated_play_before_update_collapses_to_the_last_target() {
    let (asset, mut skeleton) = fixture();
    let idle = asset.animation_id("idle").expect("animation exists");
    let fall = asset.animation_id("fall").expect("animation exists");
    let jump = asset.animation_id("jump").expect("animation exists");
    let cat = asset.bone_id("cat").expect("bone exists");
    let mut player = AnimationPlayer::new(&skeleton);
    let mut event_count = 0;

    let idle_playback = player
        .play(idle, PlayOptions::looping())
        .expect("animation is asset-local")
        .playback();
    let fall_outcome = player
        .play(fall, PlayOptions::looping())
        .expect("animation is asset-local");
    assert_eq!(fall_outcome.interrupted(), Some(idle_playback));
    let jump_outcome = player
        .play(jump, PlayOptions::looping())
        .expect("animation is asset-local");
    assert_eq!(jump_outcome.interrupted(), Some(fall_outcome.playback()));

    let frame = player
        .update(
            &mut skeleton,
            Duration::ZERO,
            &mut |_event: AnimationEvent<'_>| event_count += 1,
        )
        .expect("player remains bound")
        .solve();
    assert_eq!(event_count, 0);
    assert_eq!(frame.report().current(), Some(jump_outcome.playback()));
    assert!(
        (frame
            .bone(cat)
            .expect("bone is asset-local")
            .local_transform()
            .rotation()
            .as_degrees()
            - 120.0)
            .abs()
            < 1.0e-4
    );
}

#[test]
fn zero_duration_loop_emits_time_zero_only_on_first_application() {
    let (asset, mut skeleton) = fixture();
    let zero = asset.animation_id("zero").expect("animation exists");
    let mut player = AnimationPlayer::new(&skeleton);
    let mut count = 0;
    player
        .play(zero, PlayOptions::looping())
        .expect("animation is asset-local");

    for delta in [Duration::ZERO, Duration::from_secs(1), Duration::MAX] {
        let _frame = player
            .update(&mut skeleton, delta, &mut |_event: AnimationEvent<'_>| {
                count += 1
            })
            .expect("player remains bound")
            .solve();
    }

    assert_eq!(count, 1);
    assert_eq!(player.status().loop_index(), Some(0));
}

#[test]
fn rapid_interruption_freezes_the_presented_base_without_procedural_edits() {
    let (asset, mut skeleton) = fixture();
    let idle = asset.animation_id("idle").expect("animation exists");
    let fall = asset.animation_id("fall").expect("animation exists");
    let jump = asset.animation_id("jump").expect("animation exists");
    let cat = asset.bone_id("cat").expect("bone exists");
    let mut player = AnimationPlayer::new(&skeleton);

    player
        .play(idle, PlayOptions::looping())
        .expect("animation belongs to this player");
    let _frame = player
        .update(&mut skeleton, Duration::ZERO, &mut ())
        .expect("player remains bound")
        .solve();
    player
        .play(
            fall,
            PlayOptions::looping().with_transition(Transition::Crossfade(Crossfade::new(
                Duration::from_secs(1),
            ))),
        )
        .expect("animation belongs to this player");
    let mut pose = player
        .update(&mut skeleton, Duration::from_millis(500), &mut ())
        .expect("player remains bound");
    {
        let mut edit = pose.edit();
        let local = edit.bone_local(cat).expect("bone belongs to this asset");
        edit.set_bone_local(
            cat,
            BoneTransform::new(
                local.translation(),
                Angle::from_degrees(70.0).expect("test angle is finite"),
                local.scale(),
                local.shear(),
            )
            .expect("test transform is finite"),
        )
        .expect("bone belongs to this asset");
    }
    let edited = pose.solve();
    assert!(
        (edited
            .bone(cat)
            .expect("bone belongs to this asset")
            .local_transform()
            .rotation()
            .as_degrees()
            - 70.0)
            .abs()
            < 1.0e-4
    );
    drop(edited);

    player
        .play(
            jump,
            PlayOptions::looping().with_transition(Transition::Crossfade(Crossfade::new(
                Duration::from_secs(1),
            ))),
        )
        .expect("animation belongs to this player");
    let frame = player
        .update(&mut skeleton, Duration::from_millis(500), &mut ())
        .expect("player remains bound")
        .solve();
    assert!(
        (frame
            .bone(cat)
            .expect("bone belongs to this asset")
            .local_transform()
            .rotation()
            .as_degrees()
            - 82.5)
            .abs()
            < 1.0e-4
    );
}

#[test]
fn discrete_properties_switch_at_the_configured_eased_threshold() {
    let (asset, mut skeleton) = fixture();
    let idle = asset.animation_id("idle").expect("animation exists");
    let alternate = asset.animation_id("alternate").expect("animation exists");
    let mirror = asset.animation_id("mirror").expect("animation exists");
    let cat = asset.bone_id("cat").expect("bone exists");
    let body_slot = asset.slot_id("body-slot").expect("slot exists");
    let alternate_attachment = asset
        .attachments()
        .find(|attachment| attachment.name() == "alternate")
        .expect("alternate attachment exists")
        .id();
    let midpoint = DiscreteSwitches::uniform(Mix::new(0.5).expect("midpoint is normalized"));
    let mut player = AnimationPlayer::new(&skeleton);

    player
        .play(idle, PlayOptions::looping())
        .expect("animation belongs to this player");
    let _frame = player
        .update(&mut skeleton, Duration::ZERO, &mut ())
        .expect("player remains bound")
        .solve();
    player
        .play(
            alternate,
            PlayOptions::looping().with_transition(Transition::Crossfade(
                Crossfade::new(Duration::from_secs(1)).with_discrete(midpoint),
            )),
        )
        .expect("animation belongs to this player");
    let before = player
        .update(&mut skeleton, Duration::from_millis(499), &mut ())
        .expect("player remains bound")
        .solve();
    assert_ne!(
        before
            .slot(body_slot)
            .expect("slot belongs to this asset")
            .attachment(),
        Some(alternate_attachment)
    );
    drop(before);
    let at = player
        .update(&mut skeleton, Duration::from_millis(1), &mut ())
        .expect("player remains bound")
        .solve();
    assert_eq!(
        at.slot(body_slot)
            .expect("slot belongs to this asset")
            .attachment(),
        Some(alternate_attachment)
    );
    drop(at);

    player
        .play(idle, PlayOptions::looping())
        .expect("animation belongs to this player");
    let _frame = player
        .update(&mut skeleton, Duration::ZERO, &mut ())
        .expect("player remains bound")
        .solve();
    player
        .play(
            mirror,
            PlayOptions::looping().with_transition(Transition::Crossfade(
                Crossfade::new(Duration::from_secs(1)).with_discrete(midpoint),
            )),
        )
        .expect("animation belongs to this player");
    let before = player
        .update(&mut skeleton, Duration::from_millis(499), &mut ())
        .expect("player remains bound")
        .solve();
    assert!(
        before
            .bone(cat)
            .expect("bone belongs to this asset")
            .local_transform()
            .scale()
            .x
            .is_sign_positive()
    );
    drop(before);
    let at = player
        .update(&mut skeleton, Duration::from_millis(1), &mut ())
        .expect("player remains bound")
        .solve();
    assert!(
        at.bone(cat)
            .expect("bone belongs to this asset")
            .local_transform()
            .scale()
            .x
            .is_sign_negative()
    );
}

#[test]
fn command_and_instance_errors_leave_player_state_unchanged() {
    let (asset, mut skeleton) = fixture();
    let idle = asset.animation_id("idle").expect("animation exists");
    let mut other_instance = Skeleton::new(Arc::clone(&asset));
    let foreign_asset = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[{"name":"root"}],
          "animations":{"foreign":{}}
        }"#,
        b"foreign.png\n",
    )
    .expect("foreign fixture loads")
    .into_asset();
    let foreign = foreign_asset
        .animation_id("foreign")
        .expect("animation exists");
    let mut player = AnimationPlayer::new(&skeleton);
    let outcome = player
        .play(idle, PlayOptions::looping())
        .expect("animation belongs to this player");
    let before = player.status();

    assert!(matches!(
        player.play(foreign, PlayOptions::once()),
        Err(PlayerError::InvalidAnimation(_error))
    ));
    assert_eq!(player.status(), before);
    assert!(matches!(
        player.update(&mut other_instance, Duration::from_secs(1), &mut ()),
        Err(PlayerError::ForeignSkeleton)
    ));
    assert_eq!(player.status(), before);

    let frame = player
        .update(&mut skeleton, Duration::from_millis(250), &mut ())
        .expect("the bound instance remains usable")
        .solve();
    assert_eq!(frame.report().current(), Some(outcome.playback()));
    assert_eq!(player.status().position(), Some(Duration::from_millis(250)));
}

#[test]
fn idle_player_observes_skin_changes_instead_of_restoring_stale_attachments() {
    let (asset, mut skeleton) = fixture();
    let alternate_skin = asset.skin_id("alternate-skin").expect("skin exists");
    let body_slot = asset.slot_id("body-slot").expect("slot exists");
    let expected = asset
        .skin(alternate_skin)
        .expect("skin is asset-local")
        .attachment(body_slot, "body")
        .expect("slot is asset-local")
        .expect("skin supplies body");
    let mut player = AnimationPlayer::new(&skeleton);

    skeleton
        .set_skin_layers(&[alternate_skin])
        .expect("skin is asset-local");
    let frame = player
        .update(&mut skeleton, Duration::ZERO, &mut ())
        .expect("player remains bound")
        .solve();

    assert_eq!(
        frame
            .slot(body_slot)
            .expect("slot is asset-local")
            .attachment(),
        Some(expected)
    );
}

#[test]
fn idle_player_resolves_a_setup_placeholder_first_supplied_by_a_new_skin() {
    let asset = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[{"name":"root"}],
          "slots":[{"name":"hat-slot","bone":"root","attachment":"hat"}],
          "skins":[{
            "name":"party-hat",
            "attachments":{
              "hat-slot":{
                "hat":{"path":"hat","width":8,"height":8}
              }
            }
          }]
        }"#,
        b"cat.png\n\tsize: 8, 8\nhat\n\tbounds: 0, 0, 8, 8\n",
    )
    .expect("optional setup-placeholder fixture loads")
    .into_asset();
    let party_hat = asset.skin_id("party-hat").expect("skin exists");
    let hat_slot = asset.slot_id("hat-slot").expect("slot exists");
    let expected = asset
        .skin(party_hat)
        .expect("skin is asset-local")
        .attachment(hat_slot, "hat")
        .expect("slot is asset-local")
        .expect("skin supplies hat");
    let mut skeleton = Skeleton::new(Arc::clone(&asset));
    let mut player = AnimationPlayer::new(&skeleton);

    skeleton
        .set_skin_layers(&[party_hat])
        .expect("skin is asset-local");
    let frame = player
        .update(&mut skeleton, Duration::ZERO, &mut ())
        .expect("player remains bound")
        .solve();

    assert_eq!(
        frame
            .slot(hat_slot)
            .expect("slot is asset-local")
            .attachment(),
        Some(expected)
    );
}

#[test]
fn skin_changes_remap_a_frozen_crossfade_source() {
    let (asset, mut skeleton) = fixture();
    let idle = asset.animation_id("idle").expect("animation exists");
    let fall = asset.animation_id("fall").expect("animation exists");
    let alternate_skin = asset.skin_id("alternate-skin").expect("skin exists");
    let body_slot = asset.slot_id("body-slot").expect("slot exists");
    let expected = asset
        .skin(alternate_skin)
        .expect("skin is asset-local")
        .attachment(body_slot, "body")
        .expect("slot is asset-local")
        .expect("skin supplies body");
    let mut player = AnimationPlayer::new(&skeleton);
    player
        .play(idle, PlayOptions::looping())
        .expect("animation is asset-local");
    let _frame = player
        .update(&mut skeleton, Duration::ZERO, &mut ())
        .expect("player remains bound")
        .solve();
    player
        .play(
            fall,
            PlayOptions::looping().with_transition(Transition::Crossfade(
                Crossfade::new(Duration::from_secs(1))
                    .with_discrete(DiscreteSwitches::TARGET_AT_END),
            )),
        )
        .expect("animation is asset-local");
    let _frame = player
        .update(&mut skeleton, Duration::from_millis(250), &mut ())
        .expect("player remains bound")
        .solve();

    skeleton
        .set_skin_layers(&[alternate_skin])
        .expect("skin is asset-local");
    let frame = player
        .update(&mut skeleton, Duration::ZERO, &mut ())
        .expect("player remains bound")
        .solve();
    assert_eq!(
        frame
            .slot(body_slot)
            .expect("slot is asset-local")
            .attachment(),
        Some(expected)
    );
}

#[test]
fn event_emitting_playback_and_skin_remapping_allocate_nothing() {
    let (asset, mut skeleton) = fixture();
    let idle = asset.animation_id("idle").expect("animation exists");
    let alternate_skin = asset.skin_id("alternate-skin").expect("skin exists");
    let mut player = AnimationPlayer::new(&skeleton);
    player
        .play(idle, PlayOptions::looping())
        .expect("animation is asset-local");
    let _frame = player
        .update(&mut skeleton, Duration::ZERO, &mut ())
        .expect("player remains bound")
        .solve();
    let alternate = [alternate_skin];
    let mut event_count = 0_usize;

    let allocations = allocation_counter::measure(|| {
        for step in 0..128 {
            skeleton
                .set_skin_layers(if step % 2 == 0 { &alternate } else { &[] })
                .expect("skin is asset-local");
            let frame = player
                .update(
                    &mut skeleton,
                    Duration::from_millis(500),
                    &mut |event: AnimationEvent<'_>| {
                        event_count += 1;
                        std::hint::black_box(event.definition().name());
                    },
                )
                .expect("player remains bound")
                .solve();
            std::hint::black_box(frame.draw_items().count());
        }
    });

    assert!(event_count > 0);
    assert_eq!(allocations.count_total, 0);
    assert_eq!(allocations.bytes_total, 0);
}

#[test]
fn stop_can_crossfade_to_setup_pose_and_then_become_idle() {
    let (asset, mut skeleton) = fixture();
    let fall = asset.animation_id("fall").expect("animation exists");
    let cat = asset.bone_id("cat").expect("bone exists");
    let mut player = AnimationPlayer::new(&skeleton);
    assert_eq!(
        player.stop(Transition::Crossfade(Crossfade::new(Duration::from_secs(
            1
        )))),
        None
    );
    assert!(player.status().is_idle());

    let playback = player
        .play(fall, PlayOptions::looping())
        .expect("animation belongs to this player")
        .playback();
    let _frame = player
        .update(&mut skeleton, Duration::ZERO, &mut ())
        .expect("player remains bound")
        .solve();
    assert_eq!(
        player.stop(Transition::Crossfade(Crossfade::new(Duration::from_secs(
            1
        )))),
        Some(playback)
    );

    let half = player
        .update(&mut skeleton, Duration::from_millis(500), &mut ())
        .expect("player remains bound")
        .solve();
    assert!(
        (half
            .bone(cat)
            .expect("bone belongs to this asset")
            .local_transform()
            .rotation()
            .as_degrees()
            - 45.0)
            .abs()
            < 1.0e-4
    );
    drop(half);
    let stopping = player.status();
    assert_eq!(
        player.stop(Transition::Crossfade(Crossfade::new(Duration::from_secs(
            1
        )))),
        None
    );
    assert_eq!(player.status(), stopping);
    let setup = player
        .update(&mut skeleton, Duration::from_millis(500), &mut ())
        .expect("player remains bound")
        .solve();
    assert!(
        setup
            .bone(cat)
            .expect("bone belongs to this asset")
            .local_transform()
            .rotation()
            .as_degrees()
            .abs()
            < 1.0e-4
    );
    assert!(player.status().is_idle());
}

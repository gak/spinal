//! Public contracts for Stage 4 pose solving and renderer-neutral output.

use std::{sync::Arc, time::Duration};

use spinal::{
    Angle, AnimationPlayer, BoneTransform, DiagnosticCode, IkTargetReach, Mix, PlayOptions,
    PlaybackMode, Rgba, Shear, Skeleton, load_json,
};

const ATLAS: &str = "\
cat.png
\tsize: 32, 16
body
\tbounds: 0, 0, 8, 8
";

const JSON: &str = r#"{
  "skeleton":{"spine":"4.3.23"},
  "bones":[
    {"name":"root"},
    {"name":"cat","parent":"root","x":10,"length":8},
    {"name":"target","parent":"root","x":10,"y":10}
  ],
  "slots":[{"name":"body-slot","bone":"cat","attachment":"body"}],
  "skins":[{
    "name":"default",
    "attachments":{
      "body-slot":{
        "body":{"path":"body","width":8,"height":8}
      }
    }
  }],
  "constraints":[{
    "name":"aim",
    "type":"ik",
    "bones":["cat"],
    "target":"target",
    "mix":1
  }],
  "animations":{
    "idle":{
      "bones":{"cat":{"rotate":[{"value":0},{"time":1,"value":0}]}}
    },
    "turn":{
      "bones":{"cat":{"rotate":[{"value":90},{"time":1,"value":90}]}}
    }
  }
}"#;

fn fixture() -> (Arc<spinal::SkeletonAsset>, Skeleton) {
    let asset = load_json(JSON.as_bytes(), ATLAS.as_bytes())
        .expect("the frame fixture should load")
        .into_asset();
    let skeleton = Skeleton::new(Arc::clone(&asset));
    (asset, skeleton)
}

#[test]
fn standalone_pose_editing_precedes_world_solving_and_ik() {
    let (asset, mut skeleton) = fixture();
    let idle = asset.animation_id("idle").expect("animation exists");
    let cat = asset.bone_id("cat").expect("bone exists");
    let aim = asset.ik_constraint_id("aim").expect("IK exists");

    skeleton
        .sample_animation(idle, Duration::ZERO, PlaybackMode::Once)
        .expect("animation is asset-local");
    let mut pose = skeleton.editable_pose();
    {
        let mut edit = pose.edit();
        let sampled = edit.bone_local(cat).expect("bone is asset-local");
        edit.set_bone_local(
            cat,
            BoneTransform::new(
                sampled.translation(),
                Angle::from_degrees(-45.0).expect("test angle is finite"),
                sampled.scale(),
                sampled.shear(),
            )
            .expect("test transform is finite"),
        )
        .expect("bone is asset-local");
        edit.set_ik_mix(aim, Mix::ONE)
            .expect("constraint is asset-local");
    }

    let frame = pose.solve();
    let cat = frame.bone(cat).expect("bone is asset-local");
    assert!((cat.local_transform().rotation().as_degrees() - 90.0).abs() < 1.0e-4);
    assert_vec2_near(cat.world_transform().translation(), [10.0, 0.0]);
    assert_vec2_near(cat.world_transform().x_axis(), [0.0, 1.0]);
    assert_eq!(
        frame
            .ik_status(aim)
            .expect("constraint is asset-local")
            .target_reach(),
        None
    );
}

#[test]
fn repeated_standalone_solves_are_idempotent_and_preserve_the_unconstrained_pose() {
    let (asset, mut skeleton) = fixture();
    let cat = asset.bone_id("cat").expect("bone exists");
    let aim = asset.ik_constraint_id("aim").expect("IK exists");
    {
        let mut pose = skeleton.editable_pose();
        pose.edit()
            .set_ik_mix(aim, Mix::new(0.5).expect("half mix is normalized"))
            .expect("constraint is asset-local");
        let first = pose.solve();
        assert!(
            (first
                .bone(cat)
                .expect("bone is asset-local")
                .local_transform()
                .rotation()
                .as_degrees()
                - 45.0)
                .abs()
                < 1.0e-4
        );
    }

    assert!(
        skeleton
            .bone_pose(cat)
            .expect("bone is asset-local")
            .local_transform()
            .rotation()
            .as_degrees()
            .abs()
            < 1.0e-4
    );
    let second = skeleton.editable_pose().solve();
    assert!(
        (second
            .bone(cat)
            .expect("bone is asset-local")
            .local_transform()
            .rotation()
            .as_degrees()
            - 45.0)
            .abs()
            < 1.0e-4
    );
}

#[test]
fn two_bone_ik_reaches_a_target_through_the_integrated_frame_solver() {
    let asset = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[
            {"name":"root"},
            {"name":"upper","parent":"root"},
            {"name":"lower","parent":"upper","x":10,"length":10},
            {"name":"target","parent":"root","x":10,"y":10}
          ],
          "constraints":[{
            "name":"paw",
            "type":"ik",
            "bones":["upper","lower"],
            "target":"target",
            "bendPositive":true
          }]
        }"#,
        b"cat.png\n",
    )
    .expect("two-bone fixture should load")
    .into_asset();
    let lower = asset.bone_id("lower").expect("bone exists");
    let paw = asset.ik_constraint_id("paw").expect("IK exists");
    let mut skeleton = Skeleton::new(asset);

    let frame = skeleton.editable_pose().solve();
    let tip = frame
        .bone(lower)
        .expect("bone is asset-local")
        .world_transform()
        .transform_point(spinal::glam::Vec2::new(10.0, 0.0));
    assert_vec2_near(tip, [10.0, 10.0]);
    assert_eq!(
        frame
            .ik_status(paw)
            .expect("constraint is asset-local")
            .target_reach(),
        Some(IkTargetReach::Reachable)
    );
}

#[test]
fn two_bone_ik_zeroes_the_parent_local_shear_before_solving() {
    let asset = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[
            {"name":"root"},
            {"name":"upper","parent":"root","shearX":20,"shearY":-15},
            {"name":"lower","parent":"upper","x":10,"length":10},
            {"name":"target","parent":"root","x":10,"y":10}
          ],
          "constraints":[{
            "name":"paw",
            "type":"ik",
            "bones":["upper","lower"],
            "target":"target",
            "bendPositive":true
          }]
        }"#,
        b"cat.png\n",
    )
    .expect("sheared two-bone fixture should load")
    .into_asset();
    let upper = asset.bone_id("upper").expect("bone exists");
    let lower = asset.bone_id("lower").expect("bone exists");
    let mut skeleton = Skeleton::new(asset);

    let frame = skeleton.editable_pose().solve();
    assert_eq!(
        frame
            .bone(upper)
            .expect("bone is asset-local")
            .local_transform()
            .shear(),
        Shear::ZERO
    );
    let tip = frame
        .bone(lower)
        .expect("bone is asset-local")
        .world_transform()
        .transform_point(spinal::glam::Vec2::new(10.0, 0.0));
    assert_vec2_near(tip, [10.0, 10.0]);
}

#[test]
fn solved_frames_produce_ordered_renderer_neutral_regions() {
    let (asset, mut skeleton) = fixture();
    let body = asset
        .attachments()
        .find(|attachment| attachment.name() == "body")
        .expect("attachment exists")
        .id();

    let frame = skeleton.editable_pose().solve();
    let mut items = frame.draw_items();
    let item = items.next().expect("setup attachment is visible");
    let region = match item {
        spinal::DrawItemRef::Region(region) => region,
        _future => panic!("the first renderer profile emits only rigid regions"),
    };
    assert_eq!(region.attachment(), body);
    assert_eq!(
        region.atlas_page(),
        asset.atlas_page_id("cat.png").expect("page exists")
    );
    assert_eq!(
        region.uvs().expect("page has dimensions"),
        [
            spinal::glam::Vec2::new(0.0, 0.5),
            spinal::glam::Vec2::new(0.0, 0.0),
            spinal::glam::Vec2::new(0.25, 0.0),
            spinal::glam::Vec2::new(0.25, 0.5),
        ]
    );
    assert_eq!(region.color(), Rgba::WHITE);
    assert!(items.next().is_none());
}

#[test]
fn unsafe_ik_preserves_a_finite_fk_pose_and_reports_degradation() {
    let asset = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[
            {"name":"root"},
            {"name":"cat","parent":"root","scaleX":0},
            {"name":"target","parent":"root","x":10}
          ],
          "constraints":[{
            "name":"aim",
            "type":"ik",
            "bones":["cat"],
            "target":"target"
          }]
        }"#,
        b"cat.png\n",
    )
    .expect("singular runtime geometry is valid asset data")
    .into_asset();
    let aim = asset.ik_constraint_id("aim").expect("IK exists");
    let cat = asset.bone_id("cat").expect("bone exists");
    let mut skeleton = Skeleton::new(asset);

    let frame = skeleton.editable_pose().solve();
    let status = frame.ik_status(aim).expect("constraint is asset-local");
    assert!(status.is_degraded());
    assert!(frame.has_runtime_degradations());
    assert!(frame.has_degradations());
    assert!(
        frame
            .bone(cat)
            .expect("bone is asset-local")
            .world_transform()
            .x_axis()
            .is_finite()
    );
}

#[test]
fn coincident_one_bone_target_preserves_fk_without_false_degradation() {
    let asset = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[
            {"name":"root"},
            {"name":"cat","parent":"root","rotation":30},
            {"name":"target","parent":"root"}
          ],
          "constraints":[{
            "name":"aim",
            "type":"ik",
            "bones":["cat"],
            "target":"target"
          }]
        }"#,
        b"cat.png\n",
    )
    .expect("coincident targets are valid asset data")
    .into_asset();
    let aim = asset.ik_constraint_id("aim").expect("IK exists");
    let cat = asset.bone_id("cat").expect("bone exists");
    let mut skeleton = Skeleton::new(asset);

    let frame = skeleton.editable_pose().solve();
    let status = frame.ik_status(aim).expect("constraint is asset-local");
    assert!(status.is_active());
    assert!(status.preserved_underdetermined());
    assert!(!status.is_degraded());
    assert!(!frame.has_degradations());
    assert!(
        (frame
            .bone(cat)
            .expect("bone is asset-local")
            .local_transform()
            .rotation()
            .as_degrees()
            - 30.0)
            .abs()
            < 1.0e-4
    );
}

#[test]
fn extreme_finite_one_bone_target_is_not_mistaken_for_coincident() {
    let asset = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[
            {"name":"root"},
            {"name":"cat","parent":"root","x":3.4028235e38},
            {"name":"target","parent":"root","x":-3.4028235e38}
          ],
          "constraints":[{
            "name":"aim",
            "type":"ik",
            "bones":["cat"],
            "target":"target"
          }]
        }"#,
        b"cat.png\n",
    )
    .expect("extreme finite coordinates are valid asset data")
    .into_asset();
    let aim = asset.ik_constraint_id("aim").expect("IK exists");
    let cat = asset.bone_id("cat").expect("bone exists");
    let mut skeleton = Skeleton::new(asset);

    let frame = skeleton.editable_pose().solve();
    let status = frame.ik_status(aim).expect("constraint is asset-local");
    assert!(status.is_active());
    assert!(!status.preserved_underdetermined());
    assert!(!status.is_degraded());
    assert!(
        (frame
            .bone(cat)
            .expect("bone is asset-local")
            .local_transform()
            .rotation()
            .as_degrees()
            .abs()
            - 180.0)
            .abs()
            < 1.0e-3
    );
}

#[test]
fn one_bone_coincidence_is_invariant_under_large_parent_translation() {
    let mut rotations = Vec::new();
    for root_x in [0.0_f32, 100_000_000.0] {
        let json = format!(
            r#"{{
              "skeleton":{{"spine":"4.3.23"}},
              "bones":[
                {{"name":"root","x":{root_x}}},
                {{"name":"cat","parent":"root"}},
                {{"name":"target","parent":"root","y":96}}
              ],
              "constraints":[{{
                "name":"aim",
                "type":"ik",
                "bones":["cat"],
                "target":"target"
              }}]
            }}"#
        );
        let asset = load_json(json.as_bytes(), b"cat.png\n")
            .expect("translated one-bone fixture loads")
            .into_asset();
        let cat = asset.bone_id("cat").expect("bone exists");
        let aim = asset.ik_constraint_id("aim").expect("IK exists");
        let mut skeleton = Skeleton::new(asset);

        let frame = skeleton.editable_pose().solve();
        assert!(
            !frame
                .ik_status(aim)
                .expect("constraint is asset-local")
                .preserved_underdetermined()
        );
        rotations.push(
            frame
                .bone(cat)
                .expect("bone is asset-local")
                .local_transform()
                .rotation()
                .as_degrees(),
        );
    }

    assert!((rotations[0] - 90.0).abs() < 1.0e-4);
    assert!((rotations[1] - rotations[0]).abs() < 1.0e-4);
}

#[test]
fn unsupported_visible_content_is_active_until_its_slot_is_hidden() {
    let asset = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[{"name":"root"}],
          "slots":[{"name":"visual","bone":"root","attachment":"mesh"}],
          "skins":[{
            "name":"default",
            "attachments":{
              "visual":{
                "mesh":{
                  "type":"mesh",
                  "uvs":[0,0,1,0,1,1],
                  "triangles":[0,1,2],
                  "vertices":[0,0,1,0,1,1]
                }
              }
            }
          }],
          "animations":{
            "hide":{"slots":{"visual":{"attachment":[{"name":"mesh"},{"time":1,"name":null}]}}}
          }
        }"#,
        b"cat.png\n",
    )
    .expect("unsupported attachment should be retained")
    .into_asset();
    let hide = asset.animation_id("hide").expect("animation exists");
    let mut skeleton = Skeleton::new(Arc::clone(&asset));

    let visible = skeleton.editable_pose().solve();
    assert!(visible.has_degradations());
    assert!(
        visible
            .active_diagnostics()
            .any(|diagnostic| { diagnostic.code() == DiagnosticCode::UnsupportedAttachmentType })
    );
    drop(visible);

    skeleton
        .sample_animation(hide, Duration::from_secs(1), PlaybackMode::Once)
        .expect("animation is asset-local");
    let hidden = skeleton.editable_pose().solve();
    assert!(!hidden.has_degradations());
    assert!(hidden.active_diagnostics().next().is_none());
}

#[test]
fn steady_state_player_crossfade_solve_and_draw_allocate_nothing() {
    let (asset, mut skeleton) = fixture();
    let idle = asset.animation_id("idle").expect("animation exists");
    let turn = asset.animation_id("turn").expect("animation exists");
    let mut player = AnimationPlayer::new(&skeleton);
    player
        .play(idle, PlayOptions::looping())
        .expect("animation is asset-local");
    let _frame = player
        .update(&mut skeleton, Duration::ZERO, &mut ())
        .expect("player is bound to the skeleton")
        .solve();
    player
        .play(
            turn,
            PlayOptions::looping().with_transition(spinal::Transition::Crossfade(
                spinal::Crossfade::new(Duration::from_secs(1)),
            )),
        )
        .expect("animation is asset-local");

    let allocations = allocation_counter::measure(|| {
        for _step in 0..128 {
            let frame = player
                .update(&mut skeleton, Duration::from_millis(4), &mut ())
                .expect("player is bound to the skeleton")
                .solve();
            for item in frame.draw_items() {
                let region = match item {
                    spinal::DrawItemRef::Region(region) => region,
                    _future => panic!("the first renderer profile emits only rigid regions"),
                };
                std::hint::black_box(region.positions());
            }
        }
    });

    assert_eq!(allocations.count_total, 0);
    assert_eq!(allocations.bytes_total, 0);
}

fn assert_vec2_near(actual: spinal::glam::Vec2, expected: [f32; 2]) {
    assert!((actual.x - expected[0]).abs() < 1.0e-4);
    assert!((actual.y - expected[1]).abs() < 1.0e-4);
}

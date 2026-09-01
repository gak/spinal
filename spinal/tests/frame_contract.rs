//! Public contracts for Stage 4 pose solving and renderer-neutral output.

use std::{sync::Arc, time::Duration};

use spinal::{
    Angle, AnimationEvent, AnimationPlayer, AtlasPageId, AtlasRegionId, AttachmentId, BoneId,
    BoneTransform, Crossfade, DiagnosticCode, DiagnosticScope, DiagnosticSeverity, DrawItemRef,
    IkConstraintId, IkSolveStatus, IkTargetReach, Mix, PlayOptions, PlaybackMode, Rgba, Shear,
    Skeleton, SkeletonAsset, SlotBlendMode, SlotId, SolvedFrame, TransformMix, Transition,
    UpdateReport, WorldTransform, load_json,
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
fn ordered_ik_then_rotation_transform_constraint_drives_spineboy_style_aiming() {
    let asset = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[
            {"name":"root"},
            {"name":"crosshair","parent":"root","x":0,"y":10},
            {"name":"aim-constraint-target","parent":"root","rotation":10},
            {"name":"torso","parent":"root","rotation":100}
          ],
          "constraints":[
            {
              "type":"ik",
              "name":"aim-torso-ik",
              "target":"crosshair",
              "bones":["aim-constraint-target"]
            },
            {
              "type":"transform",
              "name":"aim-torso-transform",
              "source":"aim-constraint-target",
              "bones":["torso"],
              "rotation":30,
              "properties":{"rotate":{"to":{"rotate":{"max":100}}}},
              "mixRotate":0
            }
          ],
          "animations":{
            "aim":{
              "transform":{
                "aim-torso-transform":[{"mixRotate":0.5}]
              }
            }
          }
        }"#,
        b"page.png\n",
    )
    .expect("Spineboy-style constraint chain should load")
    .into_asset();
    let mut skeleton = Skeleton::new(Arc::clone(&asset));
    let aim = asset.animation_id("aim").unwrap();
    let transform = asset
        .transform_constraint_id("aim-torso-transform")
        .unwrap();
    let torso = asset.bone_id("torso").unwrap();

    skeleton
        .sample_animation(aim, Duration::ZERO, PlaybackMode::Once)
        .unwrap();
    assert_eq!(
        skeleton
            .transform_constraint_pose(transform)
            .unwrap()
            .mix_rotate(),
        TransformMix::new(0.5).unwrap()
    );
    let frame = skeleton.editable_pose().solve();

    let torso_world = frame.bone(torso).unwrap().world_transform();
    assert_angle_near(world_rotation(torso_world), 110.0);
    let status = frame.transform_status(transform).unwrap();
    assert!(status.is_active());
    assert!(!status.is_degraded());
}

#[test]
fn unsupported_transform_modes_load_but_preserve_the_unconstrained_pose() {
    let cases = [
        (
            "local source",
            r#""localSource":true,"#,
            r#"{"rotate":{"to":{"rotate":{"max":100}}}}"#,
        ),
        (
            "local target",
            r#""localTarget":true,"#,
            r#"{"rotate":{"to":{"rotate":{"max":100}}}}"#,
        ),
        (
            "additive",
            r#""additive":true,"#,
            r#"{"rotate":{"to":{"rotate":{"max":100}}}}"#,
        ),
        (
            "clamped",
            r#""clamp":true,"#,
            r#"{"rotate":{"to":{"rotate":{"max":100}}}}"#,
        ),
        (
            "remapped source property",
            "",
            r#"{"x":{"to":{"rotate":{"max":100}}}}"#,
        ),
    ];

    for (label, option, properties) in cases {
        let json = format!(
            r#"{{
              "skeleton":{{"spine":"4.3.23"}},
              "bones":[
                {{"name":"root"}},
                {{"name":"source","parent":"root","x":25,"rotation":90}},
                {{"name":"constrained","parent":"root","rotation":10}}
              ],
              "constraints":[{{
                "type":"transform",
                "name":"copy",
                "source":"source",
                "bones":["constrained"],
                {option}
                "properties":{properties},
                "mixRotate":1
              }}]
            }}"#
        );
        let report = load_json(json.as_bytes(), b"page.png\n")
            .unwrap_or_else(|error| panic!("{label} transform constraint should load: {error}"));
        assert!(
            report.diagnostics().iter().any(|diagnostic| {
                diagnostic.code() == DiagnosticCode::UnsupportedConstraintOption
            }),
            "{label} must retain an explicit degraded diagnostic"
        );
        let asset = report.into_asset();
        let constrained = asset.bone_id("constrained").unwrap();
        let constraint = asset.transform_constraint_id("copy").unwrap();
        let mut skeleton = Skeleton::new(asset);

        let frame = skeleton.editable_pose().solve();
        assert_angle_near(
            world_rotation(frame.bone(constrained).unwrap().world_transform()),
            10.0,
        );
        assert!(
            !frame.transform_status(constraint).unwrap().is_active(),
            "{label} must not receive partially invented runtime semantics"
        );
        assert!(frame.has_degradations());
    }
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
        _future => panic!("the rigid fixture should emit a region draw item"),
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
fn solved_frames_skin_weighted_meshes_after_procedural_edits() {
    let report = spinal::load_json(
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
            "uvs":[0,0,1,0,1,1],
            "triangles":[0,1,2],
            "vertices":[
              1,1,-10,0,1,
              2,1,0,0,0.5,2,-20,0,0.5,
              1,2,-20,10,1
            ],
            "hull":3
          }}}}]
        }"#,
        b"page.png\n\tsize: 100, 100\nbody\n\tbounds: 10, 20, 40, 20\n",
    )
    .expect("weighted mesh fixture loads");
    let asset = report.into_asset();
    let right = asset.bone_id("right").expect("right bone exists");
    let mut skeleton = Skeleton::new(Arc::clone(&asset));

    {
        let mut pose = skeleton.editable_pose();
        let current = pose
            .edit()
            .bone_local(right)
            .expect("bone belongs to asset");
        pose.edit()
            .set_bone_local(
                right,
                BoneTransform::new(
                    spinal::glam::Vec2::new(40.0, 0.0),
                    current.rotation(),
                    current.scale(),
                    current.shear(),
                )
                .expect("finite transform"),
            )
            .expect("bone belongs to asset");
        let frame = pose.solve();
        let mesh = match frame.draw_items().next().expect("mesh is visible") {
            spinal::DrawItemRef::Mesh(mesh) => mesh,
            _other => panic!("weighted attachment should produce indexed mesh geometry"),
        };
        assert_eq!(
            mesh.positions(),
            [
                spinal::glam::Vec2::new(0.0, 0.0),
                spinal::glam::Vec2::new(15.0, 0.0),
                spinal::glam::Vec2::new(20.0, 10.0),
            ]
        );
        assert_eq!(mesh.triangles(), &[0, 1, 2]);
        assert_eq!(
            mesh.uvs()
                .expect("atlas page declares a size")
                .collect::<Vec<_>>(),
            [
                spinal::glam::Vec2::new(0.1, 0.2),
                spinal::glam::Vec2::new(0.5, 0.2),
                spinal::glam::Vec2::new(0.5, 0.4),
            ]
        );
    }
}

#[test]
fn weighted_mesh_skinning_observes_the_final_ik_constrained_bone() {
    let report = spinal::load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[
            {"name":"root"},
            {"name":"limb","parent":"root","length":10},
            {"name":"target","parent":"root","y":10}
          ],
          "slots":[{"name":"mesh-slot","bone":"root","attachment":"mesh"}],
          "skins":[{"name":"default","attachments":{"mesh-slot":{"mesh":{
            "type":"mesh",
            "uvs":[0,0,1,0,0,1],
            "triangles":[0,1,2],
            "vertices":[
              1,1,10,0,1,
              1,1,9,0,1,
              1,1,10,1,1
            ],
            "hull":3
          }}}}],
          "constraints":[{
            "name":"reach","type":"ik","bones":["limb"],"target":"target"
          }]
        }"#,
        b"page.png\n\tsize: 16, 16\nmesh\n\tbounds: 0, 0, 16, 16\n",
    )
    .expect("weighted IK fixture loads");
    let mut skeleton = Skeleton::new(report.into_asset());

    let frame = skeleton.editable_pose().solve();
    let mesh = match frame.draw_items().next().expect("weighted mesh is visible") {
        spinal::DrawItemRef::Mesh(mesh) => mesh,
        _other => panic!("weighted fixture produces a mesh draw"),
    };
    assert_vec2_near(mesh.positions()[0], [0.0, 10.0]);
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
          "slots":[{"name":"visual","bone":"root","attachment":"clip"}],
          "skins":[{
            "name":"default",
            "attachments":{
              "visual":{
                "clip":{
                  "type":"clipping",
                  "vertexCount":3,
                  "vertices":[0,0,1,0,1,1]
                }
              }
            }
          }],
          "animations":{
            "hide":{"slots":{"visual":{"attachment":[{"name":"clip"},{"time":1,"name":null}]}}}
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
fn whole_solved_frame_is_deterministic_across_identical_instances() {
    let (asset, _skeleton) = fixture();

    let first = deterministic_solved_snapshot(&asset);
    let second = deterministic_solved_snapshot(&asset);

    assert_eq!(first, second);
    assert_eq!(first.bones.len(), asset.bones().len());
    assert_eq!(first.slots.len(), asset.slots().len());
    assert_eq!(first.draws.len(), 1);
    assert_eq!(first.ik_statuses.len(), asset.ik_constraints().len());
}

#[test]
fn animation_diagnostic_stays_active_during_crossfade_then_retires() {
    let asset = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[{"name":"root"}],
          "animations":{
            "unsupported":{
              "bones":{"root":{"rotate":[{"value":0},{"time":1,"value":0}]}},
              "deform":{}
            },
            "clean":{
              "bones":{"root":{"rotate":[{"value":0},{"time":1,"value":0}]}}
            }
          }
        }"#,
        b"cat.png\n",
    )
    .expect("animation diagnostic fixture should load")
    .into_asset();
    let unsupported = asset.animation_id("unsupported").expect("animation exists");
    let clean = asset.animation_id("clean").expect("animation exists");
    let scope = DiagnosticScope::Animation(unsupported);
    let mut skeleton = Skeleton::new(Arc::clone(&asset));
    let mut player = AnimationPlayer::new(&skeleton);

    player
        .play(unsupported, PlayOptions::looping())
        .expect("animation is asset-local");
    let source = player
        .update(&mut skeleton, Duration::ZERO, &mut ())
        .expect("player is bound to the skeleton")
        .solve();
    assert_active_diagnostic(&source, DiagnosticCode::UnsupportedTimelineType, scope);
    drop(source);

    player
        .play(
            clean,
            PlayOptions::looping().with_transition(Transition::Crossfade(Crossfade::new(
                Duration::from_secs(1),
            ))),
        )
        .expect("animation is asset-local");
    let crossing = player
        .update(&mut skeleton, Duration::from_millis(500), &mut ())
        .expect("player is bound to the skeleton")
        .solve();
    assert_active_diagnostic(&crossing, DiagnosticCode::UnsupportedTimelineType, scope);
    drop(crossing);

    let complete = player
        .update(&mut skeleton, Duration::from_millis(500), &mut ())
        .expect("player is bound to the skeleton")
        .solve();
    assert_inactive_diagnostic(&complete, DiagnosticCode::UnsupportedTimelineType, scope);
}

#[test]
fn skin_diagnostic_follows_selected_skin_layers() {
    let asset = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[{"name":"root"}],
          "skins":[
            {"name":"default","attachments":{}},
            {"name":"outfit","bones":["root"],"attachments":{}}
          ]
        }"#,
        b"cat.png\n",
    )
    .expect("skin diagnostic fixture should load")
    .into_asset();
    let outfit = asset.skin_id("outfit").expect("skin exists");
    let scope = DiagnosticScope::Skin(outfit);
    let mut skeleton = Skeleton::new(Arc::clone(&asset));

    let unselected = skeleton.editable_pose().solve();
    assert_inactive_diagnostic(&unselected, DiagnosticCode::IgnoredSkinBones, scope);
    drop(unselected);

    skeleton
        .set_skin_layers(&[outfit])
        .expect("skin is asset-local");
    let selected = skeleton.editable_pose().solve();
    assert_active_diagnostic(&selected, DiagnosticCode::IgnoredSkinBones, scope);
    drop(selected);

    skeleton
        .set_skin_layers(&[])
        .expect("an empty skin stack is valid");
    let cleared = skeleton.editable_pose().solve();
    assert_inactive_diagnostic(&cleared, DiagnosticCode::IgnoredSkinBones, scope);
}

#[test]
fn ik_diagnostic_follows_the_evaluated_mix() {
    let asset = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[
            {"name":"root"},
            {"name":"cat","parent":"root"},
            {"name":"target","parent":"root","x":10}
          ],
          "constraints":[{
            "name":"aim",
            "type":"ik",
            "bones":["cat"],
            "target":"target",
            "mix":0,
            "compress":true
          }]
        }"#,
        b"cat.png\n",
    )
    .expect("IK diagnostic fixture should load")
    .into_asset();
    let aim = asset.ik_constraint_id("aim").expect("IK exists");
    let scope = DiagnosticScope::IkConstraint(aim);
    let mut skeleton = Skeleton::new(Arc::clone(&asset));

    let setup = skeleton.editable_pose().solve();
    assert_inactive_diagnostic(&setup, DiagnosticCode::UnsupportedConstraintOption, scope);
    drop(setup);

    let mut pose = skeleton.editable_pose();
    pose.edit()
        .set_ik_mix(aim, Mix::ONE)
        .expect("constraint is asset-local");
    let active = pose.solve();
    assert_active_diagnostic(&active, DiagnosticCode::UnsupportedConstraintOption, scope);
    drop(active);

    let mut pose = skeleton.editable_pose();
    pose.edit()
        .set_ik_mix(aim, Mix::ZERO)
        .expect("constraint is asset-local");
    let inactive = pose.solve();
    assert_inactive_diagnostic(
        &inactive,
        DiagnosticCode::UnsupportedConstraintOption,
        scope,
    );
}

#[test]
fn atlas_page_and_region_diagnostics_follow_visible_regions() {
    let asset = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[{"name":"root"}],
          "slots":[{"name":"visual","bone":"root","attachment":"body"}],
          "skins":[{
            "name":"default",
            "attachments":{
              "visual":{
                "body":{"path":"body","width":8,"height":8}
              }
            }
          }],
          "animations":{
            "hide":{"slots":{"visual":{"attachment":[{"name":"body"},{"time":1,"name":null}]}}}
          }
        }"#,
        b"cat.png\n\tsize: 8, 8\n\tpma: true\nbody\n\tbounds: 0, 0, 8, 8\n\trotate: 45\n",
    )
    .expect("atlas diagnostic fixture should load")
    .into_asset();
    let hide = asset.animation_id("hide").expect("animation exists");
    let page = asset.atlas_page_id("cat.png").expect("page exists");
    let region = asset
        .atlas_regions()
        .find(|candidate| candidate.name() == "body")
        .expect("region exists")
        .id();
    let mut skeleton = Skeleton::new(Arc::clone(&asset));

    let visible = skeleton.editable_pose().solve();
    assert_active_diagnostic(
        &visible,
        DiagnosticCode::AlphaEncodingMismatch,
        DiagnosticScope::AtlasPage(page),
    );
    assert_active_diagnostic(
        &visible,
        DiagnosticCode::UnsupportedAtlasRotation,
        DiagnosticScope::AtlasRegion(region),
    );
    drop(visible);

    skeleton
        .sample_animation(hide, Duration::from_secs(1), PlaybackMode::Once)
        .expect("animation is asset-local");
    let hidden = skeleton.editable_pose().solve();
    assert_inactive_diagnostic(
        &hidden,
        DiagnosticCode::AlphaEncodingMismatch,
        DiagnosticScope::AtlasPage(page),
    );
    assert_inactive_diagnostic(
        &hidden,
        DiagnosticCode::UnsupportedAtlasRotation,
        DiagnosticScope::AtlasRegion(region),
    );
}

#[test]
fn event_diagnostic_is_emission_scoped_and_never_frame_active() {
    let asset = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[{"name":"root"}],
          "events":{"cue":{"futurePayload":true}},
          "animations":{"signal":{"events":[{"name":"cue"}]}}
        }"#,
        b"cat.png\n",
    )
    .expect("event diagnostic fixture should load")
    .into_asset();
    let cue = asset.event_id("cue").expect("event exists");
    let signal = asset.animation_id("signal").expect("animation exists");
    let scope = DiagnosticScope::Event(cue);
    let mut skeleton = Skeleton::new(Arc::clone(&asset));

    let setup = skeleton.editable_pose().solve();
    assert_inactive_diagnostic(&setup, DiagnosticCode::UnknownField, scope);
    drop(setup);

    let mut player = AnimationPlayer::new(&skeleton);
    player
        .play(signal, PlayOptions::once())
        .expect("animation is asset-local");
    let mut emitted = false;
    let frame = player
        .update(
            &mut skeleton,
            Duration::ZERO,
            &mut |event: AnimationEvent<'_>| {
                emitted = true;
                assert!(event.has_degradations());
                assert!(event.diagnostics().any(|diagnostic| {
                    diagnostic.code() == DiagnosticCode::UnknownField && diagnostic.scope() == scope
                }));
            },
        )
        .expect("player is bound to the skeleton")
        .solve();
    assert!(emitted);
    assert_inactive_diagnostic(&frame, DiagnosticCode::UnknownField, scope);
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
                    _future => panic!("the deterministic fixture contains only regions"),
                };
                std::hint::black_box(region.positions());
            }
        }
    });

    assert_eq!(allocations.count_total, 0);
    assert_eq!(allocations.bytes_total, 0);
}

const DEFORM_STEADY_STATE_JSON: &str = r#"{
  "skeleton":{"spine":"4.3.23"},
  "bones":[
    {"name":"root"},
    {"name":"quad-bone","parent":"root"}
  ],
  "slots":[{"name":"quad-slot","bone":"quad-bone","attachment":"quad"}],
  "skins":[{
    "name":"default",
    "attachments":{"quad-slot":{"quad":{
      "type":"mesh",
      "uvs":[0,0,1,0,1,1,0,1],
      "triangles":[0,1,2,0,2,3],
      "vertices":[-1,-1,1,-1,1,1,-1,1],
      "hull":4,
      "width":2,
      "height":2
    }}}
  }],
  "animations":{
    "idle":{
      "bones":{"quad-bone":{"rotate":[{"value":0},{"time":1,"value":0}]}},
      "attachments":{"default":{"quad-slot":{"quad":{"deform":[{},{"time":1}]}}}}
    },
    "turn":{
      "bones":{"quad-bone":{"rotate":[{"value":90},{"time":1,"value":90}]}},
      "attachments":{"default":{"quad-slot":{"quad":{"deform":[
        {"offset":0,"vertices":[0.5,-0.5]},
        {"time":1,"offset":0,"vertices":[0.5,-0.5]}
      ]}}}}
    }
  }
}"#;

const DEFORM_STEADY_STATE_ATLAS: &str = "\
quad.png
\tsize: 2, 2
quad
\tbounds: 0, 0, 2, 2
";

/// A deform-bearing companion to
/// [`steady_state_player_crossfade_solve_and_draw_allocate_nothing`],
/// covering `Skeleton::sample_animation`'s `TimelineData::Deform` arm and
/// `update_mesh_world_positions`'s per-component deform blending, neither
/// of which the region-only shared fixture above ever exercises. `idle`
/// carries an unauthored (all-zero) deform key and `turn` carries a
/// nonzero one, so the crossfade also exercises deform's "snap" behavior
/// (see [`spinal::Transition::Crossfade`]'s doc) every step.
#[test]
fn steady_state_player_crossfade_with_deform_solve_and_draw_allocate_nothing() {
    let asset = load_json(
        DEFORM_STEADY_STATE_JSON.as_bytes(),
        DEFORM_STEADY_STATE_ATLAS.as_bytes(),
    )
    .expect("the deform-bearing steady-state fixture should load")
    .into_asset();
    let mut skeleton = Skeleton::new(Arc::clone(&asset));
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
                match item {
                    spinal::DrawItemRef::Mesh(mesh) => {
                        std::hint::black_box(mesh.positions());
                    }
                    _future => panic!("the deterministic fixture contains only one mesh"),
                }
            }
        }
    });

    assert_eq!(allocations.count_total, 0);
    assert_eq!(allocations.bytes_total, 0);
}

#[derive(Debug, PartialEq)]
struct SolvedFrameSnapshot {
    report: UpdateReport,
    bones: Vec<(BoneId, BoneTransform, WorldTransform)>,
    slots: Vec<(SlotId, Rgba, Option<AttachmentId>)>,
    draws: Vec<RegionSnapshot>,
    ik_statuses: Vec<(IkConstraintId, IkSolveStatus)>,
    diagnostics: Vec<(DiagnosticSeverity, DiagnosticCode, DiagnosticScope)>,
    has_degradations: bool,
    has_runtime_degradations: bool,
}

#[derive(Debug, PartialEq)]
struct RegionSnapshot {
    slot: SlotId,
    attachment: AttachmentId,
    atlas_page: AtlasPageId,
    atlas_region: AtlasRegionId,
    positions: [spinal::glam::Vec2; 4],
    uvs: Option<[spinal::glam::Vec2; 4]>,
    color: Rgba,
    blend_mode: SlotBlendMode,
}

fn deterministic_solved_snapshot(asset: &Arc<SkeletonAsset>) -> SolvedFrameSnapshot {
    let idle = asset.animation_id("idle").expect("animation exists");
    let turn = asset.animation_id("turn").expect("animation exists");
    let aim = asset.ik_constraint_id("aim").expect("IK exists");
    let mut skeleton = Skeleton::new(Arc::clone(asset));
    let mut player = AnimationPlayer::new(&skeleton);

    player
        .play(idle, PlayOptions::looping())
        .expect("animation is asset-local");
    let initial = player
        .update(&mut skeleton, Duration::from_millis(125), &mut ())
        .expect("player is bound to the skeleton")
        .solve();
    drop(initial);

    player
        .play(
            turn,
            PlayOptions::looping().with_transition(Transition::Crossfade(Crossfade::new(
                Duration::from_secs(1),
            ))),
        )
        .expect("animation is asset-local");
    let mut pose = player
        .update(&mut skeleton, Duration::from_millis(375), &mut ())
        .expect("player is bound to the skeleton");
    pose.edit()
        .set_ik_mix(aim, Mix::new(0.5).expect("half mix is normalized"))
        .expect("constraint is asset-local");

    snapshot_solved_frame(&pose.solve())
}

fn snapshot_solved_frame(frame: &SolvedFrame<'_>) -> SolvedFrameSnapshot {
    SolvedFrameSnapshot {
        report: frame.report(),
        bones: frame
            .bones()
            .map(|bone| (bone.id(), bone.local_transform(), bone.world_transform()))
            .collect(),
        slots: frame
            .slots()
            .map(|slot| (slot.id(), slot.color(), slot.attachment()))
            .collect(),
        draws: frame
            .draw_items()
            .map(|item| {
                let region = match item {
                    DrawItemRef::Region(region) => region,
                    _future => {
                        panic!("the deterministic fixture emits only rigid region draw items")
                    }
                };
                RegionSnapshot {
                    slot: region.slot(),
                    attachment: region.attachment(),
                    atlas_page: region.atlas_page(),
                    atlas_region: region.atlas_region(),
                    positions: region.positions(),
                    uvs: region.uvs(),
                    color: region.color(),
                    blend_mode: region.blend_mode(),
                }
            })
            .collect(),
        ik_statuses: frame.ik_statuses().collect(),
        diagnostics: frame
            .active_diagnostics()
            .map(|diagnostic| (diagnostic.severity(), diagnostic.code(), diagnostic.scope()))
            .collect(),
        has_degradations: frame.has_degradations(),
        has_runtime_degradations: frame.has_runtime_degradations(),
    }
}

fn assert_active_diagnostic(frame: &SolvedFrame<'_>, code: DiagnosticCode, scope: DiagnosticScope) {
    assert!(
        frame
            .active_diagnostics()
            .any(|diagnostic| diagnostic.code() == code && diagnostic.scope() == scope),
        "expected active diagnostic {code:?} at {scope:?}"
    );
}

fn assert_inactive_diagnostic(
    frame: &SolvedFrame<'_>,
    code: DiagnosticCode,
    scope: DiagnosticScope,
) {
    assert!(
        !frame
            .active_diagnostics()
            .any(|diagnostic| diagnostic.code() == code && diagnostic.scope() == scope),
        "expected inactive diagnostic {code:?} at {scope:?}"
    );
}

fn assert_vec2_near(actual: spinal::glam::Vec2, expected: [f32; 2]) {
    assert!((actual.x - expected[0]).abs() < 1.0e-4);
    assert!((actual.y - expected[1]).abs() < 1.0e-4);
}

fn world_rotation(transform: WorldTransform) -> f32 {
    transform
        .x_axis()
        .y
        .atan2(transform.x_axis().x)
        .to_degrees()
}

fn assert_angle_near(actual: f32, expected: f32) {
    let difference = (actual - expected + 180.0).rem_euclid(360.0) - 180.0;
    assert!(
        difference.abs() < 1.0e-3,
        "expected {expected} degrees, got {actual} degrees"
    );
}

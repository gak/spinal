//! Compatibility tripwires exercised against untracked, exact 4.3.23 exports.

use std::{
    collections::HashSet,
    env,
    f32::consts::{PI, TAU},
    fs,
    path::Path,
    sync::Arc,
    time::Duration,
};

use serde_json::Value;
use spinal::{
    AnimationMixer, AnimationPlayer, BoneTransform, Crossfade, DiagnosticCode, IkTargetReach,
    PlayOptions, PlaybackMode, Skeleton, SkeletonAsset, TrackOptions, TransformMix, Transition,
    load_json,
};

const FIXTURE_ROOT_ENV: &str = "SPINAL_4_3_23_FIXTURES";
const AIM_PREVIEW_ENV: &str = "SPINAL_SPINEBOY_AIM_PREVIEW";

struct Expected {
    directory: &'static str,
    stem: &'static str,
    bones: usize,
    slots: usize,
    skins: usize,
    attachments: usize,
    animations: usize,
    ik_constraints: usize,
    transform_constraints: usize,
    constraints: usize,
    atlas_regions: usize,
    meshes: usize,
    weighted_meshes: usize,
    diagnostic_codes: &'static [DiagnosticCode],
}

const ESSENTIAL: Expected = Expected {
    directory: "ess",
    stem: "spineboy-ess",
    bones: 18,
    slots: 20,
    skins: 1,
    attachments: 27,
    animations: 8,
    ik_constraints: 0,
    transform_constraints: 0,
    constraints: 0,
    atlas_regions: 26,
    meshes: 0,
    weighted_meshes: 0,
    diagnostic_codes: &[
        DiagnosticCode::UnsupportedAttachmentType,
        DiagnosticCode::UnsupportedBlendMode,
        DiagnosticCode::AlphaEncodingMismatch,
    ],
};

const PROFESSIONAL: Expected = Expected {
    directory: "pro",
    stem: "spineboy-pro",
    bones: 67,
    slots: 52,
    skins: 1,
    attachments: 80,
    animations: 11,
    ik_constraints: 7,
    transform_constraints: 7,
    constraints: 14,
    atlas_regions: 40,
    meshes: 12,
    weighted_meshes: 10,
    diagnostic_codes: &[
        DiagnosticCode::UnsupportedAttachmentType,
        DiagnosticCode::UnsupportedBoneTransformMode,
        DiagnosticCode::UnsupportedConstraintOption,
        DiagnosticCode::UnsupportedTimelineType,
        DiagnosticCode::UnsupportedBlendMode,
        DiagnosticCode::AlphaEncodingMismatch,
    ],
};

#[test]
#[ignore = "requires external fixtures; see github.com/gak/spinal/blob/main/fixtures/README.md"]
fn official_spineboy_exports_are_exact_version_compatibility_tripwires() {
    let root = env::var_os(FIXTURE_ROOT_ENV).unwrap_or_else(|| {
        panic!(
            "{FIXTURE_ROOT_ENV} must point at the external fixture root; \
             see https://github.com/gak/spinal/blob/main/fixtures/README.md"
        )
    });
    let root = Path::new(&root);

    validate_fixture(root, &ESSENTIAL);
    validate_fixture(root, &PROFESSIONAL);
}

#[test]
#[ignore = "requires the derived rigid aiming preview; see github.com/gak/spinal/blob/main/fixtures/README.md"]
fn prepared_spineboy_aim_preview_draws_while_the_base_changes_and_target_moves() {
    let root = env::var_os(AIM_PREVIEW_ENV).unwrap_or_else(|| {
        panic!(
            "{AIM_PREVIEW_ENV} must point at a preview produced by \
             tools/prepare-spineboy-aim-preview.sh"
        )
    });
    let root = Path::new(&root);
    let report = load_json(
        &read(&root.join("spineboy-rigid-aim.json")),
        &read(&root.join("spineboy-rigid-aim.atlas")),
    )
    .expect("the derived rigid aiming preview loads");
    let asset = report.into_asset();
    let walk = asset.animation_id("walk").expect("preview retains walk");
    let run = asset.animation_id("run").expect("preview retains run");
    let aim = asset.animation_id("aim").expect("preview retains aim");
    let crosshair = asset
        .bone_id("crosshair")
        .expect("preview retains the control bone");
    let rear_arm = asset
        .bone_id("rear-upper-arm")
        .expect("preview retains the aiming arm");

    let mut skeleton = Skeleton::new(Arc::clone(&asset));
    let mut mixer = AnimationMixer::new(&skeleton);
    mixer
        .base_track_mut()
        .play(walk, PlayOptions::looping())
        .expect("walk starts");
    let aim_track = mixer
        .insert_track(TrackOptions::override_track())
        .expect("aim track is inserted");
    mixer
        .track_mut(aim_track)
        .expect("aim track exists")
        .play(aim, PlayOptions::looping())
        .expect("aim starts");
    let aim_playback = mixer
        .track(aim_track)
        .expect("aim track exists")
        .status()
        .playback()
        .expect("aim playback is observable");

    let first_target = spinal::glam::Vec2::new(360.0, 420.0);
    let mut pose = mixer
        .update(&mut skeleton, Duration::ZERO, &mut ())
        .expect("walk plus aim updates");
    pose.targets()
        .set_skeleton_position(crosshair, first_target)
        .expect("the first mouse target is finite and reachable");
    let frame = pose.solve();
    assert!(
        frame.draw_items().count() > 0,
        "the supported preview must render the rigid Spineboy regions"
    );
    assert_points_at(&frame, rear_arm, crosshair, 0.999);
    drop(frame);

    mixer
        .base_track_mut()
        .play(
            run,
            PlayOptions::looping().with_transition(Transition::Crossfade(Crossfade::new(
                Duration::from_millis(200),
            ))),
        )
        .expect("base crossfades to run");
    let second_target = spinal::glam::Vec2::new(-220.0, 310.0);
    let mut pose = mixer
        .update(&mut skeleton, Duration::from_millis(100), &mut ())
        .expect("run crossfade plus aim updates");
    pose.targets()
        .set_skeleton_position(crosshair, second_target)
        .expect("the second mouse target is finite and reachable");
    let frame = pose.solve();
    assert!(
        frame.draw_items().count() > 0,
        "the base crossfade must retain drawable rigid regions"
    );
    assert_points_at(&frame, rear_arm, crosshair, 0.999);
    assert_eq!(
        mixer
            .track(aim_track)
            .expect("aim track survives the base change")
            .status()
            .playback(),
        Some(aim_playback),
        "changing the base must not restart or replace aim"
    );
}

#[test]
#[ignore = "requires the derived rigid aiming preview; see github.com/gak/spinal/blob/main/fixtures/README.md"]
fn prepared_spineboy_base_crossfades_never_spin_the_aimed_head() {
    let root = env::var_os(AIM_PREVIEW_ENV).unwrap_or_else(|| {
        panic!(
            "{AIM_PREVIEW_ENV} must point at a preview produced by \
             tools/prepare-spineboy-aim-preview.sh"
        )
    });
    let root = Path::new(&root);
    let report = load_json(
        &read(&root.join("spineboy-rigid-aim.json")),
        &read(&root.join("spineboy-rigid-aim.atlas")),
    )
    .expect("the derived rigid aiming preview loads");
    let asset = report.into_asset();
    let aim = asset.animation_id("aim").expect("preview retains aim");
    let crosshair = asset
        .bone_id("crosshair")
        .expect("preview retains the control bone");
    let head = asset.bone_id("head").expect("preview retains the head");
    let base_animations = ["idle", "run", "walk"];
    let target = spinal::glam::Vec2::new(360.0, 420.0);

    for from_name in base_animations {
        for to_name in base_animations {
            if from_name == to_name {
                continue;
            }
            let from = asset
                .animation_id(from_name)
                .unwrap_or_else(|| panic!("preview retains {from_name}"));
            let to = asset
                .animation_id(to_name)
                .unwrap_or_else(|| panic!("preview retains {to_name}"));
            let mut skeleton = Skeleton::new(Arc::clone(&asset));
            let mut mixer = AnimationMixer::new(&skeleton);
            mixer
                .base_track_mut()
                .play(from, PlayOptions::looping())
                .unwrap_or_else(|error| panic!("{from_name} starts: {error}"));
            let aim_track = mixer
                .insert_track(TrackOptions::override_track())
                .expect("aim track is inserted");
            mixer
                .track_mut(aim_track)
                .expect("aim track exists")
                .play(aim, PlayOptions::looping())
                .expect("aim starts");

            let mut pose = mixer
                .update(&mut skeleton, Duration::from_millis(500), &mut ())
                .unwrap_or_else(|error| panic!("{from_name} warms up: {error}"));
            pose.targets()
                .set_skeleton_position(crosshair, target)
                .expect("the mouse target is finite and reachable");
            let frame = pose.solve();
            let mut previous = world_rotation_radians(&frame, head);
            drop(frame);

            mixer
                .base_track_mut()
                .play(
                    to,
                    PlayOptions::looping().with_transition(Transition::Crossfade(Crossfade::new(
                        Duration::from_millis(200),
                    ))),
                )
                .unwrap_or_else(|error| panic!("{from_name} -> {to_name} starts: {error}"));

            let mut travelled = 0.0_f32;
            for _frame_index in 0..20 {
                let mut pose = mixer
                    .update(&mut skeleton, Duration::from_millis(10), &mut ())
                    .unwrap_or_else(|error| panic!("{from_name} -> {to_name} updates: {error}"));
                pose.targets()
                    .set_skeleton_position(crosshair, target)
                    .expect("the mouse target remains finite and reachable");
                let frame = pose.solve();
                let current = world_rotation_radians(&frame, head);
                let delta = shortest_delta(previous, current);
                travelled += delta.abs();
                previous = current;
            }

            assert!(
                travelled < PI,
                "{from_name} -> {to_name} rotated the aimed head through {:.1} degrees",
                travelled.to_degrees()
            );
        }
    }
}

fn world_rotation_radians(frame: &spinal::SolvedFrame<'_>, bone: spinal::BoneId) -> f32 {
    let axis = frame
        .bone(bone)
        .expect("the solved bone belongs to the preview")
        .world_transform()
        .x_axis();
    axis.y.atan2(axis.x)
}

fn shortest_delta(from: f32, to: f32) -> f32 {
    let wrapped = (to - from).rem_euclid(TAU);
    if wrapped > PI { wrapped - TAU } else { wrapped }
}

fn validate_fixture(root: &Path, expected: &Expected) {
    let directory = root.join(expected.directory);
    let json = read(&directory.join(format!("{}.json", expected.stem)));
    let atlas = read(&directory.join(format!("{}.atlas", expected.stem)));
    if expected.stem == "spineboy-pro" {
        assert_professional_export_presence(&json);
    }
    let report = load_json(&json, &atlas).unwrap_or_else(|error| {
        panic!(
            "{} must load as a bounded exact-version export: {error}",
            expected.stem
        )
    });
    let asset = report.into_asset();

    assert_eq!(asset.spine_version(), "4.3.23", "{}", expected.stem);
    assert_eq!(asset.bones().len(), expected.bones, "{}", expected.stem);
    assert_eq!(asset.slots().len(), expected.slots, "{}", expected.stem);
    assert_eq!(asset.skins().len(), expected.skins, "{}", expected.stem);
    assert_eq!(
        asset.attachments().len(),
        expected.attachments,
        "{}",
        expected.stem
    );
    assert_eq!(
        asset.animations().len(),
        expected.animations,
        "{}",
        expected.stem
    );
    assert_eq!(
        asset.ik_constraints().len(),
        expected.ik_constraints,
        "{}",
        expected.stem
    );
    assert_eq!(
        asset.transform_constraints().len(),
        expected.transform_constraints,
        "{}",
        expected.stem
    );
    assert_eq!(
        asset.constraints().len(),
        expected.constraints,
        "{}",
        expected.stem
    );
    assert_eq!(asset.atlas_pages().len(), 1, "{}", expected.stem);
    assert_eq!(
        asset.atlas_regions().len(),
        expected.atlas_regions,
        "{}",
        expected.stem
    );
    let meshes = asset
        .attachments()
        .filter_map(|attachment| attachment.as_mesh())
        .collect::<Vec<_>>();
    assert_eq!(meshes.len(), expected.meshes, "{}", expected.stem);
    assert_eq!(
        meshes.iter().filter(|mesh| mesh.is_weighted()).count(),
        expected.weighted_meshes,
        "{}",
        expected.stem
    );
    for mesh in meshes {
        assert_eq!(mesh.vertex_count(), mesh.uvs().len(), "{}", expected.stem);
        assert!(
            mesh.triangles()
                .iter()
                .all(|index| (*index as usize) < mesh.vertex_count()),
            "{} retains valid indexed mesh topology",
            expected.stem
        );
    }
    let actual_codes = asset
        .diagnostics()
        .iter()
        .map(spinal::Diagnostic::code)
        .collect::<HashSet<_>>();
    let expected_codes = expected
        .diagnostic_codes
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    assert_eq!(
        actual_codes,
        expected_codes,
        "{} must diagnose every unsupported feature family without inventing others; \
         actual diagnostics: {:#?}",
        expected.stem,
        asset.diagnostics()
    );

    if expected.stem == "spineboy-pro" {
        exercise_absolute_curve_regressions(&asset);
        exercise_professional_leg_ik(&asset);
        exercise_professional_aim(&asset);
        exercise_professional_mesh_uv_origin(&asset);
    }
    exercise_every_animation(asset, expected.stem);
}

fn exercise_professional_mesh_uv_origin(asset: &Arc<SkeletonAsset>) {
    let mut skeleton = Skeleton::new(Arc::clone(asset));
    let frame = skeleton.editable_pose().solve();
    let head = frame
        .draw_items()
        .find_map(|draw| match draw {
            spinal::DrawItemRef::Mesh(mesh)
                if asset
                    .attachment(mesh.attachment())
                    .is_ok_and(|attachment| attachment.name() == "head") =>
            {
                Some(mesh)
            }
            _other => None,
        })
        .expect("the exact Professional setup pose draws its head mesh");
    let first = head
        .uvs()
        .expect("the exact atlas declares its page size")
        .next()
        .expect("the head mesh has vertices");

    assert!((first.x - 0.889_404_1).abs() < 1.0e-6);
    assert!((first.y - 0.730_076_4).abs() < 1.0e-6);
}

fn assert_professional_export_presence(json: &[u8]) {
    let root: Value = serde_json::from_slice(json).expect("the checksummed Pro JSON is valid");
    let constraints = root["constraints"]
        .as_array()
        .expect("the Pro export has unified constraints");
    assert_eq!(
        constraints
            .iter()
            .filter(|constraint| constraint["type"] == "ik")
            .count(),
        7
    );
    let ik_arities = constraints
        .iter()
        .filter(|constraint| constraint["type"] == "ik")
        .map(|constraint| {
            constraint["bones"]
                .as_array()
                .expect("each Pro IK constraint lists its constrained bones")
                .len()
        })
        .collect::<HashSet<_>>();
    assert_eq!(
        ik_arities,
        HashSet::from([1, 2]),
        "the exact Pro export must exercise both supported IK chain sizes"
    );
    assert_eq!(
        constraints
            .iter()
            .filter(|constraint| constraint["type"] == "transform")
            .count(),
        7
    );

    let mut weighted_meshes = 0;
    let mut unweighted_meshes = 0;
    let mut clipping = 0;
    let mut bounding_boxes = 0;
    for skin in root["skins"].as_array().expect("the Pro export has skins") {
        for slot in skin["attachments"]
            .as_object()
            .expect("skin attachments are grouped by slot")
            .values()
        {
            for attachment in slot
                .as_object()
                .expect("slot attachments are named objects")
                .values()
            {
                match attachment["type"].as_str().unwrap_or("region") {
                    "mesh" => {
                        let vertices = attachment["vertices"]
                            .as_array()
                            .expect("mesh vertices are exported")
                            .len();
                        let uvs = attachment["uvs"]
                            .as_array()
                            .expect("mesh UVs are exported")
                            .len();
                        if vertices == uvs {
                            unweighted_meshes += 1;
                        } else {
                            weighted_meshes += 1;
                        }
                    }
                    "clipping" => clipping += 1,
                    "boundingbox" => bounding_boxes += 1,
                    _other => {}
                }
            }
        }
    }
    assert!(weighted_meshes > 0);
    assert!(unweighted_meshes > 0);
    assert_eq!(clipping, 1);
    assert_eq!(bounding_boxes, 1);

    let animations = root["animations"]
        .as_object()
        .expect("the Pro export has animations");
    let deform_frames = animations
        .values()
        .flat_map(|animation| {
            animation["attachments"]
                .as_object()
                .into_iter()
                .flat_map(|skins| skins.values())
        })
        .flat_map(|skin| {
            skin.as_object()
                .into_iter()
                .flat_map(|slots| slots.values())
        })
        .flat_map(|slot| {
            slot.as_object()
                .into_iter()
                .flat_map(|attachments| attachments.values())
        })
        .filter_map(|attachment| attachment["deform"].as_array())
        .map(Vec::len)
        .sum::<usize>();
    assert!(deform_frames > 0);

    let attachment_frames = animations
        .values()
        .flat_map(|animation| {
            animation["slots"]
                .as_object()
                .into_iter()
                .flat_map(|slots| slots.values())
        })
        .filter_map(|slot| slot["attachment"].as_array())
        .map(Vec::len)
        .sum::<usize>();
    assert!(
        attachment_frames > 0,
        "the exact Pro export must exercise attachment timelines"
    );

    let softness_frames = animations
        .values()
        .flat_map(|animation| {
            animation["ik"]
                .as_object()
                .into_iter()
                .flat_map(|constraints| constraints.values())
        })
        .flat_map(|timeline| {
            timeline
                .as_array()
                .into_iter()
                .flat_map(|frames| frames.iter())
        })
        .filter(|frame| frame.get("softness").is_some())
        .count();
    assert!(softness_frames > 0);
    assert!(animations.values().any(|animation| {
        animation["transform"]
            .as_object()
            .is_some_and(|value| !value.is_empty())
    }));
    assert!(
        root["slots"]
            .as_array()
            .expect("the Pro export has slots")
            .iter()
            .any(|slot| slot["blend"] == "additive")
    );
    assert!(
        root["bones"]
            .as_array()
            .expect("the Pro export has bones")
            .iter()
            .any(|bone| bone["inherit"] == "noRotationOrReflection")
    );
}

fn exercise_absolute_curve_regressions(asset: &Arc<SkeletonAsset>) {
    let death = asset.animation_id("death").expect("death animation exists");
    let hair = asset.bone_id("hair2").expect("hair2 bone exists");
    let mut skeleton = Skeleton::new(Arc::clone(asset));
    let rotation_at = |skeleton: &mut Skeleton, position| {
        skeleton
            .sample_animation(death, position, PlaybackMode::Once)
            .expect("death samples");
        skeleton
            .bone_pose(hair)
            .expect("hair2 belongs to the skeleton")
            .local_transform()
            .rotation()
            .as_degrees()
    };
    let start = rotation_at(&mut skeleton, Duration::from_nanos(1_266_666_700));
    let middle = rotation_at(&mut skeleton, Duration::from_nanos(1_633_333_350));
    let end = rotation_at(&mut skeleton, Duration::from_secs(2));
    assert!(
        (start - end).abs() < 1.0e-4,
        "the exact fixture has equal endpoint values"
    );
    assert!(
        (middle - start).abs() > 0.05,
        "absolute Bezier value handles must preserve an excursion between equal endpoints"
    );

    let shoot = asset.animation_id("shoot").expect("shoot animation exists");
    let muzzle_ring = asset
        .slot_id("muzzle-ring")
        .expect("muzzle-ring slot exists");
    skeleton
        .sample_animation(shoot, Duration::from_nanos(133_333_330), PlaybackMode::Once)
        .expect("shoot samples");
    let alpha = skeleton
        .slot_pose(muzzle_ring)
        .expect("muzzle-ring belongs to the skeleton")
        .color()
        .alpha();
    assert!(
        (alpha - 0.865_46).abs() < 0.01,
        "absolute RGBA handles must sample in frame/value coordinates, got {alpha}"
    );
}

fn exercise_professional_leg_ik(asset: &Arc<SkeletonAsset>) {
    let walk = asset.animation_id("walk").expect("walk animation exists");
    for position in [
        Duration::ZERO,
        Duration::from_millis(250),
        Duration::from_millis(500),
    ] {
        let mut skeleton = Skeleton::new(Arc::clone(asset));
        skeleton
            .sample_animation(walk, position, PlaybackMode::Loop)
            .expect("walk samples");
        let frame = skeleton.editable_pose().solve();

        for constraint_name in ["front-leg-ik", "rear-leg-ik"] {
            let constraint_id = asset
                .ik_constraint_id(constraint_name)
                .expect("the professional export contains both leg IK constraints");
            let constraint = asset
                .ik_constraint(constraint_id)
                .expect("leg IK belongs to the professional asset");
            let [parent, child] = constraint
                .bones()
                .collect::<Vec<_>>()
                .try_into()
                .expect("leg IK is a two-bone chain");
            let child_frame = frame.bone(child).expect("the child belongs to the frame");
            let child_local = child_frame.local_transform();
            let bend_measure =
                (child_local.rotation().as_radians() + child_local.shear().x().as_radians()).sin();
            assert!(
                bend_measure <= 1.0e-4,
                "{constraint_name} at {position:?} must preserve Spine's exported negative bend direction, got {bend_measure} (rotation={}°, shear={}°, setup bend={:?}, local={:?}, parent={:?}, target={:?})",
                child_local.rotation().as_degrees(),
                child_local.shear().x().as_degrees(),
                constraint.bend_direction(),
                child_local,
                frame.bone(parent).unwrap().world_transform(),
                frame.bone(constraint.target()).unwrap().world_transform(),
            );

            let target = frame
                .bone(constraint.target())
                .expect("the IK target belongs to the frame")
                .world_transform()
                .translation();
            let child_tip = child_frame
                .world_transform()
                .transform_point(spinal::glam::Vec2::new(
                    asset.bone(child).unwrap().length(),
                    0.0,
                ));
            let status = frame.ik_status(constraint_id).unwrap();
            match status.target_reach() {
                Some(IkTargetReach::Reachable) => assert!(
                    child_tip.distance(target) < 0.1,
                    "{constraint_name} at {position:?} must place a reachable child tip on its target (distance {})",
                    child_tip.distance(target)
                ),
                Some(IkTargetReach::BeyondReach) => assert!(
                    child_tip.distance(target).is_finite(),
                    "{constraint_name} at {position:?} must retain a finite closest pose"
                ),
                None => panic!("{constraint_name} at {position:?} did not report target reach"),
                Some(_) => panic!("{constraint_name} at {position:?} reported an unknown reach"),
            }
            assert_eq!(
                status.issue(),
                None,
                "{constraint_name} must solve without a runtime fallback"
            );
            assert_eq!(
                frame.bone(parent).unwrap().id(),
                parent,
                "the authored leg parent remains the solved chain root"
            );
        }
    }
}

fn exercise_professional_aim(asset: &Arc<SkeletonAsset>) {
    let aim = asset.animation_id("aim").expect("aim animation exists");
    let crosshair = asset.bone_id("crosshair").expect("crosshair exists");
    let source = asset
        .bone_id("aim-constraint-target")
        .expect("aim source exists");
    let rear_arm = asset
        .bone_id("rear-upper-arm")
        .expect("rear aiming arm exists");
    let target = spinal::glam::Vec2::new(360.0, 420.0);
    let transform_names = [
        ("aim-torso-transform", "torso", 0.423),
        ("aim-head-transform", "head", 0.659),
        ("aim-front-arm-transform", "front-upper-arm", 0.784),
    ];

    let mut actual_skeleton = Skeleton::new(Arc::clone(asset));
    actual_skeleton
        .sample_animation(aim, Duration::ZERO, PlaybackMode::Once)
        .expect("aim samples");
    move_crosshair(&mut actual_skeleton, crosshair, target);
    for (constraint_name, _bone_name, expected_mix) in transform_names {
        let constraint = asset
            .transform_constraint_id(constraint_name)
            .expect("the official aim transform is typed");
        let mix = actual_skeleton
            .transform_constraint_pose(constraint)
            .unwrap()
            .mix_rotate()
            .get();
        assert!(
            (mix - expected_mix).abs() < 1.0e-6,
            "{constraint_name} must retain its aim animation mix"
        );
    }
    let actual = actual_skeleton.editable_pose().solve();
    assert_points_at(&actual, source, crosshair, 0.999_999);
    assert_points_at(&actual, rear_arm, crosshair, 0.999);

    for (constraint_name, bone_name, expected_mix) in transform_names {
        let constraint_id = asset.transform_constraint_id(constraint_name).unwrap();
        let constraint = asset.transform_constraint(constraint_id).unwrap();
        assert!(constraint.copies_rotation());
        assert!(!constraint.uses_local_source());
        assert!(!constraint.uses_local_target());
        assert!(!constraint.is_additive());
        let bone = asset.bone_id(bone_name).unwrap();

        let mut without_this_constraint = Skeleton::new(Arc::clone(asset));
        without_this_constraint
            .sample_animation(aim, Duration::ZERO, PlaybackMode::Once)
            .expect("aim samples");
        move_crosshair(&mut without_this_constraint, crosshair, target);
        {
            let mut pose = without_this_constraint.editable_pose();
            pose.edit()
                .set_transform_mix_rotate(constraint_id, TransformMix::ZERO)
                .unwrap();
            let baseline = pose.solve();
            let baseline_rotation = world_rotation(baseline.bone(bone).unwrap().world_transform());
            let source_rotation = world_rotation(baseline.bone(source).unwrap().world_transform());
            let desired = source_rotation + constraint.rotation_offset().as_degrees();
            let expected = mix_degrees(baseline_rotation, desired, expected_mix);
            let actual_rotation = world_rotation(actual.bone(bone).unwrap().world_transform());
            assert_angle_near(
                actual_rotation,
                expected,
                &format!("{constraint_name} must copy the aimed source rotation"),
            );
        }

        let status = actual.transform_status(constraint_id).unwrap();
        assert!(status.is_active(), "{constraint_name} must be active");
        assert_eq!(
            status.issue(),
            None,
            "{constraint_name} must solve without a runtime fallback"
        );
    }
}

fn move_crosshair(skeleton: &mut Skeleton, crosshair: spinal::BoneId, target: spinal::glam::Vec2) {
    let current = skeleton.bone_pose(crosshair).unwrap().local_transform();
    let mut pose = skeleton.editable_pose();
    pose.edit()
        .set_bone_local(
            crosshair,
            BoneTransform::new(target, current.rotation(), current.scale(), current.shear())
                .expect("the test target is finite"),
        )
        .unwrap();
    drop(pose);
}

fn assert_points_at(
    frame: &spinal::SolvedFrame<'_>,
    bone: spinal::BoneId,
    target: spinal::BoneId,
    minimum_dot: f32,
) {
    let bone = frame.bone(bone).unwrap().world_transform();
    let target = frame.bone(target).unwrap().world_transform().translation();
    let direction = (target - bone.translation()).normalize();
    let x_axis = bone.x_axis().normalize();
    assert!(
        direction.dot(x_axis) >= minimum_dot,
        "aiming bone must point at crosshair: dot={}",
        direction.dot(x_axis)
    );
}

fn world_rotation(transform: spinal::WorldTransform) -> f32 {
    transform
        .x_axis()
        .y
        .atan2(transform.x_axis().x)
        .to_degrees()
}

fn mix_degrees(current: f32, desired: f32, mix: f32) -> f32 {
    let difference = (desired - current + 180.0).rem_euclid(360.0) - 180.0;
    current + difference * mix
}

fn assert_angle_near(actual: f32, expected: f32, message: &str) {
    let difference = (actual - expected + 180.0).rem_euclid(360.0) - 180.0;
    assert!(
        difference.abs() < 1.0e-3,
        "{message}: expected {expected}°, got {actual}°"
    );
}

fn exercise_every_animation(asset: Arc<SkeletonAsset>, fixture_name: &str) {
    for animation in asset.animations() {
        let mut skeleton = Skeleton::new(Arc::clone(&asset));
        let samples = [
            Duration::ZERO,
            animation.duration() / 2,
            animation.duration(),
        ];
        for position in samples {
            skeleton
                .sample_animation(animation.id(), position, PlaybackMode::Once)
                .unwrap_or_else(|error| {
                    panic!(
                        "{fixture_name} animation `{}` must sample at {position:?}: {error}",
                        animation.name()
                    )
                });
            let frame = skeleton.editable_pose().solve();
            for bone in frame.bones() {
                let world = bone.world_transform();
                assert!(
                    world.translation().is_finite()
                        && world.x_axis().is_finite()
                        && world.y_axis().is_finite(),
                    "{fixture_name} animation `{}` produced a non-finite bone at {position:?}",
                    animation.name()
                );
            }
            for draw in frame.draw_items() {
                match draw {
                    spinal::DrawItemRef::Region(region) => assert!(
                        region
                            .positions()
                            .iter()
                            .all(|position| position.is_finite()),
                        "{fixture_name} animation `{}` produced non-finite region geometry at {position:?}",
                        animation.name()
                    ),
                    spinal::DrawItemRef::Mesh(mesh) => {
                        assert!(
                            mesh.positions().iter().all(|position| position.is_finite()),
                            "{fixture_name} animation `{}` produced non-finite mesh geometry at {position:?}",
                            animation.name()
                        );
                        assert_eq!(mesh.positions().len(), mesh.source_uvs().len());
                        assert!(
                            mesh.triangles()
                                .iter()
                                .all(|index| (*index as usize) < mesh.positions().len())
                        );
                    }
                    _future => {}
                }
            }
        }

        let mut skeleton = Skeleton::new(Arc::clone(&asset));
        let mut player = AnimationPlayer::new(&skeleton);
        player
            .play(animation.id(), PlayOptions::once())
            .unwrap_or_else(|error| {
                panic!(
                    "{fixture_name} animation `{}` must start in the one-track player: {error}",
                    animation.name()
                )
            });
        let mut emitted_events = Vec::new();
        let frame = player
            .update(
                &mut skeleton,
                animation.duration(),
                &mut |event: spinal::AnimationEvent<'_>| {
                    emitted_events.push(event.definition().name().to_owned());
                },
            )
            .unwrap_or_else(|error| {
                panic!(
                    "{fixture_name} animation `{}` must play to its endpoint: {error}",
                    animation.name()
                )
            })
            .solve();
        assert!(
            frame.bones().all(|bone| {
                let world = bone.world_transform();
                world.translation().is_finite()
                    && world.x_axis().is_finite()
                    && world.y_axis().is_finite()
            }),
            "{fixture_name} animation `{}` produced a non-finite player frame",
            animation.name()
        );
        assert!(
            emitted_events
                .iter()
                .all(|name| asset.event_id(name).is_some()),
            "{fixture_name} animation `{}` emitted an unknown event",
            animation.name()
        );
    }

    let animations = asset
        .animations()
        .map(|animation| animation.id())
        .collect::<Vec<_>>();
    if let [first, second, ..] = animations.as_slice() {
        let mut skeleton = Skeleton::new(Arc::clone(&asset));
        let mut player = AnimationPlayer::new(&skeleton);
        player
            .play(*first, PlayOptions::looping())
            .expect("the first exact-export animation belongs to the player");
        let first_frame = player
            .update(&mut skeleton, Duration::from_millis(16), &mut ())
            .expect("the first exact-export animation advances")
            .solve();
        assert!(
            first_frame.bones().all(|bone| {
                let world = bone.world_transform();
                world.translation().is_finite()
                    && world.x_axis().is_finite()
                    && world.y_axis().is_finite()
            }),
            "{fixture_name} first player frame is non-finite"
        );
        player
            .play(
                *second,
                PlayOptions::once().with_transition(Transition::Crossfade(Crossfade::new(
                    Duration::from_millis(100),
                ))),
            )
            .expect("the second exact-export animation interrupts safely");
        let frame = player
            .update(&mut skeleton, Duration::from_millis(50), &mut ())
            .expect("the exact-export crossfade advances")
            .solve();
        assert!(
            player.status().transition_mix().is_some(),
            "{fixture_name} must exercise a live exact-export crossfade"
        );
        assert!(
            frame.bones().all(|bone| {
                let world = bone.world_transform();
                world.translation().is_finite()
                    && world.x_axis().is_finite()
                    && world.y_axis().is_finite()
            }),
            "{fixture_name} crossfade produced a non-finite player frame"
        );
    }
}

fn read(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

/// A minimal, structurally real slice of Esoteric Software's official
/// Spineboy Professional example rig (see `assets/spineboy-pro-4.1`,
/// `LICENSE-ESOTERIC.txt`), used only to discriminate two possible deform
/// wire-format layouts against genuine editor output rather than a
/// project-authored fixture (whose own single-influence deformed vertices
/// happen to be numerically identical under either layout; see
/// `spinal/tests/deform_timeline_contract.rs`'s module doc for why that
/// fixture alone cannot prove this).
///
/// `bones[0..10]`, the `rear-foot` slot, its weighted mesh attachment, and
/// the `hoverboard` animation's `rear-foot` deform timeline are extracted
/// from `assets/spineboy-pro-4.1/spineboy-pro.json` and re-serialized here
/// (verified against that file's actual parsed values, not retyped by
/// hand): every string, number, and array element below is token-verbatim
/// from the source, but the JSON is reformatted to compact single-line
/// arrays rather than the source file's original tab-indented layout, so
/// this is not a byte-for-byte excerpt. Two deliberate differences beyond
/// formatting: the mesh attachment's nonessential `edges` field (a
/// silhouette/wireframe hint the loader does not read) is omitted, and the
/// skeleton version is changed from the source file's actual "4.1.08" to
/// "4.3.23", because Spinal's loader hard-rejects any non-4.3 minor version
/// before it would ever reach deform parsing, and the deform wire shape
/// being tested here is unrelated to that version gate. `bones[0..10]` is
/// the smallest prefix of the real bone array that is self-contained
/// (every bone's parent resolves within the prefix) and covers every bone
/// index (8 and 9) `rear-foot`'s real weighted vertices reference.
const SPINEBOY_PRO_REAR_FOOT_JSON: &[u8] = br#"{
"skeleton": { "spine": "4.3.23" },
"bones": [
  {"name":"root","rotation":0.05},
  {"name":"hip","parent":"root","y":247.27},
  {"name":"crosshair","parent":"root","x":302.83,"y":569.45,"color":"ff3f00ff"},
  {"name":"aim-constraint-target","parent":"hip","length":26.24,"rotation":19.61,"x":1.02,"y":5.62,"color":"abe323ff"},
  {"name":"rear-foot-target","parent":"root","x":61.91,"y":0.42,"color":"ff3f00ff"},
  {"name":"rear-leg-target","parent":"rear-foot-target","x":-33.91,"y":37.34,"color":"ff3f00ff"},
  {"name":"rear-thigh","parent":"hip","length":85.72,"rotation":-72.54,"x":8.91,"y":-5.63,"color":"ff000dff"},
  {"name":"rear-shin","parent":"rear-thigh","length":121.88,"rotation":-19.83,"x":86.1,"y":-1.33,"color":"ff000dff"},
  {"name":"rear-foot","parent":"rear-shin","length":51.58,"rotation":45.78,"x":121.46,"y":-0.76,"color":"ff000dff"},
  {"name":"back-foot-tip","parent":"rear-foot","length":50.3,"rotation":-0.85,"x":51.17,"y":0.24,"transform":"noRotationOrReflection","color":"ff000dff"}
],
"slots": [ {"name":"rear-foot","bone":"rear-foot","attachment":"rear-foot"} ],
"skins": [
  {
    "name": "default",
    "attachments": {
      "rear-foot": {
        "rear-foot": {
          "type": "mesh",
          "uvs": [0.48368,0.1387,0.51991,0.21424,0.551,0.27907,0.58838,0.29816,0.63489,0.32191,0.77342,0.39267,1,0.73347,1,1,0.54831,0.99883,0.31161,1,0,1,0,0.41397,0.13631,0,0.41717,0],
          "triangles": [8,3,4,8,4,5,8,5,6,8,6,7,11,1,10,3,9,2,2,10,1,12,13,0,0,11,12,1,11,0,2,9,10,3,8,9],
          "vertices": [2,8,10.45,29.41,0.90802,9,-6.74,49.62,0.09198,2,8,16.56,29.27,0.84259,9,-2.65,45.09,0.15741,2,8,21.8,29.15,0.69807,9,0.85,41.2,0.30193,2,8,25.53,31.43,0.52955,9,5.08,40.05,0.47045,2,8,30.18,34.27,0.39303,9,10.33,38.62,0.60697,2,8,44.02,42.73,0.27525,9,25.98,34.36,0.72475,2,8,76.47,47.28,0.21597,9,51.56,13.9,0.78403,2,8,88.09,36.29,0.28719,9,51.55,-2.09,0.71281,2,8,52.94,-0.73,0.47576,9,0.52,-1.98,0.52424,2,8,34.63,-20.23,0.68757,9,-26.23,-2.03,0.31243,2,8,10.44,-45.81,0.84141,9,-61.43,-2,0.15859,2,8,-15.11,-21.64,0.93283,9,-61.4,33.15,0.06717,1,8,-22.57,6.61,1,1,8,-0.76,29.67,1],
          "hull": 14,
          "width": 113,
          "height": 60
        }
      }
    }
  }
],
"animations": {
  "hoverboard": {
    "attachments": {
      "default": {
        "rear-foot": {
          "rear-foot": {
            "deform": [
              {
                "offset": 28,
                "vertices": [-1.93078,1.34782,-0.31417,2.33363,3.05122,0.33946,2.31472,-2.01678,2.17583,-2.05795,-0.04277,-2.99459,1.15429,0.26328,0.97501,-0.67169]
              }
            ]
          }
        }
      }
    }
  }
}
}"#;

#[test]
fn spineboy_pro_hoverboard_deform_only_fits_the_per_influence_domain() {
    // Skip gracefully, matching `atlas::tests::historical_owned_fixtures_are_non_normative_smoke_tests`:
    // published packages intentionally exclude these licensed example
    // assets (they are not in any crate's `include` list), so this must
    // not fail a from-crates-io or packaged build. This is not the
    // `#[ignore]`d external-fixture pattern above because these assets are
    // already vendored in the repository at `assets/`; there is nothing to
    // configure.
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets");
    if !assets.is_dir() {
        return;
    }

    // The real `rear-foot` mesh (copied verbatim above) has 14 vertices,
    // parsed here from the same embedded JSON to keep this number honest
    // and traceable rather than a bare magic constant.
    let real_source = fs::read(assets.join("spineboy-pro-4.1/spineboy-pro.json"))
        .expect("tracked historical spineboy-pro export exists");
    let real_source: serde_json::Value =
        serde_json::from_slice(&real_source).expect("tracked export is valid JSON");
    let uv_floats = real_source["skins"][0]["attachments"]["rear-foot"]["rear-foot"]["uvs"]
        .as_array()
        .expect("rear-foot mesh has uvs")
        .len();
    let vertex_count = uv_floats / 2;
    assert_eq!(
        vertex_count, 14,
        "rear-foot vertex count moved; re-derive the fixture above"
    );

    // Its weighted "vertices" wire data (also copied verbatim above) packs
    // 26 total bone contributions across those 14 vertices (several
    // vertices, like the first two, blend two bones each): a per-*vertex*
    // deform domain would be 14 * 2 = 28 floats, but the real editor wrote
    // this animation's rear-foot deform key as `offset: 28, vertices: [16
    // numbers]`, i.e. floats 28..44. Under a per-vertex domain (28 floats,
    // valid indices 0..27) that key starts already past the end and
    // Spinal's own bounds check in `deform_length_for_attachment` /
    // `parse_deform_frames` would reject it as a schema violation. It is
    // only valid under the per-influence domain this implementation uses
    // (2 floats per bone contribution, 26 * 2 = 52 floats, so 28..44 fits
    // inside 0..52). A successful load below is therefore only possible
    // because Spinal indexes deform per bone contribution, not per vertex
    // -- exactly what a project-authored fixture with only single-influence
    // deformed vertices cannot discriminate (see this file's fixture doc).
    let report = load_json(
        SPINEBOY_PRO_REAR_FOOT_JSON,
        b"page.png\n\tsize: 16, 16\nrear-foot\n\tbounds: 0, 0, 16, 16\n",
    )
    .expect(
        "the real hoverboard rear-foot deform key (offset 28, 16 floats) only fits the \
             per-influence domain (26 contributions * 2 = 52 floats); a per-vertex domain \
             (14 vertices * 2 = 28) would reject offset 28 + 16 = 44 > 28 via Spinal's own \
             deform-length bounds check",
    );
    assert!(
        report
            .diagnostics()
            .iter()
            .all(|diagnostic| diagnostic.code() != DiagnosticCode::UnsupportedTimelineType),
        "the rear-foot deform timeline itself must parse, not degrade: {:#?}",
        report.diagnostics()
    );
}

//! Compatibility tripwires exercised against untracked, exact 4.3.23 exports.

use std::{collections::HashSet, env, fs, path::Path, sync::Arc, time::Duration};

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
    }
    exercise_every_animation(asset, expected.stem);
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
                let spinal::DrawItemRef::Region(region) = draw else {
                    continue;
                };
                assert!(
                    region
                        .positions()
                        .iter()
                        .all(|position| position.is_finite()),
                    "{fixture_name} animation `{}` produced non-finite draw geometry at {position:?}",
                    animation.name()
                );
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

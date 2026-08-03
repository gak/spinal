use bevy::math::Vec2;
use spinal::{BoneId, BoneTransform, IkTargetReach, Skeleton};

use super::{rig::RigBinding, walk::WalkParameters};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SegmentKind {
    Bone,
    ParentLink,
    IkChain,
    IkLink,
    TransformConstraint,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Segment {
    pub(crate) start: Vec2,
    pub(crate) end: Vec2,
    pub(crate) kind: SegmentKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MarkerKind {
    Joint,
    IkControl,
    IkTarget,
    BodyControl,
    Problem,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Marker {
    pub(crate) position: Vec2,
    pub(crate) kind: MarkerKind,
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct Geometry {
    pub(crate) segments: Vec<Segment>,
    pub(crate) markers: Vec<Marker>,
}

/// Mirrors the example's procedural edits in a standalone skeleton and
/// returns renderer-independent debug geometry in skeleton space.
pub(crate) fn solve_geometry(
    show: bool,
    skeleton: &mut Skeleton,
    binding: &RigBinding,
    parameters: WalkParameters,
    time: f32,
) -> Geometry {
    if !show {
        return Geometry::default();
    }

    let pose = parameters.sample_preview(time);
    let (control_ids, body_id) = {
        let asset = skeleton.asset();
        let controls = binding.controls.each_ref().map(|control| {
            asset
                .bone_id(&control.name)
                .expect("a discovered control remains in its skeleton")
        });
        let body = asset
            .bone_id(&binding.body.name)
            .expect("the discovered body control remains in its skeleton");
        (controls, body)
    };

    skeleton.reset_to_setup_pose();
    let mut editable = skeleton.editable_pose();
    {
        let mut editor = editable.edit();
        for ((control, control_id), offset) in
            binding.controls.iter().zip(control_ids).zip(pose.controls)
        {
            editor
                .set_bone_local(control_id, translated(control.setup, offset))
                .expect("a discovered control ID belongs to the debug skeleton");
        }
        editor
            .set_bone_local(body_id, translated(binding.body.setup, pose.body))
            .expect("the discovered body ID belongs to the debug skeleton");
    }
    let solved = editable.solve();
    geometry_from_solved(&solved, body_id)
}

fn geometry_from_solved(solved: &spinal::SolvedFrame<'_>, body_id: BoneId) -> Geometry {
    let asset = solved.asset();
    let mut geometry = Geometry::default();

    for bone in asset.bones() {
        let world = solved
            .bone(bone.id())
            .expect("a loaded bone belongs to its solved frame")
            .world_transform();
        let origin = world.translation();
        push_marker(&mut geometry, origin, MarkerKind::Joint);
        push_bone_segment(
            &mut geometry,
            origin,
            world,
            bone.length(),
            SegmentKind::Bone,
        );

        if let Some(parent_id) = bone.parent() {
            let parent = asset
                .bone(parent_id)
                .expect("a loaded parent belongs to its asset");
            let parent_world = solved
                .bone(parent_id)
                .expect("a loaded parent belongs to its solved frame")
                .world_transform();
            let parent_tip = parent_world.transform_point(Vec2::new(parent.length(), 0.0));
            if origin.distance_squared(parent_tip) > 0.25 {
                push_segment(&mut geometry, parent_tip, origin, SegmentKind::ParentLink);
            }
        }
    }

    if let Ok(body) = solved.bone(body_id) {
        push_marker(
            &mut geometry,
            body.world_transform().translation(),
            MarkerKind::BodyControl,
        );
    }

    for constraint in asset.ik_constraints() {
        let chain_root = constraint
            .bones()
            .next()
            .expect("a loaded IK constraint has at least one constrained bone");
        for bone_id in constraint.bones() {
            let bone = asset
                .bone(bone_id)
                .expect("an IK chain bone belongs to its asset");
            let world = solved
                .bone(bone_id)
                .expect("an IK chain bone belongs to its solved frame")
                .world_transform();
            push_bone_segment(
                &mut geometry,
                world.translation(),
                world,
                bone.length(),
                SegmentKind::IkChain,
            );
        }

        let target_id = constraint.target();
        let target = asset
            .bone(target_id)
            .expect("an IK target belongs to its asset");
        let target_position = solved
            .bone(target_id)
            .expect("an IK target belongs to its solved frame")
            .world_transform()
            .translation();
        push_marker(&mut geometry, target_position, MarkerKind::IkTarget);

        if let Some(control_id) = target.parent() {
            let control_position = solved
                .bone(control_id)
                .expect("an IK target parent belongs to its solved frame")
                .world_transform()
                .translation();
            push_marker(&mut geometry, control_position, MarkerKind::IkControl);
        }

        let chain_position = solved
            .bone(chain_root)
            .expect("an IK chain root belongs to its solved frame")
            .world_transform()
            .translation();
        push_segment(
            &mut geometry,
            chain_position,
            target_position,
            SegmentKind::IkLink,
        );

        let has_problem = solved.ik_status(constraint.id()).is_ok_and(|status| {
            status.is_degraded() || status.target_reach() == Some(IkTargetReach::BeyondReach)
        });
        if has_problem {
            push_marker(&mut geometry, target_position, MarkerKind::Problem);
        }
    }

    for constraint in asset.transform_constraints() {
        let source_position = solved
            .bone(constraint.source())
            .expect("a transform source belongs to its solved frame")
            .world_transform()
            .translation();
        let degraded = solved
            .transform_status(constraint.id())
            .is_ok_and(spinal::TransformConstraintSolveStatus::is_degraded);
        for bone_id in constraint.bones() {
            let position = solved
                .bone(bone_id)
                .expect("a constrained bone belongs to its solved frame")
                .world_transform()
                .translation();
            push_segment(
                &mut geometry,
                source_position,
                position,
                SegmentKind::TransformConstraint,
            );
            if degraded {
                push_marker(&mut geometry, position, MarkerKind::Problem);
            }
        }
    }

    geometry
}

fn push_bone_segment(
    geometry: &mut Geometry,
    origin: Vec2,
    world: spinal::WorldTransform,
    length: f32,
    kind: SegmentKind,
) {
    if length.abs() <= f32::EPSILON {
        return;
    }
    push_segment(
        geometry,
        origin,
        world.transform_point(Vec2::new(length, 0.0)),
        kind,
    );
}

fn push_segment(geometry: &mut Geometry, start: Vec2, end: Vec2, kind: SegmentKind) {
    if start.is_finite() && end.is_finite() {
        geometry.segments.push(Segment { start, end, kind });
    }
}

fn push_marker(geometry: &mut Geometry, position: Vec2, kind: MarkerKind) {
    if position.is_finite() {
        geometry.markers.push(Marker { position, kind });
    }
}

fn translated(setup: BoneTransform, offset: Vec2) -> BoneTransform {
    BoneTransform::new(
        setup.translation() + offset,
        setup.rotation(),
        setup.scale(),
        setup.shear(),
    )
    .expect("finite editor parameters preserve a finite setup transform")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::rig::{TEST_ATLAS, TEST_JSON, discover};

    fn fixture() -> (Skeleton, RigBinding) {
        let asset = spinal::load_json(TEST_JSON, TEST_ATLAS)
            .expect("fixture loads")
            .into_asset();
        let binding = discover(&asset).expect("fixture rig is discovered");
        (Skeleton::new(Arc::clone(&asset)), binding)
    }

    #[test]
    fn hidden_overlay_does_no_work_and_returns_no_geometry() {
        let (mut skeleton, binding) = fixture();
        let geometry = solve_geometry(
            false,
            &mut skeleton,
            &binding,
            WalkParameters::default(),
            0.0,
        );

        assert_eq!(geometry, Geometry::default());
    }

    #[test]
    fn solved_overlay_is_finite_and_marks_each_ik_control_and_target() {
        let (mut skeleton, binding) = fixture();
        let geometry = solve_geometry(
            true,
            &mut skeleton,
            &binding,
            WalkParameters::default(),
            0.0,
        );

        assert!(!geometry.segments.is_empty());
        assert!(
            geometry
                .segments
                .iter()
                .all(|segment| segment.start.is_finite() && segment.end.is_finite())
        );
        assert_eq!(
            geometry
                .markers
                .iter()
                .filter(|marker| marker.kind == MarkerKind::IkControl)
                .count(),
            4
        );
        assert_eq!(
            geometry
                .markers
                .iter()
                .filter(|marker| marker.kind == MarkerKind::IkTarget)
                .count(),
            4
        );
        assert_eq!(
            geometry
                .markers
                .iter()
                .filter(|marker| marker.kind == MarkerKind::BodyControl)
                .count(),
            1
        );
        assert_eq!(
            geometry
                .segments
                .iter()
                .filter(|segment| segment.kind == SegmentKind::IkLink)
                .count(),
            4
        );
    }

    #[test]
    fn control_markers_follow_the_walk_pose() {
        let (mut skeleton, binding) = fixture();
        let parameters = WalkParameters::default();
        let first = solve_geometry(true, &mut skeleton, &binding, parameters, 0.0);
        let second = solve_geometry(
            true,
            &mut skeleton,
            &binding,
            parameters,
            parameters.duration() * 0.5,
        );
        let controls = |geometry: &Geometry| {
            geometry
                .markers
                .iter()
                .filter(|marker| marker.kind == MarkerKind::IkControl)
                .map(|marker| marker.position)
                .collect::<Vec<_>>()
        };

        assert_ne!(controls(&first), controls(&second));
    }

    #[test]
    fn unreachable_ik_target_gets_the_same_red_problem_marker_as_save_validation() {
        let (mut skeleton, binding) = fixture();
        let parameters = WalkParameters {
            stride: 40.0,
            lift: 24.0,
            bob: 0.0,
        };
        let geometry = solve_geometry(true, &mut skeleton, &binding, parameters, 0.0);

        assert!(
            geometry
                .markers
                .iter()
                .any(|marker| marker.kind == MarkerKind::Problem)
        );
    }
}

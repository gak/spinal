use glam::Vec2;

use crate::{
    Angle, BendDirection, BoneTransform, Mix, Rgba, Shear, SkeletonAsset, TransformMix,
    asset::TransformConstraintPoseData, world::shortest_angle_delta,
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct BlendSwitches {
    pub(crate) attachment: f32,
    pub(crate) draw_order: f32,
    pub(crate) ik_bend: f32,
    pub(crate) scale_sign: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct AngleBranch {
    direction: i8,
    unwrapped_delta: f64,
}

#[derive(Clone, Copy, Debug, Default)]
struct BoneAngleBranches {
    rotation: AngleBranch,
    shear_x: AngleBranch,
    shear_y: AngleBranch,
}

#[derive(Debug)]
pub(crate) struct AngleBranches {
    bones: Box<[BoneAngleBranches]>,
}

impl AngleBranches {
    pub(crate) fn new(bone_count: usize) -> Self {
        Self {
            bones: vec![BoneAngleBranches::default(); bone_count].into_boxed_slice(),
        }
    }

    pub(crate) fn reset(&mut self) {
        self.bones.fill(BoneAngleBranches::default());
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct BonePose {
    pub(crate) local_transform: BoneTransform,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SlotPose {
    pub(crate) color: Rgba,
    pub(crate) attachment_placeholder: Option<u32>,
    pub(crate) attachment: Option<u32>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct IkConstraintPose {
    pub(crate) mix: Mix,
    pub(crate) bend_direction: BendDirection,
}

#[derive(Debug)]
pub(crate) struct PoseBuffers {
    pub(crate) bones: Box<[BonePose]>,
    pub(crate) slots: Box<[SlotPose]>,
    pub(crate) ik_constraints: Box<[IkConstraintPose]>,
    pub(crate) transform_constraints: Box<[TransformConstraintPoseData]>,
    pub(crate) draw_order: Box<[u32]>,
    pub(crate) active_animations: Vec<u32>,
}

impl PoseBuffers {
    pub(crate) fn new(asset: &SkeletonAsset) -> Self {
        Self {
            bones: asset
                .bones()
                .map(|bone| BonePose {
                    local_transform: bone.setup_transform(),
                })
                .collect(),
            slots: asset
                .slots()
                .map(|slot| SlotPose {
                    color: Rgba::from_rgba8(slot.color()),
                    attachment_placeholder: None,
                    attachment: None,
                })
                .collect(),
            ik_constraints: asset
                .ik_constraints()
                .map(|constraint| IkConstraintPose {
                    mix: constraint.mix(),
                    bend_direction: constraint.bend_direction(),
                })
                .collect(),
            transform_constraints: asset
                .transform_constraints()
                .map(|constraint| {
                    asset
                        .transform_constraint_data(constraint.ordinal())
                        .setup_pose
                })
                .collect(),
            draw_order: (0..asset.slots().len()).map(|index| index as u32).collect(),
            active_animations: Vec::with_capacity(asset.animations().len()),
        }
    }

    pub(crate) fn copy_from(&mut self, source: &Self) {
        self.bones.copy_from_slice(&source.bones);
        self.slots.copy_from_slice(&source.slots);
        self.ik_constraints.copy_from_slice(&source.ik_constraints);
        self.transform_constraints
            .copy_from_slice(&source.transform_constraints);
        self.draw_order.copy_from_slice(&source.draw_order);
        self.active_animations.clear();
        self.active_animations
            .extend_from_slice(&source.active_animations);
    }

    pub(crate) fn blend_from(
        &mut self,
        source: &Self,
        amount: f32,
        switches: BlendSwitches,
        branches: &mut AngleBranches,
    ) {
        for ((target, source), branch) in self
            .bones
            .iter_mut()
            .zip(&source.bones)
            .zip(&mut branches.bones)
        {
            let target_transform = target.local_transform;
            let source_transform = source.local_transform;
            let translation = Vec2::new(
                lerp_finite(
                    source_transform.translation().x,
                    target_transform.translation().x,
                    amount,
                ),
                lerp_finite(
                    source_transform.translation().y,
                    target_transform.translation().y,
                    amount,
                ),
            );
            let rotation = blend_angle(
                source_transform.rotation(),
                target_transform.rotation(),
                amount,
                &mut branch.rotation,
            );
            let scale = Vec2::new(
                blend_signed_scale(
                    source_transform.scale().x,
                    target_transform.scale().x,
                    amount,
                    switches.scale_sign,
                ),
                blend_signed_scale(
                    source_transform.scale().y,
                    target_transform.scale().y,
                    amount,
                    switches.scale_sign,
                ),
            );
            let shear = Shear::new(
                blend_angle(
                    source_transform.shear().x(),
                    target_transform.shear().x(),
                    amount,
                    &mut branch.shear_x,
                ),
                blend_angle(
                    source_transform.shear().y(),
                    target_transform.shear().y(),
                    amount,
                    &mut branch.shear_y,
                ),
            );
            target.local_transform = BoneTransform::new(translation, rotation, scale, shear)
                .expect("finite source and target poses produce a finite blend");
        }

        for (target, source) in self.slots.iter_mut().zip(&source.slots) {
            target.color = source.color.lerp(target.color, [amount; 4]);
            if amount < switches.attachment {
                target.attachment_placeholder = source.attachment_placeholder;
                target.attachment = source.attachment;
            }
        }

        for (target, source) in self.ik_constraints.iter_mut().zip(&source.ik_constraints) {
            target.mix = Mix::clamped(lerp_finite(source.mix.get(), target.mix.get(), amount))
                .expect("finite IK poses produce a finite blend");
            if amount < switches.ik_bend {
                target.bend_direction = source.bend_direction;
            }
        }

        for (target, source) in self
            .transform_constraints
            .iter_mut()
            .zip(&source.transform_constraints)
        {
            *target = TransformConstraintPoseData {
                mix_rotate: blend_transform_mix(source.mix_rotate, target.mix_rotate, amount),
                mix_x: blend_transform_mix(source.mix_x, target.mix_x, amount),
                mix_y: blend_transform_mix(source.mix_y, target.mix_y, amount),
                mix_scale_x: blend_transform_mix(source.mix_scale_x, target.mix_scale_x, amount),
                mix_scale_y: blend_transform_mix(source.mix_scale_y, target.mix_scale_y, amount),
                mix_shear_y: blend_transform_mix(source.mix_shear_y, target.mix_shear_y, amount),
            };
        }

        if amount < switches.draw_order {
            self.draw_order.copy_from_slice(&source.draw_order);
        }
        if amount < 1.0 {
            for animation in &source.active_animations {
                if !self.active_animations.contains(animation) {
                    self.active_animations.push(*animation);
                }
            }
        }
    }
}

fn blend_transform_mix(source: TransformMix, target: TransformMix, amount: f32) -> TransformMix {
    TransformMix::new(lerp_finite(source.get(), target.get(), amount))
        .expect("finite transform constraint poses produce a finite blend")
}

fn blend_angle(source: Angle, target: Angle, amount: f32, branch: &mut AngleBranch) -> Angle {
    let raw_delta = f64::from(target.as_radians()) - f64::from(source.as_radians());
    let shortest = f64::from(shortest_angle_delta(source, target));
    if branch.direction == 0 {
        branch.unwrapped_delta = shortest;
        if shortest != 0.0 {
            branch.direction = if shortest.is_sign_positive() { 1 } else { -1 };
        }
    } else {
        let turns = ((branch.unwrapped_delta - raw_delta) / core::f64::consts::TAU).round();
        let mut delta = raw_delta + turns * core::f64::consts::TAU;
        if branch.direction > 0 && delta < 0.0 {
            delta += (-delta / core::f64::consts::TAU).ceil() * core::f64::consts::TAU;
        } else if branch.direction < 0 && delta > 0.0 {
            delta -= (delta / core::f64::consts::TAU).ceil() * core::f64::consts::TAU;
        }
        branch.unwrapped_delta = delta;
    }
    let blended = f64::from(source.as_radians()) + branch.unwrapped_delta * f64::from(amount);
    Angle::from_radians(saturating_f32(blended)).expect("finite angles produce a finite blend")
}

fn blend_signed_scale(source: f32, target: f32, amount: f32, sign_switch: f32) -> f32 {
    let magnitude = lerp_finite(source.abs(), target.abs(), amount);
    let sign_source = if source == 0.0 {
        target.signum()
    } else {
        source.signum()
    };
    let sign_target = if target == 0.0 {
        sign_source
    } else {
        target.signum()
    };
    let sign = if amount < sign_switch {
        sign_source
    } else {
        sign_target
    };
    magnitude * sign
}

fn lerp_finite(source: f32, target: f32, amount: f32) -> f32 {
    let value = f64::from(source) + (f64::from(target) - f64::from(source)) * f64::from(amount);
    saturating_f32(value)
}

fn saturating_f32(value: f64) -> f32 {
    value.clamp(-(f32::MAX as f64), f32::MAX as f64) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn switches(at: f32) -> BlendSwitches {
        BlendSwitches {
            attachment: at,
            draw_order: at,
            ik_bend: at,
            scale_sign: at,
        }
    }

    fn transform(rotation: f32, scale: Vec2) -> BoneTransform {
        BoneTransform::new(
            Vec2::ZERO,
            Angle::from_degrees(rotation).expect("finite test angle"),
            scale,
            Shear::ZERO,
        )
        .expect("finite test transform")
    }

    #[test]
    fn angular_blending_uses_the_short_path_and_remembers_its_branch() {
        let asset = SkeletonAsset::test_fixture("cat");
        let mut source = PoseBuffers::new(&asset);
        let mut target = PoseBuffers::new(&asset);
        source.bones[0].local_transform = transform(179.0, Vec2::ONE);
        target.bones[0].local_transform = transform(-179.0, Vec2::ONE);
        let mut branches = AngleBranches::new(source.bones.len());

        target.blend_from(&source, 0.5, switches(0.0), &mut branches);
        let midpoint = target.bones[0]
            .local_transform
            .rotation()
            .as_degrees()
            .abs();
        assert!((midpoint - 180.0).abs() < 1.0e-3);

        let mut moving_target = PoseBuffers::new(&asset);
        moving_target.bones[0].local_transform = transform(170.0, Vec2::ONE);
        moving_target.blend_from(&source, 0.75, switches(0.0), &mut branches);
        assert!(
            moving_target.bones[0]
                .local_transform
                .rotation()
                .as_degrees()
                > 179.0
        );

        let mut exact_source_target = PoseBuffers::new(&asset);
        exact_source_target.bones[0].local_transform = transform(179.0, Vec2::ONE);
        exact_source_target.blend_from(&source, 0.75, switches(0.0), &mut branches);
        assert!(
            exact_source_target.bones[0]
                .local_transform
                .rotation()
                .as_degrees()
                > 400.0,
            "the remembered positive winding must not snap to zero at equality"
        );
    }

    #[test]
    fn scale_sign_and_discrete_values_switch_at_the_configured_threshold() {
        let asset = SkeletonAsset::test_fixture("cat");
        let mut source = PoseBuffers::new(&asset);
        let mut target = PoseBuffers::new(&asset);
        source.bones[0].local_transform = transform(0.0, Vec2::new(-2.0, 2.0));
        target.bones[0].local_transform = transform(0.0, Vec2::new(4.0, -4.0));
        source.slots[0].attachment = Some(3);
        target.slots[0].attachment = Some(7);
        source.draw_order.swap(0, 1);
        source.ik_constraints[0].bend_direction = BendDirection::Negative;
        target.ik_constraints[0].bend_direction = BendDirection::Positive;
        let mut branches = AngleBranches::new(source.bones.len());

        target.blend_from(&source, 0.25, switches(0.5), &mut branches);
        assert_eq!(
            target.bones[0].local_transform.scale(),
            Vec2::new(-2.5, 2.5)
        );
        assert_eq!(target.slots[0].attachment, Some(3));
        assert_eq!(target.draw_order, source.draw_order);
        assert_eq!(
            target.ik_constraints[0].bend_direction,
            BendDirection::Negative
        );

        let mut at_threshold = PoseBuffers::new(&asset);
        at_threshold.bones[0].local_transform = transform(0.0, Vec2::new(4.0, -4.0));
        at_threshold.slots[0].attachment = Some(7);
        let mut threshold_branches = AngleBranches::new(source.bones.len());
        at_threshold.blend_from(&source, 0.5, switches(0.5), &mut threshold_branches);
        assert_eq!(
            at_threshold.bones[0].local_transform.scale(),
            Vec2::new(3.0, -3.0)
        );
        assert_eq!(at_threshold.slots[0].attachment, Some(7));
    }

    #[test]
    fn finite_extreme_transforms_blend_without_overflow_or_panic() {
        let asset = SkeletonAsset::test_fixture("cat");
        let mut source = PoseBuffers::new(&asset);
        let mut target = PoseBuffers::new(&asset);
        let negative = Angle::from_radians(-f32::MAX).expect("finite angle");
        let positive = Angle::from_radians(f32::MAX).expect("finite angle");
        source.bones[0].local_transform = BoneTransform::new(
            Vec2::splat(-f32::MAX),
            negative,
            Vec2::ONE,
            Shear::new(negative, negative),
        )
        .expect("finite source transform");
        target.bones[0].local_transform = BoneTransform::new(
            Vec2::splat(f32::MAX),
            positive,
            Vec2::ONE,
            Shear::new(positive, positive),
        )
        .expect("finite target transform");
        let mut branches = AngleBranches::new(source.bones.len());

        target.blend_from(&source, 0.5, switches(0.0), &mut branches);

        assert_eq!(target.bones[0].local_transform.translation(), Vec2::ZERO);
        assert!(
            target.bones[0]
                .local_transform
                .rotation()
                .as_radians()
                .is_finite()
        );
        assert!(
            target.bones[0]
                .local_transform
                .shear()
                .as_radians()
                .is_finite()
        );
    }
}

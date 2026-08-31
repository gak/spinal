use glam::Vec2;

use crate::{
    Angle, BendDirection, BoneTransform, Mix, Rgba, RotationPath, Shear, SkeletonAsset,
    TransformMix, asset::TransformConstraintPoseData, world::shortest_angle_delta,
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

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WeightedContribution<T> {
    pub(crate) value: T,
    pub(crate) influence: f32,
}

impl<T> WeightedContribution<T> {
    pub(crate) const fn full(value: T) -> Self {
        Self {
            value,
            influence: 1.0,
        }
    }
}

/// Independent per-axis contributions for a two-component bone property.
///
/// Splitting each axis lets a single-axis timeline (for example
/// `translatex`) carry its own influence so mixing it leaves the other axis
/// fully sourced from the lower track, rather than dragging it toward a
/// placeholder value at the track's blend weight.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct AxisContribution<T> {
    pub(crate) x: Option<WeightedContribution<T>>,
    pub(crate) y: Option<WeightedContribution<T>>,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct BoneContribution {
    pub(crate) translation: AxisContribution<f32>,
    pub(crate) rotation: Option<WeightedContribution<Angle>>,
    pub(crate) scale_magnitude: AxisContribution<f32>,
    pub(crate) shear: AxisContribution<Angle>,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SlotContribution {
    pub(crate) color: Option<WeightedContribution<Rgba>>,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct IkContribution {
    pub(crate) mix: Option<WeightedContribution<Mix>>,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TransformContribution {
    pub(crate) mix_rotate: Option<WeightedContribution<TransformMix>>,
    pub(crate) mix_x: Option<WeightedContribution<TransformMix>>,
    pub(crate) mix_y: Option<WeightedContribution<TransformMix>>,
    pub(crate) mix_scale_x: Option<WeightedContribution<TransformMix>>,
    pub(crate) mix_scale_y: Option<WeightedContribution<TransformMix>>,
    pub(crate) mix_shear_y: Option<WeightedContribution<TransformMix>>,
}

#[derive(Debug)]
pub(crate) struct ContributionPose {
    pub(crate) bones: Box<[BoneContribution]>,
    pub(crate) slots: Box<[SlotContribution]>,
    pub(crate) ik_constraints: Box<[IkContribution]>,
    pub(crate) transform_constraints: Box<[TransformContribution]>,
    pub(crate) active_animations: Vec<u32>,
}

impl ContributionPose {
    pub(crate) fn new(asset: &SkeletonAsset) -> Self {
        Self {
            bones: vec![BoneContribution::default(); asset.bones().len()].into_boxed_slice(),
            slots: vec![SlotContribution::default(); asset.slots().len()].into_boxed_slice(),
            ik_constraints: vec![IkContribution::default(); asset.ik_constraints().len()]
                .into_boxed_slice(),
            transform_constraints: vec![
                TransformContribution::default();
                asset.transform_constraints().len()
            ]
            .into_boxed_slice(),
            active_animations: Vec::with_capacity(asset.animations().len()),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.bones.fill(BoneContribution::default());
        self.slots.fill(SlotContribution::default());
        self.ik_constraints.fill(IkContribution::default());
        self.transform_constraints
            .fill(TransformContribution::default());
        self.active_animations.clear();
    }

    pub(crate) fn copy_from(&mut self, source: &Self) {
        self.bones.copy_from_slice(&source.bones);
        self.slots.copy_from_slice(&source.slots);
        self.ik_constraints.copy_from_slice(&source.ik_constraints);
        self.transform_constraints
            .copy_from_slice(&source.transform_constraints);
        self.active_animations.clear();
        self.active_animations
            .extend_from_slice(&source.active_animations);
    }

    pub(crate) fn mix_from(
        &mut self,
        source: &Self,
        target: &Self,
        amount: f32,
        rotation_path: RotationPath,
        branches: &mut AngleBranches,
    ) {
        for (((output, source), target), branch) in self
            .bones
            .iter_mut()
            .zip(&source.bones)
            .zip(&target.bones)
            .zip(&mut branches.bones)
        {
            output.translation = AxisContribution {
                x: mix_scalar_contribution(source.translation.x, target.translation.x, amount),
                y: mix_scalar_contribution(source.translation.y, target.translation.y, amount),
            };
            output.rotation = mix_angle_contribution(
                source.rotation,
                target.rotation,
                amount,
                rotation_path,
                &mut branch.rotation,
            );
            output.scale_magnitude = AxisContribution {
                x: mix_scalar_contribution(
                    source.scale_magnitude.x,
                    target.scale_magnitude.x,
                    amount,
                ),
                y: mix_scalar_contribution(
                    source.scale_magnitude.y,
                    target.scale_magnitude.y,
                    amount,
                ),
            };
            output.shear = AxisContribution {
                x: mix_angle_contribution(
                    source.shear.x,
                    target.shear.x,
                    amount,
                    rotation_path,
                    &mut branch.shear_x,
                ),
                y: mix_angle_contribution(
                    source.shear.y,
                    target.shear.y,
                    amount,
                    rotation_path,
                    &mut branch.shear_y,
                ),
            };
        }
        for ((output, source), target) in
            self.slots.iter_mut().zip(&source.slots).zip(&target.slots)
        {
            output.color = mix_color_contribution(source.color, target.color, amount);
        }
        for ((output, source), target) in self
            .ik_constraints
            .iter_mut()
            .zip(&source.ik_constraints)
            .zip(&target.ik_constraints)
        {
            output.mix = mix_normalized_mix_contribution(source.mix, target.mix, amount);
        }
        for ((output, source), target) in self
            .transform_constraints
            .iter_mut()
            .zip(&source.transform_constraints)
            .zip(&target.transform_constraints)
        {
            output.mix_rotate =
                mix_transform_contribution(source.mix_rotate, target.mix_rotate, amount);
            output.mix_x = mix_transform_contribution(source.mix_x, target.mix_x, amount);
            output.mix_y = mix_transform_contribution(source.mix_y, target.mix_y, amount);
            output.mix_scale_x =
                mix_transform_contribution(source.mix_scale_x, target.mix_scale_x, amount);
            output.mix_scale_y =
                mix_transform_contribution(source.mix_scale_y, target.mix_scale_y, amount);
            output.mix_shear_y =
                mix_transform_contribution(source.mix_shear_y, target.mix_shear_y, amount);
        }

        self.active_animations.clear();
        if amount < 1.0 {
            self.active_animations
                .extend_from_slice(&source.active_animations);
        }
        for animation in &target.active_animations {
            if !self.active_animations.contains(animation) {
                self.active_animations.push(*animation);
            }
        }
    }

    pub(crate) fn apply_to(
        &self,
        target: &mut PoseBuffers,
        weight: Mix,
        branches: &mut AngleBranches,
    ) {
        let amount = weight.get();
        if amount == 0.0 {
            branches.reset();
            return;
        }

        for ((target, contribution), branch) in target
            .bones
            .iter_mut()
            .zip(&self.bones)
            .zip(&mut branches.bones)
        {
            let current = target.local_transform;
            if contribution.rotation.is_none() {
                branch.rotation = AngleBranch::default();
            }
            if contribution.shear.x.is_none() {
                branch.shear_x = AngleBranch::default();
            }
            if contribution.shear.y.is_none() {
                branch.shear_y = AngleBranch::default();
            }
            let translation = Vec2::new(
                contribution
                    .translation
                    .x
                    .map_or(current.translation().x, |contribution| {
                        lerp_finite(
                            current.translation().x,
                            contribution.value,
                            amount * contribution.influence,
                        )
                    }),
                contribution
                    .translation
                    .y
                    .map_or(current.translation().y, |contribution| {
                        lerp_finite(
                            current.translation().y,
                            contribution.value,
                            amount * contribution.influence,
                        )
                    }),
            );
            let rotation = contribution.rotation.map_or(current.rotation(), |value| {
                blend_angle(
                    current.rotation(),
                    value.value,
                    amount * value.influence,
                    RotationPath::Shortest,
                    &mut branch.rotation,
                )
            });
            let scale = Vec2::new(
                contribution
                    .scale_magnitude
                    .x
                    .map_or(current.scale().x, |contribution| {
                        blend_scale_magnitude(
                            current.scale().x,
                            contribution.value,
                            amount * contribution.influence,
                        )
                    }),
                contribution
                    .scale_magnitude
                    .y
                    .map_or(current.scale().y, |contribution| {
                        blend_scale_magnitude(
                            current.scale().y,
                            contribution.value,
                            amount * contribution.influence,
                        )
                    }),
            );
            let shear = Shear::new(
                contribution
                    .shear
                    .x
                    .map_or(current.shear().x(), |contribution| {
                        blend_angle(
                            current.shear().x(),
                            contribution.value,
                            amount * contribution.influence,
                            RotationPath::Shortest,
                            &mut branch.shear_x,
                        )
                    }),
                contribution
                    .shear
                    .y
                    .map_or(current.shear().y(), |contribution| {
                        blend_angle(
                            current.shear().y(),
                            contribution.value,
                            amount * contribution.influence,
                            RotationPath::Shortest,
                            &mut branch.shear_y,
                        )
                    }),
            );
            target.local_transform = BoneTransform::new(translation, rotation, scale, shear)
                .expect("finite lower poses and contributions produce a finite transform");
        }

        for (target, contribution) in target.slots.iter_mut().zip(&self.slots) {
            if let Some(contribution) = contribution.color {
                target.color = target
                    .color
                    .lerp(contribution.value, [amount * contribution.influence; 4]);
            }
        }

        for (target, contribution) in target.ik_constraints.iter_mut().zip(&self.ik_constraints) {
            if let Some(contribution) = contribution.mix {
                target.mix = Mix::clamped(lerp_finite(
                    target.mix.get(),
                    contribution.value.get(),
                    amount * contribution.influence,
                ))
                .expect("finite IK contributions produce a finite normalized mix");
            }
        }

        for (target, contribution) in target
            .transform_constraints
            .iter_mut()
            .zip(&self.transform_constraints)
        {
            target.mix_rotate =
                apply_transform_contribution(target.mix_rotate, contribution.mix_rotate, amount);
            target.mix_x = apply_transform_contribution(target.mix_x, contribution.mix_x, amount);
            target.mix_y = apply_transform_contribution(target.mix_y, contribution.mix_y, amount);
            target.mix_scale_x =
                apply_transform_contribution(target.mix_scale_x, contribution.mix_scale_x, amount);
            target.mix_scale_y =
                apply_transform_contribution(target.mix_scale_y, contribution.mix_scale_y, amount);
            target.mix_shear_y =
                apply_transform_contribution(target.mix_shear_y, contribution.mix_shear_y, amount);
        }

        for animation in &self.active_animations {
            if !target.active_animations.contains(animation) {
                target.active_animations.push(*animation);
            }
        }
    }
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
        rotation_path: RotationPath,
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
                rotation_path,
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
                    rotation_path,
                    &mut branch.shear_x,
                ),
                blend_angle(
                    source_transform.shear().y(),
                    target_transform.shear().y(),
                    amount,
                    rotation_path,
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

fn apply_transform_contribution(
    lower: TransformMix,
    contribution: Option<WeightedContribution<TransformMix>>,
    amount: f32,
) -> TransformMix {
    contribution.map_or(lower, |contribution| {
        TransformMix::new(lerp_finite(
            lower.get(),
            contribution.value.get(),
            amount * contribution.influence,
        ))
        .expect("finite transform contributions produce a finite mix")
    })
}

fn contribution_mix_factors<T>(
    source: Option<WeightedContribution<T>>,
    target: Option<WeightedContribution<T>>,
    amount: f32,
) -> Option<(f32, f32)>
where
    T: Copy,
{
    let source_weight = (1.0 - amount) * source.map_or(0.0, |contribution| contribution.influence);
    let target_weight = amount * target.map_or(0.0, |contribution| contribution.influence);
    let influence = source_weight + target_weight;
    if influence == 0.0 {
        None
    } else {
        Some((influence, target_weight / influence))
    }
}

fn mix_scalar_contribution(
    source: Option<WeightedContribution<f32>>,
    target: Option<WeightedContribution<f32>>,
    amount: f32,
) -> Option<WeightedContribution<f32>> {
    let (influence, target_share) = contribution_mix_factors(source, target, amount)?;
    let source_value = source.or(target)?.value;
    let target_value = target.or(source)?.value;
    Some(WeightedContribution {
        value: lerp_finite(source_value, target_value, target_share),
        influence,
    })
}

fn mix_angle_contribution(
    source: Option<WeightedContribution<Angle>>,
    target: Option<WeightedContribution<Angle>>,
    amount: f32,
    rotation_path: RotationPath,
    branch: &mut AngleBranch,
) -> Option<WeightedContribution<Angle>> {
    let (influence, target_share) = contribution_mix_factors(source, target, amount)?;
    let source_value = source.or(target)?.value;
    let target_value = target.or(source)?.value;
    Some(WeightedContribution {
        value: blend_angle(
            source_value,
            target_value,
            target_share,
            rotation_path,
            branch,
        ),
        influence,
    })
}

fn mix_color_contribution(
    source: Option<WeightedContribution<Rgba>>,
    target: Option<WeightedContribution<Rgba>>,
    amount: f32,
) -> Option<WeightedContribution<Rgba>> {
    let (influence, target_share) = contribution_mix_factors(source, target, amount)?;
    let source_value = source.or(target)?.value;
    let target_value = target.or(source)?.value;
    Some(WeightedContribution {
        value: source_value.lerp(target_value, [target_share; 4]),
        influence,
    })
}

fn mix_normalized_mix_contribution(
    source: Option<WeightedContribution<Mix>>,
    target: Option<WeightedContribution<Mix>>,
    amount: f32,
) -> Option<WeightedContribution<Mix>> {
    let (influence, target_share) = contribution_mix_factors(source, target, amount)?;
    let source_value = source.or(target)?.value;
    let target_value = target.or(source)?.value;
    Some(WeightedContribution {
        value: Mix::clamped(lerp_finite(
            source_value.get(),
            target_value.get(),
            target_share,
        ))
        .expect("finite normalized contributions produce a normalized value"),
        influence,
    })
}

fn mix_transform_contribution(
    source: Option<WeightedContribution<TransformMix>>,
    target: Option<WeightedContribution<TransformMix>>,
    amount: f32,
) -> Option<WeightedContribution<TransformMix>> {
    let (influence, target_share) = contribution_mix_factors(source, target, amount)?;
    let source_value = source.or(target)?.value;
    let target_value = target.or(source)?.value;
    Some(WeightedContribution {
        value: TransformMix::new(lerp_finite(
            source_value.get(),
            target_value.get(),
            target_share,
        ))
        .expect("finite transform contributions produce a finite value"),
        influence,
    })
}

fn blend_scale_magnitude(lower: f32, target_magnitude: f32, amount: f32) -> f32 {
    let magnitude = lerp_finite(lower.abs(), target_magnitude.abs(), amount);
    if lower.is_sign_negative() {
        -magnitude
    } else {
        magnitude
    }
}

fn blend_angle(
    source: Angle,
    target: Angle,
    amount: f32,
    rotation_path: RotationPath,
    branch: &mut AngleBranch,
) -> Angle {
    let raw_delta = f64::from(target.as_radians()) - f64::from(source.as_radians());
    let shortest = f64::from(shortest_angle_delta(source, target));
    if rotation_path == RotationPath::Shortest {
        *branch = AngleBranch::default();
        let blended = f64::from(source.as_radians()) + shortest * f64::from(amount);
        return Angle::from_radians(saturating_f32(blended))
            .expect("finite angles produce a finite blend");
    }
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
    fn preserved_rotation_direction_uses_the_initial_short_path_then_remembers_it() {
        let asset = SkeletonAsset::test_fixture("cat");
        let mut source = PoseBuffers::new(&asset);
        let mut target = PoseBuffers::new(&asset);
        source.bones[0].local_transform = transform(179.0, Vec2::ONE);
        target.bones[0].local_transform = transform(-179.0, Vec2::ONE);
        let mut branches = AngleBranches::new(source.bones.len());

        target.blend_from(
            &source,
            0.5,
            switches(0.0),
            RotationPath::PreserveDirection,
            &mut branches,
        );
        let midpoint = target.bones[0]
            .local_transform
            .rotation()
            .as_degrees()
            .abs();
        assert!((midpoint - 180.0).abs() < 1.0e-3);

        let mut moving_target = PoseBuffers::new(&asset);
        moving_target.bones[0].local_transform = transform(170.0, Vec2::ONE);
        moving_target.blend_from(
            &source,
            0.75,
            switches(0.0),
            RotationPath::PreserveDirection,
            &mut branches,
        );
        assert!(
            moving_target.bones[0]
                .local_transform
                .rotation()
                .as_degrees()
                > 179.0
        );

        let mut exact_source_target = PoseBuffers::new(&asset);
        exact_source_target.bones[0].local_transform = transform(179.0, Vec2::ONE);
        exact_source_target.blend_from(
            &source,
            0.75,
            switches(0.0),
            RotationPath::PreserveDirection,
            &mut branches,
        );
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
    fn shortest_rotation_path_reconsiders_a_moving_target_without_a_full_turn() {
        let asset = SkeletonAsset::test_fixture("cat");
        let mut source = PoseBuffers::new(&asset);
        source.bones[0].local_transform = transform(0.0, Vec2::ONE);
        let mut branches = AngleBranches::new(source.bones.len());

        let mut positive_target = PoseBuffers::new(&asset);
        positive_target.bones[0].local_transform = transform(10.0, Vec2::ONE);
        positive_target.blend_from(
            &source,
            0.5,
            switches(0.0),
            RotationPath::Shortest,
            &mut branches,
        );
        assert!(
            (positive_target.bones[0]
                .local_transform
                .rotation()
                .as_degrees()
                - 5.0)
                .abs()
                < 1.0e-4
        );

        let mut negative_target = PoseBuffers::new(&asset);
        negative_target.bones[0].local_transform = transform(-10.0, Vec2::ONE);
        negative_target.blend_from(
            &source,
            0.5,
            switches(0.0),
            RotationPath::Shortest,
            &mut branches,
        );
        assert!(
            (negative_target.bones[0]
                .local_transform
                .rotation()
                .as_degrees()
                + 5.0)
                .abs()
                < 1.0e-4
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

        target.blend_from(
            &source,
            0.25,
            switches(0.5),
            RotationPath::Shortest,
            &mut branches,
        );
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
        at_threshold.blend_from(
            &source,
            0.5,
            switches(0.5),
            RotationPath::Shortest,
            &mut threshold_branches,
        );
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

        target.blend_from(
            &source,
            0.5,
            switches(0.0),
            RotationPath::Shortest,
            &mut branches,
        );

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

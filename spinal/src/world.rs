use glam::Vec2;
use thiserror::Error;

use crate::{Angle, BendDirection, BoneTransform};

const SAFE_INVERSE_EPSILON: f64 = 32.0 * f32::EPSILON as f64;
const GEOMETRY_EPSILON: f64 = 32.0 * f32::EPSILON as f64;

/// A finite affine transform from bone-local coordinates to skeleton space.
///
/// The two axes are the columns of the linear transform. Skeleton space is
/// the coordinate system containing the root bone, not an engine's world
/// coordinate system.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldTransform {
    translation: Vec2,
    x_axis: Vec2,
    y_axis: Vec2,
}

impl WorldTransform {
    /// The identity transform.
    pub const IDENTITY: Self = Self {
        translation: Vec2::ZERO,
        x_axis: Vec2::X,
        y_axis: Vec2::Y,
    };

    /// Constructs a transform after checking every component is finite.
    pub fn new(
        translation: Vec2,
        x_axis: Vec2,
        y_axis: Vec2,
    ) -> Result<Self, InvalidWorldTransform> {
        if !translation.is_finite() {
            return Err(InvalidWorldTransform::new("translation"));
        }
        if !x_axis.is_finite() {
            return Err(InvalidWorldTransform::new("x axis"));
        }
        if !y_axis.is_finite() {
            return Err(InvalidWorldTransform::new("y axis"));
        }
        Ok(Self {
            translation,
            x_axis,
            y_axis,
        })
    }

    /// Returns the skeleton-space origin.
    #[must_use]
    pub const fn translation(self) -> Vec2 {
        self.translation
    }

    /// Returns the transformed local X axis.
    #[must_use]
    pub const fn x_axis(self) -> Vec2 {
        self.x_axis
    }

    /// Returns the transformed local Y axis.
    #[must_use]
    pub const fn y_axis(self) -> Vec2 {
        self.y_axis
    }

    /// Transforms a finite local point into skeleton space.
    ///
    /// Results outside `f32` range are saturated. Non-finite input returns
    /// zero rather than contaminating the evaluated pose.
    #[must_use]
    pub fn transform_point(self, point: Vec2) -> Vec2 {
        if !point.is_finite() {
            return Vec2::ZERO;
        }
        let (x, y) = self.transform_point_f64(f64::from(point.x), f64::from(point.y));
        Vec2::new(saturating_f32(x), saturating_f32(y))
    }

    /// Transforms a finite local vector into skeleton space.
    ///
    /// Results outside `f32` range are saturated. Non-finite input returns
    /// zero rather than contaminating the evaluated pose.
    #[must_use]
    pub fn transform_vector(self, vector: Vec2) -> Vec2 {
        if !vector.is_finite() {
            return Vec2::ZERO;
        }
        let (x, y) = self.transform_vector_f64(f64::from(vector.x), f64::from(vector.y));
        Vec2::new(saturating_f32(x), saturating_f32(y))
    }

    /// Returns the signed area scale of the linear transform.
    ///
    /// A negative value identifies a reflection. Zero identifies a singular
    /// transform.
    #[must_use]
    pub fn determinant(self) -> f64 {
        f64::from(self.x_axis.x) * f64::from(self.y_axis.y)
            - f64::from(self.y_axis.x) * f64::from(self.x_axis.y)
    }

    /// Transforms a skeleton-space point back to local coordinates.
    ///
    /// Returns `None` for non-finite input, a singular or numerically unsafe
    /// transform, or a result outside finite `f32` range.
    #[must_use]
    pub fn try_inverse_point(self, point: Vec2) -> Option<Vec2> {
        if !point.is_finite() {
            return None;
        }
        let (x, y) = self.try_inverse_point_f64(f64::from(point.x), f64::from(point.y))?;
        checked_vec2(x, y)
    }

    fn transform_point_f64(self, x: f64, y: f64) -> (f64, f64) {
        let (x, y) = self.transform_vector_f64(x, y);
        (
            x + f64::from(self.translation.x),
            y + f64::from(self.translation.y),
        )
    }

    fn transform_vector_f64(self, x: f64, y: f64) -> (f64, f64) {
        (
            f64::from(self.x_axis.x) * x + f64::from(self.y_axis.x) * y,
            f64::from(self.x_axis.y) * x + f64::from(self.y_axis.y) * y,
        )
    }

    fn try_inverse_point_f64(self, x: f64, y: f64) -> Option<(f64, f64)> {
        self.try_inverse_vector_f64(
            x - f64::from(self.translation.x),
            y - f64::from(self.translation.y),
        )
    }

    fn try_inverse_vector_f64(self, x: f64, y: f64) -> Option<(f64, f64)> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let a = f64::from(self.x_axis.x);
        let b = f64::from(self.y_axis.x);
        let c = f64::from(self.x_axis.y);
        let d = f64::from(self.y_axis.y);
        let determinant = a * d - b * c;
        let x_length = a.hypot(c);
        let y_length = b.hypot(d);
        let area_scale = x_length * y_length;
        if area_scale == 0.0
            || !area_scale.is_finite()
            || determinant.abs() <= SAFE_INVERSE_EPSILON * area_scale
        {
            return None;
        }
        let local_x = (d * x - b * y) / determinant;
        let local_y = (-c * x + a * y) / determinant;
        (local_x.is_finite() && local_y.is_finite()).then_some((local_x, local_y))
    }
}

impl Default for WorldTransform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// Returned when a world transform contains NaN or infinity.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("world transform {component} must be finite")]
pub struct InvalidWorldTransform {
    component: &'static str,
}

impl InvalidWorldTransform {
    const fn new(component: &'static str) -> Self {
        Self { component }
    }

    /// Returns the rejected component.
    #[must_use]
    pub const fn component(self) -> &'static str {
        self.component
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IkReach {
    Reached,
    Closest,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TwoBoneIkSolution {
    pub(crate) parent_rotation: Angle,
    pub(crate) child_rotation: Angle,
    pub(crate) child_translation_y: f32,
    pub(crate) child_y_was_zeroed: bool,
    pub(crate) reach: IkReach,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum OneBoneIkSolution {
    Rotation(Angle),
    PreservedCoincident,
}

pub(crate) fn normal_local_to_world(
    parent: Option<WorldTransform>,
    local: BoneTransform,
) -> WorldTransform {
    let rotation = f64::from(local.rotation().as_radians());
    let shear = local.shear();
    let x_angle = rotation + f64::from(shear.x().as_radians());
    let y_angle = rotation + std::f64::consts::FRAC_PI_2 + f64::from(shear.y().as_radians());
    let (x_sine, x_cosine) = x_angle.sin_cos();
    let (y_sine, y_cosine) = y_angle.sin_cos();
    let scale = local.scale();
    let local_x = (f64::from(scale.x) * x_cosine, f64::from(scale.x) * x_sine);
    let local_y = (f64::from(scale.y) * y_cosine, f64::from(scale.y) * y_sine);
    let translation = local.translation();

    let (world_translation, world_x, world_y) = if let Some(parent) = parent {
        (
            parent.transform_point_f64(f64::from(translation.x), f64::from(translation.y)),
            parent.transform_vector_f64(local_x.0, local_x.1),
            parent.transform_vector_f64(local_y.0, local_y.1),
        )
    } else {
        (
            (f64::from(translation.x), f64::from(translation.y)),
            local_x,
            local_y,
        )
    };

    WorldTransform {
        translation: saturated_vec2(world_translation.0, world_translation.1),
        x_axis: saturated_vec2(world_x.0, world_x.1),
        y_axis: saturated_vec2(world_y.0, world_y.1),
    }
}

pub(crate) fn shortest_angle_delta(from: Angle, to: Angle) -> f32 {
    saturating_f32(shortest_angle_delta_f64(
        f64::from(from.as_radians()),
        f64::from(to.as_radians()),
    ))
}

pub(crate) fn solve_one_bone_ik(
    parent: Option<WorldTransform>,
    bone_local: BoneTransform,
    target_world: Vec2,
) -> Option<OneBoneIkSolution> {
    if !target_world.is_finite() || bone_local.scale().x == 0.0 {
        return None;
    }

    let (target_x, target_y) = if let Some(parent) = parent {
        parent.try_inverse_point_f64(f64::from(target_world.x), f64::from(target_world.y))?
    } else {
        (f64::from(target_world.x), f64::from(target_world.y))
    };
    let translation = bone_local.translation();
    let target_x = target_x - f64::from(translation.x);
    let target_y = target_y - f64::from(translation.y);
    let target_distance = target_x.hypot(target_y);
    if target_distance <= GEOMETRY_EPSILON * target_distance.max(1.0) {
        return Some(OneBoneIkSolution::PreservedCoincident);
    }

    let signed_scale_correction = if bone_local.scale().x < 0.0 {
        std::f64::consts::PI
    } else {
        0.0
    };
    let target_rotation = target_y.atan2(target_x)
        - f64::from(bone_local.shear().x().as_radians())
        - signed_scale_correction;
    nearest_angle(bone_local.rotation(), target_rotation).map(OneBoneIkSolution::Rotation)
}

pub(crate) fn solve_two_bone_ik(
    grandparent: Option<WorldTransform>,
    parent_local: BoneTransform,
    child_local: BoneTransform,
    child_length: f32,
    target_world: Vec2,
    bend_direction: BendDirection,
) -> Option<TwoBoneIkSolution> {
    if !target_world.is_finite() || !child_length.is_finite() || child_length <= 0.0 {
        return None;
    }

    let (target_x, target_y) = if let Some(grandparent) = grandparent {
        grandparent.try_inverse_point_f64(f64::from(target_world.x), f64::from(target_world.y))?
    } else {
        (f64::from(target_world.x), f64::from(target_world.y))
    };
    let parent_translation = parent_local.translation();
    let target_x = target_x - f64::from(parent_translation.x);
    let target_y = target_y - f64::from(parent_translation.y);
    let target_distance = target_x.hypot(target_y);

    let parent_scale = parent_local.scale();
    let scale_x = f64::from(parent_scale.x);
    let scale_y = f64::from(parent_scale.y);
    let child_scale_x = f64::from(child_local.scale().x);
    let child_reach = f64::from(child_length) * child_scale_x;
    if scale_x == 0.0 || scale_y == 0.0 || child_reach == 0.0 {
        return None;
    }

    let child_translation = child_local.translation();
    let child_x = f64::from(child_translation.x);
    let original_child_y = f64::from(child_translation.y);
    let scale_size = scale_x.abs().max(scale_y.abs());
    let uniform_scale = (scale_x.abs() - scale_y.abs()).abs() <= GEOMETRY_EPSILON * scale_size;
    let child_y = if uniform_scale { original_child_y } else { 0.0 };
    if child_x.hypot(child_y) <= GEOMETRY_EPSILON {
        return None;
    }

    let mut theta_values = [None; 8];
    let mut theta_count = 0;
    if uniform_scale {
        let first_length = child_x.hypot(child_y);
        let scaled_target_distance = target_distance / scale_x.abs();
        let denominator = 2.0 * child_reach * first_length;
        if denominator == 0.0 || !scaled_target_distance.is_finite() {
            return None;
        }
        let cosine = ((scaled_target_distance * scaled_target_distance)
            - first_length * first_length
            - child_reach * child_reach)
            / denominator;
        let joint_angle = cosine.clamp(-1.0, 1.0).acos();
        let first_angle = child_y.atan2(child_x);
        push_unique_angle(
            &mut theta_values,
            &mut theta_count,
            first_angle + joint_angle,
        );
        push_unique_angle(
            &mut theta_values,
            &mut theta_count,
            first_angle - joint_angle,
        );
    } else {
        let scale_x_squared = scale_x * scale_x;
        let scale_y_squared = scale_y * scale_y;
        let reach_squared = child_reach * child_reach;
        let quadratic = reach_squared * (scale_x_squared - scale_y_squared);
        let linear = 2.0 * scale_x_squared * child_x * child_reach;
        let constant = scale_x_squared * child_x * child_x + scale_y_squared * reach_squared
            - target_distance * target_distance;
        let coefficient_size = quadratic
            .abs()
            .max(linear.abs())
            .max(constant.abs())
            .max(1.0);
        let quadratic_is_zero = quadratic.abs() <= GEOMETRY_EPSILON * coefficient_size;
        let mut cosine_values = [0.0; 3];
        let mut cosine_count = 0;

        if quadratic_is_zero {
            if linear.abs() > GEOMETRY_EPSILON * coefficient_size {
                push_cosine(
                    &mut cosine_values,
                    &mut cosine_count,
                    -constant / linear,
                    true,
                );
            }
        } else {
            let discriminant = linear * linear - 4.0 * quadratic * constant;
            let discriminant_scale = (linear * linear)
                .abs()
                .max((4.0 * quadratic * constant).abs())
                .max(1.0);
            if discriminant >= -GEOMETRY_EPSILON * discriminant_scale {
                let square_root = discriminant.max(0.0).sqrt();
                push_cosine(
                    &mut cosine_values,
                    &mut cosine_count,
                    (-linear + square_root) / (2.0 * quadratic),
                    false,
                );
                push_cosine(
                    &mut cosine_values,
                    &mut cosine_count,
                    (-linear - square_root) / (2.0 * quadratic),
                    false,
                );
            }
        }

        if cosine_count == 0 {
            push_cosine(&mut cosine_values, &mut cosine_count, -1.0, true);
            push_cosine(&mut cosine_values, &mut cosine_count, 1.0, true);
            if !quadratic_is_zero {
                push_cosine(
                    &mut cosine_values,
                    &mut cosine_count,
                    -linear / (2.0 * quadratic),
                    true,
                );
            } else if linear != 0.0 {
                push_cosine(
                    &mut cosine_values,
                    &mut cosine_count,
                    -constant / linear,
                    true,
                );
            }
        }

        for cosine in cosine_values.into_iter().take(cosine_count) {
            let sine = (1.0 - cosine * cosine).max(0.0).sqrt();
            push_unique_angle(&mut theta_values, &mut theta_count, sine.atan2(cosine));
            push_unique_angle(&mut theta_values, &mut theta_count, (-sine).atan2(cosine));
        }
    }

    let child_shear = f64::from(child_local.shear().x().as_radians());
    let current_parent_rotation = f64::from(parent_local.rotation().as_radians());
    let current_child_rotation = f64::from(child_local.rotation().as_radians());
    let positive_bend = !matches!(bend_direction, BendDirection::Negative);
    let mut best = None;

    for theta in theta_values.into_iter().take(theta_count).flatten() {
        let cosine = theta.cos();
        let sine = theta.sin();
        let tip_x = scale_x * (child_x + child_reach * cosine);
        let tip_y = scale_y * (child_y + child_reach * sine);
        let tip_distance = tip_x.hypot(tip_y);
        if !tip_distance.is_finite() {
            continue;
        }
        let parent_target =
            if target_distance <= GEOMETRY_EPSILON || tip_distance <= GEOMETRY_EPSILON {
                current_parent_rotation
            } else {
                target_y.atan2(target_x) - tip_y.atan2(tip_x)
            };
        let child_target = theta - child_shear;
        let parent_delta = shortest_angle_delta_f64(current_parent_rotation, parent_target);
        let child_delta = shortest_angle_delta_f64(current_child_rotation, child_target);
        let bend_measure = child_target.sin();
        let bend_matches = if positive_bend {
            bend_measure >= -GEOMETRY_EPSILON
        } else {
            bend_measure <= GEOMETRY_EPSILON
        };
        let error = (tip_distance - target_distance).abs();
        let angular_cost = parent_delta.abs() + child_delta.abs();
        let candidate = IkCandidate {
            parent_rotation: current_parent_rotation + parent_delta,
            child_rotation: current_child_rotation + child_delta,
            bend_matches,
            error,
            angular_cost,
            theta,
        };
        if best.is_none_or(|best| candidate.is_better_than(best)) {
            best = Some(candidate);
        }
    }

    let best = best?;
    let reach_scale = target_distance.max(best.error + target_distance).max(1.0);
    Some(TwoBoneIkSolution {
        parent_rotation: angle_from_f64(best.parent_rotation)?,
        child_rotation: angle_from_f64(best.child_rotation)?,
        child_translation_y: saturating_f32(child_y),
        child_y_was_zeroed: !uniform_scale && original_child_y != 0.0,
        reach: if best.error <= GEOMETRY_EPSILON * reach_scale {
            IkReach::Reached
        } else {
            IkReach::Closest
        },
    })
}

#[derive(Clone, Copy)]
struct IkCandidate {
    parent_rotation: f64,
    child_rotation: f64,
    bend_matches: bool,
    error: f64,
    angular_cost: f64,
    theta: f64,
}

impl IkCandidate {
    fn is_better_than(self, other: Self) -> bool {
        if self.bend_matches != other.bend_matches {
            return self.bend_matches;
        }
        let error_tolerance = GEOMETRY_EPSILON * self.error.max(other.error).max(1.0);
        if (self.error - other.error).abs() > error_tolerance {
            return self.error < other.error;
        }
        let angle_tolerance = GEOMETRY_EPSILON * self.angular_cost.max(other.angular_cost).max(1.0);
        if (self.angular_cost - other.angular_cost).abs() > angle_tolerance {
            return self.angular_cost < other.angular_cost;
        }
        self.theta < other.theta
    }
}

fn push_cosine(values: &mut [f64; 3], count: &mut usize, value: f64, clamp: bool) {
    if !value.is_finite() {
        return;
    }
    if !clamp && !(-1.0 - GEOMETRY_EPSILON..=1.0 + GEOMETRY_EPSILON).contains(&value) {
        return;
    }
    let value = value.clamp(-1.0, 1.0);
    if values[..*count]
        .iter()
        .any(|existing| (existing - value).abs() <= GEOMETRY_EPSILON)
    {
        return;
    }
    if let Some(destination) = values.get_mut(*count) {
        *destination = value;
        *count += 1;
    }
}

fn push_unique_angle(values: &mut [Option<f64>; 8], count: &mut usize, value: f64) {
    if !value.is_finite()
        || values[..*count]
            .iter()
            .flatten()
            .any(|existing| shortest_angle_delta_f64(*existing, value).abs() <= GEOMETRY_EPSILON)
    {
        return;
    }
    if let Some(destination) = values.get_mut(*count) {
        *destination = Some(value);
        *count += 1;
    }
}

fn nearest_angle(current: Angle, target_radians: f64) -> Option<Angle> {
    let current_radians = f64::from(current.as_radians());
    angle_from_f64(current_radians + shortest_angle_delta_f64(current_radians, target_radians))
}

fn angle_from_f64(radians: f64) -> Option<Angle> {
    radians
        .is_finite()
        .then(|| Angle::from_radians(saturating_f32(radians)).ok())
        .flatten()
}

fn shortest_angle_delta_f64(from: f64, to: f64) -> f64 {
    let mut delta =
        (to - from + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
    if (delta.abs() - std::f64::consts::PI).abs() <= 4.0 * f64::from(f32::EPSILON) {
        delta = -std::f64::consts::PI;
    }
    delta
}

fn checked_vec2(x: f64, y: f64) -> Option<Vec2> {
    let maximum = f64::from(f32::MAX);
    (x.is_finite() && y.is_finite() && x.abs() <= maximum && y.abs() <= maximum)
        .then(|| Vec2::new(x as f32, y as f32))
}

fn saturated_vec2(x: f64, y: f64) -> Vec2 {
    Vec2::new(saturating_f32(x), saturating_f32(y))
}

fn saturating_f32(value: f64) -> f32 {
    if value.is_nan() {
        0.0
    } else if value > f64::from(f32::MAX) {
        f32::MAX
    } else if value < -f64::from(f32::MAX) {
        -f32::MAX
    } else {
        value as f32
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::PI;

    use super::*;
    use crate::{Mix, Shear};

    const TOLERANCE: f32 = 0.000_1;

    fn angle(degrees: f32) -> Angle {
        Angle::from_degrees(degrees).expect("test angles are finite")
    }

    fn transform(
        translation: Vec2,
        rotation_degrees: f32,
        scale: Vec2,
        shear_degrees: Vec2,
    ) -> BoneTransform {
        BoneTransform::new(
            translation,
            angle(rotation_degrees),
            scale,
            Shear::from_degrees(shear_degrees.x, shear_degrees.y).expect("test shear is finite"),
        )
        .expect("test transforms are finite")
    }

    fn one_bone_rotation(
        parent: Option<WorldTransform>,
        bone: BoneTransform,
        target: Vec2,
    ) -> Option<Angle> {
        match solve_one_bone_ik(parent, bone, target) {
            Some(OneBoneIkSolution::Rotation(rotation)) => Some(rotation),
            Some(OneBoneIkSolution::PreservedCoincident) | None => None,
        }
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= TOLERANCE,
            "expected {expected}, got {actual}"
        );
    }

    fn assert_vec_close(actual: Vec2, expected: Vec2) {
        assert_close(actual.x, expected.x);
        assert_close(actual.y, expected.y);
    }

    #[test]
    fn root_and_parent_composition_use_matrix_columns() {
        let root = normal_local_to_world(
            None,
            transform(Vec2::new(10.0, 20.0), 90.0, Vec2::new(3.0, 2.0), Vec2::ZERO),
        );
        assert_vec_close(root.translation(), Vec2::new(10.0, 20.0));
        assert_vec_close(root.x_axis(), Vec2::new(0.0, 3.0));
        assert_vec_close(root.y_axis(), Vec2::new(-2.0, 0.0));

        let child = normal_local_to_world(
            Some(root),
            transform(Vec2::new(4.0, 5.0), 90.0, Vec2::ONE, Vec2::ZERO),
        );
        assert_vec_close(child.translation(), Vec2::new(0.0, 32.0));
        assert_vec_close(child.x_axis(), Vec2::new(-2.0, 0.0));
        assert_vec_close(child.y_axis(), Vec2::new(0.0, -3.0));
    }

    #[test]
    fn shear_rotates_each_local_axis_independently() {
        let world = normal_local_to_world(
            None,
            transform(Vec2::ZERO, 0.0, Vec2::new(2.0, 3.0), Vec2::new(30.0, -20.0)),
        );
        assert_vec_close(
            world.x_axis(),
            Vec2::new(2.0 * 30.0_f32.to_radians().cos(), 1.0),
        );
        assert_vec_close(
            world.y_axis(),
            Vec2::new(
                3.0 * 70.0_f32.to_radians().cos(),
                3.0 * 70.0_f32.to_radians().sin(),
            ),
        );
    }

    #[test]
    fn reflection_keeps_signed_axes_and_tip_direction() {
        let world = normal_local_to_world(
            None,
            transform(Vec2::ZERO, 0.0, Vec2::new(-2.0, 3.0), Vec2::ZERO),
        );
        assert_close(world.determinant() as f32, -6.0);
        assert_vec_close(
            world.transform_vector(Vec2::new(4.0, 0.0)),
            Vec2::new(-8.0, 0.0),
        );
    }

    #[test]
    fn inverse_round_trips_reflection_and_rejects_singular_axes() {
        let reflected = WorldTransform::new(
            Vec2::new(7.0, -2.0),
            Vec2::new(-2.0, 0.0),
            Vec2::new(0.0, 3.0),
        )
        .expect("the transform is finite");
        let local = Vec2::new(4.0, 5.0);
        let world = reflected.transform_point(local);
        assert_vec_close(
            reflected
                .try_inverse_point(world)
                .expect("a reflection is invertible"),
            local,
        );

        let singular =
            WorldTransform::new(Vec2::ZERO, Vec2::ZERO, Vec2::Y).expect("finite transform");
        assert_eq!(singular.try_inverse_point(Vec2::ONE), None);
    }

    #[test]
    fn constructor_rejects_nonfinite_components() {
        let error = WorldTransform::new(Vec2::NAN, Vec2::X, Vec2::Y)
            .expect_err("NaN must not enter a world transform");
        assert_eq!(error.component(), "translation");
    }

    #[test]
    fn shortest_angle_has_a_stable_negative_pi_tie() {
        let delta = shortest_angle_delta(Angle::ZERO, angle(180.0));
        assert_close(delta, -PI);
        let reverse = shortest_angle_delta(angle(180.0), Angle::ZERO);
        assert_close(reverse, -PI);
    }

    #[test]
    fn one_bone_ik_accounts_for_nonuniform_and_reflected_parents() {
        let bone = transform(Vec2::ZERO, 0.0, Vec2::ONE, Vec2::ZERO);
        let nonuniform = WorldTransform::new(Vec2::ZERO, Vec2::new(2.0, 0.0), Vec2::Y)
            .expect("finite transform");
        let desired = one_bone_rotation(Some(nonuniform), bone, Vec2::new(2.0, 1.0))
            .expect("the target has a direction");
        assert_close(desired.as_degrees(), 45.0);

        let reflected =
            WorldTransform::new(Vec2::ZERO, Vec2::NEG_X, Vec2::Y).expect("finite transform");
        let desired = one_bone_rotation(Some(reflected), bone, Vec2::X)
            .expect("a reflection remains invertible");
        assert_close(desired.as_degrees(), -180.0);
    }

    #[test]
    fn one_bone_ik_accounts_for_signed_bone_scale_and_mix_inputs() {
        let bone = transform(Vec2::ZERO, 0.0, Vec2::new(-1.0, 1.0), Vec2::ZERO);
        let desired = one_bone_rotation(None, bone, Vec2::X)
            .expect("negative scale has a well-defined pointing rotation");
        assert_close(desired.as_degrees(), -180.0);

        let mix = Mix::new(0.25).expect("the mix is normalized");
        let quarter = Angle::from_radians(
            Angle::ZERO.as_radians() + mix.get() * shortest_angle_delta(Angle::ZERO, angle(90.0)),
        )
        .expect("the mixed angle is finite");
        assert_close(quarter.as_degrees(), 22.5);
    }

    #[test]
    fn one_bone_ik_preserves_underdetermined_or_singular_poses() {
        let bone = transform(Vec2::ZERO, 10.0, Vec2::ONE, Vec2::ZERO);
        assert_eq!(one_bone_rotation(None, bone, Vec2::ZERO), None);
        let singular =
            WorldTransform::new(Vec2::ZERO, Vec2::ZERO, Vec2::Y).expect("finite transform");
        assert_eq!(one_bone_rotation(Some(singular), bone, Vec2::X), None);
    }

    #[test]
    fn two_bone_ik_selects_each_bend_branch() {
        let parent = transform(Vec2::ZERO, 0.0, Vec2::ONE, Vec2::ZERO);
        let child = transform(Vec2::new(3.0, 0.0), 0.0, Vec2::ONE, Vec2::ZERO);

        let positive = solve_two_bone_ik(
            None,
            parent,
            child,
            4.0,
            Vec2::new(0.0, 5.0),
            BendDirection::Positive,
        )
        .expect("the target is reachable");
        assert_close(positive.parent_rotation.as_degrees(), 36.869_9);
        assert_close(positive.child_rotation.as_degrees(), 90.0);
        assert_eq!(positive.reach, IkReach::Reached);

        let negative = solve_two_bone_ik(
            None,
            parent,
            child,
            4.0,
            Vec2::new(0.0, 5.0),
            BendDirection::Negative,
        )
        .expect("the target is reachable");
        assert_close(negative.parent_rotation.as_degrees(), 143.130_1);
        assert_close(negative.child_rotation.as_degrees(), -90.0);
        assert_eq!(negative.reach, IkReach::Reached);
    }

    #[test]
    fn two_bone_ik_projects_unreachable_targets_to_closest_reach() {
        let parent = transform(Vec2::ZERO, 0.0, Vec2::ONE, Vec2::ZERO);
        let child = transform(Vec2::new(3.0, 0.0), 0.0, Vec2::ONE, Vec2::ZERO);
        let solution = solve_two_bone_ik(
            None,
            parent,
            child,
            4.0,
            Vec2::new(20.0, 0.0),
            BendDirection::Positive,
        )
        .expect("an unreachable target still has a closest pose");
        assert_close(solution.parent_rotation.as_degrees(), 0.0);
        assert_close(solution.child_rotation.as_degrees(), 0.0);
        assert_eq!(solution.reach, IkReach::Closest);
    }

    #[test]
    fn two_bone_ik_solves_nonuniform_parent_scale_and_zeroes_child_y() {
        let parent = transform(Vec2::ZERO, 0.0, Vec2::new(2.0, 1.0), Vec2::ZERO);
        let child = transform(Vec2::new(3.0, 7.0), 0.0, Vec2::ONE, Vec2::ZERO);
        let solution = solve_two_bone_ik(
            None,
            parent,
            child,
            4.0,
            Vec2::new(10.0, 12.0_f32.sqrt()),
            BendDirection::Positive,
        )
        .expect("the quadratic has a reachable positive branch");
        assert_close(solution.parent_rotation.as_degrees(), 0.0);
        assert_close(solution.child_rotation.as_degrees(), 60.0);
        assert_close(solution.child_translation_y, 0.0);
        assert!(solution.child_y_was_zeroed);
        assert_eq!(solution.reach, IkReach::Reached);
    }

    #[test]
    fn two_bone_ik_rejects_singular_inputs_without_invalid_numbers() {
        let parent = transform(Vec2::ZERO, 0.0, Vec2::ONE, Vec2::ZERO);
        let child = transform(Vec2::new(3.0, 0.0), 0.0, Vec2::ONE, Vec2::ZERO);
        let singular =
            WorldTransform::new(Vec2::ZERO, Vec2::ZERO, Vec2::Y).expect("finite transform");
        assert_eq!(
            solve_two_bone_ik(
                Some(singular),
                parent,
                child,
                4.0,
                Vec2::X,
                BendDirection::Positive,
            ),
            None
        );
        assert_eq!(
            solve_two_bone_ik(None, parent, child, 0.0, Vec2::X, BendDirection::Positive,),
            None
        );
    }
}

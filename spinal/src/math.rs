use glam::Vec2;
use thiserror::Error;

/// A finite angle stored canonically in radians.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Angle(f32);

impl Angle {
    /// Zero radians.
    pub const ZERO: Self = Self(0.0);

    /// Constructs an angle from a finite number of radians.
    pub fn from_radians(radians: f32) -> Result<Self, InvalidAngle> {
        if radians.is_finite() {
            Ok(Self(radians))
        } else {
            Err(InvalidAngle)
        }
    }

    /// Constructs an angle from a finite number of degrees.
    pub fn from_degrees(degrees: f32) -> Result<Self, InvalidAngle> {
        Self::from_radians(degrees.to_radians())
    }

    /// Returns the angle in radians.
    #[must_use]
    pub const fn as_radians(self) -> f32 {
        self.0
    }

    /// Returns the angle in degrees.
    #[must_use]
    pub fn as_degrees(self) -> f32 {
        self.0.to_degrees()
    }
}

impl Default for Angle {
    fn default() -> Self {
        Self::ZERO
    }
}

/// Returned when an angle is NaN or infinite.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("an angle must be finite")]
pub struct InvalidAngle;

/// Unit-safe X/Y shear angles as authored on a bone.
///
/// Spine exports both values in degrees. Spinal stores them canonically in
/// radians through [`Angle`] so callers cannot accidentally pass degree values
/// where runtime radians are expected.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Shear {
    x: Angle,
    y: Angle,
}

impl Shear {
    /// No shear on either axis.
    pub const ZERO: Self = Self::new(Angle::ZERO, Angle::ZERO);

    /// Constructs shear from two already-validated angles.
    #[must_use]
    pub const fn new(x: Angle, y: Angle) -> Self {
        Self { x, y }
    }

    /// Constructs shear from finite authored degree values.
    pub fn from_degrees(x: f32, y: f32) -> Result<Self, InvalidAngle> {
        Ok(Self::new(Angle::from_degrees(x)?, Angle::from_degrees(y)?))
    }

    /// Constructs shear from finite runtime radian values.
    pub fn from_radians(x: f32, y: f32) -> Result<Self, InvalidAngle> {
        Ok(Self::new(Angle::from_radians(x)?, Angle::from_radians(y)?))
    }

    /// Returns the X-axis shear angle.
    #[must_use]
    pub const fn x(self) -> Angle {
        self.x
    }

    /// Returns the Y-axis shear angle.
    #[must_use]
    pub const fn y(self) -> Angle {
        self.y
    }

    /// Returns both shear angles in canonical radians.
    #[must_use]
    pub fn as_radians(self) -> Vec2 {
        Vec2::new(self.x.as_radians(), self.y.as_radians())
    }
}

/// A finite normalized influence in the inclusive range `0.0..=1.0`.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Mix(f32);

impl Mix {
    /// No influence.
    pub const ZERO: Self = Self(0.0);

    /// Full influence.
    pub const ONE: Self = Self(1.0);

    /// Constructs a mix without silently changing its value.
    pub fn new(value: f32) -> Result<Self, InvalidMix> {
        if value.is_finite() && (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(InvalidMix { value })
        }
    }

    /// Constructs a mix by explicitly clamping a finite value.
    ///
    /// NaN and infinity remain errors because they cannot be clamped into a
    /// meaningful animation influence.
    pub fn clamped(value: f32) -> Result<Self, InvalidMix> {
        if value.is_finite() {
            Ok(Self(value.clamp(0.0, 1.0)))
        } else {
            Err(InvalidMix { value })
        }
    }

    /// Returns the normalized influence.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

/// Returned when a mix is non-finite or outside `0.0..=1.0`.
#[derive(Clone, Copy, Debug, Error, PartialEq)]
#[error("mix must be finite and in 0.0..=1.0, got {value}")]
pub struct InvalidMix {
    value: f32,
}

impl InvalidMix {
    /// Returns the rejected value.
    #[must_use]
    pub const fn value(self) -> f32 {
        self.value
    }
}

/// A finite, intentionally unbounded transform-constraint influence.
///
/// Unlike IK mix, Spine transform-constraint mixes may be negative to apply
/// the copied property in the opposite direction or greater than one to
/// exaggerate it.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct TransformMix(f32);

impl TransformMix {
    /// No influence.
    pub const ZERO: Self = Self(0.0);

    /// Full influence.
    pub const ONE: Self = Self(1.0);

    /// Constructs a transform mix without clamping it.
    pub fn new(value: f32) -> Result<Self, InvalidTransformMix> {
        if value.is_finite() {
            Ok(Self(value))
        } else {
            Err(InvalidTransformMix { value })
        }
    }

    /// Returns the authored influence.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

/// Returned when a transform-constraint mix is NaN or infinite.
#[derive(Clone, Copy, Debug, Error, PartialEq)]
#[error("transform constraint mix must be finite, got {value}")]
pub struct InvalidTransformMix {
    value: f32,
}

impl InvalidTransformMix {
    /// Returns the rejected value.
    #[must_use]
    pub const fn value(self) -> f32 {
        self.value
    }
}

/// A bone-local 2D transform.
///
/// Rotation and both shear axes are represented by unit-safe angles stored in
/// radians.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoneTransform {
    translation: Vec2,
    rotation: Angle,
    scale: Vec2,
    shear: Shear,
}

impl BoneTransform {
    /// The identity transform.
    pub const IDENTITY: Self = Self {
        translation: Vec2::ZERO,
        rotation: Angle::ZERO,
        scale: Vec2::ONE,
        shear: Shear::ZERO,
    };

    /// Constructs a transform after checking every numeric component.
    pub fn new(
        translation: Vec2,
        rotation: Angle,
        scale: Vec2,
        shear: Shear,
    ) -> Result<Self, InvalidBoneTransform> {
        if !translation.is_finite() {
            return Err(InvalidBoneTransform::new("translation"));
        }
        if !scale.is_finite() {
            return Err(InvalidBoneTransform::new("scale"));
        }
        Ok(Self {
            translation,
            rotation,
            scale,
            shear,
        })
    }

    /// Returns the local translation.
    #[must_use]
    pub const fn translation(self) -> Vec2 {
        self.translation
    }

    /// Returns the local rotation.
    #[must_use]
    pub const fn rotation(self) -> Angle {
        self.rotation
    }

    /// Returns the local scale.
    #[must_use]
    pub const fn scale(self) -> Vec2 {
        self.scale
    }

    /// Returns the unit-safe local shear angles.
    #[must_use]
    pub const fn shear(self) -> Shear {
        self.shear
    }
}

impl Default for BoneTransform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// Returned when a transform contains NaN or infinity.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("transform {component} must be finite")]
pub struct InvalidBoneTransform {
    component: &'static str,
}

impl InvalidBoneTransform {
    const fn new(component: &'static str) -> Self {
        Self { component }
    }

    /// Returns the invalid transform component.
    #[must_use]
    pub const fn component(self) -> &'static str {
        self.component
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transforms_reject_each_non_finite_vector_component() {
        let invalid_translation = BoneTransform::new(
            Vec2::new(f32::NAN, 0.0),
            Angle::ZERO,
            Vec2::ONE,
            Shear::ZERO,
        )
        .expect_err("NaN translation must be rejected");
        assert_eq!(invalid_translation.component(), "translation");

        let invalid_scale = BoneTransform::new(
            Vec2::ZERO,
            Angle::ZERO,
            Vec2::new(1.0, f32::INFINITY),
            Shear::ZERO,
        )
        .expect_err("infinite scale must be rejected");
        assert_eq!(invalid_scale.component(), "scale");
    }

    #[test]
    fn explicit_mix_clamping_still_rejects_non_finite_values() {
        assert_eq!(Mix::clamped(-2.0).expect("finite values clamp"), Mix::ZERO);
        assert_eq!(Mix::clamped(2.0).expect("finite values clamp"), Mix::ONE);
        assert!(Mix::clamped(f32::NAN).is_err());
        assert!(Mix::clamped(f32::INFINITY).is_err());
    }

    #[test]
    fn transform_mix_is_finite_but_intentionally_unbounded() {
        assert_eq!(TransformMix::new(-2.0).unwrap().get(), -2.0);
        assert_eq!(TransformMix::new(3.0).unwrap().get(), 3.0);
        assert!(TransformMix::new(f32::NAN).is_err());
        assert!(TransformMix::new(f32::INFINITY).is_err());
    }
}

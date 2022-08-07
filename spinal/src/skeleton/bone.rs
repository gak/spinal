use crate::color::Color;
use bevy_math::Vec2;
use strum::FromRepr;

#[derive(Debug, PartialEq, FromRepr)]
pub enum ParentTransform {
    Normal,
    OnlyTranslation,
    NoRotationOrReflection,
    NoScale,
    NoScaleOrReflection,
}

impl Default for ParentTransform {
    fn default() -> Self {
        ParentTransform::Normal
    }
}

impl From<u8> for ParentTransform {
    fn from(v: u8) -> Self {
        ParentTransform::from_repr(v.into()).unwrap_or(ParentTransform::Normal)
    }
}

#[derive(Debug)]
pub struct Bone {
    /// The bone name. This is unique for the skeleton.
    pub name: String,

    /// Parent of this bone.
    ///
    /// `None` is the root bone, which should also be the first entry in `Skeleton::bones`.
    pub parent: Option<usize>,

    /// The length of the bone. The bone length is not typically used at runtime except to draw
    /// debug lines for the bones. Assume 0 if omitted.
    pub length: f32,

    /// Determines how parent bone transforms are inherited: normal, onlyTranslation,
    /// noRotationOrReflection, noScale, or noScaleOrReflection. Assume normal if omitted.
    pub transform: ParentTransform,

    /// If true, the bone is only active when the active skin has the bone. Assume false if omitted.
    pub skin: bool,

    pub position: Vec2,
    /// The position of the bone relative to the parent for the setup pose.
    /// Assume origin if omitted.

    /// The rotation in degrees of the bone relative to the parent for the setup pose.
    /// Assume 0 if omitted.
    pub rotation: f32,

    /// The scale of the bone for the setup pose. Assume `Vec2::ONE` if omitted.
    pub scale: Vec2,

    /// The shear of the bone for the setup pose. Assume `Vec2::ZERO` if omitted.
    pub shear: Vec2,

    /// The color of the bone, as it was in Spine. Assume 0x989898FF RGBA if omitted.
    pub color: Color,
}

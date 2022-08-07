use serde::Deserialize;
use strum::FromRepr;

#[derive(Debug, Deserialize, PartialEq, FromRepr)]
#[serde(rename_all = "camelCase")]
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

#[derive(Debug, Deserialize, Default)]
#[serde(untagged)]
pub enum BoneParent {
    #[default]
    Root,
    String(String),
    Index(usize),
}

impl From<u32> for BoneParent {
    fn from(index: u32) -> Self {
        BoneParent::Index(index as usize)
    }
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Color {
    Number(u32),
    String(String),
}

impl Color {
    pub fn white() -> Self {
        Color::Number(0xFFFFFFFF)
    }

    pub fn bone_default() -> Self {
        Color::Number(0x989898FF)
    }

    pub fn bounding_box_default() -> Self {
        Color::Number(0x989898FF)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bone {
    /// The bone name. This is unique for the skeleton.
    pub name: String,

    /// Parent of this bone.
    ///
    /// We ultimately want `BoneParent::Index`, but JSON will use a string reference.
    ///
    /// TODO: Instead of enum, maybe use `JsonBone` vs `Bone` which only contains the index.
    #[serde(default)]
    pub parent: BoneParent,

    /// The length of the bone. The bone length is not typically used at runtime except to draw
    /// debug lines for the bones. Assume 0 if omitted.
    #[serde(default)]
    pub length: f32,

    /// Determines how parent bone transforms are inherited: normal, onlyTranslation,
    /// noRotationOrReflection, noScale, or noScaleOrReflection. Assume normal if omitted.
    #[serde(default)]
    pub transform: ParentTransform,

    /// If true, the bone is only active when the active skin has the bone. Assume false if omitted.
    #[serde(default)]
    pub skin: bool,

    /// The X position of the bone relative to the parent for the setup pose. Assume 0 if omitted.
    #[serde(default)]
    pub x: f32,

    /// The Y position of the bone relative to the parent for the setup pose. Assume 0 if omitted.
    #[serde(default)]
    pub y: f32,

    /// The rotation in degrees of the bone relative to the parent for the setup pose.
    /// Assume 0 if omitted.
    #[serde(default)]
    pub rotation: f32,

    /// The X scale of the bone for the setup pose. Assume 1 if omitted.
    #[serde(default = "crate::f32_one")]
    pub scale_x: f32,

    /// The Y scale of the bone for the setup pose. Assume 1 if omitted.
    #[serde(default = "crate::f32_one")]
    pub scale_y: f32,

    /// The X shear of the bone for the setup pose. Assume 0 if omitted.
    #[serde(default)]
    pub shear_x: f32,

    /// The Y shear of the bone for the setup pose. Assume 0 if omitted.
    #[serde(default)]
    pub shear_y: f32,

    /// The color of the bone, as it was in Spine. Assume 0x989898FF RGBA if omitted.
    #[serde(default = "Color::bone_default")]
    pub color: Color,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let s = serde_json::from_str::<Bone>(r#"{"name": "root"}"#).unwrap();
        assert_eq!(s.length, 0.0);
        assert_eq!(s.transform, ParentTransform::Normal);
        assert_eq!(s.skin, false);
        assert_eq!(s.x, 0.0);
        assert_eq!(s.y, 0.0);
        assert_eq!(s.rotation, 0.0);
        assert_eq!(s.scale_x, 1.0);
        assert_eq!(s.scale_y, 1.0);
        assert_eq!(s.shear_x, 0.0);
        assert_eq!(s.shear_y, 0.0);
        assert_eq!(s.color, Color::Number(0x989898FF));
    }

    #[test]
    fn rename() {
        let s = serde_json::from_str::<Bone>(r#"{"name": "root", "shearX": 5}"#).unwrap();
        assert_eq!(s.shear_x, 5.0);
    }
}

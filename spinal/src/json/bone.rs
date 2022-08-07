use crate::color::Color;
use crate::skeleton::Bone;
use serde::Deserialize;
use strum::FromRepr;
// use super::f32_one;

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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonBone {
    pub name: String,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub length: f32,
    #[serde(default)]
    pub transform: ParentTransform,
    #[serde(default)]
    pub skin: bool,
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    #[serde(default)]
    pub rotation: f32,
    #[serde(default = "super::f32_one")]
    pub scale_x: f32,
    #[serde(default = "super::f32_one")]
    pub scale_y: f32,
    #[serde(default)]
    pub shear_x: f32,
    #[serde(default)]
    pub shear_y: f32,
    #[serde(default = "super::bone_color")]
    pub color: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let s = serde_json::from_str::<JsonBone>(r#"{"name": "root"}"#).unwrap();
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
        assert_eq!(s.color, "989898FF");
    }

    #[test]
    fn rename() {
        let s = serde_json::from_str::<JsonBone>(r#"{"name": "root", "shearX": 5}"#).unwrap();
        assert_eq!(s.shear_x, 5.0);
    }
}

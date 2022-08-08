use crate::color::Color;
use crate::skeleton::Bone;
use bevy_math::Vec2;
use serde::Deserialize;
use strum::FromRepr;
// use super::f32_one;

#[derive(Debug, Deserialize, PartialEq, FromRepr, Clone, Copy)]
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

impl From<ParentTransform> for crate::skeleton::ParentTransform {
    fn from(json: ParentTransform) -> Self {
        match json {
            ParentTransform::Normal => crate::skeleton::ParentTransform::Normal,
            ParentTransform::OnlyTranslation => crate::skeleton::ParentTransform::OnlyTranslation,
            ParentTransform::NoRotationOrReflection => {
                crate::skeleton::ParentTransform::NoRotationOrReflection
            }
            ParentTransform::NoScale => crate::skeleton::ParentTransform::NoScale,
            ParentTransform::NoScaleOrReflection => {
                crate::skeleton::ParentTransform::NoScaleOrReflection
            }
        }
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

impl JsonBone {
    pub fn to_bone(&self, parent: Option<usize>) -> Bone {
        Bone {
            name: self.name.clone(),
            parent,
            length: self.length,
            transform: self.transform.into(),
            skin: self.skin,
            position: Vec2::new(self.x, self.y),
            rotation: self.rotation,
            scale: Vec2::new(self.scale_x, self.scale_y),
            shear: Vec2::new(self.shear_x, self.shear_y),
            color: self.color.as_str().into(),
        }
    }
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

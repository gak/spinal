use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
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
pub struct Bone {
    /// The bone name. This is unique for the skeleton.
    name: String,

    /// The length of the bone. The bone length is not typically used at runtime except to draw
    /// debug lines for the bones. Assume 0 if omitted.
    #[serde(default)]
    length: f32,

    /// Determines how parent bone transforms are inherited: normal, onlyTranslation,
    /// noRotationOrReflection, noScale, or noScaleOrReflection. Assume normal if omitted.
    #[serde(default)]
    transform: ParentTransform,

    /// If true, the bone is only active when the active skin has the bone. Assume false if omitted.
    #[serde(default)]
    skin: bool,

    /// The X position of the bone relative to the parent for the setup pose. Assume 0 if omitted.
    #[serde(default)]
    x: f32,

    /// The Y position of the bone relative to the parent for the setup pose. Assume 0 if omitted.
    #[serde(default)]
    y: f32,

    /// The rotation in degrees of the bone relative to the parent for the setup pose.
    /// Assume 0 if omitted.
    #[serde(default)]
    rotation: f32,

    /// The X scale of the bone for the setup pose. Assume 1 if omitted.
    #[serde(default = "f32_one")]
    scale_x: f32,

    /// The Y scale of the bone for the setup pose. Assume 1 if omitted.
    #[serde(default = "f32_one")]
    scale_y: f32,

    /// The X shear of the bone for the setup pose. Assume 0 if omitted.
    #[serde(default)]
    shear_x: f32,

    /// The Y shear of the bone for the setup pose. Assume 0 if omitted.
    #[serde(default)]
    shear_y: f32,

    /// The color of the bone, as it was in Spine. Assume 0x989898FF RGBA if omitted.
    #[serde(default = "default_color")]
    color: u32,
}

fn f32_one() -> f32 {
    1.0
}

fn default_color() -> u32 {
    0x989898FF
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
        assert_eq!(s.color, 0x989898FF);
    }

    #[test]
    fn rename() {
        let s = serde_json::from_str::<Bone>(r#"{"name": "root", "shearX": 5}"#).unwrap();
        assert_eq!(s.shear_x, 5.0);
    }
}
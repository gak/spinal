use crate::json::JsonSkeleton;
use crate::skeleton::{Info, Skeleton};
use bevy_math::Vec2;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonInfo {
    /// A hash of all the skeleton data. This can be used by tools to detect if the data has
    /// changed since the last time it was loaded.
    pub hash: String,

    /// The version of Spine that exported the data.
    ///
    /// Currently only supports 4.1.
    // TODO: Use semver::Version.
    #[serde(rename = "spine")]
    pub version: String,

    /// The x coordinate of the bottom left corner of the AABB for the skeleton's attachments as
    /// it was in the setup pose in Spine.
    pub x: f32,

    /// The y coordinate of the bottom left corner of the AABB for the skeleton's attachments as
    /// it was in the setup pose in Spine.
    pub y: f32,

    /// The AABB width for the skeleton's attachments as it was in the setup pose in Spine.
    ///
    /// This can be used as a general size of the skeleton, though the skeleton's AABB depends on
    /// how it is posed.
    pub width: f32,

    /// The AABB height for the skeleton's attachments as it was in the setup pose in Spine.
    pub height: f32,

    /// The dopesheet framerate in frames per second, as it was in Spine. Assume 30 if omitted.
    pub fps: Option<f32>,

    /// The images path, as it was in Spine.
    pub images: Option<PathBuf>,

    /// The audio path, as it was in Spine.
    pub audio: Option<PathBuf>,
}

impl JsonInfo {
    /// The coordinate of the bottom left corner of the AABB for the skeleton's attachments as
    /// it was in the setup pose in Spine.
    pub fn size(&self) -> Vec2 {
        Vec2::new(self.width, self.height)
    }

    pub fn origin(&self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
}

impl From<JsonInfo> for Info {
    fn from(json: JsonInfo) -> Self {
        Info {
            hash: json.hash.to_owned(),
            version: json.version.to_owned(),
            bottom_left: Vec2::new(json.x, json.y),
            size: Vec2::new(json.width, json.height),
            fps: json.fps,
            images: json.images,
            audio: json.audio,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        let json = r#"
            {
                "hash": "itfFESDjM1c",
                "spine": "4.1.06",
                "x": -188.63,
                "y": -7.94,
                "width": 418.45,
                "height": 686.2,
                "images": "./images/",
                "audio": ""
            }
        "#;
        let skeleton = serde_json::from_str::<JsonInfo>(json).unwrap();
        assert_eq!(skeleton.origin(), Vec2::new(-188.63, -7.94));
        assert_eq!(skeleton.size(), Vec2::new(418.45, 686.2));
    }
}

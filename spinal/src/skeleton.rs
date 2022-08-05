use std::path::PathBuf;
use bevy_math::Vec2;
use semver::Version;
use serde::{Deserialize};

#[derive(Debug, Deserialize)]
pub struct Skeleton {
    /// A hash of all the skeleton data. This can be used by tools to detect if the data has
    /// changed since the last time it was loaded.
    pub hash: String,

    /// The version of Spine that exported the data.
    ///
    /// Currently only supports 4.1.
    // TODO: Use semver::Version.
    pub spine: String,

    /// The coordinate of the bottom left corner of the AABB for the skeleton's attachments as
    /// it was in the setup pose in Spine.
    pub bottom_left_aabb: Vec2,

    /// The AABB width for the skeleton's attachments as it was in the setup pose in Spine.
    ///
    /// This can be used as a general size of the skeleton, though the skeleton's AABB depends on
    /// how it is posed.
    pub size: Vec2,

    /// The dopesheet framerate in frames per second, as it was in Spine. Assume 30 if omitted.
    pub fps: Option<f32>,

    /// The images path, as it was in Spine.
    pub images: Option<PathBuf>,
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
        let skeleton = serde_json::from_str::<Skeleton>(json).unwrap();
        dbg!(skeleton);
        todo!();
    }
}
use bevy_math::Vec2;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Default)]
pub struct Info {
    /// A hash of all the skeleton data. This can be used by tools to detect if the data has
    /// changed since the last time it was loaded.
    pub hash: String,

    /// The version of Spine that exported the data.
    ///
    /// Currently only supports 4.1.
    // TODO: Use semver::Version.
    pub version: String,

    /// The coordinate of the bottom left corner of the AABB for the skeleton's attachments as
    /// it was in the setup pose in Spine.
    pub bottom_left: Vec2,

    /// The AABB width and hight for the skeleton's attachments as it was in the setup pose in
    /// Spine.
    ///
    /// This can be used as a general size of the skeleton, though the skeleton's AABB depends on
    /// how it is posed.
    pub size: Vec2,

    /// The dopesheet framerate in frames per second, as it was in Spine. Assume 30 if omitted.
    pub fps: Option<f32>,

    /// The images path, as it was in Spine.
    pub images: Option<PathBuf>,

    /// The audio path, as it was in Spine.
    pub audio: Option<PathBuf>,
}

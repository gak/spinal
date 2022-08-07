mod attachment;
mod bone;
mod ik;
mod info;
mod skin;
mod slot;

use crate::color::Color;
use crate::skeleton::Skeleton;
use crate::SpinalError;
use serde::{Deserialize, Deserializer};

/// Parse a JSON skeleton.
pub fn parse(b: &[u8]) -> Result<Skeleton, SpinalError> {
    Ok(serde_json::from_slice::<JsonSkeleton>(b).unwrap().into()) // TODO: error
}

/// This struct is specifically for deserializing JSON. It is not intended to be used directly.
///
/// We use a separate struct for deserializing JSON because:
///
/// * It's nicer to not have to deal with `String`s as references to other elements. We can just
///   save the `String` then process into index references after finishing parsing the JSON.
/// * (In the future) the user can potentially disable JSON deserialization, which can remove the
///   serde dependency.
/// * There's a hack in Attachment where there is optionally an untagged enum which isn't supported
///   by serde.
#[derive(Debug, Deserialize)]
// #[serde(deny_unknown_fields)]
pub struct JsonSkeleton {
    pub skeleton: info::JsonInfo,
    pub bones: Vec<bone::JsonBone>,
    pub slots: Vec<slot::JsonSlot>,
    pub ik: Vec<ik::JsonIk>,
    pub skins: Vec<skin::JsonSkin>,
}

impl From<JsonSkeleton> for Skeleton {
    fn from(json: JsonSkeleton) -> Self {
        Self {
            info: json.skeleton.into(),
            bones: vec![],
            slots: vec![],
            ik: vec![],
            skins: vec![],
        }
    }
}

/// A helper for serde default.
pub(crate) fn f32_one() -> f32 {
    1.0
}

/// A helper for serde default.
pub(crate) fn default_true() -> bool {
    true
}

/// A helper for serde default.
pub(crate) fn white() -> String {
    Color::white().into()
}

/// A helper for serde default.
pub(crate) fn bounding_box_color() -> String {
    Color::bounding_box_default().into()
}

/// A helper for serde default.
pub(crate) fn bone_color() -> String {
    Color::bone_default().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_all() {
        let b = include_bytes!("../../assets/spineboy-pro-4.1/spineboy-pro.json");
        let skel = parse(b).unwrap();
        dbg!(skel);
    }
}

mod attachment;
mod bone;
mod ik;
mod info;
mod skin;
mod slot;

use self::bone::JsonBone;
use self::ik::JsonIk;
use self::info::JsonInfo;
use self::skin::JsonSkin;
use self::slot::JsonSlot;
use crate::color::Color;
use crate::skeleton::{Bone, Ik, Skeleton, Slot};
use crate::SpinalError;
use bevy_utils::HashMap;
use serde::{Deserialize, Deserializer};

/// Parse a JSON skeleton.
pub fn parse(b: &[u8]) -> Result<Skeleton, SpinalError> {
    Ok(serde_json::from_slice::<JsonSkeleton>(b)
        .unwrap()
        .try_into()?) // TODO: error
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
    pub skeleton: JsonInfo,
    pub bones: Vec<JsonBone>,
    pub slots: Vec<JsonSlot>,
    pub ik: Vec<JsonIk>,
    pub skins: Vec<JsonSkin>,
}

impl JsonSkeleton {
    // fn to_bones(&self, lookup: &Lookup) -> Result<Vec<Bone>, SpinalError> {
    //     let mut bones = Vec::with_capacity(self.bones.len());
    //     for json_bone in &self.bones {
    //         let parent_id = lookup.opt_bone_name_to_id(json_bone.parent.as_deref())?;
    //         bones.push(json_bone.into_bone(parent_id));
    //     }
    //     Ok(bones)
    // }
    //
    // fn to_slots(&self, lookup: &Lookup) -> Result<Vec<Slot>, SpinalError> {
    //     let mut slots = Vec::with_capacity(self.slots.len());
    //     for json_slot in &self.slots {
    //         let bone_id = lookup.bone_name_to_id(json_slot.bone.as_str())?;
    //         slots.push(json_slot.to_slot(bone_id)?);
    //     }
    //     Ok(slots)
    // }
    //
    // fn to_ik(&self, lookup: &Lookup) -> Result<Vec<Ik>, SpinalError> {
    //     let mut ik = Vec::with_capacity(self.ik.len());
    //     for json_ik in &self.ik {
    //         let bones = lookup.bone_name_to_id(json_ik.bone.as_str())?;
    //         let target_id = lookup.bone_name_to_id(json_ik.target.as_str())?;
    //         ik.push(json_ik.to_ik(bone_id, target_id));
    //     }
    //     Ok(ik)
    // }
}

impl TryFrom<JsonSkeleton> for Skeleton {
    type Error = SpinalError;

    fn try_from(json: JsonSkeleton) -> Result<Self, Self::Error> {
        let lookup = Lookup::new(&json);
        let JsonSkeleton {
            bones,
            slots,
            ik,
            skins,
            ..
        } = json;
        let bones = bones
            .into_iter()
            .map(|b| b.into_bone(&lookup))
            .collect::<Result<_, _>>()?;
        let slots = slots
            .into_iter()
            .map(|s| s.into_slot(&lookup))
            .collect::<Result<_, _>>()?;
        let ik = ik
            .into_iter()
            .map(|i| i.into_ik(&lookup))
            .collect::<Result<_, _>>()?;
        // let skins = skins
        //     .into_iter()
        //     .map(|s| s.into_skin(&lookup))
        //     .collect::<Result<_, _>>()?;

        Ok(Self {
            info: json.skeleton.into(),
            strings: vec![],
            bones,
            slots,
            ik,
            transforms: vec![],
            paths: vec![],
            skins: vec![],
            events: vec![],
        })
    }
}

pub struct Lookup {
    bone_name_to_id: HashMap<String, usize>,
}

impl Lookup {
    fn new(json: &JsonSkeleton) -> Self {
        let bone_name_to_id = json
            .bones
            .iter()
            .enumerate()
            .map(|(i, bone)| (bone.name.to_owned(), i))
            .collect();

        Self { bone_name_to_id }
    }

    fn bone_name_to_id(&self, name: &str) -> Result<usize, SpinalError> {
        self.bone_name_to_id
            .get(name)
            .map(|i| *i)
            .ok_or_else(|| SpinalError::InvalidBoneReference(name.to_owned()))
    }

    fn opt_bone_name_to_id(&self, name: Option<&str>) -> Result<Option<usize>, SpinalError> {
        name.map(|name| self.bone_name_to_id(name)).transpose()
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

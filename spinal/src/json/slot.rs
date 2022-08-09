use crate::color::Color;
use crate::json::Lookup;
use crate::skeleton::Slot;
use crate::SpinalError;
use serde::Deserialize;
use strum::FromRepr;

#[derive(Debug, Deserialize)]
pub struct JsonSlot {
    pub name: String,
    pub bone: String,
    #[serde(default = "super::white")]
    pub color: String,
    pub dark: Option<String>,
    pub attachment: Option<String>,
    #[serde(default)]
    pub blend: Blend,
}

impl JsonSlot {
    pub fn into_slot(self, lookup: &Lookup) -> Result<Slot, SpinalError> {
        Ok(Slot {
            name: self.name.clone(),
            bone: lookup.bone_name_to_id(self.bone.as_str())?,
            color: self.color.as_str().into(),
            dark: self.dark.as_ref().map(|dark| dark.as_str().into()),
            attachment: todo!(),
            blend: self.blend.into(),
        })
    }
}

#[derive(Debug, Deserialize, PartialEq, FromRepr, Copy, Clone)]
#[serde(rename_all = "camelCase")]
pub enum Blend {
    Normal,
    Additive,
    Multiply,
    Screen,
}

impl Default for Blend {
    fn default() -> Self {
        Blend::Normal
    }
}

impl From<Blend> for crate::skeleton::Blend {
    fn from(json_blend: Blend) -> Self {
        crate::skeleton::Blend::from_repr(json_blend as usize).unwrap()
    }
}

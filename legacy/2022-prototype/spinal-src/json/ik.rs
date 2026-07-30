use crate::json::Lookup;
use crate::skeleton::Ik;
use crate::SpinalError;
use serde::Deserialize;
use strum::FromRepr;

#[derive(Debug, Deserialize)]
pub struct JsonIk {
    pub name: String,
    pub order: u32,
    #[serde(default)]
    pub skin: bool,
    pub bones: Vec<String>,
    pub target: String,
    #[serde(default = "super::f32_one")]
    pub mix: f32,
    #[serde(default)]
    pub softness: f32,
    #[serde(default)]
    pub bend_positive: bool,
    #[serde(default)]
    pub compress: bool,
    #[serde(default)]
    pub stretch: bool,
    #[serde(default)]
    pub uniform: bool,
}

impl JsonIk {
    pub fn into_ik(self, lookup: &Lookup) -> Result<Ik, SpinalError> {
        Ok(Ik {
            name: self.name,
            order: self.order,
            skin: self.skin,
            bones: self
                .bones
                .iter()
                .map(|name| lookup.bone_name_to_id(name.as_str()))
                .collect::<Result<Vec<_>, _>>()?,
            target: lookup.bone_name_to_id(self.target.as_str())?,
            mix: self.mix,
            softness: self.softness,
            bend: todo!(),
            compress: self.compress,
            stretch: self.stretch,
            uniform: self.uniform,
        })
    }
}

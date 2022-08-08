use crate::json::attachment::JsonAttachment;
use crate::json::Lookup;
use crate::skeleton::Skin;
use crate::SpinalError;
use bevy_utils::HashMap;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct JsonSkin {
    name: String,

    attachments: HashMap<String, JsonAttachment>,
}

impl JsonSkin {
    pub fn into_skin(self, lookup: &Lookup) -> Result<Skin, SpinalError> {
        let attachments = todo!();

        Ok(Skin {
            name: self.name,
            attachments,
        })
    }
}

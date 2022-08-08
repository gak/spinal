use crate::skeleton::Attachment;
use bevy_utils::HashMap;

#[derive(Debug)]
pub struct Skin {
    pub name: String,

    pub attachments: HashMap<String, Attachment>,
}

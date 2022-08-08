use crate::skeleton::Attachment;
use bevy_utils::HashMap;

#[derive(Debug)]
pub struct Skin {
    pub name: String,
    pub bones: Vec<usize>,
    pub ik: Vec<usize>,
    pub transforms: Vec<usize>,
    pub paths: Vec<usize>,
    pub attachments: HashMap<String, Attachment>,
}

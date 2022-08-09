use crate::skeleton::AttachmentSlot;
use bevy_utils::HashMap;

#[derive(Debug, Default)]
pub struct Skin {
    pub name: String,
    pub bones: Vec<usize>,
    pub ik: Vec<usize>,
    pub transforms: Vec<usize>,
    pub paths: Vec<usize>,
    pub attachments: HashMap<String, AttachmentSlot>,
}

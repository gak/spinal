use crate::skeleton::{Attachment, AttachmentData};
use bevy_utils::HashMap;

#[derive(Debug, Default)]
pub struct Skin {
    pub name: String,
    pub bones: Vec<usize>,
    pub ik: Vec<usize>,
    pub transforms: Vec<usize>,
    pub paths: Vec<usize>,

    // It turns out that the JSON hierarchy is different from the binary format. The binary data
    // is flat like this, and slots reference attachments, which creates the hierarchy.
    pub attachments: Vec<Attachment>,
}

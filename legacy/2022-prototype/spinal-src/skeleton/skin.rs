use crate::skeleton::{Attachment, AttachmentData, Slot};
use bevy_utils::HashMap;

#[derive(Debug, Default)]
pub struct Skin {
    pub name: String,

    /// Only part of the non-default skin.
    pub bones: Vec<usize>,
    /// Only part of the non-default skin.
    pub ik: Vec<usize>,
    /// Only part of the non-default skin.
    pub transforms: Vec<usize>,
    /// Only part of the non-default skin.
    pub paths: Vec<usize>,

    pub slots: Vec<SkinSlot>,
}

#[derive(Debug, Default)]
pub struct SkinSlot {
    pub slot: usize,
    pub attachments: Vec<Attachment>,
}

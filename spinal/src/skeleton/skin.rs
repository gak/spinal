use crate::skeleton::Attachment;
use std::collections::HashMap;

#[derive(Debug)]
pub struct Skin {
    name: String,

    attachments: HashMap<String, Attachment>,
}

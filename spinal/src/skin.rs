use crate::Attachment;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct Skin {
    name: String,

    attachments: HashMap<String, Attachment>,
}

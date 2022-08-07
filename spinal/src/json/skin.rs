use crate::json::attachment::JsonAttachment;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct JsonSkin {
    name: String,

    attachments: HashMap<String, JsonAttachment>,
}

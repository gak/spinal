use crate::color::Color;
use serde::Deserialize;
use strum::FromRepr;

#[derive(Debug, Deserialize)]
pub struct JsonSlot {
    pub name: String,
    pub bone: String,
    #[serde(default = "super::white")]
    pub color: String,
    pub dark: Option<String>,
    pub attachment: Option<String>,
    #[serde(default)]
    pub blend: Blend,
}

#[derive(Debug, Deserialize, PartialEq, FromRepr)]
#[serde(rename_all = "camelCase")]
pub enum Blend {
    Normal,
    Additive,
    Multiply,
    Screen,
}

impl Default for Blend {
    fn default() -> Self {
        Blend::Normal
    }
}

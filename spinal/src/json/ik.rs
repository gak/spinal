use serde::Deserialize;
use strum::FromRepr;

#[derive(Debug, Deserialize)]
pub struct JsonIk {
    pub name: String,
    pub order: u32,
    #[serde(default)]
    pub skin: bool,
    pub bones: Vec<String>,
    pub target: String,
    #[serde(default = "super::f32_one")]
    pub mix: f32,
    #[serde(default)]
    pub softness: f32,
    #[serde(default)]
    pub bend_positive: bool,
    #[serde(default)]
    pub compress: bool,
    #[serde(default)]
    pub stretch: bool,
    #[serde(default)]
    pub uniform: bool,
}

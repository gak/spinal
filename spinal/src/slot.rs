use crate::Color;
use serde::Deserialize;
use strum::FromRepr;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Reference {
    Index(usize),
    Name(String),
}

#[derive(Debug, Deserialize)]
pub struct Slot {
    /// The slot name. This is unique for the skeleton.
    pub name: String,

    /// The bone that this slot is attached to.
    pub bone: Reference,

    /// The color of the slot for the setup pose. This is an 8 character string containing 4 two
    /// digit hex numbers in RGBA order. Assume "FF" for alpha if alpha is omitted.
    /// Assume "FFFFFFFF" if omitted.
    #[serde(default = "Color::white")]
    pub color: Color,

    /// The dark color of the slot for the setup pose, used for two color tinting. This is a 6
    /// character string containing 3 two digit hex numbers in RGB order. Omitted when two color
    /// tinting is not used.
    pub dark: Option<Color>,

    /// The name of the slot's attachment for the setup pose. Assume no attachment for the setup
    /// pose if omitted.
    ///
    /// The `Reference::Index` is the index of the Skeleton::strings array.
    pub attachment: Option<Reference>,

    /// The type of blending to use when drawing the slot's visible attachment: normal, additive,
    /// multiply, or screen.
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

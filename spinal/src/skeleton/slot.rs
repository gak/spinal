use crate::color::Color;
use strum::FromRepr;

#[derive(Debug)]
pub struct Slot {
    /// The slot name. This is unique for the skeleton.
    pub name: String,

    /// The bone that this slot is attached to.
    pub bone: usize,

    /// The color of the slot for the setup pose. This is an 8 character string containing 4 two
    /// digit hex numbers in RGBA order. Assume "FF" for alpha if alpha is omitted.
    /// Assume "FFFFFFFF" if omitted.
    pub color: Color,

    /// The dark color of the slot for the setup pose, used for two color tinting. Omitted when two
    /// color tinting is not used.
    pub dark: Option<Color>,

    /// The name of the slot's attachment for the setup pose. Assume no attachment for the setup
    /// pose if omitted.
    ///
    /// This is a reference to the string lookup.
    pub attachment: usize,

    /// The type of blending to use when drawing the slot's visible attachment: normal, additive,
    /// multiply, or screen.
    pub blend: Blend,
}

#[derive(Debug, PartialEq, FromRepr)]
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

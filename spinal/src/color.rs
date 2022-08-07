#[derive(Debug, PartialEq)]
pub struct Color(u32);

impl Color {
    pub fn white() -> Self {
        Color(0xFFFFFFFF)
    }

    pub fn bone_default() -> Self {
        Color(0x989898FF)
    }

    pub fn bounding_box_default() -> Self {
        Color(0x989898FF)
    }
}

impl From<Color> for String {
    fn from(color: Color) -> Self {
        format!("{:08X}", color.0)
    }
}

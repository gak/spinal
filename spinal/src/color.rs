#[derive(Debug, PartialEq)]
pub struct Color(pub u32);

impl Color {
    pub fn from_hex(s: &str) -> Self {
        let s = s.trim_start_matches("#");
        let r = u32::from_str_radix(&s[0..2], 16).unwrap();
        let g = u32::from_str_radix(&s[2..4], 16).unwrap();
        let b = u32::from_str_radix(&s[4..6], 16).unwrap();
        let a = u32::from_str_radix(&s[6..8], 16).unwrap();
        Color(a << 24 | b << 16 | g << 8 | r)
    }

    pub fn white() -> Self {
        Color(0xFFFFFFFF)
    }

    pub fn bone_default() -> Self {
        Color(0x989898FF)
    }

    pub fn bounding_box_default() -> Self {
        Color(0x60F000FF)
    }
}

impl From<Color> for String {
    fn from(color: Color) -> Self {
        format!("{:08X}", color.0)
    }
}

impl From<&str> for Color {
    fn from(s: &str) -> Self {
        Color::from_hex(&s)
    }
}

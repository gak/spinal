use core::fmt;
use thiserror::Error;

/// An unsigned pixel extent.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct PixelSize {
    width: u32,
    height: u32,
}

impl PixelSize {
    /// Creates a pixel extent.
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// Returns the width in pixels.
    #[must_use]
    pub const fn width(self) -> u32 {
        self.width
    }

    /// Returns the height in pixels.
    #[must_use]
    pub const fn height(self) -> u32 {
        self.height
    }
}

/// An unsigned rectangle in atlas-page pixels.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct PixelRect {
    x: u32,
    y: u32,
    size: PixelSize,
}

impl PixelRect {
    /// Creates a page-space pixel rectangle.
    #[must_use]
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            size: PixelSize::new(width, height),
        }
    }

    /// Returns the horizontal page coordinate.
    #[must_use]
    pub const fn x(self) -> u32 {
        self.x
    }

    /// Returns the vertical page coordinate.
    #[must_use]
    pub const fn y(self) -> u32 {
        self.y
    }

    /// Returns the packed extent.
    #[must_use]
    pub const fn size(self) -> PixelSize {
        self.size
    }

    /// Returns the packed width.
    #[must_use]
    pub const fn width(self) -> u32 {
        self.size.width()
    }

    /// Returns the packed height.
    #[must_use]
    pub const fn height(self) -> u32 {
        self.size.height()
    }
}

/// Trimming metadata for one packed atlas region.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Trim {
    left: u32,
    bottom: u32,
    original_size: PixelSize,
}

impl Trim {
    /// Creates trimming metadata.
    #[must_use]
    pub const fn new(left: u32, bottom: u32, original_width: u32, original_height: u32) -> Self {
        Self {
            left,
            bottom,
            original_size: PixelSize::new(original_width, original_height),
        }
    }

    /// Returns the trimmed pixels on the left.
    #[must_use]
    pub const fn left(self) -> u32 {
        self.left
    }

    /// Returns the trimmed pixels on the bottom.
    #[must_use]
    pub const fn bottom(self) -> u32 {
        self.bottom
    }

    /// Returns the unpacked image extent.
    #[must_use]
    pub const fn original_size(self) -> PixelSize {
        self.original_size
    }
}

/// The authored packed rotation in counter-clockwise degrees.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct AtlasRotation(f32);

impl AtlasRotation {
    /// No packed rotation.
    pub const ZERO: Self = Self(0.0);

    pub(crate) fn new(degrees: f32) -> Option<Self> {
        (degrees.is_finite() && (0.0..=360.0).contains(&degrees)).then_some(Self(degrees))
    }

    /// Returns the counter-clockwise rotation in degrees.
    #[must_use]
    pub const fn as_degrees(self) -> f32 {
        self.0
    }

    /// Returns whether the first renderer profile can draw this rotation.
    #[must_use]
    pub fn is_quarter_turn(self) -> bool {
        matches!(self.0, 0.0 | 90.0 | 180.0 | 270.0 | 360.0)
    }
}

/// An eight-bit straight RGBA colour.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Rgba8 {
    red: u8,
    green: u8,
    blue: u8,
    alpha: u8,
}

impl Rgba8 {
    /// Opaque white.
    pub const WHITE: Self = Self::new(255, 255, 255, 255);

    /// Creates a colour from red, green, blue, and alpha channels.
    #[must_use]
    pub const fn new(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }

    /// Returns the red channel.
    #[must_use]
    pub const fn red(self) -> u8 {
        self.red
    }

    /// Returns the green channel.
    #[must_use]
    pub const fn green(self) -> u8 {
        self.green
    }

    /// Returns the blue channel.
    #[must_use]
    pub const fn blue(self) -> u8 {
        self.blue
    }

    /// Returns the alpha channel.
    #[must_use]
    pub const fn alpha(self) -> u8 {
        self.alpha
    }

    /// Returns channels in RGBA order.
    #[must_use]
    pub const fn to_array(self) -> [u8; 4] {
        [self.red, self.green, self.blue, self.alpha]
    }
}

impl Default for Rgba8 {
    fn default() -> Self {
        Self::WHITE
    }
}

impl fmt::Display for Rgba8 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:02X}{:02X}{:02X}{:02X}",
            self.red, self.green, self.blue, self.alpha
        )
    }
}

/// A normalized finite RGBA modulation colour.
///
/// Components use the inclusive range `0.0..=1.0`. The values are interpolated
/// as authored modulation channels; Spinal does not perform a colour-space
/// conversion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    red: f32,
    green: f32,
    blue: f32,
    alpha: f32,
}

impl Rgba {
    /// Opaque white.
    pub const WHITE: Self = Self {
        red: 1.0,
        green: 1.0,
        blue: 1.0,
        alpha: 1.0,
    };

    /// Fully transparent black.
    pub const TRANSPARENT: Self = Self {
        red: 0.0,
        green: 0.0,
        blue: 0.0,
        alpha: 0.0,
    };

    /// Creates a colour after validating every normalized component.
    pub fn new(red: f32, green: f32, blue: f32, alpha: f32) -> Result<Self, InvalidRgba> {
        for (channel, value) in [
            ("red", red),
            ("green", green),
            ("blue", blue),
            ("alpha", alpha),
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(InvalidRgba { channel, value });
            }
        }
        Ok(Self {
            red,
            green,
            blue,
            alpha,
        })
    }

    /// Converts eight-bit unorm channels without loss.
    #[must_use]
    pub fn from_rgba8(colour: Rgba8) -> Self {
        let [red, green, blue, alpha] = colour.to_array();
        const SCALE: f32 = 1.0 / 255.0;
        Self {
            red: red as f32 * SCALE,
            green: green as f32 * SCALE,
            blue: blue as f32 * SCALE,
            alpha: alpha as f32 * SCALE,
        }
    }

    /// Returns the red channel.
    #[must_use]
    pub const fn red(self) -> f32 {
        self.red
    }

    /// Returns the green channel.
    #[must_use]
    pub const fn green(self) -> f32 {
        self.green
    }

    /// Returns the blue channel.
    #[must_use]
    pub const fn blue(self) -> f32 {
        self.blue
    }

    /// Returns the alpha channel.
    #[must_use]
    pub const fn alpha(self) -> f32 {
        self.alpha
    }

    /// Returns channels in RGBA order.
    #[must_use]
    pub const fn to_array(self) -> [f32; 4] {
        [self.red, self.green, self.blue, self.alpha]
    }

    pub(crate) fn lerp(self, other: Self, amounts: [f32; 4]) -> Self {
        let start = self.to_array();
        let end = other.to_array();
        Self {
            red: (start[0] + (end[0] - start[0]) * amounts[0]).clamp(0.0, 1.0),
            green: (start[1] + (end[1] - start[1]) * amounts[1]).clamp(0.0, 1.0),
            blue: (start[2] + (end[2] - start[2]) * amounts[2]).clamp(0.0, 1.0),
            alpha: (start[3] + (end[3] - start[3]) * amounts[3]).clamp(0.0, 1.0),
        }
    }
}

impl Default for Rgba {
    fn default() -> Self {
        Self::WHITE
    }
}

/// Returned when a normalized colour component is non-finite or outside
/// `0.0..=1.0`.
#[derive(Clone, Copy, Debug, Error, PartialEq)]
#[error("RGBA {channel} must be finite and in 0.0..=1.0, got {value}")]
pub struct InvalidRgba {
    channel: &'static str,
    value: f32,
}

impl InvalidRgba {
    /// Returns the rejected channel name.
    #[must_use]
    pub const fn channel(self) -> &'static str {
        self.channel
    }

    /// Returns the rejected component value.
    #[must_use]
    pub const fn value(self) -> f32 {
        self.value
    }
}

/// How pixel alpha is encoded in an atlas page.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum AlphaEncoding {
    /// RGB channels are independent of alpha.
    #[default]
    Straight,
    /// RGB channels have already been multiplied by alpha.
    Premultiplied,
}

/// A documented atlas texture filter.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum TextureFilter {
    /// Nearest-neighbour sampling.
    #[default]
    Nearest,
    /// Linear sampling.
    Linear,
    /// Mipmapped sampling with implementation-selected filtering.
    MipMap,
    /// Nearest sampling between and within mip levels.
    MipMapNearestNearest,
    /// Linear sampling between mip levels and nearest within each level.
    MipMapLinearNearest,
    /// Nearest sampling between mip levels and linear within each level.
    MipMapNearestLinear,
    /// Linear sampling between and within mip levels.
    MipMapLinearLinear,
    /// A valid atlas token that this version does not recognize.
    Unknown,
}

/// A documented atlas texture format.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum TextureFormat {
    /// Alpha only.
    Alpha,
    /// Intensity only.
    Intensity,
    /// Luminance and alpha.
    LuminanceAlpha,
    /// 16-bit RGB.
    Rgb565,
    /// 16-bit RGBA.
    Rgba4444,
    /// 24-bit RGB.
    Rgb888,
    /// 32-bit RGBA.
    #[default]
    Rgba8888,
    /// A valid atlas token that this version does not recognize.
    Unknown,
}

/// Texture wrapping on each atlas-page axis.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct WrapMode {
    x: bool,
    y: bool,
}

impl WrapMode {
    /// No wrapping.
    pub const CLAMP: Self = Self::new(false, false);

    /// Creates an axis-specific wrapping mode.
    #[must_use]
    pub const fn new(x: bool, y: bool) -> Self {
        Self { x, y }
    }

    /// Returns whether the X axis repeats.
    #[must_use]
    pub const fn x(self) -> bool {
        self.x
    }

    /// Returns whether the Y axis repeats.
    #[must_use]
    pub const fn y(self) -> bool {
        self.y
    }
}

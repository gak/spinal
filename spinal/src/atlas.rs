use bevy_math::Vec2;
pub mod parser;

/// A parsed Spine atlas.
///
/// To read an atlas file into this struct, use [`AtlasParser::parse`].
///
/// See http://esotericsoftware.com/spine-atlas-format
#[derive(Debug)]
pub struct Atlas {
    pages: Vec<Page>,
}

// Page entries are separated by a blank line.
#[derive(Debug)]
pub struct Page {
    header: Header,
    regions: Vec<Region>,
}

/// The header provides the page image name and information about loading and rendering the image.
#[derive(Debug)]
pub struct Header {
    /// The first line is the image name for this page. Where the image is located is up to the
    /// atlas loader, but typically it is relative to directory containing the atlas file.
    pub name: String,

    /// The width and height of the page image. This is useful for the atlas loader to know
    /// before it loads the image, eg to allocate a buffer. 0,0 if omitted.
    pub size: Vec2,

    /// The format the atlas loader should use to store the image in memory. Atlas loaders
    /// may ignore this property. Possible values: Alpha, Intensity, LuminanceAlpha, RGB565,
    /// RGBA4444, RGB888, or RGBA8888. RGBA8888 if omitted.
    pub format: TextureFormatPixelInfo,

    /// Texture filter minification and magnification settings. Atlas loaders may ignore this
    /// property. Possible values: Nearest, Linear, MipMap, MipMapNearestNearest, MipMapLinearNearest,
    /// MipMapNearestLinear, or MipMapLinearLinear. Nearest if omitted.
    pub filter: TextureFilter,

    /// Texture wrap settings. Atlas loaders may ignore this property. Possible values: x, y, xy,
    /// or none.
    pub repeat: TextureRepeat,

    /// If true, the image for this page has had premultiplied alpha applied. false if omitted.
    pub pma: bool,
}

#[derive(Debug, Default)]
pub enum TextureFormatPixelInfo {
    Alpha,
    Intensity,
    LuminanceAlpha,
    RGB565,
    RGBA4444,
    RGB888,
    #[default]
    RGBA8888,
}

#[derive(Debug, Default)]
pub enum TextureFilter {
    #[default]
    Nearest,
    Linear,
    MipMap,
    MipMapNearestNearest,
    MipMapLinearNearest,
    MipMapNearestLinear,
    MipMapLinearLinear,
}

#[derive(Debug, Default)]
pub enum TextureRepeat {
    #[default]
    None,
    X,
    Y,
    XY,
}

/// The region section provides the region location within the page image and other information about the region.
/*

   name: The first line is the region name. This is used to find a region in the atlas. Multiple regions may have the same name if they have a different index.
   index: An index allows many images to be packed using the same name, as long as each has a different index. Typically the index is the frame number for regions that will be shown sequentially for frame-by-frame animation. -1 if omitted.
   bounds: The x and y pixel location of this image within the page image and the packed image size, which is the pixel size of this image within the page image. 0,0,0,0 if omitted.
   offsets: The amount of whitespace pixels that were stripped from the left and bottom edges of the image before it was packed and the original image size, which is the pixel size of this image before it was packed. If whitespace stripping was performed, the original image size may be larger than the packed image size. If omitted, 0,0 is used for the left and bottom edges and the original image size is equal to the packed image size.
   rotate: If true, the region was stored in the page image rotated by 90 degrees counter clockwise. Otherwise it may be false for 0 rotation or a number representing degrees from 0 to 360. 0 if omitted.
   split: The left, right, top, and bottom splits for a ninepatch. These are the number of pixels from the original image edges. Splits define a 3x3 grid for a scaling an image without stretching all parts of the image. null if omitted.
   pad: The left, right, top, and bottom padding for a ninepatch. These are the number of pixels from the original image edges. Padding allows content placed on top of a ninepatch to be inset differently from the splits. null if omitted.

*/
#[derive(Debug, Default)]
pub struct Region {
    /// The first line is the region name. This is used to find a region in the atlas. Multiple
    /// regions may have the same name if they have a different index.
    pub name: String,

    /// An index allows many images to be packed using the same name, as long as each has a
    /// different index. Typically the index is the frame number for regions that will be shown
    /// sequentially for frame-by-frame animation. -1 if omitted.
    pub index: Option<usize>,

    /// The x and y pixel location of this image within the page image and the packed image size,
    /// which is the pixel size of this image within the page image. 0,0,0,0 if omitted.
    pub bounds: Option<Bounds>,

    /// The amount of whitespace pixels that were stripped from the left and bottom edges of the
    /// image before it was packed and the original image size, which is the pixel size of this
    /// image before it was packed. If whitespace stripping was performed, the original image size
    /// may be larger than the packed image size. If omitted, 0,0 is used for the left and bottom
    /// edges and the original image size is equal to the packed image size.
    pub offsets: Option<()>, // TODO: ??

    /// If true, the region was stored in the page image rotated by 90 degrees counter clockwise.
    /// Otherwise it may be false for 0 rotation or a number representing degrees from 0 to 360.
    /// 0 if omitted.
    pub rotate: Option<f32>,

    /// The left, right, top, and bottom splits for a ninepatch. These are the number of pixels
    /// from the original image edges. Splits define a 3x3 grid for a scaling an image without
    /// stretching all parts of the image. null if omitted.
    pub split: Option<()>, // TODO: ??

    /// The left, right, top, and bottom padding for a ninepatch. These are the number of pixels
    /// from the original image edges. Padding allows content placed on top of a ninepatch to be
    /// inset differently from the splits. null if omitted.
    pub pad: Option<()>, // TODO: ??
}

#[derive(Debug, Default)]
pub struct Bounds {
    position: Vec2,
    size: Vec2,
}
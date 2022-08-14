use bevy_math::Vec2;
pub mod parser;

/// A parsed Spine atlas.
///
/// To read an atlas file into this struct, use [`AtlasParser::parse`].
///
/// See http://esotericsoftware.com/spine-atlas-format
#[derive(Debug)]
pub struct Atlas {
    pub pages: Vec<AtlasPage>,
}

// Page entries are separated by a blank line.
#[derive(Debug)]
pub struct AtlasPage {
    pub header: Header,
    pub regions: Vec<AtlasRegion>,
}

/// The header provides the page image name and information about loading and rendering the image.
#[derive(Debug, Default)]
pub struct Header {
    /// The first line is the image name for this page. Where the image is located is up to the
    /// atlas loader, but typically it is relative to directory containing the atlas file.
    pub name: String,

    /// The width and height of the page image. This is useful for the atlas loader to know
    /// before it loads the image, eg to allocate a buffer. 0,0 if omitted.
    pub size: Vec2,

    /// If true, the image for this page has had premultiplied alpha applied. false if omitted.
    pub premultiplied_alpha: bool,
}

/// The region section provides the region location within the page image and other information
/// about the region.
#[derive(Debug, Default)]
pub struct AtlasRegion {
    /// The first line is the region name. This is used to find a region in the atlas. Multiple
    /// regions may have the same name if they have a different index.
    pub name: String,

    /// An index allows many images to be packed using the same name, as long as each has a
    /// different index. Typically the index is the frame number for regions that will be shown
    /// sequentially for frame-by-frame animation. -1 if omitted.
    pub index: Option<usize>,

    /// The x and y pixel location of this image within the page image and the packed image size,
    /// which is the pixel size of this image within the page image. 0,0,0,0 if omitted.
    pub bounds: Option<Rect>,

    /// The amount of whitespace pixels that were stripped from the left and bottom edges of the
    /// image before it was packed and the original image size, which is the pixel size of this
    /// image before it was packed. If whitespace stripping was performed, the original image size
    /// may be larger than the packed image size. If omitted, 0,0 is used for the left and bottom
    /// edges and the original image size is equal to the packed image size.
    pub offsets: Option<Rect>,

    /// If true, the region was stored in the page image rotated by 90 degrees counter clockwise.
    /// Otherwise it may be false for 0 rotation or a number representing degrees from 0 to 360.
    /// 0 if omitted.
    pub rotate: f32,

    /// The left, right, top, and bottom splits for a ninepatch. These are the number of pixels
    /// from the original image edges. Splits define a 3x3 grid for a scaling an image without
    /// stretching all parts of the image. null if omitted.
    pub split: Option<()>, // TODO: ??

    /// The left, right, top, and bottom padding for a ninepatch. These are the number of pixels
    /// from the original image edges. Padding allows content placed on top of a ninepatch to be
    /// inset differently from the splits. null if omitted.
    pub pad: Option<()>, // TODO: ??
}

#[derive(Debug, Default, Clone)]
pub struct Rect {
    pub position: Vec2,
    pub size: Vec2,
}

use crate::color::Color;
use bevy_math::Vec2;
use bevy_utils::HashMap;
use std::path::PathBuf;
use strum::{EnumDiscriminants, FromRepr};

#[derive(Debug)]
pub struct AttachmentSlot(pub HashMap<String, Attachment>);

#[derive(Debug)]
pub enum Vertices {
    Weighted { positions: Vec<Vec2> },
    Unweighted { vertices: Vec<BoneInfluence> },
}

#[derive(Debug)]
pub struct BoneInfluence {
    index: usize,
    position: Vec2,
    weight: f32,
}

// The discriminant stuff here is to generate another enum for the attachment type that can be used
// to map an integer back to the enum. It's called `AttachmentType` and has the same discriminant
// values but no data within them.
#[derive(Debug, EnumDiscriminants)]
#[strum_discriminants(name(AttachmentType))]
#[strum_discriminants(derive(FromRepr))]
#[strum_discriminants(vis(pub))]
pub enum Attachment {
    Region(RegionAttachment),
    BoundingBox(BoundingBoxAttachment),
    Mesh(MeshAttachment),
    LinkedMesh(LinkedMeshAttachment),
    Path(PathAttachment),
    Point(PointAttachment),
    Clipping(ClippingAttachment),
}

#[derive(Debug)]
pub struct RegionAttachment {
    pub path: Option<PathBuf>,
    pub position: Vec2,
    pub scale: Vec2,
    pub rotation: f32,
    pub size: Vec2,
    pub color: Color,
}

#[derive(Debug)]
pub struct BoundingBoxAttachment {
    pub vertex_count: u32,
    pub vertices: Vec<f32>,
    pub color: Color,
}

#[derive(Debug)]
pub struct MeshAttachment {
    pub uvs: Vec<u32>,
    pub triangles: Vec<u32>,
    pub vertices: Vec<f32>,
    pub hull: u32,
    pub edges: Option<Vec<u32>>,
    pub width: Option<f32>,
    pub height: Option<f32>,
}

#[derive(Debug)]
pub struct LinkedMeshAttachment {
    pub path: Option<String>,
    pub skin: Option<String>,
    pub parent: Option<String>,
    pub deform: bool,
    pub color: Color,
    pub size: Option<Vec2>,
}

#[derive(Debug)]
pub struct PathAttachment {
    pub closed: bool,
    pub constant_speed: bool,
}

#[derive(Debug)]
pub struct PointAttachment {
    pub position: Vec2,
    // TODO: Complete this...
}

#[derive(Debug)]
pub struct ClippingAttachment {
    /// The index of the slot where clipping stops.
    pub end_slot_index: usize,

    /// The clipping polygon vertices.
    pub vertices: Vertices,

    /// The color of the clipping attachment in Spine. Assume CE3A3AFF RGBA if omitted.
    /// Nonessential.
    pub color: Option<Color>,
}

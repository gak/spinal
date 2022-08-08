use crate::color::Color;
use bevy_math::Vec2;
use bevy_utils::HashMap;
use strum::{EnumDiscriminants, FromRepr};

#[derive(Debug)]
pub struct AttachmentSlot(pub HashMap<String, Attachment>);

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
    path: Option<String>,
    position: Vec2,
    scale: Vec2,
    rotation: f32,
    size: Vec2,
    color: Color,
}

#[derive(Debug)]
pub struct BoundingBoxAttachment {
    vertex_count: u32,
    vertices: Vec<f32>,
    color: Color,
}

#[derive(Debug)]
pub struct MeshAttachment {
    uvs: Vec<u32>,
    triangles: Vec<u32>,
    vertices: Vec<f32>,
    hull: u32,
    edges: Option<Vec<u32>>,
    width: Option<f32>,
    height: Option<f32>,
}

#[derive(Debug)]
pub struct LinkedMeshAttachment {
    path: Option<String>,
    skin: Option<String>,
    parent: Option<String>,
    deform: bool,
    color: Color,
    size: Option<Vec2>,
}

#[derive(Debug)]
pub struct PathAttachment {
    closed: bool,
    constant_speed: bool,
}

#[derive(Debug)]
pub struct PointAttachment {
    pub position: Vec2,
    // TODO: Complete this...
}

#[derive(Debug)]
pub struct ClippingAttachment {
    end: String,
    // TODO: Complete this...
}

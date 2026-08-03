use crate::color::Color;
use bevy_math::Vec2;
use std::path::PathBuf;
use strum::{EnumDiscriminants, FromRepr};

#[derive(Debug)]
pub struct Attachment {
    pub placeholder_name: String,
    pub attachment_name: String,
    pub data: AttachmentData,
}

// The discriminant stuff here is to generate another enum for the attachment type that can be used
// to map an integer back to the enum. It's called `AttachmentType` and has the same discriminant
// values but no data within them.
#[derive(Debug, EnumDiscriminants)]
#[strum_discriminants(name(AttachmentType))]
#[strum_discriminants(derive(FromRepr))]
#[strum_discriminants(vis(pub))]
pub enum AttachmentData {
    Region(RegionAttachment),
    BoundingBox(BoundingBoxAttachment),
    Mesh(MeshAttachment),
    LinkedMesh(LinkedMeshAttachment),
    Path(PathAttachment),
    Point(PointAttachment),
    Clipping(ClippingAttachment),
}

/// A textured rectangle.
#[derive(Debug)]
pub struct RegionAttachment {
    pub path: Option<PathBuf>,
    pub position: Vec2,
    pub scale: Vec2,
    pub rotation: f32,
    pub size: Vec2,
    pub color: Color,
    // TODO: Sequence?
}

/// A polygon used for hit detection, physics, etc.
#[derive(Debug)]
pub struct BoundingBoxAttachment {
    pub vertices: Vertices,

    /// The color of the bounding box in Spine. Assume 60F000FF RGBA if omitted. Nonessential.
    pub color: Color,
}

/// A textured mesh whose vertices may be influenced by multiple bones using weights.
#[derive(Debug)]
pub struct MeshAttachment {
    /// If not `None`, this value is used instead of the attachment name to look up the texture
    /// region.
    pub path_string: usize,

    /// The color to tint the attachment.
    pub color: Color,

    /// The texture coordinate for the vertex.
    pub uvs: Vec<Vec2>,

    /// The index of the vertex for each point.
    pub vertex_index: Vec<usize>,

    /// The mesh vertices.
    pub vertices: Vertices,

    /// The number of vertices that make up the polygon hull. The hull vertices are always first
    /// in the vertices list.
    pub hull_count: usize,

    /// The index of the edges between connected vertices. Nonessential.
    pub edges: Option<Vec<usize>>,

    /// The size of the image used by the mesh. Nonessential.
    pub size: Option<Vec2>,
}

/// A mesh which shares the UVs, vertices, and weights of another mesh.
#[derive(Debug)]
pub struct LinkedMeshAttachment {
    pub path: Option<String>,
    pub skin: Option<String>,
    pub parent: Option<String>,
    pub deform: bool,
    pub color: Color,
    pub size: Option<Vec2>,
}

/// A cubic spline, often used for moving bones along a path.
#[derive(Debug)]
pub struct PathAttachment {
    pub closed: bool,
    pub constant_speed: bool,
}

/// A single point and a rotation, often used for spawning projectiles or particles.
#[derive(Debug)]
pub struct PointAttachment {
    pub rotation: f32,
    pub position: Vec2,
    pub color: Option<Color>,
}

/// A polygon used to clip drawing of other attachments.
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

#[derive(Debug)]
pub enum Vertices {
    Positions { positions: Vec<Vec2> },
    BoneInfluenced { vertices: Vec<Vec<BoneInfluence>> },
}

#[derive(Debug)]
pub struct BoneInfluence {
    pub index: usize,
    pub position: Vec2,
    pub weight: f32,
}

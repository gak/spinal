use crate::color::Color;
use bevy_math::Vec2;
use bevy_utils::HashMap;

#[derive(Debug)]
pub struct Attachment(pub HashMap<String, SubAttachment>);

#[derive(Debug)]
pub enum SubAttachment {
    Mesh(Mesh),
    LinkedMesh(LinkedMesh),
    BoundingBox(BoundingBox),
    Path(Path),
    Point(Point),
    Clipping(Clipping),
    Region(Region),
}

#[derive(Debug)]
pub struct Region {
    path: Option<String>,
    position: Vec2,
    scale: Vec2,
    rotation: f32,
    size: Vec2,
    color: Color,
}

#[derive(Debug)]
pub struct Mesh {
    uvs: Vec<u32>,
    triangles: Vec<u32>,
    vertices: Vec<f32>,
    hull: u32,
    edges: Option<Vec<u32>>,
    width: Option<f32>,
    height: Option<f32>,
}

#[derive(Debug)]
pub struct LinkedMesh {
    path: Option<String>,
    skin: Option<String>,
    parent: Option<String>,
    deform: bool,
    color: Color,
    size: Option<Vec2>,
}

#[derive(Debug)]
pub struct BoundingBox {
    vertex_count: u32,
    vertices: Vec<f32>,
    color: Color,
}

#[derive(Debug)]
pub struct Path {
    closed: bool,
    constant_speed: bool,
}

#[derive(Debug)]
pub struct Point {
    pub position: Vec2,
    // TODO: Complete this...
}

#[derive(Debug)]
pub struct Clipping {
    end: String,
    // TODO: Complete this...
}

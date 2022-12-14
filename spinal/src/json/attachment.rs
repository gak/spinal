use super::{default_true, f32_one};
use crate::color::Color;
use bevy_utils::HashMap;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct JsonAttachment(pub HashMap<String, SubAttachment>);

/// This is a hack because of the optional tag for `Region`.
///
/// See https://github.com/serde-rs/serde/issues/1799
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum SubAttachment {
    Tagged(TaggedSubAttachment),
    Region(Region),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TaggedSubAttachment {
    Mesh(Mesh),
    LinkedMesh(LinkedMesh),
    BoundingBox(BoundingBox),
    Path(Path),
    Point(Point),
    Clipping(Clipping),
    Region(Region),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Region {
    path: Option<String>,
    #[serde(default)]
    x: f32,
    #[serde(default)]
    y: f32,
    #[serde(default = "super::f32_one")]
    scale_x: f32,
    #[serde(default = "super::f32_one")]
    scale_y: f32,
    #[serde(default)]
    rotation: f32,
    width: f32,
    height: f32,
    #[serde(default = "super::white")]
    color: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mesh {
    uvs: Vec<u32>,
    triangles: Vec<u32>,
    vertices: Vec<f32>,
    hull: u32,
    edges: Option<Vec<u32>>,
    width: Option<f32>,
    height: Option<f32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedMesh {
    path: Option<String>,
    skin: Option<String>,
    parent: Option<String>,
    #[serde(default = "super::default_true")]
    deform: bool,
    #[serde(default = "super::white")]
    color: String,
    width: Option<f32>,
    height: Option<f32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundingBox {
    vertex_count: u32,
    vertices: Vec<f32>,
    #[serde(default = "super::bounding_box_color")]
    color: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Path {
    #[serde(default)]
    closed: bool,

    #[serde(default = "super::default_true")]
    constant_speed: bool,
    // TODO: Complete this...
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    #[serde(default)]
    pub x: f32,

    #[serde(default)]
    pub y: f32,
    // TODO: Complete this...
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Clipping {
    end: String,
    // TODO: Complete this...
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tagged() {
        let j = r#"
            {
                "type": "boundingbox",
                "vertexCount": 6,
                "vertices": [ -19.14, -70.3, 40.8, -118.08, 257.78, -115.62, 285.17, 57.18, 120.77, 164.95, -5.07, 76.95 ]
            }
        "#;

        let attachment = serde_json::from_str::<SubAttachment>(j).unwrap();
        dbg!(&attachment);
        if let SubAttachment::Tagged(TaggedSubAttachment::BoundingBox(r)) = attachment {
            assert_eq!(r.vertex_count, 6);
            assert_eq!(
                r.vertices,
                vec![
                    -19.14, -70.3, 40.8, -118.08, 257.78, -115.62, 285.17, 57.18, 120.77, 164.95,
                    -5.07, 76.95
                ]
            );
        } else {
            panic!("Expected BoundingBox {:?}", attachment);
        }
    }

    #[test]
    fn untagged() {
        let j = r#"
		    { "x": 58.29, "y": -2.75, "rotation": 92.37, "width": 75, "height": 178 }
        "#;

        let attachment = serde_json::from_str::<SubAttachment>(j).unwrap();
        dbg!(&attachment);
        if let SubAttachment::Region(r) = attachment {
            assert_eq!(r.x, 58.29);
            assert_eq!(r.y, -2.75);
            assert_eq!(r.rotation, 92.37);
            assert_eq!(r.width, 75.);
            assert_eq!(r.height, 178.);
        } else {
            panic!("Expected Region {:?}", attachment);
        }
    }
}

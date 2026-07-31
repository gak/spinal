use std::ops::Range;

use glam::Vec2;

use crate::{AtlasRegionId, AttachmentRef, BoneId, Rgba8, SkeletonAsset};

#[derive(Debug)]
pub(crate) struct MeshAttachmentData {
    pub(crate) colour: Rgba8,
    pub(crate) atlas_region: u32,
    pub(crate) geometry: u32,
    pub(crate) source_mesh: Option<u32>,
    pub(crate) inherits_deform: bool,
}

#[derive(Debug)]
pub(crate) struct MeshGeometryData {
    pub(crate) uvs: Box<[Vec2]>,
    pub(crate) triangles: Box<[u32]>,
    pub(crate) vertices: MeshVerticesData,
    pub(crate) hull: u32,
}

#[derive(Debug)]
pub(crate) enum MeshVerticesData {
    Unweighted(Box<[Vec2]>),
    Weighted {
        vertices: Box<[Range<u32>]>,
        influences: Box<[MeshInfluenceData]>,
    },
}

impl MeshVerticesData {
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Unweighted(vertices) => vertices.len(),
            Self::Weighted { vertices, .. } => vertices.len(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MeshInfluenceData {
    pub(crate) bone: u32,
    pub(crate) bind_position: Vec2,
    pub(crate) weight: f32,
}

/// A typed borrowed view of one indexed textured mesh attachment.
#[derive(Clone, Copy, Debug)]
pub struct MeshAttachmentRef<'a> {
    attachment: AttachmentRef<'a>,
}

impl<'a> MeshAttachmentRef<'a> {
    pub(crate) const fn new(attachment: AttachmentRef<'a>) -> Self {
        Self { attachment }
    }

    /// Returns the attachment that owns this mesh payload.
    #[must_use]
    pub const fn attachment(self) -> AttachmentRef<'a> {
        self.attachment
    }

    /// Returns the authored light colour.
    #[must_use]
    pub fn color(self) -> Rgba8 {
        self.data().colour
    }

    /// Returns the linked atlas region.
    #[must_use]
    pub fn atlas_region(self) -> AtlasRegionId {
        AtlasRegionId::new(self.attachment.asset.key, self.data().atlas_region)
    }

    /// Returns whether vertices use authored multi-bone influences.
    #[must_use]
    pub fn is_weighted(self) -> bool {
        matches!(self.geometry().vertices, MeshVerticesData::Weighted { .. })
    }

    /// Returns the number of vertices in source order.
    #[must_use]
    pub fn vertex_count(self) -> usize {
        self.geometry().vertices.len()
    }

    /// Returns one vertex by source-order index.
    #[must_use]
    pub fn vertex(self, index: usize) -> Option<MeshVertexRef<'a>> {
        (index < self.vertex_count()).then_some(MeshVertexRef { mesh: self, index })
    }

    /// Iterates vertices in source order without allocating.
    pub fn vertices(
        self,
    ) -> impl DoubleEndedIterator<Item = MeshVertexRef<'a>> + ExactSizeIterator + 'a {
        (0..self.vertex_count()).map(move |index| MeshVertexRef { mesh: self, index })
    }

    /// Returns normalized image-space texture coordinates in vertex order.
    ///
    /// Spine mesh UVs use the source image's top-left origin. Renderer-ready
    /// page-space UVs are available from [`crate::MeshDrawItemRef`].
    #[must_use]
    pub fn uvs(self) -> &'a [Vec2] {
        &self.geometry().uvs
    }

    /// Returns authored triangle indices in draw order.
    #[must_use]
    pub fn triangles(self) -> &'a [u32] {
        &self.geometry().triangles
    }

    /// Returns the number of leading vertices that make up the outer hull.
    #[must_use]
    pub fn hull(self) -> u32 {
        self.geometry().hull
    }

    /// Returns the direct source mesh when this is a linked mesh.
    #[must_use]
    pub fn source_mesh(self) -> Option<MeshAttachmentRef<'a>> {
        self.data().source_mesh.map(|index| {
            MeshAttachmentRef::new(AttachmentRef::from_ordinal(
                self.attachment.asset,
                index as usize,
            ))
        })
    }

    /// Returns whether a linked mesh inherits its source deform timelines.
    ///
    /// The flag is retained even when deform timelines are outside the active
    /// runtime profile.
    #[must_use]
    pub fn inherits_deform(self) -> bool {
        self.data().inherits_deform
    }

    fn data(self) -> &'a MeshAttachmentData {
        self.attachment
            .mesh()
            .expect("MeshAttachmentRef is constructed only for mesh attachments")
    }

    fn geometry(self) -> &'a MeshGeometryData {
        self.attachment
            .asset
            .mesh_geometry_data(self.data().geometry as usize)
    }
}

/// A borrowed source vertex from an indexed mesh attachment.
#[derive(Clone, Copy, Debug)]
pub struct MeshVertexRef<'a> {
    mesh: MeshAttachmentRef<'a>,
    index: usize,
}

impl<'a> MeshVertexRef<'a> {
    /// Returns the attachment containing this vertex.
    #[must_use]
    pub const fn mesh(self) -> MeshAttachmentRef<'a> {
        self.mesh
    }

    /// Returns the source-order vertex index.
    #[must_use]
    pub const fn ordinal(self) -> usize {
        self.index
    }

    /// Returns the slot-bone-local position for an unweighted vertex.
    ///
    /// Weighted vertices instead expose one or more bind positions through
    /// [`Self::influences`].
    #[must_use]
    pub fn local_position(self) -> Option<Vec2> {
        match &self.mesh.geometry().vertices {
            MeshVerticesData::Unweighted(vertices) => Some(vertices[self.index]),
            MeshVerticesData::Weighted { .. } => None,
        }
    }

    /// Iterates authored bone influences for a weighted vertex.
    ///
    /// The iterator is empty for an unweighted vertex.
    pub fn influences(
        self,
    ) -> impl DoubleEndedIterator<Item = MeshInfluenceRef<'a>> + ExactSizeIterator + 'a {
        let (range, influences) = match &self.mesh.geometry().vertices {
            MeshVerticesData::Unweighted(_) => (0..0, &[][..]),
            MeshVerticesData::Weighted {
                vertices,
                influences,
            } => (vertices[self.index].clone(), influences.as_ref()),
        };
        range.map(move |index| MeshInfluenceRef {
            asset: self.mesh.attachment.asset,
            data: influences[index as usize],
        })
    }
}

/// One authored bone influence for a weighted mesh vertex.
#[derive(Clone, Copy, Debug)]
pub struct MeshInfluenceRef<'a> {
    asset: &'a SkeletonAsset,
    data: MeshInfluenceData,
}

impl MeshInfluenceRef<'_> {
    /// Returns the influencing bone.
    #[must_use]
    pub fn bone(self) -> BoneId {
        BoneId::new(self.asset.key, self.data.bone)
    }

    /// Returns the bind position in the influencing bone's local space.
    #[must_use]
    pub const fn bind_position(self) -> Vec2 {
        self.data.bind_position
    }

    /// Returns the authored linear-blend weight.
    #[must_use]
    pub const fn weight(self) -> f32 {
        self.data.weight
    }
}

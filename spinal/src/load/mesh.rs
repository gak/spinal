use std::{collections::HashMap, ops::Range};

use glam::Vec2;

use crate::{
    asset::{AtlasRegionData, AttachmentData, AttachmentDataKind},
    json::{JsonMember, JsonValue},
    mesh::{MeshGeometryData, MeshInfluenceData, MeshVerticesData},
};

use super::{
    LoadError, LoadErrorKind,
    schema::{
        array, error, finite_f32, index_pointer, pointer, required_member, schema_error, u32_value,
    },
};

#[derive(Clone, Debug)]
pub(super) struct PendingLinkedMesh {
    pub(super) attachment: u32,
    pub(super) source_skin: Box<str>,
    pub(super) parent: Box<str>,
    pub(super) path: Box<str>,
}

pub(super) fn resolve_attachment_atlas_region(
    atlas: &HashMap<Box<str>, Vec<u32>>,
    atlas_regions: &[AtlasRegionData],
    lookup_name: &str,
    attachment_kind: &str,
    path: &str,
) -> Result<u32, LoadError> {
    let matches = atlas.get(lookup_name).map_or(&[][..], Vec::as_slice);
    let atlas_region = match matches {
        [] => {
            return Err(error(
                LoadErrorKind::MissingAtlasRegion,
                path,
                format!("{attachment_kind} attachment requires atlas region {lookup_name:?}"),
            ));
        }
        [index] => *index,
        _ => {
            return Err(error(
                LoadErrorKind::AmbiguousAtlasRegion,
                path,
                format!(
                    "{attachment_kind} attachment {lookup_name:?} matches multiple atlas regions"
                ),
            ));
        }
    };
    if atlas_regions.get(atlas_region as usize).is_none() {
        return Err(schema_error(
            path,
            "atlas lookup produced an invalid region index",
        ));
    }
    Ok(atlas_region)
}

pub(super) fn parse_mesh_geometry(
    attachment: &[JsonMember],
    path: &str,
    bone_count: usize,
) -> Result<MeshGeometryData, LoadError> {
    let uvs_path = pointer(path, "uvs");
    let uv_values = array(required_member(attachment, "uvs", path)?, &uvs_path)?;
    if uv_values.len() < 6 || uv_values.len() % 2 != 0 {
        return Err(schema_error(
            &uvs_path,
            "mesh UVs must contain an even number of components for at least three vertices",
        ));
    }
    let mut uvs = Vec::with_capacity(uv_values.len() / 2);
    for (index, pair) in uv_values.chunks_exact(2).enumerate() {
        let component = index * 2;
        uvs.push(Vec2::new(
            finite_f32(&pair[0], &index_pointer(&uvs_path, component))?,
            finite_f32(&pair[1], &index_pointer(&uvs_path, component + 1))?,
        ));
    }
    let vertex_count = uvs.len();

    let triangles_path = pointer(path, "triangles");
    let triangle_values = array(
        required_member(attachment, "triangles", path)?,
        &triangles_path,
    )?;
    if triangle_values.is_empty() || triangle_values.len() % 3 != 0 {
        return Err(schema_error(
            &triangles_path,
            "mesh triangles must contain one or more complete index triples",
        ));
    }
    let mut triangles = Vec::with_capacity(triangle_values.len());
    for (index, value) in triangle_values.iter().enumerate() {
        let index_path = index_pointer(&triangles_path, index);
        let vertex = u32_value(value, &index_path)?;
        if vertex as usize >= vertex_count {
            return Err(schema_error(
                &index_path,
                format!("mesh triangle index {vertex} is outside {vertex_count} vertices"),
            ));
        }
        triangles.push(vertex);
    }

    let vertices_path = pointer(path, "vertices");
    let vertex_values = array(
        required_member(attachment, "vertices", path)?,
        &vertices_path,
    )?;
    let vertices = if vertex_values.len() == uv_values.len() {
        let mut positions = Vec::with_capacity(vertex_count);
        for (index, pair) in vertex_values.chunks_exact(2).enumerate() {
            let component = index * 2;
            positions.push(Vec2::new(
                finite_f32(&pair[0], &index_pointer(&vertices_path, component))?,
                finite_f32(&pair[1], &index_pointer(&vertices_path, component + 1))?,
            ));
        }
        MeshVerticesData::Unweighted(positions.into_boxed_slice())
    } else if vertex_values.len() > uv_values.len() {
        parse_weighted_vertices(vertex_values, &vertices_path, vertex_count, bone_count)?
    } else {
        return Err(schema_error(
            &vertices_path,
            format!(
                "mesh has {} vertex values for {vertex_count} UV vertices",
                vertex_values.len()
            ),
        ));
    };

    let hull_path = pointer(path, "hull");
    let hull = u32_value(required_member(attachment, "hull", path)?, &hull_path)?;
    if hull as usize > vertex_count {
        return Err(schema_error(
            &hull_path,
            format!("mesh hull {hull} exceeds {vertex_count} vertices"),
        ));
    }

    Ok(MeshGeometryData {
        uvs: uvs.into_boxed_slice(),
        triangles: triangles.into_boxed_slice(),
        vertices,
        hull,
    })
}

fn parse_weighted_vertices(
    values: &[JsonValue],
    path: &str,
    vertex_count: usize,
    bone_count: usize,
) -> Result<MeshVerticesData, LoadError> {
    let mut cursor = 0;
    let mut vertices = Vec::with_capacity(vertex_count);
    let mut influences = Vec::new();
    for vertex_index in 0..vertex_count {
        let count_path = index_pointer(path, cursor);
        let Some(count_value) = values.get(cursor) else {
            return Err(schema_error(
                &count_path,
                format!("weighted vertex {vertex_index} is missing its bone count"),
            ));
        };
        let influence_count = u32_value(count_value, &count_path)?;
        if influence_count == 0 {
            return Err(schema_error(
                &count_path,
                "weighted mesh vertices must have at least one bone influence",
            ));
        }
        cursor += 1;
        let needed = usize::try_from(influence_count)
            .ok()
            .and_then(|count| count.checked_mul(4))
            .and_then(|components| cursor.checked_add(components));
        if needed.is_none_or(|end| end > values.len()) {
            return Err(schema_error(
                &count_path,
                format!(
                    "weighted vertex {vertex_index} declares {influence_count} influences but its stream is truncated"
                ),
            ));
        }

        let start = index_u32(influences.len(), path)?;
        let mut weight_sum = 0.0_f64;
        for _influence_index in 0..influence_count {
            let bone_path = index_pointer(path, cursor);
            let bone = u32_value(&values[cursor], &bone_path)?;
            if bone as usize >= bone_count {
                return Err(schema_error(
                    &bone_path,
                    format!("weighted mesh bone index {bone} is outside {bone_count} bones"),
                ));
            }
            let x = finite_f32(&values[cursor + 1], &index_pointer(path, cursor + 1))?;
            let y = finite_f32(&values[cursor + 2], &index_pointer(path, cursor + 2))?;
            let weight_path = index_pointer(path, cursor + 3);
            let weight = finite_f32(&values[cursor + 3], &weight_path)?;
            if !(0.0..=1.0).contains(&weight) {
                return Err(schema_error(
                    &weight_path,
                    "mesh influence weight must be between zero and one",
                ));
            }
            weight_sum += f64::from(weight);
            influences.push(MeshInfluenceData {
                bone,
                bind_position: Vec2::new(x, y),
                weight,
            });
            cursor += 4;
        }
        if (weight_sum - 1.0).abs() > 1.0e-3 {
            return Err(schema_error(
                &count_path,
                format!("weighted vertex {vertex_index} weights sum to {weight_sum}, not one"),
            ));
        }
        let end = index_u32(influences.len(), path)?;
        vertices.push(Range { start, end });
    }
    if cursor != values.len() {
        return Err(schema_error(
            &index_pointer(path, cursor),
            format!(
                "weighted mesh stream has {} trailing values after {vertex_count} vertices",
                values.len() - cursor
            ),
        ));
    }
    Ok(MeshVerticesData::Weighted {
        vertices: vertices.into_boxed_slice(),
        influences: influences.into_boxed_slice(),
    })
}

pub(super) fn resolve_linked_meshes(
    attachments: &mut [AttachmentData],
    skins: &HashMap<Box<str>, u32>,
    pending: &[PendingLinkedMesh],
) -> Result<(), LoadError> {
    let pending_by_attachment = pending
        .iter()
        .map(|link| (link.attachment, link.clone()))
        .collect::<HashMap<_, _>>();
    let mut states = vec![0_u8; attachments.len()];
    for link in pending {
        resolve_linked_mesh(
            link.attachment,
            attachments,
            skins,
            &pending_by_attachment,
            &mut states,
        )?;
    }
    Ok(())
}

fn resolve_linked_mesh(
    index: u32,
    attachments: &mut [AttachmentData],
    skins: &HashMap<Box<str>, u32>,
    pending: &HashMap<u32, PendingLinkedMesh>,
    states: &mut [u8],
) -> Result<(), LoadError> {
    let index_usize = index as usize;
    match states[index_usize] {
        2 => return Ok(()),
        1 => {
            let path = pending
                .get(&index)
                .map_or("/skins", |link| link.path.as_ref());
            return Err(error(
                LoadErrorKind::UnresolvedReference,
                &pointer(path, "parent"),
                "linked mesh parent cycle",
            ));
        }
        _ => {}
    }
    states[index_usize] = 1;
    let link = pending
        .get(&index)
        .expect("only pending linked meshes are resolved")
        .clone();
    let source_skin = skins
        .get(link.source_skin.as_ref())
        .copied()
        .ok_or_else(|| {
            error(
                LoadErrorKind::UnresolvedReference,
                &pointer(&link.path, "skin"),
                format!(
                    "linked mesh source skin {:?} does not exist",
                    link.source_skin
                ),
            )
        })?;
    let slot = attachments[index_usize].slot;
    let sources = attachments
        .iter()
        .enumerate()
        .filter(|(_source_index, attachment)| {
            attachment.skin == source_skin
                && attachment.slot == slot
                && attachment.placeholder_name.as_ref() == link.parent.as_ref()
                && matches!(attachment.kind, AttachmentDataKind::Mesh(_))
        })
        .map(|(source_index, _attachment)| source_index as u32)
        .collect::<Vec<_>>();
    let source = match sources.as_slice() {
        [source] => *source,
        [] => {
            return Err(error(
                LoadErrorKind::UnresolvedReference,
                &pointer(&link.path, "parent"),
                format!(
                    "linked mesh parent {:?} does not exist in skin {:?} under the same slot",
                    link.parent, link.source_skin
                ),
            ));
        }
        _ => {
            return Err(error(
                LoadErrorKind::UnresolvedReference,
                &pointer(&link.path, "parent"),
                format!(
                    "linked mesh parent {:?} is ambiguous in skin {:?}",
                    link.parent, link.source_skin
                ),
            ));
        }
    };
    if pending.contains_key(&source) {
        resolve_linked_mesh(source, attachments, skins, pending, states)?;
    }
    let geometry = match &attachments[source as usize].kind {
        AttachmentDataKind::Mesh(mesh) if mesh.geometry != u32::MAX => mesh.geometry,
        _ => {
            return Err(error(
                LoadErrorKind::UnresolvedReference,
                &pointer(&link.path, "parent"),
                "linked mesh parent did not resolve to mesh geometry",
            ));
        }
    };
    let AttachmentDataKind::Mesh(mesh) = &mut attachments[index_usize].kind else {
        unreachable!("a pending linked mesh retains a mesh sentinel")
    };
    mesh.geometry = geometry;
    mesh.source_mesh = Some(source);
    states[index_usize] = 2;
    Ok(())
}

fn index_u32(index: usize, path: &str) -> Result<u32, LoadError> {
    u32::try_from(index).map_err(|_error| {
        error(
            LoadErrorKind::CapacityExceeded,
            path,
            "table index exceeds the asset-scoped ID representation",
        )
    })
}

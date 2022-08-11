use super::{boolean, col, col_opt, float, str, varint, varint_usize, vec2, BinaryParser};
use crate::binary::seq;
use crate::color::Color;
use crate::skeleton::{
    Attachment, AttachmentData, AttachmentType, BoneInfluence, BoundingBoxAttachment,
    ClippingAttachment, MeshAttachment, PointAttachment, RegionAttachment, Skin, SkinSlot,
    Vertices,
};
use bevy_utils::{tracing, HashMap};
use nom::multi::{count, length_count};
use nom::number::complete::{be_u16, be_u8};
use nom::sequence::tuple;
use nom::IResult;
use tracing::{instrument, trace, trace_span, warn};

impl BinaryParser {
    #[instrument(skip(self, b))]
    pub(crate) fn skins<'a>(&self, b: &'a [u8]) -> IResult<&'a [u8], Vec<Skin>> {
        let mut skins = Vec::new();

        let (b, default_skin) = self.skin(true)(b)?;
        skins.push(default_skin);

        let (b, mut extra_skins) = length_count(varint, self.skin(false))(b)?;
        skins.append(&mut extra_skins);

        Ok((b, skins))
    }

    fn skin(&self, is_default: bool) -> impl FnMut(&[u8]) -> IResult<&[u8], Skin> + '_ {
        move |b: &[u8]| {
            let _span = trace_span!("skin").entered();
            trace!(?is_default, "--> Skin");

            let mut skin = Skin::default();

            let (b, slot_count) = if is_default {
                let (b, slot_count) = varint_usize(b)?;
                skin.name = "default".to_string();
                (b, slot_count)
            } else {
                let (b, (name, bones, ik, transforms, paths, slot_count)) = tuple((
                    str,
                    length_count(varint, varint_usize),
                    length_count(varint, varint_usize),
                    length_count(varint, varint_usize),
                    length_count(varint, varint_usize),
                    varint_usize,
                ))(b)?;
                skin.name = name;
                skin.bones = bones;
                skin.ik = ik;
                skin.transforms = transforms;
                skin.paths = paths;

                (b, slot_count)
            };

            let (b, skin_slots) = count(self.skin_slot(), slot_count)(b)?;
            skin.slots = skin_slots;

            trace!(?is_default, "<-- Skin.");
            Ok((b, skin))
        }
    }

    fn skin_slot<'a>(&self) -> impl FnMut(&'a [u8]) -> IResult<&'a [u8], SkinSlot> + '_ {
        |b: &[u8]| {
            // TODO: A cleanup. Updating `b` in this loop makes this code harder to read.
            let (b, slot) = varint_usize(b)?;
            trace!(?slot, slot_name = ?self.skeleton.slots.get(slot).map(|s| &s.name));

            let (b, attachment_count) = varint_usize(b)?;
            trace!(?attachment_count);

            let mut attachments = Vec::with_capacity(attachment_count);
            let mut b = b;
            for _ in 0..attachment_count {
                // TODO: Move this closure into a parser function.
                let attachments_data = self.attachment()(b)?;
                b = attachments_data.0;
                let (attachment_name, attachment_data) = attachments_data.1;
                // trace!(?attachment_name, ?attachment);

                let attachment = Attachment {
                    name: attachment_name.to_string(),
                    data: attachment_data,
                };
                attachments.push(attachment);
            }

            let skin_slot = SkinSlot { slot, attachments };
            Ok((b, skin_slot))
        }
    }

    fn attachment<'a>(&'a self) -> impl FnMut(&[u8]) -> IResult<&[u8], (&'a str, AttachmentData)> {
        |b: &[u8]| {
            let span = trace_span!("attachment").entered();

            // (docs) "placeholder name": The name in the skin under which the attachment will be
            // stored.
            let (b, placeholder_name) = self.str_table()(b).unwrap(); // TODO: error

            // (docs) The attachment name. If null, use the placeholder name. This is unique for the
            // skeleton. For image attachments this is a key used to look up the texture region, for
            // example on disk or in a texture atlas.
            let (b, attachment_name) = self.str_table_opt()(b).unwrap(); // TODO: error
            let attachment_name = attachment_name.unwrap_or(placeholder_name);

            let (b, attachment_type) = attachment_type(b)?;
            trace!(?placeholder_name, ?attachment_name, ?attachment_type);

            let (b, attachment) = match attachment_type {
                AttachmentType::Region => self.region(b)?,
                AttachmentType::BoundingBox => self.bounding_box(b)?,
                AttachmentType::Mesh => self.mesh(b)?,
                AttachmentType::Clipping => self.clipping(b)?,
                AttachmentType::Point => self.point(b)?,
                _ => todo!("{:?}", attachment_type),
            };

            Ok((b, (placeholder_name, attachment)))
        }
    }

    #[instrument(skip(self, b))]
    fn region<'a>(&self, b: &'a [u8]) -> IResult<&'a [u8], AttachmentData> {
        let (b, (path, rotation, position, scale, size, color)) =
            tuple((self.str_table_opt(), float, vec2, vec2, vec2, col_opt))(b)?;
        let color = color.unwrap_or_else(|| Color::white());

        // This is probably sequence. Not documented.
        let (b, _maybe_sequence) = seq(b)?;

        let attachment = AttachmentData::Region(RegionAttachment {
            path: path.map(|v| v.into()), // TODO: error
            position,
            scale,
            rotation,
            size,
            color,
        });
        trace!(?attachment);
        Ok((b, attachment))
    }

    #[instrument(skip(self, b))]
    fn bounding_box<'a>(&self, b: &'a [u8]) -> IResult<&'a [u8], AttachmentData> {
        let (b, vertices_count) = varint_usize(b)?;
        let (b, vertices) = vertices(b, vertices_count)?;

        let mut color = Color::bounding_box_default();
        let b = if self.parse_non_essential {
            let (b, c) = col_opt(b)?;
            if let Some(c) = c {
                color = c;
            }
            b
        } else {
            b
        };

        let attachment = AttachmentData::BoundingBox(BoundingBoxAttachment { vertices, color });
        trace!(?attachment);
        Ok((b, attachment))
    }

    #[instrument(skip(self, b))]
    fn mesh<'a>(&self, b: &'a [u8]) -> IResult<&'a [u8], AttachmentData> {
        let (b, (path_string, color)) = tuple((varint_usize, col))(b)?;

        // The UV count seems to be shared with `vertices()` later on.
        // The docs don't mention this!
        let (b, vertices_count) = varint_usize(b)?;
        trace!(?vertices_count);
        let (b, uvs) = count(vec2, vertices_count)(b)?;
        trace!(?uvs);

        let (b, vertex_index) = length_count(varint, be_u16)(b)?;
        let vertex_index = vertex_index.into_iter().map(|v| v as usize).collect();
        trace!(?vertex_index);

        let (b, vertices) = vertices(b, vertices_count)?;
        trace!(?vertices);

        // (docs) The number of vertices that make up the polygon hull. The hull vertices are
        // always first in the vertices list.
        // TODO: Make a separate array for hull vertices?
        let (b, hull_count) = varint_usize(b)?;
        trace!(?hull_count);

        let mut mesh = MeshAttachment {
            path_string,
            color,
            uvs,
            vertex_index,
            vertices,
            hull_count,
            edges: None,
            size: None,
        };

        let b = match self.parse_non_essential {
            true => {
                // Could this be a non essential flag? No, it's always 0 in spineboy pro.
                // Probably sequence.
                let (b, _) = seq(b)?;

                let (b, edges) = length_count(varint, be_u16)(b)?;
                let edges = edges.into_iter().map(|v| v as usize).collect();
                trace!(?edges);
                let (b, size) = vec2(b)?;
                mesh.edges = Some(edges);
                mesh.size = Some(size);
                b
            }
            false => b,
        };

        let attachment = AttachmentData::Mesh(mesh);
        trace!(?attachment);
        Ok((b, attachment))
    }

    #[instrument(skip(self, b))]
    fn point<'a>(&self, b: &'a [u8]) -> IResult<&'a [u8], AttachmentData> {
        let (b, (rotation, position)) = tuple((float, vec2))(b)?;
        let (b, color) = if self.parse_non_essential {
            col_opt(b)?
        } else {
            (b, None)
        };

        let attachment = AttachmentData::Point(PointAttachment {
            position,
            rotation,
            color,
        });

        trace!(?attachment);
        Ok((b, attachment))
    }

    #[instrument(skip(self, b))]
    fn clipping<'a>(&self, b: &'a [u8]) -> IResult<&'a [u8], AttachmentData> {
        let (b, end_slot_index) = varint_usize(b)?;
        let (b, vertices_count) = varint_usize(b)?;
        let (b, vertices) = vertices(b, vertices_count)?;
        let (b, color) = match self.parse_non_essential {
            true => col_opt(b)?,
            false => (b, None),
        };

        let attachment = AttachmentData::Clipping(ClippingAttachment {
            end_slot_index,
            vertices,
            color,
        });

        trace!(?attachment);
        Ok((b, attachment))
    }
}

fn attachment_type(b: &[u8]) -> IResult<&[u8], AttachmentType> {
    let (b, attachment_type_id) = be_u8(b)?;
    let attachment_type = AttachmentType::from_repr(attachment_type_id as usize);
    Ok((b, attachment_type.unwrap())) // TODO: error
}

#[instrument(skip(b))]
fn vertices(b: &[u8], vertices_count: usize) -> IResult<&[u8], Vertices> {
    let (b, is_influenced) = boolean(b)?;
    trace!(?vertices_count, ?is_influenced);

    if is_influenced {
        // vertices_count (6) doesn't match data in eye-indifferent (4).
        // The JSON only has 4 sets of entries that are length of 2 BoneInfluence.
        // The first 4 are loaded correctly here, but then it overflows to some other data.
        // The other data looks like this:
        /*
        0000:   04 00 08 00  00 00 02 00  02 00 04 00  04 00 06 00   ................
        0010:   00 00 06 42  ba 00 00 42  b2 00 00 04  00 02 00 ff   ...B...B........
        0020:   ff ff ff 04  3f 80 00 00  3f 80 00 00  00 00 00 00   ....?...?.......
         */
        // Looks like this is hull_count in the next varint in the mesh attachment.
        let (b, vertices) = count(bone_vertices, vertices_count)(b)?;
        trace!(?vertices);

        // println!("{}", pretty_hex(&b));

        Ok((b, Vertices::BoneInfluenced { vertices }))
    } else {
        let (b, positions) = count(vec2, vertices_count)(b)?;
        Ok((b, Vertices::Positions { positions }))
    }
}

fn bone_vertices(b: &[u8]) -> IResult<&[u8], Vec<BoneInfluence>> {
    let (b, bones) = length_count(varint, bone_influence)(b)?;
    Ok((b, bones))
}

fn bone_influence(b: &[u8]) -> IResult<&[u8], BoneInfluence> {
    // The docs say this is a float but it is not!
    let (b, index) = varint_usize(b)?;
    let (b, position) = vec2(b)?;
    let (b, weight) = float(b)?;
    Ok((
        b,
        BoneInfluence {
            index,
            position,
            weight,
        },
    ))
}

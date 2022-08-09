use crate::binary::{boolean, col, col_opt, float, str, varint, varint_usize, vec2, BinaryParser};
use crate::color::Color;
use crate::skeleton::{
    Attachment, AttachmentSlot, AttachmentType, ClippingAttachment, RegionAttachment, Skin,
    Vertices,
};
use bevy_utils::HashMap;
use nom::multi::{count, length_count};
use nom::number::complete::be_u8;
use nom::sequence::tuple;
use nom::IResult;

impl BinaryParser {
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

            dbg!(&skin);
            dbg!(slot_count);

            skin.attachments.reserve(slot_count);
            let mut b = b;
            for _ in 0..slot_count {
                // TODO: A cleanup. Updating `b` in this loop makes this code harder to read.
                let slot_name = self.str_table_opt()(b)?;
                b = slot_name.0;
                let slot_name = slot_name.1;

                let attachment_count = varint_usize(b)?;
                b = attachment_count.0;
                let attachment_count = attachment_count.1;

                for _ in 0..attachment_count {
                    let attachments_data = self.attachment()(b)?;
                    b = attachments_data.0;
                    let (attachment_name, attachment) = attachments_data.1;
                    dbg!(attachment_name, &attachment);

                    // skin.attachments
                    //     .entry(slot_name.to_string())
                    //     .or_default()
                    //     .0
                    //     .insert(attachment_name.to_string(), attachment);
                }
            }

            Ok((b, skin))
        }
    }

    fn attachment<'a>(&'a self) -> impl FnMut(&[u8]) -> IResult<&[u8], (&'a str, Attachment)> {
        |b: &[u8]| {
            // (docs) "placeholder name": The name in the skin under which the attachment will be
            // stored.
            let (b, slot_name) = self.str_table()(b).unwrap(); // TODO: error
            dbg!(&slot_name);

            // (docs) The attachment name. If null, use the placeholder name. This is unique for the
            // skeleton. For image attachments this is a key used to look up the texture region, for
            // example on disk or in a texture atlas.
            let (b, name) = self.str_table_opt()(b).unwrap(); // TODO: error
            let name = name.unwrap_or_else(|| slot_name);
            dbg!(&name);

            let (b, attachment_type) = attachment_type(b)?;
            dbg!(&attachment_type);

            let (b, attachment) = match attachment_type {
                AttachmentType::Region => self.region(b)?,
                AttachmentType::Clipping => self.clipping(b)?,
                _ => todo!("{:?}", attachment_type),
            };

            Ok((b, (slot_name, attachment)))
        }
    }

    fn region<'a>(&self, b: &'a [u8]) -> IResult<&'a [u8], Attachment> {
        let (b, (path, rotation, position, scale, size, color)) =
            tuple((self.str_table_opt(), float, vec2, vec2, vec2, col_opt))(b)?;
        let color = color.unwrap_or_else(|| Color::white());

        dbg!(&path, &rotation, &position, &scale, &size, &color);

        Ok((
            b,
            Attachment::Region(RegionAttachment {
                path: path.map(|v| v.into()), // TODO: error
                position,
                scale,
                rotation,
                size,
                color,
            }),
        ))
    }

    fn clipping<'a>(&self, b: &'a [u8]) -> IResult<&'a [u8], Attachment> {
        let (b, (end_slot_index, vertices)) = tuple((varint_usize, vertices))(b)?;
        let (b, color) = match self.parse_non_essential {
            true => col_opt(b)?,
            false => (b, None),
        };
        let attachment = Attachment::Clipping(ClippingAttachment {
            end_slot_index,
            vertices,
            color,
        });
        Ok((b, attachment))
    }
}

fn attachment_type(b: &[u8]) -> IResult<&[u8], AttachmentType> {
    let (b, attachment_type_id) = be_u8(b)?;
    dbg!(attachment_type_id);
    Ok((
        b,
        AttachmentType::from_repr(attachment_type_id as usize).unwrap(),
    )) // TODO: error
}

fn vertices(b: &[u8]) -> IResult<&[u8], Vertices> {
    let (b, vertices_count) = varint_usize(b)?;
    let (b, is_weighted) = boolean(b)?;
    dbg!(vertices_count, is_weighted);
    if is_weighted {
        // length_count(varint, bone_vertices)(b)
        todo!()
    } else {
        let (b, positions) = count(vec2, vertices_count)(b)?;
        Ok((b, Vertices::Weighted { positions }))
    }
}

fn weighted_vertices(b: &[u8]) -> IResult<&[u8], Vertices> {
    let (b, positions) = length_count(varint, vec2)(b)?;
    dbg!(&positions);
    Ok((b, Vertices::Weighted { positions }))
}

fn bone_vertices(b: &[u8]) -> IResult<&[u8], Vertices> {
    todo!()
}

use crate::binary::{boolean, col, col_opt, float, str, varint, varint_usize, vec2, BinaryParser};
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

        let (b, extra_skins_count) = varint_usize(b)?;
        skins.reserve(extra_skins_count + 1);

        let mut b = b;
        for _ in 0..extra_skins_count {
            let r = self.skin(false)(b)?;
            b = r.0;
            skins.push(r.1);
        }

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

            skin.attachments.reserve(slot_count);
            let mut b = b;
            for _ in 0..slot_count {
                let (b, slot_index) = varint_usize(b)?;
                dbg!(slot_index);
                let (b, attachments) = length_count(varint, self.attachment())(b)?;
            }

            Ok((b, skin))
        }
    }

    fn attachment(&self) -> impl FnMut(&[u8]) -> IResult<&[u8], Attachment> + '_ {
        |b: &[u8]| {
            // TODO: with_capacity
            let mut attachments: HashMap<String, AttachmentSlot> = HashMap::new();

            // (docs) "placeholder name": The name in the skin under which the attachment will be
            // stored.
            let (b, slot_name) = self.str_table()(b).unwrap(); // TODO: error
            let slot_name = slot_name.unwrap(); // TODO: error, this is required
            dbg!(&slot_name);

            // (docs) The attachment name. If null, use the placeholder name. This is unique for the
            // skeleton. For image attachments this is a key used to look up the texture region, for
            // example on disk or in a texture atlas.
            let (b, name) = self.str_table()(b).unwrap(); // TODO: error
            let name = name.unwrap_or_else(|| slot_name);
            dbg!(&name);

            let (b, attachment_type) = attachment_type(b)?;
            dbg!(&attachment_type);

            let (b, attachment) = match attachment_type {
                AttachmentType::Region => {
                    let (b, (path, rotation, position, scale, size, color)) =
                        tuple((self.str_table(), float, vec2, vec2, vec2, col))(b)?;

                    dbg!(&path, &rotation, &position, &scale, &size, &color);

                    (
                        b,
                        Attachment::Region(RegionAttachment {
                            path: path.map(|v| v.into()), // TODO: error
                            position,
                            scale,
                            rotation,
                            size,
                            color,
                        }),
                    )
                }
                AttachmentType::Clipping => {
                    // This is a lookup into the slots array.
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
                    (b, attachment)
                }
                _ => todo!(),
            };

            Ok((b, attachment))
        }
    }
}

fn vertices(b: &[u8]) -> IResult<&[u8], Vertices> {
    let (b, vertices_count) = varint_usize(b)?;
    let (b, is_weighted) = boolean(b)?;
    dbg!(vertices_count, is_weighted);
    if !is_weighted {
        let (b, positions) = count(vec2, vertices_count)(b)?;
        Ok((b, Vertices::Weighted { positions }))
    } else {
        // length_count(varint, bone_vertices)(b)
        todo!()
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

fn attachment_type(b: &[u8]) -> IResult<&[u8], AttachmentType> {
    let (b, attachment_type_id) = be_u8(b)?;
    dbg!(attachment_type_id);
    Ok((
        b,
        AttachmentType::from_repr(attachment_type_id as usize).unwrap(),
    )) // TODO: error
}

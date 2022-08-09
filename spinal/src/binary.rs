use crate::color::Color;
use crate::skeleton::{
    Attachment, AttachmentSlot, AttachmentType, Blend, Bone, ClippingAttachment, Ik, Info,
    ParentTransform, Path, PathPositionMode, PathRotateMode, PathSpacingMode, RegionAttachment,
    Skin, Slot, Transform, Vertices,
};
use crate::{Skeleton, SpinalError};
use bevy_math::Vec2;
use bevy_utils::tracing::warn;
use bevy_utils::HashMap;
use nom::bytes::complete::take;
use nom::combinator::{eof, map_res};
use nom::multi::{count, length_count};
use nom::number::complete::{be_f32, be_i8, be_u32, be_u64, be_u8};
use nom::sequence::{pair, tuple};
use nom::IResult;

struct Strings<'a> {
    strings: Vec<&'a str>,
}

impl<'a> Strings<'a> {
    fn new(s: &'a Vec<String>) -> Self {
        let strings = s.iter().map(|s| s.as_str()).collect::<Vec<&str>>();
        Self { strings }
    }

    fn get(&self, idx: usize) -> Result<Option<&'a str>, SpinalError> {
        Ok(match idx {
            0 => None,
            _ => Some(
                &self
                    .strings
                    .get(idx - 1)
                    .ok_or_else(|| SpinalError::InvalidStringIndex(idx))?,
            ),
        })
    }

    fn parse(&'a self) -> impl FnMut(&[u8]) -> IResult<&[u8], Option<&'a str>> + 'a {
        |b: &[u8]| {
            let (b, idx) = varint_usize(b)?;
            let s = self.get(idx).unwrap(); // TODO: error
            Ok((b, s))
        }
    }
    // fn parse(&self, b: &[u8]) -> IResult<&[u8], &str> {
    //     let (b, idx) = varint_usize(b)?;
    //     let s = self.strings.get(idx).unwrap(); // TODO: error
    //     Ok((b, s))
    // }
}

pub fn parse(b: &[u8]) -> Result<Skeleton, SpinalError> {
    let (_, skel) = parser(b).unwrap();
    Ok(skel)
}

pub fn parser(b: &[u8]) -> IResult<&[u8], Skeleton> {
    let (b, info) = info(b)?;
    let (b, strings) = length_count(varint, str)(b)?;
    let strings = Strings::new(&strings);
    let (b, bones) = bones(b)?;
    let (b, slots) = length_count(varint, slot(&strings))(b)?;
    let (b, ik) = length_count(varint, ik)(b)?;
    let (b, transforms) = length_count(varint, transform)(b)?;
    let (b, paths) = length_count(varint, path)(b)?;
    let (b, skins) = skins(&strings)(b)?;

    let skel = Skeleton {
        info,
        bones,
        slots,
        ik,
        transforms,
        paths,
        skins: vec![],
    };

    // TODO: Make sure we're at the end!
    // eof(b)?;

    Ok((b, skel))
}

fn info(b: &[u8]) -> IResult<&[u8], Info> {
    let (b, (hash, version, bottom_left, size, non_essential)) =
        tuple((be_u64, str, vec2, vec2, boolean))(b)?;
    let mut info = Info {
        hash: format!("{:x}", hash),
        version,
        bottom_left,
        size,
        fps: None,
        images: None,
        audio: None,
    };
    let b = if non_essential {
        let (b, (fps, images, audio)) = tuple((float, str, str))(b)?;
        info.fps = Some(fps);
        info.images = Some(images.into());
        info.audio = Some(audio.into());
        b
    } else {
        b
    };

    Ok((b, info))
}

fn bones(b: &[u8]) -> IResult<&[u8], Vec<Bone>> {
    let (b, bone_count) = varint_usize(b)?;
    let mut bones = Vec::with_capacity(bone_count);
    if bone_count == 0 {
        return Ok((b, bones));
    }

    let (mut b, parent) = bone(b, true)?;
    bones.push(parent);

    for _ in 1..bone_count {
        let v = bone(b, false)?;
        b = v.0;
        bones.push(v.1);
    }

    Ok((b, bones))
}

fn bone(b: &[u8], root: bool) -> IResult<&[u8], Bone> {
    let (b, name) = str(b)?;
    let (b, parent) = bone_parent(b, root)?;
    let (b, (rotation, position, scale, shear, length)) =
        tuple((float, vec2, vec2, vec2, float))(b)?;
    let (b, (transform, skin, color)) = tuple((transform_mode, boolean, col))(b)?;

    let bone = Bone {
        name,
        parent,
        rotation,
        position,
        scale,
        shear,
        length,
        transform,
        skin,
        color,
    };
    Ok((b, bone))
}

fn bone_parent(b: &[u8], root: bool) -> IResult<&[u8], Option<usize>> {
    Ok(match root {
        true => (b, None),
        false => {
            let (b, v) = varint_usize(b)?;
            (b, Some(v))
        }
    })
}

fn transform_mode(b: &[u8]) -> IResult<&[u8], ParentTransform> {
    let (b, v) = be_u8(b)?;
    Ok((b, v.into()))
}

fn slot<'a>(strings: &'a Strings) -> impl FnMut(&[u8]) -> IResult<&[u8], Slot> + 'a {
    move |b: &[u8]| {
        let (b, name) = str(b)?;
        let (b, bone) = varint_usize(b)?;
        let (b, color) = col(b)?;
        let (b, dark) = col_opt(b)?;
        let (b, attachment) = varint_usize(b)?;
        let attachment = match attachment {
            0 => None,
            n => Some(
                strings
                    .get(n)
                    .unwrap() // TODO: error
                    .unwrap() // TODO: error on null string
                    .to_string(),
            ),
        };

        let (b, blend) = varint(b)?;
        let blend = blend.try_into().unwrap(); // TODO: error
        let blend = Blend::from_repr(blend).unwrap(); // TODO: error

        let slot = Slot {
            name,
            bone,
            color,
            dark,
            attachment,
            blend,
        };
        Ok((b, slot))
    }
}

fn ik(b: &[u8]) -> IResult<&[u8], Ik> {
    let (b, (name, order, skin)) = tuple((str, varint, boolean))(b)?;
    let (b, bones) = length_count(varint, varint)(b)?;
    assert!(bones.len() == 1 || bones.len() == 2); // TODO: error
    let bones = bones.into_iter().map(|v| v as usize).collect();
    let (b, (target, mix, softness, bend_direction, compress, stretch, uniform)) =
        tuple((varint, float, float, be_i8, boolean, boolean, boolean))(b)?;
    let target = target as usize;
    let bend_positive = match bend_direction {
        -1 => false,
        1 => true,
        _ => panic!("Invalid bend direction"), // TODO: error
    };

    let ik = Ik {
        name,
        order,
        skin,
        bones,
        target,
        mix,
        softness,
        bend_positive,
        compress,
        stretch,
        uniform,
    };
    Ok((b, ik))
}

fn transform(b: &[u8]) -> IResult<&[u8], Transform> {
    let (b, (name, order_index, skin_required, bones, target)) = tuple((
        str,
        varint,
        boolean,
        length_count(varint, varint_usize),
        varint_usize,
    ))(b)?;
    let (b, (local, relative, offset_rotation, offset_distance, offset_scale, offset_shear_y)) =
        tuple((boolean, boolean, float, vec2, vec2, float))(b)?;
    let (b, (rotate_mix, translate_mix, scale_mix, shear_mix_y)) =
        tuple((float, vec2, vec2, float))(b)?;
    let transform = Transform {
        name,
        order_index,
        skin_required,
        bones,
        target,
        local,
        relative,
        offset_rotation,
        offset_distance,
        offset_scale,
        offset_shear_y,
        rotate_mix,
        translate_mix,
        scale_mix,
        shear_mix_y,
    };
    Ok((b, transform))
}

fn path(b: &[u8]) -> IResult<&[u8], Path> {
    let (b, (name, order_index, skin_required, bones, target_slot)) = tuple((
        str,
        varint,
        boolean,
        length_count(varint, varint_usize),
        varint_usize,
    ))(b)?;
    let (b, (position_mode, spacing_mode, rotate_mode)) =
        tuple((varint_usize, varint_usize, varint_usize))(b)?;
    let position_mode = PathPositionMode::from_repr(position_mode).unwrap(); // TODO: error
    let spacing_mode = PathSpacingMode::from_repr(spacing_mode).unwrap(); // TODO: error
    let rotate_mode = PathRotateMode::from_repr(rotate_mode).unwrap(); // TODO: error
    let (b, (offset_rotation, position, spacing, rotate_mix, translate_mix)) =
        tuple((float, float, float, float, float))(b)?;

    let path = Path {
        name,
        order_index,
        skin_required,
        bones,
        target_slot,
        position_mode,
        spacing_mode,
        rotate_mode,
        offset_rotation,
        position,
        spacing,
        rotate_mix,
        translate_mix,
    };
    Ok((b, path))
}

fn skins<'a>(strings: &'a Strings) -> impl FnMut(&[u8]) -> IResult<&[u8], Vec<Skin>> + 'a {
    move |b: &[u8]| {
        let mut skins = Vec::new();
        let (b, default_skin) = skin(strings, true)(b)?;
        skins.push(default_skin);

        let (b, extra_skins_count) = varint_usize(b)?;
        skins.reserve(extra_skins_count + 1);

        let mut b = b;
        for _ in 0..extra_skins_count {
            let r = skin(strings, false)(b)?;
            b = r.0;
            skins.push(r.1);
        }

        Ok((b, skins))
    }
}

fn skin<'a>(
    strings: &'a Strings,
    is_default: bool,
) -> impl FnMut(&[u8]) -> IResult<&[u8], Skin> + 'a {
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
            let (b, attachments) = length_count(varint, attachment(strings))(b)?;
        }

        Ok((b, skin))
    }
}

fn attachment<'a>(strings: &'a Strings) -> impl FnMut(&[u8]) -> IResult<&[u8], Attachment> + 'a {
    move |b: &[u8]| {
        // TODO: with_capacity
        let mut attachments: HashMap<String, AttachmentSlot> = HashMap::new();

        // (docs) "placeholder name": The name in the skin under which the attachment will be
        // stored.
        let (b, slot_name) = strings.parse()(b).unwrap(); // TODO: error
        let slot_name = slot_name.unwrap(); // TODO: error, this is required
        dbg!(&slot_name);

        // (docs) The attachment name. If null, use the placeholder name. This is unique for the
        // skeleton. For image attachments this is a key used to look up the texture region, for
        // example on disk or in a texture atlas.
        let (b, name) = strings.parse()(b).unwrap(); // TODO: error
        let name = name.unwrap_or_else(|| slot_name);
        dbg!(&name);

        let (b, attachment_type) = attachment_type(b)?;
        dbg!(&attachment_type);

        let (b, attachment) = match attachment_type {
            AttachmentType::Region => {
                let (b, (path, rotation, position, scale, size, color)) =
                    tuple((strings.parse(), float, vec2, vec2, vec2, col))(b)?;

                // TODO: position * scale

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
                // TODO: essential
                let (b, color) = col_opt(b)?;

                (
                    b,
                    Attachment::Clipping(ClippingAttachment {
                        end_slot_index,
                        vertices,
                        color,
                    }),
                )
            }
            _ => todo!(),
        };

        Ok((b, attachment))
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

// TODO: I can't find docs on how this works so ignoring this chunk for now.
fn seq(b: &[u8]) -> IResult<&[u8], ()> {
    let (b, is_sequence) = boolean(b)?;
    if !is_sequence {
        return Ok((b, ()));
    }

    warn!("Ignoring sequence in attachment.");
    let (b, _) = tuple((varint, varint, varint))(b)?;
    Ok((b, ()))
}

fn col(b: &[u8]) -> IResult<&[u8], Color> {
    let (b, v) = be_u32(b)?;
    Ok((b, Color(v)))
}

fn col_opt(b: &[u8]) -> IResult<&[u8], Option<Color>> {
    let (b, v) = be_u32(b)?;
    Ok((b, if v == u32::MAX { None } else { Some(Color(v)) }))
}

fn vec2(b: &[u8]) -> IResult<&[u8], Vec2> {
    let (b, (x, y)) = tuple((float, float))(b)?;
    Ok((b, Vec2::new(x, y)))
}

/// A string is a varint+ length followed by zero or more UTF-8 characters.
///
/// If the length is 0, the string is null (which can be considered the same as empty for most
/// purposes). If the length is 1, the string is empty.
///
/// Otherwise, the length is followed by length - 1 bytes.
fn str_opt(bytes: &[u8]) -> IResult<&[u8], Option<String>> {
    let (bytes, strlen) = varint(bytes)?;
    match strlen {
        0 => Ok((bytes, None)),
        1 => Ok((bytes, Some(String::new()))),
        _ => {
            let (bytes, taken) = take(strlen - 1)(bytes)?;
            let s: String = String::from_utf8(taken.to_vec()).unwrap(); // TODO: nom error
            Ok((bytes, Some(s)))
        }
    }
}

fn str(b: &[u8]) -> IResult<&[u8], String> {
    let (b, s) = str_opt(b)?;
    match s {
        Some(s) => Ok((b, s)),
        None => todo!(),
    }
}

fn boolean(b: &[u8]) -> IResult<&[u8], bool> {
    let (b, byte) = be_u8(b)?;
    match byte {
        0 => Ok((b, false)),
        1 => Ok((b, true)),
        _ => todo!(),
    }
}

fn float(b: &[u8]) -> IResult<&[u8], f32> {
    be_f32(b)
}

/// A varint is an int but it is stored as 1 to 5 bytes, depending on the value.
///
/// There are two kinds of varints, varint+ is optimized to take up less space for small positive
/// values and varint- for small negative (and positive) values.
///
/// For each byte in the varint, the MSB is set if there are additional bytes. If the result is
/// optimized for small negative values, it is shifted.
fn varint(b: &[u8]) -> IResult<&[u8], u32> {
    let mut offset = 0;
    let mut value: u32 = 0;
    loop {
        let b = b
            .get(offset)
            .ok_or(nom::Err::Incomplete(nom::Needed::new(1)))?;
        value |= ((b & 0x7F) as u32) << (offset as u32 * 7);
        offset += 1;
        if b & 0x80 == 0 || offset == 5 {
            break;
        }
    }
    return Ok((&b[offset..], value));
}

fn varint_usize(b: &[u8]) -> IResult<&[u8], usize> {
    let (b, v) = varint(b)?;
    Ok((b, v as usize))
}

/// If the lowest bit is set, the value is negative.
fn varint_signed(b: &[u8]) -> IResult<&[u8], i32> {
    let (b, value) = varint(b)?;
    let negative = value & 1 == 1;
    let shifted = (value >> 1) as i32;
    let value = if negative { -(shifted + 1) } else { shifted };
    Ok((b, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser() {
        let b = include_bytes!("../../assets/spineboy-pro-4.1/spineboy-pro.skel");
        let skel = parse(b).unwrap();
        assert_eq!(skel.info.version, "4.1.06".to_string());
        assert_eq!(skel.bones.len(), 67);
        assert_eq!(skel.slots.len(), 52);
        assert_eq!(skel.ik.len(), 7);
        dbg!(skel);
    }

    #[test]
    fn varint_pos() {
        assert_eq!(varint(&[0b00000000]).unwrap().0.len(), 0);
        assert_eq!(varint(&[0b00000000]).unwrap().1, 0);
        assert_eq!(varint(&[0b00000001]).unwrap().1, 1);
        assert_eq!(varint(&[0b01111111]).unwrap().1, 127);
        assert_eq!(varint(&[0b11111111, 0b00000000]).unwrap().1, 127);
        assert_eq!(varint(&[0b11111111, 0b00000001]).unwrap().1, 255);
        let v = varint(&[0b11111111, 0b01111111]).unwrap();
        assert_eq!(v.1, 0x3FFF);
        let v = varint(&[0b11111111, 0b11111111, 0b11111111, 0b01111111]).unwrap();
        assert_eq!(v.1, 0xFFF_FFFF);

        // Leave an extra byte at the end to see if we have a remainder.
        let v = varint(&[0b11111111, 0b11111111, 0b11111111, 0b01111111, 0x42]).unwrap();
        assert_eq!(v.0, &[0x42]);
        assert_eq!(v.1, 0xFFF_FFFF);

        // 5 bytes!
        let v = varint(&[0b11111111, 0b11111111, 0b11111111, 0b11111111, 0b00000001]).unwrap();
        assert_eq!(v.1, 0x1FFF_FFFF);
        let v = varint(&[0b11111111, 0b11111111, 0b11111111, 0b11111111, 0b00000011]).unwrap();
        assert_eq!(v.0.len(), 0);
        assert_eq!(v.1, 0x3FFF_FFFF);
        let v = varint(&[0b11111111, 0b11111111, 0b11111111, 0b11111111, 0b01111111]).unwrap();
        assert_eq!(v.0.len(), 0);
        assert_eq!(v.1, 0xFFFF_FFFF);

        // 5 bytes with overflow.
        let v = varint(&[
            0b11111111, 0b11111111, 0b11111111, 0b11111111, 0b01111111, 0x42,
        ])
        .unwrap();
        assert_eq!(v.0.len(), 1);
        assert_eq!(v.1, 0xFFFF_FFFF);
    }

    #[test]
    fn varint_neg() {
        assert_eq!(varint_signed(&[0b00000000]).unwrap().1, 0);
        assert_eq!(varint_signed(&[0b00000001]).unwrap().1, -1);
        assert_eq!(varint_signed(&[0b00000010]).unwrap().1, 1);
        assert_eq!(varint_signed(&[0b00000011]).unwrap().1, -2);
        assert_eq!(varint_signed(&[0b01111110]).unwrap().1, 63);
        assert_eq!(varint_signed(&[0b01111111]).unwrap().1, -64);
        let v = varint_signed(&[0b11111111, 0b11111111, 0b11111111, 0b01111111]).unwrap();
        assert_eq!(v.1, -0x800_0000);
        let v = varint_signed(&[0b11111110, 0b11111111, 0b11111111, 0b01111111]).unwrap();
        assert_eq!(v.1, 0x7FF_FFFF);
        let v =
            varint_signed(&[0b11111110, 0b11111111, 0b11111111, 0b11111111, 0b11111111]).unwrap();
        assert_eq!(v.1, 0x7FFF_FFFF);
    }
}

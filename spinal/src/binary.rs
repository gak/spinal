use crate::bone::{BoneParent, Color, ParentTransform};
use crate::slot::{Blend, Slot};
use crate::{Bone, Ik, Info, Reference, Skeleton, SpinalError};
use nom::bytes::complete::take;
use nom::combinator::{eof, map_res};
use nom::multi::{count, length_count};
use nom::number::complete::{be_f32, be_i8, be_u32, be_u64, be_u8};
use nom::sequence::{pair, tuple};
use nom::IResult;
use std::io::Read;

pub fn parse(b: &[u8]) -> Result<Skeleton, SpinalError> {
    let (_, skel) = parser(b).unwrap();
    Ok(skel)
}

pub fn parser(b: &[u8]) -> IResult<&[u8], Skeleton> {
    let (b, info) = info(b)?;
    let (b, strings) = length_count(varint, str)(b)?;
    let (b, bones) = bones(b)?;
    let (b, slots) = length_count(varint, slot)(b)?;
    let (b, ik) = length_count(varint, ik)(b)?;

    let skel = Skeleton {
        info,
        bones,
        slots,
        ik,
    };

    // TODO: Make sure we're at the end!
    // eof(b)?;

    Ok((b, skel))
}

fn info(b: &[u8]) -> IResult<&[u8], Info> {
    let (b, (hash, version, x, y, width, height, non_essential)) =
        tuple((be_u64, str, float, float, float, float, boolean))(b)?;
    let mut info = Info {
        hash: format!("{:x}", hash),
        version,
        x,
        y,
        width,
        height,
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
    let (b, bone_count) = varint(b)?;
    let bone_count = bone_count as usize;
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
    let (b, (rotation, x, y, scale_x, scale_y, shear_x, shear_y, length)) =
        tuple((float, float, float, float, float, float, float, float))(b)?;
    let (b, (transform, skin, color)) = tuple((transform_mode, boolean, col))(b)?;

    let bone = Bone {
        name,
        parent,
        rotation,
        x,
        y,
        scale_x,
        scale_y,
        shear_x,
        shear_y,
        length,
        transform,
        skin,
        color,
    };
    Ok((b, bone))
}

fn bone_parent(b: &[u8], root: bool) -> IResult<&[u8], BoneParent> {
    Ok(match root {
        true => (b, BoneParent::Root),
        false => {
            let (b, v) = varint(b)?;
            (b, v.into())
        }
    })
}

fn transform_mode(b: &[u8]) -> IResult<&[u8], ParentTransform> {
    let (b, v) = be_u8(b)?;
    Ok((b, v.into()))
}

fn slot(b: &[u8]) -> IResult<&[u8], Slot> {
    let (b, name) = str(b)?;
    let (b, bone) = varint(b)?;
    let bone = Reference::Index(bone as usize);
    let (b, color) = col(b)?;
    if let Color::Number(c) = &color {
        dbg!(&name, format!("{:x}", &c));
    }
    let (b, dark) = col_opt(b)?;
    if let Some(Color::Number(dark)) = &dark {
        dbg!(&name, format!("{:x}", &dark));
    }
    let (b, attachment) = varint(b)?;
    let attachment = match attachment {
        0 => None,
        n => Some(Reference::Index(n as usize)),
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

fn ik(b: &[u8]) -> IResult<&[u8], Ik> {
    let (b, (name, order, skin)) = tuple((str, varint, boolean))(b)?;
    let (b, bones) = length_count(varint, varint)(b)?;
    assert!(bones.len() == 1 || bones.len() == 2); // TODO: error
    let bones = bones
        .into_iter()
        .map(|v| Reference::Index(v as usize))
        .collect();
    let (b, (target, mix, softness, bend_direction, compress, stretch, uniform)) =
        tuple((varint, float, float, be_i8, boolean, boolean, boolean))(b)?;
    let target = Reference::Index(target as usize);
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

fn col(b: &[u8]) -> IResult<&[u8], Color> {
    let (b, v) = be_u32(b)?;
    Ok((b, Color::Number(v)))
}

fn col_opt(b: &[u8]) -> IResult<&[u8], Option<Color>> {
    let (b, v) = be_u32(b)?;
    Ok((
        b,
        if v == u32::MAX {
            dbg!("col skipped");
            None
        } else {
            Some(Color::Number(v))
        },
    ))
}

/// A string is a varint+ length followed by zero or more UTF-8 characters.
///
/// If the length is 0, the string is null (which can be considered the same as empty for most
/// purposes). If the length is 1, the string is empty.
///
/// Otherwise, the length is followed by length - 1 bytes.
fn str_opt(bytes: &[u8]) -> IResult<&[u8], Option<String>> {
    let (bytes, strlen) = varint(bytes)?;
    dbg!(&strlen);
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

fn varint(b: &[u8]) -> IResult<&[u8], u32> {
    let mut offset = 0;
    let mut value: u32 = 0;
    loop {
        let b = b
            .get(offset)
            .ok_or(nom::Err::Incomplete(nom::Needed::new(1)))?;
        value |= ((b & 0x7F) as u32) << (offset as u32 * 7);
        if b & 0x80 == 0 || offset == 3 {
            break;
        }
        offset += 1;
    }
    return Ok((&b[offset + 1..], value));
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
        assert_eq!(v.1, 0xFFFFFFF);

        // Leave an extra byte at the end to see if we have a remainder.
        let v = varint(&[0b11111111, 0b11111111, 0b11111111, 0b11111111, 0x42]).unwrap();
        assert_eq!(v.0, &[0x42]);
        assert_eq!(v.1, 0xFFFFFFF);
    }

    #[test]
    fn varint_neg() {
        assert_eq!(varint_signed(&[0b00000000]).unwrap().1, 0);
        assert_eq!(varint_signed(&[0b00000001]).unwrap().1, -1);
        assert_eq!(varint_signed(&[0b00000010]).unwrap().1, 1);
        assert_eq!(varint_signed(&[0b00000011]).unwrap().1, -2);
        assert_eq!(varint_signed(&[0b01111110]).unwrap().1, 63);
        assert_eq!(varint_signed(&[0b01111111]).unwrap().1, -64);
        let v = varint_signed(&[0b11111111, 0b11111111, 0b11111111, 0b11111111]).unwrap();
        assert_eq!(v.1, -0x800_0000);
        let v = varint_signed(&[0b11111110, 0b11111111, 0b11111111, 0b11111111]).unwrap();
        assert_eq!(v.1, 0x7FF_FFFF);
    }
}

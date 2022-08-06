use crate::bone::{BoneParent, ParentTransform};
use crate::{Bone, Info, Skeleton, SpinalError};
use nom::bytes::complete::take;
use nom::combinator::{eof, map_res};
use nom::multi::{count, length_count};
use nom::number::complete::{be_f32, be_u32, be_u64, be_u8};
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

    let skel = Skeleton { info, bones };

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
    let (b, (transform, skin, color)) = tuple((transform_mode, boolean, be_u32))(b)?;

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
    // let transform = ParentTransform::from_repr(v.into()).unwrap(); // TODO: error
    Ok((b, v.into()))
}

/// A string is a varint+ length followed by zero or more UTF-8 characters.
///
/// If the length is 0, the string is null (which can be considered the same as empty for most
/// purposes). If the length is 1, the string is empty.
///
/// Otherwise, the length is followed by length - 1 bytes.
fn opt_str(bytes: &[u8]) -> IResult<&[u8], Option<String>> {
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
    let (b, s) = opt_str(b)?;
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
        let info = &skel.info;
        assert_eq!(info.version, "4.1.06".to_string());
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

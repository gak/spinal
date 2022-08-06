use crate::{Info, Skeleton, SpinalError};
use nom::bytes::complete::take;
use nom::combinator::map_res;
use nom::number::complete::{be_f32, be_u32, be_u64, be_u8};
use nom::sequence::{pair, tuple};
use nom::IResult;
use std::io::Read;

fn parse_binary(b: &[u8]) -> IResult<&[u8], Skeleton> {
    let (b, info) = info(b)?;
    dbg!(info);
    todo!()
}

fn info(b: &[u8]) -> IResult<&[u8], Info> {
    let (b, (hash, version, x, y, width, height, non_essential)) =
        tuple((hash, str, be_f32, be_f32, be_f32, be_f32, boolean))(b)?;
    dbg!(hash, &version, x, y, width, height, non_essential);
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
    if non_essential {
        let (b, (fps, images, audio)) = tuple((be_f32, str, str))(b)?;
        info.fps = Some(fps);
        info.images = Some(images.into());
        info.audio = Some(audio.into());
    };

    Ok((b, info))
}

/// A string is a varint+ length followed by zero or more UTF-8 characters.
///
/// If the length is 0, the string is null (which can be considered the same as empty for most
/// purposes). If the length is 1, the string is empty.
///
/// Otherwise, the length is followed by length - 1 bytes.
fn opt_str(bytes: &[u8]) -> IResult<&[u8], Option<String>> {
    let (bytes, strlen) = varint_positive(bytes)?;
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

fn hash(b: &[u8]) -> IResult<&[u8], u64> {
    be_u64(b)
}

fn varint_positive(b: &[u8]) -> IResult<&[u8], u32> {
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
fn varint_negative(b: &[u8]) -> IResult<&[u8], i32> {
    let (b, value) = varint_positive(b)?;
    let negative = value & 1 == 1;
    let shifted = (value >> 1) as i32;
    let value = if negative { -(shifted + 1) } else { shifted };
    Ok((b, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse() {
        let b = include_bytes!("../../assets/spineboy-pro-4.1/spineboy-pro.skel");
        parse_binary(b).unwrap();
    }

    #[test]
    fn varint_pos() {
        assert_eq!(varint_positive(&[0b00000000]).unwrap().0.len(), 0);
        assert_eq!(varint_positive(&[0b00000000]).unwrap().1, 0);
        assert_eq!(varint_positive(&[0b00000001]).unwrap().1, 1);
        assert_eq!(varint_positive(&[0b01111111]).unwrap().1, 127);
        assert_eq!(varint_positive(&[0b11111111, 0b00000000]).unwrap().1, 127);
        assert_eq!(varint_positive(&[0b11111111, 0b00000001]).unwrap().1, 255);
        let v = varint_positive(&[0b11111111, 0b01111111]).unwrap();
        assert_eq!(v.1, 0x3FFF);
        let v = varint_positive(&[0b11111111, 0b11111111, 0b11111111, 0b01111111]).unwrap();
        assert_eq!(v.1, 0xFFFFFFF);

        // Leave an extra byte at the end to see if we have a remainder.
        let v = varint_positive(&[0b11111111, 0b11111111, 0b11111111, 0b11111111, 0x42]).unwrap();
        assert_eq!(v.0, &[0x42]);
        assert_eq!(v.1, 0xFFFFFFF);
    }

    #[test]
    fn varint_neg() {
        assert_eq!(varint_negative(&[0b00000000]).unwrap().1, 0);
        assert_eq!(varint_negative(&[0b00000001]).unwrap().1, -1);
        assert_eq!(varint_negative(&[0b00000010]).unwrap().1, 1);
        assert_eq!(varint_negative(&[0b00000011]).unwrap().1, -2);
        assert_eq!(varint_negative(&[0b01111110]).unwrap().1, 63);
        assert_eq!(varint_negative(&[0b01111111]).unwrap().1, -64);
        let v = varint_negative(&[0b11111111, 0b11111111, 0b11111111, 0b11111111]).unwrap();
        assert_eq!(v.1, -0x800_0000);
        let v = varint_negative(&[0b11111110, 0b11111111, 0b11111111, 0b11111111]).unwrap();
        assert_eq!(v.1, 0x7FF_FFFF);
    }
}

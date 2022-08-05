use crate::{Info, Skeleton, SpinalError};
use nom::bytes::complete::take;
use nom::IResult;
use std::io::Read;

fn parse_binary(bytes: &[u8]) -> Result<Skeleton, SpinalError> {
    let (bytes, hash) = opt_str(bytes)?;
    dbg!(hash);
    todo!()
}

/// A string is a varint+ length followed by zero or more UTF-8 characters.
///
/// If the length is 0, the string is null (which can be considered the same as empty for most
/// purposes). If the length is 1, the string is empty.
///
/// Otherwise, the length is followed by length - 1 bytes.
fn opt_str(bytes: &[u8]) -> IResult<&[u8], Option<String>> {
    let (bytes, count) = varint_positive(bytes)?;
    match count {
        0 => Ok((bytes, None)),
        1 => Ok((bytes, Some(String::new()))),
        _ => {
            let (bytes, taken) = take(count - 1)(bytes)?;
            let s: String = String::from_utf8(taken.to_vec()).unwrap(); // TODO: map_err
            Ok((bytes, Some(s)))
        }
    }
}

fn str(bytes: &[u8]) -> IResult<&[u8], String> {
    let (bytes, s) = opt_str(bytes)?;
    match s {
        Some(s) => Ok((bytes, s)),
        None => Err(nom::Err::Error(nom::Context::Code(
            bytes,
            nom::Err::,
        ))),
    }
}

fn varint_positive(bytes: &[u8]) -> IResult<&[u8], u32> {
    let mut offset = 0;
    let mut value: u32 = 0;
    loop {
        let b = bytes
            .get(offset)
            .ok_or(nom::Err::Incomplete(nom::Needed::new(1)))?;
        value |= ((b & 0x7F) as u32) << (offset as u32 * 7);
        if b & 0x80 == 0 || offset == 3 {
            break;
        }
        offset += 1;
    }
    return Ok((&bytes[offset + 1..], value));
}

/// If the lowest bit is set, the value is negative.
fn varint_negative(bytes: &[u8]) -> IResult<&[u8], i32> {
    let (bytes, value) = varint_positive(bytes)?;
    let negative = value & 1 == 1;
    let shifted = (value >> 1) as i32;
    let value = if negative { -(shifted + 1) } else { shifted };
    Ok((bytes, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_pos() {
        assert_eq!(varint_positive(&[0b00000000]).unwrap().1, 0);
        assert_eq!(varint_positive(&[0b00000001]).unwrap().1, 1);
        assert_eq!(varint_positive(&[0b01111111]).unwrap().1, 127);
        assert_eq!(varint_positive(&[0b11111111, 0b00000000]).unwrap().1, 127);
        assert_eq!(varint_positive(&[0b11111111, 0b00000001]).unwrap().1, 255);
        assert_eq!(
            varint_positive(&[0b11111111, 0b01111111]).unwrap().1,
            0x3FFF
        );
        assert_eq!(
            varint_positive(&[0b11111111, 0b11111111, 0b11111111, 0b01111111])
                .unwrap()
                .1,
            0xFFFFFFF
        );
        assert_eq!(
            varint_positive(&[0b11111111, 0b11111111, 0b11111111, 0b11111111])
                .unwrap()
                .1,
            0xFFFFFFF
        );
    }

    #[test]
    fn varint_neg() {
        assert_eq!(varint_negative(&[0b00000000]).unwrap().1, 0);
        assert_eq!(varint_negative(&[0b00000001]).unwrap().1, -1);
        assert_eq!(varint_negative(&[0b00000010]).unwrap().1, 1);
        assert_eq!(varint_negative(&[0b00000011]).unwrap().1, -2);
        assert_eq!(varint_negative(&[0b01111110]).unwrap().1, 63);
        assert_eq!(varint_negative(&[0b01111111]).unwrap().1, -64);
        assert_eq!(
            varint_negative(&[0b11111111, 0b11111111, 0b11111111, 0b11111111])
                .unwrap()
                .1,
            -0x800_0000
        );
        assert_eq!(
            varint_negative(&[0b11111110, 0b11111111, 0b11111111, 0b11111111])
                .unwrap()
                .1,
            0x7FF_FFFF
        );
    }
}

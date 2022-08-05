use std::io::Read;
use nom;
use crate::{Skeleton, SpinalError};

fn parse_binary(bytes: &[u8]) -> Result<Skeleton, SpinalError> {
    todo!()
}

fn varint_positive(bytes: &[u8]) -> nom::IResult<&[u8], u32> {
    println!("--");
    let mut offset = 0;
    let mut value: u32 = 0;
    loop {
        dbg!(offset);
        let b = bytes.get(offset).ok_or(nom::Err::Incomplete(nom::Needed::new(offset + 1)))?;
        dbg!(offset, b);
        value |= ((b & 0x7F) as u32) << (offset as u32 * 7);
        if b & 0x80 == 0 || offset == 3{
            break;
        }
        offset += 1;
    }
    return Ok((&bytes[offset + 1..], value));
}

/// If the lowest bit is set, the value is negative.
fn varint_negative(bytes: &[u8]) -> nom::IResult<&[u8], i32> {
    let (bytes, value) = varint_positive(bytes)?;
    let negative = value & 1 == 1;
    let shifted = (value >> 1) as i32;
    let value = if negative {
        -(shifted + 1)
    } else {
        shifted
    };
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
        assert_eq!(varint_positive(&[0b11111111, 0b01111111]).unwrap().1, 0x3FFF);
        assert_eq!(varint_positive(&[0b11111111, 0b11111111, 0b11111111, 0b01111111]).unwrap().1, 0xFFFFFFF);
        assert_eq!(varint_positive(&[0b11111111, 0b11111111, 0b11111111, 0b11111111]).unwrap().1, 0xFFFFFFF);
    }

    #[test]
    fn varint_neg() {
        assert_eq!(varint_negative(&[0b00000000]).unwrap().1, 0);
        assert_eq!(varint_negative(&[0b00000001]).unwrap().1, -1);
        assert_eq!(varint_negative(&[0b00000010]).unwrap().1, 1);
        assert_eq!(varint_negative(&[0b00000011]).unwrap().1, -2);
        assert_eq!(varint_negative(&[0b01111110]).unwrap().1, 63);
        assert_eq!(varint_negative(&[0b01111111]).unwrap().1, -64);
        assert_eq!(varint_negative(&[0b11111111, 0b11111111, 0b11111111, 0b11111111]).unwrap().1, -0x800_0000);
        assert_eq!(varint_negative(&[0b11111110, 0b11111111, 0b11111111, 0b11111111]).unwrap().1, 0x7FF_FFFF);
    }
}
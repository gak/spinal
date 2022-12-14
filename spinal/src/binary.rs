mod animation;
mod bone;
mod skin;

use crate::binary::bone::bones;
use crate::color::Color;
use crate::skeleton::{
    AttachmentData, AttachmentType, Blend, Bone, ClippingAttachment, Event, Ik, Info,
    ParentTransform, Path, PathPositionMode, PathRotateMode, PathSpacingMode, RegionAttachment,
    Skin, Slot, Transform, Vertices,
};
use crate::{Angle, Skeleton, SpinalError};
use bevy_math::Vec2;
use bevy_utils::tracing::warn;
use nom::bytes::complete::take;
use nom::combinator::{eof, map_res};
use nom::multi::{count, length_count};
use nom::number::complete::{be_f32, be_i8, be_u32, be_u64, be_u8};
use nom::sequence::{pair, tuple};
use nom::IResult;
use tracing::{debug, instrument, trace};

/// Reads a binary skeleton file and returns a [Skeleton].
// If the code path doesn't need any information from [Parser] just use static functions.
pub struct BinarySkeletonParser {
    parse_non_essential: bool,
    skeleton: Skeleton,
}

impl BinarySkeletonParser {
    #[instrument(skip(b))]
    pub fn parse(b: &[u8]) -> Result<Skeleton, SpinalError> {
        debug!(len = ?b.len(), "Parsing binary skeleton.");
        let mut parser = BinarySkeletonParser {
            parse_non_essential: false,
            skeleton: Skeleton::default(),
        };
        let (_, skeleton) = parser.parser(b).unwrap(); // TODO: error
        Ok(skeleton)
    }

    #[instrument(skip(self, b))]
    fn parser(mut self, b: &[u8]) -> IResult<&[u8], Skeleton> {
        let (b, (parse_non_essential, info)) = Self::info(b)?;
        self.skeleton.info = info;
        self.parse_non_essential = parse_non_essential;
        trace!(?parse_non_essential);

        let (b, strings) = length_count(varint, str)(b)?;
        self.skeleton.strings = strings;

        let (b, bones) = bones(b)?;
        self.skeleton.bones = bones;
        self.skeleton.bones_tree = self.skeleton.build_bones_tree();

        let (b, slots) = length_count(varint, self.slot())(b)?;
        self.skeleton.slots = slots;

        let (b, ik) = length_count(varint, ik)(b)?;
        self.skeleton.ik = ik;

        let (b, transforms) = length_count(varint, transform)(b)?;
        self.skeleton.transforms = transforms;

        let (b, paths) = length_count(varint, path)(b)?;
        self.skeleton.paths = paths;

        let (b, skins) = self.skins(b)?;
        self.skeleton.skins = skins;

        let (b, events) = length_count(varint, self.event())(b)?;
        self.skeleton.events = events;

        let (b, animations) = length_count(varint, self.animation())(b)?;
        self.skeleton.animations = animations;
        self.skeleton.animations_by_name = self
            .skeleton
            .animations
            .iter()
            .map(|a| (a.name.clone(), a.clone()))
            .collect();

        // eof(b)?;

        Ok((b, self.skeleton))
    }

    fn info(b: &[u8]) -> IResult<&[u8], (bool, Info)> {
        let (b, (hash, version, bottom_left, size, parse_non_essential)) =
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
        let b = if parse_non_essential {
            let (b, (fps, images, audio)) = tuple((float, str, str))(b)?;
            info.fps = Some(fps);
            info.images = Some(images.into());
            info.audio = Some(audio.into());
            b
        } else {
            b
        };

        Ok((b, (parse_non_essential, info)))
    }

    fn slot(&self) -> impl FnMut(&[u8]) -> IResult<&[u8], Slot> + '_ {
        move |b: &[u8]| {
            let (b, name) = str(b)?;
            let (b, bone) = varint_usize(b)?;
            let (b, color) = col(b)?;
            let (b, dark) = col_opt(b)?;
            let (b, attachment) = self.str_table_opt()(b)?;
            let attachment = attachment.map(|s| s.to_string());

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

    fn event(&self) -> impl FnMut(&[u8]) -> IResult<&[u8], Event> + '_ {
        |b: &[u8]| {
            let (b, name) = self.str_table()(b)?;
            let (b, int_val) = varint_signed(b)?;
            let (b, float_val) = float(b)?;
            let (b, str_val) = str_opt(b)?;
            let (b, audio_path) = str_opt(b)?;

            // Undocumented. Volume and balance is (probably) skipped if audio_path is not set.
            // TODO: Verify this.
            let (b, audio_volume, audio_balance) = if audio_path.is_some() {
                let (b, audio_volume) = float(b)?;
                let (b, audio_balance) = float(b)?;
                (b, audio_volume, audio_balance)
            } else {
                (b, 0., 0.)
            };
            let event = Event {
                name: name.to_string(),
                int: int_val,
                float: float_val,
                string: str_val.map(|s| s.to_string()),
                audio_path,
                audio_volume,
                audio_balance,
            };
            trace!(?event);
            Ok((b, event))
        }
    }

    fn str_lookup_internal(&self, idx: usize) -> Result<Option<&str>, SpinalError> {
        Ok(match idx {
            0 => None,
            _ => Some(
                &self
                    .skeleton
                    .strings
                    .get(idx - 1)
                    .map(|s| s.as_str())
                    .ok_or_else(|| SpinalError::InvalidStringIndex(idx))?,
            ),
        })
    }

    fn str_table_opt<'a>(&'a self) -> impl FnMut(&[u8]) -> IResult<&[u8], Option<&'a str>> {
        |b: &[u8]| {
            let (b, idx) = varint_usize(b)?;
            let s = self.str_lookup_internal(idx).unwrap(); // TODO: error
            Ok((b, s))
        }
    }

    fn str_table<'a>(&'a self) -> impl FnMut(&[u8]) -> IResult<&[u8], &'a str> {
        |b: &[u8]| {
            let (b, opt_str) = self.str_table_opt()(b)?;
            let s = opt_str.unwrap(); // TODO: error handling
            Ok((b, s))
        }
    }
}

fn ik(b: &[u8]) -> IResult<&[u8], Ik> {
    let (b, (name, order, skin)) = tuple((str, varint, boolean))(b)?;
    let (b, bones) = length_count(varint, varint)(b)?;
    assert!(bones.len() == 1 || bones.len() == 2); // TODO: error
    let bones = bones.into_iter().map(|v| v as usize).collect();
    let (b, (target, mix, softness, bend, compress, stretch, uniform)) =
        tuple((varint, float, float, bend, boolean, boolean, boolean))(b)?;
    let target = target as usize;

    let ik = Ik {
        name,
        order,
        skin,
        bones,
        target,
        mix,
        softness,
        bend,
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

// TODO: I can't find docs on how this works so ignoring this chunk for now.
#[instrument(skip(b))]
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

fn degrees(b: &[u8]) -> IResult<&[u8], Angle> {
    let (b, v) = float(b)?;
    Ok((b, Angle::degrees(v)))
}

#[derive(Debug)]
pub enum Bend {
    Positive,
    Negative,
}

fn bend(b: &[u8]) -> IResult<&[u8], Bend> {
    let (b, neg_or_pos) = be_i8(b)?;
    Ok((
        b,
        match neg_or_pos {
            -1 => Bend::Negative,
            1 => Bend::Positive,
            _ => panic!("Invalid bend direction"), // TODO: error
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
    match strlen {
        0 => Ok((bytes, None)),
        1 => Ok((bytes, Some(String::new()))),
        _ => {
            let (bytes, taken) = take(strlen - 1)(bytes)?;
            // TODO: std::str::from_utf8 ?
            let s: String = String::from_utf8(taken.to_vec()).unwrap(); // TODO: nom error
            Ok((bytes, Some(s)))
        }
    }
}

fn str(b: &[u8]) -> IResult<&[u8], String> {
    let (b, s) = str_opt(b)?;
    match s {
        Some(s) => Ok((b, s)),
        None => panic!(), // TODO: Error
    }
}

fn boolean(b: &[u8]) -> IResult<&[u8], bool> {
    let (b, byte) = be_u8(b)?;
    match byte {
        0 => Ok((b, false)),
        1 => Ok((b, true)),
        _ => panic!(),
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

fn length_count_first_flagged<'a, F, RF, O>(f: F) -> impl Fn(&'a [u8]) -> IResult<&'a [u8], Vec<O>>
where
    F: Fn(bool) -> RF,
    RF: Fn(&'a [u8]) -> IResult<&'a [u8], O>,
{
    move |b| {
        let (b, c) = varint_usize(b)?;
        if c == 0 {
            return Ok((b, Vec::with_capacity(0)));
        }
        let (b, first) = f(true)(b)?;
        let mut items = vec![first];
        let (b, mut rest) = count(f(false), c - 1)(b)?;
        items.append(&mut rest);
        Ok((b, items))
    }
}

/// Grabs a varint as the count, similar to length_count, except it passes in true on the last
/// iteration.
fn length_count_last_flagged<'a, F, RF, O>(f: F) -> impl Fn(&'a [u8]) -> IResult<&'a [u8], Vec<O>>
where
    F: Fn(bool) -> RF,
    RF: Fn(&'a [u8]) -> IResult<&'a [u8], O>,
{
    move |b| {
        let (b, c) = varint_usize(b)?;
        if c == 0 {
            return Ok((b, Vec::with_capacity(0)));
        }
        let (b, mut items) = count(f(false), c - 1)(b)?;
        let (b, last) = f(true)(b)?;
        items.push(last);
        Ok((b, items))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_log::test;

    #[test]
    fn parser() {
        // let b = include_bytes!("../../assets/spineboy-pro-4.1/spineboy-pro.skel");
        // let b = include_bytes!("../../assets/ess-test/spineboy-ess.skel");
        // let b = include_bytes!("../../assets/test/skeleton.skel");

        let b = include_bytes!("../../assets/spineboy-ess-4.1/spineboy-ess.skel");
        let skeleton = BinarySkeletonParser::parse(b).unwrap();
        dbg!(&skeleton);
        assert_eq!(skeleton.info.version, "4.1.08".to_string());
        assert_eq!(skeleton.bones.len(), 18);
        assert_eq!(skeleton.slots.len(), 20);
        assert_eq!(skeleton.ik.len(), 0);

        // Spineboy pro
        let b = include_bytes!("../../assets/spineboy-pro-4.1/spineboy-pro.skel");
        let skeleton = BinarySkeletonParser::parse(b).unwrap();
        assert_eq!(skeleton.info.version, "4.1.08".to_string());
        assert_eq!(skeleton.bones.len(), 67);
        assert_eq!(skeleton.slots.len(), 52);
        assert_eq!(skeleton.ik.len(), 7);
    }

    fn length_count_fn(last: bool) -> impl Fn(&[u8]) -> IResult<&[u8], (bool, u8)> {
        move |b: &[u8]| Ok((&b[1..], (last, b[0])))
    }

    #[test]
    fn length_count_last() {
        let data = &[4, 0, 1, 2, 3];
        let (b, items) = length_count_last_flagged(length_count_fn)(data).unwrap();
        assert_eq!(b.len(), 0);
        assert_eq!(items, vec![(false, 0), (false, 1), (false, 2), (true, 3)]);
    }

    #[test]
    fn length_count_last_one() {
        let data = &[1, 0];
        let (b, items) = length_count_last_flagged(length_count_fn)(data).unwrap();
        assert_eq!(b.len(), 0);
        assert_eq!(items, vec![(true, 0)]);
    }

    #[test]
    fn length_count_last_zero() {
        let data = &[0];
        let (b, items) = length_count_last_flagged(length_count_fn)(data).unwrap();
        assert_eq!(b.len(), 0);
        assert_eq!(items.len(), 0);
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

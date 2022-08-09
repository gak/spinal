use crate::binary::{boolean, col, float, str, varint_usize, vec2};
use crate::skeleton::{Bone, ParentTransform};
use nom::number::complete::be_u8;
use nom::sequence::tuple;
use nom::IResult;

pub(crate) fn bones(b: &[u8]) -> IResult<&[u8], Vec<Bone>> {
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

pub(crate) fn bone(b: &[u8], root: bool) -> IResult<&[u8], Bone> {
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

pub(crate) fn transform_mode(b: &[u8]) -> IResult<&[u8], ParentTransform> {
    let (b, v) = be_u8(b)?;
    Ok((b, v.into()))
}

pub(crate) fn bone_parent(b: &[u8], root: bool) -> IResult<&[u8], Option<usize>> {
    Ok(match root {
        true => (b, None),
        false => {
            let (b, v) = varint_usize(b)?;
            (b, Some(v))
        }
    })
}

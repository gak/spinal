use crate::binary::{
    bend, col, degrees, float, length_count_last_flagged, str, varint, varint_usize, vec2,
    BinarySkeletonParser,
};
use crate::color::Color;
use crate::skeleton::{Animation, BezierCurve, Curve};
use crate::Angle;
use bevy_math::Vec2;
use nom::character::complete::u8;
use nom::error::dbg_dmp;
use nom::multi::{count, length_count};
use nom::number::complete::be_u8;
use nom::sequence::tuple;
use nom::IResult;
use tracing::trace;

#[derive(Debug)]
struct AnimatedSlot {
    slot_index: usize,
    keyframes: Vec<SlotKeyframe>,
}

#[derive(Debug)]
enum SlotKeyframe {
    Attachment(f32, Option<String>),
    OneColor(f32, Color, Curve),
    TwoColor(f32, Color, Color, Curve),
}

#[derive(Debug)]
struct AnimatedBone {
    bone_index: usize,
    keyframes: Vec<BoneKeyframe>,
}

#[derive(Debug)]
enum BoneKeyframe {
    BoneRotate(f32, Angle, Option<Curve>),
    BoneTranslate(f32, BoneKeyframeData),
    BoneScale(f32, BoneKeyframeData),
    BoneShear(f32, BoneKeyframeData),
}

#[derive(Debug)]
struct BoneKeyframeData {
    amount: Vec2,
    curve: Option<Curve>,
}

impl BinarySkeletonParser {
    pub fn animation(&self) -> impl FnMut(&[u8]) -> IResult<&[u8], Animation> + '_ {
        |b: &[u8]| {
            let (b, name) = str(b)?; // Undocumented
            trace!(?name, "----------------------------------->");
            println!("after animation name: {:?}", &b[0..20]);
            // Spineboy pro at this point:
            // "aim"
            // [10, 1, 46, 1, 0, 1, 0, 0, 0, 0, 2, 5, 33, 1, 0, 1, 0, 0, 0, 0]
            // Aim doesn't have 10 slots, so it's something else.
            let (b, _) = be_u8(b)?; // ?

            let (b, slots) = length_count(varint, self.animated_slot())(b)?;
            trace!(?slots);

            println!("after slots {:?}", &b[0..20]);
            let (b, bones) = length_count(varint, self.animated_bone())(b)?;
            trace!(?bones);

            println!("after bones {:?}", &b[0..20]);
            let (b, ik) = length_count(varint, animated_ik)(b)?;
            trace!(?ik);

            println!("after ik {:?}", &b[0..20]);
            let (b, transforms) = length_count(varint, animated_transform)(b)?;

            println!("after transform {:?}", &b[0..20]);
            let (b, paths) = length_count(varint, Self::todo)(b)?;
            let (b, skins) = length_count(varint, Self::todo)(b)?;
            let (b, draw_orders) = length_count(varint, Self::todo)(b)?;
            let (b, events) = length_count(varint, Self::todo)(b)?;

            // TODO: Fill in
            Ok((
                b,
                Animation {
                    name,
                    keyframes: vec![],
                },
            ))
        }
    }

    fn todo(b: &[u8]) -> IResult<&[u8], Vec<()>> {
        Ok((b, vec![]))
    }

    fn animated_slot(&self) -> impl FnMut(&[u8]) -> IResult<&[u8], AnimatedSlot> + '_ {
        |b: &[u8]| {
            let (b, slot_index) = varint_usize(b)?;
            trace!(?slot_index);
            trace!(slot_name = ?self.skeleton.slots[slot_index].name);

            let (b, timelines) = length_count(varint, self.slot_timeline())(b)?;
            dbg!(&timelines);
            let timelines: Vec<SlotKeyframe> = timelines.into_iter().flatten().collect();
            let slot = AnimatedSlot {
                slot_index,
                keyframes: timelines,
            };
            Ok((b, slot))
        }
    }

    fn slot_timeline(&self) -> impl FnMut(&[u8]) -> IResult<&[u8], Vec<SlotKeyframe>> + '_ {
        |b: &[u8]| {
            let (b, timeline_type) = be_u8(b)?;
            let (b, keyframes) = length_count(varint, self.slot_keyframe(timeline_type))(b)?;
            Ok((b, keyframes))
        }
    }

    fn slot_keyframe(
        &self,
        keyframe_type: u8,
    ) -> impl FnMut(&[u8]) -> IResult<&[u8], SlotKeyframe> + '_ {
        move |b: &[u8]| {
            println!("slot keyframe {:?}", &b[0..20]);
            let (b, time) = float(b)?;
            let (b, keyframe) = match keyframe_type {
                0 => {
                    let (b, attachment) = self.str_table_opt()(b)?;
                    let keyframe =
                        SlotKeyframe::Attachment(time, attachment.map(|s| s.to_string()));
                    // let (b, attachment) = varint(b)?;
                    trace!(?attachment);
                    (b, keyframe)
                }
                1 => {
                    let (b, color) = col(b)?;
                    let (b, c) = curve(b)?;
                    let keyframe = SlotKeyframe::OneColor(time, color, c);
                    (b, keyframe)
                }
                2 => {
                    let (b, color1) = col(b)?;
                    let (b, color2) = col(b)?;
                    let (b, c) = curve(b)?;
                    let keyframe = SlotKeyframe::TwoColor(time, color1, color2, c);
                    (b, keyframe)
                }
                _ => panic!("Unknown timeline type {}", keyframe_type),
            };

            Ok((b, keyframe))
        }
    }

    fn animated_bone(&self) -> impl FnMut(&[u8]) -> IResult<&[u8], AnimatedBone> + '_ {
        |b: &[u8]| {
            println!("animated_bone: {:?}", &b[0..20]);
            let (b, bone_index) = varint_usize(b)?;
            trace!(?bone_index);
            trace!(bone_name = ?self.skeleton.bones[bone_index].name);
            let (b, keyframes) = length_count(varint, bone_timeline)(b)?;
            trace!(?keyframes);
            let keyframes = keyframes.into_iter().flatten().collect();
            let bone = AnimatedBone {
                bone_index,
                keyframes,
            };
            Ok((b, bone))
        }
    }
}

fn bone_timeline(b: &[u8]) -> IResult<&[u8], Vec<BoneKeyframe>> {
    let (b, timeline_type) = be_u8(b)?;
    let (b, keyframes) = length_count_last_flagged(|last| bone_keyframe(timeline_type, last))(b)?;
    Ok((b, keyframes))
}

fn bone_keyframe(timeline_type: u8, last: bool) -> impl Fn(&[u8]) -> IResult<&[u8], BoneKeyframe> {
    move |b: &[u8]| {
        trace!(?timeline_type);
        let (b, time) = float(b)?;
        trace!(?time);
        let (b, what_is_this) = be_u8(b)?; // ??? This might be before time.
        trace!(?what_is_this);

        let (b, keyframe) = match timeline_type {
            0 => {
                let (b, rotation) = degrees(b)?;
                let (b, c) = if last {
                    (b, None)
                } else {
                    let (b, c) = curve(b)?;
                    (b, Some(c))
                };
                let keyframe = BoneKeyframe::BoneRotate(time, rotation, c);
                (b, keyframe)
            }
            1 => {
                let (b, data) = bone_keyframe_data(b, last)?;
                let timeline_type = BoneKeyframe::BoneTranslate(time, data);
                (b, timeline_type)
            }
            2 => {
                let (b, data) = bone_keyframe_data(b, last)?;
                let timeline_type = BoneKeyframe::BoneScale(time, data);
                (b, timeline_type)
            }
            3 => {
                let (b, data) = bone_keyframe_data(b, last)?;
                let timeline_type = BoneKeyframe::BoneShear(time, data);
                (b, timeline_type)
            }
            _ => panic!("Unknown timeline type {}", timeline_type),
        };
        Ok((b, keyframe))
    }
}

fn bone_keyframe_data(b: &[u8], last: bool) -> IResult<&[u8], BoneKeyframeData> {
    let (b, amount) = vec2(b)?;
    let (b, c) = if !last {
        let (b, c) = curve(b)?;
        (b, Some(c))
    } else {
        (b, None)
    };
    let data = BoneKeyframeData { amount, curve: c };
    Ok((b, data))
}

// TODO: Just nomming and not saving.
// [1, 0, 1, 0, 0, 0, 0, 0, 63, 126, 184, 82, 0, 0, 0, 0, 1, 0, 0, 3]
// 1 IK entry (already nommed)
// Index of IK 0 <-- this fn
// 1 keyframes <-- length_count
// 0 0 0 0 - time <-- ik_keyframes
// 0 ?? (similar to bones 0 near time)
// 63 126 184 82 - mix!
// 0 0 0 0 ?
// 1 ?
fn animated_ik(b: &[u8]) -> IResult<&[u8], Vec<()>> {
    let (b, ik_index) = varint_usize(b)?;
    trace!(?ik_index);
    let (b, keyframes) = length_count_last_flagged(|last| ik_keyframe(last))(b)?;
    Ok((b, keyframes))
}

// // TODO: Just nomming and not saving.
// fn ik_timeline(b: &[u8]) -> IResult<&[u8], Vec<()>> {
//     let (b, keyframes) = length_count_last_flagged(ik_keyframes)(b)?;
//     Ok((b, keyframes))
// }

// TODO: Just nomming and not saving.
fn ik_keyframe(last: bool) -> impl Fn(&[u8]) -> IResult<&[u8], ()> {
    move |b: &[u8]| {
        println!("keyframe: {:?}", &b[0..20]);
        // [0, 0, 0, 0, 0, 63, 126, 184, 82, 0, 0, 0, 0, 1,  0,  0,  3, 0, 1, 0]
        // [0, 0, 0, 0, 0, 63, 126, 184, 82, 0, 0, 0, 0, 1,  1,  0,  3, 0, 1, 0] turn on compress
        // [0, 0, 0, 0, 0, 63, 126, 184, 82, 0, 0, 0, 0, 1,  0,  1,  3, 0, 1, 0] turn on stretch
        // [ time    ]  ?  [ mix ?        ]  [ ??     ] bnd? ^   ^  [ transforms? ]
        //                                                   compress, stretch
        let (b, time) = float(b)?;
        trace!(?time);
        let (b, what_is_this) = be_u8(b)?;
        let (b, mix) = float(b)?;
        trace!(?mix);

        let (b, what_is_this) = float(b)?;
        trace!(?what_is_this);

        let (b, bend) = bend(b)?;
        trace!(?bend);

        let (b, compress) = be_u8(b)?;
        let (b, stretch) = be_u8(b)?;

        let (b, c) = if last {
            (b, None)
        } else {
            let (b, c) = curve(b)?;
            (b, Some(c))
        };

        Ok((b, ()))
    }
}

// TODO: Just nomming and not saving.
fn animated_transform(b: &[u8]) -> IResult<&[u8], Vec<()>> {
    println!("animated_transform: {:?}", &b[0..50]);
    // [0, 1, 0, 0, 0, 0, 0, 63, 72, 180, 58, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 63, 40, 180, 58, 0, 0, 0, 0, 0, 0, 0, 0]
    //  ^--transform index   [ rotate mix  ]  [ trnmix ]  [ scale  ]  [ shear  ]  [ ?????? ]  [ ?????? ]  ^--transform index
    //     ^--1 frame count
    //        [ time   ]  ?
    /*
       "aim-front-arm-transform": [ (0)
           { "mixRotate": 0.784, "mixX": 0, "mixScaleX": 0, "mixShearY": 0 }
       ],
       "aim-head-transform": [ (1)
           { "mixRotate": 0.659, "mixX": 0, "mixScaleX": 0, "mixShearY": 0 }
       ],
       "aim-torso-transform": [ (3)
           { "mixRotate": 0.423, "mixX": 0, "mixScaleX": 0, "mixShearY": 0 }
       ]
    */

    let (b, transform_index) = varint_usize(b)?;
    let (b, keyframes) = length_count(varint, animated_keyframe)(b)?;
    Ok((b, keyframes))
}

// TODO: Just nomming and not saving.
fn animated_keyframe(b: &[u8]) -> IResult<&[u8], ()> {
    let (b, time) = float(b)?;
    trace!(?time);

    let (b, what_is_this) = be_u8(b)?;

    let (b, rotate_mix) = float(b)?;
    trace!(?rotate_mix);
    let (b, translate_mix) = float(b)?;
    trace!(?translate_mix);
    let (b, scale_mix) = float(b)?;
    trace!(?scale_mix);
    let (b, shear_mix) = float(b)?;
    trace!(?shear_mix);
    let (b, what_is_this) = float(b)?;
    let (b, what_is_this) = float(b)?;
    Ok((b, ()))
}

fn curve(b: &[u8]) -> IResult<&[u8], Curve> {
    let (b, curve_type) = be_u8(b)?;
    let (b, curve) = match curve_type {
        0 => (b, Curve::Stepped),
        1 => (b, Curve::Linear),
        2 => {
            let (b, (cx1, cy1, cx2, cy2)) = tuple((float, float, float, float))(b)?;
            (b, Curve::Bezier(BezierCurve { cx1, cy1, cx2, cy2 }))
        }
        _ => panic!("Unknown curve type {}", curve_type),
    };
    Ok((b, curve))
}

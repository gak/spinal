use crate::binary::{
    bend, col, degrees, float, length_count_first_flagged, length_count_last_flagged, str, varint,
    varint_usize, vec2, BinarySkeletonParser,
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
use tracing::{trace, warn};

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
            println!("animated_bone: {:?}", &b[0..100]);
            let (b, bone_index) = varint_usize(b)?;
            trace!(?bone_index);
            trace!(bone_name = ?self.skeleton.bones[bone_index].name);

            // 33, 1, 0, 1,   0, 0, 0, 0, 0, 66, 16, 81, 18, 42, 1, 0, 1, 0, 0, 0, 0, 0, 193, 212, 108, 11, 41, 1, 0, 1, 0, 0, 0, 0, 0, 66, 121, 56, 178, 32, 1, 0, 1, 0, 0, 0, 0, 0, 65, 17, 200, 236, 43, 1, 0, 1, 0, 0, 0, 0, 0, 190, 156, 55, 128, 1, 0, 1, 0, 0, 0, 0, 0, 63, 126, 184, 82, 0, 0, 0, 0, 1, 0, 0, 3, 0, 1, 0, 0, 0, 0, 0, 63, 72, 180, 58, 0, 0, 0, 0]
            // ^^ bone index  ?? [ time   ]
            //     1 timeline
            //        rotate
            //           1 keyframe
            //

            // Stops at death -> bones -> head
            // [46, 2, 0, 15, 13, 0, 0, 0, 0, 192, 52, 253, 192, 61, 136, 136, 137, 65, 66, 251, 198, 2, 60, 125, 10, 179, 192, 52, 253, 192, 61, 18, 48, 107, 65, 75, 130, 82,...
            //  ^-- bone_index    [ time   ]  [ -2.827 value? ]  [ .066 2nd time?]  [ 12.18 value   ] ?  [ 0.015        ]  [ -2.82         ]  [ 0.035       ]  [ 12.7        ]
            //      ^-- 2 timelines                              [ SECOND ROTATE? ...........       ]    [ This looks like the first curve ..................................]
            //         ^-- rotate timeline_type                ^^-- Missing curve here                ^ curve type?
            //            ^-- 15 keyframes rotations                Maybe curves are after the first rotate unlike JSON?
            //                ^^-- ??
            //
            // [62, 8, 136, 137, 192, 219, 121, 92, 2, 61, 196, 156, 86, 65, 58, 232, 228, 61, 243, 243, 205, 191, 146, 118, 16,
            // [ 0.133     ?  ]  [ -6.85       ? ] CRV [ 0.096        ]  [ 11.68        ]  [ assume 3th    ]  [ assume 4th    ]
            // [ 3RD time     ]  [ 3RD value     ] TYP [ 2ND CURVE    ]
            //
            // [ 62, 153, 153, 154, 194, 19, 116, 146, 2, 62, 24, 220, 15, 193, 84, 71, 15, 62, 87, 137, 190, 194, 21, 30, 40, 62, 238, 238, 240, 193, 187, 234, 181, 2, 62, 181, 62]
            // [ 4th time (0.3)  ]  [ 4th val -36.8 ] CRV [ 0.149       ]
            //                                              3RD CURVE
            /*
               "rotate": [
                   {
                       "value": -2.83,
                       "curve": [ 0.015, -2.83, 0.036, 12.72 ]
                   },
                   {
                       "time": 0.0667,
                       "value": 12.19,
                       "curve": [ 0.096, 11.68, 0.119, -1.14 ]
                   },

            // Bone scale has 2 curves?
            // [62, 238, 238, 240, 63, 128, 0, 0, 63, 128, 0, 0, 63, 0, 0, 0, 63, 136, 81, 236, 63, 112, 163, 215, 2, 62, 240, 32, 197, 63, 128, 163, 215, 62, 251, 187, 189, 63, 136, 81, 236, 62, 243, 51, 52, 63, 130, 73, 37, 62, 251, 187, 189, 63, 112, 163, 215, 63, 17, 17, 18, 63, 125, 84, 177,
            // [ time 0.466     ]  [ value (1) ]  [ 1 ???     ]  [ 0.5 ??  ]  [ 1.065        ]  [ 0.94          ]     [ 0.469        ]  [ 2             ]  [ 3             ]  [ 4            ]  [ 1           ]  [ 2           ]  [ 3             ]  [ 4 0.94        ]  [ 0.566      ]  [ 0.98957354   ]
            // #1 time             [ x y scale                ]  [ #2 time ]  [ x y scale                       ]  C  [ Curve 1                                                              ]  [ Curve 2                                                            ]  [ #3 time    ] ...

                "scale": [
                    {
                        "time": 0.4667,
                        "curve": [ 0.469, 1.005, 0.492, 1.065, 0.475, 1.018, 0.492, 0.94 ]
                    },
                    {
                        "time": 0.5,
                        "x": 1.065,
                        "y": 0.94,
                        "curve": [ 0.517, 1.065, 0.541, 0.991, 0.517, 0.94, 0.542, 1.026 ]
                    },
                    {
                        "time": 0.5667,
                        "x": 0.99,
                        "y": 1.025,
                        "curve": [ 0.593, 0.988, 0.609, 1.002, 0.595, 1.024, 0.607, 1.001 ]
                    },

            // Scale is 4. Looks like the timeline types are very different from the docs.
            // Scale has 2 curves, for X and Y.

            */

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
    println!("BONE TIMELINE! {:?}", &b[0..100]);
    let (b, timeline_type) = be_u8(b)?;
    trace!(?timeline_type);
    let (b, keyframe_count) = varint_usize(b)?;
    let (b, what_is_this) = be_u8(b)?; // ???
    trace!(?what_is_this);
    let (b, first) = bone_keyframe(timeline_type, true)(b)?;
    let (b, mut remaining) = if keyframe_count > 1 {
        count(bone_keyframe(timeline_type, false), keyframe_count - 1)(b)?
    } else {
        (b, vec![])
    };
    let mut keyframes = vec![first];
    keyframes.append(&mut remaining);
    Ok((b, keyframes))
}

fn bone_keyframe(timeline_type: u8, first: bool) -> impl Fn(&[u8]) -> IResult<&[u8], BoneKeyframe> {
    move |b: &[u8]| {
        println!("bone keyframe {:?}", &b[0..100]);
        let (b, time) = float(b)?;
        trace!(?time);

        let (b, keyframe) = match timeline_type {
            0 => {
                let (b, rotation) = degrees(b)?;
                trace!(?rotation);
                trace!(?first);
                let (b, c) = if first {
                    (b, None)
                } else {
                    let (b, c) = curve(b)?;
                    (b, Some(c))
                };
                trace!(?c);
                let keyframe = BoneKeyframe::BoneRotate(time, rotation, c);
                (b, keyframe)
            }
            1 => {
                let (b, data) = bone_keyframe_data(b, first)?;
                let timeline_type = BoneKeyframe::BoneTranslate(time, data);
                (b, timeline_type)
            }
            2 => {
                let (b, data) = bone_keyframe_data(b, first)?;
                let timeline_type = BoneKeyframe::BoneScale(time, data);
                (b, timeline_type)
            }
            3 => {
                let (b, data) = bone_keyframe_data(b, first)?;
                let timeline_type = BoneKeyframe::BoneShear(time, data);
                (b, timeline_type)
            }
            _ => panic!("Unknown timeline type {}", timeline_type),
        };
        Ok((b, keyframe))
    }
}

fn bone_keyframe_data(b: &[u8], first: bool) -> IResult<&[u8], BoneKeyframeData> {
    let (b, amount) = vec2(b)?;
    let (b, c) = if first {
        (b, None)
    } else {
        let (b, c) = curve(b)?;
        (b, Some(c))
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

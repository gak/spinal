use crate::binary::{col, float, str, varint, varint_usize, BinarySkeletonParser};
use crate::color::Color;
use crate::skeleton::{Animation, BezierCurve, Curve};
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
    timelines: Vec<Timeline>,
}

#[derive(Debug)]
struct Timeline {
    time: f32,
    timeline_type: TimelineType,
}

#[derive(Debug)]
enum TimelineType {
    Attachment(Option<String>),
    OneColor(Color, Curve),
    TwoColor(Color, Color, Curve),
}

impl BinarySkeletonParser {
    pub fn animation(&self) -> impl FnMut(&[u8]) -> IResult<&[u8], Animation> + '_ {
        |b: &[u8]| {
            let (b, name) = str(b)?; // Undocumented
            trace!(?name);
            println!("{:?}", &b[0..20]);
            // Spineboy pro at this point:
            // "aim"
            // [10, 1, 46, 1, 0, 1, 0, 0, 0, 0, 2, 5, 33, 1, 0, 1, 0, 0, 0, 0]
            // Aim doesn't have 10 slots, so it's something else.
            let (b, _) = be_u8(b)?; // ?

            // 1 looks like the count of the slots for Aim.
            let (b, slot_count) = varint(b)?;

            // 46 is the "crosshair" slot.
            let (b, slot_index) = varint(b)?;

            // We're expecting 1 timeline.
            let (b, timeline_count) = varint(b)?;

            // ... of SLOT_ATTACHMENT (0)
            let (b, timeline_type) = be_u8(b)?;

            // 1 frame
            let (b, timeline_type) = be_u8(b)?;

            // 0 0 0 0 float is zero.
            let (b, time) = float(b)?;
            trace!(?time);

            // attachment name!
            let (b, attachment_name) = self.str_table_opt()(b)?;
            trace!(?attachment_name);

            let (b, slots) = length_count(varint, self.animated_slot())(b)?;
            trace!(?slots);

            todo!()
        }
    }

    fn animated_slot(&self) -> impl FnMut(&[u8]) -> IResult<&[u8], AnimatedSlot> + '_ {
        |b: &[u8]| {
            let (b, slot_index) = varint_usize(b)?;
            trace!(?slot_index);
            trace!(slot_name = ?self.skeleton.slots[slot_index].name);
            let (b, timelines) = length_count(varint, self.timeline())(b)?;
            let slot = AnimatedSlot {
                slot_index,
                timelines,
            };
            Ok((b, slot))
        }
    }

    fn timeline(&self) -> impl FnMut(&[u8]) -> IResult<&[u8], Timeline> + '_ {
        |b: &[u8]| {
            let (b, timeline_type) = be_u8(b)?;
            let (b, frame_count) = varint(b)?;
            let (b, time) = float(b)?;
            let (b, timeline_type) = match timeline_type {
                0 => {
                    let (b, attachment) = self.str_table_opt()(b)?;
                    let timeline_type = TimelineType::Attachment(attachment.map(|s| s.to_string()));
                    (b, timeline_type)
                }
                1 => {
                    let (b, color) = col(b)?;
                    let (b, c) = curve(b)?;
                    let timeline_type = TimelineType::OneColor(color, c);
                    (b, timeline_type)
                }
                2 => {
                    let (b, color1) = col(b)?;
                    let (b, color2) = col(b)?;
                    let (b, c) = curve(b)?;
                    let timeline_type = TimelineType::TwoColor(color1, color2, c);
                    (b, timeline_type)
                }
                _ => panic!("Unknown timeline type {}", timeline_type),
            };

            let timeline = Timeline {
                time,
                timeline_type,
            };
            Ok((b, timeline))
        }
    }
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

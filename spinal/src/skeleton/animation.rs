use crate::color::Color;
use crate::skeleton::Event;
use crate::{Angle, BoneModification};
use bevy_math::{Affine3A, Vec2};

#[derive(Debug, Clone)]
pub struct Animation {
    pub name: String,
    pub bones: Vec<AnimatedBone>,
}

#[derive(Debug, Clone)]
pub struct AnimatedBone {
    pub bone_index: usize,
    pub timelines: Vec<Timeline>,
}

#[derive(Debug, Clone)]
pub struct Timeline {
    // data_type: BoneKeyframeDataType, // TODO: Use this as a sanity check? Or just make a ::new() and make frames private.
    /// These [BoneKeyframe]s should all be the same discriminant.
    pub frames: Vec<BoneKeyframe>,
}

#[derive(Debug, Clone)]
pub struct BoneKeyframe {
    pub time: f32,
    pub data: BoneKeyframeData,
}

// http://en.esotericsoftware.com/spine-binary-format is wrong about repr values.
// See http://en.esotericsoftware.com/spine-api-reference#SkeletonBinary for an updated list.
#[derive(Debug, Clone, strum::EnumDiscriminants)]
#[strum_discriminants(name(BoneKeyframeDataType))]
#[strum_discriminants(derive(strum::FromRepr))]
#[strum_discriminants(vis(pub))]
pub enum BoneKeyframeData {
    BoneRotate(Angle, Interpolation<1>),
    BoneTranslate(Vec2, Interpolation<2>),
    BoneTranslateX(()),
    // TODO: Implement these
    BoneTranslateY(()),
    BoneScale(Vec2, Interpolation<2>),
    BoneScaleX(()),
    BoneScaleY(()),
    BoneShear(Vec2, Interpolation<2>),
    BoneShearX(()),
    BoneShearY(()),
}

impl BoneKeyframeData {
    pub fn to_bone_modification(&self) -> BoneModification {
        match self {
            BoneKeyframeData::BoneRotate(rotation, _) => BoneModification::from_rotation(rotation.to_owned()),
            BoneKeyframeData::BoneTranslate(translate, _) => BoneModification::from_translation(translate.to_owned()),
            BoneKeyframeData::BoneScale(scale, _) => BoneModification::from_scale(scale.to_owned()),
            _ => todo!(),
        }
    }
}

#[derive(Debug, Clone, strum::EnumDiscriminants)]
#[strum_discriminants(name(InterpolationType))]
#[strum_discriminants(derive(strum::FromRepr))]
#[strum_discriminants(vis(pub))]
pub enum Interpolation<const N: usize> {
    Linear,
    Stepped,
    Bezier([Bezier; N]),

    None,
}

#[derive(Debug)]
pub struct AnimatedSlot {
    pub slot_index: usize,
    pub keyframes: Vec<SlotKeyframe>,
}

#[derive(Debug)]
pub enum SlotKeyframe {
    Attachment(f32, Option<String>),
    OneColor(f32, Color, Interpolation<1>),
    TwoColor(f32, Color, Color, Interpolation<1>),
}

pub struct AnimatedEvent {
    pub time: f32,
    pub event: Event,
}

// #[derive(Debug)]
// pub enum AnimationKeyframe {
//     Bone(BoneKeyframe),
//     Slot(SlotKeyframe),
// }
//
// #[derive(Debug)]
// pub struct BoneKeyframe {
//     pub bone_idx: usize,
//     pub affinity: Affine3A,
// }
//
// #[derive(Debug)]
// pub struct SlotKeyframe {
//     pub slot: usize,
//     pub slot_action: SlotAction,
// }
//
// #[derive(Debug)]
// pub enum SlotAction {
//     Attachment(SlotAttachmentAction),
//     // Color(Vec<ColorKeyframe>),
//     // TwoColor(Vec<TwoColorKeyframe>),
// }
//
// #[derive(Debug)]
// pub struct SlotAttachmentAction {
//     pub attachment: usize,
// }
//
// //
//
// // #[derive(Debug)]
// // pub struct SlotAnimation {
// //     slot: usize,
// //     timelines: Vec<AnimationTimeline>,
// // }
//
// #[derive(Debug)]
// pub struct AnimationTimeline {
//     timeline_type: AnimationTimelineType,
//     //
// }
//
// #[derive(Debug)]
// pub enum AnimationTimelineType {
//     Attachment(Vec<AttachmentKeyframe>),
//     Color(Vec<ColorKeyframe>),
//     TwoColor(Vec<TwoColorKeyframe>),
// }
//
// #[derive(Debug)]
// pub struct AttachmentKeyframe {
//     /// The time in seconds for the keyframe.
//     time: f32,
//
//     /// The name of the attachment to set on the slot, or `None` to clear the slot's attachment.
//     attachment_string: Option<usize>,
// }
//
// // TODO: Make a special type for Vec<ColorKeyframe> where the last frame does not have a curve.
// #[derive(Debug)]
// pub struct ColorKeyframe {
//     /// The time in seconds for the keyframe.
//     time: f32,
//
//     /// The slot color to set for the keyframe.
//     color: Color,
//
//     /// The keyframe's curve. The curve is omitted for the last keyframe.
//     curve: Option<Curve>,
// }
//
// #[derive(Debug)]
// pub struct TwoColorKeyframe {
//     /// The time in seconds for the keyframe.
//     time: f32,
//
//     /// The light slot color to set for the keyframe.
//     light: Color,
//
//     /// The dark slot color to set for the keyframe.
//     dark: Color,
//
//     /// The keyframe's curve. The curve is omitted for the last keyframe.
//     curve: Option<Curve>,
// }
//
// #[derive(Debug)]
// pub struct BoneAnimation {
//     bone: usize,
//     timelines: Vec<AnimationTimeline>,
// }

// /// A curve defines the interpolation to use between a keyframe and the next keyframe:
// /// linear, stepped, or a Bézier curve.
// #[derive(Debug, Clone)]
// pub enum Curve {
//     Linear,
//     Stepped,
//     Bezier(BezierCurve),
// }

/// The Bézier curve has 4 values which define the control points: cx1, cy1, cx2, cy2.
/// The X axis is from 0 to 1 and represents the percent of time between the two keyframes.
/// The Y axis is from 0 to 1 and represents the percent of the difference between the keyframe's
/// values.
#[derive(Debug, Clone)]
pub struct Bezier {
    pub cx1: f32,
    pub cy1: f32,
    pub cx2: f32,
    pub cy2: f32,
}

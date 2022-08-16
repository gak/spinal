use crate::color::Color;
use bevy_math::Affine3A;

#[derive(Debug)]
pub struct Animation {
    name: String,
    keyframes: Vec<Keyframe>,
}

#[derive(Debug)]
struct Keyframe {
    time: f32,

    /// Index into `Animation.keyframes` for the next keyframe.
    next: Option<usize>,

    animation: AnimationKeyframe,
}

#[derive(Debug)]
enum AnimationKeyframe {
    Bone(BoneKeyframe),
    Slot(SlotKeyframe),
}

#[derive(Debug)]
struct BoneKeyframe {
    bone_idx: usize,
    affinity: Affine3A,
}

#[derive(Debug)]
struct SlotKeyframe {
    slot: usize,
    slot_action: SlotAction,
}

#[derive(Debug)]
pub enum SlotAction {
    Attachment(SlotAttachmentAction),
    // Color(Vec<ColorKeyframe>),
    // TwoColor(Vec<TwoColorKeyframe>),
}

#[derive(Debug)]
pub struct SlotAttachmentAction {
    attachment: usize,
}

//

// #[derive(Debug)]
// pub struct SlotAnimation {
//     slot: usize,
//     timelines: Vec<AnimationTimeline>,
// }

#[derive(Debug)]
pub struct AnimationTimeline {
    timeline_type: AnimationTimelineType,
    //
}

#[derive(Debug)]
pub enum AnimationTimelineType {
    Attachment(Vec<AttachmentKeyframe>),
    Color(Vec<ColorKeyframe>),
    TwoColor(Vec<TwoColorKeyframe>),
}

#[derive(Debug)]
pub struct AttachmentKeyframe {
    /// The time in seconds for the keyframe.
    time: f32,

    /// The name of the attachment to set on the slot, or `None` to clear the slot's attachment.
    attachment_string: Option<usize>,
}

// TODO: Make a special type for Vec<ColorKeyframe> where the last frame does not have a curve.
#[derive(Debug)]
pub struct ColorKeyframe {
    /// The time in seconds for the keyframe.
    time: f32,

    /// The slot color to set for the keyframe.
    color: Color,

    /// The keyframe's curve. The curve is omitted for the last keyframe.
    curve: Option<Curve>,
}

#[derive(Debug)]
pub struct TwoColorKeyframe {
    /// The time in seconds for the keyframe.
    time: f32,

    /// The light slot color to set for the keyframe.
    light: Color,

    /// The dark slot color to set for the keyframe.
    dark: Color,

    /// The keyframe's curve. The curve is omitted for the last keyframe.
    curve: Option<Curve>,
}

#[derive(Debug)]
pub struct BoneAnimation {
    bone: usize,
    timelines: Vec<AnimationTimeline>,
}

/// A curve defines the interpolation to use between a keyframe and the next keyframe:
/// linear, stepped, or a Bézier curve.
#[derive(Debug)]
pub enum Curve {
    Linear,
    Stepped,
    Bezier(BezierCurve),
}

/// The Bézier curve has 4 values which define the control points: cx1, cy1, cx2, cy2.
/// The X axis is from 0 to 1 and represents the percent of time between the two keyframes.
/// The Y axis is from 0 to 1 and represents the percent of the difference between the keyframe's
/// values.
#[derive(Debug)]
pub struct BezierCurve {
    pub cx1: f32,
    pub cy1: f32,
    pub cx2: f32,
    pub cy2: f32,
}

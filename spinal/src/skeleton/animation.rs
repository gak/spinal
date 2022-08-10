use crate::color::Color;

#[derive(Debug)]
pub struct Animation {
    slots: Vec<SlotAnimation>,
    bones: Vec<BoneAnimation>,
    // ik: Vec<IkAnimation>,
    // transforms: Vec<TransformAnimation>,
    // paths: Vec<PathAnimation>,
    // skins: Vec<SkinAnimation>,
    // draw_orders: Vec<DrawOrderAnimation>,
    // events: Vec<EventAnimation>,
}

#[derive(Debug)]
pub struct SlotAnimation {
    slot: usize,
    timelines: Vec<AnimationTimeline>,
}

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
pub struct Curve {
    /// The type of curve.
    curve_type: CurveType,

    /// The curve's parameters.
    parameters: Vec<f32>,
}

#[derive(Debug)]
pub enum CurveType {
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
    cx1: f32,
    cy1: f32,
    cx2: f32,
    cy2: f32,
}

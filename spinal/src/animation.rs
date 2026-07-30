use std::time::Duration;

use crate::Rgba8;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FrameCurve<const CHANNELS: usize> {
    Linear,
    Stepped,
    Bezier([[f32; 4]; CHANNELS]),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ScalarFrame {
    pub(crate) time: f32,
    pub(crate) value: f32,
    pub(crate) curve: FrameCurve<1>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Vec2Frame {
    pub(crate) time: f32,
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) curve: FrameCurve<2>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ColourFrame {
    pub(crate) time: f32,
    pub(crate) colour: Rgba8,
    pub(crate) curve: FrameCurve<4>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AttachmentFrame {
    pub(crate) time: f32,
    pub(crate) name: Option<Box<str>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct IkFrame {
    pub(crate) time: f32,
    pub(crate) mix: f32,
    pub(crate) bend_positive: bool,
    pub(crate) curve: FrameCurve<2>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DrawOrderOffset {
    pub(crate) slot: u32,
    pub(crate) offset: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DrawOrderFrame {
    pub(crate) time: f32,
    pub(crate) offsets: Box<[DrawOrderOffset]>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EventPayload {
    pub(crate) integer: i32,
    pub(crate) float: f32,
    pub(crate) string: Option<Box<str>>,
    pub(crate) volume: f32,
    pub(crate) balance: f32,
}

impl Default for EventPayload {
    fn default() -> Self {
        Self {
            integer: 0,
            float: 0.0,
            string: None,
            volume: 1.0,
            balance: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EventFrame {
    pub(crate) time: f32,
    pub(crate) event: u32,
    pub(crate) payload: EventPayload,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TimelineData {
    BoneRotate {
        bone: u32,
        frames: Box<[ScalarFrame]>,
    },
    BoneTranslate {
        bone: u32,
        frames: Box<[Vec2Frame]>,
    },
    BoneScale {
        bone: u32,
        frames: Box<[Vec2Frame]>,
    },
    BoneShear {
        bone: u32,
        frames: Box<[Vec2Frame]>,
    },
    SlotAttachment {
        slot: u32,
        frames: Box<[AttachmentFrame]>,
    },
    SlotColour {
        slot: u32,
        frames: Box<[ColourFrame]>,
    },
    Ik {
        constraint: u32,
        frames: Box<[IkFrame]>,
    },
    DrawOrder {
        frames: Box<[DrawOrderFrame]>,
    },
    Events {
        frames: Box<[EventFrame]>,
    },
    Unsupported {
        name: Box<str>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AnimationData {
    pub(crate) name: Box<str>,
    pub(crate) duration: Duration,
    pub(crate) timelines: Box<[TimelineData]>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EventDefinitionData {
    pub(crate) name: Box<str>,
    pub(crate) payload: EventPayload,
    pub(crate) audio: Option<Box<str>>,
}

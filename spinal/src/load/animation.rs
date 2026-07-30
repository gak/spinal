use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use crate::{
    DiagnosticCode,
    animation::{
        AnimationData, AttachmentFrame, ColourFrame, DrawOrderFrame, DrawOrderOffset,
        EventDefinitionData, EventFrame, EventPayload, FrameCurve, IkFrame, ScalarFrame,
        TimelineData, Vec2Frame,
    },
    json::{JsonMember, JsonValue},
};

use super::{
    LoadError, LoadErrorKind, PendingDiagnostic, PendingScope,
    schema::{
        array, bool_or, colour, error, f32_or, finite_f32, i32_value, index_pointer, member,
        nonempty_string, nonnegative_f32, object, pointer, required_member, schema_error, string,
        unique_members,
    },
};

pub(crate) struct AnimationLinks<'a> {
    pub(crate) bones: &'a HashMap<Box<str>, u32>,
    pub(crate) slots: &'a HashMap<Box<str>, u32>,
    pub(crate) ik_constraints: &'a HashMap<Box<str>, u32>,
    pub(crate) events: &'a HashMap<Box<str>, u32>,
    pub(crate) event_definitions: &'a [EventDefinitionData],
    pub(crate) attachment_names: &'a HashMap<u32, HashSet<Box<str>>>,
}

pub(crate) fn parse_animations(
    value: Option<&JsonValue>,
    links: AnimationLinks<'_>,
    pending: &mut Vec<PendingDiagnostic>,
) -> Result<Box<[AnimationData]>, LoadError> {
    let Some(value) = value else {
        return Ok(Box::default());
    };
    let animations = object(value, "/animations")?;
    unique_members(animations, "/animations")?;
    ensure_capacity(animations.len(), "/animations")?;

    let mut output = Vec::with_capacity(animations.len());
    for (index, animation) in animations.iter().enumerate() {
        let path = pointer("/animations", animation.name());
        if animation.name().is_empty() {
            return Err(schema_error(&path, "animation name must not be empty"));
        }
        let animation_index = index_u32(index, &path)?;
        let data = object(animation.value(), &path)?;
        unique_members(data, &path)?;
        let mut timelines = Vec::new();
        let mut duration = 0.0_f32;

        if let Some(value) = member(data, "bones", &path)? {
            parse_bone_timelines(
                value,
                &pointer(&path, "bones"),
                &links,
                animation.name(),
                animation_index,
                &mut timelines,
                &mut duration,
                pending,
            )?;
        }
        if let Some(value) = member(data, "slots", &path)? {
            parse_slot_timelines(
                value,
                &pointer(&path, "slots"),
                &links,
                animation.name(),
                animation_index,
                &mut timelines,
                &mut duration,
                pending,
            )?;
        }
        if let Some(value) = member(data, "ik", &path)? {
            parse_ik_timelines(
                value,
                &pointer(&path, "ik"),
                &links,
                animation.name(),
                animation_index,
                &mut timelines,
                &mut duration,
                pending,
            )?;
        }
        let draw_order = member(data, "drawOrder", &path)?;
        let legacy_draw_order = member(data, "draworder", &path)?;
        match (draw_order, legacy_draw_order) {
            (Some(_), Some(_)) => {
                return Err(schema_error(
                    &path,
                    "draw order is specified by both \"drawOrder\" and \"draworder\"",
                ));
            }
            (Some(value), None) => {
                let timeline_path = pointer(&path, "drawOrder");
                if !retain_draw_order_with_unknown_fields(
                    value,
                    &timeline_path,
                    animation.name(),
                    animation_index,
                    &mut timelines,
                    &mut duration,
                    pending,
                )? {
                    let frames =
                        parse_draw_order_frames(value, &timeline_path, &links, &mut duration)?;
                    timelines.push(TimelineData::DrawOrder { frames });
                }
            }
            (None, Some(value)) => {
                let timeline_path = pointer(&path, "draworder");
                if !retain_draw_order_with_unknown_fields(
                    value,
                    &timeline_path,
                    animation.name(),
                    animation_index,
                    &mut timelines,
                    &mut duration,
                    pending,
                )? {
                    let frames =
                        parse_draw_order_frames(value, &timeline_path, &links, &mut duration)?;
                    timelines.push(TimelineData::DrawOrder { frames });
                }
            }
            (None, None) => {}
        }
        if let Some(value) = member(data, "events", &path)? {
            let timeline_path = pointer(&path, "events");
            if !retain_timeline_with_unknown_fields(
                value,
                &timeline_path,
                &[
                    "time", "name", "int", "float", "string", "volume", "balance",
                ],
                "events/options",
                animation.name(),
                animation_index,
                &mut timelines,
                &mut duration,
                pending,
            )? {
                let frames = parse_event_frames(value, &timeline_path, &links, &mut duration)?;
                timelines.push(TimelineData::Events { frames });
            }
        }

        for section in data {
            if matches!(
                section.name(),
                "bones" | "slots" | "ik" | "drawOrder" | "draworder" | "events"
            ) {
                continue;
            }
            duration = duration.max(maximum_nested_time(section.value()));
            retain_unsupported(
                section.name(),
                animation.name(),
                animation_index,
                &mut timelines,
                pending,
            );
        }

        let duration = Duration::try_from_secs_f32(duration).map_err(|_error| {
            error(
                LoadErrorKind::NonFiniteNumber,
                &path,
                "animation duration must be finite and nonnegative",
            )
        })?;
        output.push(AnimationData {
            name: animation.name().into(),
            duration,
            timelines: timelines.into_boxed_slice(),
        });
    }
    Ok(output.into_boxed_slice())
}

#[allow(clippy::too_many_arguments)]
fn parse_bone_timelines(
    value: &JsonValue,
    path: &str,
    links: &AnimationLinks<'_>,
    animation_name: &str,
    animation_index: u32,
    output: &mut Vec<TimelineData>,
    duration: &mut f32,
    pending: &mut Vec<PendingDiagnostic>,
) -> Result<(), LoadError> {
    let bones = object(value, path)?;
    unique_members(bones, path)?;
    for bone in bones {
        let bone_path = pointer(path, bone.name());
        let bone_index = links.bones.get(bone.name()).copied().ok_or_else(|| {
            error(
                LoadErrorKind::UnresolvedReference,
                &bone_path,
                format!("animation bone {:?} does not exist", bone.name()),
            )
        })?;
        let timelines = object(bone.value(), &bone_path)?;
        unique_members(timelines, &bone_path)?;
        for timeline in timelines {
            let timeline_path = pointer(&bone_path, timeline.name());
            let known_fields = match timeline.name() {
                "rotate" => Some(&["time", "value", "angle", "curve", "c2", "c3", "c4"][..]),
                "translate" | "scale" | "shear" => {
                    Some(&["time", "x", "y", "curve", "c2", "c3", "c4"][..])
                }
                _ => None,
            };
            if let Some(known_fields) = known_fields
                && retain_timeline_with_unknown_fields(
                    timeline.value(),
                    &timeline_path,
                    known_fields,
                    &format!("bones/{}", timeline.name()),
                    animation_name,
                    animation_index,
                    output,
                    duration,
                    pending,
                )?
            {
                continue;
            }
            match timeline.name() {
                "rotate" => output.push(TimelineData::BoneRotate {
                    bone: bone_index,
                    frames: parse_scalar_frames(
                        timeline.value(),
                        &timeline_path,
                        ScalarKind::Rotation,
                        duration,
                    )?,
                }),
                "translate" => output.push(TimelineData::BoneTranslate {
                    bone: bone_index,
                    frames: parse_vec2_frames(
                        timeline.value(),
                        &timeline_path,
                        Vec2Kind::Translation,
                        duration,
                    )?,
                }),
                "scale" => output.push(TimelineData::BoneScale {
                    bone: bone_index,
                    frames: parse_vec2_frames(
                        timeline.value(),
                        &timeline_path,
                        Vec2Kind::Scale,
                        duration,
                    )?,
                }),
                "shear" => output.push(TimelineData::BoneShear {
                    bone: bone_index,
                    frames: parse_vec2_frames(
                        timeline.value(),
                        &timeline_path,
                        Vec2Kind::Shear,
                        duration,
                    )?,
                }),
                unsupported => {
                    *duration = duration.max(maximum_nested_time(timeline.value()));
                    retain_unsupported(
                        &format!("bones/{unsupported}"),
                        animation_name,
                        animation_index,
                        output,
                        pending,
                    );
                }
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn parse_slot_timelines(
    value: &JsonValue,
    path: &str,
    links: &AnimationLinks<'_>,
    animation_name: &str,
    animation_index: u32,
    output: &mut Vec<TimelineData>,
    duration: &mut f32,
    pending: &mut Vec<PendingDiagnostic>,
) -> Result<(), LoadError> {
    let slots = object(value, path)?;
    unique_members(slots, path)?;
    for slot in slots {
        let slot_path = pointer(path, slot.name());
        let slot_index = links.slots.get(slot.name()).copied().ok_or_else(|| {
            error(
                LoadErrorKind::UnresolvedReference,
                &slot_path,
                format!("animation slot {:?} does not exist", slot.name()),
            )
        })?;
        let timelines = object(slot.value(), &slot_path)?;
        unique_members(timelines, &slot_path)?;
        let mut colour_seen = false;
        for timeline in timelines {
            let timeline_path = pointer(&slot_path, timeline.name());
            let known_fields = match timeline.name() {
                "attachment" => Some(&["time", "name"][..]),
                "rgba" | "color" => Some(&["time", "color", "curve", "c2", "c3", "c4"][..]),
                _ => None,
            };
            if let Some(known_fields) = known_fields
                && retain_timeline_with_unknown_fields(
                    timeline.value(),
                    &timeline_path,
                    known_fields,
                    &format!("slots/{}", timeline.name()),
                    animation_name,
                    animation_index,
                    output,
                    duration,
                    pending,
                )?
            {
                continue;
            }
            match timeline.name() {
                "attachment" => output.push(TimelineData::SlotAttachment {
                    slot: slot_index,
                    frames: parse_attachment_frames(
                        timeline.value(),
                        &timeline_path,
                        slot_index,
                        links,
                        duration,
                    )?,
                }),
                "rgba" | "color" if !colour_seen => {
                    colour_seen = true;
                    output.push(TimelineData::SlotColour {
                        slot: slot_index,
                        frames: parse_colour_frames(timeline.value(), &timeline_path, duration)?,
                    });
                }
                "rgba" | "color" => {
                    return Err(schema_error(
                        &timeline_path,
                        "slot colour is specified by both modern and legacy timelines",
                    ));
                }
                unsupported => {
                    *duration = duration.max(maximum_nested_time(timeline.value()));
                    retain_unsupported(
                        &format!("slots/{unsupported}"),
                        animation_name,
                        animation_index,
                        output,
                        pending,
                    );
                }
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn parse_ik_timelines(
    value: &JsonValue,
    path: &str,
    links: &AnimationLinks<'_>,
    animation_name: &str,
    animation_index: u32,
    output: &mut Vec<TimelineData>,
    duration: &mut f32,
    pending: &mut Vec<PendingDiagnostic>,
) -> Result<(), LoadError> {
    let constraints = object(value, path)?;
    unique_members(constraints, path)?;
    for constraint in constraints {
        let constraint_path = pointer(path, constraint.name());
        let constraint_index = links
            .ik_constraints
            .get(constraint.name())
            .copied()
            .ok_or_else(|| {
                error(
                    LoadErrorKind::UnresolvedReference,
                    &constraint_path,
                    format!(
                        "animation IK constraint {:?} does not exist",
                        constraint.name()
                    ),
                )
            })?;
        if retain_timeline_with_unknown_fields(
            constraint.value(),
            &constraint_path,
            &[
                "time",
                "mix",
                "softness",
                "bendPositive",
                "compress",
                "stretch",
                "curve",
                "c2",
                "c3",
                "c4",
            ],
            "ik/options",
            animation_name,
            animation_index,
            output,
            duration,
            pending,
        )? {
            continue;
        }
        let (frames, advanced) = parse_ik_frames(constraint.value(), &constraint_path, duration)?;
        if advanced {
            retain_unsupported(
                "ik/options",
                animation_name,
                animation_index,
                output,
                pending,
            );
            continue;
        }
        output.push(TimelineData::Ik {
            constraint: constraint_index,
            frames,
        });
    }
    Ok(())
}

enum ScalarKind {
    Rotation,
}

fn parse_scalar_frames(
    value: &JsonValue,
    path: &str,
    kind: ScalarKind,
    duration: &mut f32,
) -> Result<Box<[ScalarFrame]>, LoadError> {
    let values = frame_values(value, path)?;
    let mut frames = Vec::with_capacity(values.len());
    let mut previous = None;
    for (index, value) in values.iter().enumerate() {
        let frame_path = index_pointer(path, index);
        let frame = frame_object(value, &frame_path)?;
        let time = frame_time(frame, &frame_path)?;
        require_strict_time(previous, time, &pointer(&frame_path, "time"))?;
        previous = Some(time);
        *duration = duration.max(time);
        let value = match kind {
            ScalarKind::Rotation => aliased_f32(frame, "value", "angle", &frame_path, 0.0)?,
        };
        frames.push(ScalarFrame {
            time,
            value,
            curve: parse_curve::<1>(frame, &frame_path)?,
        });
    }
    Ok(frames.into_boxed_slice())
}

#[derive(Clone, Copy)]
enum Vec2Kind {
    Translation,
    Scale,
    Shear,
}

fn parse_vec2_frames(
    value: &JsonValue,
    path: &str,
    kind: Vec2Kind,
    duration: &mut f32,
) -> Result<Box<[Vec2Frame]>, LoadError> {
    let values = frame_values(value, path)?;
    let default = match kind {
        Vec2Kind::Translation | Vec2Kind::Shear => 0.0,
        Vec2Kind::Scale => 1.0,
    };
    let mut frames = Vec::with_capacity(values.len());
    let mut previous = None;
    for (index, value) in values.iter().enumerate() {
        let frame_path = index_pointer(path, index);
        let frame = frame_object(value, &frame_path)?;
        let time = frame_time(frame, &frame_path)?;
        require_strict_time(previous, time, &pointer(&frame_path, "time"))?;
        previous = Some(time);
        *duration = duration.max(time);
        frames.push(Vec2Frame {
            time,
            x: f32_or(frame, "x", &frame_path, default)?,
            y: f32_or(frame, "y", &frame_path, default)?,
            curve: parse_curve::<2>(frame, &frame_path)?,
        });
    }
    Ok(frames.into_boxed_slice())
}

fn parse_attachment_frames(
    value: &JsonValue,
    path: &str,
    slot: u32,
    links: &AnimationLinks<'_>,
    duration: &mut f32,
) -> Result<Box<[AttachmentFrame]>, LoadError> {
    let values = frame_values(value, path)?;
    let mut frames = Vec::with_capacity(values.len());
    let mut previous = None;
    for (index, value) in values.iter().enumerate() {
        let frame_path = index_pointer(path, index);
        let frame = frame_object(value, &frame_path)?;
        let time = frame_time(frame, &frame_path)?;
        require_strict_time(previous, time, &pointer(&frame_path, "time"))?;
        previous = Some(time);
        *duration = duration.max(time);
        let name = match member(frame, "name", &frame_path)? {
            None | Some(JsonValue::Null) => None,
            Some(value) => Some(string(value, &pointer(&frame_path, "name"))?),
        };
        if let Some(name) = name {
            let exists = links
                .attachment_names
                .get(&slot)
                .is_some_and(|names| names.contains(name));
            if !exists {
                return Err(error(
                    LoadErrorKind::UnresolvedReference,
                    &pointer(&frame_path, "name"),
                    format!("attachment timeline references unknown attachment {name:?}"),
                ));
            }
        }
        frames.push(AttachmentFrame {
            time,
            name: name.map(Box::from),
        });
    }
    Ok(frames.into_boxed_slice())
}

fn parse_colour_frames(
    value: &JsonValue,
    path: &str,
    duration: &mut f32,
) -> Result<Box<[ColourFrame]>, LoadError> {
    let values = frame_values(value, path)?;
    let mut frames = Vec::with_capacity(values.len());
    let mut previous = None;
    for (index, value) in values.iter().enumerate() {
        let frame_path = index_pointer(path, index);
        let frame = frame_object(value, &frame_path)?;
        let time = frame_time(frame, &frame_path)?;
        require_strict_time(previous, time, &pointer(&frame_path, "time"))?;
        previous = Some(time);
        *duration = duration.max(time);
        let colour_value = required_member(frame, "color", &frame_path)?;
        frames.push(ColourFrame {
            time,
            colour: colour(colour_value, &pointer(&frame_path, "color"))?,
            curve: parse_curve::<4>(frame, &frame_path)?,
        });
    }
    Ok(frames.into_boxed_slice())
}

fn parse_ik_frames(
    value: &JsonValue,
    path: &str,
    duration: &mut f32,
) -> Result<(Box<[IkFrame]>, bool), LoadError> {
    let values = frame_values(value, path)?;
    let mut frames = Vec::with_capacity(values.len());
    let mut previous = None;
    let mut advanced = false;
    for (index, value) in values.iter().enumerate() {
        let frame_path = index_pointer(path, index);
        let frame = frame_object(value, &frame_path)?;
        let time = frame_time(frame, &frame_path)?;
        require_strict_time(previous, time, &pointer(&frame_path, "time"))?;
        previous = Some(time);
        *duration = duration.max(time);
        let mix = f32_or(frame, "mix", &frame_path, 1.0)?;
        if !(0.0..=1.0).contains(&mix) {
            return Err(schema_error(
                &pointer(&frame_path, "mix"),
                "IK key mix must be in the inclusive range 0 through 1",
            ));
        }
        let softness = f32_or(frame, "softness", &frame_path, 0.0)?;
        let compress = bool_or(frame, "compress", &frame_path, false)?;
        let stretch = bool_or(frame, "stretch", &frame_path, false)?;
        advanced |= softness != 0.0 || compress || stretch;
        frames.push(IkFrame {
            time,
            mix,
            bend_positive: bool_or(frame, "bendPositive", &frame_path, true)?,
            curve: parse_curve::<2>(frame, &frame_path)?,
        });
    }
    Ok((frames.into_boxed_slice(), advanced))
}

fn parse_draw_order_frames(
    value: &JsonValue,
    path: &str,
    links: &AnimationLinks<'_>,
    duration: &mut f32,
) -> Result<Box<[DrawOrderFrame]>, LoadError> {
    let values = frame_values(value, path)?;
    let mut frames = Vec::with_capacity(values.len());
    let mut previous = None;
    for (index, value) in values.iter().enumerate() {
        let frame_path = index_pointer(path, index);
        let frame = frame_object(value, &frame_path)?;
        let time = frame_time(frame, &frame_path)?;
        require_strict_time(previous, time, &pointer(&frame_path, "time"))?;
        previous = Some(time);
        *duration = duration.max(time);
        let offsets = match member(frame, "offsets", &frame_path)? {
            None => Box::default(),
            Some(value) => {
                let values = array(value, &pointer(&frame_path, "offsets"))?;
                let mut offsets = Vec::with_capacity(values.len());
                let mut seen_slots = HashSet::with_capacity(values.len());
                let mut seen_destinations = HashSet::with_capacity(values.len());
                let mut previous_slot = None;
                for (offset_index, value) in values.iter().enumerate() {
                    let offset_path = index_pointer(&pointer(&frame_path, "offsets"), offset_index);
                    let offset = frame_object(value, &offset_path)?;
                    let slot_name = nonempty_string(
                        required_member(offset, "slot", &offset_path)?,
                        &pointer(&offset_path, "slot"),
                    )?;
                    let slot = links.slots.get(slot_name).copied().ok_or_else(|| {
                        error(
                            LoadErrorKind::UnresolvedReference,
                            &pointer(&offset_path, "slot"),
                            format!("draw-order slot {slot_name:?} does not exist"),
                        )
                    })?;
                    if !seen_slots.insert(slot) {
                        return Err(error(
                            LoadErrorKind::InvalidOrder,
                            &pointer(&offset_path, "slot"),
                            format!("draw-order slot {slot_name:?} is listed more than once"),
                        ));
                    }
                    if previous_slot.is_some_and(|previous| slot <= previous) {
                        return Err(error(
                            LoadErrorKind::InvalidOrder,
                            &pointer(&offset_path, "slot"),
                            "draw-order offsets must follow setup slot order",
                        ));
                    }
                    previous_slot = Some(slot);
                    let offset_value = i32_value(
                        required_member(offset, "offset", &offset_path)?,
                        &pointer(&offset_path, "offset"),
                    )?;
                    let destination = i64::from(slot) + i64::from(offset_value);
                    if !(0..i64::try_from(links.slots.len()).unwrap_or(i64::MAX))
                        .contains(&destination)
                    {
                        return Err(error(
                            LoadErrorKind::InvalidOrder,
                            &pointer(&offset_path, "offset"),
                            format!(
                                "draw-order destination {destination} is outside the slot table"
                            ),
                        ));
                    }
                    if !seen_destinations.insert(destination) {
                        return Err(error(
                            LoadErrorKind::InvalidOrder,
                            &pointer(&offset_path, "offset"),
                            format!("multiple draw-order offsets target destination {destination}"),
                        ));
                    }
                    offsets.push(DrawOrderOffset {
                        slot,
                        offset: offset_value,
                    });
                }
                offsets.into_boxed_slice()
            }
        };
        frames.push(DrawOrderFrame { time, offsets });
    }
    Ok(frames.into_boxed_slice())
}

fn parse_event_frames(
    value: &JsonValue,
    path: &str,
    links: &AnimationLinks<'_>,
    duration: &mut f32,
) -> Result<Box<[EventFrame]>, LoadError> {
    let values = frame_values(value, path)?;
    let mut frames = Vec::with_capacity(values.len());
    let mut previous = None;
    for (index, value) in values.iter().enumerate() {
        let frame_path = index_pointer(path, index);
        let frame = frame_object(value, &frame_path)?;
        let time = frame_time(frame, &frame_path)?;
        require_nondecreasing_time(previous, time, &pointer(&frame_path, "time"))?;
        previous = Some(time);
        *duration = duration.max(time);
        let name = nonempty_string(
            required_member(frame, "name", &frame_path)?,
            &pointer(&frame_path, "name"),
        )?;
        let event = links.events.get(name).copied().ok_or_else(|| {
            error(
                LoadErrorKind::UnresolvedReference,
                &pointer(&frame_path, "name"),
                format!("animation event {name:?} is not defined"),
            )
        })?;
        let definition = links
            .event_definitions
            .get(event as usize)
            .ok_or_else(|| schema_error(&frame_path, "event definition index is invalid"))?;
        let integer = match member(frame, "int", &frame_path)? {
            None => definition.payload.integer,
            Some(value) => i32_value(value, &pointer(&frame_path, "int"))?,
        };
        let string = match member(frame, "string", &frame_path)? {
            None => definition.payload.string.clone(),
            Some(JsonValue::Null) => None,
            Some(value) => Some(
                string(value, &pointer(&frame_path, "string"))?
                    .to_owned()
                    .into_boxed_str(),
            ),
        };
        frames.push(EventFrame {
            time,
            event,
            payload: EventPayload {
                integer,
                float: f32_or(frame, "float", &frame_path, definition.payload.float)?,
                string,
                volume: f32_or(frame, "volume", &frame_path, definition.payload.volume)?,
                balance: f32_or(frame, "balance", &frame_path, definition.payload.balance)?,
            },
        });
    }
    Ok(frames.into_boxed_slice())
}

fn frame_values<'a>(value: &'a JsonValue, path: &str) -> Result<&'a [JsonValue], LoadError> {
    let values = array(value, path)?;
    if values.is_empty() {
        return Err(schema_error(path, "timeline must contain at least one key"));
    }
    Ok(values)
}

fn frame_object<'a>(value: &'a JsonValue, path: &str) -> Result<&'a [JsonMember], LoadError> {
    let frame = object(value, path)?;
    unique_members(frame, path)?;
    Ok(frame)
}

fn frame_time(frame: &[JsonMember], path: &str) -> Result<f32, LoadError> {
    match member(frame, "time", path)? {
        None => Ok(0.0),
        Some(value) => nonnegative_f32(value, &pointer(path, "time")),
    }
}

fn aliased_f32(
    frame: &[JsonMember],
    modern: &str,
    legacy: &str,
    path: &str,
    default: f32,
) -> Result<f32, LoadError> {
    let modern_value = member(frame, modern, path)?;
    let legacy_value = member(frame, legacy, path)?;
    match (modern_value, legacy_value) {
        (Some(_), Some(_)) => Err(schema_error(
            path,
            format!("key value is specified by both {modern:?} and {legacy:?}"),
        )),
        (Some(value), None) => finite_f32(value, &pointer(path, modern)),
        (None, Some(value)) => finite_f32(value, &pointer(path, legacy)),
        (None, None) => Ok(default),
    }
}

fn parse_curve<const CHANNELS: usize>(
    frame: &[JsonMember],
    path: &str,
) -> Result<FrameCurve<CHANNELS>, LoadError> {
    let curve = member(frame, "curve", path)?;
    let mut separate_control = None;
    for name in ["c2", "c3", "c4"] {
        if member(frame, name, path)?.is_some() {
            separate_control = Some(name);
            break;
        }
    }
    let Some(value) = curve else {
        if let Some(name) = separate_control {
            return Err(schema_error(
                &pointer(path, name),
                format!("{name} requires a numeric curve value"),
            ));
        }
        return Ok(FrameCurve::Linear);
    };
    let curve_path = pointer(path, "curve");
    if !matches!(
        value,
        JsonValue::I64(_) | JsonValue::U64(_) | JsonValue::F64(_)
    ) && let Some(name) = separate_control
    {
        return Err(schema_error(
            &pointer(path, name),
            format!("{name} is only valid with a numeric curve value"),
        ));
    }
    match value {
        JsonValue::String(value) if value.as_ref() == "stepped" => Ok(FrameCurve::Stepped),
        JsonValue::String(value) if value.as_ref() == "linear" => Ok(FrameCurve::Linear),
        JsonValue::Array(values) if values.len() == CHANNELS * 4 => {
            let mut curves = [[0.0; 4]; CHANNELS];
            for (index, value) in values.iter().enumerate() {
                curves[index / 4][index % 4] =
                    finite_f32(value, &index_pointer(&curve_path, index))?;
            }
            Ok(FrameCurve::Bezier(curves))
        }
        JsonValue::I64(_) | JsonValue::U64(_) | JsonValue::F64(_) => {
            let points = [
                finite_f32(value, &curve_path)?,
                f32_or(frame, "c2", path, 0.0)?,
                f32_or(frame, "c3", path, 1.0)?,
                f32_or(frame, "c4", path, 1.0)?,
            ];
            Ok(FrameCurve::Bezier([points; CHANNELS]))
        }
        JsonValue::Array(_) => Err(schema_error(
            &curve_path,
            format!(
                "Bezier curve array must contain {} numbers for this timeline",
                CHANNELS * 4
            ),
        )),
        JsonValue::String(_) => Err(schema_error(
            &curve_path,
            "curve string must be \"linear\" or \"stepped\"",
        )),
        _ => Err(schema_error(
            &curve_path,
            "curve must be a string, number, or numeric array",
        )),
    }
}

fn require_strict_time(previous: Option<f32>, current: f32, path: &str) -> Result<(), LoadError> {
    if previous.is_some_and(|previous| current <= previous) {
        Err(error(
            LoadErrorKind::InvalidOrder,
            path,
            "interpolated timeline key times must be strictly increasing",
        ))
    } else {
        Ok(())
    }
}

fn require_nondecreasing_time(
    previous: Option<f32>,
    current: f32,
    path: &str,
) -> Result<(), LoadError> {
    if previous.is_some_and(|previous| current < previous) {
        Err(error(
            LoadErrorKind::InvalidOrder,
            path,
            "event key times must be nondecreasing",
        ))
    } else {
        Ok(())
    }
}

fn retain_unsupported(
    name: &str,
    animation_name: &str,
    animation_index: u32,
    output: &mut Vec<TimelineData>,
    pending: &mut Vec<PendingDiagnostic>,
) {
    output.push(TimelineData::Unsupported { name: name.into() });
    pending.push(PendingDiagnostic::degraded(
        DiagnosticCode::UnsupportedTimelineType,
        PendingScope::Animation(animation_index),
        format!("animation {animation_name:?} contains unsupported timeline {name:?}"),
    ));
}

#[allow(clippy::too_many_arguments)]
fn retain_unsupported_with_detail(
    name: &str,
    animation_name: &str,
    detail: &str,
    animation_index: u32,
    output: &mut Vec<TimelineData>,
    pending: &mut Vec<PendingDiagnostic>,
) {
    output.push(TimelineData::Unsupported { name: name.into() });
    pending.push(PendingDiagnostic::degraded(
        DiagnosticCode::UnsupportedTimelineType,
        PendingScope::Animation(animation_index),
        format!("animation {animation_name:?} contains unsupported timeline {name:?}: {detail}"),
    ));
}

#[allow(clippy::too_many_arguments)]
fn retain_timeline_with_unknown_fields(
    value: &JsonValue,
    path: &str,
    known: &[&str],
    name: &str,
    animation_name: &str,
    animation_index: u32,
    output: &mut Vec<TimelineData>,
    duration: &mut f32,
    pending: &mut Vec<PendingDiagnostic>,
) -> Result<bool, LoadError> {
    let values = frame_values(value, path)?;
    for (index, value) in values.iter().enumerate() {
        let frame_path = index_pointer(path, index);
        let frame = frame_object(value, &frame_path)?;
        if let Some(unknown) = frame.iter().find(|member| !known.contains(&member.name())) {
            *duration = duration.max(maximum_nested_time(value));
            let unknown_path = pointer(&frame_path, unknown.name());
            retain_unsupported_with_detail(
                name,
                animation_name,
                &format!("unknown field {:?} at {unknown_path}", unknown.name()),
                animation_index,
                output,
                pending,
            );
            return Ok(true);
        }
    }
    Ok(false)
}

#[allow(clippy::too_many_arguments)]
fn retain_draw_order_with_unknown_fields(
    value: &JsonValue,
    path: &str,
    animation_name: &str,
    animation_index: u32,
    output: &mut Vec<TimelineData>,
    duration: &mut f32,
    pending: &mut Vec<PendingDiagnostic>,
) -> Result<bool, LoadError> {
    if retain_timeline_with_unknown_fields(
        value,
        path,
        &["time", "offsets"],
        "drawOrder/options",
        animation_name,
        animation_index,
        output,
        duration,
        pending,
    )? {
        return Ok(true);
    }

    for (frame_index, frame_value) in frame_values(value, path)?.iter().enumerate() {
        let frame_path = index_pointer(path, frame_index);
        let frame = frame_object(frame_value, &frame_path)?;
        let Some(offsets) = member(frame, "offsets", &frame_path)? else {
            continue;
        };
        let offsets_path = pointer(&frame_path, "offsets");
        for (offset_index, offset_value) in array(offsets, &offsets_path)?.iter().enumerate() {
            let offset_path = index_pointer(&offsets_path, offset_index);
            let offset = frame_object(offset_value, &offset_path)?;
            if let Some(unknown) = offset
                .iter()
                .find(|member| !matches!(member.name(), "slot" | "offset"))
            {
                *duration = duration.max(maximum_nested_time(value));
                let unknown_path = pointer(&offset_path, unknown.name());
                retain_unsupported_with_detail(
                    "drawOrder/offsets",
                    animation_name,
                    &format!("unknown field {:?} at {unknown_path}", unknown.name()),
                    animation_index,
                    output,
                    pending,
                );
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn maximum_nested_time(value: &JsonValue) -> f32 {
    match value {
        JsonValue::Array(values) => values.iter().map(maximum_nested_time).fold(0.0, f32::max),
        JsonValue::Object(members) => members.iter().fold(0.0_f32, |maximum, member| {
            let own = if member.name() == "time" {
                member
                    .value()
                    .as_number_f64()
                    .filter(|value| value.is_finite() && *value >= 0.0 && *value <= f32::MAX as f64)
                    .map_or(0.0, |value| value as f32)
            } else {
                0.0
            };
            maximum.max(own).max(maximum_nested_time(member.value()))
        }),
        _ => 0.0,
    }
}

fn ensure_capacity(length: usize, path: &str) -> Result<(), LoadError> {
    if u32::try_from(length).is_ok() {
        Ok(())
    } else {
        Err(error(
            LoadErrorKind::CapacityExceeded,
            path,
            "animation table exceeds the asset-scoped ID representation",
        ))
    }
}

fn index_u32(index: usize, path: &str) -> Result<u32, LoadError> {
    u32::try_from(index).map_err(|_error| {
        error(
            LoadErrorKind::CapacityExceeded,
            path,
            "animation index exceeds the asset-scoped ID representation",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_duration_scan_is_finite_and_nonnegative() {
        let value =
            crate::json::parse_json(br#"{"one":[{"time":0.5},{"time":-2}],"two":{"time":1.25}}"#)
                .expect("valid JSON");
        assert_eq!(maximum_nested_time(&value), 1.25);
    }

    #[test]
    fn curve_arrays_are_typed_by_timeline_channel_count() {
        let values = (0..8)
            .map(|value| JsonValue::F64(f64::from(value)))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let frame = [JsonMember::test_fixture("curve", JsonValue::Array(values))];
        assert!(matches!(
            parse_curve::<2>(&frame, "/frame").expect("two-channel curve"),
            FrameCurve::Bezier(curves) if curves.len() == 2
        ));
        assert!(parse_curve::<1>(&frame, "/frame").is_err());
    }

    #[test]
    fn compact_numeric_curves_use_documented_control_point_defaults() {
        let frame = [JsonMember::test_fixture("curve", JsonValue::F64(0.25))];
        assert_eq!(
            parse_curve::<2>(&frame, "/frame").expect("documented defaults"),
            FrameCurve::Bezier([[0.25, 0.0, 1.0, 1.0]; 2])
        );
    }
}

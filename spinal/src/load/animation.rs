use std::collections::{HashMap, HashSet};

use crate::{
    BendDirection, DiagnosticCode, Mix, Rgba, TransformMix,
    animation::{
        AnimationData, AttachmentFrame, ColourFrame, DrawOrderFrame, DrawOrderOffset,
        EventDefinitionData, EventFrame, EventPayload, FrameCurve, IkFrame, NANOS_PER_SECOND,
        ScalarFrame, TimelineData, TimelineTime, TransformFrame, Vec2Frame,
        animation_deferred_override_properties, animation_properties, transform_pose_values,
    },
    asset::{IkConstraintData, TransformConstraintData, TransformConstraintPoseData},
    json::{JsonMember, JsonValue},
};

use super::{
    LoadError, LoadErrorKind, PendingDiagnostic, PendingDiagnostics, PendingScope,
    schema::{
        array, bool_or, colour, error, f32_or, finite_f32, i32_value, index_pointer, member,
        nonempty_string, object, pointer, required_member, schema_error, string, unique_members,
    },
};

pub(crate) struct AnimationLinks<'a> {
    pub(crate) bones: &'a HashMap<Box<str>, u32>,
    pub(crate) slots: &'a HashMap<Box<str>, u32>,
    pub(crate) ik_constraints: &'a HashMap<Box<str>, u32>,
    pub(crate) ik_constraint_data: &'a [IkConstraintData],
    pub(crate) transform_constraints: &'a HashMap<Box<str>, u32>,
    pub(crate) transform_constraint_data: &'a [TransformConstraintData],
    pub(crate) events: &'a HashMap<Box<str>, u32>,
    pub(crate) event_definitions: &'a [EventDefinitionData],
    pub(crate) attachment_names: &'a HashMap<u32, HashSet<Box<str>>>,
}

pub(crate) fn parse_animations(
    value: Option<&JsonValue>,
    links: AnimationLinks<'_>,
    pending: &mut PendingDiagnostics,
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
        let mut duration = TimelineTime::ZERO;

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
        if let Some(value) = member(data, "transform", &path)? {
            parse_transform_timelines(
                value,
                &pointer(&path, "transform"),
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
                "bones" | "slots" | "ik" | "transform" | "drawOrder" | "draworder" | "events"
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

        let properties = animation_properties(&timelines);
        let deferred_override_properties =
            animation_deferred_override_properties(&timelines, links.ik_constraint_data);
        output.push(AnimationData {
            name: animation.name().into(),
            duration,
            timelines: timelines.into_boxed_slice(),
            properties,
            deferred_override_properties,
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
    duration: &mut TimelineTime,
    pending: &mut PendingDiagnostics,
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
                "translatex" | "translatey" | "scalex" | "scaley" | "shearx" | "sheary" => {
                    Some(&["time", "value", "curve", "c2", "c3", "c4"][..])
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
                "translatex" => output.push(TimelineData::BoneTranslateX {
                    bone: bone_index,
                    frames: parse_scalar_frames(
                        timeline.value(),
                        &timeline_path,
                        ScalarKind::Translation,
                        duration,
                    )?,
                }),
                "translatey" => output.push(TimelineData::BoneTranslateY {
                    bone: bone_index,
                    frames: parse_scalar_frames(
                        timeline.value(),
                        &timeline_path,
                        ScalarKind::Translation,
                        duration,
                    )?,
                }),
                "scalex" => output.push(TimelineData::BoneScaleX {
                    bone: bone_index,
                    frames: parse_scalar_frames(
                        timeline.value(),
                        &timeline_path,
                        ScalarKind::Scale,
                        duration,
                    )?,
                }),
                "scaley" => output.push(TimelineData::BoneScaleY {
                    bone: bone_index,
                    frames: parse_scalar_frames(
                        timeline.value(),
                        &timeline_path,
                        ScalarKind::Scale,
                        duration,
                    )?,
                }),
                "shearx" => output.push(TimelineData::BoneShearX {
                    bone: bone_index,
                    frames: parse_scalar_frames(
                        timeline.value(),
                        &timeline_path,
                        ScalarKind::Shear,
                        duration,
                    )?,
                }),
                "sheary" => output.push(TimelineData::BoneShearY {
                    bone: bone_index,
                    frames: parse_scalar_frames(
                        timeline.value(),
                        &timeline_path,
                        ScalarKind::Shear,
                        duration,
                    )?,
                }),
                unsupported => {
                    *duration = (*duration).max(maximum_nested_time(timeline.value()));
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
    duration: &mut TimelineTime,
    pending: &mut PendingDiagnostics,
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
                    *duration = (*duration).max(maximum_nested_time(timeline.value()));
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
    duration: &mut TimelineTime,
    pending: &mut PendingDiagnostics,
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

#[allow(clippy::too_many_arguments)]
fn parse_transform_timelines(
    value: &JsonValue,
    path: &str,
    links: &AnimationLinks<'_>,
    animation_name: &str,
    animation_index: u32,
    output: &mut Vec<TimelineData>,
    duration: &mut TimelineTime,
    pending: &mut PendingDiagnostics,
) -> Result<(), LoadError> {
    let constraints = object(value, path)?;
    unique_members(constraints, path)?;
    for constraint in constraints {
        let constraint_path = pointer(path, constraint.name());
        let constraint_index = links
            .transform_constraints
            .get(constraint.name())
            .copied()
            .ok_or_else(|| {
                error(
                    LoadErrorKind::UnresolvedReference,
                    &constraint_path,
                    format!(
                        "animation transform constraint {:?} does not exist",
                        constraint.name()
                    ),
                )
            })?;
        if retain_timeline_with_unknown_fields(
            constraint.value(),
            &constraint_path,
            &[
                "time",
                "mixRotate",
                "mixX",
                "mixY",
                "mixScaleX",
                "mixScaleY",
                "mixShearY",
                "curve",
                "c2",
                "c3",
                "c4",
            ],
            "transform/options",
            animation_name,
            animation_index,
            output,
            duration,
            pending,
        )? {
            continue;
        }
        let setup = links
            .transform_constraint_data
            .get(constraint_index as usize)
            .ok_or_else(|| {
                schema_error(
                    &constraint_path,
                    "transform constraint setup-pose link is invalid",
                )
            })?
            .setup_pose;
        output.push(TimelineData::Transform {
            constraint: constraint_index,
            frames: parse_transform_frames(constraint.value(), &constraint_path, setup, duration)?,
        });
    }
    Ok(())
}

enum ScalarKind {
    Rotation,
    Translation,
    Scale,
    Shear,
}

fn parse_scalar_frames(
    value: &JsonValue,
    path: &str,
    kind: ScalarKind,
    duration: &mut TimelineTime,
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
        *duration = (*duration).max(time);
        let value = match kind {
            ScalarKind::Rotation => aliased_f32(frame, "value", "angle", &frame_path, 0.0)?,
            ScalarKind::Translation | ScalarKind::Shear => {
                f32_or(frame, "value", &frame_path, 0.0)?
            }
            ScalarKind::Scale => f32_or(frame, "value", &frame_path, 1.0)?,
        };
        frames.push(ScalarFrame {
            time,
            value,
            curve: FrameCurve::Linear,
        });
    }
    for (index, value) in values.iter().enumerate() {
        let frame_path = index_pointer(path, index);
        let frame = frame_object(value, &frame_path)?;
        let coordinates = AbsoluteCurve {
            start_time: frames[index].time,
            start_values: [frames[index].value],
            end: frames.get(index + 1).map(|next| (next.time, [next.value])),
        };
        frames[index].curve = parse_curve(frame, &frame_path, coordinates)?;
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
    duration: &mut TimelineTime,
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
        *duration = (*duration).max(time);
        frames.push(Vec2Frame {
            time,
            x: f32_or(frame, "x", &frame_path, default)?,
            y: f32_or(frame, "y", &frame_path, default)?,
            curve: FrameCurve::Linear,
        });
    }
    for (index, value) in values.iter().enumerate() {
        let frame_path = index_pointer(path, index);
        let frame = frame_object(value, &frame_path)?;
        let coordinates = AbsoluteCurve {
            start_time: frames[index].time,
            start_values: [frames[index].x, frames[index].y],
            end: frames
                .get(index + 1)
                .map(|next| (next.time, [next.x, next.y])),
        };
        frames[index].curve = parse_curve(frame, &frame_path, coordinates)?;
    }
    Ok(frames.into_boxed_slice())
}

fn parse_attachment_frames(
    value: &JsonValue,
    path: &str,
    slot: u32,
    links: &AnimationLinks<'_>,
    duration: &mut TimelineTime,
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
        *duration = (*duration).max(time);
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
            placeholder_name: name.map(Box::from),
        });
    }
    Ok(frames.into_boxed_slice())
}

fn parse_colour_frames(
    value: &JsonValue,
    path: &str,
    duration: &mut TimelineTime,
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
        *duration = (*duration).max(time);
        let colour_value = required_member(frame, "color", &frame_path)?;
        frames.push(ColourFrame {
            time,
            colour: colour(colour_value, &pointer(&frame_path, "color"))?,
            curve: FrameCurve::Linear,
        });
    }
    for (index, value) in values.iter().enumerate() {
        let frame_path = index_pointer(path, index);
        let frame = frame_object(value, &frame_path)?;
        let coordinates = AbsoluteCurve {
            start_time: frames[index].time,
            start_values: Rgba::from_rgba8(frames[index].colour).to_array(),
            end: frames
                .get(index + 1)
                .map(|next| (next.time, Rgba::from_rgba8(next.colour).to_array())),
        };
        frames[index].curve = parse_curve(frame, &frame_path, coordinates)?;
    }
    Ok(frames.into_boxed_slice())
}

fn parse_ik_frames(
    value: &JsonValue,
    path: &str,
    duration: &mut TimelineTime,
) -> Result<(Box<[IkFrame]>, bool), LoadError> {
    let values = frame_values(value, path)?;
    let mut frames = Vec::with_capacity(values.len());
    let mut curve_values = Vec::with_capacity(values.len());
    let mut previous = None;
    let mut advanced = false;
    for (index, value) in values.iter().enumerate() {
        let frame_path = index_pointer(path, index);
        let frame = frame_object(value, &frame_path)?;
        let time = frame_time(frame, &frame_path)?;
        require_strict_time(previous, time, &pointer(&frame_path, "time"))?;
        previous = Some(time);
        *duration = (*duration).max(time);
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
        curve_values.push([mix, softness]);
        frames.push(IkFrame {
            time,
            mix: Mix::new(mix)
                .expect("IK mix was validated to be finite and in the inclusive unit range"),
            bend_direction: if bool_or(frame, "bendPositive", &frame_path, true)? {
                BendDirection::Positive
            } else {
                BendDirection::Negative
            },
            curve: FrameCurve::Linear,
        });
    }
    for (index, value) in values.iter().enumerate() {
        let frame_path = index_pointer(path, index);
        let frame = frame_object(value, &frame_path)?;
        let coordinates = AbsoluteCurve {
            start_time: frames[index].time,
            start_values: curve_values[index],
            end: frames
                .get(index + 1)
                .map(|next| (next.time, curve_values[index + 1])),
        };
        frames[index].curve = parse_curve(frame, &frame_path, coordinates)?;
    }
    Ok((frames.into_boxed_slice(), advanced))
}

fn parse_transform_frames(
    value: &JsonValue,
    path: &str,
    setup: TransformConstraintPoseData,
    duration: &mut TimelineTime,
) -> Result<Box<[TransformFrame]>, LoadError> {
    let values = frame_values(value, path)?;
    let setup = transform_pose_values(setup);
    let mut frames = Vec::with_capacity(values.len());
    let mut previous = None;
    for (index, value) in values.iter().enumerate() {
        let frame_path = index_pointer(path, index);
        let frame = frame_object(value, &frame_path)?;
        let time = frame_time(frame, &frame_path)?;
        require_strict_time(previous, time, &pointer(&frame_path, "time"))?;
        previous = Some(time);
        *duration = (*duration).max(time);
        let values = [
            f32_or(frame, "mixRotate", &frame_path, setup[0])?,
            f32_or(frame, "mixX", &frame_path, setup[1])?,
            transform_frame_value(frame, "mixY", "mixX", &frame_path, setup[2])?,
            f32_or(frame, "mixScaleX", &frame_path, setup[3])?,
            transform_frame_value(frame, "mixScaleY", "mixScaleX", &frame_path, setup[4])?,
            f32_or(frame, "mixShearY", &frame_path, setup[5])?,
        ];
        frames.push(TransformFrame {
            time,
            pose: transform_pose(values, &frame_path)?,
            curve: FrameCurve::Linear,
        });
    }
    for (index, value) in values.iter().enumerate() {
        let frame_path = index_pointer(path, index);
        let frame = frame_object(value, &frame_path)?;
        let coordinates = AbsoluteCurve {
            start_time: frames[index].time,
            start_values: transform_pose_values(frames[index].pose),
            end: frames
                .get(index + 1)
                .map(|next| (next.time, transform_pose_values(next.pose))),
        };
        frames[index].curve = parse_curve(frame, &frame_path, coordinates)?;
    }
    Ok(frames.into_boxed_slice())
}

fn transform_frame_value(
    frame: &[JsonMember],
    field: &str,
    fallback: &str,
    path: &str,
    default: f32,
) -> Result<f32, LoadError> {
    match member(frame, field, path)? {
        Some(value) => finite_f32(value, &pointer(path, field)),
        None => match member(frame, fallback, path)? {
            Some(value) => finite_f32(value, &pointer(path, fallback)),
            None => Ok(default),
        },
    }
}

fn transform_pose(values: [f32; 6], path: &str) -> Result<TransformConstraintPoseData, LoadError> {
    let mix = |value, field| {
        TransformMix::new(value).map_err(|_error| {
            schema_error(
                &pointer(path, field),
                "transform constraint mix must be finite",
            )
        })
    };
    Ok(TransformConstraintPoseData {
        mix_rotate: mix(values[0], "mixRotate")?,
        mix_x: mix(values[1], "mixX")?,
        mix_y: mix(values[2], "mixY")?,
        mix_scale_x: mix(values[3], "mixScaleX")?,
        mix_scale_y: mix(values[4], "mixScaleY")?,
        mix_shear_y: mix(values[5], "mixShearY")?,
    })
}

fn parse_draw_order_frames(
    value: &JsonValue,
    path: &str,
    links: &AnimationLinks<'_>,
    duration: &mut TimelineTime,
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
        *duration = (*duration).max(time);
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
    duration: &mut TimelineTime,
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
        *duration = (*duration).max(time);
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

fn frame_time(frame: &[JsonMember], path: &str) -> Result<TimelineTime, LoadError> {
    match member(frame, "time", path)? {
        None => Ok(TimelineTime::ZERO),
        Some(value) => {
            let time_path = pointer(path, "time");
            let seconds = value.as_number_f64().ok_or_else(|| {
                error(
                    LoadErrorKind::SchemaViolation,
                    &time_path,
                    "key time must be a number",
                )
            })?;
            TimelineTime::from_seconds_f64(seconds).ok_or_else(|| {
                error(
                    LoadErrorKind::NonFiniteNumber,
                    &time_path,
                    "key time must be finite, nonnegative, and representable in nanoseconds",
                )
            })
        }
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

#[derive(Clone, Copy)]
struct AbsoluteCurve<const CHANNELS: usize> {
    start_time: TimelineTime,
    start_values: [f32; CHANNELS],
    end: Option<(TimelineTime, [f32; CHANNELS])>,
}

fn parse_curve<const CHANNELS: usize>(
    frame: &[JsonMember],
    path: &str,
    coordinates: AbsoluteCurve<CHANNELS>,
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
            normalize_absolute_curve_time(&mut curves, coordinates, &curve_path)?;
            Ok(FrameCurve::Bezier(curves))
        }
        JsonValue::I64(_) | JsonValue::U64(_) | JsonValue::F64(_) => {
            let points = [
                finite_f32(value, &curve_path)?,
                f32_or(frame, "c2", path, 0.0)?,
                f32_or(frame, "c3", path, 1.0)?,
                f32_or(frame, "c4", path, 1.0)?,
            ];
            let mut curves = [points; CHANNELS];
            validate_curve_x(&curves, &curve_path)?;
            denormalize_curve_values(&mut curves, coordinates, path)?;
            Ok(FrameCurve::Bezier(curves))
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

fn normalize_absolute_curve_time<const CHANNELS: usize>(
    curves: &mut [[f32; 4]; CHANNELS],
    coordinates: AbsoluteCurve<CHANNELS>,
    path: &str,
) -> Result<(), LoadError> {
    let Some((end_time, _end_values)) = coordinates.end else {
        for [x1, _y1, x2, _y2] in curves {
            *x1 = 0.0;
            *x2 = 1.0;
        }
        return Ok(());
    };
    let start_time = coordinates.start_time.ticks as f64 / NANOS_PER_SECOND as f64;
    let end_time = end_time.ticks as f64 / NANOS_PER_SECOND as f64;
    let time_span = end_time - start_time;
    for (channel, [x1, _y1, x2, _y2]) in curves.iter_mut().enumerate() {
        *x1 = checked_curve_f32(
            (f64::from(*x1) - start_time) / time_span,
            &index_pointer(path, channel * 4),
        )?;
        *x2 = checked_curve_f32(
            (f64::from(*x2) - start_time) / time_span,
            &index_pointer(path, channel * 4 + 2),
        )?;
    }
    Ok(())
}

fn denormalize_curve_values<const CHANNELS: usize>(
    curves: &mut [[f32; 4]; CHANNELS],
    coordinates: AbsoluteCurve<CHANNELS>,
    path: &str,
) -> Result<(), LoadError> {
    for (channel, [_x1, y1, _x2, y2]) in curves.iter_mut().enumerate() {
        let start = f64::from(coordinates.start_values[channel]);
        let end = coordinates
            .end
            .map_or(start, |(_time, values)| f64::from(values[channel]));
        let span = end - start;
        *y1 = checked_curve_f32(start + span * f64::from(*y1), &pointer(path, "c2"))?;
        *y2 = checked_curve_f32(start + span * f64::from(*y2), &pointer(path, "c4"))?;
    }
    Ok(())
}

fn checked_curve_f32(value: f64, path: &str) -> Result<f32, LoadError> {
    if value.is_finite() && value.abs() <= f64::from(f32::MAX) {
        Ok(value as f32)
    } else {
        Err(error(
            LoadErrorKind::NonFiniteNumber,
            path,
            "Bezier control conversion must remain finite and representable",
        ))
    }
}

fn validate_curve_x<const CHANNELS: usize>(
    curves: &[[f32; 4]; CHANNELS],
    path: &str,
) -> Result<(), LoadError> {
    if curves
        .iter()
        .all(|[x1, _y1, x2, _y2]| (0.0..=1.0).contains(x1) && (0.0..=1.0).contains(x2))
    {
        Ok(())
    } else {
        Err(schema_error(
            path,
            "Bezier X control points must be in the inclusive range 0 through 1",
        ))
    }
}

fn require_strict_time(
    previous: Option<TimelineTime>,
    current: TimelineTime,
    path: &str,
) -> Result<(), LoadError> {
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
    previous: Option<TimelineTime>,
    current: TimelineTime,
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
    pending: &mut PendingDiagnostics,
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
    pending: &mut PendingDiagnostics,
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
    duration: &mut TimelineTime,
    pending: &mut PendingDiagnostics,
) -> Result<bool, LoadError> {
    let values = frame_values(value, path)?;
    for (index, value) in values.iter().enumerate() {
        let frame_path = index_pointer(path, index);
        let frame = frame_object(value, &frame_path)?;
        if let Some(unknown) = frame.iter().find(|member| !known.contains(&member.name())) {
            *duration = (*duration).max(maximum_nested_time(value));
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
    duration: &mut TimelineTime,
    pending: &mut PendingDiagnostics,
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
                *duration = (*duration).max(maximum_nested_time(value));
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

fn maximum_nested_time(value: &JsonValue) -> TimelineTime {
    match value {
        JsonValue::Array(values) => values
            .iter()
            .map(maximum_nested_time)
            .fold(TimelineTime::ZERO, TimelineTime::max),
        JsonValue::Object(members) => members.iter().fold(TimelineTime::ZERO, |maximum, member| {
            let own = if member.name() == "time" {
                member
                    .value()
                    .as_number_f64()
                    .and_then(TimelineTime::from_seconds_f64)
                    .unwrap_or(TimelineTime::ZERO)
            } else {
                TimelineTime::ZERO
            };
            maximum.max(own).max(maximum_nested_time(member.value()))
        }),
        _ => TimelineTime::ZERO,
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

    fn coordinates<const CHANNELS: usize>(
        start_values: [f32; CHANNELS],
        end_values: [f32; CHANNELS],
    ) -> AbsoluteCurve<CHANNELS> {
        AbsoluteCurve {
            start_time: TimelineTime::ZERO,
            start_values,
            end: Some((
                TimelineTime::from_seconds_f64(1.0).expect("valid time"),
                end_values,
            )),
        }
    }

    #[test]
    fn nested_duration_scan_is_finite_and_nonnegative() {
        let value =
            crate::json::parse_json(br#"{"one":[{"time":0.5},{"time":-2}],"two":{"time":1.25}}"#)
                .expect("valid JSON");
        assert_eq!(
            maximum_nested_time(&value),
            TimelineTime::from_seconds_f64(1.25).expect("representable time")
        );
    }

    #[test]
    fn curve_arrays_are_typed_by_timeline_channel_count() {
        let values = [0.1, 0.2, 0.8, 0.9, 0.1, 0.2, 0.8, 0.9]
            .into_iter()
            .map(JsonValue::F64)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let frame = [JsonMember::test_fixture("curve", JsonValue::Array(values))];
        assert!(matches!(
            parse_curve(
                &frame,
                "/frame",
                coordinates([0.0, 0.0], [1.0, 1.0])
            )
            .expect("two-channel curve"),
            FrameCurve::Bezier(curves) if curves.len() == 2
        ));
        assert!(parse_curve(&frame, "/frame", coordinates([0.0], [1.0])).is_err());
    }

    #[test]
    fn compact_numeric_curves_use_documented_control_point_defaults() {
        let frame = [JsonMember::test_fixture("curve", JsonValue::F64(0.25))];
        assert_eq!(
            parse_curve(&frame, "/frame", coordinates([10.0, 20.0], [20.0, 40.0]))
                .expect("documented defaults"),
            FrameCurve::Bezier([[0.25, 10.0, 1.0, 20.0], [0.25, 20.0, 1.0, 40.0]])
        );
    }

    #[test]
    fn compact_bezier_x_control_points_stay_in_the_documented_time_domain() {
        let frame = [
            JsonMember::test_fixture("curve", JsonValue::F64(-0.1)),
            JsonMember::test_fixture("c3", JsonValue::F64(1.0)),
        ];
        assert!(parse_curve(&frame, "/frame", coordinates([0.0], [1.0])).is_err());
    }

    #[test]
    fn absolute_bone_curve_coordinates_are_normalized_between_keys() {
        let values = [2.388_889, -18.877_235, 2.544_444_6, -1.956_676_5]
            .into_iter()
            .map(JsonValue::F64)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let frame = [JsonMember::test_fixture("curve", JsonValue::Array(values))];
        let coordinates = AbsoluteCurve {
            start_time: TimelineTime::from_seconds_f64(2.233_333).expect("valid time"),
            start_values: [-18.877_235],
            end: Some((
                TimelineTime::from_seconds_f64(2.566_667).expect("valid time"),
                [-1.956_676_5],
            )),
        };
        let curve = parse_curve::<1>(&frame, "/frame", coordinates).expect("exact export curve");
        let FrameCurve::Bezier([curve]) = curve else {
            panic!("array curve must remain Bézier");
        };
        assert!((curve[0] - 0.466_667).abs() < 1.0e-5);
        assert!((curve[1] + 18.877_235).abs() < 1.0e-5);
        assert!((curve[2] - 0.933_333).abs() < 1.0e-5);
        assert!((curve[3] + 1.956_676_5).abs() < 1.0e-5);
    }

    #[test]
    fn absolute_bone_curve_handles_may_cross_a_key_time() {
        let values = [0.643_353_3, 2.774_735, 0.622_348_8, -39.428_56]
            .into_iter()
            .map(JsonValue::F64)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let frame = [JsonMember::test_fixture("curve", JsonValue::Array(values))];
        let coordinates = AbsoluteCurve {
            start_time: TimelineTime::from_seconds_f64(0.633_333_3).expect("valid time"),
            start_values: [9.047_508],
            end: Some((
                TimelineTime::from_seconds_f64(0.7).expect("valid time"),
                [-39.499_33],
            )),
        };
        let curve = parse_curve::<1>(&frame, "/frame", coordinates).expect("exact export curve");
        let FrameCurve::Bezier([curve]) = curve else {
            panic!("array curve must remain Bézier");
        };
        assert!(curve[0] > 0.0);
        assert!(curve[2] < 0.0);
        assert!(curve.into_iter().all(f32::is_finite));
    }
}

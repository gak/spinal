use std::collections::BTreeMap;

use serde_json::{Map, Number, Value};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TranslationKey {
    pub(crate) time: f32,
    pub(crate) x: f32,
    pub(crate) y: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TranslationTimeline<'a> {
    pub(crate) bone: &'a str,
    pub(crate) keys: &'a [TranslationKey],
}

#[derive(Debug, Error, PartialEq)]
pub(crate) enum JsonEditError {
    #[error("skeleton JSON is not valid JSON: {0}")]
    InvalidJson(String),
    #[error("skeleton JSON is not a top-level object")]
    InvalidRoot,
    #[error("JSON member `{0}` occurs more than once")]
    DuplicateMember(String),
    #[error("animation name must not be empty")]
    EmptyAnimationName,
    #[error("bone name must not be empty")]
    EmptyBoneName,
    #[error("bone `{0}` occurs more than once in the draft")]
    DuplicateBone(String),
    #[error("bone `{bone}` has no translation keys")]
    EmptyTimeline { bone: String },
    #[error("bone `{bone}` has a non-finite key at index {key}")]
    NonFiniteKey { bone: String, key: usize },
    #[error("bone `{bone}` key times must be non-negative and strictly increasing")]
    InvalidKeyOrder { bone: String },
    #[error("generated animation is not a JSON object")]
    InvalidAnimation,
    #[error("skeleton JSON has malformed object syntax")]
    MalformedObject,
}

pub(crate) fn encode_translation_animation(
    timelines: &[TranslationTimeline<'_>],
) -> Result<String, JsonEditError> {
    let mut ordered = BTreeMap::new();
    for timeline in timelines {
        if timeline.bone.is_empty() {
            return Err(JsonEditError::EmptyBoneName);
        }
        if timeline.keys.is_empty() {
            return Err(JsonEditError::EmptyTimeline {
                bone: timeline.bone.to_owned(),
            });
        }
        if ordered.contains_key(timeline.bone) {
            return Err(JsonEditError::DuplicateBone(timeline.bone.to_owned()));
        }
        let mut previous_time = None;
        let mut frames = Vec::with_capacity(timeline.keys.len());
        for (index, key) in timeline.keys.iter().copied().enumerate() {
            if !key.time.is_finite() || !key.x.is_finite() || !key.y.is_finite() {
                return Err(JsonEditError::NonFiniteKey {
                    bone: timeline.bone.to_owned(),
                    key: index,
                });
            }
            if key.time < 0.0 || previous_time.is_some_and(|previous| key.time <= previous) {
                return Err(JsonEditError::InvalidKeyOrder {
                    bone: timeline.bone.to_owned(),
                });
            }
            previous_time = Some(key.time);

            let mut frame = Map::new();
            if key.time != 0.0 {
                frame.insert("time".to_owned(), finite_number(key.time));
            }
            frame.insert("x".to_owned(), finite_number(key.x));
            frame.insert("y".to_owned(), finite_number(key.y));
            frames.push(Value::Object(frame));
        }
        ordered.insert(timeline.bone, Value::Array(frames));
    }

    let bones = ordered
        .into_iter()
        .map(|(bone, frames)| {
            let mut timelines = Map::new();
            timelines.insert("translate".to_owned(), frames);
            (bone.to_owned(), Value::Object(timelines))
        })
        .collect::<Map<_, _>>();
    let mut animation = Map::new();
    animation.insert("bones".to_owned(), Value::Object(bones));
    serde_json::to_string_pretty(&Value::Object(animation))
        .map_err(|error| JsonEditError::InvalidJson(error.to_string()))
}

pub(crate) fn upsert_animation(
    source: &str,
    animation_name: &str,
    animation_json: &str,
) -> Result<String, JsonEditError> {
    if animation_name.is_empty() {
        return Err(JsonEditError::EmptyAnimationName);
    }
    let root: Value = serde_json::from_str(source)
        .map_err(|error| JsonEditError::InvalidJson(error.to_string()))?;
    if !root.is_object() {
        return Err(JsonEditError::InvalidRoot);
    }
    let animation: Value = serde_json::from_str(animation_json)
        .map_err(|error| JsonEditError::InvalidJson(error.to_string()))?;
    if !animation.is_object() {
        return Err(JsonEditError::InvalidAnimation);
    }

    let root_object = parse_object(source, 0)?;
    let animations = unique_member(&root_object, "animations")?;
    match animations {
        Some(animations) => {
            let animations_object = parse_object(source, animations.value_start)?;
            if animations_object.end != animations.value_end {
                return Err(JsonEditError::MalformedObject);
            }
            upsert_object_member(source, &animations_object, animation_name, animation_json)
        }
        None => upsert_object_member(
            source,
            &root_object,
            "animations",
            &format!(
                "{{\n  {}: {}\n}}",
                serde_json::to_string(animation_name)
                    .map_err(|error| JsonEditError::InvalidJson(error.to_string()))?,
                indent_after_first_line(animation_json, "  ")
            ),
        ),
    }
}

fn finite_number(value: f32) -> Value {
    Value::Number(Number::from_f64(f64::from(value)).expect("validated finite f32"))
}

#[derive(Clone, Debug)]
struct ObjectMember {
    name: String,
    key_start: usize,
    value_start: usize,
    value_end: usize,
}

#[derive(Clone, Debug)]
struct ObjectSpan {
    end: usize,
    close: usize,
    members: Vec<ObjectMember>,
}

fn upsert_object_member(
    source: &str,
    object: &ObjectSpan,
    name: &str,
    value: &str,
) -> Result<String, JsonEditError> {
    if let Some(member) = unique_member(object, name)? {
        let indent = line_indent(source, member.key_start);
        let replacement = indent_after_first_line(value, indent);
        let mut output = String::with_capacity(
            source.len() - (member.value_end - member.value_start) + replacement.len(),
        );
        output.push_str(&source[..member.value_start]);
        output.push_str(&replacement);
        output.push_str(&source[member.value_end..]);
        return Ok(output);
    }

    let closing_indent = line_indent(source, object.close);
    let member_indent = object.members.first().map_or_else(
        || format!("{closing_indent}\t"),
        |member| line_indent(source, member.key_start).to_owned(),
    );
    let encoded_name = serde_json::to_string(name)
        .map_err(|error| JsonEditError::InvalidJson(error.to_string()))?;
    let formatted_value = indent_after_first_line(value, &member_indent);
    let comma = if object.members.is_empty() { "" } else { "," };
    let insertion =
        format!("{comma}\n{member_indent}{encoded_name}: {formatted_value}\n{closing_indent}");
    let mut output = String::with_capacity(source.len() + insertion.len());
    output.push_str(&source[..object.close]);
    output.push_str(&insertion);
    output.push_str(&source[object.close..]);
    Ok(output)
}

fn indent_after_first_line(value: &str, indent: &str) -> String {
    value.replace('\n', &format!("\n{indent}"))
}

fn line_indent(source: &str, offset: usize) -> &str {
    let line_start = source[..offset]
        .rfind('\n')
        .map_or(0, |newline| newline + 1);
    let prefix = &source[line_start..offset];
    if prefix.chars().all(char::is_whitespace) {
        prefix
    } else {
        ""
    }
}

fn unique_member<'a>(
    object: &'a ObjectSpan,
    name: &str,
) -> Result<Option<&'a ObjectMember>, JsonEditError> {
    let mut matches = object.members.iter().filter(|member| member.name == name);
    let first = matches.next();
    if matches.next().is_some() {
        return Err(JsonEditError::DuplicateMember(name.to_owned()));
    }
    Ok(first)
}

fn parse_object(source: &str, start: usize) -> Result<ObjectSpan, JsonEditError> {
    let bytes = source.as_bytes();
    let start = skip_whitespace(bytes, start);
    if bytes.get(start) != Some(&b'{') {
        return Err(JsonEditError::MalformedObject);
    }
    let mut cursor = skip_whitespace(bytes, start + 1);
    let mut members = Vec::new();
    if bytes.get(cursor) == Some(&b'}') {
        return Ok(ObjectSpan {
            end: cursor + 1,
            close: cursor,
            members,
        });
    }

    loop {
        let key_start = cursor;
        let key_end = skip_string(bytes, key_start)?;
        let name: String = serde_json::from_str(&source[key_start..key_end])
            .map_err(|error| JsonEditError::InvalidJson(error.to_string()))?;
        cursor = skip_whitespace(bytes, key_end);
        if bytes.get(cursor) != Some(&b':') {
            return Err(JsonEditError::MalformedObject);
        }
        let value_start = skip_whitespace(bytes, cursor + 1);
        let value_end = skip_value(bytes, value_start)?;
        members.push(ObjectMember {
            name,
            key_start,
            value_start,
            value_end,
        });
        cursor = skip_whitespace(bytes, value_end);
        match bytes.get(cursor) {
            Some(b',') => cursor = skip_whitespace(bytes, cursor + 1),
            Some(b'}') => {
                return Ok(ObjectSpan {
                    end: cursor + 1,
                    close: cursor,
                    members,
                });
            }
            _other => return Err(JsonEditError::MalformedObject),
        }
    }
}

fn skip_value(bytes: &[u8], start: usize) -> Result<usize, JsonEditError> {
    match bytes.get(start) {
        Some(b'"') => skip_string(bytes, start),
        Some(b'{') => skip_container(bytes, start, b'{', b'}'),
        Some(b'[') => skip_container(bytes, start, b'[', b']'),
        Some(_other) => {
            let mut cursor = start;
            while let Some(byte) = bytes.get(cursor) {
                if byte.is_ascii_whitespace() || matches!(byte, b',' | b'}' | b']') {
                    break;
                }
                cursor += 1;
            }
            (cursor > start)
                .then_some(cursor)
                .ok_or(JsonEditError::MalformedObject)
        }
        None => Err(JsonEditError::MalformedObject),
    }
}

fn skip_container(bytes: &[u8], start: usize, open: u8, close: u8) -> Result<usize, JsonEditError> {
    let mut depth = 0_usize;
    let mut cursor = start;
    while let Some(byte) = bytes.get(cursor).copied() {
        if byte == b'"' {
            cursor = skip_string(bytes, cursor)?;
            continue;
        }
        if byte == open {
            depth += 1;
        } else if byte == close {
            depth = depth.checked_sub(1).ok_or(JsonEditError::MalformedObject)?;
            if depth == 0 {
                return Ok(cursor + 1);
            }
        }
        cursor += 1;
    }
    Err(JsonEditError::MalformedObject)
}

fn skip_string(bytes: &[u8], start: usize) -> Result<usize, JsonEditError> {
    if bytes.get(start) != Some(&b'"') {
        return Err(JsonEditError::MalformedObject);
    }
    let mut cursor = start + 1;
    while let Some(byte) = bytes.get(cursor).copied() {
        match byte {
            b'\\' => {
                cursor = cursor
                    .checked_add(2)
                    .ok_or(JsonEditError::MalformedObject)?
            }
            b'"' => return Ok(cursor + 1),
            _other => cursor += 1,
        }
    }
    Err(JsonEditError::MalformedObject)
}

fn skip_whitespace(bytes: &[u8], mut cursor: usize) -> usize {
    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }
    cursor
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "{\n\"skeleton\": {\"spine\":\"4.3.23\"},\n\"bones\": [{\"name\":\"root\"}],\n\"skins\": [{\"name\":\"default\"}],\n\"animations\": {\n\t\"idle\": {\"events\":[{\"time\":0.5,\"name\":\"blink\"}]}\n}\n}\n";

    #[test]
    fn translation_animation_uses_spine_relative_bone_timelines() {
        let keys = [
            TranslationKey {
                time: 0.0,
                x: 3.0,
                y: -2.0,
            },
            TranslationKey {
                time: 0.25,
                x: 5.5,
                y: 4.0,
            },
        ];
        let encoded = encode_translation_animation(&[TranslationTimeline {
            bone: "paw control",
            keys: &keys,
        }])
        .expect("finite keys encode");
        let value: serde_json::Value = serde_json::from_str(&encoded).expect("valid JSON");

        assert_eq!(
            value["bones"]["paw control"]["translate"],
            serde_json::json!([
                {"x": 3.0, "y": -2.0},
                {"time": 0.25, "x": 5.5, "y": 4.0}
            ])
        );
    }

    #[test]
    fn insert_preserves_every_existing_byte_outside_animations() {
        let animation = r#"{"bones":{"paw":{"translate":[{"x":1.0,"y":2.0}]}}}"#;
        let updated = upsert_animation(SOURCE, "walk-draft", animation).expect("insert succeeds");

        assert!(updated.starts_with(&SOURCE[..SOURCE.find("{\n\t\"idle\"").unwrap()]));
        assert!(updated.contains("\"idle\": {\"events\":[{\"time\":0.5,\"name\":\"blink\"}]}"));
        assert!(updated.ends_with("\n}\n}\n"));
        let value: serde_json::Value = serde_json::from_str(&updated).expect("valid JSON");
        assert_eq!(
            value["animations"]["walk-draft"],
            serde_json::from_str::<serde_json::Value>(animation).unwrap()
        );
    }

    #[test]
    fn replace_changes_only_the_selected_animation_value() {
        let first = upsert_animation(SOURCE, "walk-draft", r#"{"bones":{"paw":{}}}"#)
            .expect("first insert succeeds");
        let second = upsert_animation(&first, "walk-draft", r#"{"bones":{"paw2":{}}}"#)
            .expect("replacement succeeds");

        assert_eq!(second.matches("\"walk-draft\"").count(), 1);
        assert!(second.contains("\"idle\": {\"events\":[{\"time\":0.5,\"name\":\"blink\"}]}"));
        let value: serde_json::Value = serde_json::from_str(&second).expect("valid JSON");
        assert!(
            value["animations"]["walk-draft"]["bones"]
                .get("paw")
                .is_none()
        );
        assert!(
            value["animations"]["walk-draft"]["bones"]
                .get("paw2")
                .is_some()
        );
    }
}

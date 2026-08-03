use std::collections::HashSet;

use crate::{
    Rgba8,
    json::{JsonMember, JsonValue},
};

use super::{LoadDocument, LoadError, LoadErrorKind, SourceLocation};

pub(crate) fn object<'a>(value: &'a JsonValue, path: &str) -> Result<&'a [JsonMember], LoadError> {
    value.as_object().ok_or_else(|| {
        schema_error(
            path,
            format!("expected object, found {}", value.kind_name()),
        )
    })
}

pub(crate) fn array<'a>(value: &'a JsonValue, path: &str) -> Result<&'a [JsonValue], LoadError> {
    value
        .as_array()
        .ok_or_else(|| schema_error(path, format!("expected array, found {}", value.kind_name())))
}

pub(crate) fn unique_members(members: &[JsonMember], path: &str) -> Result<(), LoadError> {
    let mut seen = HashSet::with_capacity(members.len());
    for member in members {
        if !seen.insert(member.name()) {
            return Err(error(
                LoadErrorKind::DuplicateField,
                &pointer(path, member.name()),
                format!("duplicate object member {:?}", member.name()),
            ));
        }
    }
    Ok(())
}

pub(crate) fn member<'a>(
    members: &'a [JsonMember],
    name: &str,
    path: &str,
) -> Result<Option<&'a JsonValue>, LoadError> {
    let mut matches = members
        .iter()
        .filter(|member| member.name() == name)
        .map(JsonMember::value);
    let first = matches.next();
    if first.is_some() && matches.next().is_some() {
        return Err(error(
            LoadErrorKind::DuplicateField,
            &pointer(path, name),
            format!("duplicate object member {name:?}"),
        ));
    }
    Ok(first)
}

pub(crate) fn required_member<'a>(
    members: &'a [JsonMember],
    name: &str,
    path: &str,
) -> Result<&'a JsonValue, LoadError> {
    member(members, name, path)?.ok_or_else(|| {
        schema_error(
            &pointer(path, name),
            format!("required field {name:?} is missing"),
        )
    })
}

pub(crate) fn string<'a>(value: &'a JsonValue, path: &str) -> Result<&'a str, LoadError> {
    value.as_str().ok_or_else(|| {
        schema_error(
            path,
            format!("expected string, found {}", value.kind_name()),
        )
    })
}

pub(crate) fn nonempty_string<'a>(value: &'a JsonValue, path: &str) -> Result<&'a str, LoadError> {
    let value = string(value, path)?;
    if value.is_empty() {
        Err(schema_error(path, "string must not be empty"))
    } else {
        Ok(value)
    }
}

pub(crate) fn optional_string<'a>(
    members: &'a [JsonMember],
    name: &str,
    path: &str,
) -> Result<Option<&'a str>, LoadError> {
    match member(members, name, path)? {
        None | Some(JsonValue::Null) => Ok(None),
        Some(value) => string(value, &pointer(path, name)).map(Some),
    }
}

pub(crate) fn optional_nonempty_string<'a>(
    members: &'a [JsonMember],
    name: &str,
    path: &str,
) -> Result<Option<&'a str>, LoadError> {
    match member(members, name, path)? {
        None | Some(JsonValue::Null) => Ok(None),
        Some(value) => nonempty_string(value, &pointer(path, name)).map(Some),
    }
}

pub(crate) fn bool_or(
    members: &[JsonMember],
    name: &str,
    path: &str,
    default: bool,
) -> Result<bool, LoadError> {
    match member(members, name, path)? {
        None => Ok(default),
        Some(value) => value.as_bool().ok_or_else(|| {
            schema_error(
                &pointer(path, name),
                format!("expected boolean, found {}", value.kind_name()),
            )
        }),
    }
}

pub(crate) fn f32_or(
    members: &[JsonMember],
    name: &str,
    path: &str,
    default: f32,
) -> Result<f32, LoadError> {
    match member(members, name, path)? {
        None => Ok(default),
        Some(value) => finite_f32(value, &pointer(path, name)),
    }
}

pub(crate) fn finite_f32(value: &JsonValue, path: &str) -> Result<f32, LoadError> {
    let number = value.as_number_f64().ok_or_else(|| {
        schema_error(
            path,
            format!("expected number, found {}", value.kind_name()),
        )
    })?;
    let converted = number as f32;
    if number.is_finite() && converted.is_finite() {
        Ok(converted)
    } else {
        Err(error(
            LoadErrorKind::NonFiniteNumber,
            path,
            "number must be finite and representable as f32",
        ))
    }
}

pub(crate) fn u32_or(
    members: &[JsonMember],
    name: &str,
    path: &str,
    default: u32,
) -> Result<u32, LoadError> {
    match member(members, name, path)? {
        None => Ok(default),
        Some(value) => u32_value(value, &pointer(path, name)),
    }
}

pub(crate) fn u32_value(value: &JsonValue, path: &str) -> Result<u32, LoadError> {
    let number = integer_i128(value, path)?;
    u32::try_from(number)
        .map_err(|_error| schema_error(path, format!("integer {number} is outside the u32 range")))
}

pub(crate) fn i32_value(value: &JsonValue, path: &str) -> Result<i32, LoadError> {
    let number = integer_i128(value, path)?;
    i32::try_from(number)
        .map_err(|_error| schema_error(path, format!("integer {number} is outside the i32 range")))
}

fn integer_i128(value: &JsonValue, path: &str) -> Result<i128, LoadError> {
    match value {
        JsonValue::I64(value) => Ok(i128::from(*value)),
        JsonValue::U64(value) => Ok(i128::from(*value)),
        JsonValue::F64(value)
            if value.is_finite()
                && value.fract() == 0.0
                && *value >= i128::MIN as f64
                && *value <= i128::MAX as f64 =>
        {
            Ok(*value as i128)
        }
        JsonValue::F64(_) => Err(schema_error(path, "expected an integral finite number")),
        _ => Err(schema_error(
            path,
            format!("expected integer, found {}", value.kind_name()),
        )),
    }
}

pub(crate) fn colour_or(
    members: &[JsonMember],
    name: &str,
    path: &str,
    default: Rgba8,
) -> Result<Rgba8, LoadError> {
    match member(members, name, path)? {
        None => Ok(default),
        Some(value) => colour(value, &pointer(path, name)),
    }
}

pub(crate) fn colour(value: &JsonValue, path: &str) -> Result<Rgba8, LoadError> {
    let text = string(value, path)?;
    let channels = match text.len() {
        6 => [
            parse_hex_channel(text, 0, path)?,
            parse_hex_channel(text, 2, path)?,
            parse_hex_channel(text, 4, path)?,
            255,
        ],
        8 => [
            parse_hex_channel(text, 0, path)?,
            parse_hex_channel(text, 2, path)?,
            parse_hex_channel(text, 4, path)?,
            parse_hex_channel(text, 6, path)?,
        ],
        _ => {
            return Err(schema_error(
                path,
                "colour must contain six RGB or eight RGBA hexadecimal digits",
            ));
        }
    };
    Ok(Rgba8::new(
        channels[0],
        channels[1],
        channels[2],
        channels[3],
    ))
}

fn parse_hex_channel(text: &str, offset: usize, path: &str) -> Result<u8, LoadError> {
    text.get(offset..offset + 2)
        .and_then(|channel| u8::from_str_radix(channel, 16).ok())
        .ok_or_else(|| schema_error(path, "colour contains a non-hexadecimal channel"))
}

pub(crate) fn pointer(base: &str, segment: &str) -> String {
    let mut pointer = String::with_capacity(base.len() + segment.len() + 1);
    pointer.push_str(base);
    pointer.push('/');
    for character in segment.chars() {
        match character {
            '~' => pointer.push_str("~0"),
            '/' => pointer.push_str("~1"),
            other => pointer.push(other),
        }
    }
    pointer
}

pub(crate) fn index_pointer(base: &str, index: usize) -> String {
    pointer(base, &index.to_string())
}

pub(crate) fn schema_error(path: &str, message: impl Into<Box<str>>) -> LoadError {
    error(LoadErrorKind::SchemaViolation, path, message)
}

pub(crate) fn error(kind: LoadErrorKind, path: &str, message: impl Into<Box<str>>) -> LoadError {
    LoadError::new(
        kind,
        message,
        SourceLocation::for_document(LoadDocument::SkeletonJson).with_path(path),
    )
}

#[cfg(test)]
mod tests {
    use crate::json::parse_json;

    use super::*;

    #[test]
    fn pointers_escape_rfc_6901_segments() {
        assert_eq!(
            pointer("/skins/0/attachments", "face/eyes~open"),
            "/skins/0/attachments/face~1eyes~0open"
        );
    }

    #[test]
    fn duplicate_members_are_never_silently_selected() {
        let value = parse_json(br#"{"name":"one","name":"two"}"#).expect("valid syntax");
        let members = object(&value, "").expect("object");
        let error = member(members, "name", "").expect_err("duplicate must fail");
        assert_eq!(error.kind(), LoadErrorKind::DuplicateField);
        assert_eq!(error.path(), Some("/name"));
    }
}

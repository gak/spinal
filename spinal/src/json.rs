//! Private, lossless JSON syntax tree used by the skeleton decoder.
//!
//! `serde_json::Value` stores objects in a map, which discards duplicate names
//! and may discard source order. Both are useful when producing precise schema
//! diagnostics, so this module deserializes into an ordered list of members
//! instead.

use core::fmt;
use core::str;

use serde::Deserialize;
use serde::de::{Deserializer, MapAccess, SeqAccess, Visitor};

use crate::load::error::{LoadDocument, LoadError, LoadErrorKind, SourceLocation};

/// A JSON value that preserves integer kinds and ordered object members.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum JsonValue {
    /// The JSON `null` value.
    Null,
    /// A JSON boolean.
    Bool(bool),
    /// A negative integer representable as an `i64`.
    I64(i64),
    /// A non-negative integer representable as a `u64`.
    U64(u64),
    /// A finite JSON number represented as an `f64`.
    F64(f64),
    /// A JSON string.
    String(Box<str>),
    /// A JSON array in source order.
    Array(Box<[Self]>),
    /// A JSON object whose members, including duplicate names, are in source
    /// order.
    Object(Box<[JsonMember]>),
}

impl JsonValue {
    /// Returns a short schema-facing name for this value's JSON kind.
    pub(crate) const fn kind_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "boolean",
            Self::I64(_) | Self::U64(_) | Self::F64(_) => "number",
            Self::String(_) => "string",
            Self::Array(_) => "array",
            Self::Object(_) => "object",
        }
    }

    /// Returns whether this is the JSON `null` value.
    #[cfg(test)]
    pub(crate) const fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Returns the boolean value, if this is a JSON boolean.
    pub(crate) const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns the signed integer, if this number was represented as an
    /// `i64`.
    #[cfg(test)]
    pub(crate) const fn as_i64(&self) -> Option<i64> {
        match self {
            Self::I64(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns the unsigned integer, if this number was represented as a
    /// `u64`.
    #[cfg(test)]
    pub(crate) const fn as_u64(&self) -> Option<u64> {
        match self {
            Self::U64(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns the floating-point value, if this number was represented as an
    /// `f64`.
    #[cfg(test)]
    pub(crate) const fn as_f64(&self) -> Option<f64> {
        match self {
            Self::F64(value) => Some(*value),
            _ => None,
        }
    }

    /// Converts any JSON numeric representation to an `f64`.
    pub(crate) fn as_number_f64(&self) -> Option<f64> {
        match self {
            Self::I64(value) => Some(*value as f64),
            Self::U64(value) => Some(*value as f64),
            Self::F64(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns the string contents, if this is a JSON string.
    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the ordered elements, if this is a JSON array.
    pub(crate) fn as_array(&self) -> Option<&[Self]> {
        match self {
            Self::Array(values) => Some(values),
            _ => None,
        }
    }

    /// Returns all ordered members, if this is a JSON object.
    pub(crate) fn as_object(&self) -> Option<&[JsonMember]> {
        match self {
            Self::Object(members) => Some(members),
            _ => None,
        }
    }

    /// Returns every value for `name` in source order.
    ///
    /// Returning an iterator rather than a single value ensures schema code
    /// must make an explicit decision about duplicate members.
    #[cfg(test)]
    pub(crate) fn member_values<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Iterator<Item = &'a Self> + 'a {
        self.as_object()
            .into_iter()
            .flatten()
            .filter(move |member| member.name() == name)
            .map(JsonMember::value)
    }
}

/// One named JSON object member.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct JsonMember {
    name: Box<str>,
    value: JsonValue,
}

impl JsonMember {
    #[cfg(test)]
    pub(crate) fn test_fixture(name: &str, value: JsonValue) -> Self {
        Self {
            name: name.into(),
            value,
        }
    }

    /// Returns the member name.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// Returns the member value.
    pub(crate) const fn value(&self) -> &JsonValue {
        &self.value
    }
}

impl<'de> Deserialize<'de> for JsonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(JsonValueVisitor)
    }
}

struct JsonValueVisitor;

impl<'de> Visitor<'de> for JsonValueVisitor {
    type Value = JsonValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(JsonValue::Null)
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(JsonValue::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(JsonValue::I64(value))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(JsonValue::U64(value))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
        Ok(JsonValue::F64(value))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(JsonValue::String(value.into()))
    }

    fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(JsonValue::String(value.into()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(JsonValue::String(value.into_boxed_str()))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::with_capacity(sequence.size_hint().unwrap_or(0));
        while let Some(value) = sequence.next_element()? {
            values.push(value);
        }
        Ok(JsonValue::Array(values.into_boxed_slice()))
    }

    fn visit_map<A>(self, mut object: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut members = Vec::with_capacity(object.size_hint().unwrap_or(0));
        while let Some(name) = object.next_key::<Box<str>>()? {
            let value = object.next_value()?;
            members.push(JsonMember { name, value });
        }
        Ok(JsonValue::Object(members.into_boxed_slice()))
    }
}

/// Parses one complete Spine skeleton JSON document.
pub(crate) fn parse_json(input: &[u8]) -> Result<JsonValue, LoadError> {
    let text = str::from_utf8(input).map_err(|error| invalid_utf8_error(input, error))?;
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let value =
        JsonValue::deserialize(&mut deserializer).map_err(|error| syntax_error(text, &error))?;
    deserializer
        .end()
        .map_err(|error| syntax_error(text, &error))?;
    Ok(value)
}

fn invalid_utf8_error(input: &[u8], error: str::Utf8Error) -> LoadError {
    let byte_offset = error.valid_up_to();
    let (line, column) = line_column_at_byte_offset(input, byte_offset);
    LoadError::new(
        LoadErrorKind::InvalidUtf8,
        "skeleton JSON is not valid UTF-8",
        SourceLocation::for_document(LoadDocument::SkeletonJson).with_text_position(
            line,
            column,
            Some(byte_offset),
        ),
    )
}

fn syntax_error(input: &str, error: &serde_json::Error) -> LoadError {
    let line = error.line().max(1);
    let reported_column = error.column();
    let byte_offset = byte_offset_at_text_position(input, line, reported_column);
    let column = byte_offset
        .and_then(|offset| character_column_at_byte_offset(input, offset))
        .unwrap_or_else(|| reported_column.max(1));
    let message = if error.is_eof() {
        "skeleton JSON ended before the value was complete"
    } else {
        "skeleton JSON is not syntactically valid"
    };
    LoadError::new(
        LoadErrorKind::Syntax,
        message,
        SourceLocation::for_document(LoadDocument::SkeletonJson).with_text_position(
            line,
            column,
            byte_offset,
        ),
    )
}

fn line_column_at_byte_offset(input: &[u8], byte_offset: usize) -> (usize, usize) {
    let prefix = &input[..byte_offset.min(input.len())];
    let line = prefix.iter().filter(|byte| **byte == b'\n').count() + 1;
    let line_start = prefix
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |newline| newline + 1);
    let column = str::from_utf8(&prefix[line_start..]).map_or(1, |line| line.chars().count() + 1);
    (line, column)
}

fn character_column_at_byte_offset(input: &str, byte_offset: usize) -> Option<usize> {
    let prefix = input.get(..byte_offset.min(input.len()))?;
    let line_start = prefix.rfind('\n').map_or(0, |newline| newline + 1);
    Some(prefix.get(line_start..)?.chars().count() + 1)
}

fn byte_offset_at_text_position(input: &str, line: usize, column: usize) -> Option<usize> {
    if line == 0 {
        return None;
    }

    let bytes = input.as_bytes();
    let mut line_start = 0;
    let mut current_line = 1;
    while current_line < line {
        let relative_newline = bytes
            .get(line_start..)?
            .iter()
            .position(|byte| *byte == b'\n')?;
        line_start = line_start.checked_add(relative_newline + 1)?;
        current_line += 1;
    }

    let line_end = bytes
        .get(line_start..)?
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(bytes.len(), |relative_newline| {
            line_start + relative_newline
        });
    let relative_column = column.saturating_sub(1);
    let candidate = line_start.checked_add(relative_column)?;
    (candidate <= line_end).then_some(candidate)
}

#[cfg(test)]
mod tests {
    use std::panic;

    use super::{JsonValue, parse_json};
    use crate::load::error::{LoadDocument, LoadErrorKind};

    #[test]
    fn preserves_object_order_and_duplicate_names() {
        let value = parse_json(br#"{"tail":1,"head":2,"tail":3}"#).unwrap();
        let members = value.as_object().unwrap();

        assert_eq!(
            members
                .iter()
                .map(|member| member.name())
                .collect::<Vec<_>>(),
            ["tail", "head", "tail"]
        );
        assert_eq!(
            value
                .member_values("tail")
                .map(JsonValue::as_u64)
                .collect::<Vec<_>>(),
            [Some(1), Some(3)]
        );
    }

    #[test]
    fn preserves_numeric_representations() {
        let value = parse_json(
            br#"[-1,0,9223372036854775807,9223372036854775808,18446744073709551615,1.25,1e2]"#,
        )
        .unwrap();

        assert_eq!(
            value.as_array().unwrap(),
            [
                JsonValue::I64(-1),
                JsonValue::U64(0),
                JsonValue::U64(9_223_372_036_854_775_807),
                JsonValue::U64(9_223_372_036_854_775_808),
                JsonValue::U64(18_446_744_073_709_551_615),
                JsonValue::F64(1.25),
                JsonValue::F64(100.0),
            ]
        );
    }

    #[test]
    fn accessors_distinguish_json_kinds() {
        let value = parse_json(br#"[null,true,"cat",[-2],{"n":0.5}]"#).unwrap();
        let values = value.as_array().unwrap();

        assert!(values[0].is_null());
        assert_eq!(values[0].kind_name(), "null");
        assert_eq!(values[1].as_bool(), Some(true));
        assert_eq!(values[2].as_str(), Some("cat"));
        assert_eq!(values[3].as_array().unwrap()[0].as_i64(), Some(-2));
        let number = values[4].member_values("n").next().unwrap();
        assert_eq!(number.as_f64(), Some(0.5));
        assert_eq!(number.as_number_f64(), Some(0.5));
    }

    #[test]
    fn invalid_utf8_has_an_exact_location() {
        let input = b"{\n  \"cat\": \xFF}";
        let error = parse_json(input).unwrap_err();

        assert_eq!(error.kind(), LoadErrorKind::InvalidUtf8);
        assert_eq!(error.location().document(), LoadDocument::SkeletonJson);
        assert_eq!(error.location().line(), Some(2));
        assert_eq!(error.location().column(), Some(10));
        assert_eq!(error.location().byte_offset(), Some(11));
    }

    #[test]
    fn error_columns_count_unicode_characters_while_offsets_count_bytes() {
        let invalid_utf8 = parse_json(b"{\n  \"\xC3\xA9\": \xFF}").unwrap_err();
        assert_eq!(invalid_utf8.location().line(), Some(2));
        assert_eq!(invalid_utf8.location().column(), Some(8));
        assert_eq!(invalid_utf8.location().byte_offset(), Some(10));

        let syntax = parse_json("{\n  \"é\": truX\n}".as_bytes()).unwrap_err();
        assert_eq!(syntax.location().line(), Some(2));
        assert_eq!(syntax.location().column(), Some(11));
        assert_eq!(syntax.location().byte_offset(), Some(13));
    }

    #[test]
    fn rejects_trailing_json_values() {
        let error = parse_json(b"{}\n  []").unwrap_err();

        assert_eq!(error.kind(), LoadErrorKind::Syntax);
        assert_eq!(error.location().line(), Some(2));
        assert_eq!(error.location().column(), Some(3));
        assert_eq!(error.location().byte_offset(), Some(5));
    }

    #[test]
    fn syntax_errors_have_one_based_text_and_byte_locations() {
        let error = parse_json(b"{\n  \"cat\": truX\n}").unwrap_err();

        assert_eq!(error.kind(), LoadErrorKind::Syntax);
        assert_eq!(error.location().document(), LoadDocument::SkeletonJson);
        assert_eq!(error.location().line(), Some(2));
        assert_eq!(error.location().column(), Some(13));
        assert_eq!(error.location().byte_offset(), Some(14));
    }

    #[test]
    fn arbitrary_inputs_never_panic() {
        for byte in u8::MIN..=u8::MAX {
            assert!(panic::catch_unwind(|| parse_json(&[byte])).is_ok());
        }

        let mut state = 0xA8F1_D93B_6C42_750Eu64;
        for length in 0..256 {
            let mut input = Vec::with_capacity(length);
            for _index in 0..length {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                input.push(state as u8);
            }
            assert!(
                panic::catch_unwind(|| parse_json(&input)).is_ok(),
                "parser panicked for {input:?}"
            );
        }

        let mut deeply_nested = vec![b'['; 256];
        deeply_nested.extend(core::iter::repeat_n(b']', 256));
        assert!(panic::catch_unwind(|| parse_json(&deeply_nested)).is_ok());
    }
}

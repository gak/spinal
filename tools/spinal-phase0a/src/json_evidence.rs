//! Bounded, deterministic JSON evidence for structured project documents.

use crate::digest::sha256_bytes;
use std::collections::{BTreeMap, BTreeSet};
use std::str;
use thiserror::Error;

const MAX_SUPPORTED_BYTES: usize = 64 * 1024 * 1024;
const MAX_SUPPORTED_DEPTH: usize = 128;
const MAX_SUPPORTED_NODES: usize = 2_000_000;
const SETUP_FINGERPRINT_DOMAIN: &[u8] = b"json-evidence/setup/v1";
const ANIMATION_FINGERPRINT_DOMAIN: &[u8] = b"json-evidence/animation/v1";

/// Resource limits applied while parsing one JSON document.
///
/// Limits may be reduced for smaller trust boundaries, but cannot exceed the
/// implementation's fixed safety ceilings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JsonLimits {
    max_bytes: usize,
    max_depth: usize,
    max_nodes: usize,
}

impl JsonLimits {
    /// Creates nonzero limits within the implementation's safety ceilings.
    pub fn new(
        max_bytes: usize,
        max_depth: usize,
        max_nodes: usize,
    ) -> Result<Self, JsonEvidenceError> {
        if max_bytes == 0
            || max_depth == 0
            || max_nodes == 0
            || max_bytes > MAX_SUPPORTED_BYTES
            || max_depth > MAX_SUPPORTED_DEPTH
            || max_nodes > MAX_SUPPORTED_NODES
        {
            return Err(JsonEvidenceError::InvalidLimits);
        }

        Ok(Self {
            max_bytes,
            max_depth,
            max_nodes,
        })
    }
}

impl Default for JsonLimits {
    fn default() -> Self {
        Self {
            max_bytes: MAX_SUPPORTED_BYTES,
            max_depth: MAX_SUPPORTED_DEPTH,
            max_nodes: MAX_SUPPORTED_NODES,
        }
    }
}

/// A parsed document and its deterministic comparison evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JsonEvidence {
    canonical_pretty: String,
    normalized_pretty: String,
    root: JsonValue,
    setup_fingerprint: String,
    animation_fingerprints: BTreeMap<String, String>,
}

impl JsonEvidence {
    /// Parses and validates a document under explicit resource limits.
    ///
    /// The root, `skeleton`, and `animations` values must be objects. The
    /// normalized form removes only `/skeleton/hash`. The setup fingerprint
    /// additionally excludes the top-level `animations` object.
    pub fn from_slice(input: &[u8], limits: JsonLimits) -> Result<Self, JsonEvidenceError> {
        let root = Parser::parse(input, limits)?;
        let root_object = root.as_object().ok_or(JsonEvidenceError::RootNotObject)?;

        let skeleton = root_object
            .get("skeleton")
            .ok_or(JsonEvidenceError::MissingSkeleton)?;
        if !matches!(skeleton, JsonValue::Object(_)) {
            return Err(JsonEvidenceError::SkeletonNotObject);
        }

        let animations = root_object
            .get("animations")
            .ok_or(JsonEvidenceError::MissingAnimations)?;
        let animations = animations
            .as_object()
            .ok_or(JsonEvidenceError::AnimationsNotObject)?;

        let canonical_pretty = root.canonical_pretty();
        let mut normalized = root.clone();
        let normalized_root = normalized
            .as_object_mut()
            .expect("the root object was validated above");
        normalized_root
            .get_mut("skeleton")
            .and_then(JsonValue::as_object_mut)
            .expect("the skeleton object was validated above")
            .remove("hash");
        let normalized_pretty = normalized.canonical_pretty();

        let mut setup = normalized.clone();
        setup
            .as_object_mut()
            .expect("the root object was validated above")
            .remove("animations");
        let canonical_setup = setup.canonical_compact();
        let setup_fingerprint =
            framed_fingerprint(SETUP_FINGERPRINT_DOMAIN, &[canonical_setup.as_bytes()]);

        let animation_fingerprints = animations
            .iter()
            .map(|(name, animation)| {
                let canonical_animation = animation.canonical_compact();
                (
                    name.clone(),
                    framed_fingerprint(
                        ANIMATION_FINGERPRINT_DOMAIN,
                        &[name.as_bytes(), canonical_animation.as_bytes()],
                    ),
                )
            })
            .collect();

        Ok(Self {
            canonical_pretty,
            normalized_pretty,
            root,
            setup_fingerprint,
            animation_fingerprints,
        })
    }

    /// Returns canonical pretty JSON with object keys sorted and arrays intact.
    ///
    /// Number spellings are preserved exactly as they appeared in the input.
    pub fn canonical_pretty_json(&self) -> &str {
        &self.canonical_pretty
    }

    /// Returns canonical pretty JSON after removing only `/skeleton/hash`.
    pub fn normalized_pretty_json(&self) -> &str {
        &self.normalized_pretty
    }

    /// Returns the lowercase SHA-256 of normalized setup data.
    ///
    /// Setup data excludes the top-level `animations` object and
    /// `/skeleton/hash`.
    pub fn setup_fingerprint(&self) -> &str {
        &self.setup_fingerprint
    }

    /// Returns lowercase SHA-256 fingerprints for animations keyed by name.
    pub fn animation_fingerprints(&self) -> &BTreeMap<String, String> {
        &self.animation_fingerprints
    }

    /// Computes semantic differences in deterministic pointer order.
    ///
    /// Pointers use RFC 6901 escaping. An absent side represents an addition or
    /// removal, and present values are compact canonical JSON fragments. Only a
    /// changed, present string at `/skeleton/hash` is marked approved volatile;
    /// the raw difference is always retained.
    pub fn semantic_differences(&self, after: &Self) -> Vec<JsonDifference> {
        let mut differences = Vec::new();
        collect_differences("", Some(&self.root), Some(&after.root), &mut differences);
        differences
    }
}

/// One deterministic semantic JSON difference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JsonDifference {
    pointer: String,
    before: Option<String>,
    after: Option<String>,
    approved_volatile: bool,
}

impl JsonDifference {
    /// Returns the RFC 6901 pointer to the changed value.
    pub fn pointer(&self) -> &str {
        &self.pointer
    }

    /// Returns the prior compact canonical JSON fragment, if present.
    pub fn before_json(&self) -> Option<&str> {
        self.before.as_deref()
    }

    /// Returns the subsequent compact canonical JSON fragment, if present.
    pub fn after_json(&self) -> Option<&str> {
        self.after.as_deref()
    }

    /// Returns whether fixed policy approves this exact volatile change.
    ///
    /// Approval is true only for a changed, present string-to-string value at
    /// `/skeleton/hash`. The difference remains present for audit evidence.
    pub fn approved_volatile(&self) -> bool {
        self.approved_volatile
    }
}

/// Failures while constructing deterministic JSON evidence.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum JsonEvidenceError {
    /// One or more limits were zero or exceeded a fixed safety ceiling.
    #[error("JSON limits must be nonzero and within fixed safety ceilings")]
    InvalidLimits,
    /// The input exceeded the configured byte limit.
    #[error("JSON input is {actual} bytes, exceeding the {limit}-byte limit")]
    InputTooLarge {
        /// Observed input size.
        actual: usize,
        /// Configured byte limit.
        limit: usize,
    },
    /// The input was not valid UTF-8.
    #[error("JSON input is not valid UTF-8 at byte {offset}")]
    InvalidUtf8 {
        /// Byte offset at which UTF-8 validation failed.
        offset: usize,
    },
    /// The input was not strict JSON.
    #[error("invalid JSON at byte {offset}: {detail}")]
    InvalidSyntax {
        /// Byte offset associated with the syntax failure.
        offset: usize,
        /// Stable human-readable explanation.
        detail: &'static str,
    },
    /// An object repeated the same decoded key.
    #[error("duplicate JSON object key {key:?} at byte {offset}")]
    DuplicateObjectKey {
        /// Repeated decoded object key.
        key: String,
        /// Byte offset of the repeated key.
        offset: usize,
    },
    /// JSON nesting exceeded the configured depth limit.
    #[error("JSON nesting exceeds the configured depth limit of {limit}")]
    DepthLimitExceeded {
        /// Configured maximum value depth, counting the root as one.
        limit: usize,
    },
    /// The parsed value count exceeded the configured node limit.
    #[error("JSON value count exceeds the configured node limit of {limit}")]
    NodeLimitExceeded {
        /// Configured maximum number of JSON values.
        limit: usize,
    },
    /// The top-level JSON value was not an object.
    #[error("top-level JSON value must be an object")]
    RootNotObject,
    /// The required top-level `skeleton` member was absent.
    #[error("top-level JSON object is missing `skeleton`")]
    MissingSkeleton,
    /// The top-level `skeleton` member was not an object.
    #[error("top-level `skeleton` value must be an object")]
    SkeletonNotObject,
    /// The required top-level `animations` member was absent.
    #[error("top-level JSON object is missing `animations`")]
    MissingAnimations,
    /// The top-level `animations` member was not an object.
    #[error("top-level `animations` value must be an object")]
    AnimationsNotObject,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum JsonValue {
    Null,
    Boolean(bool),
    Number(String),
    String(String),
    Array(Vec<Self>),
    Object(BTreeMap<String, Self>),
}

impl JsonValue {
    fn as_object(&self) -> Option<&BTreeMap<String, Self>> {
        match self {
            Self::Object(object) => Some(object),
            _ => None,
        }
    }

    fn as_object_mut(&mut self) -> Option<&mut BTreeMap<String, Self>> {
        match self {
            Self::Object(object) => Some(object),
            _ => None,
        }
    }

    fn canonical_pretty(&self) -> String {
        let mut output = String::new();
        self.write_pretty(&mut output, 0);
        output
    }

    fn write_pretty(&self, output: &mut String, indent: usize) {
        match self {
            Self::Null => output.push_str("null"),
            Self::Boolean(value) => output.push_str(if *value { "true" } else { "false" }),
            Self::Number(value) => output.push_str(value),
            Self::String(value) => write_json_string(output, value),
            Self::Array(values) if values.is_empty() => output.push_str("[]"),
            Self::Array(values) => {
                output.push_str("[\n");
                for (index, value) in values.iter().enumerate() {
                    write_indent(output, indent + 1);
                    value.write_pretty(output, indent + 1);
                    if index + 1 != values.len() {
                        output.push(',');
                    }
                    output.push('\n');
                }
                write_indent(output, indent);
                output.push(']');
            }
            Self::Object(values) if values.is_empty() => output.push_str("{}"),
            Self::Object(values) => {
                output.push_str("{\n");
                for (index, (key, value)) in values.iter().enumerate() {
                    write_indent(output, indent + 1);
                    write_json_string(output, key);
                    output.push_str(": ");
                    value.write_pretty(output, indent + 1);
                    if index + 1 != values.len() {
                        output.push(',');
                    }
                    output.push('\n');
                }
                write_indent(output, indent);
                output.push('}');
            }
        }
    }

    fn canonical_compact(&self) -> String {
        let mut output = String::new();
        self.write_compact(&mut output);
        output
    }

    fn write_compact(&self, output: &mut String) {
        match self {
            Self::Null => output.push_str("null"),
            Self::Boolean(value) => output.push_str(if *value { "true" } else { "false" }),
            Self::Number(value) => output.push_str(value),
            Self::String(value) => write_json_string(output, value),
            Self::Array(values) => {
                output.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        output.push(',');
                    }
                    value.write_compact(output);
                }
                output.push(']');
            }
            Self::Object(values) => {
                output.push('{');
                for (index, (key, value)) in values.iter().enumerate() {
                    if index != 0 {
                        output.push(',');
                    }
                    write_json_string(output, key);
                    output.push(':');
                    value.write_compact(output);
                }
                output.push('}');
            }
        }
    }
}

struct Parser<'input> {
    input: &'input [u8],
    position: usize,
    nodes: usize,
    limits: JsonLimits,
}

impl<'input> Parser<'input> {
    fn parse(input: &'input [u8], limits: JsonLimits) -> Result<JsonValue, JsonEvidenceError> {
        if input.len() > limits.max_bytes {
            return Err(JsonEvidenceError::InputTooLarge {
                actual: input.len(),
                limit: limits.max_bytes,
            });
        }
        str::from_utf8(input).map_err(|error| JsonEvidenceError::InvalidUtf8 {
            offset: error.valid_up_to(),
        })?;

        let mut parser = Self {
            input,
            position: 0,
            nodes: 0,
            limits,
        };
        parser.skip_whitespace();
        let value = parser.parse_value(1)?;
        parser.skip_whitespace();
        if parser.position != input.len() {
            return Err(parser.syntax("trailing characters after the top-level value"));
        }
        Ok(value)
    }

    fn parse_value(&mut self, depth: usize) -> Result<JsonValue, JsonEvidenceError> {
        if depth > self.limits.max_depth {
            return Err(JsonEvidenceError::DepthLimitExceeded {
                limit: self.limits.max_depth,
            });
        }
        self.nodes = self
            .nodes
            .checked_add(1)
            .ok_or(JsonEvidenceError::NodeLimitExceeded {
                limit: self.limits.max_nodes,
            })?;
        if self.nodes > self.limits.max_nodes {
            return Err(JsonEvidenceError::NodeLimitExceeded {
                limit: self.limits.max_nodes,
            });
        }

        let byte = self
            .input
            .get(self.position)
            .copied()
            .ok_or_else(|| self.syntax("expected a JSON value"))?;
        match byte {
            b'n' => {
                self.parse_literal(b"null")?;
                Ok(JsonValue::Null)
            }
            b't' => {
                self.parse_literal(b"true")?;
                Ok(JsonValue::Boolean(true))
            }
            b'f' => {
                self.parse_literal(b"false")?;
                Ok(JsonValue::Boolean(false))
            }
            b'"' => self.parse_string().map(JsonValue::String),
            b'[' => self.parse_array(depth),
            b'{' => self.parse_object(depth),
            b'-' | b'0'..=b'9' => self.parse_number().map(JsonValue::Number),
            _ => Err(self.syntax("expected a JSON value")),
        }
    }

    fn parse_literal(&mut self, literal: &[u8]) -> Result<(), JsonEvidenceError> {
        let end = self.position.saturating_add(literal.len());
        if self.input.get(self.position..end) != Some(literal) {
            return Err(self.syntax("invalid JSON literal"));
        }
        self.position = end;
        Ok(())
    }

    fn parse_string(&mut self) -> Result<String, JsonEvidenceError> {
        let start = self.position;
        self.position += 1;
        while let Some(byte) = self.input.get(self.position).copied() {
            match byte {
                b'"' => {
                    self.position += 1;
                    let token = str::from_utf8(&self.input[start..self.position])
                        .expect("the complete input was validated as UTF-8");
                    return serde_json::from_str::<String>(token)
                        .map_err(|_| self.syntax_at(start, "invalid JSON string escape"));
                }
                b'\\' => {
                    self.position += 1;
                    let escaped = self
                        .input
                        .get(self.position)
                        .copied()
                        .ok_or_else(|| self.syntax("unterminated JSON string escape"))?;
                    match escaped {
                        b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {
                            self.position += 1;
                        }
                        b'u' => {
                            self.position += 1;
                            let end = self.position.saturating_add(4);
                            let digits = self.input.get(self.position..end).ok_or_else(|| {
                                self.syntax("incomplete Unicode escape in JSON string")
                            })?;
                            if !digits.iter().all(u8::is_ascii_hexdigit) {
                                return Err(self.syntax("invalid Unicode escape in JSON string"));
                            }
                            self.position = end;
                        }
                        _ => return Err(self.syntax("invalid JSON string escape")),
                    }
                }
                0x00..=0x1f => {
                    return Err(self.syntax("unescaped control byte in JSON string"));
                }
                _ => self.position += 1,
            }
        }

        Err(self.syntax_at(start, "unterminated JSON string"))
    }

    fn parse_array(&mut self, depth: usize) -> Result<JsonValue, JsonEvidenceError> {
        self.position += 1;
        self.skip_whitespace();
        let mut values = Vec::new();
        if self.consume(b']') {
            return Ok(JsonValue::Array(values));
        }

        loop {
            values.push(self.parse_value(depth + 1)?);
            self.skip_whitespace();
            if self.consume(b']') {
                return Ok(JsonValue::Array(values));
            }
            if !self.consume(b',') {
                return Err(self.syntax("expected `,` or `]` in JSON array"));
            }
            self.skip_whitespace();
        }
    }

    fn parse_object(&mut self, depth: usize) -> Result<JsonValue, JsonEvidenceError> {
        self.position += 1;
        self.skip_whitespace();
        let mut values = BTreeMap::new();
        if self.consume(b'}') {
            return Ok(JsonValue::Object(values));
        }

        loop {
            if self.input.get(self.position) != Some(&b'"') {
                return Err(self.syntax("expected a quoted key in JSON object"));
            }
            let key_offset = self.position;
            let key = self.parse_string()?;
            self.skip_whitespace();
            if !self.consume(b':') {
                return Err(self.syntax("expected `:` after JSON object key"));
            }
            self.skip_whitespace();
            let value = self.parse_value(depth + 1)?;
            if values.insert(key.clone(), value).is_some() {
                return Err(JsonEvidenceError::DuplicateObjectKey {
                    key,
                    offset: key_offset,
                });
            }
            self.skip_whitespace();
            if self.consume(b'}') {
                return Ok(JsonValue::Object(values));
            }
            if !self.consume(b',') {
                return Err(self.syntax("expected `,` or `}` in JSON object"));
            }
            self.skip_whitespace();
        }
    }

    fn parse_number(&mut self) -> Result<String, JsonEvidenceError> {
        let start = self.position;
        self.consume(b'-');

        match self.input.get(self.position).copied() {
            Some(b'0') => {
                self.position += 1;
                if self
                    .input
                    .get(self.position)
                    .is_some_and(u8::is_ascii_digit)
                {
                    return Err(self.syntax("leading zero in JSON number"));
                }
            }
            Some(b'1'..=b'9') => {
                self.position += 1;
                self.consume_ascii_digits();
            }
            _ => return Err(self.syntax("expected integer digits in JSON number")),
        }

        if self.consume(b'.') {
            if !self
                .input
                .get(self.position)
                .is_some_and(u8::is_ascii_digit)
            {
                return Err(self.syntax("expected fraction digits in JSON number"));
            }
            self.consume_ascii_digits();
        }

        if self
            .input
            .get(self.position)
            .is_some_and(|byte| *byte == b'e' || *byte == b'E')
        {
            self.position += 1;
            if self
                .input
                .get(self.position)
                .is_some_and(|byte| *byte == b'+' || *byte == b'-')
            {
                self.position += 1;
            }
            if !self
                .input
                .get(self.position)
                .is_some_and(u8::is_ascii_digit)
            {
                return Err(self.syntax("expected exponent digits in JSON number"));
            }
            self.consume_ascii_digits();
        }

        Ok(str::from_utf8(&self.input[start..self.position])
            .expect("the complete input was validated as UTF-8")
            .to_owned())
    }

    fn consume_ascii_digits(&mut self) {
        while self
            .input
            .get(self.position)
            .is_some_and(u8::is_ascii_digit)
        {
            self.position += 1;
        }
    }

    fn skip_whitespace(&mut self) {
        while self
            .input
            .get(self.position)
            .is_some_and(|byte| matches!(byte, b' ' | b'\n' | b'\r' | b'\t'))
        {
            self.position += 1;
        }
    }

    fn consume(&mut self, expected: u8) -> bool {
        if self.input.get(self.position) == Some(&expected) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn syntax(&self, detail: &'static str) -> JsonEvidenceError {
        self.syntax_at(self.position, detail)
    }

    fn syntax_at(&self, offset: usize, detail: &'static str) -> JsonEvidenceError {
        JsonEvidenceError::InvalidSyntax { offset, detail }
    }
}

fn collect_differences(
    pointer: &str,
    before: Option<&JsonValue>,
    after: Option<&JsonValue>,
    output: &mut Vec<JsonDifference>,
) {
    match (before, after) {
        (Some(before), Some(after)) if before == after => {}
        (Some(JsonValue::Object(before)), Some(JsonValue::Object(after))) => {
            let keys: BTreeSet<_> = before.keys().chain(after.keys()).collect();
            for key in keys {
                let child_pointer = append_pointer(pointer, key);
                collect_differences(&child_pointer, before.get(key), after.get(key), output);
            }
        }
        (Some(JsonValue::Array(before)), Some(JsonValue::Array(after))) => {
            for index in 0..before.len().max(after.len()) {
                let child_pointer = append_pointer(pointer, &index.to_string());
                collect_differences(&child_pointer, before.get(index), after.get(index), output);
            }
        }
        (before, after) => output.push(JsonDifference {
            pointer: pointer.to_owned(),
            before: before.map(JsonValue::canonical_compact),
            after: after.map(JsonValue::canonical_compact),
            approved_volatile: approved_volatile(pointer, before, after),
        }),
    }
}

fn approved_volatile(pointer: &str, before: Option<&JsonValue>, after: Option<&JsonValue>) -> bool {
    matches!(
        (pointer, before, after),
        (
            "/skeleton/hash",
            Some(JsonValue::String(before)),
            Some(JsonValue::String(after))
        ) if before != after
    )
}

fn framed_fingerprint(domain: &[u8], fields: &[&[u8]]) -> String {
    let capacity = domain.len()
        + fields.iter().map(|field| field.len()).sum::<usize>()
        + (fields.len() + 1) * size_of::<u64>();
    let mut framed = Vec::with_capacity(capacity);
    append_fingerprint_field(&mut framed, domain);
    for field in fields {
        append_fingerprint_field(&mut framed, field);
    }
    sha256_bytes(&framed)
}

fn append_fingerprint_field(framed: &mut Vec<u8>, field: &[u8]) {
    let length = u64::try_from(field.len()).expect("JSON resource limits fit within u64");
    framed.extend_from_slice(&length.to_be_bytes());
    framed.extend_from_slice(field);
}

fn append_pointer(parent: &str, token: &str) -> String {
    let mut pointer = String::with_capacity(parent.len() + token.len() + 1);
    pointer.push_str(parent);
    pointer.push('/');
    for character in token.chars() {
        match character {
            '~' => pointer.push_str("~0"),
            '/' => pointer.push_str("~1"),
            _ => pointer.push(character),
        }
    }
    pointer
}

fn write_indent(output: &mut String, indent: usize) {
    for _ in 0..indent {
        output.push_str("  ");
    }
}

fn write_json_string(output: &mut String, value: &str) {
    output.push_str(&serde_json::to_string(value).expect("serializing a string cannot fail"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(body: &str) -> JsonEvidence {
        JsonEvidence::from_slice(body.as_bytes(), JsonLimits::default()).expect("valid evidence")
    }

    #[test]
    fn rejects_duplicate_decoded_object_keys() {
        let error = JsonEvidence::from_slice(
            br#"{"skeleton":{},"animations":{},"animations":{}}"#,
            JsonLimits::default(),
        )
        .expect_err("duplicate literal key must fail");
        assert!(matches!(
            error,
            JsonEvidenceError::DuplicateObjectKey { ref key, .. } if key == "animations"
        ));

        let error = JsonEvidence::from_slice(
            br#"{"skeleton":{},"animations":{},"\u0061":1,"a":2}"#,
            JsonLimits::default(),
        )
        .expect_err("escape-equivalent key must fail");
        assert!(matches!(
            error,
            JsonEvidenceError::DuplicateObjectKey { ref key, .. } if key == "a"
        ));
    }

    #[test]
    fn canonical_output_sorts_objects_and_preserves_numbers_and_array_order() {
        let evidence = document(
            r#"{"z":1E+02,"skeleton":{"hash":"volatile"},"animations":{},"a":[3,1.0,-0,2e-3]}"#,
        );
        assert_eq!(
            evidence.canonical_pretty_json(),
            r#"{
  "a": [
    3,
    1.0,
    -0,
    2e-3
  ],
  "animations": {},
  "skeleton": {
    "hash": "volatile"
  },
  "z": 1E+02
}"#
        );
    }

    #[test]
    fn normalization_removes_only_the_fixed_volatile_pointer() {
        let first = document(r#"{"skeleton":{"hash":"one","x":1},"animations":{},"hash":"kept"}"#);
        let second = document(r#"{"skeleton":{"hash":"two","x":1},"animations":{},"hash":"kept"}"#);

        assert_eq!(
            first.normalized_pretty_json(),
            second.normalized_pretty_json()
        );
        let differences = first.semantic_differences(&second);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].pointer(), "/skeleton/hash");
        assert_eq!(differences[0].before_json(), Some(r#""one""#));
        assert_eq!(differences[0].after_json(), Some(r#""two""#));
        assert!(differences[0].approved_volatile());
        assert!(first.normalized_pretty_json().contains(r#""hash": "kept""#));
        assert!(!first.normalized_pretty_json().contains("one"));
    }

    #[test]
    fn volatile_approval_rejects_missing_equal_wrong_type_and_other_pointers() {
        let with_string = document(r#"{"skeleton":{"hash":"one"},"animations":{},"hash":"one"}"#);
        let without_hash = document(r#"{"skeleton":{},"animations":{},"hash":"one"}"#);
        let with_number = document(r#"{"skeleton":{"hash":1},"animations":{},"hash":"one"}"#);
        let added_hash = without_hash.semantic_differences(&with_string);
        let removed_hash = with_string.semantic_differences(&without_hash);
        let type_change = with_string.semantic_differences(&with_number);

        for differences in [&added_hash, &removed_hash, &type_change] {
            assert_eq!(differences.len(), 1);
            assert_eq!(differences[0].pointer(), "/skeleton/hash");
            assert!(!differences[0].approved_volatile());
        }

        assert!(with_string.semantic_differences(&with_string).is_empty());
        let equal = JsonValue::String("same".to_owned());
        assert!(!approved_volatile(
            "/skeleton/hash",
            Some(&equal),
            Some(&equal)
        ));

        let changed_other_pointer =
            document(r#"{"skeleton":{"hash":"one"},"animations":{},"hash":"two"}"#);
        let differences = with_string.semantic_differences(&changed_other_pointer);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].pointer(), "/hash");
        assert!(!differences[0].approved_volatile());
    }

    #[test]
    fn setup_and_animation_fingerprints_have_independent_scope() {
        let base = document(
            r#"{"skeleton":{"hash":"one","x":1},"slots":[],"animations":{"walk":{"a":1},"idle":{"a":2}}}"#,
        );
        let animation_change = document(
            r#"{"skeleton":{"hash":"two","x":1},"slots":[],"animations":{"walk":{"a":9},"idle":{"a":2}}}"#,
        );
        let setup_change = document(
            r#"{"skeleton":{"hash":"three","x":2},"slots":[],"animations":{"walk":{"a":1},"idle":{"a":2}}}"#,
        );

        assert_eq!(
            base.setup_fingerprint(),
            animation_change.setup_fingerprint()
        );
        assert_ne!(base.setup_fingerprint(), setup_change.setup_fingerprint());
        assert_eq!(
            base.animation_fingerprints().get("idle"),
            animation_change.animation_fingerprints().get("idle")
        );
        assert_ne!(
            base.animation_fingerprints().get("walk"),
            animation_change.animation_fingerprints().get("walk")
        );
        assert_eq!(
            base.animation_fingerprints(),
            setup_change.animation_fingerprints()
        );
    }

    #[test]
    fn fingerprints_are_domain_name_and_field_boundary_separated() {
        let evidence =
            document(r#"{"skeleton":{},"animations":{"left":{"value":1},"right":{"value":1}}}"#);
        assert_ne!(
            evidence.animation_fingerprints().get("left"),
            evidence.animation_fingerprints().get("right")
        );

        let body = br#"{"value":1}"#;
        assert_ne!(
            framed_fingerprint(SETUP_FINGERPRINT_DOMAIN, &[body]),
            framed_fingerprint(ANIMATION_FINGERPRINT_DOMAIN, &[body])
        );
        assert_ne!(
            framed_fingerprint(ANIMATION_FINGERPRINT_DOMAIN, &[b"a", b"bc"]),
            framed_fingerprint(ANIMATION_FINGERPRINT_DOMAIN, &[b"ab", b"c"])
        );
    }

    #[test]
    fn semantic_diff_reports_additions_removals_changes_and_escaped_pointers() {
        let before = document(
            r#"{"skeleton":{},"animations":{},"gone":true,"same":0,"a/b":{"~key":[1,2]}}"#,
        );
        let after = document(
            r#"{"skeleton":{},"animations":{},"added":null,"same":1,"a/b":{"~key":[1,3,4]}}"#,
        );

        let differences = before.semantic_differences(&after);
        let observed: Vec<_> = differences
            .iter()
            .map(|difference| {
                (
                    difference.pointer(),
                    difference.before_json(),
                    difference.after_json(),
                )
            })
            .collect();
        assert_eq!(
            observed,
            vec![
                ("/a~1b/~0key/1", Some("2"), Some("3")),
                ("/a~1b/~0key/2", None, Some("4")),
                ("/added", None, Some("null")),
                ("/gone", Some("true"), None),
                ("/same", Some("0"), Some("1")),
            ]
        );
    }

    #[test]
    fn requires_object_skeleton_and_animations_members() {
        let cases = [
            (r#"{"animations":{}}"#, JsonEvidenceError::MissingSkeleton),
            (
                r#"{"skeleton":null,"animations":{}}"#,
                JsonEvidenceError::SkeletonNotObject,
            ),
            (r#"{"skeleton":{}}"#, JsonEvidenceError::MissingAnimations),
            (
                r#"{"skeleton":{},"animations":[]}"#,
                JsonEvidenceError::AnimationsNotObject,
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(
                JsonEvidence::from_slice(input.as_bytes(), JsonLimits::default()),
                Err(expected)
            );
        }
    }

    #[test]
    fn enforces_byte_depth_and_node_bounds() {
        let byte_limits = JsonLimits::new(8, 8, 8).expect("valid limits");
        assert!(matches!(
            JsonEvidence::from_slice(br#"{"skeleton":{},"animations":{}}"#, byte_limits),
            Err(JsonEvidenceError::InputTooLarge { .. })
        ));

        let depth_limits = JsonLimits::new(1_024, 2, 100).expect("valid limits");
        assert!(matches!(
            JsonEvidence::from_slice(
                br#"{"skeleton":{"nested":{}},"animations":{}}"#,
                depth_limits
            ),
            Err(JsonEvidenceError::DepthLimitExceeded { limit: 2 })
        ));

        let node_limits = JsonLimits::new(1_024, 16, 2).expect("valid limits");
        assert!(matches!(
            JsonEvidence::from_slice(br#"{"skeleton":{},"animations":{}}"#, node_limits),
            Err(JsonEvidenceError::NodeLimitExceeded { limit: 2 })
        ));
    }

    #[test]
    fn rejects_invalid_utf8_and_invalid_limits() {
        assert!(matches!(
            JsonEvidence::from_slice(&[0xff], JsonLimits::default()),
            Err(JsonEvidenceError::InvalidUtf8 { offset: 0 })
        ));
        assert_eq!(
            JsonLimits::new(0, 1, 1),
            Err(JsonEvidenceError::InvalidLimits)
        );
        assert_eq!(
            JsonLimits::new(1, MAX_SUPPORTED_DEPTH + 1, 1),
            Err(JsonEvidenceError::InvalidLimits)
        );
    }
}

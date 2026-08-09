//! Strict, bounded comparison of the frozen Phase 0B v1 event window.
//!
//! Agreement here is rehearsal-only: this module can never make an observation
//! gate-eligible.

use std::fmt::Debug;

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::contract::{
    ANIMATION_NAME, EVENT_FLOAT_ABS, EVENT_WINDOW_END_NS, EVENT_WINDOW_ID, EVENT_WINDOW_START_NS,
};

/// Accepted observation schema version.
pub const EVENT_WINDOW_FORMAT_VERSION: u16 = 1;
/// Emitted comparison-report schema version.
pub const EVENT_COMPARISON_FORMAT_VERSION: u16 = 1;
/// Maximum bytes accepted for one JSON document.
pub const MAX_EVENT_WINDOW_BYTES: usize = 256 * 1024;
/// Maximum occurrences accepted in the one-second window.
pub const MAX_EVENT_COUNT: usize = 1024;
/// Maximum UTF-8 bytes in an identifier or diagnostic code.
pub const MAX_EVENT_IDENTIFIER_BYTES: usize = 256;
/// Maximum UTF-8 bytes in an authored string payload.
pub const MAX_EVENT_STRING_BYTES: usize = 4096;
/// Maximum diagnostic codes accepted on one occurrence.
pub const MAX_DIAGNOSTIC_CODES: usize = 64;
/// Maximum retained comparison differences.
pub const MAX_EVENT_DIFFERENCES: usize = 256;

const MAX_PATH_BYTES: usize = 192;
const MAX_VALUE_BYTES: usize = 256;
const MAX_ERROR_BYTES: usize = 256;

/// One validated authored event occurrence.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EventOccurrence {
    animation: Box<str>,
    name: Box<str>,
    local_time_ns: u64,
    loop_index: u64,
    integer: i32,
    float: f64,
    string: Option<Box<str>>,
    volume: f64,
    balance: f64,
    diagnostic_codes: Box<[Box<str>]>,
}

impl EventOccurrence {
    /// Animation name.
    pub fn animation(&self) -> &str {
        &self.animation
    }
    /// Event-definition name.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Animation-local occurrence time.
    pub const fn local_time_ns(&self) -> u64 {
        self.local_time_ns
    }
    /// Zero-based loop index.
    pub const fn loop_index(&self) -> u64 {
        self.loop_index
    }
    /// Resolved integer payload.
    pub const fn integer(&self) -> i32 {
        self.integer
    }
    /// Resolved floating-point payload.
    pub const fn float(&self) -> f64 {
        self.float
    }
    /// Resolved optional string payload.
    pub fn string(&self) -> Option<&str> {
        self.string.as_deref()
    }
    /// Resolved volume payload.
    pub const fn volume(&self) -> f64 {
        self.volume
    }
    /// Resolved balance payload.
    pub const fn balance(&self) -> f64 {
        self.balance
    }
    /// Ordered diagnostic codes.
    pub fn diagnostic_codes(&self) -> &[Box<str>] {
        &self.diagnostic_codes
    }
}

/// A validated observation of the sole frozen v1 event window.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EventWindowDocument {
    format_version: u16,
    window_id: Box<str>,
    animation: Box<str>,
    start_ns: u64,
    end_ns: u64,
    events: Box<[EventOccurrence]>,
}

impl EventWindowDocument {
    /// Schema version.
    pub const fn format_version(&self) -> u16 {
        self.format_version
    }
    /// Window identifier.
    pub fn window_id(&self) -> &str {
        &self.window_id
    }
    /// Window animation.
    pub fn animation(&self) -> &str {
        &self.animation
    }
    /// Inclusive start time.
    pub const fn start_ns(&self) -> u64 {
        self.start_ns
    }
    /// Inclusive end time.
    pub const fn end_ns(&self) -> u64 {
        self.end_ns
    }
    /// Occurrences in emission order.
    pub fn events(&self) -> &[EventOccurrence] {
        &self.events
    }
    /// Always false for the frozen rehearsal contract.
    pub const fn gate_eligible(&self) -> bool {
        false
    }
}

/// Compatibility name for callers treating a parsed document as an observation.
pub type EventWindowObservation = EventWindowDocument;

/// One bounded comparison difference.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EventDifference {
    path: Box<str>,
    expected: Box<str>,
    actual: Box<str>,
}

impl EventDifference {
    /// Stable field path.
    pub fn path(&self) -> &str {
        &self.path
    }
    /// Bounded reference rendering.
    pub fn expected(&self) -> &str {
        &self.expected
    }
    /// Bounded observed rendering.
    pub fn actual(&self) -> &str {
        &self.actual
    }
}

/// Bounded v1 event comparison report.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EventComparison {
    format_version: u16,
    fixed_policy_agreement: bool,
    diagnostic_policy_agreement: bool,
    gate_eligible: bool,
    differences: Vec<EventDifference>,
    omitted_difference_count: usize,
}

impl EventComparison {
    /// Report schema version.
    pub const fn format_version(&self) -> u16 {
        self.format_version
    }
    /// Whether all exact, tolerant, and empty-diagnostic rules agree.
    pub const fn fixed_policy_agreement(&self) -> bool {
        self.fixed_policy_agreement
    }
    /// Whether both inputs contain no diagnostic codes.
    pub const fn diagnostic_policy_agreement(&self) -> bool {
        self.diagnostic_policy_agreement
    }
    /// Always false for the frozen rehearsal contract.
    pub const fn gate_eligible(&self) -> bool {
        self.gate_eligible
    }
    /// Retained differences in deterministic traversal order.
    pub fn differences(&self) -> &[EventDifference] {
        &self.differences
    }
    /// Differences omitted after the fixed cap.
    pub const fn omitted_difference_count(&self) -> usize {
        self.omitted_difference_count
    }
}

/// Strict event-window parsing failure.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum EventWindowError {
    /// Raw input exceeded the byte bound.
    #[error("event-window JSON is {actual} bytes; maximum is {maximum}")]
    InputTooLarge {
        /// Actual length.
        actual: usize,
        /// Fixed maximum.
        maximum: usize,
    },
    /// JSON syntax or strict typed shape was invalid.
    #[error("invalid event-window JSON: {message}")]
    InvalidJson {
        /// Bounded parser detail.
        message: Box<str>,
    },
    /// A typed value violated the frozen contract.
    #[error("invalid event-window value at {path}: {message}")]
    InvalidValue {
        /// Stable field path.
        path: Box<str>,
        /// Bounded validation detail.
        message: Box<str>,
    },
}

/// A comparison input failed strict parsing.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum EventCompareError {
    /// Invalid reference document.
    #[error("expected event window is invalid: {0}")]
    InvalidExpected(EventWindowError),
    /// Invalid observed document.
    #[error("actual event window is invalid: {0}")]
    InvalidActual(EventWindowError),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDocument {
    format_version: u16,
    window_id: Box<str>,
    animation: Box<str>,
    start_ns: u64,
    end_ns: u64,
    events: Vec<RawEvent>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEvent {
    animation: Box<str>,
    name: Box<str>,
    local_time_ns: u64,
    loop_index: u64,
    integer: i32,
    float: f64,
    #[serde(deserialize_with = "deserialize_required_nullable_string")]
    string: Option<Box<str>>,
    volume: f64,
    balance: f64,
    diagnostic_codes: Vec<Box<str>>,
}

fn deserialize_required_nullable_string<'de, D>(
    deserializer: D,
) -> Result<Option<Box<str>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<Box<str>>::deserialize(deserializer)
}

/// Parses strict JSON and validates every fixed v1 value and bound.
pub fn parse_event_window_json(json: &[u8]) -> Result<EventWindowDocument, EventWindowError> {
    if json.len() > MAX_EVENT_WINDOW_BYTES {
        return Err(EventWindowError::InputTooLarge {
            actual: json.len(),
            maximum: MAX_EVENT_WINDOW_BYTES,
        });
    }
    let raw: RawDocument =
        serde_json::from_slice(json).map_err(|error| EventWindowError::InvalidJson {
            message: bounded(&error.to_string(), MAX_ERROR_BYTES),
        })?;
    validate(raw)
}

/// Strictly parses and compares reference and observed JSON documents.
pub fn compare_event_window_json(
    expected: &[u8],
    actual: &[u8],
) -> Result<EventComparison, EventCompareError> {
    let expected = parse_event_window_json(expected).map_err(EventCompareError::InvalidExpected)?;
    let actual = parse_event_window_json(actual).map_err(EventCompareError::InvalidActual)?;
    Ok(compare_event_windows(&expected, &actual))
}

/// Compares two already validated documents in emitted list order.
#[must_use]
pub fn compare_event_windows(
    expected: &EventWindowDocument,
    actual: &EventWindowDocument,
) -> EventComparison {
    let mut diffs = Diffs::default();
    diffs.exact(
        "format_version",
        &expected.format_version,
        &actual.format_version,
    );
    diffs.exact("window_id", &expected.window_id, &actual.window_id);
    diffs.exact("animation", &expected.animation, &actual.animation);
    diffs.exact("start_ns", &expected.start_ns, &actual.start_ns);
    diffs.exact("end_ns", &expected.end_ns, &actual.end_ns);
    diffs.length("events", expected.events.len(), actual.events.len());
    let no_diagnostics = expected
        .events
        .iter()
        .chain(actual.events.iter())
        .all(|event| event.diagnostic_codes.is_empty());
    for (index, (left, right)) in expected.events.iter().zip(actual.events.iter()).enumerate() {
        compare_event(&mut diffs, index, left, right);
    }
    let agrees = no_diagnostics && diffs.retained.is_empty() && diffs.omitted == 0;
    EventComparison {
        format_version: EVENT_COMPARISON_FORMAT_VERSION,
        fixed_policy_agreement: agrees,
        diagnostic_policy_agreement: no_diagnostics,
        gate_eligible: false,
        differences: diffs.retained,
        omitted_difference_count: diffs.omitted,
    }
}

fn validate(raw: RawDocument) -> Result<EventWindowDocument, EventWindowError> {
    fixed(
        "format_version",
        &raw.format_version,
        &EVENT_WINDOW_FORMAT_VERSION,
    )?;
    fixed("window_id", &raw.window_id.as_ref(), &EVENT_WINDOW_ID)?;
    fixed("animation", &raw.animation.as_ref(), &ANIMATION_NAME)?;
    fixed("start_ns", &raw.start_ns, &EVENT_WINDOW_START_NS)?;
    fixed("end_ns", &raw.end_ns, &EVENT_WINDOW_END_NS)?;
    if raw.events.len() > MAX_EVENT_COUNT {
        return bad("events", format!("count exceeds {MAX_EVENT_COUNT}"));
    }
    let mut previous = None;
    let mut events = Vec::with_capacity(raw.events.len());
    for (index, event) in raw.events.into_iter().enumerate() {
        let base = format!("events[{index}]");
        fixed(
            &format!("{base}.animation"),
            &event.animation.as_ref(),
            &ANIMATION_NAME,
        )?;
        text(
            &format!("{base}.name"),
            &event.name,
            MAX_EVENT_IDENTIFIER_BYTES,
            false,
        )?;
        if !(EVENT_WINDOW_START_NS..=EVENT_WINDOW_END_NS).contains(&event.local_time_ns) {
            return bad(
                format!("{base}.local_time_ns"),
                "outside the inclusive event window",
            );
        }
        if previous.is_some_and(|time| event.local_time_ns < time) {
            return bad(
                format!("{base}.local_time_ns"),
                "events are not time ordered",
            );
        }
        previous = Some(event.local_time_ns);
        fixed(&format!("{base}.loop_index"), &event.loop_index, &0_u64)?;
        number(&format!("{base}.float"), event.float)?;
        let string = event.string;
        if let Some(value) = string.as_deref() {
            text(
                &format!("{base}.string"),
                value,
                MAX_EVENT_STRING_BYTES,
                true,
            )?;
        }
        number(&format!("{base}.volume"), event.volume)?;
        number(&format!("{base}.balance"), event.balance)?;
        if event.diagnostic_codes.len() > MAX_DIAGNOSTIC_CODES {
            return bad(
                format!("{base}.diagnostic_codes"),
                format!("count exceeds {MAX_DIAGNOSTIC_CODES}"),
            );
        }
        for (code_index, code) in event.diagnostic_codes.iter().enumerate() {
            text(
                &format!("{base}.diagnostic_codes[{code_index}]"),
                code,
                MAX_EVENT_IDENTIFIER_BYTES,
                false,
            )?;
        }
        events.push(EventOccurrence {
            animation: event.animation,
            name: event.name,
            local_time_ns: event.local_time_ns,
            loop_index: event.loop_index,
            integer: event.integer,
            float: event.float,
            string,
            volume: event.volume,
            balance: event.balance,
            diagnostic_codes: event.diagnostic_codes.into_boxed_slice(),
        });
    }
    Ok(EventWindowDocument {
        format_version: raw.format_version,
        window_id: raw.window_id,
        animation: raw.animation,
        start_ns: raw.start_ns,
        end_ns: raw.end_ns,
        events: events.into_boxed_slice(),
    })
}

fn fixed<T: Debug + PartialEq>(
    path: &str,
    actual: &T,
    expected: &T,
) -> Result<(), EventWindowError> {
    if actual == expected {
        Ok(())
    } else {
        bad(path, format!("must equal {expected:?}"))
    }
}

fn number(path: &str, value: f64) -> Result<(), EventWindowError> {
    if value.is_finite() && value.abs() <= f64::from(f32::MAX) {
        Ok(())
    } else {
        bad(path, "must be finite and representable as f32")
    }
}

fn text(path: &str, value: &str, limit: usize, empty: bool) -> Result<(), EventWindowError> {
    let unsafe_scalar = |character: char| {
        character.is_control()
            || matches!(
                u32::from(character),
                0x2028..=0x202e | 0x2066..=0x2069 | 0xfeff
            )
    };
    if (!empty && value.is_empty()) || value.len() > limit || value.chars().any(unsafe_scalar) {
        bad(path, format!("must be safe UTF-8 of at most {limit} bytes"))
    } else {
        Ok(())
    }
}

fn error(path: &str, message: &str) -> EventWindowError {
    EventWindowError::InvalidValue {
        path: bounded(path, MAX_PATH_BYTES),
        message: bounded(message, MAX_ERROR_BYTES),
    }
}

fn bad<T>(path: impl AsRef<str>, message: impl AsRef<str>) -> Result<T, EventWindowError> {
    Err(error(path.as_ref(), message.as_ref()))
}

fn compare_event(diffs: &mut Diffs, index: usize, left: &EventOccurrence, right: &EventOccurrence) {
    let base = format!("events[{index}]");
    macro_rules! exact {
        ($field:ident) => {
            diffs.exact(
                &format!("{base}.{}", stringify!($field)),
                &left.$field,
                &right.$field,
            );
        };
    }
    exact!(animation);
    exact!(name);
    exact!(local_time_ns);
    exact!(loop_index);
    exact!(integer);
    diffs.numeric(&format!("{base}.float"), left.float, right.float);
    exact!(string);
    diffs.numeric(&format!("{base}.volume"), left.volume, right.volume);
    diffs.numeric(&format!("{base}.balance"), left.balance, right.balance);
    diffs.length(
        &format!("{base}.diagnostic_codes"),
        left.diagnostic_codes.len(),
        right.diagnostic_codes.len(),
    );
    for (code_index, (a, b)) in left
        .diagnostic_codes
        .iter()
        .zip(right.diagnostic_codes.iter())
        .enumerate()
    {
        diffs.exact(&format!("{base}.diagnostic_codes[{code_index}]"), a, b);
    }
    if !left.diagnostic_codes.is_empty() || !right.diagnostic_codes.is_empty() {
        diffs.record(
            &format!("{base}.diagnostic_codes.v1_allowlist"),
            &[] as &[&str],
            &(
                left.diagnostic_codes.as_ref(),
                right.diagnostic_codes.as_ref(),
            ),
        );
    }
}

#[derive(Default)]
struct Diffs {
    retained: Vec<EventDifference>,
    omitted: usize,
}

impl Diffs {
    fn exact<T: Debug + PartialEq + ?Sized>(&mut self, path: &str, a: &T, b: &T) {
        if a != b {
            self.record(path, a, b);
        }
    }
    fn numeric(&mut self, path: &str, a: f64, b: f64) {
        if (a - b).abs() > EVENT_FLOAT_ABS {
            self.record(path, &a, &b);
        }
    }
    fn length(&mut self, path: &str, a: usize, b: usize) {
        self.exact(&format!("{path}.length"), &a, &b);
    }
    fn record<A: Debug + ?Sized, B: Debug + ?Sized>(&mut self, path: &str, a: &A, b: &B) {
        if self.retained.len() == MAX_EVENT_DIFFERENCES {
            self.omitted = self.omitted.saturating_add(1);
        } else {
            self.retained.push(EventDifference {
                path: bounded(path, MAX_PATH_BYTES),
                expected: bounded(&format!("{a:?}"), MAX_VALUE_BYTES),
                actual: bounded(&format!("{b:?}"), MAX_VALUE_BYTES),
            });
        }
    }
}

fn bounded(value: &str, maximum: usize) -> Box<str> {
    if value.len() <= maximum {
        return Box::from(value);
    }
    let mut end = maximum.saturating_sub(3);
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &value[..end]).into_boxed_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(
        name: &str,
        time: u64,
        float: f64,
        volume: f64,
        balance: f64,
        codes: &[&str],
    ) -> String {
        format!(
            concat!(
                "{{\"animation\":\"sway\",\"name\":{},\"local_time_ns\":{},",
                "\"loop_index\":0,\"integer\":7,\"float\":{},\"string\":\"soft\",",
                "\"volume\":{},\"balance\":{},\"diagnostic_codes\":{}}}"
            ),
            serde_json::to_string(name).unwrap(),
            time,
            float,
            volume,
            balance,
            serde_json::to_string(codes).unwrap()
        )
    }

    fn document(events: &[String]) -> String {
        format!(
            concat!(
                "{{\"format_version\":1,\"window_id\":\"sway-events\",",
                "\"animation\":\"sway\",\"start_ns\":0,\"end_ns\":1000000000,\"events\":[{}]}}"
            ),
            events.join(",")
        )
    }

    fn valid() -> String {
        document(&[event("step", 250_000_000, 0.0, 0.0, 0.0, &[])])
    }

    fn rejected(json: String) {
        assert!(parse_event_window_json(json.as_bytes()).is_err(), "{json}");
    }

    #[test]
    fn parses_all_fields_and_equal_report_is_gate_ineligible() {
        let json = valid();
        let parsed = parse_event_window_json(json.as_bytes()).unwrap();
        let item = &parsed.events()[0];
        assert_eq!(
            (
                parsed.format_version(),
                parsed.window_id(),
                parsed.animation()
            ),
            (1, EVENT_WINDOW_ID, ANIMATION_NAME)
        );
        assert_eq!(
            (parsed.start_ns(), parsed.end_ns()),
            (0, EVENT_WINDOW_END_NS)
        );
        assert_eq!(
            (
                item.animation(),
                item.name(),
                item.local_time_ns(),
                item.loop_index()
            ),
            ("sway", "step", 250_000_000, 0)
        );
        assert_eq!(
            (
                item.integer(),
                item.float(),
                item.string(),
                item.volume(),
                item.balance()
            ),
            (7, 0.0, Some("soft"), 0.0, 0.0)
        );
        assert!(item.diagnostic_codes().is_empty());
        assert!(!parsed.gate_eligible());
        let explicit_null = json.replace("\"string\":\"soft\"", "\"string\":null");
        assert_eq!(
            parse_event_window_json(explicit_null.as_bytes())
                .unwrap()
                .events()[0]
                .string(),
            None
        );
        let report = compare_event_window_json(json.as_bytes(), json.as_bytes()).unwrap();
        assert!(report.fixed_policy_agreement() && report.diagnostic_policy_agreement());
        assert!(!report.gate_eligible());
        assert_eq!(
            (report.format_version(), report.omitted_difference_count()),
            (1, 0)
        );
        assert!(report.differences().is_empty());
        assert_eq!(
            serde_json::to_value(report).unwrap()["gate_eligible"],
            false
        );
    }

    #[test]
    fn serde_rejects_unknown_missing_duplicate_and_wrong_typed_fields() {
        let json = valid();
        for bad in [
            json.replace("\"events\":[", "\"unknown\":0,\"events\":["),
            json.replace("\"name\":\"step\"", "\"name\":\"step\",\"extra\":0"),
            json.replace("\"integer\":7,", ""),
            json.replace("\"string\":\"soft\",", ""),
            json.replace("\"integer\":7", "\"integer\":7,\"integer\":8"),
            json.replace(
                "\"format_version\":1",
                "\"format_version\":1,\"format_version\":1",
            ),
            json.replace("\"integer\":7", "\"integer\":2147483648"),
        ] {
            rejected(bad);
        }
        assert!(matches!(
            compare_event_window_json(b"{}", json.as_bytes()),
            Err(EventCompareError::InvalidExpected(_))
        ));
        assert!(matches!(
            compare_event_window_json(json.as_bytes(), b"{}"),
            Err(EventCompareError::InvalidActual(_))
        ));
    }

    #[test]
    fn fixed_window_animation_time_loop_and_order_are_enforced() {
        let json = valid();
        for bad in [
            json.replace("\"format_version\":1", "\"format_version\":2"),
            json.replace("sway-events", "other"),
            json.replacen("\"animation\":\"sway\"", "\"animation\":\"walk\"", 1),
            json.replace("\"start_ns\":0", "\"start_ns\":1"),
            json.replace("\"end_ns\":1000000000", "\"end_ns\":999999999"),
            json.replace("\"loop_index\":0", "\"loop_index\":1"),
            document(&[event("late", EVENT_WINDOW_END_NS + 1, 0.0, 0.0, 0.0, &[])]),
            document(&[
                event("b", 2, 0.0, 0.0, 0.0, &[]),
                event("a", 1, 0.0, 0.0, 0.0, &[]),
            ]),
        ] {
            rejected(bad);
        }
        let position = json.rfind("\"animation\":\"sway\"").unwrap();
        let mut wrong_event_animation = json;
        wrong_event_animation.replace_range(
            position..position + "\"animation\":\"sway\"".len(),
            "\"animation\":\"walk\"",
        );
        rejected(wrong_event_animation);
    }

    #[test]
    fn floats_reject_bad_ranges_and_use_inclusive_fixed_tolerance() {
        let base = document(&[event("step", 0, 0.0, 0.0, 0.0, &[])]);
        for field in ["float", "volume", "balance"] {
            rejected(base.replace(&format!("\"{field}\":0"), &format!("\"{field}\":3.5e38")));
            let at = base.replace(
                &format!("\"{field}\":0"),
                &format!("\"{field}\":{EVENT_FLOAT_ABS}"),
            );
            assert!(
                compare_event_window_json(base.as_bytes(), at.as_bytes())
                    .unwrap()
                    .fixed_policy_agreement()
            );
            let above = base.replace(
                &format!("\"{field}\":0"),
                &format!("\"{field}\":{}", EVENT_FLOAT_ABS * 1.01),
            );
            assert!(
                !compare_event_window_json(base.as_bytes(), above.as_bytes())
                    .unwrap()
                    .fixed_policy_agreement()
            );
        }
        rejected(base.replace("\"float\":0", "\"float\":1e400"));
    }

    #[test]
    fn unsafe_strings_and_counts_are_bounded() {
        rejected(document(&[event("", 0, 0.0, 0.0, 0.0, &[])]));
        rejected(document(&[event("line\nbreak", 0, 0.0, 0.0, 0.0, &[])]));
        rejected(document(&[event(
            &"n".repeat(MAX_EVENT_IDENTIFIER_BYTES + 1),
            0,
            0.0,
            0.0,
            0.0,
            &[],
        )]));
        rejected(valid().replace(
            "\"soft\"",
            &serde_json::to_string(&"s".repeat(MAX_EVENT_STRING_BYTES + 1)).unwrap(),
        ));
        let codes = vec!["code"; MAX_DIAGNOSTIC_CODES + 1];
        rejected(document(&[event("step", 0, 0.0, 0.0, 0.0, &codes)]));
        let events = vec![event("step", 0, 0.0, 0.0, 0.0, &[]); MAX_EVENT_COUNT + 1];
        let too_many = document(&events);
        assert!(too_many.len() <= MAX_EVENT_WINDOW_BYTES);
        rejected(too_many);
        assert!(matches!(
            parse_event_window_json(&vec![b' '; MAX_EVENT_WINDOW_BYTES + 1]),
            Err(EventWindowError::InputTooLarge { .. })
        ));
    }

    #[test]
    fn structure_and_same_timestamp_order_are_exact() {
        let left = document(&[
            event("first", 1, 0.0, 0.0, 0.0, &[]),
            event("second", 1, 0.0, 0.0, 0.0, &[]),
        ]);
        let right = document(&[
            event("second", 1, 0.0, 0.0, 0.0, &[]),
            event("first", 1, 0.0, 0.0, 0.0, &[]),
        ]);
        let report = compare_event_window_json(left.as_bytes(), right.as_bytes()).unwrap();
        assert_eq!(
            report
                .differences()
                .iter()
                .map(EventDifference::path)
                .collect::<Vec<_>>(),
            ["events[0].name", "events[1].name"]
        );
        assert!(!report.fixed_policy_agreement());
    }

    #[test]
    fn diagnostics_defeat_the_empty_v1_allowlist() {
        let json = document(&[event("step", 0, 0.0, 0.0, 0.0, &["degraded"])]);
        let report = compare_event_window_json(json.as_bytes(), json.as_bytes()).unwrap();
        assert!(!report.fixed_policy_agreement());
        assert!(!report.diagnostic_policy_agreement());
        assert!(!report.gate_eligible());
        assert_eq!(
            report.differences()[0].path(),
            "events[0].diagnostic_codes.v1_allowlist"
        );
    }

    #[test]
    fn differences_and_renderings_are_bounded() {
        let left: Vec<_> = (0..300)
            .map(|i| event(&format!("left-{i}"), 0, 0.0, 0.0, 0.0, &[]))
            .collect();
        let right: Vec<_> = (0..300)
            .map(|i| event(&format!("right-{i}"), 0, 0.0, 0.0, 0.0, &[]))
            .collect();
        let report =
            compare_event_window_json(document(&left).as_bytes(), document(&right).as_bytes())
                .unwrap();
        assert_eq!(report.differences().len(), MAX_EVENT_DIFFERENCES);
        assert_eq!(
            report.omitted_difference_count(),
            300 - MAX_EVENT_DIFFERENCES
        );
        assert!(report.differences().iter().all(|difference| {
            difference.path().len() <= MAX_PATH_BYTES
                && difference.expected().len() <= MAX_VALUE_BYTES
                && difference.actual().len() <= MAX_VALUE_BYTES
        }));
    }
}

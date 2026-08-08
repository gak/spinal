//! Versioned, capability-checked commands for the thin browser host.

use std::{collections::VecDeque, error::Error, fmt};

use serde::Deserialize;

use crate::command::{StepDirection, ViewerCommand};

pub(crate) const BROWSER_COMMAND_VERSION: u16 = 1;
pub(crate) const MAX_BROWSER_COMMAND_BYTES: usize = 512;
pub(crate) const BROWSER_COMMAND_QUEUE_CAPACITY: usize = 32;
const CAPABILITY_HEX_LENGTH: usize = 64;

/// Browser facts that must be established before an envelope is parsed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BrowserMessageContext<'a> {
    pub(crate) page_origin: &'a str,
    pub(crate) event_origin: &'a str,
    pub(crate) self_source: bool,
}

/// Stateful authorization for one browser-viewer launch.
#[derive(Debug)]
pub(crate) struct BrowserCommandProtocol {
    capability: Box<str>,
    last_sequence: u64,
}

impl BrowserCommandProtocol {
    pub(crate) fn new(capability: impl Into<Box<str>>) -> Result<Self, BrowserCommandError> {
        let capability = capability.into();
        if !valid_capability(&capability) {
            return Err(BrowserCommandError::InvalidLaunchCapability);
        }
        Ok(Self {
            capability,
            last_sequence: 0,
        })
    }

    /// Authenticates and converts exactly one bounded wire envelope.
    ///
    /// Failed envelopes never consume their sequence number. Accepted sequence
    /// numbers must increase, preventing duplicated browser events from
    /// toggling playback twice.
    pub(crate) fn authorize(
        &mut self,
        encoded: &str,
        context: BrowserMessageContext<'_>,
    ) -> Result<ViewerCommand, BrowserCommandError> {
        if !context.self_source {
            return Err(BrowserCommandError::WrongSource);
        }
        if context.event_origin != context.page_origin {
            return Err(BrowserCommandError::WrongOrigin);
        }
        if encoded.is_empty() || encoded.len() > MAX_BROWSER_COMMAND_BYTES {
            return Err(BrowserCommandError::InvalidSize);
        }
        let envelope: BrowserCommandEnvelope = serde_json::from_str(encoded)
            .map_err(|_error| BrowserCommandError::MalformedEnvelope)?;
        let BrowserCommandEnvelope {
            message_type: BrowserMessageType::ViewerCommand,
            version,
            capability,
            sequence,
            action,
        } = envelope;
        if version != BROWSER_COMMAND_VERSION {
            return Err(BrowserCommandError::WrongVersion);
        }
        if capability.as_ref() != self.capability.as_ref() {
            return Err(BrowserCommandError::WrongCapability);
        }
        if sequence == 0 || sequence <= self.last_sequence {
            return Err(BrowserCommandError::StaleSequence);
        }

        let command = match action {
            BrowserAction::TogglePause => ViewerCommand::TogglePause,
            BrowserAction::StepBackward => ViewerCommand::Step(StepDirection::Backward),
            BrowserAction::StepForward => ViewerCommand::Step(StepDirection::Forward),
            BrowserAction::Restart => ViewerCommand::Restart,
            BrowserAction::Refit => ViewerCommand::Refit,
        };
        self.last_sequence = sequence;
        Ok(command)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BrowserCommandEnvelope {
    #[serde(rename = "type")]
    message_type: BrowserMessageType,
    version: u16,
    capability: Box<str>,
    sequence: u64,
    action: BrowserAction,
}

#[derive(Debug, Deserialize)]
enum BrowserMessageType {
    #[serde(rename = "spinal.viewer.command")]
    ViewerCommand,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum BrowserAction {
    TogglePause,
    StepBackward,
    StepForward,
    Restart,
    Refit,
}

/// Stable rejection classes that never contain the launch capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserCommandError {
    InvalidLaunchCapability,
    WrongSource,
    WrongOrigin,
    InvalidSize,
    MalformedEnvelope,
    WrongVersion,
    WrongCapability,
    StaleSequence,
}

impl fmt::Display for BrowserCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let detail = match self {
            Self::InvalidLaunchCapability => "invalid launch capability",
            Self::WrongSource => "browser command source is not this window",
            Self::WrongOrigin => "browser command origin does not match this page",
            Self::InvalidSize => "browser command envelope has an invalid size",
            Self::MalformedEnvelope => "browser command envelope is malformed",
            Self::WrongVersion => "browser command protocol version is unsupported",
            Self::WrongCapability => "browser command capability is invalid",
            Self::StaleSequence => "browser command sequence is stale",
        };
        formatter.write_str(detail)
    }
}

impl Error for BrowserCommandError {}

fn valid_capability(value: &str) -> bool {
    value.len() == CAPABILITY_HEX_LENGTH
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

/// One bounded FIFO for commands waiting to enter Bevy's shared inbox.
#[derive(Debug, Default)]
pub(crate) struct BrowserCommandQueue {
    commands: VecDeque<ViewerCommand>,
    overflowed: bool,
}

impl BrowserCommandQueue {
    /// Adds one command, or drops the newest command and records overflow.
    pub(crate) fn try_push(
        &mut self,
        command: ViewerCommand,
    ) -> Result<(), BrowserCommandQueueError> {
        if self.commands.len() >= BROWSER_COMMAND_QUEUE_CAPACITY {
            self.overflowed = true;
            return Err(BrowserCommandQueueError::Full);
        }
        self.commands.push_back(command);
        Ok(())
    }

    pub(crate) fn drain(&mut self) -> BrowserCommandBatch {
        BrowserCommandBatch {
            commands: self.commands.drain(..).collect(),
            overflowed: std::mem::take(&mut self.overflowed),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserCommandQueueError {
    Full,
}

impl fmt::Display for BrowserCommandQueueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("browser command queue is full")
    }
}

impl Error for BrowserCommandQueueError {}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct BrowserCommandBatch {
    pub(crate) commands: Vec<ViewerCommand>,
    pub(crate) overflowed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAPABILITY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const PAGE_ORIGIN: &str = "http://127.0.0.1:8424";

    fn context() -> BrowserMessageContext<'static> {
        BrowserMessageContext {
            page_origin: PAGE_ORIGIN,
            event_origin: PAGE_ORIGIN,
            self_source: true,
        }
    }

    fn envelope(capability: &str, sequence: u64, action: &str) -> String {
        format!(
            r#"{{"type":"spinal.viewer.command","version":1,"capability":"{capability}","sequence":{sequence},"action":"{action}"}}"#
        )
    }

    #[test]
    fn exact_v1_envelopes_map_only_to_existing_shared_commands() {
        let mut protocol = BrowserCommandProtocol::new(CAPABILITY).expect("valid capability");
        for (sequence, action, expected) in [
            (1, "toggle-pause", ViewerCommand::TogglePause),
            (
                2,
                "step-backward",
                ViewerCommand::Step(StepDirection::Backward),
            ),
            (
                3,
                "step-forward",
                ViewerCommand::Step(StepDirection::Forward),
            ),
            (4, "restart", ViewerCommand::Restart),
            (5, "refit", ViewerCommand::Refit),
        ] {
            assert_eq!(
                protocol
                    .authorize(&envelope(CAPABILITY, sequence, action), context())
                    .expect("authorized command"),
                expected
            );
        }
    }

    #[test]
    fn source_origin_version_capability_and_sequence_fail_closed() {
        let mut protocol = BrowserCommandProtocol::new(CAPABILITY).expect("valid capability");
        let valid = envelope(CAPABILITY, 1, "restart");

        assert_eq!(
            protocol.authorize(
                &valid,
                BrowserMessageContext {
                    self_source: false,
                    ..context()
                }
            ),
            Err(BrowserCommandError::WrongSource)
        );
        assert_eq!(
            protocol.authorize(
                &valid,
                BrowserMessageContext {
                    event_origin: "https://example.invalid",
                    ..context()
                }
            ),
            Err(BrowserCommandError::WrongOrigin)
        );
        assert_eq!(
            protocol.authorize(&valid.replace("\"version\":1", "\"version\":2"), context()),
            Err(BrowserCommandError::WrongVersion)
        );
        assert_eq!(
            protocol.authorize(
                &envelope(
                    "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                    1,
                    "restart"
                ),
                context()
            ),
            Err(BrowserCommandError::WrongCapability)
        );
        assert_eq!(
            protocol.authorize(&envelope(CAPABILITY, 0, "restart"), context()),
            Err(BrowserCommandError::StaleSequence)
        );

        assert_eq!(
            protocol
                .authorize(&valid, context())
                .expect("failed envelopes do not consume sequence one"),
            ViewerCommand::Restart
        );
        assert_eq!(
            protocol.authorize(&valid, context()),
            Err(BrowserCommandError::StaleSequence)
        );
    }

    #[test]
    fn unknown_duplicate_ill_typed_and_oversize_envelopes_are_rejected() {
        let mut protocol = BrowserCommandProtocol::new(CAPABILITY).expect("valid capability");
        for malformed in [
            envelope(CAPABILITY, 1, "delete-project"),
            format!(
                r#"{{"type":"spinal.viewer.command","version":1,"capability":"{CAPABILITY}","sequence":1,"action":"restart","extra":true}}"#
            ),
            format!(
                r#"{{"type":"spinal.viewer.command","version":1,"version":1,"capability":"{CAPABILITY}","sequence":1,"action":"restart"}}"#
            ),
            format!(
                r#"{{"type":"spinal.viewer.command","version":"1","capability":"{CAPABILITY}","sequence":1,"action":"restart"}}"#
            ),
            format!(
                r#"{{"type":"spinal.coordinator.command","version":1,"capability":"{CAPABILITY}","sequence":1,"action":"restart"}}"#
            ),
        ] {
            assert_eq!(
                protocol.authorize(&malformed, context()),
                Err(BrowserCommandError::MalformedEnvelope)
            );
        }
        assert_eq!(
            protocol.authorize("", context()),
            Err(BrowserCommandError::InvalidSize)
        );
        assert_eq!(
            protocol.authorize(&"x".repeat(MAX_BROWSER_COMMAND_BYTES + 1), context()),
            Err(BrowserCommandError::InvalidSize)
        );
    }

    #[test]
    fn launch_capability_has_one_strict_non_secret_bearing_format() {
        for rejected in [
            "",
            "abc",
            "G123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdeg",
        ] {
            assert_eq!(
                BrowserCommandProtocol::new(rejected).expect_err("invalid capability"),
                BrowserCommandError::InvalidLaunchCapability
            );
        }
    }

    #[test]
    fn bounded_queue_is_fifo_and_drops_the_newest_command_on_overflow() {
        let mut queue = BrowserCommandQueue::default();
        for _index in 0..BROWSER_COMMAND_QUEUE_CAPACITY {
            queue
                .try_push(ViewerCommand::Restart)
                .expect("within fixed capacity");
        }
        assert_eq!(
            queue.try_push(ViewerCommand::Refit),
            Err(BrowserCommandQueueError::Full)
        );

        let batch = queue.drain();
        assert_eq!(batch.commands.len(), BROWSER_COMMAND_QUEUE_CAPACITY);
        assert!(
            batch
                .commands
                .iter()
                .all(|command| command == &ViewerCommand::Restart)
        );
        assert!(batch.overflowed);
        assert_eq!(
            queue.drain(),
            BrowserCommandBatch {
                commands: Vec::new(),
                overflowed: false,
            }
        );
    }
}

use std::{ffi::OsString, fmt, path::PathBuf};

pub(crate) const HELP: &str = "\
Validate Spine exports with the clean-room Spinal runtime.

USAGE:
    spinal check --profile loafstead-demo [OPTIONS] [PATH]

PATH may be a skeleton JSON file or a directory containing exactly one JSON
export. It defaults to the current directory. Atlas discovery first tries the
matching .atlas filename, then accepts a sole sibling .atlas file.

OPTIONS:
    --profile loafstead-demo   Apply Loafstead's Spine 4.3.23 demo contract
    --atlas PATH               Use this text atlas instead of discovering one
    --format human|json        Select human output (default) or schema-v1 JSON
    -h, --help                 Show this help

EXIT CODES:
    0  Profile passed (warnings may be present)
    1  Export failed the selected profile
    2  Command or source error
    3  Internal output error
";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutputFormat {
    Human,
    Json,
}

#[derive(Debug)]
pub(crate) struct CheckOptions {
    pub(crate) input: PathBuf,
    pub(crate) atlas: Option<PathBuf>,
    pub(crate) format: OutputFormat,
}

#[derive(Debug)]
pub(crate) enum ParseOutcome {
    Help,
    Check(CheckOptions),
}

pub(crate) fn parse(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<ParseOutcome, ParseError> {
    let mut arguments = arguments.into_iter();
    let Some(command) = arguments.next() else {
        return Err(ParseError::MissingCommand);
    };
    if matches!(command.to_str(), Some("-h" | "--help" | "help")) {
        return Ok(ParseOutcome::Help);
    }
    if command != "check" {
        return Err(ParseError::UnknownCommand(
            command.to_string_lossy().into_owned(),
        ));
    }

    let mut input = None;
    let mut atlas = None;
    let mut format = OutputFormat::Human;
    let mut profile = None;
    let mut positional_only = false;
    let mut arguments = arguments.peekable();
    while let Some(argument) = arguments.next() {
        if !positional_only && matches!(argument.to_str(), Some("-h" | "--help")) {
            return Ok(ParseOutcome::Help);
        }
        if !positional_only && argument == "--" {
            positional_only = true;
            continue;
        }
        if !positional_only && argument == "--profile" {
            profile = Some(utf8_value(
                next_value(&mut arguments, "--profile")?,
                "--profile",
            )?);
            continue;
        }
        if !positional_only && argument == "--atlas" {
            atlas = Some(PathBuf::from(next_value(&mut arguments, "--atlas")?));
            continue;
        }
        if !positional_only && argument == "--format" {
            let value = utf8_value(next_value(&mut arguments, "--format")?, "--format")?;
            format = match value.as_str() {
                "human" => OutputFormat::Human,
                "json" => OutputFormat::Json,
                _other => return Err(ParseError::UnknownFormat(value)),
            };
            continue;
        }
        if !positional_only
            && argument
                .to_str()
                .is_some_and(|argument| argument.starts_with('-'))
        {
            return Err(ParseError::UnknownOption(
                argument.to_string_lossy().into_owned(),
            ));
        }
        if input.replace(PathBuf::from(argument)).is_some() {
            return Err(ParseError::MultipleInputs);
        }
    }

    match profile.as_deref() {
        Some("loafstead-demo") => {}
        Some(other) => return Err(ParseError::UnknownProfile(other.to_owned())),
        None => return Err(ParseError::MissingProfile),
    }
    Ok(ParseOutcome::Check(CheckOptions {
        input: input.unwrap_or_else(|| PathBuf::from(".")),
        atlas,
        format,
    }))
}

fn next_value(
    arguments: &mut impl Iterator<Item = OsString>,
    option: &'static str,
) -> Result<OsString, ParseError> {
    arguments.next().ok_or(ParseError::MissingValue(option))
}

fn utf8_value(value: OsString, option: &'static str) -> Result<String, ParseError> {
    value
        .into_string()
        .map_err(|_value| ParseError::NonUnicodeValue(option))
}

pub(crate) fn requested_format(arguments: &[OsString]) -> OutputFormat {
    let mut format = OutputFormat::Human;
    let mut index = 0;
    while index < arguments.len() {
        if arguments[index] == "--" {
            break;
        }
        if arguments[index] == "--format" {
            if let Some(value) = arguments.get(index + 1).and_then(|value| value.to_str()) {
                match value {
                    "json" => format = OutputFormat::Json,
                    "human" => format = OutputFormat::Human,
                    _other => {}
                }
            }
            index += 1;
        }
        index += 1;
    }
    format
}

#[derive(Debug)]
pub(crate) enum ParseError {
    MissingCommand,
    UnknownCommand(String),
    MissingProfile,
    UnknownProfile(String),
    MissingValue(&'static str),
    UnknownOption(String),
    UnknownFormat(String),
    NonUnicodeValue(&'static str),
    MultipleInputs,
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCommand => write!(formatter, "missing command `check`"),
            Self::UnknownCommand(command) => write!(formatter, "unknown command `{command}`"),
            Self::MissingProfile => write!(formatter, "missing `--profile loafstead-demo`"),
            Self::UnknownProfile(profile) => write!(formatter, "unknown profile `{profile}`"),
            Self::MissingValue(option) => write!(formatter, "missing value for `{option}`"),
            Self::UnknownOption(option) => write!(formatter, "unknown option `{option}`"),
            Self::UnknownFormat(format) => {
                write!(
                    formatter,
                    "unknown output format `{format}`; use human or json"
                )
            }
            Self::NonUnicodeValue(option) => {
                write!(formatter, "value for `{option}` is not valid Unicode")
            }
            Self::MultipleInputs => write!(formatter, "more than one input path was provided"),
        }
    }
}

impl std::error::Error for ParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_current_directory_but_not_to_a_profile() {
        let parsed = parse(["check", "--profile", "loafstead-demo"].map(OsString::from))
            .expect("valid command");
        let ParseOutcome::Check(options) = parsed else {
            panic!("expected check command");
        };
        assert_eq!(options.input, PathBuf::from("."));
        assert_eq!(options.format, OutputFormat::Human);
    }

    #[test]
    fn rejects_unknown_profiles_instead_of_silently_weakening_the_gate() {
        let error = parse(["check", "--profile", "almost-loafstead"].map(OsString::from))
            .expect_err("unknown profile must fail");
        assert!(error.to_string().contains("almost-loafstead"));
    }

    #[test]
    fn requested_json_format_survives_a_later_parse_error() {
        let arguments = ["check", "--format", "json", "--bogus"]
            .map(OsString::from)
            .to_vec();
        assert_eq!(requested_format(&arguments), OutputFormat::Json);
        assert!(parse(arguments).is_err());
    }
}

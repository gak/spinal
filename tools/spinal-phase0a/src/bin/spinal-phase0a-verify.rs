//! Read-only verifier CLI for representative Phase 0A evidence.

use serde::Serialize;
use spinal_phase0a::{RepresentativeVerificationError, verify_representative_evidence};
use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "Verify one successfully published representative Phase 0A evidence directory.\n\nUsage:\n  spinal-phase0a-verify <canonical-absolute-evidence-directory>\n\nUse the exact evidence path printed by the representative runner. Filesystem\naliases such as macOS /tmp for /private/tmp are rejected. Nonpassing generic\ncore diagnostics are intentionally unpublished and are not verifier inputs.\n";

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct InvalidOutput<'a> {
    valid: bool,
    error: &'a str,
}

fn main() -> ExitCode {
    match parse_arguments(env::args_os()) {
        Ok(ParsedArguments::Help) => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(ParsedArguments::Verify(path)) => match verify_representative_evidence(path) {
            Ok(result) => {
                println!(
                    "{}",
                    serde_json::to_string(&result).expect("verification result is serializable")
                );
                ExitCode::SUCCESS
            }
            Err(error) => invalid(error),
        },
        Err(message) => {
            let output = InvalidOutput {
                valid: false,
                error: message,
            };
            eprintln!(
                "{}",
                serde_json::to_string(&output).expect("static usage error is serializable")
            );
            ExitCode::from(2)
        }
    }
}

fn invalid(error: RepresentativeVerificationError) -> ExitCode {
    let message = error.to_string();
    let output = InvalidOutput {
        valid: false,
        error: &message,
    };
    eprintln!(
        "{}",
        serde_json::to_string(&output).expect("verification error is serializable")
    );
    ExitCode::from(2)
}

enum ParsedArguments {
    Help,
    Verify(PathBuf),
}

fn parse_arguments(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<ParsedArguments, &'static str> {
    let mut arguments = arguments.into_iter();
    let _program = arguments.next();
    let positional = arguments.collect::<Vec<_>>();
    if positional.len() == 1 && matches!(positional[0].to_str(), Some("-h" | "--help")) {
        return Ok(ParsedArguments::Help);
    }
    let [path] = positional.as_slice() else {
        return Err("expected exactly one absolute evidence directory");
    };
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err("evidence directory must be absolute");
    }
    Ok(ParsedArguments::Verify(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(values: &[&str]) -> Result<ParsedArguments, &'static str> {
        parse_arguments(values.iter().map(OsString::from))
    }

    #[test]
    fn accepts_exactly_one_absolute_directory() {
        assert!(matches!(
            parse(&["program", "/private/evidence"]).expect("valid"),
            ParsedArguments::Verify(_)
        ));
        assert!(parse(&["program", "relative"]).is_err());
        assert!(parse(&["program"]).is_err());
        assert!(parse(&["program", "/one", "/two"]).is_err());
    }

    #[test]
    fn help_must_be_the_only_argument() {
        assert!(matches!(
            parse(&["program", "--help"]).expect("help"),
            ParsedArguments::Help
        ));
        assert!(parse(&["program", "--help", "/evidence"]).is_err());
    }
}

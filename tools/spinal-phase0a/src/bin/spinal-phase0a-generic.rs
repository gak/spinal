//! Minimal command-line entry point for the generic Phase 0A rehearsal.

use spinal_phase0a::{GenericRehearsalRequest, run_generic_rehearsal};
use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
Run and publish one generic, non-representative Phase 0A rehearsal.\n\
\n\
Usage:\n\
  spinal-phase0a-generic <case.toml> <spine-executable> <workspace-directory> <editor-lock-file> <evidence-directory>\n\
\n\
All five paths must be absolute and normalized. The workspace and evidence\n\
directories must not exist. This command cannot produce representative gate\n\
evidence.\n";

struct Arguments {
    case_path: PathBuf,
    editor_executable: PathBuf,
    workspace_root: PathBuf,
    editor_lock: PathBuf,
    evidence_destination: PathBuf,
}

enum ParsedArguments {
    Help,
    Run(Arguments),
}

fn main() -> ExitCode {
    match parse_arguments(env::args_os()) {
        Ok(ParsedArguments::Help) => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(ParsedArguments::Run(arguments)) => run(arguments),
        Err(message) => {
            eprintln!("error: {message}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
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
    let [
        case_path,
        editor_executable,
        workspace_root,
        editor_lock,
        evidence_destination,
    ] = positional.as_slice()
    else {
        return Err("expected exactly five positional paths");
    };

    Ok(ParsedArguments::Run(Arguments {
        case_path: case_path.into(),
        editor_executable: editor_executable.into(),
        workspace_root: workspace_root.into(),
        editor_lock: editor_lock.into(),
        evidence_destination: evidence_destination.into(),
    }))
}

fn run(arguments: Arguments) -> ExitCode {
    let request = GenericRehearsalRequest::new(
        arguments.case_path,
        arguments.editor_executable,
        arguments.workspace_root,
        arguments.editor_lock,
        arguments.evidence_destination,
    );
    let published = match run_generic_rehearsal(request) {
        Ok(published) => published,
        Err(error) => {
            eprintln!(
                "generic Phase 0A rehearsal could not publish evidence ({:?}): {error}",
                error.code()
            );
            return ExitCode::FAILURE;
        }
    };

    println!("scope: generic rehearsal (non-representative; gate eligible: no)");
    if let Some(workspace) = published.workspace_root() {
        println!("retained workspace: {}", workspace.display());
    }
    println!("evidence: {}", published.destination().display());
    println!("report SHA-256: {}", published.report_sha256());
    if published.passed() {
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "generic Phase 0A gate failed ({:?}); diagnostics were published",
            published.failure_code()
        );
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(values: &[&str]) -> Result<ParsedArguments, &'static str> {
        parse_arguments(values.iter().map(OsString::from))
    }

    #[test]
    fn accepts_only_the_exact_five_path_contract() {
        let parsed = parse(&["program", "/case", "/editor", "/work", "/lock", "/evidence"])
            .expect("valid arguments");
        let ParsedArguments::Run(arguments) = parsed else {
            panic!("expected run arguments");
        };
        assert_eq!(arguments.case_path, PathBuf::from("/case"));
        assert_eq!(arguments.editor_executable, PathBuf::from("/editor"));
        assert_eq!(arguments.workspace_root, PathBuf::from("/work"));
        assert_eq!(arguments.editor_lock, PathBuf::from("/lock"));
        assert_eq!(arguments.evidence_destination, PathBuf::from("/evidence"));
    }

    #[test]
    fn accepts_help_as_the_only_argument() {
        assert!(matches!(
            parse(&["program", "--help"]).expect("help"),
            ParsedArguments::Help
        ));
        assert!(matches!(
            parse(&["program", "-h"]).expect("short help"),
            ParsedArguments::Help
        ));
    }

    #[test]
    fn refuses_missing_extra_and_help_mixed_with_paths() {
        assert!(parse(&["program"]).is_err());
        assert!(parse(&["program", "/case"]).is_err());
        assert!(
            parse(&[
                "program",
                "/case",
                "/editor",
                "/work",
                "/lock",
                "/evidence",
                "/extra",
            ])
            .is_err()
        );
        assert!(parse(&["program", "--help", "/case"]).is_err());
    }
}

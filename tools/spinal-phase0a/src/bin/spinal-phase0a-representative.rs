//! Closed command-line entry point for representative Phase 0A evidence.

use spinal_phase0a::{
    RepresentativeRunRequest, propose_representative_binding, run_representative_phase0a,
};
use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
Run and publish one binding-pinned representative Phase 0A evidence candidate.\n\
\n\
Usage:\n\
  spinal-phase0a-representative --propose-binding <case.toml>\n\
  spinal-phase0a-representative <representative-binding.toml> <case.toml> <spine-executable> <workspace-directory> <editor-lock-file> <evidence-directory>\n\
\n\
Proposal mode creates no files or evidence. It prints strict binding TOML to\n\
stdout using this exact prebuilt runner's bytes and embedded clean build\n\
provenance; review and store it as an owner-private 0600 file.\n\
\n\
All six paths must be absolute and normalized. The binding and case must be\n\
owner-private exact files. The workspace and evidence directories must not\n\
exist, and their parent directories must be owner-private. This command never\n\
records the Phase 0A gate decision; a maintainer must verify and review the\n\
published candidate separately.\n";

struct Arguments {
    binding_path: PathBuf,
    case_path: PathBuf,
    editor_executable: PathBuf,
    workspace_root: PathBuf,
    editor_lock: PathBuf,
    evidence_destination: PathBuf,
}

enum ParsedArguments {
    Help,
    Propose(PathBuf),
    Run(Arguments),
}

fn main() -> ExitCode {
    match parse_arguments(env::args_os()) {
        Ok(ParsedArguments::Help) => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(ParsedArguments::Propose(case_path)) => propose(&case_path),
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
    if positional.len() == 2 && positional[0] == "--propose-binding" {
        return Ok(ParsedArguments::Propose(positional[1].clone().into()));
    }
    let [
        binding_path,
        case_path,
        editor_executable,
        workspace_root,
        editor_lock,
        evidence_destination,
    ] = positional.as_slice()
    else {
        return Err("expected exactly six positional paths");
    };

    Ok(ParsedArguments::Run(Arguments {
        binding_path: binding_path.into(),
        case_path: case_path.into(),
        editor_executable: editor_executable.into(),
        workspace_root: workspace_root.into(),
        editor_lock: editor_lock.into(),
        evidence_destination: evidence_destination.into(),
    }))
}

fn propose(case_path: &std::path::Path) -> ExitCode {
    match propose_representative_binding(case_path) {
        Ok(proposal) => {
            eprintln!(
                "WARNING: PROPOSAL ONLY. Review this binding, store it inside an owner-private directory, apply `chmod 600` to the binding file, and invoke this exact prebuilt runner. No evidence or gate decision was created."
            );
            print!("{proposal}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("binding proposal failed ({:?}): {error}", error.code());
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: Arguments) -> ExitCode {
    println!("representative Phase 0A evidence candidate; this is not a gate PASS");
    println!("any workspace created by the core will be retained for maintainer review");

    let request = RepresentativeRunRequest::new(
        arguments.binding_path,
        arguments.case_path,
        arguments.editor_executable,
        arguments.workspace_root,
        arguments.editor_lock,
        arguments.evidence_destination,
    );
    let published = match run_representative_phase0a(request) {
        Ok(published) => published,
        Err(error) => {
            eprintln!(
                "UNPUBLISHED representative attempt ({:?}): {error}",
                error.code()
            );
            eprintln!(
                "retain any partial workspace or destination for diagnosis; use fresh paths for the next attempt"
            );
            return ExitCode::FAILURE;
        }
    };

    println!("evidence: {}", published.destination().display());
    println!("report SHA-256: {}", published.report_sha256());
    if let Some(workspace) = published.workspace_root() {
        println!("retained workspace: {}", workspace.display());
    }
    println!("inner core result: passed");
    println!("reported candidate eligibility (unverified): yes");
    println!("independent verifier status: not yet run");
    println!("maintainer gate decision: not recorded; no mutation is unlocked");
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(values: &[&str]) -> Result<ParsedArguments, &'static str> {
        parse_arguments(values.iter().map(OsString::from))
    }

    #[test]
    fn accepts_only_the_exact_six_path_contract() {
        let parsed = parse(&[
            "program",
            "/binding",
            "/case",
            "/editor",
            "/work",
            "/lock",
            "/evidence",
        ])
        .expect("valid arguments");
        let ParsedArguments::Run(arguments) = parsed else {
            panic!("expected run arguments");
        };
        assert_eq!(arguments.binding_path, PathBuf::from("/binding"));
        assert_eq!(arguments.case_path, PathBuf::from("/case"));
        assert_eq!(arguments.editor_executable, PathBuf::from("/editor"));
        assert_eq!(arguments.workspace_root, PathBuf::from("/work"));
        assert_eq!(arguments.editor_lock, PathBuf::from("/lock"));
        assert_eq!(arguments.evidence_destination, PathBuf::from("/evidence"));
    }

    #[test]
    fn accepts_help_only_by_itself() {
        assert!(matches!(
            parse(&["program", "--help"]).expect("help"),
            ParsedArguments::Help
        ));
        assert!(matches!(
            parse(&["program", "-h"]).expect("short help"),
            ParsedArguments::Help
        ));
        assert!(parse(&["program", "--help", "/binding"]).is_err());
    }

    #[test]
    fn proposal_mode_is_separate_from_the_six_path_run_contract() {
        let parsed = parse(&["program", "--propose-binding", "/case"]).expect("proposal mode");
        let ParsedArguments::Propose(case) = parsed else {
            panic!("expected proposal arguments");
        };
        assert_eq!(case, PathBuf::from("/case"));
        assert!(parse(&["program", "--propose-binding"]).is_err());
        assert!(parse(&["program", "--propose-binding", "/case", "/extra"]).is_err());
    }

    #[test]
    fn refuses_missing_and_extra_paths() {
        assert!(parse(&["program"]).is_err());
        assert!(parse(&["program", "/binding"]).is_err());
        assert!(
            parse(&[
                "program",
                "/binding",
                "/case",
                "/editor",
                "/work",
                "/lock",
                "/evidence",
                "/extra",
            ])
            .is_err()
        );
    }
}

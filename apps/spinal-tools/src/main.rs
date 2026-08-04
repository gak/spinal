//! Command-line tools for validating Spine exports with Spinal.

mod check;
mod cli;
mod report;
mod source;

use std::{env, io, io::Write, process::ExitCode};

use cli::{OutputFormat, ParseOutcome};

fn main() -> ExitCode {
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    let requested_format = cli::requested_format(&arguments);
    match cli::parse(arguments) {
        Ok(ParseOutcome::Help) => write_stdout(cli::HELP.as_bytes(), 0),
        Ok(ParseOutcome::Check(options)) => run_check(options),
        Err(error) => write_tool_error(
            requested_format,
            "command-error",
            "command-invalid",
            error.to_string(),
            true,
        ),
    }
}

fn run_check(options: cli::CheckOptions) -> ExitCode {
    let format = options.format;
    let source = match source::SourceFiles::open(&options) {
        Ok(source) => source,
        Err(error) => {
            return write_tool_error(
                format,
                "source-error",
                error.code(),
                error.to_string(),
                false,
            );
        }
    };
    let report = check::loafstead_demo(&source);
    let code = report.exit_code();
    let bytes = match format {
        OutputFormat::Human => report.render_human().into_bytes(),
        OutputFormat::Json => match serde_json::to_vec_pretty(&report) {
            Ok(mut bytes) => {
                bytes.push(b'\n');
                bytes
            }
            Err(error) => {
                eprintln!("spinal: could not serialize the check report: {error}");
                return ExitCode::from(3);
            }
        },
    };
    write_stdout(&bytes, code)
}

fn write_tool_error(
    format: OutputFormat,
    status: &str,
    code: &str,
    message: String,
    show_help: bool,
) -> ExitCode {
    match format {
        OutputFormat::Human => {
            eprintln!("spinal: {}", report::visible(&message));
            if show_help {
                eprintln!("\n{}", cli::HELP);
            }
        }
        OutputFormat::Json => {
            let report = report::Report::tool_error(status, code, message);
            match serde_json::to_vec_pretty(&report) {
                Ok(mut bytes) => {
                    bytes.push(b'\n');
                    return write_stdout(&bytes, 2);
                }
                Err(serialization_error) => {
                    eprintln!("spinal: could not serialize {status}: {serialization_error}");
                    return ExitCode::from(3);
                }
            }
        }
    }
    ExitCode::from(2)
}

fn write_stdout(bytes: &[u8], success_code: u8) -> ExitCode {
    let mut stdout = io::stdout().lock();
    if let Err(error) = stdout.write_all(bytes).and_then(|()| stdout.flush()) {
        eprintln!("spinal: could not write output: {error}");
        return ExitCode::from(3);
    }
    ExitCode::from(success_code)
}

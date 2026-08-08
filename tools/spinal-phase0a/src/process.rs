use crate::digest::sha256_bytes;
use serde::Serialize;
use std::collections::BTreeSet;
use thiserror::Error;

const BLOCKING_TERMS: &[&str] = &[
    "warning",
    "error",
    "failed",
    "failure",
    "not found",
    "unknown",
    "ignored",
    "unsupported",
    "exception",
    "unable",
    "missing",
    "invalid",
    "denied",
];

// Populate only from captured, reviewed Spine 4.3.23 evidence. Case files may
// not add to or weaken this list.
const SPINE_4_3_23_INFORMATIONAL_LINES: &[&str] = &[];

/// One editor process request at the injectable process boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessRequest {
    /// Logical operation name used in evidence.
    pub operation: String,
    /// Executable selected by the caller.
    pub program: String,
    /// Exact argument vector, without shell interpretation.
    pub args: Vec<String>,
    /// Output identifiers that must be present after a successful process.
    pub required_outputs: BTreeSet<String>,
}

/// Captured result returned by a process executor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessCapture {
    /// Exit code, or `None` when no normal exit status was available.
    pub exit_code: Option<i32>,
    /// Whether the executor terminated the process after its deadline.
    pub timed_out: bool,
    /// Unmodified standard output bytes.
    pub stdout: Vec<u8>,
    /// Unmodified standard error bytes.
    pub stderr: Vec<u8>,
    /// Output identifiers observed after execution.
    pub observed_outputs: BTreeSet<String>,
}

/// Injectable boundary used to test process policy without invoking Spine.
pub trait ProcessExecutor {
    /// Executes one request and returns its unmodified capture.
    fn execute(&self, request: &ProcessRequest) -> Result<ProcessCapture, ProcessExecutionError>;
}

/// Failure to start or capture a process at the executor boundary.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("process execution failed: {message}")]
pub struct ProcessExecutionError {
    /// Stable human-readable cause supplied by the adapter.
    pub message: String,
}

/// Checked-in transcript rules for a specific editor version.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TranscriptPolicy;

impl TranscriptPolicy {
    /// Returns the deny-first policy pinned to Spine 4.3.23.
    pub const fn spine_4_3_23() -> Self {
        Self
    }
}

/// Stable failure codes emitted by process assessment.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessFailureCode {
    /// The process exceeded its deadline.
    TimedOut,
    /// The executor did not produce a normal exit status.
    MissingExitStatus,
    /// The process returned a nonzero exit code.
    NonzeroExit,
    /// Standard output or error was not UTF-8.
    NonUtf8Transcript,
    /// A deny-listed diagnostic appeared in the transcript.
    BlockingDiagnostic,
    /// A nonblank line was not an exact checked-in informational line.
    UnknownTranscriptLine,
    /// A required output was absent after execution.
    MissingOutput,
}

/// One actionable process-policy failure.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessFailure {
    /// Stable machine-readable failure code.
    pub code: ProcessFailureCode,
    /// Context suitable for the evidence report.
    pub detail: String,
}

/// Fail-closed assessment of one captured editor process.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessAssessment {
    passed: bool,
    stdout_sha256: String,
    stderr_sha256: String,
    failures: Vec<ProcessFailure>,
}

/// Serialized evidence atomically binding a request, capture, and assessment.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessEvidence {
    operation: String,
    program: String,
    args: Vec<String>,
    exit_code: Option<i32>,
    timed_out: bool,
    required_outputs: BTreeSet<String>,
    observed_outputs: BTreeSet<String>,
    assessment: ProcessAssessment,
}

/// Executes one request through an injected adapter and applies strict policy.
pub fn execute_and_assess(
    executor: &impl ProcessExecutor,
    request: &ProcessRequest,
    policy: TranscriptPolicy,
) -> Result<ProcessEvidence, ProcessExecutionError> {
    let capture = executor.execute(request)?;
    let assessment = assess_capture(request, &capture, policy);
    Ok(ProcessEvidence {
        operation: request.operation.clone(),
        program: request.program.clone(),
        args: request.args.clone(),
        exit_code: capture.exit_code,
        timed_out: capture.timed_out,
        required_outputs: request.required_outputs.clone(),
        observed_outputs: capture.observed_outputs,
        assessment,
    })
}

impl ProcessAssessment {
    /// Returns true only when process, transcript, and output checks passed.
    pub fn passed(&self) -> bool {
        self.passed
    }

    /// Returns the SHA-256 of the unmodified standard output bytes.
    pub fn stdout_sha256(&self) -> &str {
        &self.stdout_sha256
    }

    /// Returns the SHA-256 of the unmodified standard error bytes.
    pub fn stderr_sha256(&self) -> &str {
        &self.stderr_sha256
    }

    /// Returns every detected policy failure.
    pub fn failures(&self) -> &[ProcessFailure] {
        &self.failures
    }
}

impl ProcessEvidence {
    /// Returns the exact logical operation name.
    pub fn operation(&self) -> &str {
        &self.operation
    }

    /// Returns the exact executable string supplied in the request.
    pub fn program(&self) -> &str {
        &self.program
    }

    /// Returns the exact argument vector supplied in the request.
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// Returns the captured exit code, when a normal status existed.
    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    /// Returns whether the executor reported a timeout.
    pub fn timed_out(&self) -> bool {
        self.timed_out
    }

    /// Returns the exact required output identifiers.
    pub fn required_outputs(&self) -> &BTreeSet<String> {
        &self.required_outputs
    }

    /// Returns the exact observed output identifiers.
    pub fn observed_outputs(&self) -> &BTreeSet<String> {
        &self.observed_outputs
    }

    /// Returns the assessment derived from this bound request and capture.
    pub fn assessment(&self) -> &ProcessAssessment {
        &self.assessment
    }
}

fn assess_capture(
    request: &ProcessRequest,
    capture: &ProcessCapture,
    policy: TranscriptPolicy,
) -> ProcessAssessment {
    let mut failures = Vec::new();
    if capture.timed_out {
        failures.push(failure(
            ProcessFailureCode::TimedOut,
            "process exceeded its deadline",
        ));
    } else {
        match capture.exit_code {
            Some(0) => {}
            Some(code) => failures.push(failure(
                ProcessFailureCode::NonzeroExit,
                format!("process exited with code {code}"),
            )),
            None => failures.push(failure(
                ProcessFailureCode::MissingExitStatus,
                "process produced no normal exit status",
            )),
        }
    }

    classify_transcript("stdout", &capture.stdout, policy, &mut failures);
    classify_transcript("stderr", &capture.stderr, policy, &mut failures);
    for output in request
        .required_outputs
        .difference(&capture.observed_outputs)
    {
        failures.push(failure(
            ProcessFailureCode::MissingOutput,
            format!("required output `{output}` was not produced"),
        ));
    }

    ProcessAssessment {
        passed: failures.is_empty(),
        stdout_sha256: sha256_bytes(&capture.stdout),
        stderr_sha256: sha256_bytes(&capture.stderr),
        failures,
    }
}

fn classify_transcript(
    stream: &str,
    bytes: &[u8],
    _policy: TranscriptPolicy,
    failures: &mut Vec<ProcessFailure>,
) {
    let Ok(text) = std::str::from_utf8(bytes) else {
        failures.push(failure(
            ProcessFailureCode::NonUtf8Transcript,
            format!("{stream} was not UTF-8"),
        ));
        return;
    };
    for (index, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let lowercase = line.to_ascii_lowercase();
        if BLOCKING_TERMS.iter().any(|term| lowercase.contains(term)) {
            failures.push(failure(
                ProcessFailureCode::BlockingDiagnostic,
                format!("{stream} line {}: {line}", index + 1),
            ));
        } else if !SPINE_4_3_23_INFORMATIONAL_LINES.contains(&line) {
            failures.push(failure(
                ProcessFailureCode::UnknownTranscriptLine,
                format!("{stream} line {}: {line}", index + 1),
            ));
        }
    }
}

fn failure(code: ProcessFailureCode, detail: impl Into<String>) -> ProcessFailure {
    ProcessFailure {
        code,
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct FakeExecutor(ProcessCapture);

    impl ProcessExecutor for FakeExecutor {
        fn execute(
            &self,
            _request: &ProcessRequest,
        ) -> Result<ProcessCapture, ProcessExecutionError> {
            Ok(self.0.clone())
        }
    }

    fn request() -> ProcessRequest {
        ProcessRequest {
            operation: "generic-export".to_owned(),
            program: "editor".to_owned(),
            args: vec!["--fixed-version".to_owned()],
            required_outputs: BTreeSet::from(["skeleton.json".to_owned()]),
        }
    }

    fn capture() -> ProcessCapture {
        ProcessCapture {
            exit_code: Some(0),
            timed_out: false,
            stdout: Vec::new(),
            stderr: Vec::new(),
            observed_outputs: BTreeSet::from(["skeleton.json".to_owned()]),
        }
    }

    fn assess(capture: ProcessCapture) -> ProcessEvidence {
        execute_and_assess(
            &FakeExecutor(capture),
            &request(),
            TranscriptPolicy::spine_4_3_23(),
        )
        .expect("fake execution")
    }

    #[test]
    fn accepts_only_a_complete_quiet_success() {
        assert!(assess(capture()).assessment().passed());
    }

    #[test]
    fn rejects_nonzero_exit() {
        let mut value = capture();
        value.exit_code = Some(7);
        assert_has_failure(assess(value), ProcessFailureCode::NonzeroExit);
    }

    #[test]
    fn rejects_diagnostic_even_with_zero_exit() {
        let mut value = capture();
        value.stdout = b"Images path not found: ./images\n".to_vec();
        assert_has_failure(assess(value), ProcessFailureCode::BlockingDiagnostic);
    }

    #[test]
    fn rejects_timeout() {
        let mut value = capture();
        value.timed_out = true;
        value.exit_code = None;
        assert_has_failure(assess(value), ProcessFailureCode::TimedOut);
    }

    #[test]
    fn rejects_missing_output() {
        let mut value = capture();
        value.observed_outputs.clear();
        assert_has_failure(assess(value), ProcessFailureCode::MissingOutput);
    }

    #[test]
    fn rejects_unknown_nonblank_output() {
        let mut value = capture();
        value.stderr = b"unreviewed status text\n".to_vec();
        assert_has_failure(assess(value), ProcessFailureCode::UnknownTranscriptLine);
    }

    fn assert_has_failure(evidence: ProcessEvidence, code: ProcessFailureCode) {
        let assessment = evidence.assessment();
        assert!(!assessment.passed());
        assert!(
            assessment
                .failures()
                .iter()
                .any(|failure| failure.code == code)
        );
    }

    #[test]
    fn serialized_evidence_binds_request_capture_and_assessment() {
        let evidence = assess(capture());
        let value = serde_json::to_value(evidence).expect("serialize process evidence");
        assert_eq!(value["operation"], "generic-export");
        assert_eq!(value["program"], "editor");
        assert_eq!(value["args"][0], "--fixed-version");
        assert_eq!(value["exit_code"], 0);
        assert_eq!(value["timed_out"], false);
        assert_eq!(value["required_outputs"][0], "skeleton.json");
        assert_eq!(value["observed_outputs"][0], "skeleton.json");
        assert_eq!(value["assessment"]["passed"], true);
        assert_eq!(value["assessment"]["stdout_sha256"], sha256_bytes(b""));
    }
}

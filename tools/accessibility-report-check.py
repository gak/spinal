#!/usr/bin/env python3
"""Validate completed accessibility evidence and print its immutable digest."""

from __future__ import annotations

import argparse
import copy
import datetime as dt
import hashlib
import json
import pathlib
import re
import sys
import tempfile
from typing import Any


REPORT_KIND = "spinal.viewer_accessibility_evidence"
EXPECTED_FIXTURE_CLASS = "repository_self_authored_generic"
EXPECTED_PRODUCT_SURFACE = ["preview", "compare", "diagnostics", "camera"]
EXPECTED_EXCLUDED = [
    "coordinator",
    "conflict_resolution",
    "approval",
    "promotion",
    "representative_spine_correctness",
    "user_authored_motion_approval",
]
EXPECTED_AUTOMATION_LIMITATIONS = [
    "Automation cannot make an accessibility acceptance decision.",
    (
        "The 500 by 900 narrow browser pre-flight is not actual 200 percent or "
        "400 percent browser zoom or reflow evidence."
    ),
    (
        "DOM, computed-style, and AccessKit contracts do not prove VoiceOver "
        "speech, trusted keyboard focus order, focus visibility over arbitrary "
        "artwork, or human usability."
    ),
]
EXPECTED_LIMITATIONS = [
    "This is not WCAG certification or a general conformance claim.",
    "Only the recorded macOS, Chrome or Chromium, AccessKit, and VoiceOver profile is in scope.",
    "Native magnification and minimum-window review is not native 200 percent or 400 percent reflow evidence.",
    "Visual pose and animation differences do not have a complete textual equivalent.",
    "Authored animation accessibility, flashing, photosensitivity, and motion approval require separate qualified review or an agreed accommodation.",
    "Phase 0A, Phase 0B, mutation safety, production readiness, and release readiness are not decided here.",
]
EXPECTED_DECISION_STATEMENT = (
    "Automation is PRE-FLIGHT only. A named human must complete every required "
    "browser and native keyboard and VoiceOver row and authorize this report's "
    "immutable digest for recording in the plan."
)
EXPECTED_AUTOMATION_ARTIFACTS = {
    "browser_semantics": "preflight/browser-semantics.log",
    "workspace_tests": "preflight/workspace-tests.log",
    "browser_smoke_with_500px_preflight": (
        "preflight/browser-smoke-with-500px-preflight.log"
    ),
}
EXPECTED_PREFLIGHT_ARTIFACTS = {
    *EXPECTED_AUTOMATION_ARTIFACTS.values(),
    "preflight/provenance.txt",
    "preflight/state.txt",
}
EXPECTED_STATE_KEYS = {
    "format_version",
    "classification",
    "state",
    "automation_result",
    "browser_semantics_result",
    "workspace_tests_result",
    "browser_smoke_with_500px_preflight_result",
    "repository_integrity_result",
    "overall_result",
}
EXPECTED_PROVENANCE_KEYS = {
    "format_version",
    "classification",
    "bevy_checkpoint",
    "repository_commit",
    "clean_worktree",
    "generated_at_utc",
    "operating_system",
    "architecture",
    "rustc",
    "cargo",
    "node",
    "python",
    "trunk",
    "imagemagick",
    "imagemagick_command",
    "browser",
    "browser_render_smoke_window",
    "browser_accessibility_preflight_window",
    "browser_accessibility_preflight_claim",
    "browser_zoom_200_percent",
    "browser_zoom_400_percent",
    "native_voiceover",
    "browser_voiceover",
}
REQUIRED_BROWSER_ROWS = {
    "keyboard",
    "voiceover",
    "zoom_200_percent",
    "zoom_400_percent",
    "reduced_motion_contrast_and_non_color",
}
REQUIRED_NATIVE_ROWS = {
    "keyboard",
    "voiceover",
    "minimum_window_display_scale_and_magnification",
    "reduced_motion_contrast_and_non_color",
}
REQUIRED_ENVIRONMENT = {
    "operating_system",
    "architecture",
    "display_scale",
    "rustc",
    "cargo",
    "node",
    "python",
    "trunk",
    "imagemagick",
    "browser",
    "voiceover",
    "gpu_backend",
}
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")


class ValidationError(Exception):
    """A completed evidence package is inconsistent or incomplete."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValidationError(message)


def require_object(value: Any, label: str) -> dict[str, Any]:
    require(isinstance(value, dict), f"{label} must be an object")
    return value


def require_exact_keys(value: dict[str, Any], expected: set[str], label: str) -> None:
    actual = set(value)
    missing = sorted(expected - actual)
    extra = sorted(actual - expected)
    require(not missing and not extra, f"{label} keys changed; missing={missing}, extra={extra}")


def require_text(value: Any, label: str) -> str:
    require(isinstance(value, str) and bool(value.strip()), f"{label} must be recorded")
    return value


def require_utc(value: Any, label: str) -> None:
    text = require_text(value, label)
    require(text.endswith("Z"), f"{label} must be a UTC timestamp ending in Z")
    try:
        parsed = dt.datetime.fromisoformat(text[:-1] + "+00:00")
    except ValueError as error:
        raise ValidationError(f"{label} is not a valid timestamp") from error
    require(parsed.utcoffset() == dt.timedelta(0), f"{label} must be UTC")


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def reject_duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ValidationError(f"report contains duplicate JSON key: {key}")
        result[key] = value
    return result


def load_report(path: pathlib.Path) -> tuple[dict[str, Any], str]:
    require(path.is_file() and not path.is_symlink(), "report.json must be a regular file")
    try:
        encoded = path.read_bytes()
        report = json.loads(encoded.decode("utf-8"), object_pairs_hook=reject_duplicate_keys)
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise ValidationError(f"report.json could not be read: {error}") from error
    return require_object(report, "report"), sha256_bytes(encoded)


def parse_key_values(lines: list[str], expected: set[str], label: str) -> dict[str, str]:
    values: dict[str, str] = {}
    for line_number, line in enumerate(lines, 1):
        match = re.fullmatch(r"([a-z0-9_]+)=(.*)", line)
        require(match is not None, f"{label} has an invalid line {line_number}")
        key, value = match.groups()
        require(key not in values, f"{label} contains duplicate key: {key}")
        values[key] = value
    require_exact_keys(values, expected, label)
    return values


def decode_artifact(encoded: bytes, label: str) -> list[str]:
    try:
        return encoded.decode("utf-8").splitlines()
    except UnicodeError as error:
        raise ValidationError(f"{label} is not valid UTF-8: {error}") from error


def load_state_and_provenance(
    artifacts: dict[str, bytes],
) -> tuple[dict[str, str], dict[str, str]]:
    state_lines = decode_artifact(
        artifacts["preflight/state.txt"],
        "preflight/state.txt",
    )
    provenance_lines = decode_artifact(
        artifacts["preflight/provenance.txt"],
        "preflight/provenance.txt",
    )
    state = parse_key_values(state_lines, EXPECTED_STATE_KEYS, "preflight/state.txt")
    require("" in provenance_lines, "preflight/provenance.txt has no dependency-tree boundary")
    boundary = provenance_lines.index("")
    provenance = parse_key_values(
        provenance_lines[:boundary],
        EXPECTED_PROVENANCE_KEYS,
        "preflight/provenance.txt",
    )
    dependency_tree = provenance_lines[boundary + 1 :]
    require(
        dependency_tree
        and dependency_tree[0] == "locked spinal-app direct dependency tree:",
        "preflight/provenance.txt has no locked dependency-tree heading",
    )
    require(
        any(line.endswith("bevy v0.18.1") for line in dependency_tree[1:]),
        "preflight/provenance.txt does not bind Bevy 0.18.1",
    )
    return state, provenance


def safe_relative_path(value: str) -> pathlib.PurePosixPath:
    require("\\" not in value, f"checksum path is not POSIX-relative: {value}")
    path = pathlib.PurePosixPath(value)
    require(not path.is_absolute(), f"checksum path is absolute: {value}")
    require(path.parts and path.parts[0] == "preflight", f"checksum path is outside preflight: {value}")
    require(".." not in path.parts and "." not in path.parts, f"checksum path is unsafe: {value}")
    return path


def load_and_verify_manifest(
    evidence_dir: pathlib.Path,
) -> tuple[dict[str, str], str, dict[str, bytes]]:
    manifest_path = evidence_dir / "checksums.sha256"
    require(
        manifest_path.is_file() and not manifest_path.is_symlink(),
        "checksums.sha256 must be a regular file",
    )
    entries: dict[str, str] = {}
    try:
        encoded = manifest_path.read_bytes()
        lines = encoded.decode("utf-8").splitlines()
    except (OSError, UnicodeError) as error:
        raise ValidationError(f"checksums.sha256 could not be read: {error}") from error
    require(bool(lines), "checksums.sha256 is empty")
    artifacts: dict[str, bytes] = {}
    for line_number, line in enumerate(lines, 1):
        match = re.fullmatch(r"([0-9a-f]{64})  (.+)", line)
        require(match is not None, f"invalid checksum line {line_number}")
        digest, relative_text = match.groups()
        relative = safe_relative_path(relative_text).as_posix()
        require(relative not in entries, f"duplicate checksum path: {relative}")
        artifact = evidence_dir.joinpath(*pathlib.PurePosixPath(relative).parts)
        require(artifact.is_file() and not artifact.is_symlink(), f"artifact is not a regular file: {relative}")
        try:
            artifact_bytes = artifact.read_bytes()
        except OSError as error:
            raise ValidationError(f"artifact could not be read: {relative}: {error}") from error
        require(
            sha256_bytes(artifact_bytes) == digest,
            f"artifact checksum mismatch: {relative}",
        )
        entries[relative] = digest
        artifacts[relative] = artifact_bytes

    preflight = evidence_dir / "preflight"
    require(preflight.is_dir() and not preflight.is_symlink(), "preflight must be a regular directory")
    actual: set[str] = set()
    for artifact in preflight.rglob("*"):
        require(not artifact.is_symlink(), f"preflight contains a symlink: {artifact.name}")
        if artifact.is_file():
            actual.add(artifact.relative_to(evidence_dir).as_posix())
    require(set(entries) == actual, "checksums.sha256 does not exactly cover preflight artifacts")
    require(
        actual == EXPECTED_PREFLIGHT_ARTIFACTS,
        "preflight evidence does not contain the exact version-one artifact set",
    )
    return entries, sha256_bytes(encoded), artifacts


def validate_report(
    report: dict[str, Any],
    manifest: dict[str, str],
    manifest_digest: str,
    state: dict[str, str],
    provenance: dict[str, str],
) -> None:
    require_exact_keys(
        report,
        {
            "kind",
            "format_version",
            "phase0b_gate_eligible",
            "generated_at_utc",
            "scope",
            "environment",
            "automation",
            "human_review",
            "decision",
            "limitations",
        },
        "report",
    )
    require(report.get("kind") == REPORT_KIND, "unexpected report kind")
    require(report.get("format_version") == 1, "unsupported report format_version")
    require(report.get("phase0b_gate_eligible") is False, "phase0b_gate_eligible must remain false")
    require_utc(report.get("generated_at_utc"), "generated_at_utc")
    require(state.get("format_version") == "1", "state format_version changed")
    require(state.get("classification") == "pre_flight_only", "state classification changed")
    require(state.get("state") == "complete", "pre-flight state is not complete")
    require(state.get("automation_result") == "pass", "pre-flight automation did not pass")
    require(
        state.get("repository_integrity_result") == "pass",
        "repository integrity check did not pass",
    )
    require(state.get("overall_result") == "incomplete", "pre-flight overall result must remain incomplete")

    scope = require_object(report.get("scope"), "scope")
    require_exact_keys(
        scope,
        {
            "repository_commit",
            "clean_worktree",
            "bevy_checkpoint",
            "fixture_class",
            "product_surface",
            "excluded",
        },
        "scope",
    )
    repository_commit = require_text(scope.get("repository_commit"), "scope.repository_commit")
    require(
        re.fullmatch(r"[0-9a-f]{40}", repository_commit) is not None,
        "scope.repository_commit must be a lowercase 40-hex Git commit",
    )
    require(scope.get("clean_worktree") is True, "scope.clean_worktree must be true")
    require(scope.get("bevy_checkpoint") == "0.18.1", "scope.bevy_checkpoint must be 0.18.1")
    require(scope.get("fixture_class") == EXPECTED_FIXTURE_CLASS, "scope.fixture_class changed")
    require(scope.get("product_surface") == EXPECTED_PRODUCT_SURFACE, "scope.product_surface changed")
    require(scope.get("excluded") == EXPECTED_EXCLUDED, "scope.excluded nonclaims changed")
    require(provenance.get("format_version") == "1", "provenance format_version changed")
    require(
        provenance.get("classification") == "pre_flight_only",
        "provenance classification changed",
    )
    require(provenance.get("clean_worktree") == "true", "provenance worktree was not clean")
    require(
        provenance.get("repository_commit") == repository_commit,
        "report commit does not match pre-flight provenance",
    )
    require(
        provenance.get("generated_at_utc") == report.get("generated_at_utc"),
        "report timestamp does not match pre-flight provenance",
    )
    require(
        provenance.get("bevy_checkpoint") == scope.get("bevy_checkpoint"),
        "report Bevy checkpoint does not match pre-flight provenance",
    )
    for field, expected in {
        "browser_render_smoke_window": "640x480",
        "browser_accessibility_preflight_window": "500x900",
        "browser_accessibility_preflight_claim": "narrow_real_browser_preflight_only",
        "browser_zoom_200_percent": "not_run",
        "browser_zoom_400_percent": "not_run",
        "native_voiceover": "not_run",
        "browser_voiceover": "not_run",
    }.items():
        require(provenance.get(field) == expected, f"provenance {field} changed")
    require(
        provenance.get("imagemagick_command") in {"magick", "convert"},
        "provenance ImageMagick command is invalid",
    )

    environment = require_object(report.get("environment"), "environment")
    require_exact_keys(environment, REQUIRED_ENVIRONMENT, "environment")
    for field in sorted(REQUIRED_ENVIRONMENT):
        require_text(environment.get(field), f"environment.{field}")
    for field in (
        "operating_system",
        "architecture",
        "rustc",
        "cargo",
        "node",
        "python",
        "trunk",
        "imagemagick",
        "browser",
    ):
        require(
            environment.get(field) == provenance.get(field),
            f"environment.{field} does not match pre-flight provenance",
        )
    require(
        str(environment.get("operating_system")).startswith("macOS "),
        "version-one accessibility evidence must use macOS",
    )
    require(
        "Chrome" in str(environment.get("browser"))
        or "Chromium" in str(environment.get("browser")),
        "version-one accessibility evidence must record Chrome or Chromium",
    )

    automation = require_object(report.get("automation"), "automation")
    require_exact_keys(
        automation,
        {
            "classification",
            "result",
            "preflight_checksums_sha256",
            "checks",
            "limitations",
        },
        "automation",
    )
    require(automation.get("classification") == "pre_flight_only", "automation classification changed")
    require(automation.get("result") == "pass", "automation.result must be pass")
    require(
        automation.get("preflight_checksums_sha256") == manifest_digest,
        "automation.preflight_checksums_sha256 does not match checksums.sha256",
    )
    require(
        automation.get("limitations") == EXPECTED_AUTOMATION_LIMITATIONS,
        "automation.limitations changed",
    )
    checks = automation.get("checks")
    require(isinstance(checks, list), "automation.checks must be an array")
    seen: set[str] = set()
    for index, value in enumerate(checks):
        check = require_object(value, f"automation.checks[{index}]")
        require_exact_keys(check, {"id", "result", "artifact", "sha256"}, f"automation.checks[{index}]")
        check_id = require_text(check.get("id"), f"automation.checks[{index}].id")
        require(check_id in EXPECTED_AUTOMATION_ARTIFACTS, f"unexpected automation check: {check_id}")
        require(check_id not in seen, f"duplicate automation check: {check_id}")
        seen.add(check_id)
        require(check.get("result") == "pass", f"automation check did not pass: {check_id}")
        state_key = {
            "browser_semantics": "browser_semantics_result",
            "workspace_tests": "workspace_tests_result",
            "browser_smoke_with_500px_preflight": (
                "browser_smoke_with_500px_preflight_result"
            ),
        }[check_id]
        require(state.get(state_key) == "pass", f"checksummed state did not pass: {check_id}")
        artifact = EXPECTED_AUTOMATION_ARTIFACTS[check_id]
        require(check.get("artifact") == artifact, f"automation artifact changed: {check_id}")
        recorded_digest = check.get("sha256")
        require(isinstance(recorded_digest, str) and SHA256_RE.fullmatch(recorded_digest) is not None,
                f"automation artifact digest is invalid: {check_id}")
        require(manifest.get(artifact) == recorded_digest, f"automation artifact digest mismatch: {check_id}")
    require(seen == set(EXPECTED_AUTOMATION_ARTIFACTS), "required automation checks are missing")

    human = require_object(report.get("human_review"), "human_review")
    require_exact_keys(
        human,
        {
            "result",
            "reviewer",
            "reviewer_voiceover_competence_confirmed",
            "reviewed_at_utc",
            "browser",
            "native",
            "notes",
        },
        "human_review",
    )
    require(human.get("result") == "pass", "human_review.result must be pass")
    require_text(human.get("reviewer"), "human_review.reviewer")
    require(
        human.get("reviewer_voiceover_competence_confirmed") is True,
        "human reviewer VoiceOver competence must be confirmed",
    )
    require_utc(human.get("reviewed_at_utc"), "human_review.reviewed_at_utc")
    for host, required_rows in (
        ("browser", REQUIRED_BROWSER_ROWS),
        ("native", REQUIRED_NATIVE_ROWS),
    ):
        rows = require_object(human.get(host), f"human_review.{host}")
        require_exact_keys(rows, required_rows, f"human_review.{host}")
        for row in sorted(required_rows):
            require(rows.get(row) == "pass", f"human_review.{host}.{row} must be pass")
    notes = human.get("notes")
    require(isinstance(notes, list) and all(isinstance(note, str) for note in notes),
            "human_review.notes must be an array of strings")

    decision = require_object(report.get("decision"), "decision")
    require_exact_keys(
        decision,
        {
            "result",
            "authority",
            "decided_at_utc",
            "executor_is_decision_authority",
            "report_digest_recording_authorized",
            "statement",
        },
        "decision",
    )
    require(decision.get("result") == "pass", "decision.result must be the human-authored pass")
    require_text(decision.get("authority"), "decision.authority")
    require_utc(decision.get("decided_at_utc"), "decision.decided_at_utc")
    require(isinstance(decision.get("executor_is_decision_authority"), bool),
            "decision.executor_is_decision_authority must be true or false")
    require(
        decision.get("report_digest_recording_authorized") is True,
        "decision.report_digest_recording_authorized must be true",
    )
    require(
        "report_digest_recorded_in_plan" not in decision,
        "remove the circular decision.report_digest_recorded_in_plan field",
    )
    require(decision.get("statement") == EXPECTED_DECISION_STATEMENT, "decision.statement changed")
    require(report.get("limitations") == EXPECTED_LIMITATIONS, "top-level limitations changed")


def validate_evidence(evidence_dir: pathlib.Path) -> str:
    require(evidence_dir.is_absolute(), "evidence directory path must be absolute")
    require(
        evidence_dir.is_dir() and not evidence_dir.is_symlink(),
        "evidence directory must exist and must not be a symlink",
    )
    manifest, manifest_digest, artifacts = load_and_verify_manifest(evidence_dir)
    state, provenance = load_state_and_provenance(artifacts)
    report_path = evidence_dir / "report.json"
    report, report_digest = load_report(report_path)
    validate_report(
        report,
        manifest,
        manifest_digest,
        state,
        provenance,
    )
    return report_digest


def valid_self_test_report(digests: dict[str, str]) -> dict[str, Any]:
    environment = {field: "recorded" for field in REQUIRED_ENVIRONMENT}
    environment.update(
        {
            "operating_system": "macOS 15.0",
            "architecture": "arm64",
            "rustc": "rustc 1.89.0",
            "cargo": "cargo 1.89.0",
            "node": "v24.0.0",
            "python": "Python 3.9.6",
            "trunk": "trunk 0.21.14",
            "imagemagick": "Version: ImageMagick 7.1.0",
            "browser": "Google Chrome 139.0.0.0",
        }
    )
    return {
        "kind": REPORT_KIND,
        "format_version": 1,
        "phase0b_gate_eligible": False,
        "generated_at_utc": "2026-08-09T00:00:00Z",
        "scope": {
            "repository_commit": "0123456789abcdef0123456789abcdef01234567",
            "clean_worktree": True,
            "bevy_checkpoint": "0.18.1",
            "fixture_class": EXPECTED_FIXTURE_CLASS,
            "product_surface": EXPECTED_PRODUCT_SURFACE,
            "excluded": EXPECTED_EXCLUDED,
        },
        "environment": environment,
        "automation": {
            "classification": "pre_flight_only",
            "result": "pass",
            "checks": [
                {
                    "id": check_id,
                    "result": "pass",
                    "artifact": artifact,
                    "sha256": digests[artifact],
                }
                for check_id, artifact in EXPECTED_AUTOMATION_ARTIFACTS.items()
            ],
            "limitations": EXPECTED_AUTOMATION_LIMITATIONS,
        },
        "human_review": {
            "result": "pass",
            "reviewer": "Self Test",
            "reviewer_voiceover_competence_confirmed": True,
            "reviewed_at_utc": "2026-08-09T00:00:00Z",
            "browser": {row: "pass" for row in REQUIRED_BROWSER_ROWS},
            "native": {row: "pass" for row in REQUIRED_NATIVE_ROWS},
            "notes": [],
        },
        "decision": {
            "result": "pass",
            "authority": "Self Test",
            "decided_at_utc": "2026-08-09T00:00:00Z",
            "executor_is_decision_authority": True,
            "report_digest_recording_authorized": True,
            "statement": EXPECTED_DECISION_STATEMENT,
        },
        "limitations": EXPECTED_LIMITATIONS,
    }


def run_self_test() -> None:
    with tempfile.TemporaryDirectory(prefix="spinal-a11y-report-check-") as temporary:
        root = pathlib.Path(temporary)
        preflight = root / "preflight"
        preflight.mkdir()
        for relative in EXPECTED_AUTOMATION_ARTIFACTS.values():
            (root / relative).write_text(f"PASS: {relative}\n", encoding="utf-8")
        state_path = preflight / "state.txt"
        provenance_path = preflight / "provenance.txt"
        state_path.write_text(
            "\n".join(
                [
                    "format_version=1",
                    "classification=pre_flight_only",
                    "state=complete",
                    "automation_result=pass",
                    "browser_semantics_result=pass",
                    "workspace_tests_result=pass",
                    "browser_smoke_with_500px_preflight_result=pass",
                    "repository_integrity_result=pass",
                    "overall_result=incomplete",
                    "",
                ]
            ),
            encoding="utf-8",
        )
        commit = "0123456789abcdef0123456789abcdef01234567"

        def provenance_text(repository_commit: str = commit) -> str:
            return "\n".join(
                [
                    "format_version=1",
                    "classification=pre_flight_only",
                    "bevy_checkpoint=0.18.1",
                    f"repository_commit={repository_commit}",
                    "clean_worktree=true",
                    "generated_at_utc=2026-08-09T00:00:00Z",
                    "operating_system=macOS 15.0",
                    "architecture=arm64",
                    "rustc=rustc 1.89.0",
                    "cargo=cargo 1.89.0",
                    "node=v24.0.0",
                    "python=Python 3.9.6",
                    "trunk=trunk 0.21.14",
                    "imagemagick=Version: ImageMagick 7.1.0",
                    "imagemagick_command=magick",
                    "browser=Google Chrome 139.0.0.0",
                    "browser_render_smoke_window=640x480",
                    "browser_accessibility_preflight_window=500x900",
                    "browser_accessibility_preflight_claim=narrow_real_browser_preflight_only",
                    "browser_zoom_200_percent=not_run",
                    "browser_zoom_400_percent=not_run",
                    "native_voiceover=not_run",
                    "browser_voiceover=not_run",
                    "",
                    "locked spinal-app direct dependency tree:",
                    "spinal-app v0.1.0",
                    "├── bevy v0.18.1",
                    "",
                ]
            )

        provenance_path.write_text(provenance_text(), encoding="utf-8")

        def write_manifest() -> dict[str, str]:
            digests = {
                path.relative_to(root).as_posix(): sha256(path)
                for path in sorted(preflight.rglob("*"))
                if path.is_file()
            }
            (root / "checksums.sha256").write_text(
                "".join(
                    f"{digest}  {relative}\n"
                    for relative, digest in sorted(digests.items())
                ),
                encoding="utf-8",
            )
            return digests

        digests = write_manifest()
        report = valid_self_test_report(digests)

        def write_report(value: dict[str, Any]) -> None:
            value["automation"]["preflight_checksums_sha256"] = sha256(
                root / "checksums.sha256"
            )
            (root / "report.json").write_text(
                json.dumps(value, indent=2) + "\n",
                encoding="utf-8",
            )

        def expect_rejected(message: str) -> None:
            try:
                validate_evidence(root)
            except ValidationError:
                return
            raise ValidationError(message)

        write_report(report)
        validate_evidence(root)

        rejected = copy.deepcopy(report)
        rejected["human_review"]["native"]["voiceover"] = "not_run"
        write_report(rejected)
        expect_rejected("self-test accepted an incomplete human decision")

        rejected = copy.deepcopy(report)
        rejected["phase0b_gate_eligible"] = True
        write_report(rejected)
        expect_rejected("self-test accepted Phase 0B eligibility")

        rejected = copy.deepcopy(report)
        rejected["accessibility_score"] = 100
        write_report(rejected)
        expect_rejected("self-test accepted an unknown numeric score")

        state_path.write_text(
            state_path.read_text(encoding="utf-8").replace(
                "repository_integrity_result=pass",
                "repository_integrity_result=fail",
            ),
            encoding="utf-8",
        )
        write_manifest()
        write_report(report)
        expect_rejected("self-test accepted failed repository integrity")

        state_path.write_text(
            state_path.read_text(encoding="utf-8").replace(
                "repository_integrity_result=fail",
                "repository_integrity_result=pass",
            ),
            encoding="utf-8",
        )
        state_path.write_text(
            state_path.read_text(encoding="utf-8").replace(
                "automation_result=pass",
                "automation_result=fail",
            ),
            encoding="utf-8",
        )
        write_manifest()
        write_report(report)
        expect_rejected("self-test accepted checksummed failed automation state")

        state_path.write_text(
            state_path.read_text(encoding="utf-8").replace(
                "automation_result=fail",
                "automation_result=pass",
            ),
            encoding="utf-8",
        )
        provenance_path.write_text(
            provenance_text("fedcba9876543210fedcba9876543210fedcba98"),
            encoding="utf-8",
        )
        write_manifest()
        write_report(report)
        expect_rejected("self-test accepted mismatched checksummed provenance")

        provenance_path.write_text(provenance_text(), encoding="utf-8")
        write_manifest()
        write_report(report)
        changed = root / EXPECTED_AUTOMATION_ARTIFACTS["browser_semantics"]
        changed.write_text("changed\n", encoding="utf-8")
        expect_rejected("self-test accepted a changed artifact")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("evidence", nargs="?", help="absolute accessibility evidence directory")
    parser.add_argument("--self-test", action="store_true", help="run dependency-free validator tests")
    arguments = parser.parse_args()
    try:
        if arguments.self_test:
            require(arguments.evidence is None, "--self-test does not accept an evidence directory")
            run_self_test()
            print("Accessibility report checker self-test passed")
            return 0
        require(arguments.evidence is not None, "an absolute evidence directory is required")
        evidence = pathlib.Path(arguments.evidence)
        digest = validate_evidence(evidence)
    except ValidationError as error:
        print(f"Accessibility report validation failed: {error}", file=sys.stderr)
        return 1
    print("Accessibility report and pre-flight artifact checksums are valid.")
    print(f"accessibility_report_sha256={digest}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

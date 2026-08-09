#!/usr/bin/env bash
set -uo pipefail

umask 077

usage() {
    echo "usage: tools/accessibility-preflight.sh ABSOLUTE_NEW_EVIDENCE_DIR [PORT]" >&2
    echo "automation is PRE-FLIGHT only; human keyboard and VoiceOver review remains required" >&2
}

if [[ "$#" -lt 1 || "$#" -gt 2 ]]; then
    usage
    exit 2
fi

evidence_input="$1"
port="${2:-8426}"

if [[ ! "$port" =~ ^[0-9]+$ ]] || ((port < 1024 || port > 65535)); then
    usage
    exit 2
fi
if [[ "$evidence_input" != /* ]]; then
    echo "accessibility pre-flight evidence path must be absolute" >&2
    exit 2
fi
if [[ -e "$evidence_input" || -L "$evidence_input" ]]; then
    echo "accessibility pre-flight evidence destination already exists: $evidence_input" >&2
    exit 2
fi

evidence_parent="$(dirname "$evidence_input")"
evidence_name="$(basename "$evidence_input")"
if [[ ! -d "$evidence_parent" ]]; then
    echo "accessibility pre-flight evidence parent does not exist: $evidence_parent" >&2
    exit 2
fi
if [[ ! "$evidence_name" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]]; then
    echo "accessibility pre-flight evidence directory name must use letters, digits, dot, underscore, or dash" >&2
    exit 2
fi

repo_root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
    echo "accessibility pre-flight must run from the Spinal repository" >&2
    exit 2
}
repo_root="$(cd "$repo_root" && pwd -P)" || {
    echo "accessibility pre-flight could not resolve the Spinal repository" >&2
    exit 2
}
evidence_parent="$(cd "$evidence_parent" && pwd -P)" || {
    echo "accessibility pre-flight could not resolve the evidence parent" >&2
    exit 2
}
evidence_dir="$evidence_parent/$evidence_name"

case "$evidence_dir" in
    "$repo_root" | "$repo_root"/*)
        echo "accessibility pre-flight evidence must stay outside the Git repository" >&2
        exit 2
        ;;
esac

cd "$repo_root" || {
    echo "accessibility pre-flight could not enter the Spinal repository" >&2
    exit 2
}
tree_state="$(git status --porcelain --untracked-files=normal)" || {
    echo "accessibility pre-flight could not inspect the worktree" >&2
    exit 2
}
if [[ -n "$tree_state" ]]; then
    echo "accessibility pre-flight requires a clean committed worktree" >&2
    exit 2
fi

for required in git cargo rustc python3 node trunk curl sw_vers; do
    if ! command -v "$required" >/dev/null 2>&1; then
        echo "accessibility pre-flight requires $required" >&2
        exit 2
    fi
done
if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "accessibility pre-flight version one supports only the recorded macOS profile" >&2
    exit 2
fi
if ! node -e '
    if (
        typeof fetch !== "function"
        || typeof WebSocket !== "function"
        || typeof AbortController !== "function"
    ) process.exit(1);
' >/dev/null 2>&1; then
    echo "accessibility pre-flight requires Node.js with global fetch, WebSocket, and AbortController APIs" >&2
    exit 2
fi
if command -v magick >/dev/null 2>&1; then
    imagemagick_command="magick"
elif command -v convert >/dev/null 2>&1; then
    imagemagick_command="convert"
else
    echo "accessibility pre-flight requires ImageMagick" >&2
    exit 2
fi

record_version() {
    local label="$1"
    shift
    local output
    if ! output="$("$@" --version 2>&1)"; then
        echo "accessibility pre-flight could not record the $label version" >&2
        return 1
    fi
    output="${output%%$'\n'*}"
    if [[ -z "$output" ]]; then
        echo "accessibility pre-flight received an empty $label version" >&2
        return 1
    fi
    printf '%s\n' "$output"
}

node_version="$(record_version "Node.js" node)" || exit 2
python_version="$(record_version "Python" python3)" || exit 2
trunk_version="$(record_version "Trunk" trunk)" || exit 2
rustc_version="$(record_version "Rust compiler" rustc)" || exit 2
cargo_version="$(record_version "Cargo" cargo)" || exit 2
imagemagick_version="$(
    record_version "ImageMagick" "$imagemagick_command"
)" || exit 2
report_template_json="$(<docs/accessibility-report-v1.example.json)" || {
    echo "accessibility pre-flight could not snapshot the report template" >&2
    exit 2
}
if [[ -z "$report_template_json" ]]; then
    echo "accessibility pre-flight received an empty report template" >&2
    exit 2
fi

bevy_checkpoint="0.19.0"
accesskit_checkpoint="0.24.1"
dependency_tree="$(CARGO_TERM_COLOR=never cargo tree --locked --package spinal-app --depth 1 2>&1)" || {
    echo "accessibility pre-flight could not record the locked dependency tree" >&2
    exit 2
}
python3 - "$bevy_checkpoint" "$accesskit_checkpoint" "$dependency_tree" <<'PY' || exit 2
import re
import sys


_program, bevy, accesskit, tree = sys.argv
for package, expected in (("bevy", bevy), ("accesskit", accesskit)):
    matches = [
        match.group(1)
        for line in tree.splitlines()[1:]
        if (
            match := re.fullmatch(
                rf"[├└]── {re.escape(package)} v([^ ]+)(?: .*)?",
                line,
            )
        )
    ]
    if matches != [expected]:
        print(
            f"accessibility pre-flight requires exactly one direct "
            f"{package} v{expected}; found {matches}",
            file=sys.stderr,
        )
        raise SystemExit(1)
PY

mkdir -m 700 "$evidence_dir" || exit 2
mkdir -m 700 "$evidence_dir/preflight" || exit 2
state_file="$evidence_dir/preflight/state.txt"
printf '%s\n' \
    'format_version=1' \
    'classification=pre_flight_only' \
    'state=running' \
    'overall_result=incomplete' >"$state_file"

# shellcheck disable=SC2329 # Invoked indirectly by the signal traps below.
interrupted() {
    local signal="$1"
    printf '%s\n' \
        'format_version=1' \
        'classification=pre_flight_only' \
        "state=interrupted_by_${signal}" \
        'automation_result=incomplete' \
        'overall_result=incomplete' >"$state_file"
    echo "accessibility pre-flight interrupted; partial evidence retained at $evidence_dir" >&2
    exit 130
}
trap 'interrupted hup' HUP
trap 'interrupted int' INT
trap 'interrupted term' TERM

discover_browser() {
    local candidate
    if [[ -n "${CHROME_BIN:-}" && -x "${CHROME_BIN}" ]]; then
        printf '%s\n' "$CHROME_BIN"
        return 0
    fi
    candidate="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
    if [[ -x "$candidate" ]]; then
        printf '%s\n' "$candidate"
        return 0
    fi
    for candidate in google-chrome google-chrome-stable chromium chromium-browser; do
        if command -v "$candidate" >/dev/null 2>&1; then
            command -v "$candidate"
            return 0
        fi
    done
    return 1
}

browser_path="$(discover_browser || true)"
browser_version=""
if [[ -n "$browser_path" ]]; then
    browser_version="$("$browser_path" --version 2>/dev/null || true)"
fi
operating_system_version="$(sw_vers -productVersion)" || {
    echo "accessibility pre-flight could not record the macOS version" >&2
    exit 2
}
if [[ -z "$operating_system_version" ]]; then
    echo "accessibility pre-flight received an empty macOS version" >&2
    exit 2
fi
operating_system="macOS $operating_system_version"
architecture="$(uname -m)"
commit="$(git rev-parse HEAD)"
generated_at_utc="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"

{
    printf 'format_version=1\n'
    printf 'classification=pre_flight_only\n'
    printf 'bevy_checkpoint=%s\n' "$bevy_checkpoint"
    printf 'repository_commit=%s\n' "$commit"
    printf 'clean_worktree=true\n'
    printf 'generated_at_utc=%s\n' "$generated_at_utc"
    printf 'operating_system=%s\n' "$operating_system"
    printf 'architecture=%s\n' "$architecture"
    printf 'rustc=%s\n' "$rustc_version"
    printf 'cargo=%s\n' "$cargo_version"
    printf 'node=%s\n' "$node_version"
    printf 'python=%s\n' "$python_version"
    printf 'trunk=%s\n' "$trunk_version"
    printf 'imagemagick=%s\n' "$imagemagick_version"
    printf 'imagemagick_command=%s\n' "$imagemagick_command"
    printf 'browser=%s\n' "${browser_version:-not_discovered}"
    printf 'browser_render_smoke_window=640x480\n'
    printf 'browser_accessibility_preflight_window=500x900\n'
    printf 'browser_accessibility_preflight_claim=narrow_real_browser_preflight_only\n'
    printf 'browser_zoom_200_percent=not_run\n'
    printf 'browser_zoom_400_percent=not_run\n'
    printf 'native_voiceover=not_run\n'
    printf 'browser_voiceover=not_run\n'
    printf '\nlocked spinal-app direct dependency tree:\n'
    printf '%s\n' "$dependency_tree"
} >"$evidence_dir/preflight/provenance.txt"

# shellcheck disable=SC2329 # Passed by name to run_logged below.
browser_semantics() {
    python3 - apps/spinal/web/index.html <<'PY'
import sys
from html.parser import HTMLParser


class ShellAudit(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.ids = {}
        self.references = []
        self.labels_for = set()
        self.label_text = {}
        self.current_label = None
        self.controls = {}
        self.live_regions = []
        self.buttons = []
        self.current_button = None
        self.autofocus = []
        self.errors = []

    def handle_starttag(self, tag, attributes):
        attrs = dict(attributes)
        element_id = attrs.get("id")
        if element_id:
            if element_id in self.ids:
                self.errors.append(f"duplicate id: {element_id}")
            self.ids[element_id] = (tag, attrs)
        for attribute in ("aria-controls", "aria-describedby", "aria-labelledby"):
            for target in attrs.get(attribute, "").split():
                self.references.append((element_id or tag, attribute, target))
        if tag == "label" and attrs.get("for"):
            target = attrs["for"]
            self.labels_for.add(target)
            self.label_text.setdefault(target, [])
            self.current_label = target
        if tag in {"button", "input", "select", "textarea"} and element_id:
            self.controls[element_id] = (tag, attrs)
        if "aria-live" in attrs:
            self.live_regions.append((element_id, attrs))
        if "autofocus" in attrs:
            self.autofocus.append(element_id or tag)
        tabindex = attrs.get("tabindex")
        if tabindex is not None:
            try:
                if int(tabindex) > 0:
                    self.errors.append(f"positive tabindex on {element_id or tag}: {tabindex}")
            except ValueError:
                self.errors.append(f"invalid tabindex on {element_id or tag}: {tabindex}")
        if tag == "button":
            self.current_button = {"id": element_id, "attrs": attrs, "text": []}
            self.buttons.append(self.current_button)

    def handle_data(self, data):
        if self.current_label is not None:
            self.label_text[self.current_label].append(data)
        if self.current_button is not None:
            self.current_button["text"].append(data)

    def handle_endtag(self, tag):
        if tag == "label":
            self.current_label = None
        if tag == "button":
            self.current_button = None


path = sys.argv[1]
audit = ShellAudit()
with open(path, "r", encoding="utf-8") as source:
    audit.feed(source.read())

for owner, attribute, target in audit.references:
    if target not in audit.ids:
        audit.errors.append(f"{owner} {attribute} references missing id: {target}")
for target in audit.labels_for:
    if target not in audit.ids:
        audit.errors.append(f"label references missing control: {target}")
for control_id, (tag, attrs) in audit.controls.items():
    if tag in {"input", "select", "textarea"} and not (
        control_id in audit.labels_for or attrs.get("aria-label") or attrs.get("aria-labelledby")
    ):
        audit.errors.append(f"form control has no programmatic label: {control_id}")
for button in audit.buttons:
    button_id = button["id"] or "unnamed-button"
    expected_type = "submit" if button_id == "spinal-open-submit" else "button"
    if button["attrs"].get("type") != expected_type:
        audit.errors.append(f"button is not type={expected_type}: {button_id}")
    name = button["attrs"].get("aria-label") or "".join(button["text"]).strip()
    if not name:
        audit.errors.append(f"button has no accessible name: {button_id}")
if audit.autofocus:
    audit.errors.append(f"unexpected autofocus: {', '.join(audit.autofocus)}")
if len(audit.live_regions) != 1:
    audit.errors.append(f"expected one live region, found {len(audit.live_regions)}")
elif audit.live_regions[0][1].get("aria-live") != "polite":
    audit.errors.append("the sole live region is not polite")

required = {
    "spinal-app",
    "spinal-open-panel",
    "spinal-open-form",
    "spinal-open-files",
    "spinal-open-comparison-files",
    "spinal-open-submit",
    "spinal-open-error",
    "spinal-viewer",
    "spinal-status",
    "spinal-canvas",
    "spinal-transport",
    "spinal-camera-state",
    "spinal-diagnostics",
    "spinal-diagnostics-summary",
}
for required_id in sorted(required - set(audit.ids)):
    audit.errors.append(f"missing required semantic element: {required_id}")
open_inputs = {
    "spinal-open-files": ("Primary runtime-export directory (required)", True),
    "spinal-open-comparison-files": (
        "Comparison runtime-export directory (optional)",
        False,
    ),
}
for open_input_id, (expected_label, is_required) in open_inputs.items():
    open_input = audit.ids.get(open_input_id)
    if not open_input:
        continue
    tag, attrs = open_input
    if tag != "input" or attrs.get("type") != "file":
        audit.errors.append(f"Open directory control is not a file input: {open_input_id}")
    for required_attribute in ("multiple", "webkitdirectory", "disabled"):
        if required_attribute not in attrs:
            audit.errors.append(
                f"Open directory control {open_input_id} is missing {required_attribute}"
            )
    label = " ".join("".join(audit.label_text.get(open_input_id, [])).split())
    if label != expected_label:
        audit.errors.append(
            f"Open directory control {open_input_id} has label {label!r}; "
            f"expected {expected_label!r}"
        )
    described_by = set(attrs.get("aria-describedby", "").split())
    if not {"spinal-open-help", "spinal-open-error"}.issubset(described_by):
        audit.errors.append(
            f"Open directory control {open_input_id} is not described by help and error"
        )
    if is_required:
        if "required" not in attrs or attrs.get("aria-required") != "true":
            audit.errors.append("Primary Open directory control is not required")
    elif "required" in attrs or "aria-required" in attrs:
        audit.errors.append("optional Comparison Open directory control is marked required")
open_submit = audit.ids.get("spinal-open-submit")
if open_submit and "disabled" not in open_submit[1]:
    audit.errors.append("Open submit is not disabled before Rust installs its listener")
open_error = audit.ids.get("spinal-open-error")
if open_error and (
    open_error[1].get("role") != "alert"
    or open_error[1].get("tabindex") != "-1"
):
    audit.errors.append("Open error is not a focusable alert target")
viewer = audit.ids.get("spinal-viewer")
if viewer and "hidden" not in viewer[1]:
    audit.errors.append("viewer is not hidden before Open succeeds")
canvas = audit.ids.get("spinal-canvas")
if canvas:
    _, attrs = canvas
    if attrs.get("role") != "img":
        audit.errors.append("canvas role is not img")
    if attrs.get("tabindex") != "0":
        audit.errors.append("canvas is not in the natural tab order")
    if not attrs.get("aria-label"):
        audit.errors.append("canvas has no accessible name")

if audit.errors:
    for error in sorted(audit.errors):
        print(f"FAIL: {error}")
    raise SystemExit(1)
print(f"PASS: {len(audit.ids)} unique ids")
print(f"PASS: {len(audit.controls)} named form/button controls")
print("PASS: every ARIA id reference resolves")
print("PASS: exactly one polite live region")
print("LIMIT: structural DOM audit only; no keyboard, zoom/reflow, contrast, or AT claim")
PY
}

LAST_STATUS=0
run_logged() {
    local label="$1"
    local log="$2"
    shift 2
    echo "Accessibility PRE-FLIGHT: $label"
    "$@" 2>&1 | tee "$log"
    LAST_STATUS="${PIPESTATUS[0]}"
    if [[ "$LAST_STATUS" -eq 0 ]]; then
        echo "Accessibility PRE-FLIGHT check passed: $label"
    else
        echo "Accessibility PRE-FLIGHT check failed: $label" >&2
    fi
}

run_logged \
    "browser semantic shell" \
    "$evidence_dir/preflight/browser-semantics.log" \
    browser_semantics
browser_semantics_status="$LAST_STATUS"

run_logged \
    "locked workspace tests" \
    "$evidence_dir/preflight/workspace-tests.log" \
    cargo test --workspace --all-targets --locked
workspace_tests_status="$LAST_STATUS"

run_logged \
    "real Chrome smoke including 500px narrow pre-flight (not zoom/reflow evidence)" \
    "$evidence_dir/preflight/browser-smoke-with-500px-preflight.log" \
    bash tools/web-smoke.sh "$port"
browser_smoke_status="$LAST_STATUS"

repository_integrity_status=0
post_commit="$(git rev-parse HEAD 2>/dev/null)" || repository_integrity_status=1
post_tree_state="$(git status --porcelain --untracked-files=normal 2>/dev/null)" \
    || repository_integrity_status=1
if [[ "$repository_integrity_status" -ne 0 \
    || "$post_commit" != "$commit" \
    || -n "$post_tree_state" ]]; then
    repository_integrity_status=1
    echo "Accessibility PRE-FLIGHT check failed: repository changed while checks were running" >&2
else
    echo "Accessibility PRE-FLIGHT check passed: repository remained clean at $commit"
fi

if [[ "$browser_semantics_status" -eq 0 \
    && "$workspace_tests_status" -eq 0 \
    && "$browser_smoke_status" -eq 0 \
    && "$repository_integrity_status" -eq 0 ]]; then
    automation_result="pass"
else
    automation_result="fail"
fi

result_word() {
    if [[ "$1" -eq 0 ]]; then
        printf '%s\n' 'pass'
    else
        printf '%s\n' 'fail'
    fi
}
browser_semantics_result="$(result_word "$browser_semantics_status")"
workspace_tests_result="$(result_word "$workspace_tests_status")"
browser_smoke_result="$(result_word "$browser_smoke_status")"
repository_integrity_result="$(result_word "$repository_integrity_status")"

printf '%s\n' \
    'format_version=1' \
    'classification=pre_flight_only' \
    'state=complete' \
    "automation_result=$automation_result" \
    "browser_semantics_result=$browser_semantics_result" \
    "workspace_tests_result=$workspace_tests_result" \
    "browser_smoke_with_500px_preflight_result=$browser_smoke_result" \
    "repository_integrity_result=$repository_integrity_result" \
    'overall_result=incomplete' >"$state_file"

python3 - \
    "$report_template_json" \
    "$evidence_dir/report.json" \
    "$evidence_dir/checksums.sha256" \
    "$evidence_dir/preflight" \
    "$commit" \
    "$bevy_checkpoint" \
    "$generated_at_utc" \
    "$operating_system" \
    "$architecture" \
    "$rustc_version" \
    "$cargo_version" \
    "$node_version" \
    "$python_version" \
    "$trunk_version" \
    "$imagemagick_version" \
    "$browser_version" \
    "$automation_result" \
    "$browser_semantics_status" \
    "$workspace_tests_status" \
    "$browser_smoke_status" <<'PY'
import hashlib
import json
import pathlib
import sys

(
    _program,
    report_template_json,
    report_path,
    checksums_path,
    preflight_path,
    commit,
    bevy_checkpoint,
    generated_at_utc,
    operating_system,
    architecture,
    rustc_version,
    cargo_version,
    node_version,
    python_version,
    trunk_version,
    imagemagick_version,
    browser_version,
    automation_result,
    browser_semantics_status,
    workspace_tests_status,
    browser_smoke_status,
) = sys.argv


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


preflight = pathlib.Path(preflight_path)
evidence_root = preflight.parent
artifacts = sorted(path for path in preflight.rglob("*") if path.is_file())
expected_artifacts = {
    "preflight/browser-semantics.log",
    "preflight/workspace-tests.log",
    "preflight/browser-smoke-with-500px-preflight.log",
    "preflight/provenance.txt",
    "preflight/state.txt",
}
actual_artifacts = {
    path.relative_to(evidence_root).as_posix()
    for path in artifacts
}
if actual_artifacts != expected_artifacts:
    raise SystemExit("pre-flight evidence does not contain the exact version-one artifact set")
with open(checksums_path, "w", encoding="utf-8", newline="\n") as output:
    for artifact in artifacts:
        relative = artifact.relative_to(evidence_root).as_posix()
        output.write(f"{sha256(artifact)}  {relative}\n")

report = json.loads(report_template_json)
report["generated_at_utc"] = generated_at_utc
report["scope"]["repository_commit"] = commit
report["scope"]["clean_worktree"] = True
report["scope"]["bevy_checkpoint"] = bevy_checkpoint
report["environment"]["operating_system"] = operating_system
report["environment"]["architecture"] = architecture
report["environment"]["rustc"] = rustc_version
report["environment"]["cargo"] = cargo_version
report["environment"]["node"] = node_version
report["environment"]["python"] = python_version
report["environment"]["trunk"] = trunk_version
report["environment"]["imagemagick"] = imagemagick_version
report["environment"]["browser"] = browser_version or None
report["automation"]["result"] = automation_result
statuses = {
    "browser_semantics": browser_semantics_status,
    "workspace_tests": workspace_tests_status,
    "browser_smoke_with_500px_preflight": browser_smoke_status,
}
for check in report["automation"]["checks"]:
    status = int(statuses[check["id"]])
    check["result"] = "pass" if status == 0 else "fail"
    check["sha256"] = sha256(evidence_root / check["artifact"])
report["automation"]["preflight_checksums_sha256"] = sha256(pathlib.Path(checksums_path))
report["human_review"]["result"] = "not_run"
report["decision"]["result"] = "incomplete"
report["decision"]["authority"] = None
report["decision"]["report_digest_recording_authorized"] = False
with open(report_path, "w", encoding="utf-8", newline="\n") as output:
    json.dump(report, output, ensure_ascii=False, indent=2)
    output.write("\n")
PY
report_status="$?"

trap - HUP INT TERM
if [[ "$report_status" -ne 0 ]]; then
    printf '%s\n' \
        'format_version=1' \
        'classification=pre_flight_only' \
        'state=evidence_write_failed' \
        "automation_result=$automation_result" \
        'overall_result=incomplete' >"$state_file"
    echo "accessibility pre-flight could not publish its report; evidence retained at $evidence_dir" >&2
    exit 1
fi

if [[ "$automation_result" == "pass" ]]; then
    echo "Accessibility PRE-FLIGHT PASS"
    echo "Overall accessibility acceptance remains INCOMPLETE pending named human browser and native keyboard and VoiceOver review."
    echo "Evidence: $evidence_dir"
    exit 0
fi

echo "Accessibility PRE-FLIGHT FAIL" >&2
echo "Overall accessibility acceptance remains INCOMPLETE; evidence retained at $evidence_dir" >&2
exit 1

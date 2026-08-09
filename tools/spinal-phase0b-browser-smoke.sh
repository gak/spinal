#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="$(cd -- "$script_dir/.." && pwd -P)"
cd "$repo_root"

port="${1:-8427}"
if [[ ! "$port" =~ ^[1-9][0-9]{3,4}$ ]] || ((port < 1024 || port > 65535)); then
    echo "usage: tools/spinal-phase0b-browser-smoke.sh [port from 1024 through 65535]" >&2
    exit 2
fi

keep_diagnostics="${SPINAL_KEEP_PHASE0B_BROWSER_SMOKE:-0}"
if [[ "$keep_diagnostics" != "0" && "$keep_diagnostics" != "1" ]]; then
    echo "SPINAL_KEEP_PHASE0B_BROWSER_SMOKE must be 0 or 1" >&2
    exit 2
fi

for required in cargo trunk node curl python3 git rustc; do
    if ! command -v "$required" >/dev/null 2>&1; then
        echo "Phase 0B browser smoke requires $required" >&2
        exit 1
    fi
done

chrome="${CHROME_BIN:-}"
if [[ -n "$chrome" && ! -x "$chrome" ]]; then
    if command -v "$chrome" >/dev/null 2>&1; then
        chrome="$(command -v "$chrome")"
    else
        echo "CHROME_BIN is not an executable Chrome or Chromium binary: $chrome" >&2
        exit 1
    fi
fi
if [[ -z "$chrome" ]]; then
    for candidate in \
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
        "/Applications/Chromium.app/Contents/MacOS/Chromium" \
        "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary"; do
        if [[ -x "$candidate" ]]; then
            chrome="$candidate"
            break
        fi
    done
fi
if [[ -z "$chrome" ]]; then
    for candidate in google-chrome google-chrome-stable chromium chromium-browser; do
        if command -v "$candidate" >/dev/null 2>&1; then
            chrome="$(command -v "$candidate")"
            break
        fi
    done
fi
if [[ -z "$chrome" || ! -x "$chrome" ]]; then
    echo "Phase 0B browser smoke requires Chrome/Chromium or CHROME_BIN" >&2
    exit 1
fi

if [[ ! -f tools/spinal-phase0b-cdp.js ]]; then
    echo "Phase 0B browser smoke requires tools/spinal-phase0b-cdp.js" >&2
    exit 1
fi
node --check tools/spinal-phase0b-cdp.js
node tools/spinal-phase0b-cdp.js --self-test

temp_parent="${TMPDIR:-/tmp}"
if [[ ! -d "$temp_parent" ]]; then
    echo "temporary directory parent does not exist: $temp_parent" >&2
    exit 1
fi
temp_parent="$(cd -- "$temp_parent" && pwd -P)"
umask 077
smoke_dir="$(mktemp -d "${temp_parent%/}/spinal-phase0b-browser-smoke.XXXXXX")"
smoke_dir="$(cd -- "$smoke_dir" && pwd -P)"
case "$smoke_dir" in
    "${temp_parent%/}"/spinal-phase0b-browser-smoke.*) ;;
    *)
        echo "refusing unsafe Phase 0B smoke directory: $smoke_dir" >&2
        exit 1
        ;;
esac
chmod 700 "$smoke_dir"
nonce="$(python3 -c 'import secrets; print(secrets.token_hex(32))')"
if [[ ! "$nonce" =~ ^[0-9a-f]{64}$ ]]; then
    echo "Phase 0B browser smoke could not generate a fresh 256-bit nonce" >&2
    exit 1
fi

server_pid=""
chrome_pid=""
fixture_restore_needed=0
phase0b_target=""

count_literal() {
    local needle="$1"
    local file="$2"
    local count=0
    local line
    local remainder
    while IFS= read -r line || [[ -n "$line" ]]; do
        remainder="$line"
        while [[ "$remainder" == *"$needle"* ]]; do
            count="$((count + 1))"
            remainder="${remainder#*"$needle"}"
        done
    done <"$file"
    printf '%s\n' "$count"
}

stop_pid() {
    local pid="$1"
    [[ -n "$pid" ]] || return 0
    if kill -0 "$pid" 2>/dev/null; then
        kill "$pid" 2>/dev/null || true
        local attempt
        for ((attempt = 0; attempt < 40; attempt += 1)); do
            kill -0 "$pid" 2>/dev/null || break
            sleep 0.05
        done
        if kill -0 "$pid" 2>/dev/null; then
            kill -9 "$pid" 2>/dev/null || true
        fi
    fi
    wait "$pid" 2>/dev/null || true
}

cleanup() {
    local status="$?"
    local cleanup_failed=0
    local restore_failed=0
    trap - EXIT INT TERM

    stop_pid "$chrome_pid"
    stop_pid "$server_pid"

    if [[ -n "$phase0b_target" ]]; then
        case "$phase0b_target" in
            "$repo_root"/apps/spinal/web/.spinal-phase0b-rehearsal.*)
                if ! rm -f -- "$phase0b_target"; then
                    cleanup_failed=1
                    echo "Phase 0B browser smoke could not remove its private Trunk target" >&2
                fi
                ;;
            *)
                cleanup_failed=1
                echo "refusing to remove an unexpected Phase 0B Trunk target: $phase0b_target" >&2
                ;;
        esac
    fi

    if ((fixture_restore_needed)); then
        if ! env -u NO_COLOR cargo run --locked \
            --package spinal-app \
            --example prepare_web_fixture -- \
            apps/spinal/web/bundle >"$smoke_dir/fixture-restore.log" 2>&1; then
            restore_failed=1
            echo "Phase 0B browser smoke could not restore the ordinary web fixture:" >&2
            cat "$smoke_dir/fixture-restore.log" >&2
        fi
    fi

    if [[ "$keep_diagnostics" == "1" ]]; then
        echo "Phase 0B browser smoke diagnostics retained at $smoke_dir" >&2
    else
        rm -rf -- "$smoke_dir"
    fi

    if ((status == 0 && (cleanup_failed || restore_failed))); then
        status=1
    fi
    if ((status == 0)); then
        echo "Phase 0B browser smoke completed as a NON-REPRESENTATIVE rehearsal."
        echo "Result is gate_eligible=false and is not a Phase 0B PASS."
    fi
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

fixture_restore_needed=1
env -u NO_COLOR cargo run --locked \
    --package spinal-app \
    --example prepare_web_fixture -- \
    --phase0b apps/spinal/web/bundle

source_target="$repo_root/apps/spinal/web/index.html"
feature_token='data-cargo-features="web"'
phase0b_feature_token='data-cargo-features="phase0b-rehearsal"'
if [[ "$(count_literal "$feature_token" "$source_target")" != "1" ]]; then
    echo "Phase 0B browser smoke expected exactly one web feature token in index.html" >&2
    exit 1
fi
phase0b_target="$(mktemp "$repo_root/apps/spinal/web/.spinal-phase0b-rehearsal.XXXXXX")"
chmod 600 "$phase0b_target"
sed 's/data-cargo-features="web"/data-cargo-features="phase0b-rehearsal"/' \
    "$source_target" >"$phase0b_target"
if [[ "$(count_literal "$feature_token" "$phase0b_target")" != "0" ]] \
    || [[ "$(count_literal "$phase0b_feature_token" "$phase0b_target")" != "1" ]]; then
    echo "Phase 0B browser smoke could not derive its private Trunk target" >&2
    exit 1
fi

env -u NO_COLOR trunk build --release --locked \
    --no-default-features \
    --features phase0b-rehearsal \
    --config apps/spinal/web/Trunk.toml \
    --dist "$smoke_dir/dist" \
    "$phase0b_target"
served_root="$(cd -- "$smoke_dir/dist" && pwd -P)"
if [[ "$served_root" != "$smoke_dir/dist" ]]; then
    echo "Phase 0B browser smoke dist directory is not canonical" >&2
    exit 1
fi

base_url="http://127.0.0.1:${port}/"
python3 -m http.server "$port" \
    --bind 127.0.0.1 \
    --directory "$smoke_dir/dist" >"$smoke_dir/server.log" 2>&1 &
server_pid="$!"

server_ready=0
for ((attempt = 0; attempt < 100; attempt += 1)); do
    if ! kill -0 "$server_pid" 2>/dev/null; then
        break
    fi
    if curl --noproxy '*' --max-time 1 -fsS "$base_url" >/dev/null 2>&1; then
        server_ready=1
        break
    fi
    sleep 0.1
done
if [[ "$server_ready" != "1" ]] || ! kill -0 "$server_pid" 2>/dev/null; then
    cat "$smoke_dir/server.log" >&2
    echo "Phase 0B browser smoke server did not start on 127.0.0.1:${port}" >&2
    exit 1
fi

profile_dir="$smoke_dir/chrome-profile"
mkdir -m 700 "$profile_dir"
chrome_log="$smoke_dir/chrome.log"
"$chrome" \
    --headless=new \
    --no-first-run \
    --no-default-browser-check \
    --disable-background-networking \
    --disable-component-update \
    --disable-default-apps \
    --disable-extensions \
    --disable-sync \
    --hide-scrollbars \
    --proxy-server=direct:// \
    --proxy-bypass-list='*' \
    --remote-debugging-address=127.0.0.1 \
    --remote-debugging-port=0 \
    --user-data-dir="$profile_dir" \
    --window-size=640,480 \
    --force-device-scale-factor=1 \
    --use-gl=angle \
    --use-angle=swiftshader \
    --enable-unsafe-swiftshader \
    "$base_url" >"$chrome_log" 2>&1 &
chrome_pid="$!"

debugging_port_file="$profile_dir/DevToolsActivePort"
debugging_port=""
for ((attempt = 0; attempt < 200; attempt += 1)); do
    if [[ -s "$debugging_port_file" ]]; then
        IFS= read -r debugging_port <"$debugging_port_file" || true
        break
    fi
    if ! kill -0 "$chrome_pid" 2>/dev/null; then
        break
    fi
    sleep 0.05
done
if [[ ! "$debugging_port" =~ ^[1-9][0-9]{0,4}$ ]] \
    || ((debugging_port < 1 || debugging_port > 65535)) \
    || ! kill -0 "$chrome_pid" 2>/dev/null; then
    cat "$chrome_log" >&2
    echo "Phase 0B browser smoke Chrome debugging endpoint did not start" >&2
    exit 1
fi

capture_dir="$smoke_dir/capture"
if [[ -e "$capture_dir" ]]; then
    echo "Phase 0B browser smoke capture directory unexpectedly exists" >&2
    exit 1
fi
if ! node tools/spinal-phase0b-cdp.js \
    "$debugging_port" \
    "$base_url" \
    "$capture_dir" \
    "$served_root" \
    "$nonce" >"$smoke_dir/driver.stdout.log" 2>"$smoke_dir/driver.stderr.log"; then
    cat "$chrome_log" >&2
    cat "$smoke_dir/driver.stderr.log" >&2
    echo "Phase 0B browser smoke capture failed" >&2
    exit 1
fi

terminal_file="$capture_dir/phase0b-browser-terminal.json"
event_window_prefix='"format_version":1,"window_id":"sway-events","animation":"sway","start_ns":0,"end_ns":1000000000,"events":['
current_event_window_prefix=",\"event_windows\":{\"current\":{${event_window_prefix}"
proposed_event_window_prefix=",\"proposed\":{${event_window_prefix}"
if [[ ! -s "$terminal_file" ]] \
    || [[ "$(count_literal '{"format_version":3,"state":"complete","browser_capture":' "$terminal_file")" != "1" ]] \
    || [[ "$(count_literal "$current_event_window_prefix" "$terminal_file")" != "1" ]] \
    || [[ "$(count_literal "$proposed_event_window_prefix" "$terminal_file")" != "1" ]] \
    || [[ "$(count_literal "$event_window_prefix" "$terminal_file")" != "2" ]]; then
    echo "Phase 0B browser smoke did not produce the exact outer-v3 event-window envelope" >&2
    exit 1
fi
for expected_event in \
    '"animation":"sway","name":"start","local_time_ns":0,"loop_index":0,"integer":10,"float":0.0,"string":null,"volume":1.0,"balance":0.0,"diagnostic_codes":[]' \
    '"animation":"sway","name":"middle","local_time_ns":500000000,"loop_index":0,"integer":11,"float":1.25,"string":"middle","volume":1.0,"balance":0.0,"diagnostic_codes":[]' \
    '"animation":"sway","name":"end","local_time_ns":1000000000,"loop_index":0,"integer":12,"float":0.0,"string":null,"volume":0.5,"balance":-0.25,"diagnostic_codes":[]' \
    '"animation":"sway","name":"start","local_time_ns":0,"loop_index":0,"integer":20,"float":0.0,"string":null,"volume":1.0,"balance":0.0,"diagnostic_codes":[]' \
    '"animation":"sway","name":"middle","local_time_ns":500000000,"loop_index":0,"integer":21,"float":1.25,"string":"middle","volume":1.0,"balance":0.0,"diagnostic_codes":[]' \
    '"animation":"sway","name":"end","local_time_ns":1000000000,"loop_index":0,"integer":22,"float":0.0,"string":null,"volume":0.5,"balance":-0.25,"diagnostic_codes":[]'; do
    if [[ "$(count_literal "$expected_event" "$terminal_file")" != "1" ]]; then
        echo "Phase 0B browser smoke did not produce the fixed event fixture vectors" >&2
        exit 1
    fi
done
if [[ "$(count_literal '"diagnostic_codes":[]' "$terminal_file")" != "6" ]]; then
    echo "Phase 0B browser smoke event fixtures contain diagnostics" >&2
    exit 1
fi

if [[ ! -s "$capture_dir/phase0b-browser-capture-manifest.json" ]] \
    || ! grep -Fq '"evidence_class":"non_representative_rehearsal","gate_eligible":false' \
        "$capture_dir/phase0b-browser-capture-manifest.json"; then
    echo "Phase 0B browser smoke did not produce a gate-ineligible rehearsal manifest" >&2
    exit 1
fi

provenance_file="$capture_dir/phase0b-browser-provenance-receipt.json"
if [[ ! -s "$provenance_file" ]]; then
    echo "Phase 0B browser smoke did not produce its final provenance receipt" >&2
    exit 1
fi
if ! python3 - \
    "$repo_root" \
    "$smoke_dir" \
    "$capture_dir" \
    "$served_root" \
    "$nonce" <<'PY'
import hashlib
import json
import os
import pathlib
import re
import stat
import subprocess
import sys


def fail(message):
    raise SystemExit(f"Phase 0B browser provenance receipt check failed: {message}")


def reject_constant(value):
    fail(f"non-JSON numeric constant {value}")


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            fail(f"duplicate JSON key {key!r}")
        result[key] = value
    return result


def load_canonical(path, maximum, label):
    try:
        raw = path.read_bytes()
    except OSError as error:
        fail(f"cannot read {label}: {error}")
    if not raw or len(raw) > maximum or raw.endswith(b"\n"):
        fail(f"{label} is empty, oversized, or not compact")
    try:
        text = raw.decode("utf-8")
        value = json.loads(
            text,
            object_pairs_hook=unique_object,
            parse_constant=reject_constant,
        )
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        fail(f"{label} is not strict UTF-8 JSON: {error}")
    canonical = json.dumps(value, ensure_ascii=False, separators=(",", ":"))
    if canonical.encode("utf-8") != raw:
        fail(f"{label} is not canonical compact JSON")
    return raw, value


def exact_keys(value, expected, label):
    if type(value) is not dict or list(value) != expected:
        fail(f"{label} has non-canonical fields or field order")


def hex256(value, label):
    if type(value) is not str or re.fullmatch(r"[0-9a-f]{64}", value) is None:
        fail(f"{label} is not a lowercase SHA-256 value")


def nonempty(value, label):
    if type(value) is not str or not value:
        fail(f"{label} is not a non-empty string")


def sha256_bytes(value):
    return hashlib.sha256(value).hexdigest()


def byte_identity(path):
    raw = path.read_bytes()
    return {"byte_length": len(raw), "sha256": sha256_bytes(raw)}


def file_descriptor(path, name):
    identity = byte_identity(path)
    return {"file": name, **identity}


repo_root = pathlib.Path(sys.argv[1])
smoke_root = pathlib.Path(sys.argv[2])
capture_root = pathlib.Path(sys.argv[3])
served_root = pathlib.Path(sys.argv[4])
nonce = sys.argv[5]
command_env = {
    key: value
    for key, value in os.environ.items()
    if not key.upper().startswith("GIT_")
}
command_env.update({
    "LC_ALL": "C",
    "LANG": "C",
    "NO_COLOR": "1",
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_CONFIG_GLOBAL": os.devnull,
    "GIT_OPTIONAL_LOCKS": "0",
    "GIT_TERMINAL_PROMPT": "0",
})
git_command = [
    "git",
    "--no-optional-locks",
    "-c", "core.fsmonitor=false",
    "-c", f"core.excludesFile={os.devnull}",
]
manifest_path = capture_root / "phase0b-browser-capture-manifest.json"
terminal_path = capture_root / "phase0b-browser-terminal.json"
receipt_path = capture_root / "phase0b-browser-provenance-receipt.json"

hex256(nonce, "runner nonce")
if capture_root.is_symlink() or not capture_root.is_dir():
    fail("capture root is not a real directory")
if stat.S_IMODE(capture_root.stat().st_mode) != 0o700:
    fail("capture root mode is not 0700")

schedule = [
    "00-sway-start-current.png",
    "01-sway-start-proposed.png",
    "02-sway-middle-current.png",
    "03-sway-middle-proposed.png",
    "04-sway-alternate-skin-current.png",
    "05-sway-alternate-skin-proposed.png",
    "06-sway-end-current.png",
    "07-sway-end-proposed.png",
]
expected_outputs = set(schedule) | {
    terminal_path.name,
    manifest_path.name,
    receipt_path.name,
}
actual_outputs = set()
for child in capture_root.iterdir():
    child_stat = child.lstat()
    if stat.S_ISLNK(child_stat.st_mode) or not stat.S_ISREG(child_stat.st_mode):
        fail(f"capture output {child.name!r} is not a regular non-link file")
    if stat.S_IMODE(child_stat.st_mode) != 0o600:
        fail(f"capture output {child.name!r} mode is not 0600")
    actual_outputs.add(child.name)
if actual_outputs != expected_outputs:
    fail("capture output set is not the exact successful eleven-file layout")

manifest_raw, manifest = load_canonical(manifest_path, 64 * 1024, "capture manifest")
terminal_raw, terminal = load_canonical(terminal_path, 9 * 1024 * 1024, "terminal")
receipt_raw, receipt = load_canonical(receipt_path, 64 * 1024, "provenance receipt")
if receipt_path.stat().st_mtime_ns < max(
    child.stat().st_mtime_ns for child in capture_root.iterdir() if child != receipt_path
):
    fail("provenance receipt was not written after the capture artifacts")

exact_keys(manifest, [
    "format_version", "artifact_kind", "evidence_class", "gate_eligible",
    "nonce", "terminal", "screenshots",
], "capture manifest")
if manifest["format_version"] != 1 \
        or manifest["artifact_kind"] != "phase0b_browser_capture" \
        or manifest["evidence_class"] != "non_representative_rehearsal" \
        or manifest["gate_eligible"] is not False \
        or manifest["nonce"] != nonce:
    fail("capture manifest has the wrong fixed classification or nonce")
exact_keys(manifest["terminal"], ["file", "byte_length", "sha256"], "manifest terminal")
if manifest["terminal"] != file_descriptor(terminal_path, terminal_path.name):
    fail("capture manifest does not bind the exact terminal bytes")
if type(manifest["screenshots"]) is not list or len(manifest["screenshots"]) != 8:
    fail("capture manifest does not contain eight screenshot descriptors")

exact_keys(terminal, [
    "format_version", "state", "browser_capture", "event_windows", "observations",
], "outer terminal")
browser_capture = terminal["browser_capture"]
exact_keys(browser_capture, [
    "format_version", "state", "nonce", "runtime_sources", "screenshots",
], "terminal browser capture")
if terminal["format_version"] != 3 or terminal["state"] != "complete" \
        or browser_capture["format_version"] != 1 \
        or browser_capture["state"] != "complete" \
        or browser_capture["nonce"] != nonce:
    fail("terminal has the wrong schema, state, or nonce")

def check_identity(value, label):
    exact_keys(value, ["manifest_sha256", "content_sha256"], label)
    hex256(value["manifest_sha256"], f"{label}.manifest_sha256")
    hex256(value["content_sha256"], f"{label}.content_sha256")


runtime_sources = browser_capture["runtime_sources"]
exact_keys(runtime_sources, ["current", "proposed"], "terminal runtime sources")
check_identity(runtime_sources["current"], "terminal current runtime")
check_identity(runtime_sources["proposed"], "terminal proposed runtime")
if type(browser_capture["screenshots"]) is not list or len(browser_capture["screenshots"]) != 8:
    fail("terminal browser capture does not contain eight screenshots")

for sequence, expected_file in enumerate(schedule):
    screenshot = manifest["screenshots"][sequence]
    exact_keys(screenshot, ["sequence", "file", "byte_length", "sha256"], f"manifest screenshot {sequence}")
    screenshot_path = capture_root / expected_file
    if screenshot["sequence"] != sequence or screenshot["file"] != expected_file \
            or screenshot != {"sequence": sequence, **file_descriptor(screenshot_path, expected_file)}:
        fail(f"manifest screenshot {sequence} does not bind the exact PNG bytes")
    terminal_screenshot = browser_capture["screenshots"][sequence]
    if terminal_screenshot["sequence"] != sequence \
            or terminal_screenshot["png_byte_length"] != screenshot["byte_length"] \
            or terminal_screenshot["png_sha256"] != screenshot["sha256"]:
        fail(f"terminal screenshot {sequence} does not match the manifest")

exact_keys(receipt, [
    "format_version", "artifact_kind", "evidence_class", "gate_eligible",
    "relationship", "binding", "build", "browser", "graphics",
], "provenance receipt")
if receipt["format_version"] != 1 \
        or receipt["artifact_kind"] != "phase0b_browser_provenance_receipt" \
        or receipt["evidence_class"] != "non_representative_rehearsal" \
        or receipt["gate_eligible"] is not False \
        or receipt["relationship"] != "self_reported_context_not_binary_attestation":
    fail("provenance receipt has the wrong fixed classification")

binding = receipt["binding"]
exact_keys(binding, ["nonce", "runtime_sources", "capture_manifest", "terminal"], "receipt binding")
if binding["nonce"] != nonce or binding["runtime_sources"] != runtime_sources:
    fail("receipt does not bind the runner nonce and terminal runtime identities")
expected_manifest_descriptor = {
    "file": manifest_path.name,
    "byte_length": len(manifest_raw),
    "sha256": sha256_bytes(manifest_raw),
}
expected_terminal_descriptor = {
    "file": terminal_path.name,
    "byte_length": len(terminal_raw),
    "sha256": sha256_bytes(terminal_raw),
}
for value, expected, label in [
    (binding["capture_manifest"], expected_manifest_descriptor, "receipt capture manifest"),
    (binding["terminal"], expected_terminal_descriptor, "receipt terminal"),
]:
    exact_keys(value, ["file", "byte_length", "sha256"], label)
    if value != expected:
        fail(f"{label} does not bind the exact captured bytes")
if binding["terminal"] != manifest["terminal"]:
    fail("receipt and capture manifest disagree about the terminal")

forbidden_keys = {
    "user_agent", "userAgent", "commandLine", "command_line", "modelName", "model_name",
}


def walk_privacy(value):
    if type(value) is dict:
        for key, item in value.items():
            if key in forbidden_keys:
                fail(f"receipt exposes forbidden raw context field {key!r}")
            walk_privacy(item)
    elif type(value) is list:
        for item in value:
            walk_privacy(item)
    elif type(value) is str:
        if value.startswith("/") or re.match(r"^[A-Za-z]:[\\/]", value):
            fail("receipt exposes an absolute filesystem path")


walk_privacy(receipt)
receipt_text = receipt_raw.decode("utf-8")
for private_path in [repo_root, smoke_root, capture_root, served_root, smoke_root / "chrome-profile"]:
    if str(private_path) in receipt_text:
        fail("receipt exposes repository, temporary, capture, served, or profile path")

build = receipt["build"]
exact_keys(build, [
    "checkout", "cargo_lock", "trunk_config", "driver", "driver_host",
    "toolchain", "invocation", "served_files",
], "receipt build")
checkout = build["checkout"]
exact_keys(checkout, ["head", "dirty", "status_sha256"], "receipt checkout")
top_level = subprocess.check_output(
    [*git_command, "rev-parse", "--show-toplevel"],
    cwd=repo_root,
    env=command_env,
    stderr=subprocess.DEVNULL,
).decode("utf-8").strip()
try:
    resolved_top_level = pathlib.Path(top_level).resolve(strict=True)
except OSError as error:
    fail(f"Git top-level path cannot be resolved: {error}")
if top_level != str(repo_root) or resolved_top_level != repo_root:
    fail("Git commands are not bound to the canonical repository root")
head = subprocess.check_output(
    [*git_command, "rev-parse", "--verify", "HEAD"],
    cwd=repo_root,
    env=command_env,
    stderr=subprocess.DEVNULL,
).decode("ascii").strip()
status_bytes = subprocess.check_output(
    [
        *git_command,
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=all",
        "--ignore-submodules=none",
    ],
    cwd=repo_root,
    env=command_env,
    stderr=subprocess.DEVNULL,
)
if checkout != {
    "head": head,
    "dirty": bool(status_bytes),
    "status_sha256": sha256_bytes(status_bytes),
}:
    fail("receipt checkout does not match the independently observed checkout")

for actual, path, label in [
    (build["cargo_lock"], repo_root / "Cargo.lock", "Cargo.lock"),
    (build["trunk_config"], repo_root / "apps/spinal/web/Trunk.toml", "Trunk.toml"),
    (build["driver"], repo_root / "tools/spinal-phase0b-cdp.js", "capture driver"),
]:
    exact_keys(actual, ["byte_length", "sha256"], f"receipt {label}")
    if actual != byte_identity(path):
        fail(f"receipt {label} identity does not match the exact file")

driver_host = build["driver_host"]
exact_keys(driver_host, ["platform", "architecture", "node_version"], "receipt driver host")
host_json = subprocess.check_output([
    "node", "-p",
    "JSON.stringify({platform:process.platform,architecture:process.arch,node_version:process.versions.node})",
], cwd=repo_root, env=command_env).decode("utf-8")
if driver_host != json.loads(host_json):
    fail("receipt driver host does not match the running Node.js host")

toolchain = build["toolchain"]
exact_keys(toolchain, [
    "rustc_release", "rustc_commit_hash", "rustc_host", "cargo_version",
    "trunk_version", "bevy_version",
], "receipt toolchain")
rustc_fields = {}
for line in subprocess.check_output(
    ["rustc", "-vV"], cwd=repo_root, env=command_env
).decode("utf-8").splitlines():
    if ": " in line:
        key, value = line.split(": ", 1)
        rustc_fields[key] = value


def command_version(command, tool):
    output = subprocess.check_output(
        command, cwd=repo_root, env=command_env
    ).decode("utf-8").strip()
    match = re.match(rf"^{re.escape(tool)} ([0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?)", output)
    if match is None:
        fail(f"cannot independently parse {tool} version")
    return match.group(1)


expected_toolchain = {
    "rustc_release": rustc_fields.get("release"),
    "rustc_commit_hash": (
        None if rustc_fields.get("commit-hash") == "unknown"
        else rustc_fields.get("commit-hash")
    ),
    "rustc_host": rustc_fields.get("host"),
    "cargo_version": command_version(["cargo", "--version"], "cargo"),
    "trunk_version": command_version(["trunk", "--version"], "trunk"),
    "bevy_version": "0.19.0",
}
if toolchain != expected_toolchain:
    fail("receipt toolchain does not match the independently observed toolchain")

invocation = build["invocation"]
exact_keys(invocation, ["trunk_release", "target", "features"], "receipt invocation")
if invocation != {
    "trunk_release": True,
    "target": "wasm32-unknown-unknown",
    "features": ["phase0b-rehearsal"],
}:
    fail("receipt invocation is not the fixed Phase 0B rehearsal build")

served_descriptors = []
for path in served_root.rglob("*"):
    path_stat = path.lstat()
    if stat.S_ISLNK(path_stat.st_mode):
        fail("served build contains a symbolic link")
    if stat.S_ISDIR(path_stat.st_mode):
        continue
    if not stat.S_ISREG(path_stat.st_mode):
        fail("served build contains a non-regular file")
    relative = path.relative_to(served_root).as_posix()
    served_descriptors.append({"path": relative, **byte_identity(path)})
served_descriptors.sort(key=lambda item: item["path"].encode("utf-8"))
if not served_descriptors or build["served_files"] != served_descriptors:
    fail("receipt served-file inventory does not match the exact build tree")
for index, descriptor in enumerate(build["served_files"]):
    exact_keys(descriptor, ["path", "byte_length", "sha256"], f"served file {index}")

browser = receipt["browser"]
exact_keys(browser, [
    "protocol_version", "product", "revision", "js_version", "requested_launch",
], "receipt browser")
for field in ["protocol_version", "product", "revision", "js_version"]:
    nonempty(browser[field], f"receipt browser.{field}")
requested_launch = browser["requested_launch"]
exact_keys(requested_launch, [
    "headless", "gl", "angle_backend", "width_px", "height_px",
    "device_scale_factor",
], "receipt requested launch")
if requested_launch != {
    "headless": "new",
    "gl": "angle",
    "angle_backend": "swiftshader",
    "width_px": 640,
    "height_px": 480,
    "device_scale_factor": 1,
}:
    fail("receipt does not record the fixed requested browser launch")

graphics = receipt["graphics"]
exact_keys(graphics, [
    "system_devices", "feature_status", "driver_bug_workarounds", "effective_context",
], "receipt graphics")
devices = graphics["system_devices"]
if type(devices) is not list or not 1 <= len(devices) <= 8:
    fail("receipt does not retain the bounded SystemInfo device observations")
for index, device in enumerate(devices):
    exact_keys(device, [
        "vendor_id", "device_id", "vendor_string", "device_string",
        "driver_vendor", "driver_version",
    ], f"system device {index}")
    for field in ["vendor_id", "device_id"]:
        if type(device[field]) is not int or not 0 <= device[field] <= 0xFFFFFFFF:
            fail(f"system device {index}.{field} is invalid")
    for field in ["vendor_string", "device_string", "driver_vendor", "driver_version"]:
        if type(device[field]) is not str:
            fail(f"system device {index}.{field} is not a string")

features = graphics["feature_status"]
if type(features) is not list or len(features) > 128:
    fail("receipt SystemInfo feature-status observations are invalid")
feature_names = []
for index, feature in enumerate(features):
    exact_keys(feature, ["name", "status"], f"feature status {index}")
    nonempty(feature["name"], f"feature status {index}.name")
    if type(feature["status"]) is not str:
        fail(f"feature status {index}.status is not a string")
    feature_names.append(feature["name"])
if feature_names != sorted(set(feature_names), key=lambda item: item.encode("utf-8")):
    fail("receipt feature-status entries are not sorted and unique")

workarounds = graphics["driver_bug_workarounds"]
if type(workarounds) is not list or len(workarounds) > 128:
    fail("receipt SystemInfo workaround observations are invalid")
for index, workaround in enumerate(workarounds):
    nonempty(workaround, f"driver workaround {index}")
if workarounds != sorted(set(workarounds), key=lambda item: item.encode("utf-8")):
    fail("receipt driver workarounds are not sorted and unique")

effective_context = graphics["effective_context"]
exact_keys(effective_context, [
    "api", "drawing_buffer_width", "drawing_buffer_height", "vendor", "renderer",
    "version", "shading_language_version", "unmasked_vendor", "unmasked_renderer",
], "receipt effective context")
if effective_context["api"] != "webgl2" \
        or effective_context["drawing_buffer_width"] != 640 \
        or effective_context["drawing_buffer_height"] != 480:
    fail("receipt effective context is not the required 640-by-480 WebGL2 context")
for field in [
    "vendor", "renderer", "version", "shading_language_version",
    "unmasked_vendor", "unmasked_renderer",
]:
    nonempty(effective_context[field], f"receipt effective context.{field}")
PY
then
    echo "Phase 0B browser smoke did not produce a valid gate-ineligible provenance receipt" >&2
    exit 1
fi

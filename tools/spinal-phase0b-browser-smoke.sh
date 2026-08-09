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

for required in cargo trunk node curl python3; do
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
    "$capture_dir" >"$smoke_dir/driver.stdout.log" 2>"$smoke_dir/driver.stderr.log"; then
    cat "$chrome_log" >&2
    cat "$smoke_dir/driver.stderr.log" >&2
    echo "Phase 0B browser smoke capture failed" >&2
    exit 1
fi

if [[ ! -s "$capture_dir/phase0b-browser-capture-manifest.json" ]] \
    || ! grep -Fq '"evidence_class":"non_representative_rehearsal","gate_eligible":false' \
        "$capture_dir/phase0b-browser-capture-manifest.json"; then
    echo "Phase 0B browser smoke did not produce a gate-ineligible rehearsal manifest" >&2
    exit 1
fi

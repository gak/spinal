#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

port="${1:-8425}"
if [[ ! "$port" =~ ^[0-9]+$ ]] || ((port < 1024 || port > 65535)); then
    echo "usage: tools/web-smoke.sh [port from 1024 through 65535]" >&2
    exit 2
fi

chrome="${CHROME_BIN:-}"
if [[ -z "$chrome" && -x "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" ]]; then
    chrome="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
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
    echo "web smoke requires Chrome/Chromium or CHROME_BIN" >&2
    exit 1
fi
if ! command -v node >/dev/null 2>&1; then
    echo "web smoke requires Node.js for bounded Chrome interaction checks" >&2
    exit 1
fi

if command -v magick >/dev/null 2>&1; then
    image_command=(magick)
elif command -v convert >/dev/null 2>&1; then
    image_command=(convert)
else
    echo "web smoke requires ImageMagick" >&2
    exit 1
fi

chrome_common_args=(
    --headless=new
    --no-first-run
    --no-default-browser-check
    --hide-scrollbars
    --disable-background-networking
    --disable-component-update
    --disable-sync
    --use-gl=angle
    --use-angle=swiftshader
    --enable-unsafe-swiftshader
)

smoke_dir="$(mktemp -d "${TMPDIR:-/tmp}/spinal-web-smoke.XXXXXX")"
server_pid=""
chrome_pid=""

stop_pid() {
    local pid="$1"
    [[ -n "$pid" ]] || return 0
    if kill -0 "$pid" 2>/dev/null; then
        kill "$pid" 2>/dev/null || true
        for _attempt in $(seq 1 20); do
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
    local status="$1"
    stop_pid "$chrome_pid"
    stop_pid "$server_pid"
    if [[ "$status" -ne 0 && "${SPINAL_KEEP_FAILED_SMOKE:-0}" == "1" ]]; then
        echo "web smoke retained failed diagnostics at $smoke_dir" >&2
    else
        rm -rf -- "$smoke_dir"
    fi
}
trap 'cleanup "$?"' EXIT

cargo run --locked --package spinal-app --example prepare_web_fixture -- \
    apps/spinal/web/bundle
env -u NO_COLOR trunk build --release --locked \
    --config apps/spinal/web/Trunk.toml \
    --dist "$smoke_dir/dist"

open_missing_dir="$smoke_dir/open-missing-page"
open_complete_dir="$smoke_dir/open-complete"
mkdir -m 700 "$open_missing_dir" "$open_complete_dir"
for fixture_name in viewer.spine.json viewer.atlas; do
    fixture_path="$repo_root/apps/spinal/web/bundle/$fixture_name"
    if [[ ! -s "$fixture_path" ]]; then
        echo "web smoke Open fixture is missing $fixture_name" >&2
        exit 1
    fi
    install -m 600 "$fixture_path" "$open_missing_dir/$fixture_name"
    install -m 600 "$fixture_path" "$open_complete_dir/$fixture_name"
done
fixture_page="$repo_root/apps/spinal/web/bundle/viewer.png"
if [[ ! -s "$fixture_page" ]]; then
    echo "web smoke Open fixture is missing viewer.png" >&2
    exit 1
fi
install -m 600 "$fixture_page" "$open_complete_dir/viewer.png"

default_index="$smoke_dir/dist/index.html"
if [[ "$(grep -Foc 'id="spinal-app"' "$default_index")" -ne 1 ]]; then
    echo "web smoke requires exactly one Spinal app root in the built index" >&2
    exit 1
fi
if grep -Fq 'data-spinal-manifest=' "$default_index"; then
    echo "web smoke default page must require an explicit Open selection" >&2
    exit 1
fi

inject_manifest_page() {
    local manifest_path="$1"
    local output_path="$2"
    sed \
        "s#id=\"spinal-app\"#id=\"spinal-app\" data-spinal-manifest=\"${manifest_path}\"#" \
        "$default_index" >"$output_path"
    if [[ "$(grep -Foc "data-spinal-manifest=\"${manifest_path}\"" "$output_path")" -ne 1 ]]; then
        echo "web smoke could not create an explicit manifest launch page" >&2
        exit 1
    fi
}

inject_manifest_page "bundle/manifest.json" "$smoke_dir/dist/compare.html"
inject_manifest_page "bundle/preview.manifest.json" "$smoke_dir/dist/preview.html"

base_url="http://127.0.0.1:${port}/dist/"
compare_url="${base_url}compare.html"
preview_url="${base_url}preview.html"
python3 -m http.server "$port" --bind 127.0.0.1 \
    --directory "$smoke_dir" >"$smoke_dir/server.log" 2>&1 &
server_pid="$!"

for _attempt in $(seq 1 50); do
    if ! kill -0 "$server_pid" 2>/dev/null; then
        cat "$smoke_dir/server.log" >&2
        exit 1
    fi
    if curl --max-time 1 -fsS "$base_url" >/dev/null 2>&1; then
        break
    fi
    sleep 0.1
done
curl --max-time 1 -fsS "$base_url" >/dev/null || {
    cat "$smoke_dir/server.log" >&2
    exit 1
}

run_cdp_page() {
    local page_url="$1"
    local run_name="$2"
    local window_size="$3"
    local mode="$4"
    local mode_arguments=("${@:5}")
    local user_data="$smoke_dir/${run_name}-chrome"
    local chrome_log="$smoke_dir/${run_name}-chrome.log"
    local cdp_log="$smoke_dir/${run_name}-cdp.log"

    "$chrome" \
        "${chrome_common_args[@]}" \
        --remote-debugging-port=0 \
        --user-data-dir="$user_data" \
        --window-size="$window_size" \
        "$page_url" >"$chrome_log" 2>&1 &
    chrome_pid="$!"

    for _attempt in $(seq 1 200); do
        if [[ -s "$user_data/DevToolsActivePort" ]]; then
            break
        fi
        if ! kill -0 "$chrome_pid" 2>/dev/null; then
            cat "$chrome_log" >&2
            echo "web smoke Chrome interaction host stopped early: $run_name" >&2
            exit 1
        fi
        sleep 0.05
    done
    if [[ ! -s "$user_data/DevToolsActivePort" ]]; then
        cat "$chrome_log" >&2
        echo "web smoke Chrome debugging endpoint did not start: $run_name" >&2
        exit 1
    fi
    local debugging_port
    debugging_port="$(sed -n '1p' "$user_data/DevToolsActivePort")"
    if [[ ! "$debugging_port" =~ ^[0-9]+$ ]]; then
        cat "$chrome_log" >&2
        echo "web smoke Chrome debugging port is invalid: $run_name" >&2
        exit 1
    fi
    if ! node tools/web-smoke-cdp.js \
        "$debugging_port" "$mode" "${mode_arguments[@]}" >"$cdp_log" 2>&1; then
        cat "$chrome_log" >&2
        cat "$cdp_log" >&2
        echo "web smoke Chrome interaction failed: $run_name" >&2
        exit 1
    fi
    cat "$cdp_log"
    stop_pid "$chrome_pid"
    chrome_pid=""
}

run_cdp_page \
    "$base_url" \
    "open-retry" \
    "640,720" \
    "open" \
    "$open_missing_dir" \
    "$open_complete_dir" \
    "viewer.png"

run_cdp_page \
    "$compare_url" \
    "compare" \
    "640,480" \
    "capture" \
    "compare" \
    "$smoke_dir/compare.png" \
    "$smoke_dir/compare.html"

compare_html="$smoke_dir/compare.html"
compare_png="$smoke_dir/compare.png"
for expected in \
    '<title>Spinal — Compare</title>' \
    'id="preview-heading" class="visually-hidden">Animation comparison</h1>' \
    'aria-label="Comparison views"' \
    'aria-label="Spinal comparison viewport. Primary is left; Comparison is right."' \
    'id="spinal-primary-label">Primary</h2>' \
    'id="spinal-comparison-label">Comparison — setup pose</h2>' \
    'id="spinal-diagnostics-summary">Diagnostics — 2 sources compatible</summary>' \
    'id="spinal-primary-diagnostics-heading">Primary</h2>' \
    'id="spinal-comparison-diagnostics-heading">Comparison</h2>' \
    'No source compatibility findings.</li>' \
    'data-spinal-camera-synchronized="true"' \
    'data-spinal-camera-zoom="100"' \
    'data-spinal-camera-panned="false"' \
    'data-spinal-base-fit-synchronized="true"' \
    'data-spinal-base-fit-scale="' \
    'data-spinal-base-fit-center="' \
    'id="spinal-camera-state" class="camera-state">Linked view · 100% zoom</output>' \
    'Comparison does not contain animation “sway”; showing setup pose in that pane.' \
    'id="spinal-app" data-spinal-manifest="bundle/manifest.json" data-spinal-mode="compare"' \
    'id="spinal-status" role="status" aria-live="polite" aria-atomic="true" data-state="ready"'; do
    if ! grep -Fq "$expected" "$compare_html"; then
        cat "$smoke_dir/compare-chrome.log" >&2
        grep -o 'spinal-\(primary\|comparison\)-label[^<]*</h2>' "$compare_html" >&2 || true
        echo "web smoke expected live comparison marker: $expected" >&2
        exit 1
    fi
done

read -r image_width image_height < <(
    "${image_command[@]}" identify -format '%w %h\n' "$compare_png"
)
if ((image_width < 2 || image_height < 1)); then
    echo "web smoke captured invalid ${image_width}x${image_height} dimensions" >&2
    exit 1
fi
left_width="$((image_width / 2))"
right_width="$((image_width - left_width))"

primary_red="$("${image_command[@]}" "$compare_png" \
    -crop "${left_width}x${image_height}+0+0" +repage \
    -fx '((r>0.8)&&(g<0.1)&&(b<0.1))?1:0' \
    -format '%[fx:mean>0.001?1:0]' info:)"
primary_blue="$("${image_command[@]}" "$compare_png" \
    -crop "${left_width}x${image_height}+0+0" +repage \
    -fx '((b>0.8)&&(r<0.1)&&(g<0.1))?1:0' \
    -format '%[fx:mean>0.001?1:0]' info:)"
comparison_blue="$("${image_command[@]}" "$compare_png" \
    -crop "${right_width}x${image_height}+${left_width}+0" +repage \
    -fx '((b>0.8)&&(r<0.1)&&(g<0.1))?1:0' \
    -format '%[fx:mean>0.001?1:0]' info:)"
comparison_red="$("${image_command[@]}" "$compare_png" \
    -crop "${right_width}x${image_height}+${left_width}+0" +repage \
    -fx '((r>0.8)&&(g<0.1)&&(b<0.1))?1:0' \
    -format '%[fx:mean>0.001?1:0]' info:)"
if [[ "$primary_red" != "1" ]]; then
    echo "web smoke expected the fitted primary red attachment in the left half" >&2
    exit 1
fi
if [[ "$comparison_blue" != "1" ]]; then
    echo "web smoke expected the fitted comparison blue attachment in the right half" >&2
    exit 1
fi
if [[ "$primary_blue" != "0" || "$comparison_red" != "0" ]]; then
    echo "web smoke detected cross-pane Primary/Comparison rendering contamination" >&2
    exit 1
fi

run_cdp_page \
    "$compare_url" \
    "camera-refit" \
    "640,480" \
    "camera" \
    "tools/web-smoke-camera.js"

run_cdp_page \
    "$compare_url" \
    "accessibility-narrow" \
    "500,900" \
    "accessibility" \
    "tools/web-smoke-accessibility.js"

run_cdp_page \
    "$preview_url" \
    "preview" \
    "640,480" \
    "capture" \
    "preview" \
    "$smoke_dir/preview.png" \
    "$smoke_dir/preview.html"

for expected in \
    '<title>Spinal — Preview</title>' \
    'id="preview-heading" class="visually-hidden">Animation preview</h1>' \
    'aria-label="Preview view"' \
    'aria-label="Spinal preview viewport."' \
    'id="spinal-primary-label">Preview</h2>' \
    'id="spinal-comparison-label" hidden="">Comparison</h2>' \
    'id="spinal-diagnostics-summary">Diagnostics — 1 source compatible</summary>' \
    'id="spinal-primary-diagnostics-heading">Preview</h2>' \
    'id="spinal-comparison-diagnostics" class="diagnostics-source" aria-labelledby="spinal-comparison-diagnostics-heading" hidden=""' \
    'No source compatibility findings.</li>' \
    'id="spinal-app" data-spinal-manifest="bundle/preview.manifest.json" data-spinal-mode="preview"' \
    'id="spinal-status" role="status" aria-live="polite" aria-atomic="true" data-state="ready"'; do
    if ! grep -Fq "$expected" "$smoke_dir/preview.html"; then
        cat "$smoke_dir/preview-chrome.log" >&2
        grep -o 'spinal-\(primary\|comparison\)-label[^<]*</h2>' \
            "$smoke_dir/preview.html" >&2 || true
        echo "web smoke expected live Preview marker: $expected" >&2
        exit 1
    fi
done

if grep -Fq 'setup pose in that pane' "$smoke_dir/preview.html"; then
    echo "web smoke found a one-sided-animation warning in single-source Preview" >&2
    exit 1
fi

preview_red="$("${image_command[@]}" "$smoke_dir/preview.png" \
    -fx '((r>0.8)&&(g<0.1)&&(b<0.1))?1:0' \
    -format '%[fx:mean>0.001?1:0]' info:)"
preview_blue="$("${image_command[@]}" "$smoke_dir/preview.png" \
    -fx '((b>0.8)&&(r<0.1)&&(g<0.1))?1:0' \
    -format '%[fx:mean>0.001?1:0]' info:)"
if [[ "$preview_red" != "1" || "$preview_blue" != "0" ]]; then
    echo "web smoke expected only the fitted red attachment in single-source Preview" >&2
    exit 1
fi

sed '/<\/body>/i\
<script>window.setTimeout(() => { const canvas = document.getElementById("spinal-canvas"); canvas?.dispatchEvent(new Event("webglcontextlost", { cancelable: true })); window.spinalSetShellStatus("ready", "Ready — stale runtime state"); }, 3500);<\/script>' \
    "$smoke_dir/dist/preview.html" >"$smoke_dir/dist/context-loss.html"
run_cdp_page \
    "${base_url}context-loss.html" \
    "context-loss" \
    "640,480" \
    "capture" \
    "context-loss" \
    "$smoke_dir/context-loss.png" \
    "$smoke_dir/context-loss.html"

for expected in \
    'data-spinal-graphics-blocked="true"' \
    'data-state="blocked"' \
    'Viewer blocked — browser graphics were lost' \
    'id="spinal-play-toggle" type="button" data-spinal-action="toggle-pause" aria-controls="spinal-canvas" aria-label="Play" disabled=""'; do
    if ! grep -Fq "$expected" "$smoke_dir/context-loss.html"; then
        cat "$smoke_dir/context-loss-chrome.log" >&2
        echo "web smoke expected sticky graphics-loss marker: $expected" >&2
        exit 1
    fi
done

echo "Spinal browser Open/Preview/Compare isolation smoke passed"

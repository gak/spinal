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

if command -v magick >/dev/null 2>&1; then
    image_command=(magick)
elif command -v convert >/dev/null 2>&1; then
    image_command=(convert)
else
    echo "web smoke requires ImageMagick" >&2
    exit 1
fi

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
    stop_pid "$chrome_pid"
    stop_pid "$server_pid"
    rm -rf -- "$smoke_dir"
}
trap cleanup EXIT

cargo run --locked --package spinal-viewer --example prepare_web_fixture -- \
    apps/spinal-viewer/web/bundle
env -u NO_COLOR trunk build --release --locked \
    --config apps/spinal-viewer/web/Trunk.toml \
    --dist "$smoke_dir/dist"

base_url="http://127.0.0.1:${port}/dist/"
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

"$chrome" \
    --headless=new \
    --no-first-run \
    --no-default-browser-check \
    --hide-scrollbars \
    --disable-background-networking \
    --disable-component-update \
    --disable-sync \
    --use-gl=angle \
    --use-angle=swiftshader \
    --enable-unsafe-swiftshader \
    --user-data-dir="$smoke_dir/chrome" \
    --window-size=640,480 \
    --virtual-time-budget=5000 \
    --run-all-compositor-stages-before-draw \
    --screenshot="$smoke_dir/presented.png" \
    "$base_url" >"$smoke_dir/chrome.log" 2>&1 &
chrome_pid="$!"

screenshot_size="0"
stable_writes="0"
for _attempt in $(seq 1 300); do
    if [[ -s "$smoke_dir/presented.png" ]]; then
        new_size="$(wc -c < "$smoke_dir/presented.png" | tr -d ' ')"
        if [[ "$new_size" == "$screenshot_size" ]]; then
            stable_writes="$((stable_writes + 1))"
            if [[ "$stable_writes" -ge 2 ]]; then
                break
            fi
        else
            screenshot_size="$new_size"
            stable_writes="0"
        fi
    fi
    if ! kill -0 "$chrome_pid" 2>/dev/null && [[ ! -s "$smoke_dir/presented.png" ]]; then
        break
    fi
    sleep 0.1
done

if [[ ! -s "$smoke_dir/presented.png" ]] || [[ "$stable_writes" -lt 2 ]]; then
    cat "$smoke_dir/chrome.log" >&2
    echo "web smoke did not capture a complete screenshot within 30 seconds" >&2
    exit 1
fi
stop_pid "$chrome_pid"
chrome_pid=""

presented="$("${image_command[@]}" "$smoke_dir/presented.png" \
    -format "%[fx:(p{320,270}.b>0.8)&&(p{320,270}.r<0.1)&&(p{320,270}.g<0.1)?1:0]" info:)"
if [[ "$presented" != "1" ]]; then
    pixel="$("${image_command[@]}" "$smoke_dir/presented.png" \
        -format "%[pixel:p{320,270}]" info:)"
    echo "web smoke expected the fitted blue attachment at 320,270; found $pixel" >&2
    exit 1
fi

echo "Spinal browser presented-pixel smoke passed"

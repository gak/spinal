default:
    @just --list

# Build the browser host with its self-authored smoke fixture.
web-build: web-fixture
    env -u NO_COLOR trunk clean --config apps/spinal/web/Trunk.toml --dist release-dist
    env -u NO_COLOR trunk build --release --locked --config apps/spinal/web/Trunk.toml --dist release-dist

# Run the browser host in the foreground. Override with `just web 9000`.
web port="8424": web-fixture
    @echo "Spinal browser Open: http://127.0.0.1:{{ port }}/"
    env -u NO_COLOR trunk serve --locked --config apps/spinal/web/Trunk.toml --address 127.0.0.1 --port {{ port }}

# Prepare generic local-only files consumed by the browser smoke host.
web-fixture:
    cargo run --locked --package spinal-app --example prepare_web_fixture -- apps/spinal/web/bundle

# Open Spinal's native read-only Preview picker.
open:
    cargo run --locked --package spinal-app --bin spinal

# Run Spinal's read-only native Preview/Compare surface for an explicit export.
preview skeleton *args:
    cargo run --locked --package spinal-app --bin spinal -- "{{ skeleton }}" {{ args }}

# Inspect one export headlessly. Pass --json in args for compact machine output.
check skeleton *args:
    cargo run --locked --package spinal-app --bin spinal -- check "{{ skeleton }}" {{ args }}

# Run the repository's default verification suite.
test:
    cargo test --workspace --all-targets --locked

# Run and publish one generic, non-representative Phase 0A rehearsal.
phase0a-generic case editor workspace lock evidence:
    cargo run --locked --package spinal-phase0a --bin spinal-phase0a-generic -- "{{ case }}" "{{ editor }}" "{{ workspace }}" "{{ lock }}" "{{ evidence }}"

# Print a proposal-only representative binding for review; redirect it to a private 0600 file.
phase0a-binding-proposal runner case:
    "{{ runner }}" --propose-binding "{{ case }}"

# Execute the exact prebuilt, binding-pinned representative runner.
phase0a-representative runner binding case editor workspace lock evidence:
    "{{ runner }}" "{{ binding }}" "{{ case }}" "{{ editor }}" "{{ workspace }}" "{{ lock }}" "{{ evidence }}"

# Verify one exact representative evidence tree with the selected prebuilt verifier.
phase0a-verify verifier evidence:
    "{{ verifier }}" "{{ evidence }}"

# Render the browser fixtures and verify Preview/Compare pane isolation.
web-smoke port="8425":
    bash tools/web-smoke.sh "{{ port }}"

# Exercise the fixed Phase 0B browser capture seam; this rehearsal is never gate-eligible.
phase0b-browser-smoke port="8427":
    bash tools/spinal-phase0b-browser-smoke.sh "{{ port }}"

# Write PRE-FLIGHT evidence; human browser/native keyboard and VoiceOver review remains required.
accessibility-preflight evidence port="8426":
    bash tools/accessibility-preflight.sh "{{ evidence }}" "{{ port }}"

# Read-only decision/hash validation; prints a digest only when every invariant passes.
accessibility-report-check evidence:
    python3 tools/accessibility-report-check.py "{{ evidence }}"

# Exercise the accessibility report checker's fail-closed invariants.
accessibility-report-check-self-test:
    python3 tools/accessibility-report-check.py --self-test

default:
    @just --list

# Build the browser host with its self-authored smoke fixture.
web-build: web-fixture
    env -u NO_COLOR trunk clean --config apps/spinal/web/Trunk.toml --dist release-dist
    env -u NO_COLOR trunk build --release --locked --config apps/spinal/web/Trunk.toml --dist release-dist

# Run the browser host in the foreground. Override with `just web 9000`.
web port="8424": web-fixture
    @echo "Spinal browser Compare: http://127.0.0.1:{{ port }}/"
    env -u NO_COLOR trunk serve --locked --config apps/spinal/web/Trunk.toml --address 127.0.0.1 --port {{ port }}

# Prepare generic local-only files consumed by the browser smoke host.
web-fixture:
    cargo run --locked --package spinal-app --example prepare_web_fixture -- apps/spinal/web/bundle

# Run Spinal's read-only native Preview/Compare surface for one export.
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

# Render the browser fixtures and verify Preview/Compare pane isolation.
web-smoke port="8425":
    bash tools/web-smoke.sh "{{ port }}"

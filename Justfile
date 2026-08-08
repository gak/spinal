default:
    @just --list

# Build the browser host with its self-authored smoke fixture.
web-build: web-fixture
    env -u NO_COLOR trunk clean --config apps/spinal-viewer/web/Trunk.toml --dist release-dist
    env -u NO_COLOR trunk build --release --locked --config apps/spinal-viewer/web/Trunk.toml --dist release-dist

# Run the browser host in the foreground. Override with `just web 9000`.
web port="8424": web-fixture
    @echo "Spinal browser viewer: http://127.0.0.1:{{ port }}/"
    env -u NO_COLOR trunk serve --locked --config apps/spinal-viewer/web/Trunk.toml --address 127.0.0.1 --port {{ port }}

# Prepare generic local-only files consumed by the browser smoke host.
web-fixture:
    cargo run --locked --package spinal-viewer --example prepare_web_fixture -- apps/spinal-viewer/web/bundle

# Run the read-only native viewer for one export.
viewer skeleton *args:
    cargo run --locked --package spinal-viewer -- "{{ skeleton }}" {{ args }}

# Run the repository's default verification suite.
test:
    cargo test --workspace --all-targets --locked

# Run and publish one generic, non-representative Phase 0A rehearsal.
phase0a-generic case editor workspace lock evidence:
    cargo run --locked --package spinal-phase0a --bin spinal-phase0a-generic -- "{{ case }}" "{{ editor }}" "{{ workspace }}" "{{ lock }}" "{{ evidence }}"

# Render the browser fixture in headless Chrome and verify its center pixel is presented.
web-smoke port="8425":
    bash tools/web-smoke.sh "{{ port }}"

#!/usr/bin/env bash
set -euo pipefail

readonly fixture_root="${1:-${SPINAL_4_3_23_FIXTURES:-}}"
if [[ -z "${fixture_root}" ]]; then
  echo "usage: $0 <external-fixture-root>" >&2
  exit 2
fi
if [[ ! -d "${fixture_root}" ]]; then
  echo "error: external fixture root is not a directory: ${fixture_root}" >&2
  exit 2
fi

workspace_root="$(git rev-parse --show-toplevel)"
readonly workspace_root
readonly checksum_file="${workspace_root}/fixtures/SHA256SUMS"

(
  cd "${fixture_root}"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum --check "${checksum_file}"
  elif command -v shasum >/dev/null 2>&1; then
    shasum --algorithm 256 --check "${checksum_file}"
  else
    echo "error: sha256sum or shasum is required" >&2
    exit 2
  fi
)

export SPINAL_4_3_23_FIXTURES="${fixture_root}"
env -u RUSTC_WRAPPER cargo test \
  --package spinal \
  --test editor_4_3_23_contract \
  -- \
  --ignored \
  --nocapture
env -u RUSTC_WRAPPER cargo test \
  --package bevy_spinal \
  --no-default-features \
  --test asset_loader \
  exact_editor_exports_load_as_complete_bevy_assets \
  -- \
  --ignored \
  --nocapture

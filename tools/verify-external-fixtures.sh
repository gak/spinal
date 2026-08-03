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
preview_root="$(mktemp -d)"
readonly preview_root
trap 'rm -rf "${preview_root}"' EXIT

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
  official_spineboy_exports_are_exact_version_compatibility_tripwires \
  -- \
  --ignored \
  --nocapture

"${workspace_root}/tools/prepare-spineboy-aim-preview.sh" \
  "${fixture_root}/ess" \
  "${fixture_root}/pro" \
  "${preview_root}"
export SPINAL_SPINEBOY_AIM_PREVIEW="${preview_root}"
env -u RUSTC_WRAPPER cargo test \
  --package spinal \
  --test editor_4_3_23_contract \
  prepared_spineboy_ \
  -- \
  --ignored \
  --nocapture
weighted_preview_root="${preview_root}/weighted"
readonly weighted_preview_root
"${workspace_root}/tools/prepare-spineboy-weighted-preview.sh" \
  "${fixture_root}/pro" \
  "${weighted_preview_root}"
export SPINAL_SPINEBOY_WEIGHTED_PREVIEW="${weighted_preview_root}"
env -u RUSTC_WRAPPER cargo test \
  --package bevy_spinal \
  --no-default-features \
  --test asset_loader \
  prepared_professional_weighted_preview_has_drawable_output \
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
env -u RUSTC_WRAPPER cargo test \
  --package bevy_spinal \
  --no-default-features \
  --test asset_loader \
  prepared_preview_straight_alpha_reconstructs_source_pma_in_gamma_space \
  -- \
  --ignored \
  --nocapture

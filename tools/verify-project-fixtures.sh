#!/usr/bin/env bash
set -euo pipefail

readonly fixture_root_input="${1:-${SPINAL_4_3_23_PROJECT_FIXTURES:-}}"
if [[ -z "${fixture_root_input}" ]]; then
  echo "usage: $0 <project-fixture-root>" >&2
  exit 2
fi
if [[ ! -d "${fixture_root_input}" ]]; then
  echo "error: project fixture root is not a directory: ${fixture_root_input}" >&2
  exit 2
fi

fixture_root="$(cd "${fixture_root_input}" && pwd -P)"
readonly fixture_root
workspace_root="$(git rev-parse --show-toplevel)"
readonly workspace_root

required_files=(
  "MANIFEST.json"
  "SHA256SUMS"
  "provenance/README.md"
  "provenance/artwork.csv"
  "provenance/editor-version.txt"
)
for relative in "${required_files[@]}"; do
  if [[ ! -s "${fixture_root}/${relative}" ]]; then
    echo "error: required intake evidence is missing or empty: ${relative}" >&2
    exit 1
  fi
done
if ! grep -Fqx "4.3.23" "${fixture_root}/provenance/editor-version.txt"; then
  echo "error: provenance/editor-version.txt must contain exactly 4.3.23" >&2
  exit 1
fi
if [[ ! -d "${fixture_root}/provenance/source-images" ]]; then
  echo "error: provenance/source-images is required for reproducible texture packing" >&2
  exit 1
fi
if [[ ! -d "${fixture_root}/presets" ]]; then
  echo "error: presets directory is required" >&2
  exit 1
fi
if ! find "${fixture_root}/presets" -type f -print -quit | grep -q .; then
  echo "error: presets must contain the checksummed settings referenced by MANIFEST.json" >&2
  exit 1
fi

actual_checksums="$(mktemp)"
readonly actual_checksums
trap 'rm -f "${actual_checksums}"' EXIT
(
  cd "${fixture_root}"
  while IFS= read -r path; do
    if command -v sha256sum >/dev/null 2>&1; then
      sha256sum "${path}"
    elif command -v shasum >/dev/null 2>&1; then
      shasum --algorithm 256 "${path}"
    else
      echo "error: sha256sum or shasum is required" >&2
      exit 2
    fi
  done < <(find . -type f ! -name SHA256SUMS -print | LC_ALL=C sort)
) >"${actual_checksums}"
if ! diff -u "${fixture_root}/SHA256SUMS" "${actual_checksums}"; then
  echo "error: SHA256SUMS must cover every intake file in stable path order" >&2
  exit 1
fi

export SPINAL_4_3_23_PROJECT_FIXTURES="${fixture_root}"
cd "${workspace_root}"
env -u RUSTC_WRAPPER cargo test \
  --locked \
  --package spinal \
  --test project_4_3_23_contract \
  -- \
  --ignored \
  --nocapture
env -u RUSTC_WRAPPER cargo test \
  --locked \
  --package bevy_spinal \
  --no-default-features \
  --test asset_loader \
  project_owned_nonfatal_exports_load_as_complete_bevy_assets \
  -- \
  --ignored \
  --nocapture

#!/usr/bin/env bash
set -euo pipefail

readonly package_name="spinal"
readonly manifest_path="spinal/Cargo.toml"

workspace_root="$(git rev-parse --show-toplevel)"
cd "${workspace_root}"

if ! grep -Eq '^[[:space:]]*publish[[:space:]]*=[[:space:]]*false[[:space:]]*$' "${manifest_path}"; then
  echo "error: ${package_name} must remain non-publishable (publish = false)" >&2
  exit 1
fi

package_args=(package --package "${package_name}" --list --locked)
if [[ "${PACKAGE_AUDIT_ALLOW_DIRTY:-0}" == "1" ]]; then
  package_args+=(--allow-dirty)
fi

package_list="$(mktemp)"
trap 'rm -f "${package_list}"' EXIT
"${CARGO:-cargo}" "${package_args[@]}" >"${package_list}"

if [[ ! -s "${package_list}" ]]; then
  echo "error: cargo returned an empty package file list" >&2
  exit 1
fi

file_count=0
unexpected_count=0

while IFS= read -r path; do
  ((file_count += 1))

  if [[ "${path}" =~ (^|/)(assets|legacy|bevy_spinal|target)(/|$) ]]; then
    echo "error: unexpected package content: ${path} (forbidden path)" >&2
    ((unexpected_count += 1))
    continue
  fi

  case "${path}" in
    .cargo_vcs_info.json | Cargo.lock | Cargo.toml | Cargo.toml.orig)
      ;;
    LICENSE-APACHE | LICENSE-MIT | README.md | \
      src/animation.rs | src/asset.rs | src/atlas.rs | src/diagnostic.rs | src/draw.rs | \
      src/frame.rs | \
      src/geometry.rs | src/id.rs | src/json.rs | src/lib.rs | \
      src/load/animation.rs | src/load/build.rs | src/load/error.rs | \
      src/load/mod.rs | src/load/schema.rs | src/math.rs | src/player.rs | src/pose.rs | \
      src/skeleton.rs | src/world.rs | \
      tests/editor_4_3_23_contract.rs | tests/frame_contract.rs | \
      tests/loading_contract.rs | tests/player_contract.rs | \
      tests/public_contract.rs | tests/runtime_contract.rs)
      ;;
    *)
      echo "error: unexpected package content: ${path} (not allowlisted)" >&2
      ((unexpected_count += 1))
      ;;
  esac
done <"${package_list}"

required_files=(
  Cargo.toml
  Cargo.toml.orig
  LICENSE-APACHE
  LICENSE-MIT
  README.md
  src/animation.rs
  src/asset.rs
  src/atlas.rs
  src/diagnostic.rs
  src/draw.rs
  src/frame.rs
  src/geometry.rs
  src/id.rs
  src/json.rs
  src/lib.rs
  src/load/animation.rs
  src/load/build.rs
  src/load/error.rs
  src/load/mod.rs
  src/load/schema.rs
  src/math.rs
  src/player.rs
  src/pose.rs
  src/skeleton.rs
  src/world.rs
  tests/editor_4_3_23_contract.rs
  tests/frame_contract.rs
  tests/loading_contract.rs
  tests/player_contract.rs
  tests/public_contract.rs
  tests/runtime_contract.rs
)

missing_count=0
for path in "${required_files[@]}"; do
  if ! grep -Fqx "${path}" "${package_list}"; then
    echo "error: required package content is missing: ${path}" >&2
    ((missing_count += 1))
  fi
done

if [[ "${unexpected_count}" != "0" || "${missing_count}" != "0" ]]; then
  exit 1
fi

printf 'audited %d allowlisted files in %s\n' "${file_count}" "${package_name}"

readonly bevy_package_name="bevy_spinal"
readonly bevy_manifest_path="bevy_spinal/Cargo.toml"

if ! grep -Eq '^[[:space:]]*publish[[:space:]]*=[[:space:]]*false[[:space:]]*$' "${bevy_manifest_path}"; then
  echo "error: ${bevy_package_name} must remain non-publishable (publish = false)" >&2
  exit 1
fi

bevy_package_args=(package --package "${bevy_package_name}" --list --locked)
if [[ "${PACKAGE_AUDIT_ALLOW_DIRTY:-0}" == "1" ]]; then
  bevy_package_args+=(--allow-dirty)
fi

bevy_package_list="$(mktemp)"
trap 'rm -f "${package_list}" "${bevy_package_list}"' EXIT
"${CARGO:-cargo}" "${bevy_package_args[@]}" >"${bevy_package_list}"

if [[ ! -s "${bevy_package_list}" ]]; then
  echo "error: cargo returned an empty package file list" >&2
  exit 1
fi

bevy_file_count=0
bevy_unexpected_count=0

while IFS= read -r path; do
  ((bevy_file_count += 1))

  if [[ "${path}" =~ (^|/)(legacy|target)(/|$) ]]; then
    echo "error: unexpected package content: ${path} (forbidden path)" >&2
    ((bevy_unexpected_count += 1))
    continue
  fi

  case "${path}" in
    .cargo_vcs_info.json | Cargo.lock | Cargo.toml | Cargo.toml.orig)
      ;;
    LICENSE-APACHE | LICENSE-MIT | README.md | \
      examples/assets/README.md | examples/assets/viewer.atlas | \
      examples/assets/viewer.spine.json | examples/viewer.rs | \
      src/asset.rs | src/components.rs | src/lib.rs | src/plugin.rs | \
      src/render.rs | src/runtime.rs | \
      tests/asset_loader.rs | tests/public_api.rs | tests/runtime_plugin.rs)
      ;;
    *)
      echo "error: unexpected package content: ${path} (not allowlisted)" >&2
      ((bevy_unexpected_count += 1))
      ;;
  esac
done <"${bevy_package_list}"

bevy_required_files=(
  Cargo.toml
  Cargo.toml.orig
  LICENSE-APACHE
  LICENSE-MIT
  README.md
  examples/assets/README.md
  examples/assets/viewer.atlas
  examples/assets/viewer.spine.json
  examples/viewer.rs
  src/asset.rs
  src/components.rs
  src/lib.rs
  src/plugin.rs
  src/render.rs
  src/runtime.rs
  tests/asset_loader.rs
  tests/public_api.rs
  tests/runtime_plugin.rs
)

bevy_missing_count=0
for path in "${bevy_required_files[@]}"; do
  if ! grep -Fqx "${path}" "${bevy_package_list}"; then
    echo "error: required package content is missing: ${path}" >&2
    ((bevy_missing_count += 1))
  fi
done

if [[ "${bevy_unexpected_count}" != "0" || "${bevy_missing_count}" != "0" ]]; then
  exit 1
fi

printf 'audited %d allowlisted files in %s\n' "${bevy_file_count}" "${bevy_package_name}"

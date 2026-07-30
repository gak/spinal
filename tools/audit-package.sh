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
      src/asset.rs | src/diagnostic.rs | src/id.rs | src/lib.rs | \
      src/math.rs | src/skeleton.rs | tests/public_contract.rs)
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
  src/asset.rs
  src/diagnostic.rs
  src/id.rs
  src/lib.rs
  src/math.rs
  src/skeleton.rs
  tests/public_contract.rs
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

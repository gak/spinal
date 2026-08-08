#!/usr/bin/env bash
set -euo pipefail

workspace_root="$(git rev-parse --show-toplevel)"
cd "${workspace_root}"

# Keep the forbidden values split so this audit checks its own tracked source.
private_project="Loaf"
private_project+="stead"
private_person_ascii="Jos"
private_person_ascii+="e"
private_person_unicode="$(printf 'Jos\303\251')"
private_asset="Mi"
private_asset+="so"
private_pattern="${private_project}|${private_person_ascii}|${private_person_unicode}|${private_asset}"

user_root="/Use"
user_root+="rs/"
volume_root="/Vol"
volume_root+="umes/"
workspace_marker="Documents/"
workspace_marker+="Codex"
machine_pattern="${user_root}|${volume_root}|${workspace_marker}"

active_paths=()
while IFS= read -r path; do
  case "${path}" in
    PLAN-UNIFIED-SPINAL-APP.md | legacy/*)
      continue
      ;;
  esac
  [[ -e "${path}" ]] || continue
  active_paths+=("${path}")
done < <(git ls-files)

if [[ "${#active_paths[@]}" -eq 0 ]]; then
  echo "error: no active tracked paths found" >&2
  exit 1
fi

failure=0
for path in "${active_paths[@]}"; do
  if printf '%s\n' "${path}" | LC_ALL=C grep -Eiq "${private_pattern}"; then
    printf 'error: private identifier in tracked path: %s\n' "${path}" >&2
    failure=1
  fi
  if printf '%s\n' "${path}" | LC_ALL=C grep -Eq "${machine_pattern}"; then
    printf 'error: concrete user-machine path in tracked path: %s\n' "${path}" >&2
    failure=1
  fi
done

if LC_ALL=C git grep -n -I -i -E "${private_pattern}" -- "${active_paths[@]}"; then
  echo "error: private identifier found in active tracked content" >&2
  failure=1
fi

if LC_ALL=C git grep -n -I -E "${machine_pattern}" -- "${active_paths[@]}"; then
  echo "error: concrete user-machine path found in active tracked content" >&2
  failure=1
fi

if [[ "${failure}" -ne 0 ]]; then
  exit 1
fi

printf 'audited %d active tracked files for generic repository content\n' "${#active_paths[@]}"

#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
    echo "usage: $0 ESSENTIAL_EXPORT_DIR PRO_EXPORT_DIR OUTPUT_DIR" >&2
    exit 2
fi

essential_dir=$1
pro_dir=$2
output_dir=$3

essential_json="$essential_dir/spineboy-ess.json"
essential_atlas="$essential_dir/spineboy-ess.atlas"
essential_png="$essential_dir/spineboy-ess.png"
pro_json="$pro_dir/spineboy-pro.json"

for input in "$essential_json" "$essential_atlas" "$essential_png" "$pro_json"; do
    if [[ ! -f "$input" ]]; then
        echo "missing input: $input" >&2
        exit 1
    fi
done

for command in jq magick; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "required command is not installed: $command" >&2
        exit 1
    fi
done

preview_json="$output_dir/spineboy-rigid-aim.json"
preview_atlas="$output_dir/spineboy-rigid-aim.atlas"
preview_png="$output_dir/spineboy-ess.png"

for output in "$preview_json" "$preview_atlas" "$preview_png"; do
    if [[ -e "$output" ]]; then
        echo "refusing to overwrite existing output: $output" >&2
        exit 1
    fi
done

mkdir -p "$output_dir"
temporary_dir=$(mktemp -d "$output_dir/.spineboy-aim-preview.XXXXXX")
trap 'rm -rf "$temporary_dir"' EXIT

jq --slurpfile essential "$essential_json" '
    .animations |= with_entries(
        select(
            .key == "aim"
            or .key == "idle"
            or .key == "walk"
            or .key == "run"
        )
    )
    | del(.animations.aim.slots.crosshair)
    | .constraints = [
        .constraints[]
        | select(
            .name == "front-leg-ik"
            or .name == "rear-leg-ik"
            or .name == "aim-torso-ik"
            or .name == "aim-torso-transform"
            or .name == "aim-head-transform"
            or .name == "aim-front-arm-transform"
            or .name == "aim-ik"
        )
    ]
    | .bones |= map(del(.inherit))
    | .slots |= map(del(.blend))
    | .skins[0].attachments = (
        $essential[0].skins[0].attachments
        | with_entries(
            .value |= with_entries(
                select((.value.type // "region") == "region")
            )
        )
    )
' "$pro_json" >"$temporary_dir/spineboy-rigid-aim.json"

sed 's/^pma:true$/pma:false/' \
    "$essential_atlas" >"$temporary_dir/spineboy-rigid-aim.atlas"

magick "$essential_png" -alpha disassociate \
    "$temporary_dir/spineboy-ess.png"

mv "$temporary_dir/spineboy-rigid-aim.json" "$preview_json"
mv "$temporary_dir/spineboy-rigid-aim.atlas" "$preview_atlas"
mv "$temporary_dir/spineboy-ess.png" "$preview_png"

echo "prepared rigid Spineboy aiming preview in $output_dir"

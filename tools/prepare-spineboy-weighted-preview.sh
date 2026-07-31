#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
    echo "usage: $0 PROFESSIONAL_EXPORT_DIR OUTPUT_DIR" >&2
    exit 2
fi

professional_dir=$1
output_dir=$2
source_json="$professional_dir/spineboy-pro.json"
source_atlas="$professional_dir/spineboy-pro.atlas"
source_png="$professional_dir/spineboy-pro.png"

for input in "$source_json" "$source_atlas" "$source_png"; do
    if [[ ! -f "$input" ]]; then
        echo "missing input: $input" >&2
        exit 1
    fi
done

if ! command -v magick >/dev/null 2>&1; then
    echo "required command is not installed: magick" >&2
    exit 1
fi

preview_json="$output_dir/spineboy-pro.json"
preview_atlas="$output_dir/spineboy-pro.atlas"
preview_png="$output_dir/spineboy-pro.png"
for output in "$preview_json" "$preview_atlas" "$preview_png"; do
    if [[ -e "$output" ]]; then
        echo "refusing to overwrite existing output: $output" >&2
        exit 1
    fi
done

mkdir -p "$output_dir"
temporary_dir=$(mktemp -d "$output_dir/.spineboy-weighted-preview.XXXXXX")
trap 'rm -rf "$temporary_dir"' EXIT

cp "$source_json" "$temporary_dir/spineboy-pro.json"
sed 's/^pma:true$/pma:false/' \
    "$source_atlas" >"$temporary_dir/spineboy-pro.atlas"

magick \
    \( "$source_png" -colorspace RGB -alpha disassociate -colorspace sRGB \) \
    \( "$source_png" -alpha extract \) \
    -compose CopyOpacity \
    -composite \
    "$temporary_dir/spineboy-pro.png"

mv "$temporary_dir/spineboy-pro.json" "$preview_json"
mv "$temporary_dir/spineboy-pro.atlas" "$preview_atlas"
mv "$temporary_dir/spineboy-pro.png" "$preview_png"

echo "prepared weighted Spineboy Professional preview in $output_dir"

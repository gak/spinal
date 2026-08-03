#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
    echo "usage: $0 DELIVERED_EXPORT_DIR OUTPUT_DIR" >&2
    exit 2
fi

delivery_dir=$1
output_dir=$2
source_json="$delivery_dir/Base Cat 1.json"
source_atlas="$delivery_dir/Animation_2.atlas"
source_png="$delivery_dir/Animation_2.png"

for input in "$source_json" "$source_atlas" "$source_png"; do
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

if ! jq -e '.skeleton.spine == "4.3.23"' "$source_json" >/dev/null; then
    echo "expected Base Cat 1.json to declare Spine 4.3.23" >&2
    exit 1
fi

pma_pages=0
while IFS= read -r line; do
    if [[ "$line" == "pma:true" ]]; then
        ((pma_pages += 1))
    fi
done <"$source_atlas"
if [[ $pma_pages -ne 1 ]]; then
    echo "expected Animation_2.atlas to contain exactly one pma:true page" >&2
    exit 1
fi

preview_json="$output_dir/cat.spine.json"
preview_atlas="$output_dir/cat.atlas"
preview_png="$output_dir/Animation_2.png"
for output in "$preview_json" "$preview_atlas" "$preview_png"; do
    if [[ -e "$output" ]]; then
        echo "refusing to overwrite existing output: $output" >&2
        exit 1
    fi
done

mkdir -p "$output_dir"
temporary_dir=$(mktemp -d "$output_dir/.loafstead-cat-weighted-preview.XXXXXX")
trap 'rm -rf "$temporary_dir"' EXIT

# Keep the skeleton export byte-for-byte identical while giving the compound
# Bevy loader its conventional `.spine.json` plus sibling `.atlas` names.
cp "$source_json" "$temporary_dir/cat.spine.json"
sed 's/^pma:true$/pma:false/' \
    "$source_atlas" >"$temporary_dir/cat.atlas"

# Spine premultiplies this page in gamma-encoded RGB. Disassociate in that
# same space, then restore the byte-exact source alpha channel. Merely changing
# the atlas flag produces dark fringes. Alpha-zero colour cannot be recovered,
# so this remains a preview substitute for a straight-alpha export with bleed.
magick \
    \( "$source_png" -alpha disassociate \) \
    \( "$source_png" -alpha extract \) \
    -compose CopyOpacity \
    -composite \
    "$temporary_dir/Animation_2.png"

mv "$temporary_dir/cat.spine.json" "$preview_json"
mv "$temporary_dir/cat.atlas" "$preview_atlas"
mv "$temporary_dir/Animation_2.png" "$preview_png"

echo "prepared weighted Loafstead cat preview in $output_dir"

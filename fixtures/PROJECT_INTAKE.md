# Project-owned fixture intake

This checklist closes the remaining Stage 0 evidence gate. It is for
project-owned Spine 4.3.23 projects and exports, not the official Spineboy
compatibility samples.

Do not open and re-export received files before preserving the raw delivery.
The raw archive is the immutable evidence. Any reduced, renamed, or deliberately
damaged fixture is a derived artifact and must retain a link to its raw source
checksum.

## One delivery from José

Preserve these items together:

- the source `.spine` project or isolated source projects;
- the exact saved skeleton-export and texture-pack presets used for every
  export run; identical runs may reference the same checksummed preset files;
- the unmodified `.json`, `.atlas`, and every atlas page PNG;
- the complete warning output for every export run, including an explicit
  record when a run produced no warnings;
- an editor-version record showing exactly `4.3.23`;
- a source-artwork inventory recording every image's origin, owner, license,
  and redistribution status, with each derived atlas page traceable to those
  inputs;
- the external source image files used by the Spine project and texture packer,
  when their redistribution terms permit preservation;
- a short provenance note naming the Spine project owner and confirming the
  redistribution status of the project and exports.

The Spine project, export data, and artwork do not need to use the runtime's
MIT OR Apache-2.0 software license. Record their actual, separately compatible
asset licenses and do not redistribute any input whose permission is unclear.

The positive export must use [the production profile](../EXPORT_PROFILE.md):
JSON, straight-alpha RGBA8888 PNG pages, bleed, Linear filtering, clamped wrap,
and quarter-turn packing only. Multiple atlas pages are allowed and at least
one project-owned positive fixture must exercise them.

## Positive-profile evidence

Across the project-owned positive fixtures, author at least one occurrence of
every supported wire-format feature:

- a multi-page straight-alpha atlas with RGBA8888 format, Linear filtering,
  clamped wrap, positive scale metadata, packed bounds, at least one authored
  region index, a visibly trimmed region with nonzero offsets and a distinct
  original size, and at least one actually 90-degree packed region;
- normal bones, rigid regions, setup slots, setup draw order, attachment
  switching, and attachment-only skins;
- independent skin layers suitable for a breed, hat, collar, and glasses;
- one-bone and two-bone IK, including order, target, mix, and both bend
  directions;
- rotate, translate, scale, shear, IK mix, IK bend, slot attachment, slot
  colour, draw-order, and event timelines;
- linear, stepped, and Bézier interpolation.

The cat delivery should name every gameplay clip intended for the canary and
the attachment-only skin names for the available three hats, three collars,
and two existing glasses. The third glasses asset is a separate content
dependency and must remain explicitly absent rather than being represented by
invented placeholder art.

Runtime-only behavior such as ordered skin composition, one-track crossfades,
procedural overrides, and allocation-free steady state stays covered by
project-authored Rust tests. It does not require an editor-only representation.

## Isolated unsupported tripwires

Keep each tripwire to one unsupported feature whenever the editor permits it.
The required rows are the unsupported and ignored-metadata entries in
[`COVERAGE.toml`](COVERAGE.toml). They currently cover:

- weighted mesh, unweighted mesh, deform, clipping, path constraint, transform
  constraint, physics constraint, and attachment sequence;
- skin-specific bones, skin-specific constraints, two-colour tint, non-normal
  blending, and non-normal bone inheritance;
- IK softness setup and timeline data, compress, stretch, and uniform scaling;
- premultiplied alpha, non-quarter atlas rotation, and an unknown atlas page
  setting;
- bounding-box and point attachment warning cases;
- binary skeleton rejection.

If Spine cannot export one feature in isolation, document the smallest
inseparable combination in that case's `inseparable_with` array. The verifier
detects all covered unsupported features, derives the required diagnostic
severity, code, count, and scope from the checked-in contract, and rejects
undeclared companions. Do not silently reuse a broad sample as proof of an
isolated boundary.

Each manifest case names its own skeleton-export and texture-pack preset paths.
This is required for binary, premultiplied-alpha, Nonessential, and other runs
whose settings intentionally differ from the positive baseline.

Each case also contains a normalized `settings` snapshot. The verifier checks
the exact editor version, data format, Animation clean up, warnings and warning
count, Nonessential state, atlas packing, straight/PMA alpha role, bleed,
padding, filtering, wrap, format, rotation, whitespace stripping, and scale.
The saved preset and raw warning output remain immutable source evidence; the
structured snapshot makes their relevant contract machine-checkable.

A raw editor case records `source_kind = "raw-editor-export"`, its exact
top-level `source_project` path and checksum, and its preserved nonempty ZIP
delivery archive under `raw/` with the archive's exact `SHA256SUMS` value. Its
`raw_archive_members` object maps every extracted source project, skeleton,
atlas, page, preset, and warning artifact to an exact ZIP member. The verifier
checks ZIP structure and proves the extracted bytes match the member size and
CRC held by the checksummed archive. A reduced or deliberately damaged case
records `source_kind = "derived"`, an exported artifact belonging to one of
those declared raw cases, its exact checksum from `SHA256SUMS`, and a
reproducible derivation description. A derived case cannot point at itself or
at another derived case.

## Scale and Nonessential probe

Preserve two project revisions. Revision 1 uses Reference scale 1; revision 2
is created from revision 1 by changing only the skeleton's `Reference scale` to
one recorded non-default value. This is a skeleton property, so the two
revisions must have different `.spine` checksums. Export the complete matrix:

| Export | Source revision | Reference scale | Nonessential | Expected comparison |
|---|---|---:|---:|---|
| A | Revision 1 | 1 | off | Baseline |
| B | Revision 1 | 1 | on | Only documented Nonessential metadata differs from A |
| C | Revision 2 | recorded non-default value | off | Scale probe |
| D | Revision 2 | same recorded value as C | on | Scale plus Nonessential probe |

Preserve both `.spine` revisions and all four JSON files before interpreting
them. A/B must share the exact Revision 1 checksum; C/D must share the exact
Revision 2 checksum. Record a reproducible derivation naming
`skeleton.reference_scale` as the only deliberate project change. The observed
JSON diff, rather than an assumed field name, decides whether the property
affects skeleton metadata, authored values, or both. Record the exact finite,
positive non-default value and the per-export scale and Nonessential settings
in `MANIFEST.json`.

## Intake layout

Keep raw evidence outside the packaged crates until its redistribution status
is confirmed:

```text
<fixture-root>/
  provenance/
    README.md
    artwork.csv
    editor-version.txt
    projects/                  # including both scale-probe revisions
    source-images/
  presets/
    <all checksummed presets referenced by export cases>
  raw/
    <unaltered delivery archives>
  positive/
    <one directory per export, including warnings.txt>
  tripwires/
    <one directory per COVERAGE.toml id, including warnings.txt>
  scale-probe/
    a-nonessential-off/        # including warnings.txt
    b-nonessential-on/         # including warnings.txt
    c-scaled-nonessential-off/ # including warnings.txt
    d-scaled-nonessential-on/  # including warnings.txt
  MANIFEST.json
  SHA256SUMS
```

`MANIFEST.json` maps every required `COVERAGE.toml` ID to one exact artifact
and a resolvable feature-bearing location. JSON locations use `json:/...` and
must select the containing bone, slot, skin, constraint, timeline, key array,
or attachment object being evaluated, not a leaf field or another record in
the same section. Supported atlas locations use the unambiguous parsed
source-order ordinal, `atlas-page:<ordinal>` or `atlas-region:<ordinal>`, so
the verifier evaluates that exact record even when indexed regions share a
name. This includes paired region values such as packed bounds versus original
size. Unsupported raw atlas properties use `atlas:<exact trimmed property
line>`. The binary case uses `binary:<relative .skel path>` and the scale
quartet uses `scale-probe`. Aggregate statements such as "the positive fixture
covers the profile" are not sufficient evidence.

An abridged manifest shape is:

```json
{
  "format_version": 1,
  "target_spine_version": "4.3.23",
  "source_projects": [
    "provenance/projects/cat.spine",
    "provenance/projects/scale-1.spine",
    "provenance/projects/scale-2.spine"
  ],
  "project_provenance": {
    "origin": "Loafstead project source",
    "owner": "<recorded owner>",
    "license": "<actual project/export license>",
    "redistribution_status": "external-only or approved"
  },
  "artwork": [
    {
      "origin": "Loafstead cat source artwork",
      "owner": "<recorded owner>",
      "license": "<actual asset license>",
      "redistribution_status": "external-only or approved",
      "source_files": ["provenance/source-images/cat.png"],
      "derived_pages": ["positive/cat/cat-0.png", "positive/cat/cat-1.png"]
    }
  ],
  "positive": [
    {
      "id": "cat-positive",
      "json": "positive/cat/cat.spine.json",
      "atlas": "positive/cat/cat.atlas",
      "pages": ["positive/cat/cat-0.png", "positive/cat/cat-1.png"],
      "export_preset": "presets/positive.export.json",
      "texture_packer_preset": "presets/positive.pack.json",
      "warnings": "positive/cat/warnings.txt",
      "source_kind": "raw-editor-export",
      "source_project": "provenance/projects/cat.spine",
      "source_project_sha256": "<matching SHA256SUMS value>",
      "raw_archive": "raw/cat-positive.zip",
      "raw_archive_sha256": "<64 lowercase hex characters>",
      "raw_archive_members": {
        "provenance/projects/cat.spine": "delivery/cat.spine",
        "positive/cat/cat.spine.json": "delivery/export/cat.spine.json",
        "positive/cat/cat.atlas": "delivery/export/cat.atlas",
        "positive/cat/cat-0.png": "delivery/export/cat-0.png",
        "positive/cat/cat-1.png": "delivery/export/cat-1.png",
        "presets/positive.export.json": "delivery/presets/positive.export.json",
        "presets/positive.pack.json": "delivery/presets/positive.pack.json",
        "positive/cat/warnings.txt": "delivery/export/warnings.txt"
      },
      "settings": {
        "editor_version": "4.3.23",
        "format": "json",
        "animation_cleanup": false,
        "warnings": true,
        "warning_count": 0,
        "nonessential": false,
        "pack_atlas": true,
        "texture": {
          "format": "RGBA8888",
          "min_filter": "Linear",
          "mag_filter": "Linear",
          "wrap_x": "ClampToEdge",
          "wrap_y": "ClampToEdge",
          "strip_whitespace_x": true,
          "strip_whitespace_y": true,
          "edge_padding": true,
          "padding_x": 2,
          "padding_y": 2,
          "rotation": true,
          "scale": 1.0,
          "pma": false,
          "bleed": true
        }
      }
    }
  ],
  "tripwires": [
    {
      "coverage_id": "weighted-mesh-attachment",
      "json": "tripwires/weighted-mesh-attachment/case.spine.json",
      "atlas": "tripwires/weighted-mesh-attachment/case.atlas",
      "pages": ["tripwires/weighted-mesh-attachment/case.png"],
      "export_preset": "presets/positive.export.json",
      "texture_packer_preset": "presets/positive.pack.json",
      "warnings": "tripwires/weighted-mesh-attachment/warnings.txt",
      "source_kind": "raw-editor-export",
      "source_project": "provenance/projects/cat.spine",
      "source_project_sha256": "<matching SHA256SUMS value>",
      "raw_archive": "raw/weighted-mesh-attachment.zip",
      "raw_archive_sha256": "<64 lowercase hex characters>",
      "raw_archive_members": {
        "provenance/projects/cat.spine": "delivery/cat.spine",
        "tripwires/weighted-mesh-attachment/case.spine.json": "delivery/export/case.spine.json",
        "tripwires/weighted-mesh-attachment/case.atlas": "delivery/export/case.atlas",
        "tripwires/weighted-mesh-attachment/case.png": "delivery/export/case.png",
        "presets/positive.export.json": "delivery/presets/positive.export.json",
        "presets/positive.pack.json": "delivery/presets/positive.pack.json",
        "tripwires/weighted-mesh-attachment/warnings.txt": "delivery/export/warnings.txt"
      },
      "inseparable_with": [],
      "settings": "<same structured settings as the positive JSON case>"
    },
    {
      "coverage_id": "binary-skeleton",
      "binary": "tripwires/binary-skeleton/case.skel",
      "atlas": "tripwires/binary-skeleton/case.atlas",
      "pages": ["tripwires/binary-skeleton/case.png"],
      "export_preset": "presets/binary.export.json",
      "texture_packer_preset": "presets/positive.pack.json",
      "warnings": "tripwires/binary-skeleton/warnings.txt",
      "source_kind": "raw-editor-export",
      "source_project": "provenance/projects/cat.spine",
      "source_project_sha256": "<matching SHA256SUMS value>",
      "raw_archive": "raw/binary-skeleton.zip",
      "raw_archive_sha256": "<64 lowercase hex characters>",
      "raw_archive_members": {
        "provenance/projects/cat.spine": "delivery/cat.spine",
        "tripwires/binary-skeleton/case.skel": "delivery/export/case.skel",
        "tripwires/binary-skeleton/case.atlas": "delivery/export/case.atlas",
        "tripwires/binary-skeleton/case.png": "delivery/export/case.png",
        "presets/binary.export.json": "delivery/presets/binary.export.json",
        "presets/positive.pack.json": "delivery/presets/positive.pack.json",
        "tripwires/binary-skeleton/warnings.txt": "delivery/export/warnings.txt"
      },
      "inseparable_with": [],
      "settings": "<same settings object, with format set to binary>",
      "expected": "not-accepted"
    }
  ],
  "fatal": [
    {
      "id": "invalid-reference",
      "json": "fatal/invalid-reference/case.spine.json",
      "atlas": "fatal/invalid-reference/case.atlas",
      "pages": ["fatal/invalid-reference/case.png"],
      "export_preset": "presets/positive.export.json",
      "texture_packer_preset": "presets/positive.pack.json",
      "warnings": "fatal/invalid-reference/warnings.txt",
      "source_kind": "derived",
      "derived_from": "positive/cat/cat.spine.json",
      "derived_from_sha256": "<matching SHA256SUMS value>",
      "derivation": "replace one required bone reference with an absent name",
      "settings": "<same structured settings as the raw source export>",
      "expected_error": "unresolved-reference"
    }
  ],
  "coverage": [
    {
      "id": "json-4-3-23",
      "artifact": "cat-positive",
      "location": "json:/skeleton/spine"
    },
    {
      "id": "weighted-mesh-attachment",
      "artifact": "weighted-mesh-attachment",
      "location": "json:/skins/0/attachments/example/example"
    },
    {
      "id": "skeleton-reference-scale-nonessential-off-on",
      "artifact": "scale-probe",
      "location": "scale_probe"
    }
  ],
  "scale_probe": {
    "non_default_reference_scale": 2.0,
    "source_revision_derivation": {
      "from_project": "provenance/projects/scale-1.spine",
      "from_sha256": "<matching SHA256SUMS value>",
      "to_project": "provenance/projects/scale-2.spine",
      "to_sha256": "<matching SHA256SUMS value>",
      "changed_property": "skeleton.reference_scale",
      "from_value": 1.0,
      "to_value": 2.0,
      "procedure": "Open scale-1.spine in 4.3.23, change only Reference scale to 2, and save as scale-2.spine"
    },
    "nonessential_paths_at_scale_1": ["/skeleton/images"],
    "nonessential_paths_at_non_default_scale": ["/skeleton/images"],
    "scale_paths_with_nonessential_off": ["/bones/1/x"],
    "scale_paths_with_nonessential_on": ["/bones/1/x", "/skeleton/width"],
    "a": {
      "id": "scale-a",
      "reference_scale": 1.0,
      "nonessential": false,
      "json": "scale-probe/a-nonessential-off/case.spine.json",
      "atlas": "scale-probe/a-nonessential-off/case.atlas",
      "pages": ["scale-probe/a-nonessential-off/case.png"],
      "export_preset": "presets/scale-a.export.json",
      "texture_packer_preset": "presets/positive.pack.json",
      "warnings": "scale-probe/a-nonessential-off/warnings.txt",
      "source_kind": "raw-editor-export",
      "source_project": "provenance/projects/scale-1.spine",
      "source_project_sha256": "<matching SHA256SUMS value shared by A/B>",
      "raw_archive": "raw/scale-a.zip",
      "raw_archive_sha256": "<64 lowercase hex characters>",
      "raw_archive_members": {
        "provenance/projects/scale-1.spine": "delivery/scale-1.spine",
        "scale-probe/a-nonessential-off/case.spine.json": "delivery/export/case.spine.json",
        "scale-probe/a-nonessential-off/case.atlas": "delivery/export/case.atlas",
        "scale-probe/a-nonessential-off/case.png": "delivery/export/case.png",
        "presets/scale-a.export.json": "delivery/presets/scale-a.export.json",
        "presets/positive.pack.json": "delivery/presets/positive.pack.json",
        "scale-probe/a-nonessential-off/warnings.txt": "delivery/export/warnings.txt"
      },
      "settings": "<same structured settings, with nonessential false>"
    },
    "b": {},
    "c": {},
    "d": {}
  }
}
```

Cases `b`, `c`, and `d` have the same fields as `a`, with their corresponding
paths and settings. A/B must name the same scale-1 source revision; C/D must
name the same non-default-scale source revision. Those revisions must differ
and match `source_revision_derivation`. String placeholders shown for repeated
`settings` objects must be replaced with complete objects matching the
positive example. The checked-in Rust gate is the schema authority and reports
a field-specific failure for incomplete manifests.

Generate `SHA256SUMS` over every raw file in stable path order. Keep the
original delivery archive checksum separately so the extracted evidence can be
traced back to exactly what was received.

Run `tools/verify-project-fixtures.sh <fixture-root>`. This separate gate
validates the project manifest and checksums, loads and exercises positive
exports, checks exact isolated-tripwire diagnostics, checks the fatal fixture,
checks the scale-probe relationships, and loads all nonfatal cases through
Bevy.

After that command passes, update `MANIFEST.toml` and `COVERAGE.toml` with the
observed filenames, checksums, preset paths, provenance, exact diagnostic
outcomes, and `production_state = "verified"`. Only then can Stages 0, 2, 3,
and 4 lose their provisional qualifier.

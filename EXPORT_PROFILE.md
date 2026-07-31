# Loafstead Spine 4.3.23 export profile

This is the shared production preset for the first Loafstead integration. It
keeps exported content inside Spinal's deliberately small supported profile.

## Skeleton data

- Editor version: exactly `4.3.23`.
- Format: JSON, version `4.3`.
- Pretty print: optional.
- Nonessential data: off.
- Animation clean up: off until a project-owned before/after fixture has been
  reviewed.
- Warnings: on, with no unresolved export warnings accepted into the game.
- Texture atlas: pack during export.
- Images: attachments used by the exported skeleton.
- Atlas layout: atlas per skeleton.

JSON is the production format for the demo. Binary can be added later without
changing the standalone runtime or Bevy API.

## Texture packing

- Output: PNG.
- Packing: rectangles.
- Rotation: on. Quarter-turn packing is supported.
- Strip whitespace X and Y: on.
- Premultiply alpha: **off**.
- Bleed: **on**.
- Padding X and Y: at least 2 pixels.
- Edge padding: on.
- Min and mag filter: Linear.
- Wrap X and Y: Clamp to edge.
- Format: RGBA8888.
- Scale: 1.
- Multiple pages: allowed. Do not force every cat or cosmetic onto one page.

The alpha choice is intentional. Spine's texture-packer documentation says
the packing setting must match runtime rendering and recommends bleed for
straight-alpha filtering. Loafstead uses Bevy's linear rendering path, so
straight-alpha PNGs avoid gamma-space premultiplied-alpha edge errors. Spinal
loads `pma:true` atlases as bounded degraded assets, omits affected draws, and
shows the red diagnostic cross rather than silently using the wrong blend.

## Supported authoring contract

- Normal-transform bones.
- Rigid region attachments.
- Setup slots and normal slot blending.
- Attachment-only skins for breeds, hats, collars, and glasses.
- One- and two-bone IK using target, order, mix, and bend direction.
- Direct world-rotation transform constraints using source, constrained
  bones, rotation offset, order, and rotation mix.
- Rotate, translate, scale, shear, IK mix/bend, slot attachment/colour, draw
  order, transform mix, and event timelines.
- Linear, stepped, and Bézier interpolation.

Meshes, deform, clipping, path/physics constraints, non-rotation transform
mappings, local-source/local-target/additive/clamped transform modes, skin
bones or constraints, sequences, two-colour tint, non-normal blend modes,
non-normal bone inheritance, and advanced IK options are outside the first
profile.
Known unsupported records remain loadable when their boundary is safe, but
affected output is omitted and visibly diagnosed.

## Delivery checklist

Deliver the `.json`, `.atlas`, and every atlas page image together. Before
merging an export:

1. Confirm the JSON skeleton version is `4.3.23`.
2. Confirm every atlas page records straight alpha (`pma:false` or an omitted
   `pma` field).
3. Open it in the Spinal viewer and exercise every animation and cosmetic.
4. Treat any red cross as an export-contract failure unless the unsupported
   feature is an intentional tripwire.
5. Preserve the export preset, warnings, source project provenance, and file
   checksums with the project-owned conformance fixture.

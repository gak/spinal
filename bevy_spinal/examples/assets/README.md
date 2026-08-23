# Runtime showcase fixture

`viewer.spine.json` and `viewer.atlas` are small, self-authored fixtures derived
from the public Spine export-format documentation. They are not editor exports,
do not contain third-party art, and are not conformance evidence.

The runtime showcase creates both matching atlas images in memory. The default
pose and animations exercise the supported demo profile:

- straight-alpha RGBA8888 regions across two Linear-filtered, clamped atlas
  pages with explicit positive scale metadata;
- indexed, trimmed, original-size, and quarter-turn-packed atlas regions;
- one- and two-bone IK;
- rotate, translate, scale, shear, IK, slot color, slot attachment, draw-order,
  and event timelines;
- linear, stepped, and Bezier curves;
- independent attachment-only cosmetic skins; and
- crossfades plus a procedural `head` bone override through showcase controls.

`tripwire/unsupported` is a deliberately inactive clipping attachment. Press
`U` or pass `--tripwire` to activate it. Spinal retains this currently
unsupported attachment, omits it from draw output, marks the instance
`Degraded`, and shows the red diagnostic cross. The default showcase stays
`Ready`.

Hot reload is covered by the adapter's memory-backed integration tests rather
than this visual fixture. External exact-version Spineboy exports passed the
loader tests at the historical Bevy 0.18 checkpoint; their private fixture root
was unavailable for the Bevy 0.19 migration, so that external matrix is
**NOT RUN** on the current adapter. A project-owned representative export is
still required for the production asset-backed canary and complete profile
conformance.

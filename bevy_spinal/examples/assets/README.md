# Viewer fixture

`viewer.spine.json` and `viewer.atlas` are small, self-authored fixtures derived
from the public Spine export-format documentation. They are not editor exports,
do not contain third-party art, and are not conformance evidence.

The viewer creates both matching atlas images in memory. The default pose and
animations exercise the supported demo profile:

- rigid regions across two atlas pages;
- trimmed and quarter-turn-packed atlas regions;
- one- and two-bone IK;
- rotate, translate, scale, shear, IK, slot color, slot attachment, draw-order,
  and event timelines;
- linear, stepped, and Bezier curves;
- independent attachment-only cosmetic skins; and
- crossfades plus a procedural `head` bone override through viewer controls.

`tripwire/unsupported` is a deliberately inactive mesh attachment. Press `U`
or pass `--tripwire` to activate it, omit its geometry, mark the instance
`Degraded`, and show the red diagnostic cross. The default viewer stays
`Ready`.

Hot reload is covered by the adapter's memory-backed integration tests rather
than this visual fixture. José's actual Spine 4.3.23 editor export is still
required before claiming editor conformance.

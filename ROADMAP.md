# Spinal implementation roadmap

This roadmap defines capability gates, not dates. Spinal is rebuilt in the
original repository and preserves its history, but the supported runtime is a
new clean-room implementation with a deliberately small public surface.

## Product contract

The first production profile targets exports from Spine 4.3.23:

- standard JSON skeleton data and multi-page text texture atlases;
- one or more straight-alpha PNG atlas pages, with documented page format,
  filter, wrap, and positive scale metadata;
- packed bounds, indices, whitespace-trim offsets and original sizes, and
  packed rotations in quarter turns;
- normal-transform bones and rigid region attachments;
- setup slots, draw order, attachment switching, and simple attachment-only
  skins, including ordered runtime composition for independent cosmetics;
- one- and two-bone IK with target, order, mix, and bend direction;
- direct world-rotation transform constraints with source, constrained bones,
  rotation offset, authored order, and unbounded rotation mix;
- rotate, translate, scale, and shear bone timelines;
- IK mix and bend-direction timelines;
- transform-constraint mix timelines, with rotation as the supported output
  channel;
- linear, stepped, and Bezier interpolation;
- slot attachment and colour timelines, draw-order timelines, and events;
- one animation track with interruption-safe crossfades;
- explicit procedural bone overrides after animation and before constraints;
- allocation-free steady-state evaluation after instance construction.

Loafstead's initial cosmetics are hats, collars, and glasses. They use simple
skin attachment swaps. Coats and weighted meshes are outside the first
contract.

This is a closed-world contract: a feature not explicitly listed as supported
has no implied runtime semantics. Known-but-unsupported data is diagnosed,
retained where useful, and ignored only when record boundaries are clear and a
coherent skeleton remains. Affected content continues to load and an active
`Degraded` diagnostic is visible in the Bevy adapter as an obvious red cross.
A `Warning` means output remains equivalent and never produces the cross.

The first profile does not support weighted or unweighted meshes, deform
timelines, clipping, path constraints, non-rotation transform mappings,
local-source/local-target/additive/clamped transform modes, physics constraints,
skin-specific bones or constraints, sequences, two-colour tint,
non-normal blend modes, non-normal bone transform or inheritance modes,
multiple animation tracks, binary skeleton data, IK softness, IK compress,
IK stretch, IK uniform scaling, or timelines for those IK options.
Premultiplied-alpha pages, non-quarter-turn packed rotations, and unknown atlas
page settings are also outside the first renderer profile.

Bounding boxes and point attachments may be retained as ignored metadata with
a warning because Loafstead does not consume them. Meshes, paths, clipping,
sequences, unsupported constraint types or options, and unsupported timelines
are safely skipped only when their containing record is unambiguous; they
produce a degraded diagnostic scoped to the affected element. Otherwise the
loader returns a fatal unsupported-data error.

Invalid syntax, non-finite required numbers, duplicate required names, invalid
parent order, unresolved bones, slots, constraint targets or sources, or
required atlas regions, and unsupported major or minor format versions are
fatal. A fatal load lets Loafstead use its sprite fallback.

## Stage 0: freeze the evidence and export profile

Status: exact external Spineboy Essential and Professional exports pass;
project-owned profile fixtures and their complete presets remain pending.

- Record exact official documentation URLs and access dates.
- Save José's editor-generated JSON export and texture-pack presets.
- Preserve unmodified 4.3.23 raw exports, source-project provenance, editor
  version output, and checksums.
- Include positive fixtures for every supported feature and one-feature
  tripwires for every unsupported feature.
- Probe a non-default Skeleton Reference scale with Nonessential data both
  enabled and disabled; do not assume a JSON field name without export evidence.
- Keep official sample files non-normative and outside packaged crates.
- Intake project-owned evidence through
  `tools/verify-project-fixtures.sh <fixture-root>`, which checks complete
  checksums, provenance, per-run presets and warnings, coverage locations,
  positive behavior, exact tripwire diagnostics, the fatal path, scale diffs,
  and Bevy compound loading.

Gate: every format claim maps to an official document or an observed,
checksummed 4.3.23 editor export. The external Spineboy samples cover a broad
wire-format tripwire, but complete supported-profile conformance remains
provisional until project-owned exports record every preset and feature.

## Stage 1: standalone foundation

Status: complete.

- Preserve the original Git history and establish an honest green baseline.
- Make `spinal` renderer-independent and park the Bevy 0.8 prototype.
- Introduce immutable shared assets, owned mutable instances, asset-scoped
  typed IDs, borrowed views, checked numeric types, and structured diagnostics.
- Pin the MSRV, lock dependencies, define package contents, and make the
  supported surface warning-free.
- Add clean-room contribution controls, source provenance, CI, dependency
  policy, and package audits.

Gate: format, test, Clippy, documentation, MSRV, dependency, and package checks
are green with no Bevy dependency in `spinal`.

## Stage 2: JSON and atlas loading

Status: provisionally complete; exact-version Spineboy compatibility tripwires
pass and complete project-owned fixture conformance remains a Stage 0 gate.

- Parse JSON and multi-page text atlases into private input models.
- Validate version, finite numbers, ordering, topology, names, references, and
  atlas links before constructing an immutable asset.
- Return stable loader errors that do not expose parser-library types.
- Return a load report whose diagnostics are also retained by the asset.
- Preserve source order and build allocation-free name-to-ID lookups.
- Classify supported, safely degraded, and fatal features explicitly.
- Treat demo inputs as trusted and caller-size-bounded; add configurable
  `LoadLimits` for untrusted inputs after the demo.

Gate: documentation-derived tests pass, malformed inputs never panic, package
fuzz targets and valid seed corpora cover both entry points, and raw 4.3.23
fixtures pass once available.

## Stage 3: pose and animation runtime

Status: complete for the provisional clean-room profile. Exact Spineboy
editor-export compatibility tripwires pass; complete profile conformance
remains gated by Stage 0.

- Retain key times as exact integer nanosecond ticks for boundaries and use the
  same ordered values for interpolation.
- Restore complete setup state and sample supported timelines at an absolute
  position through a renderer-independent low-level API.
- Sample supported timelines with linear, stepped, and Bezier curves.
- Resolve attachment placeholders through ordered attachment-only skin layers,
  then the default skin.
- Reconstruct slot colour, attachment, IK state, and draw order without
  steady-state allocation.

Gate: golden timeline tests, exact decimal boundary tests, deterministic
snapshots, skin-composition tests, fuzz evaluation, and allocation tests pass.

## Stage 4: stateful animation and solved frames

Status: complete for the provisional clean-room profile. Exact Spineboy
editor-export compatibility tripwires pass; complete profile conformance
remains gated by Stage 0.

- Add one asset-scoped animation player over the absolute sampler.
- Play, loop, interrupt, and crossfade on one animation track.
- Emit borrowed events exactly once across looping and transition boundaries.
- Expose a scoped pose-edit phase for procedural bone overrides.
- Evaluate world transforms, apply supported IK and transform constraints in
  authored order, and expose the final renderer-independent draw list.
- Track only diagnostics affecting the current solved frame.

Gate: documentation-derived world and IK math tests, transition and event
boundary tests, rapid interruption tests, deterministic snapshots,
active-diagnostic tests, fuzz evaluation, and allocator-counting tests pass.

## Stage 5: Bevy 0.18 adapter and Loafstead canary

Status: adapter complete and review-clear; external exact-version compound
assets pass. The reversible Loafstead scaffold is implemented, but its current
public Git pin is diagnostic-only. Positive replacement remains pending a
reviewed post-`dbbdf023` immutable pin, José's cat export, and the third
glasses asset.

- Create a new `bevy_spinal` plugin rather than upgrading the old Bevy 0.8
  architecture.
- Load skeleton, atlas, and page-image dependencies through Bevy assets.
- Keep runtime evaluation in `spinal`; keep ECS, hot reload, extraction, and
  rendering in the adapter.
- Batch rigid quads with correct draw order, colour, alpha, and multi-page
  textures.
- Provide components and systems for animation, crossfades, skins, events,
  procedural overrides, and hot reload.
- Render an unmistakable red-cross gizmo over content affected by an active
  degraded diagnostic.
- Provide a small viewer for asset and animation inspection.
- Consume `spinal` and `bevy_spinal` from a pinned Git revision in CI until a
  release is intentionally made.
- Replace one cat with a reversible canary path before broad migration.
- Map Loafstead states to authored clips and crossfade policy.
- Add three hats, three collars, and three glasses as attachment-only skins.
- Preserve sprite fallback while the canary is enabled.
- Keep the diagnostic entity visible for usable no-draw states, while retaining
  the sprite fallback unless Loafstead's explicit replacement policy passes.
- Add Loafstead-side logging, viewer fixtures, integration tests, and QA for
  transparency, draw order, sleep/eat/fall transitions, cosmetics, and active
  diagnostics.

Gate: the viewer exercises every supported feature, hot reload rebuilds
instances safely, unsupported tripwires remain visible without crashing, and
the canary matches gameplay behavior and performance with a safe sprite
fallback.

## Stage 6: AnimationMixer

Status: complete for the current mixer scope; implementation contract is
[PLAN-V0.4-ANIMATION-MIXER.md](PLAN-V0.4-ANIMATION-MIXER.md).

- Preserve the complete one-track API while adding a standalone permanent base
  track and ordered override tracks.
- Sample sparse continuous property contributions and compose each against the
  live lower-track pose.
- Add independent track weights, weight fades, and interruption-safe
  within-track crossfades.
- Keep authored events, lifecycle reports, diagnostics, identity errors, and
  hot reload track-aware and deterministic.
- Add first-class skeleton-space control targets before one authored-order
  constraint solve.
- Expose a declarative named-track Bevy facade and a walk-plus-mouse-aim
  viewer.

Gate: every mixer requirement and deferred feature in the plan has direct
evidence, optimized mixing matches a slow reference compositor, and all
one-track compatibility, allocation, package, and external-fixture gates pass.
The project-owned cat fixture and Loafstead visual canary remain separate
Stage 0 and Stage 5 gates.

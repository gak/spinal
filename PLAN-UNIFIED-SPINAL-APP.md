# Unified Spinal Application Plan

Status: reviewed and approved for staged implementation
Primary compatibility target: Spine 4.3.23 JSON plus text atlases
Release target: none; open-source release work is intentionally deferred

## Name and positioning

**Spinal** remains the only product name.

This initiative is the **Unified Spinal Application Plan**. It is not a new
product called Spinal Collab, Spinal Studio, or Spinal Workbench.

The product promise is:

> Preview exactly what Spinal will render, diagnose incompatibilities, compare
> revisions, and safely accept animation-only updates.

User-facing terms are:

- **Preview** for viewing one export;
- **Compare** for viewing two revisions with one synchronized clock;
- **Diagnostics** for compatibility and runtime findings;
- **Review** for a submitted animation update;
- **Current version**, **submission**, and **proposed version** instead of
  master, handoff, and candidate where plain language is clearer.

`coordinator` is an internal implementation term only.

## Final architecture decision

Spinal is one repository and one product with deliberately narrow internal
boundaries:

```text
spinal/
├── spinal/                 renderer-independent runtime library
├── bevy_spinal/            Bevy asset, ECS, and rendering adapter
└── apps/
    └── spinal/             package `spinal-app`, binary `spinal`
        ├── shared viewer session and UI model
        ├── native host
        ├── browser/WASM host
        └── native-only coordinator capability
```

The existing `apps/spinal-viewer` implementation moves into the unified app.
Generic command-line checking also becomes part of the single `spinal`
command. There is no separate `spinal_viewer_core` crate, no duplicate WASM
viewer, and no public-facing `spinal_collab` application.

The coordinator remains a capability boundary because browser WASM cannot
execute Spine, use arbitrary filesystem paths, or own durable SQLite jobs. It
is not automatically a process boundary:

- the native app calls it in-process;
- `spinal serve` explicitly exposes it through a protected loopback host;
- a standalone browser viewer can open local export bundles without it;
- no login item, permanent daemon, or fixed port is installed by default.

The current Python/FastAPI implementation is a characterization prototype, not
the final product boundary. Preserve its proven behavior with tests during
consolidation. The production target is a native-only Rust coordinator inside
the Spinal application; do not begin that port until Phase 0 passes.

## Principles

1. **Evidence before product surface.** The CLI import and runtime premise must
   pass before more workflow UI is built.
2. **Fail closed.** Missing validators, warnings, ambiguous skeletons,
   unsupported features, stale versions, and unverified provenance block
   promotion.
3. **One viewer implementation.** Native and browser hosts share the same
   session, transport, rendering, commands, diagnostics, and review state.
4. **One comparison clock.** Compare uses one Bevy application, two
   `SpinalInstance`s, and one authoritative time source.
5. **The `.spine` project remains canonical.** Exported JSON is runtime data,
   diagnostic evidence, and animation interchange; it is never text-merged or
   treated as the project source of truth.
6. **Whole animations are the merge unit.** Do not merge timelines, keyframes,
   constraints, setup data, or binary `.spine` differences.
7. **Project policy stays with the project.** Spinal contains no downstream
   animation names, skeleton names, cosmetics, paths, IDs, branding, or
   acceptance rules.
8. **Privileges are optional.** Preview and Compare do not require Spine or a
   coordinator. Merge features require a separately installed and licensed
   Spine editor.
9. **No silent degradation.** Unsupported behavior is explicit and actionable;
   plausible but incorrect rendering is a failure.
10. **Graduate capabilities separately.** Read-only viewing becomes stable
    before mutation, automatic promotion, remote operation, or advanced tools.

## Scope

### Included

- Spine 4.3.23 JSON and text-atlas loading for the explicitly supported
  profile;
- native and browser/WASM Preview;
- synchronized Current/Proposed Compare;
- animation and skin selection, play/pause, loop, speed, seek, frame stepping,
  fit/reset, and camera synchronization;
- rig and constraint inventory, diagnostics, selected-bone overlay, and event
  log;
- local, single-user animation-update intake;
- immutable version packages and provenance manifests;
- whole-animation three-way comparison and conflict decisions;
- Spine CLI candidate construction from a copy of the current project;
- fail-closed validation and atomic promotion;
- explicit headless checking and candidate construction;
- generic fixtures in Spinal and private consumer acceptance outside Spinal.

### Deferred

- cloud hosting, remote collaboration, authentication, teams, assignments,
  comments, or notifications;
- LLM editing or merging of JSON or `.spine` files;
- automatic timeline or keyframe merging;
- setup, skeleton, skin, attachment, constraint, or asset changes;
- video or FFmpeg preview generation;
- transition-sequence authoring or a general animation editor;
- deep interactive rig, constraint, attachment, and event inspectors;
- a plugin system or generalized project-policy framework;
- public crates, signed installers, hosted demos, or other release work;
- automatic headless promotion without an explicit reviewed policy.

## Phase 0: go/no-go evidence gates

Phase 0 has two gates. Phase 0A proves the licensed editor import premise before
any coordinator port or mutation workflow begins. Read-only Phase 1 viewer
consolidation may proceed because it supplies the shared browser/runtime
surface required by Phase 0B. Phase 3 cannot begin until both gates pass with a
representative project and animation-only submission.

### Phase 0A: Spine capability preflight

Prove at runtime, rather than echoing configuration, that:

- the selected executable exists and is the approved Spine launcher;
- exact editor version 4.3.23 runs;
- the installed license is activated for the required operations;
- the launcher accepts every advanced import/export argument used by Spinal;
- the exact requested skeleton is discovered without a fallback guess;
- CLI calls return expected exit codes and warning output;
- the native Spinal validator is installed and usable;
- Spine CLI work can be serialized safely.

Record the exact commands, executable identity, version output, exit codes,
stdout, stderr, warnings, and output checksums.

Run every CLI probe inside its complete package context, including required
empty asset directories. A missing-path message such as `Images path not
found` is blocking even when Spine exits successfully; it is never hidden by
an allowlist or by a warning detector that only searches for the words
`warning` and `error`.

Phase 0 emits one machine-readable assertion matrix. Every required assertion
has its own result and evidence digest; the overall result is the conjunction
of all assertions. Missing, skipped, or degraded evidence means
`passed = false`.

### JSON round trip

Using the approved pretty, nonessential export preset:

1. Export `source.spine` to JSON.
2. Import that JSON into `reconstructed.spine`.
3. Export `reconstructed.spine` again.
4. Normalize both exports.
5. Produce the unmodified textual diff, then the normalized textual and
   semantic differences.
6. Record a narrow allowlist of harmless volatile fields.
7. Record every represented property observed not to survive reconstruction,
   plus the fixture's known coverage limits.
8. Repeat the process to test determinism.

Round-trip evidence does not authorize reconstructing production masters from
JSON. Production candidates always begin as copies of the current `.spine`
project.

### Whole-animation import

Import one new animation and replace one existing animation in separate copies
of the current project. After each operation, export and prove:

- the imported animation fingerprint equals the submission fingerprint;
- setup, skeleton, skins, attachments, constraints, and assets equal current;
- every unselected animation equals current;
- selected animation replacement is the only semantic change;
- repeating the same import is idempotent;
- warnings, partial outputs, timeouts, and nonzero exits fail the operation;
- the current source package is byte-for-byte unchanged.

### Phase 0B: consolidated runtime and viewer gate

Run this gate after the Phase 1 shared viewer exists.

The resulting candidate runtime bundle must:

- load through native Spinal without fallback validation;
- decode every atlas page and required texture;
- evaluate every changed animation at key times and bounded samples;
- expose no blocking unsupported feature used by the representative project;
- produce the same semantic draw stream natively and in WASM;
- render correctly in a browser canvas;
- support selection, skins, playback, looping, speed, seek, stepping, and fit;
- match known bone transforms and active attachments at selected timestamps.

Validation covers both Current and Proposed. Parser errors, degraded features,
unsupported features used by either bundle, missing pages, texture decode
failures, and evaluation failures are blocking. Any nonblocking informational
diagnostic must be explicitly enumerated by stable code.

### Go/no-go decisions

Any unexplained semantic change, ignored warning, missing target, false-green
validation, or source mutation fails Phase 0A and stops mutation work for
review. Any unsupported required feature or native/WASM semantic mismatch fails
Phase 0B and stops coordinator and review work. There is no automatic fallback
to another runtime, GUI automation, FFmpeg, or LLM editing.

## Phase 1: consolidate the viewer

Consolidate the existing native viewer and WASM prototype before adding new
workflow features.

### Shared model

Create one internal application model containing:

- `SourceBundle`: JSON, atlas, texture pages, provenance, and diagnostics,
  independent of paths or URLs;
- `ViewerSession`: selected source, animation, skin, camera, transport, and
  diagnostics;
- `ReviewClock`: exact shared time, pause, speed, loop, seek, and stepping;
- commands for select, play/pause, restart, step, seek, fit, camera, overlays,
  and comparison state;
- canonical snapshots used to test native/WASM parity.

The implementation invariants are:

- a `SourceBundle` is an immutable virtual file map loaded through a named
  in-memory Bevy asset source;
- source slots are Primary and optional Comparison;
- animation identity is its name, never its catalog index;
- Compare waits at an all-sources-ready barrier before synchronizing playback;
- normal playback advances both instances from the same Bevy delta rather than
  seeking every frame, which would suppress crossed events;
- a two-camera/render-layer integration test is required before Compare is
  considered complete;
- the browser bridge is versioned, checks origin, source, and a per-launch
  capability, and contains no coordinator-specific product naming.

Native paths, browser `File` objects, ZIPs, embedded assets, and coordinator
URLs all populate `SourceBundle` through thin host adapters.

### One comparison renderer

Delete the two-iframe comparison model. Compare is one Bevy application with:

- two `SpinalInstance`s;
- two viewports or one composited comparison surface;
- one `ReviewClock`;
- independently controllable camera synchronization;
- visible Current and Proposed labels;
- both durations shown when they differ;
- side-by-side initially, with wipe or overlay deferred until needed.

### UI model

Use one document-centric shell:

- **Open** is a command and empty state;
- **Preview** is the default workspace;
- **Compare** appears when a second source is present;
- **Diagnostics** is a contextual inspector/drawer;
- the first inspector is limited to inventory, diagnostics, selected-bone
  overlay, and event log;
- **Review** appears only for a handoff workflow;
- transport remains directly beneath the canvas;
- the canvas keeps most of the usable area.

Do not implement a five-mode navigation system or duplicate the canvas across
separate mini-apps.

### Accessibility boundary

Share the application state and actions, not necessarily every pixel of host
chrome:

- native uses Bevy/AccessKit platform semantics;
- browser uses semantic HTML controls and panels around the shared Bevy canvas;
- all review actions are keyboard-operable;
- the canvas has a concise accessible state summary and a structured inspector;
- loaded animations begin paused and only an explicit user action starts playback;
- playback does not announce every frame;
- change, warning, and failure states never rely on color alone;
- reduced-motion settings disable incidental motion and flicker comparisons.

The acceptance target is WCAG 2.2 AA for application chrome and every critical
review task. Browser and native hosts may render different thin control shells
when their accessibility platforms require it; that does not permit duplicate
session, command, transport, renderer, diagnostic, or review-policy logic.

## Phase 2: Bevy 0.19 migration

First consolidate on the existing pinned Bevy 0.18.1 implementation. Then
upgrade the whole workspace to Bevy 0.19 in a separate, reviewable change.

- Do not support both Bevy versions in one branch.
- Keep `bevy_spinal` dependencies and default features minimal.
- Use target-specific native features rather than unconditional X11 features.
- Build native and WASM targets on every change after the migration.
- Prefer WebGPU for the supported browser profile; add a separate WebGL2 build
  only if older-browser demand is demonstrated.
- Keep browser WASM single-threaded until profiling proves otherwise.
- Rerun the complete Phase 0B native/WASM evidence matrix on Bevy 0.19. Phase
  3 remains blocked until that post-migration matrix passes; evidence from the
  Bevy 0.18.1 implementation does not carry across automatically.

## Phase 3: generic coordinator capability

Move only generic, proven behavior from the current prototype.

Begin with one narrow native vertical slice: three manifest-backed packages,
no conflicts, exact analysis-authorized imports, fail-closed validation, and a
hash-bound Ready proposal with no promotion. Exercise that slice on several
production-like handoffs while retaining the Python suite as characterization
evidence. Build the full durable queue, recovery, and promotion surface only
after this workflow evidence supports it.

### Project storage

The first Current version is created through an explicit **Create project**
flow. The coordinator supplies a full package containing exactly one `.spine`
project and its assets, selects the target skeleton when discovery is
ambiguous, and approves the generated project ID and version metadata. Spinal
validates the complete package, snapshots it immutably, and only then records
it as Current. A bare `.spine` file can never bootstrap a project.

Store immutable records for:

- project;
- current version and version history;
- submission;
- analysis and conflict decisions;
- proposed version and runtime artifacts;
- validation report;
- promotion and review decisions.

Operational projects and uploads remain outside the Git repository.

Every package contains `spinal-project.json` with:

- schema version;
- project ID;
- base version;
- target skeleton identity;
- Spine editor version;
- source `.spine` SHA-256;
- canonical payload digest;
- export-profile identity where available.

The canonical payload digest covers a deterministic inventory of every
package path, required empty directory, file length, and file digest, excluding
the manifest's own digest field. The exact uploaded or downloaded archive
bytes receive a separate archive SHA-256 stored with the immutable version
record, outside the archive itself. No checksum is defined recursively over a
container that embeds that same checksum.

The package inventory also records required directories. Explicit empty asset
directories are preserved through extraction, staging, candidate construction,
and ZIP creation. Every Base, Current, and Submission CLI export is staged in
the declared package context; a bare submission may borrow only the immutable
asset context of its declared base while its returned `.spine` file remains
the sole submitted project input.

A bare `.spine` submission or unmanifested package is accepted only after the
coordinator selects an exact immutable Base version. Spinal binds that Base
version and digest before analysis, supplies only Base's immutable asset
context when staging a bare project, and records that the provenance was
coordinator-selected. Promotion is permitted only after all normal structural,
asset, version, runtime, per-animation review, and stale-Current gates pass.
Assets included in an unmanifested package must still match Base exactly in the
first release profile.

### Intake safety

ZIP intake rejects:

- traversal and absolute paths;
- symlinks and special files;
- encrypted archives;
- duplicate or portable-name-colliding paths;
- multiple `.spine` projects;
- entry, depth, filename, compressed, decompressed, pixel, and disk limits.

Finder metadata is ignored. Actual decompressed bytes are counted while
streaming rather than trusting archive declarations alone. Required empty
directory entries are retained rather than discarded as archive noise.

### Three-way analysis

For Base, Current, and Submission:

- export normalized diagnostic JSON with the approved exact-version preset;
- compute a setup fingerprint excluding animations and approved volatile
  fields;
- compute one independent fingerprint per animation;
- compare packaged assets and provenance;
- reject setup, asset, deletion, unsupported-feature, and version changes.

Classify animations without merging their internals:

- submission changed only: use submission;
- current changed only: retain current;
- both changed different animations: combine automatically;
- both changed the same animation: require a visual decision;
- deletion: reject in the first release profile.

A submission with no net animation change ends as **No changes** and creates no
new version.

### Candidate construction

Construct a candidate only from a copy of Current. Use the Phase-0-verified
Spine CLI import operation, enabling replacement only for explicitly selected
conflicts.

Replacement is permitted only for the exact animation allowlist authorized by
analysis: safe submission-only edits to existing animations plus conflicts
explicitly resolved to **Use submission**. It is never blanket project
replacement.

After import:

- repeat structural and animation fingerprint checks;
- verify only approved animations changed;
- export the runtime review bundle;
- run native Spinal parsing, texture decoding, evaluation, and diagnostics;
- fail on warnings, nonzero exit, timeout, missing validator, degraded result,
  stale Current, or version mismatch.

Analysis, conflict decisions, validation, per-animation review, and promotion
records bind to the exact Base, Current, Submission, and Proposed package
digests to which they apply.

## Phase 4: review and promotion

### Conflict review

Do not ask for text-only conflict choices. Open the affected animation directly
in synchronized visual comparison.

The data comparison is Base/Current/Submission. The UI may show two panes and
allow switching the baseline rather than showing three simultaneous canvases.
For each same-animation conflict, offer:

- **Keep current**;
- **Use submission**;
- **Reject submission** as one global workflow action, not a misleading
  per-animation choice.

### Proposed-version review

Post-build review compares Current and Proposed. It:

- defaults to changed animations only;
- provides previous/next changed animation;
- records review separately for every changed animation;
- shows progress such as `2 of 3 reviewed`;
- shows new diagnostics relative to Current;
- keeps approval disabled until every changed animation is reviewed and all
  gates pass;
- names the exact version promoted by the approval action.

Promotion rechecks that Current has not advanced, makes validated artifacts
durable, and updates the current-version pointer atomically. It then offers the
complete current package and next versioned submission package. No coordinator
re-upload is required.

## Phase 5: headless and browser hosting

Expose the same application behavior through:

```text
spinal <export>              open Preview natively
spinal compare A B           open synchronized Compare
spinal check ...             deterministic read-only validation
spinal serve                 explicit local browser/workspace session
```

Headless reports use stable exit codes, stable diagnostic codes, and optional
machine-readable JSON. Successful validation does not imply promotion. Any
headless promotion requires an explicit policy flag and is deferred until the
interactive workflow has production evidence.

`spinal serve`:

- binds only to loopback on an ephemeral port;
- rejects wildcard and every non-loopback bind address, including `0.0.0.0`;
- serves UI and API from one origin;
- uses a per-launch capability/session;
- validates Host and Origin;
- uses SameSite cookies and CSRF protection for mutations;
- permits no wildcard CORS;
- sets CSP, frame, MIME, and referrer protections;
- has no configuration path that silently turns local mode into remote mode.

After the limited production beta has passed its ten-handoff evidence gate,
add `spinal merge ...` for headless candidate construction. It uses the same
analysis and validation policy as the interactive workflow. Successful merge
still does not imply promotion, and unattended promotion remains deferred.

## Reliability and security

- Hold one OS-level lock for a state root.
- Serialize Spine CLI operations unless licensing and concurrency are proven.
- Run jobs outside UI/request threads with durable progress states.
- Make every phase idempotent or explicitly non-repeatable.
- Recover interrupted jobs without changing Current.
- Use per-job temporary directories, bounded output, minimal environment,
  closed stdin, cancellation, timeout, and process-tree termination.
- Validate the approved Spine executable and exact version before every job
  session.
- Stage, validate, hash, flush, and atomically install artifacts before the
  current-version transaction commits.
- Use private file permissions and redact project paths and secrets from user
  errors and ordinary logs.
- Create state directories and files with private user-only permissions.
- Treat animator submissions as trusted-team artifacts; public untrusted upload
  processing is out of scope without isolated workers.

## Verification

### Fast tests

- parser, atlas, evaluation, constraints, drawing, mixer, and diagnostics;
- normalization and fingerprint rules;
- three-way classification;
- exact shared-clock behavior, including unequal durations and jittered deltas;
- coordinator state transitions, idempotence, and stale-current checks;
- ZIP and manifest validation;
- a genericity audit rejecting active downstream-project identifiers and
  machine paths.

### Integration tests

- identical generic runtime bundle through native and WASM hosts;
- fake Spine adapter for intake, analysis, conflict, candidate, validation, and
  promotion workflows;
- restart recovery, double-submit, double-merge, and double-promotion;
- semantic draw-stream snapshots at a small set of meaningful timestamps;
- browser build and real-browser loading smoke;
- keyboard and accessible-name checks for the critical review path.

### Licensed release/acceptance runner

An opt-in runner with activated Spine 4.3.23 proves:

- exact-version capability preflight;
- normalized round-trip allowlist;
- existing and new animation import fingerprints;
- unchanged setup, assets, and unselected animations;
- candidate native/WASM load and evaluation;
- warning, timeout, partial-output, and nonzero-exit failure behavior.

Spinal owns one generic, project-owned, redistributable 4.3.23 fixture covering
the supported profile. Each downstream project keeps its private master and
submission fixtures outside Spinal and runs the same acceptance contract as a
consumer.

## Milestone and pivot gates

The work graduates in deliberately small capability levels:

- **Stable viewer:** read-only Preview, Compare, and Diagnostics are trustworthy
  natively and in the supported browser profile.
- **Handoff beta:** merge and promotion remain visibly experimental until at
  least ten consecutive production-like handoffs complete without corruption,
  lost work, false-green validation, or unrecoverable interruption.
- **Automation:** stable machine-readable diagnostics precede non-interactive
  candidate construction; unattended promotion remains deferred.

Stop or simplify when evidence contradicts the product shape:

- stop merge implementation if Phase 0 finds unexplained semantic changes;
- reconsider animation-only handoffs if more than two of the first ten real
  submissions legitimately require setup, attachment, constraint, or asset
  changes;
- keep native as the primary host if native and browser cannot share the
  session, rendering, and review behavior without substantial duplication;
- replace durable queued workflow with a guided one-shot flow if real use is
  only occasional and never benefits from resumable history;
- do not add transition authoring, remote collaboration, or generalized merge
  policy without repeated observed demand.

## Downstream working practice

1. The coordinator sends the latest versioned package.
2. The animator opens it in Spine 4.3.23.
3. The animator changes only assigned animations.
4. The animator does not change setup, bones, slots, skins, attachments,
   constraints, or assets.
5. The animator returns the `.spine` file or package and lists changed
   animations.
6. The coordinator submits it to Spinal, resolves conflicts visually, reviews
   every changed animation, and approves the proposed version.
7. Spinal produces the next complete versioned package.

If operational project files are version-controlled separately, use Git LFS
for `.spine`, archives, images, source artwork, audio, and other large binary
assets. Never text-merge `.spine` files.

## Implementation order

```text
0A. Strict Spine CLI, round-trip, and animation-import evidence gate
1. One shared viewer/session and single-clock Compare on Bevy 0.18.1
0B. Consolidated native/WASM runtime and viewer evidence gate
2. Whole-workspace migration to Bevy 0.19
3A. Remove downstream-specific application policy; add a no-conflict native
    coordinator vertical slice
3B. Add the evidence-backed immutable package, durable job, and recovery model
4. Visual conflict resolution, per-animation Review, and atomic promotion
5. Explicit `spinal serve` and headless check
6. Private downstream-project acceptance and limited production beta
7. Headless candidate construction only after the beta evidence gate
```

Do not start Phase 3 until Phase 0A and Phase 0B pass. Read-only Phase 1 work may
proceed in parallel with Phase 0A fixture preparation only when it does not
assume the merge premise has passed.

## Final review disposition

The plan is approved as a staged development direction with these conditions:

- Phase 0 is a real stop/go gate, not a documentation exercise.
- Viewer consolidation precedes workflow expansion.
- Project-specific policy never enters Spinal.
- Compare uses one renderer and one clock.
- Mutation and promotion remain fail-closed.
- Coordinator privilege is optional and on-demand.
- Advanced authoring, remote collaboration, and release work remain deferred.

This is the KISS boundary: one product, two runtime libraries, one application,
one optional privileged capability, and one animation-level merge rule.

## Parked open-source release notes

These are retained for later and are not part of the active implementation
plan:

- rewrite the public landing page around a quick start, screenshot/demo,
  compatibility table, and explicit support matrix;
- resolve old crates.io `0.0.1`, experimental tags, and future versioning;
- publish `spinal` before `bevy_spinal`, with complete Cargo metadata and clean
  package dry runs;
- move restrictive historical example assets out of the primary code/license
  story or replace them with project-owned fixtures;
- add explicit Spine trademark and non-affiliation wording;
- obtain independent review of the clean-room and compatibility claims before
  a broad release;
- add `SECURITY.md`, a private provenance-contact route, issue templates, a
  code of conduct, release documentation, and contributor lanes;
- build signed/notarized native releases and an attested generic browser demo;
- add a documented Bevy compatibility table and MSRV policy.

No release date or public stability promise should be attached until the
generic conformance fixture, real project acceptance gate, and recovery tests
have passed.

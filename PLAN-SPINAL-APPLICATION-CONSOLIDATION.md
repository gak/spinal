# Spinal Application Consolidation Plan

Plan status: approved for staged implementation
Evidence status: Phase 0 has not passed; mutation work remains blocked
Primary compatibility target: Spine 4.3.23 JSON plus text atlases
Release target: none; open-source release work is intentionally deferred

Current gate state:

| Area | State |
| --- | --- |
| Phase 0A evidence harness | implementation complete; fresh controlled-failure and licensed rehearsals pending |
| Phase 0A licensed generic calibration | **NO-GO FINDING RECORDED**; corrected fresh rehearsal pending |
| Phase 0A representative downstream-project run | **NOT RUN** |
| Shared viewer consolidation | shared bundle intake and browser transport foundation complete; full viewer work in progress |
| Phase 0B correctness matrix | **NOT RUN** |
| Mutation and promotion | blocked by Phase 0A and Phase 0B |

## Name and positioning

**Spinal** remains the only product name.

This initiative is **Spinal Application Consolidation**. It is not a new
product called Spinal Collab, Spinal Studio, or Spinal Workbench.

The product promise is:

> Preview exactly what Spinal will render, diagnose incompatibilities, compare
> revisions, and safely accept animation-only updates.

User-facing terms are:

- **Preview** for viewing one export;
- **Compare** for viewing two revisions with one synchronized clock;
- **Diagnostics** for compatibility and runtime findings;
- **Review** for a submitted animation update;
- **Reviewer** for the person who inspects and approves a proposed version;
- **Current version**, **submission**, and **proposed version** instead of
  master, handoff, and candidate where plain language is clearer.

`coordinator` is an internal implementation term only.

The version terms used throughout are:

- **Base**: the immutable Spinal version from which an animator started;
- **Current**: the latest approved immutable project version;
- **Submission**: the animator's returned project or package;
- **Proposed**: a validated version constructed from Current plus approved
  whole-animation changes, before promotion.

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

### Stable viewer scope

- Spine 4.3.23 JSON and text-atlas loading for the explicitly supported
  profile;
- native and browser/WASM Preview;
- synchronized Primary/Comparison viewing for any two revisions;
- animation and skin selection, play/pause, loop, speed, seek, frame stepping,
  fit/reset, and camera synchronization;
- concise inventory and actionable compatibility diagnostics;
- generic fixtures in Spinal and private consumer acceptance outside Spinal.

### Handoff beta scope, after both Phase 0 gates pass

- local, single-user animation-update intake;
- immutable version packages and provenance manifests;
- whole-animation three-way comparison and conflict decisions;
- Spine CLI candidate construction from a copy of the current project;
- fail-closed validation, explicit per-animation review, audited restore, and
  atomic promotion.

### Later committed automation

- deterministic headless checking with stable diagnostics;
- headless candidate construction only after the handoff beta evidence gate;
- no unattended promotion until a separate policy and evidence gate approves
  it.

### Deferred

- cloud hosting, remote collaboration, authentication, teams, assignments,
  comments, or notifications;
- LLM editing or merging of JSON or `.spine` files;
- automatic timeline or keyframe merging;
- setup, skeleton, skin, attachment, constraint, or asset changes;
- video or FFmpeg preview generation;
- transition-sequence authoring or a general animation editor;
- selected-bone overlays, event exploration, and deep interactive rig,
  constraint, or attachment inspectors;
- a plugin system or generalized project-policy framework;
- project-shaped procedural animation editors or tools that write runtime JSON;
- public crates, signed installers, hosted demos, or other release work;
- automatic headless promotion without an explicit reviewed policy.

## Phase 0: go/no-go evidence gates

Phase 0 has two gates. Phase 0A proves the licensed editor import premise before
any coordinator port or mutation workflow begins. Read-only Phase 1 viewer
consolidation and the Phase 2 migration may proceed because they supply the
final browser/runtime surface required by Phase 0B. Phase 3 cannot begin until
both gates pass with a representative project and animation-only submission.

`tools/spinal-phase0a` is an internal, unshipped conformance harness, not part
of the product CLI or the coordinator architecture. Its checked-in recipe and
README are authoritative for exact probe mechanics; this plan governs the gate
and its consequences. Freeze the harness after the fresh generic and
representative runs. Phase 3 may reuse proven product primitives—typed CLI
calls, fresh-copy mutation, fingerprints, warning handling, locks, and runtime
validation—but must not transplant the fixed 22-operation recipe, evidence
publisher, build-context capture, or full filesystem-tamper audit unless new
product evidence independently requires them. Ordinary `spinal check` remains
the product-facing validation command.

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

The first licensed host contract is the project owner's recorded macOS host and
architecture. Invoke the approved Mac CLI executable directly and select
exactly `--update 4.3.23` for every job; a family selector such as `4.3.xx` is
not acceptable. Record whether the command may use a prewarmed editor cache or
network, who owns the activated seat, and which opt-in CI host (if any) may run
licensed acceptance. Missing or revoked activation is a hard gate, never a
reason to fall back or expose license material in evidence.

Run every CLI probe inside its complete package context, including required
empty asset directories. A missing-path message such as `Images path not
found` is blocking even when Spine exits successfully; it is never hidden by
an allowlist or by a warning detector that only searches for the words
`warning` and `error`.

Phase 0 emits one machine-readable assertion matrix. Every required assertion
has its own result and evidence digest; the overall result is the conjunction
of all assertions. Missing, skipped, or degraded evidence means
`passed = false`.

An assertion cannot accept a caller-supplied pass boolean or cite an unrelated
artifact. Its result is derived from typed, assertion-specific evidence and
the exact process operation that produced it. Adversarial tests prove that
wrong version, wrong license state, wrong skeleton, unsupported arguments,
warnings, partial output, timeout, and nonzero exit each make the overall gate
fail.

Every evidence envelope records the exact harness binary digest, available
source revision and dirty state, lockfile digest, relevant Rust/runtime
versions, approved Spine launcher identity and digest, host OS and
architecture, fixture/package digests, and export preset. Phase 0B evidence
additionally records the Bevy version, WASM toolchain, browser version, and
GPU/backend profile. A CLI-only Phase 0A report does not fabricate browser or
GPU metadata. Sensitive activation material is never recorded.

The first real run uses only disposable, same-filesystem staged copies. A
calibration transcript cannot pass the gate: warning and result rules are
reviewed, checked in, and then exercised by a fresh run. Output discovery,
normalization, safe staging, orchestration, and typed assertion derivation
must be complete before Phase 0A can change from **NOT RUN** to a result.

The production rehearsal is one fixed, linear recipe rather than a general
job framework. It performs exactly 22 ordered editor operations: version and
advanced-help probes; three project inventories; the two deterministic JSON
round trips; first and repeated existing-animation imports; one successful
new-animation import; one isolated duplicate-new-animation collision control
and its diagnostic export; and one final missing-`./images` negative control.
Removing, adding, reordering, relabelling, or rebinding any operation invalidates
the run. A generic fixture rehearsal can exercise the machinery but cannot be
converted into representative gate evidence.

The collision control uses a separate disposable project that already
contains the submitted new animation. It must prove the observed Spine 4.3.23
hazard exactly: a repeated no-`--replace` import may exit zero, report a
requested-name-to-renamed-name collision, and add the renamed duplicate. That
diagnostic is accepted only as the expected result of this negative-control
operation. The ordinary new-animation import must remain diagnostic-free, and
its clean candidate and first export are never reused by the collision control.

On 2026-08-08, a licensed generic calibration reached the old repeated-new
slot and confirmed this hazard: Spine 4.3.23 exited zero, reported
`gesture -> gesture2`, and exported both animations. The run stopped before
publication because the then-current success-only publisher could not emit a
failure matrix. This calibration is not representative downstream-project
evidence and cannot pass either Phase 0 gate. The corrected runner must publish
a fresh controlled-failure report for equivalent failures before the generic
rehearsal is repeated.

All editor work occurs in one fresh owner-private run directory. Preparation
stages immutable package copies, two explicit current-derived candidates, an
isolated duplicate-collision copy, the missing-path-control copy, fixed output
slots, and the checked-in export preset; then the workspace is sealed. Each
command must consume a staged file or a verified output from an earlier
successful operation and may mutate only its exact declared slot.
Descriptor-relative snapshots bind file identity, mode, owner, link count,
timestamps, size, and digest before and after every call.
Hard links, path aliases, between-operation edits, undeclared files, and
same-byte replacements fail closed. Inputs are bounded by fixed depth, entry,
per-file, total-byte, process-time, and transcript-size limits. The three
original packages are rechecked after the final editor operation.

This boundary does not claim kernel-level isolation from malicious code already
running as the same operating-system user. In particular, a temporary file
created and removed entirely during an editor call may be unobservable. The
licensed editor and host user are trusted; all persistent pre-call, post-call,
and between-call state is audited.

Evidence format v4 identifies every artifact by the full
`role + portable path + SHA-256` triple. Equal empty transcripts are valid when
their paths differ. Assertions and processes cite exact identities, and a
fresh `0700` evidence directory receives create-only `0600` artifacts only
after a complete privacy and integrity preflight. `report.json` is published
last. Any unhidden `Licensed to:` text blocks publication without echoing the
sensitive line.

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
- repeating an existing-animation import with explicit `--replace` is
  idempotent;
- a new-animation import without replacement is single-apply, while the
  isolated duplicate-name control proves and records its unsafe retry
  behavior;
- warnings, partial outputs, timeouts, and nonzero exits fail the operation;
- the current source package is byte-for-byte unchanged.

### Phase 0B: consolidated runtime and viewer gate

After the Phase 1 shared viewer exists on Bevy 0.18.1, run a lightweight
rehearsal covering feature inventory, representative native load, browser
load, and semantic parity. Fix any failure before migrating. Run the one
authoritative complete matrix below after the Phase 2 Bevy 0.19 migration.

The resulting candidate runtime bundle must:

- load through native Spinal without fallback validation;
- decode every atlas page and required texture;
- evaluate every changed animation at key times and bounded samples;
- expose no blocking unsupported feature used by the representative project;
- produce the same semantic draw stream natively and in WASM;
- render correctly in a browser canvas;
- match small, tolerant framebuffer or screenshot references at meaningful
  timestamps in both native and browser hosts;
- support selection, skins, playback, looping, speed, seek, stepping, and fit;
- match known bone transforms and active attachments at selected timestamps.

Validation covers both Current and Proposed. Parser errors, degraded features,
unsupported features used by either bundle, missing pages, texture decode
failures, and evaluation failures are blocking. Any nonblocking informational
diagnostic must be explicitly enumerated by stable code.

Native/WASM agreement is parity evidence, not a correctness oracle. Before the
gate runs, check in an evidence specification naming the sample schedule,
semantic draw fields, numeric and pixel tolerances, browser/GPU profile, and
reference provenance. Use project-owned analytical fixtures for independently
known bone transforms, attachments, slot order, vertices, colors, and events;
use images rendered by licensed Spine 4.3.23 at recorded timestamps as the
independent appearance oracle for the representative private project. Spinal's
own output can never generate its expected result.

### Go/no-go decisions

Any unexplained semantic change, ignored warning, missing target, false-green
validation, or source mutation fails Phase 0A and stops mutation work for
review. Any unsupported required feature or native/WASM semantic mismatch fails
the rehearsal or Phase 0B and stops later work for review. Only the complete
post-migration Phase 0B result unlocks coordinator and review work. There is no
automatic fallback to another runtime, GUI automation, FFmpeg, or LLM editing.

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
- visible Primary and Comparison labels, replaced by Current and Proposed in a
  Review session;
- both durations shown when they differ;
- side-by-side initially, with wipe or overlay deferred until needed.

### UI model

Use one document-centric shell:

- **Open** is a command and empty state;
- **Preview** is the default workspace;
- **Compare** appears when a second source is present;
- **Diagnostics** is a contextual inspector/drawer;
- the first inspector is limited to concise inventory and diagnostics;
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
- Keep one browser rendering backend in the first supported profile. WebGL2 is
  the current proven target; record and test the exact desktop browser matrix
  before Phase 0B is accepted. Reconsider WebGPU only as a separate,
  evidence-backed migration rather than maintaining two default builds.
- Keep browser WASM single-threaded until profiling proves otherwise.
- Run the authoritative complete Phase 0B native/WASM evidence matrix on Bevy
  0.19. Phase 3 remains blocked until it passes; the Bevy 0.18.1 rehearsal does
  not carry across automatically.

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
flow. The project owner supplies a full package containing exactly one `.spine`
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
reviewer selects an exact immutable Base version. Spinal binds that Base
version and digest before analysis, supplies only Base's immutable asset
context when staging a bare project, and records that the provenance was
reviewer-selected. Promotion is permitted only after all normal structural,
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
- new whole animation in Submission: allow it only when every reference
  resolves to an existing approved setup object and all structural, runtime,
  and review gates pass;
- current changed only: retain current;
- both changed different animations: combine automatically;
- both changed the same animation to the same resulting fingerprint: retain
  Current and record a convergent no-op;
- both changed the same animation to different resulting fingerprints: require
  a visual decision;
- deletion: reject in the first release profile.

A submission with no net animation change ends as **No changes** and creates no
new version.

### Candidate construction

Construct a candidate only from a copy of Current. Use the Phase-0-verified
Spine CLI import operation. Replace an existing animation only when its exact
name was authorized by analysis, whether conflict-free or explicitly resolved;
add an approved new animation without replacement mode.

Before adding a new animation, prove from the exact staged Current export that
its name is absent. Treat the editor mutation as non-repeatable: every attempt
starts from a fresh Current copy, and an interrupted, timed-out, or otherwise
uncertain attempt is discarded rather than retried in place. A recovered job
may repeat read-only analysis, but candidate construction receives a new
attempt identity and a new copy. After the import, require the exact authorized
animation-name set and per-animation fingerprints; any renamed duplicate or
collision diagnostic fails production even when Spine exits zero and writes a
project.

The exact mutation allowlist authorized by analysis contains safe
submission-only edits, approved new whole animations, and conflicts explicitly
resolved to **Use submission**. Replacement mode applies only to authorized
existing animation names; it is never blanket project replacement.

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
- never treats loading, selecting, or merely playing an animation as review;
  the reviewer explicitly acknowledges it after successful rendering;
- shows progress such as `2 of 3 reviewed`;
- shows new diagnostics relative to Current;
- keeps approval disabled until every changed animation is reviewed and all
  gates pass;
- names the exact version promoted by the approval action.

Each acknowledgment is bound to the exact Proposed package digest and its
diagnostic result. Rebuilding or changing Proposed invalidates every earlier
acknowledgment.

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
- accepts opaque package and job IDs plus bounded uploads, never server
  filesystem paths or raw Spine CLI arguments;
- keeps the session capability out of URLs, logs, and referrers;
- validates Host and Origin;
- uses SameSite cookies and CSRF protection for mutations;
- permits no wildcard CORS;
- sets CSP, frame, MIME, and referrer protections;
- has no configuration path that silently turns local mode into remote mode.

The standalone WASM build compile-time excludes coordinator, SQLite,
filesystem-mutation, and process-launch dependencies.

After the limited production beta has passed its ten-handoff evidence gate,
add `spinal merge ...` for headless candidate construction. It uses the same
analysis and validation policy as the interactive workflow. Successful merge
still does not imply promotion, and unattended promotion remains deferred.

## Reliability and security

- Hold one OS-level lock for a state root.
- Keep that coordinator-lifetime state-root ownership lock distinct from the
  per-call lock that serializes Spine CLI operations.
- Serialize Spine CLI operations unless licensing and concurrency are proven.
- Run jobs outside UI/request threads with durable progress states.
- Make every phase idempotent or explicitly non-repeatable. In particular,
  never retry a new-animation editor mutation on the same candidate; discard
  uncertain output and rebuild from a freshly verified Current copy.
- Recover interrupted jobs without changing Current.
- Use per-job temporary directories, bounded output, minimal environment,
  closed stdin, cancellation, timeout, and process-tree termination.
- Validate the approved Spine executable and exact version before every job
  session.
- Persist `cleanup uncertain` before releasing control after incomplete
  process-tree termination. On restart, prove the recorded process and process
  group are gone or require an explicit safe-recovery action before launching
  Spine again; an in-memory poison flag is insufficient.
- Stage proposed artifacts on the same filesystem, validate and hash them,
  flush files and directories, atomically rename them into durable storage,
  then use one SQLite compare-and-swap transaction to advance Current only if
  its expected digest is still current.
- Reconcile every crash point at startup: incomplete staging, durable orphan,
  committed database row without a pointer, pointer without a complete row,
  and interrupted cleanup. Orphan deletion is bounded and audited.
- Restoring an older immutable version is an audited forward operation; it
  creates a new version/current decision and never mutates or erases history.
- Version the state schema. Before any migration, create and verify a restorable
  backup; test both forward migration and backup restore before promotion beta.
- Cancellation is state-specific: read-only analysis may stop cleanly; a CLI
  cancellation must complete process cleanup or enter `cleanup uncertain`;
  cancellation is disabled once the atomic promotion commit begins.
- Every failed workflow outcome states whether Current changed, whether another
  attempt is safe, what evidence was retained, and exactly one state-specific
  next action. Mutation failures never offer a generic **Retry** action.
- `cleanup uncertain` disables all further Spine work until the recorded
  process state is verified and the explicit recovery action completes.
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
- crash injection at every staging/promotion boundary, startup reconciliation,
  forward restore, double-submit, double-merge, and double-promotion;
- semantic draw-stream snapshots at a small set of meaningful timestamps;
- tolerant native and browser pixel references at the same timestamps;
- browser build and real-browser presented-pixel smoke;
- keyboard and accessible-name checks for the critical review path.

Before stable-viewer acceptance, document and run the critical-path
accessibility matrix: keyboard-only use, visible focus, 200% and 400% browser
zoom/reflow, reduced motion, contrast and non-color status, and screen-reader
operation with the supported browser and native AccessKit host. This is the
evidence behind the WCAG 2.2 AA target; passing unit smoke tests alone is not.

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

A qualifying beta handoff starts from an immutable version, completes the full
analysis/review/promotion or explicit no-change path, and retains its evidence
record. Across the ten-run sequence, the suite must cover a new animation, an
existing-animation replacement, disjoint parallel edits, a same-animation
conflict, and a mandatory crash-recovery drill. Corruption, lost work,
false-green validation, unrecoverable interruption, or a policy-changing fix
resets the consecutive count after the fix is verified; ordinary rejected bad
input does not.

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

1. The project owner sends the latest versioned package.
2. The animator opens it in Spine 4.3.23.
3. The animator changes only assigned animations.
4. The animator does not change setup, bones, slots, skins, attachments,
   constraints, or assets.
5. The animator returns the `.spine` file or package and lists changed
   animations.
6. The reviewer submits it to Spinal, resolves conflicts visually, reviews
   every changed animation, and approves the proposed version.
7. Spinal produces the next complete versioned package.

If operational project files are version-controlled separately, use Git LFS
for `.spine`, archives, images, source artwork, audio, and other large binary
assets. Never text-merge `.spine` files.

## Implementation order

```text
0A. Strict Spine CLI, round-trip, and animation-import evidence gate
1. One shared viewer/session and single-clock Compare on Bevy 0.18.1
0B-rehearsal. Lightweight native/WASM feature and parity check on Bevy 0.18.1
2. Whole-workspace migration to Bevy 0.19
0B. Authoritative native/WASM correctness and viewer evidence gate
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

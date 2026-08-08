# Spinal Application Consolidation

**Viewer, Review, and Animation Handoff Plan**

Plan status: approved for staged implementation
Evidence status: Phase 0 has not passed; mutation remains blocked
Primary compatibility target: Spine 4.3.23 JSON plus text atlases
Release target: none; open-source release work is intentionally deferred

## Current gate state

| Area | State |
| --- | --- |
| Phase 0A evidence harness | Implementation complete; controlled-failure and licensed generic rehearsals passed |
| Phase 0A generic calibration | **PASS (NON-REPRESENTATIVE)** at `2a68e1f`; 25 of 25 assertions passed |
| Phase 0A representative run | **NOT RUN** |
| Shared viewer | Unified native/browser Preview and Compare, synchronized skin controls, contextual Diagnostics, and linked camera interaction complete; formal accessibility evidence remains |
| Phase 0B semantic foundation | Authenticated bundles, shared v1 contract, strict comparison, native capture, and identity-bound opt-in browser capture exist; the checked-in generic case remains **NOT RUN** and gate-ineligible |
| Phase 0B representative correctness matrix | **NOT RUN**; no representative evidence or pass is claimed |
| Mutation and promotion | Blocked by representative Phase 0A and Phase 0B |

## Document authority

This plan owns product boundaries, phase order, gate consequences, and the
decision to proceed, stop, or simplify. Linked tool documentation and runbooks
own executable mechanics, evidence formats, recovery procedures, and historical
logs. A tool exit code or report cannot change a gate state by itself.

- [Phase 0 Evidence Runbook](docs/PHASE-0-EVIDENCE-RUNBOOK.md)
- [Coordinator Recovery Runbook](docs/COORDINATOR-RECOVERY-RUNBOOK.md)
- [Parked Open-Source Release Notes](docs/PARKED-OPEN-SOURCE-RELEASE-NOTES.md)

Private project fixtures, licensed-editor evidence, operational packages, and
uploads stay outside this repository.

## Name and product promise

**Spinal** is the only product name. This initiative is **Spinal Application
Consolidation**, not Spinal Collab, Studio, or Workbench.

> Preview exactly what Spinal will render, diagnose incompatibilities, compare
> revisions, and safely accept animation-only updates.

User-facing terms are:

- **Preview**: view one export;
- **Compare**: view two revisions with one synchronized clock;
- **Diagnostics**: compatibility and runtime findings;
- **Review**: inspect a submitted animation update;
- **Current version**, **submission**, and **proposed version** instead of
  master, handoff, and candidate where plain language is clearer.

`coordinator` is an internal capability term. Version terms are:

- **Base**: immutable Spinal version from which the animator started;
- **Current**: latest approved immutable project version;
- **Submission**: animator's returned project or package;
- **Proposed**: validated Current copy containing approved whole-animation
  changes, before promotion.

## Who uses Spinal in v1

The v1 project owner is also reviewer and coordinator. The animator does not
need Spinal; they need only the approved Spine 4.3.23 editor and the package the
owner supplies.

The complete human journey is:

1. The owner creates a project from one complete version package.
2. The owner sends the latest versioned package to an animator.
3. The animator changes only assigned animations and returns a `.spine` file or
   submission ZIP. Their changed-animation list is useful but advisory.
4. The owner opens the native app or starts the local browser workspace,
   submits the return, and selects its immutable Base when a manifest does not
   prove one.
5. Spinal analyses Base, Current, and Submission and explains safe changes,
   conflicts, or a blocking failure without changing Current.
6. The owner resolves same-animation conflicts visually, builds Proposed from a
   copy of Current, and reviews every changed animation.
7. The owner explicitly approves the digest-bound Proposed version.
8. Spinal advances Current atomically and provides the next complete package.

## Capability map

Native and local-browser hosts are coequal ways to use the same local workflow.
The browser workspace works only while its foreground local Spinal host runs.

| Surface | Viewer capability | Handoff capability | Requirement |
| --- | --- | --- | --- |
| Native app | Preview and synchronized Compare | Full local workflow after evidence gates | Spinal; licensed Spine 4.3.23 only for merge work |
| Local browser workspace | Same Preview, Compare, diagnostics, conflict, and Review model | Same upload-to-approval workflow while `spinal serve` runs | Foreground loopback host; licensed Spine 4.3.23 only for merge work |
| Standalone WASM | Preview and Compare only | None; no coordinator, process launch, SQLite, or promotion | Supported browser WASM/WebGL2 profile |
| Headless CLI | Deterministic read-only `spinal check` | Candidate construction only after handoff beta; never approval | No editor for check; licensed Spine 4.3.23 for later construction |

Three headless promises are fixed:

1. `spinal check` is fully headless, read-only, deterministic, and part of the
   stable viewer capability.
2. Headless candidate construction arrives only after interactive handoff beta.
3. A headless command never implies approval or performs unattended promotion.

## Package vocabulary

- A **version package** is a complete ZIP with exactly one `.spine` project,
  required assets and directories, and `spinal-project.json`.
- A **submission** is one bare `.spine` file or a ZIP with exactly one `.spine`
  project. A missing manifest requires the owner to select Base.
- Every version download is a complete ZIP, never only a changed `.spine` file.
- A returned ZIP contains the exact provenance manifest for that immutable
  version.
- The animator's change list is advisory. Three-way fingerprints and structural
  comparison are authoritative.

## User-visible acceptance stories

- Open one supported export and control animation, skin, playback, time, and
  camera without installing Spine.
- Compare two exports with one clock and clear per-pane state.
- Submit disjoint animation changes and reach validated Proposed without manual
  merge editing.
- See a same-animation conflict in synchronized comparison and make one explicit
  whole-animation decision.
- Receive an actionable failure for setup or asset changes with Current
  unchanged.
- Close and reopen during interruption with Current unchanged and one clear
  recovery action; a one-shot workflow safely restarts from immutable inputs.
- Approve fully reviewed Proposed and download the next complete package without
  re-uploading it.
- Run `spinal check` without a UI and receive stable exit codes and JSON without
  constructing or promoting a version.

## Architecture

Spinal is one repository and one product with narrow internal boundaries:

```text
spinal/
├── spinal/                 renderer-independent runtime library
├── bevy_spinal/            Bevy asset, ECS, and rendering adapter
└── apps/
    └── spinal/             package `spinal-app`, binary `spinal`
        ├── shared viewer session and UI model
        ├── native host
        ├── browser/WASM host
        └── optional native-only coordinator capability
```

There is no separate viewer-core crate, duplicate WASM viewer, or public
`spinal_collab` service. The former viewer implementation belongs in
`apps/spinal`.

`bevy_spinal/examples/runtime_showcase.rs` remains an adapter and conformance
example only. Product session state, Review workflow, browser hosting,
coordinator policy, and viewer development stay in `apps/spinal`.

The coordinator is a capability boundary because browser WASM cannot execute
Spine, use arbitrary filesystem paths, or own local state. It is not necessarily
a process boundary:

- native calls it in-process;
- `spinal serve` exposes it through a protected loopback host;
- standalone WASM remains viewer-only;
- no daemon, login item, permanent port, or cloud service is installed.

The Python/FastAPI prototype is characterization evidence, not the product
boundary. The production target is generic Rust inside `apps/spinal`, and its
port does not begin before both Phase 0 gates pass.

## Principles and scope

1. Evidence precedes mutation UI.
2. Missing validators, warnings, stale versions, ambiguous skeletons, unsupported
   features, and unverified provenance fail closed.
3. Native and browser share session, transport, renderer, commands, diagnostics,
   and review state.
4. Compare uses one Bevy app, two instances, and one authoritative clock.
5. `.spine` remains canonical. JSON is runtime data, evidence, and interchange,
   never a text-merged source of truth.
6. Whole animations are the only merge unit; timelines and keyframes are not.
7. Spinal contains no downstream names, paths, branding, IDs, or project policy.
8. Preview and Compare require no editor; merge requires licensed Spine 4.3.23.
9. Unsupported behavior is explicit; plausible incorrect rendering is failure.
10. Read-only capabilities stabilize before mutation, promotion, or authoring.

Stable viewer includes Preview, synchronized Compare, controls, concise
diagnostics, deterministic `spinal check`, generic fixtures, and private
consumer acceptance. Handoff beta adds local intake, immutable packages,
whole-animation three-way analysis, visual conflict decisions, fresh-copy CLI
construction, explicit Review, and atomic promotion.

Deferred are cloud/remote collaboration; accounts and teams; LLM or text merge;
timeline, setup, rig, skin, constraint, or asset merging; FFmpeg/video preview;
animation authoring and deep inspectors; plugin/policy frameworks; public
release work; and unattended promotion.

## Phase 0: representative go/no-go gates

Phase 0A proves licensed-editor round-trip and whole-animation import. Phase 0B
proves the consolidated native/browser runtime renders the representative
project correctly. Phase 1 read-only work and Phase 2 migration may supply the
surface needed by Phase 0B, but Phase 3 waits for both representative reports.

### Executable representative path

| Gate | Owner | Runner/adapter | Evidence | Pass authority |
| --- | --- | --- | --- | --- |
| Phase 0A | Owner/reviewer with activated 4.3.23 seat and private Current, replacement Submission, and new-animation Submission | **Not yet implemented:** a closed representative entry point and envelope in `tools/spinal-phase0a`, reusing the frozen operation primitives while binding those three exact packages; the existing generic binary is permanently gate-ineligible | Versioned representative matrix, transcripts, semantic diffs, digests, provenance, source-integrity proof in private storage | Maintainer/reviewer inspects a fresh report and records PASS here |
| Phase 0B | Owner/reviewer with private Current/Proposed and independent references | Reviewed `tools/spinal-phase0b` runner, native capture, browser/WASM host after Bevy 0.19 | Versioned matrix binding semantic frames, events, pixels, diagnostics, toolchains, browser/GPU, and reference provenance | Maintainer/reviewer records PASS only when every assertion passes |

Implement and review the closed Phase 0A representative adapter before its
run; generic calibration cannot be relabelled. After Phase 0A passes, the owner
may construct one private, disposable, non-promotable Proposed copy from fresh
Current through the proven import recipe solely as Phase 0B input. After
migration, the representative Phase 0B matrix runs on those exact Current and
Proposed bundles. Both reports must be fresh, complete, private, and PASS before
Phase 3A.

### Phase 0A required result

The fixed runner proves:

- approved launcher, activation, exact `--update 4.3.23`, advanced arguments,
  and exact skeleton selection;
- deterministic pretty/nonessential JSON export/import/export, narrow volatile
  allowlist, and recorded losses;
- replacement of one existing animation and addition of one new animation;
- unchanged setup, skins, attachments, constraints, assets, and unselected
  animations;
- semantic idempotence for replacement and an isolated duplicate-new-animation
  hazard control;
- fail-closed warnings, missing paths, partial output, timeout, and nonzero exit;
- native validator success and byte preservation of original packages.

Production never reconstructs Current from JSON. Proposed always starts as a
fresh Current copy. Generic calibration cannot become representative evidence.

`tools/spinal-phase0a` is an internal unshipped harness, not product CLI or job
framework. Product code may reuse proven primitives, not its fixed evidence
recipe without separate justification.

### Phase 0B required result

After a lightweight Bevy 0.18.1 rehearsal, run one authoritative matrix on Bevy
0.19. Current and Proposed must:

- load through native and browser/WASM without fallback;
- decode every atlas page/texture and expose no blocking unsupported feature;
- evaluate changed animations at fixed meaningful samples;
- produce matching complete semantic frames/events across hosts;
- match independently known bones, attachments, slot order, vertices, colors,
  events, and tolerant browser pixels;
- render in a real browser and support all Review controls.

Parity is not correctness. Semantic expectations come from project-owned
analytical references; appearance references come from licensed Spine 4.3.23
renders. Spinal never generates its own expected result.

Implementation checkpoint, 2026-08-09: the case loader authenticates its
semantic inputs and isolated Current/Proposed runtime bundles; one shared v1
contract drives strict comparison and the exact native/browser schedule. The
low-level native capture preflights both assets, while the opt-in browser path
binds observations to both runtime manifest/content digests and rejects
external commands. The checked-in generic 0.18.1 case remains `not_run`,
permanently `gate_eligible = false`, without fixtures or references. There is
no identity-bound two-host owner runner, event/pixel comparison, evidence
publisher, or representative matrix. This is foundation, not evidence or PASS.

Any unexplained semantic change, ignored warning, missing target, source
mutation, false green, unsupported required feature, self-generated oracle, or
native/WASM mismatch stops mutation work. There is no fallback to another
runtime, GUI automation, FFmpeg, or LLM editing.

Mechanics, evidence format, staging rules, and calibration history are in the
[Phase 0 Evidence Runbook](docs/PHASE-0-EVIDENCE-RUNBOOK.md).

## Phase 1: stable shared viewer and check

### Implemented checkpoint

As of 2026-08-09, the Bevy 0.18.1 checkpoint includes:

- one immutable bundle/session/clock/runtime/command/camera path across hosts;
- one-source Preview and two-layer/two-viewport single-app Compare;
- digest-pinned browser manifests, bounded same-origin loading, exact lengths
  and hashes, redirect rejection, and a launch deadline;
- shared animation/transport/seek/step/restart/fit behavior;
- fail-closed per-source state and explicit one-sided setup-pose messaging;
- one shared, versioned source-inspection model with bundle identity, bounded
  inventory, canonical stable-name diagnostics, and no host-path leakage;
- deterministic, fully headless, read-only `spinal check` with human and JSON
  output plus fixed compatible/degraded/usage/source/internal exit statuses;
- pre-page-I/O bundle file-count limits, bounded reports, actionable path-free
  source codes, and exact-byte compatible/degraded v1 schema fixtures;
- one contextual read-only Diagnostics surface across native and browser hosts,
  populated from the same immutable inspection as `spinal check`, with bounded
  visible findings, explicit omissions, semantic HTML/AccessKit labels, and no
  commands or workflow state;
- one bounded camera state shared by Preview and both Compare panes, with
  pointer-anchored zoom, drag/touch pan, scoped keyboard controls, and one
  recoverable **Fit view** action;
- one union fit for Current and Proposed geometry, applied with an identical
  world-to-screen mapping so camera normalization cannot hide a difference;
- real-browser presented-pixel/render-isolation smoke, linked camera mutation
  and Fit-recovery proof, and accessible camera state labels.

Synchronized skin selection, synthetic Default/named semantics, per-source
presence and fallback messaging, skin-aware fit, and accessible native/browser
skin controls are implemented. The inspection/check foundation is implemented;
the visible Diagnostics surface and camera interaction are implemented.
Formal accessibility acceptance evidence remains. These are generic
implementation checks, not Phase 0B evidence.

### Shared viewer contract

- Immutable `SourceBundle`, host-neutral `ViewerSession`, exact `ReviewClock`,
  commands, and canonical parity snapshots are shared.
- Source slots are Primary and optional Comparison; animation identity is name.
- Compare waits for all sources and advances both from one Bevy delta.
- Browser messages are versioned and validate origin/source/capability.
- Native paths, browser files, ZIPs, embedded assets, and coordinator URLs are
  thin adapters into the same bundle.
- Compare is one renderer and clock, never two iframes.

Use one document shell: **Open**, default **Preview**, conditional **Compare**,
contextual **Diagnostics**, workflow-only **Review**, transport under the
canvas, and side-by-side first. Do not create mini-apps or five-mode navigation.

The read-only `spinal check` foundation ships before coordinator work. It
shares loading, validation, inventory, and stable diagnostics, creates no
project or Proposed, opens no window, and offers deterministic exit codes and
optional JSON.

### Accessibility boundary

- Native uses Bevy/AccessKit; browser uses semantic HTML around shared canvas.
- Workflow actions are keyboard-operable with visible focus.
- Canvas has an accessible state summary and structured inspector.
- Content starts paused; playback is explicit and frames are not announced.
- State is not color-only; reduced motion disables incidental motion/flicker.
- Errors/conflicts receive focus and async work uses a restrained live region.

Target WCAG 2.2 AA for chrome/workflow. Visual motion approval still requires a
qualified visual reviewer or agreed accommodation; diagnostics do not replace
that judgment. Thin host shells may differ, shared product logic may not.

## Phase 2: Bevy 0.19 migration

Consolidate on 0.18.1, then upgrade the whole workspace separately.

- Never support both Bevy versions on one branch.
- Raise MSRV/CI from Rust 1.89 to 1.95 with the migration.
- Keep adapter dependencies/features minimal and target-specific.
- Build native and WASM after every later change.
- Keep WebGL2 first and WASM single-threaded; reconsider WebGPU separately.
- Record exact toolchains/browser/GPU in evidence.
- Run full representative Phase 0B after migration; rehearsal does not carry.

## Phase 3: generic coordinator capability

Phase 3A starts only after fresh representative Phase 0A and 0B PASS entries.
Begin with one guided, no-conflict slice in native and local browser: three
manifest packages, authorized imports, fail-closed validation, digest-bound
Ready Proposed, and no promotion. Retain Python characterization tests while
generic Rust replaces the prototype.

### Native and browser hosts

Native and `spinal serve` are coequal local surfaces. `spinal serve` ships with
the first browser upload/Review flow and:

- selects ephemeral loopback only, starts one-origin UI/API, waits for readiness,
  then opens the browser;
- prints a clickable fallback URL, stays foreground, and leaves no daemon;
- ends its local session when closed;
- keeps per-launch capability out of URLs/logs/referrers;
- accepts bounded uploads/opaque IDs, never paths or raw CLI arguments;
- validates Host/Origin and uses SameSite, CSRF, no wildcard CORS, CSP, frame,
  MIME, and referrer protections;
- cannot silently become remote.

Standalone WASM compile-time excludes coordinator, SQLite, filesystem mutation,
and process launch.

### Packages and intake

**Create project** requires a complete version package. The owner resolves an
ambiguous skeleton and approves generated identity. Spinal validates and
immutably snapshots it before setting Current; bare `.spine` cannot bootstrap.

Immutable records cover project, versions, Submission, analysis, conflicts,
Proposed/runtime artifacts, validation, Review, promotion, and restore.

`spinal-project.json` records schema/project/Base, target skeleton, editor
version, source SHA-256, deterministic payload digest, and export profile. The
payload inventory covers paths, required empty directories, lengths, and file
digests without recursively hashing its own field. Archive bytes get a separate
external hash. Empty asset directories survive every stage.

Bare/unmanifested Submission requires explicit immutable Base. It may borrow
only Base asset context; included assets must equal Base.

ZIP intake rejects traversal, absolute paths, links/special files, encryption,
duplicate/portable-colliding names, multiple projects, and configured entry,
depth, name, byte, pixel, and disk limits. Streaming counts actual decompressed
bytes. Finder metadata is ignored; required empty directories are retained.

### Three-way analysis

For Base, Current, Submission: export normalized JSON, fingerprint setup and
each animation, compare assets/provenance, and reject setup, asset, deletion,
unsupported-feature, or version changes.

- Submission-only change: use Submission.
- New animation: allow only with resolved approved setup references and all
  structural/runtime/Review gates.
- Current-only change: retain Current.
- Different animations changed: combine.
- Same animation/same result: retain Current as convergent no-op.
- Same animation/different result: require visual choice.
- Deletion: reject v1.

No net change ends **No changes** and creates no version.

### Proposed construction

Start only from fresh Current and use the Phase-0-proven import. Replacement is
per authorized existing name; new names use add mode. Prove a new name absent
before import. Mutation is non-repeatable: interruption, uncertainty, or
collision discards the copy; every attempt gets fresh Current and identity.
Renamed duplicates fail even on exit zero.

After import, repeat structure/assets/fingerprints, prove only authorization
changed, export Review runtime, run native load/decode/evaluation/diagnostics,
and fail warnings, timeout, nonzero, missing validator, degradation, version
mismatch, or stale Current. All decisions bind exact package digests.

### Post-3A durability decision

Default to guided one-shot with immutable inputs, fresh-copy mutation,
digest-bound Proposed, and safe restart. After production-like 3A runs, record
whether duration, interruption, or concurrency warrants durable jobs. If yes,
Phase 3B implements the full recovery model before beta; if no, retain only the
audit and atomic state correctness needs. See the
[Coordinator Recovery Runbook](docs/COORDINATOR-RECOVERY-RUNBOOK.md).

## Phase 4: Review and promotion

Same-animation conflict is visual, never text-only. Show synchronized comparison
and offer **Keep current**, **Use submission**, plus one global **Reject
submission** action.

Post-build Review compares Current/Proposed, defaults to changed animations,
navigates changes, requires explicit acknowledgment after successful rendering,
shows progress/new diagnostics, and blocks approval until every changed
animation and gate passes. Loading or playing is not acknowledgment. Review
binds Proposed digest and invalidates on rebuild.

Promotion rechecks Current, makes artifacts durable, atomically advances the
pointer, and offers complete Current/next handoff ZIP without re-upload.

### Failure map

| Situation | Current changed? | Safe to continue? | One next action |
| --- | --- | --- | --- |
| Invalid ZIP | No | Yes, after correction | Fix/recreate package and resubmit |
| Wrong editor version | No | Yes, after re-export | Re-export with 4.3.23 |
| Forbidden setup/asset change | No | Yes, with corrected return | Request animation-only return |
| Same-animation conflict | No | Yes, after human choice | Review visually and choose |
| Viewer/unsupported feature | No | Yes, after resolution | Resolve named incompatibility, rebuild |
| Current advanced | No | Yes, after reanalysis | Analyse against new Current |
| Interrupted; cleanup confirmed | No | Yes, from fresh Current | Start fresh construction |
| Process state uncertain | No | No | Complete explicit cleanup recovery |

## Phase 5: private acceptance and limited beta

Run the qualifying handoffs described under Milestones with owner-controlled
packages and no public release promise. Record false greens, corruption, lost
work, recovery failures, and policy-changing failures before deciding that the
interactive workflow is stable enough for post-beta automation.

## Phase 6: post-beta headless construction

After ten qualifying handoffs, add `spinal merge ...`. It uses interactive
analysis/validation and emits digest-bound Proposed plus machine report with
stable codes. It never advances Current, approves, or implies Review. Unattended
promotion requires a separate policy, threat model, evidence gate, and review.

## Reliability and security outcomes

- Current never changes during intake, analysis, Review, construction,
  validation, cancellation, or failed recovery.
- Uncertain mutation is discarded and rebuilt from fresh Current.
- Spine work is serialized/bounded/cancellable and blocked after `cleanup
  uncertain` until explicit recovery.
- Approval is digest-bound; promotion uses durable same-filesystem staging and
  atomic stale-Current compare-and-swap.
- Restore is an audited forward version, never history mutation.
- Private paths, assets, capabilities, and license material stay out of logs/Git.
- Public untrusted upload remains out of scope.

Detailed conditional recovery is in the
[Coordinator Recovery Runbook](docs/COORDINATOR-RECOVERY-RUNBOOK.md).

## Verification

Fast tests cover runtime behavior, diagnostics, fingerprints, three-way rules,
shared clock, ZIP/manifest/genericity, workflow state, and stale Current.

Integration covers identical native/WASM bundles, fake-Spine end-to-end flow,
semantic/pixel references, real-browser presented pixels, durability-selected
crash points, duplicate actions, keyboard names/operation, focus after
error/conflict, and restrained live progress.

Before stable viewer acceptance, run keyboard, visible focus, 200%/400%
zoom/reflow, reduced motion, contrast/non-color status, and screen reader tests
in supported browser and AccessKit host. This supports WCAG AA chrome/workflow,
not a claim to replace human visual judgment.

Activated acceptance proves exact-version capability, round-trip allowlist,
new/existing fingerprints, unchanged setup/assets, native/WASM load, and
fail-closed warning/timeout/partial/nonzero behavior. Spinal owns a
redistributable generic fixture; consumers keep private representative fixtures,
licensed renders, and evidence outside Git.

## Milestones and pivots

- **Stable viewer:** Preview, Compare, Diagnostics, and check are trustworthy.
- **Handoff beta:** experimental until ten consecutive qualifying handoffs have
  no corruption, lost work, false green, or unrecoverable interruption.
- **Automation:** check precedes merge; unattended promotion stays deferred.

Ten runs cover new animation, replacement, disjoint edits, same-name conflict,
and required recovery drill. A policy-changing failure resets the count after
repair; expected bad-input rejection does not.

Stop merge for unexplained Phase 0 change. Reconsider animation-only if more
than two of ten real returns need forbidden changes. Keep native primary if host
sharing requires duplication. Keep one-shot unless 3A proves queue need. Add no
authoring, remote collaboration, or generalized policy without repeated demand.

## Working practice

1. Owner sends latest version ZIP.
2. Animator opens it in Spine 4.3.23.
3. Animator changes assigned animations only, never setup/rig/skins/assets.
4. Animator returns `.spine` or ZIP plus advisory change list.
5. Owner submits, resolves conflicts, reviews every change, and approves.
6. Spinal produces the next complete version ZIP.

Use Git LFS for operational `.spine`, archives, images, artwork, audio, and
large binaries if separately version-controlled. Never text-merge `.spine`.

## Implementation order

```text
0A. Representative CLI round-trip and animation-import gate
1.  Shared viewer/Compare/check on Bevy 0.18.1
0B-rehearsal. Non-representative native/WASM parity on 0.18.1
2.  Whole-workspace migration to Bevy 0.19
0B. Representative native/WASM correctness gate
3A. Guided no-conflict native/browser slice with thin spinal serve
3B. Evidence-based durable-job decision; implement only when yes
4.  Visual conflict Review, acknowledgments, atomic promotion
5.  Private acceptance and limited beta
6.  Headless construction after beta
```

Do not start Phase 3 until representative Phase 0A/0B PASS is recorded here.

## Final disposition

Approved with real stop/go gates, one consolidated viewer, generic policy, one
renderer/clock, fail-closed mutation, coequal native/local-browser UX, optional
on-demand privilege, one-shot default, and deferred authoring/remote/release.

The KISS boundary is one product, two runtime libraries, one application, one
optional privileged capability, one whole-animation rule, and no infrastructure
before evidence demonstrates need.

Open-source observations remain in
[Parked Open-Source Release Notes](docs/PARKED-OPEN-SOURCE-RELEASE-NOTES.md),
without release date or stability promise.

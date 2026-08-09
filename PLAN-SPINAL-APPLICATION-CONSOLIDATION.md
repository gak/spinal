# Spinal Application Consolidation

**Viewer, Review, and Animation Handoff Plan**

Plan status: approved for staged implementation
Evidence status: Phase 0 has not passed; mutation remains blocked
Primary compatibility target: Spine 4.3.23 JSON plus text atlases
Release target: none; open-source release work is intentionally deferred

## Current gate state

| Area | State |
| --- | --- |
| Phase 0A harness | Generic controlled-failure and licensed calibration are complete; the closed representative adapter, binding, outer publisher, and read-only verifier were implemented and reviewed at `b229339` |
| Phase 0A generic calibration | **PASS (NON-REPRESENTATIVE)** at `2a68e1f`; 25 of 25 assertions passed |
| Phase 0A representative run | **NOT RUN** |
| Runtime baseline | Whole-workspace Bevy 0.19.0, AccessKit 0.24.1, glam 0.32.1, and Rust 1.95 migration recorded at `07af12d`; the complete runnable local native/WASM/production-Chrome matrix passes, while configured CI/platform results remain pending |
| Shared viewer | Launch-only Open, Preview, Compare, Diagnostics, linked camera interaction, and native/browser transport parity are implemented; the `228f757` automated accessibility PRE-FLIGHT **PASS** covers the current Open/taxonomy surface, named human native/browser keyboard and VoiceOver review remains **NOT RUN**, and acceptance remains **INCOMPLETE** |
| Phase 0B semantic foundation | Authenticated bundles, shared v1 contract, identity-bound native semantic/event capture, fresh-nonce browser semantic/pixel capture with source-positioned outer-v3 event windows, strict host parsing, semantic/event/pixel comparison primitives, and a strict generic browser/build/effective-GPU context receipt exist; the passing local real-Chrome smoke and receipt are self-authored, self-reported/context-only, and gate-ineligible, while the checked-in generic Bevy 0.18.1 case remains frozen, **NOT RUN**, and unusable as 0.19 evidence |
| Phase 0B representative correctness matrix | **NOT RUN**; no representative evidence or pass is claimed |
| Mutation and promotion | Blocked by representative Phase 0A and Phase 0B |

## Document authority

This plan owns product boundaries, phase order, gate consequences, and the
decision to proceed, stop, or simplify. Linked tool documentation and runbooks
own executable mechanics, evidence formats, recovery procedures, and historical
logs. A tool exit code or report cannot change a gate state by itself.

- [Phase 0 Evidence Runbook](docs/PHASE-0-EVIDENCE-RUNBOOK.md)
- [Accessibility Acceptance Runbook](docs/ACCESSIBILITY-ACCEPTANCE-RUNBOOK.md)
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
| Phase 0A | Owner/reviewer with activated 4.3.23 seat and private Current, replacement Submission, and new-animation Submission | **Implemented and reviewed at `b229339`:** a closed representative entry point, owner-private binding, format-v5 outer publisher, and read-only verifier in `tools/spinal-phase0a`; the inner format-v4 generic report remains unchanged and permanently gate-ineligible | Versioned representative matrix, transcripts, semantic diffs, digests, provenance, source-integrity proof in private storage | Maintainer/reviewer inspects and independently verifies a fresh report, then alone records PASS here |
| Phase 0B | Owner/reviewer with private Current/Proposed and independent references | **Foundation only:** shared contract, identity-bound native semantic/event capture, fresh-nonce browser semantic/pixel capture with source-positioned outer-v3 event windows, strict host parsing, event/pixel comparators, and a generic browser/build/effective-GPU context receipt exist; the representative private v2 case/policy, independent references, identity-bound owner runner, representative provenance bindings, publisher/verifier, and matrix remain unimplemented | Versioned matrix binding semantic frames, events, pixels, diagnostics, toolchains, browser/GPU, and reference provenance | Maintainer/reviewer records PASS only when every assertion passes |

The closed Phase 0A representative path passed implementation review at
`b229339`; generic calibration cannot be relabelled. The representative run
remains **NOT RUN** pending an eligible private three-package fixture. After
Phase 0A passes, the owner may construct one private,
disposable, non-promotable Proposed copy from fresh
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

The representative evidence is deliberately compositional. A format-v5 outer
report binds an exact owner-private binding and case, the three role-tagged
package trees, a clean source revision and exact `Cargo.lock`, and the exact
prebuilt representative runner. An eligible passing candidate requires every
one of the 22 inner process records to contain the hashed
representative-binding marker. The enclosed format-v4 core
retains `generic_rehearsal` scope and
`representative_gate_eligible: false`; it is never rewritten or relabelled.
It is produced fresh for that binding and cannot be substituted with a prior
generic rehearsal. Only the outer report can describe a representative
evidence candidate.

The independent verifier is read-only. It rechecks the closed layout, hashes,
inventories, identities, cross-links, and marker coverage. It does not rerun
Spine or the native validator, reclassify retained transcripts, or rederive the
semantic comparisons. A controlled-failure core always produces
generic-v4 diagnostics only; the representative destination remains
**UNPUBLISHED** and receives no outer format-v5 report. Even a successful
runner and verifier result is only a candidate: the maintainer/reviewer alone
may record Phase 0A PASS in this plan, and mutation stays locked until both
representative gates pass.

`tools/spinal-phase0a` is an internal unshipped harness, not product CLI or job
framework. Product code may reuse proven primitives, not its fixed evidence
recipe without separate justification.

### Phase 0B required result

The lightweight Bevy 0.18.1 rehearsal contract is frozen, was not run, and
carries no evidence forward. Run one authoritative matrix on Bevy 0.19.
Current and Proposed must:

- load through native and browser/WASM without fallback;
- decode every atlas page/texture and expose no blocking unsupported feature;
- evaluate changed animations at fixed meaningful samples;
- produce matching complete semantic frames/events across hosts;
- match independently known bones, attachments, slot order, vertices, colors,
  events, and tolerant browser pixels;
- render in a real browser and support all Preview/Compare controls.

Parity is not correctness. Semantic expectations come from project-owned
analytical references; appearance references come from licensed Spine 4.3.23
renders. Spinal never generates its own expected result.

Implementation checkpoint, 2026-08-09: the case loader authenticates its
semantic inputs and isolated Current/Proposed runtime bundles; one shared v1
contract drives the exact native/browser schedule and strict semantic, event,
and pixel comparison. Native semantic capture now constructs Bevy assets
directly from those retained bundles and preserves both manifest/content
identities. Bevy-authored events retain stable diagnostic codes. A fresh native
app also captures the fixed zero-to-one-second event window for both retained
bundles, validates playback and message identity at every deterministic step,
and produces strict event documents bound to the same bundle digests.

The additive generic Bevy 0.19 browser seam uses a fresh runner-generated
256-bit nonce retained independently of the driver. After readiness it creates
two hidden instances from the exact loaded Current/Proposed asset handles,
captures a fresh no-seek `sway`/Once event window through the inclusive
endpoint, and removes them before capturing
the four fixed samples in sample-major order. It isolates each full 640-by-480
presentation for two strict Bevy updates, waits through a CDP two-frame
compositor barrier, and retains eight exact original static RGB8/RGBA8 PNGs.
The outer version 3 document requires both strict event windows and binds each
screenshot receipt to its semantic frame, acknowledged play/seek generations,
and exact runtime identity. The strict Rust host parser also requires the
independently retained expected nonce and the already loaded bundle identities.
Pixel comparison normalizes RGB8/RGBA8 to RGBA in memory without replacing the
original PNGs.

The driver writes one final create-only generic provenance receipt after the
screenshots, terminal document, and capture manifest. It binds the runner nonce
and exact runtime identities to both capture artifacts, inventories the served
build, and records build/toolchain, CDP Browser/SystemInfo, and effective WebGL
context. The strict parser rejects noncanonical context or broken bindings; the
smoke independently rechecks the exact files, inventory, and private
permissions. This is `self_reported_context_not_binary_attestation`:
it does not attest the browser process/executable or toolchain distributions.

`just phase0b-browser-smoke 8427` passes locally against real headless Chrome
on the self-authored fixture. It requires neither FFmpeg nor ImageMagick. That
result is `non_representative_rehearsal`, always `gate_eligible = false`, and is
not an independent oracle, evidence, or PASS; configured CI results remain
pending. The checked-in generic 0.18.1 case remains frozen, `not_run`,
permanently gate-ineligible, and empty of fixtures/references. Independent
analytical/licensed-Spine references, a representative private v2 case/policy,
the identity-bound owner runner, representative provenance policy and bindings,
and the publisher/verifier remain. Representative Phase 0B is **NOT RUN** and
mutation remains locked.

Any unexplained semantic change, ignored warning, missing target, source
mutation, false green, unsupported required feature, self-generated oracle, or
native/WASM mismatch stops mutation work. There is no fallback to another
runtime, GUI automation, FFmpeg, or LLM editing.

Mechanics, evidence format, staging rules, and calibration history are in the
[Phase 0 Evidence Runbook](docs/PHASE-0-EVIDENCE-RUNBOOK.md).

## Phase 1: stable shared viewer and check

### Historical Bevy 0.18.1 implementation checkpoint

As of 2026-08-09, the historical Bevy 0.18.1 checkpoint includes:

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
- one union fit for Primary and Comparison geometry, applied with an identical
  world-to-screen mapping so camera normalization cannot hide a difference;
- real-browser presented-pixel/render-isolation smoke, linked camera mutation
  and Fit-recovery proof, and accessible camera state labels.

Synchronized skin selection, synthetic Default/named semantics, per-source
presence and fallback messaging, skin-aware fit, and accessible native/browser
skin controls are implemented. The inspection/check foundation is implemented;
the visible Diagnostics surface and camera interaction are implemented. The
automated accessibility pre-flight is recorded below; formal human acceptance
remains. These are generic implementation checks, not Phase 0B evidence.

Current Bevy 0.19 viewer parity checkpoint, 2026-08-09: native and browser now
expose loop, the fixed `0.25×`/`0.5×`/`1×`/`1.5×`/`2×` speed set, and absolute
timeline control through the same command, shared clock, and runtime-projected
state. Persistent browser identity is Preview or Compare; Review remains
workflow-only. These are generic implementation checks, not Phase 0A/0B
evidence.

Current read-only Open checkpoint, 2026-08-09: invoking native Spinal without
paths opens sequential single-file system pickers for a required Primary and,
only after Primary preflight succeeds, an optional Comparison. Cancelling
Primary exits without a window; cancelling Comparison launches paused Preview;
selecting two valid exports launches paused Compare. A picker or preflight error
aborts the launch with its Primary/Comparison role attributed and creates no
partial viewer. The manifest-free browser page exposes one required Primary and
one optional Comparison directory control and validates the selected source or
pair atomically before launching paused Preview or Compare. Both launch adapters
enforce one shared aggregate budget of 128 runtime files, 64 MiB encoded bytes,
and 192 MiB decoded texture bytes before the viewer starts. Positional native
Preview/Compare and authenticated browser manifest launches remain unchanged.
This slice is launch-only: it does not accept `.spine`, ZIP, or workflow
packages and adds no project, Base, Submission, Proposed, server, mutation,
Review, or promotion state.

### Shared viewer contract

- Immutable `SourceBundle`, host-neutral `ViewerSession`, exact `ReviewClock`,
  commands, and canonical parity snapshots are shared.
- Source slots are Primary and optional Comparison; animation identity is name.
- Compare waits for all sources and advances both from one Bevy delta.
- Browser messages are versioned and validate origin/source/capability.
- Native paths and browser-selected files are implemented as thin adapters into
  the same bundle. ZIPs, embedded assets, and coordinator URLs remain future
  adapters behind their applicable phase gates.
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

Accessibility automation is PRE-FLIGHT only. Browser 200%/400% zoom and
reflow require the recorded human browser run. Native uses minimum-window,
display-scale, and platform-magnification review; that is not relabelled as
native 200%/400% reflow. The exact acceptance procedure and non-claims are in
the [Accessibility Acceptance Runbook](docs/ACCESSIBILITY-ACCEPTANCE-RUNBOOK.md).

Accessibility acceptance record: **INCOMPLETE**.

The next four bullets retain the historical Bevy 0.18.1 record verbatim:

- Automated PRE-FLIGHT: **PASS** on 2026-08-08 UTC at commit
  `81f065e026cf688ad8b52a8f207b8e25dc8e8fa4` using the macOS/Chrome v1
  profile. Browser semantics, the locked workspace suite, the real-browser
  Preview/Compare and 500-pixel narrow checks, and the end-of-run clean-repository
  check passed.
- Private evidence identifier: `accessibility-81f065e`; checksummed pre-flight
  manifest SHA-256:
  `e041060a386717da272e12379bdee511286407160be921dc04d87d7ef32e667b`.
- Named human browser/native keyboard and VoiceOver review: **NOT RUN**.
- Decision authority and final immutable report digest: absent. The generated
  report remains `incomplete` and `phase0b_gate_eligible=false`.

Recorded Bevy 0.19 pre-flight at `07af12d`:

- Automated PRE-FLIGHT: **PASS** on 2026-08-09 UTC at commit
  `07af12de9f726b15fd66d629ae345b2e66686276`. Browser semantics, the locked
  workspace suite, the production real-browser Preview/Compare and 500-pixel
  narrow checks, and the end-of-run clean-repository check passed.
- Private evidence identifier: `accessibility-07af12d-bevy-0.19.0`;
  checksummed pre-flight manifest SHA-256:
  `037e2700c42a9bbf17f31887e42255b37313ee515148be616e88ea89cd832564`.
- Named human browser/native keyboard and VoiceOver review: **NOT RUN**.
- Decision authority and final immutable report digest: absent. The generated
  report remains `incomplete` and `phase0b_gate_eligible=false`.

Historical current-surface Bevy 0.19 pre-flight at `64f01f0`:

- Automated PRE-FLIGHT: **PASS** on 2026-08-09 UTC at commit
  `64f01f072e3e7e4f7b89426ee0a51f74fcdb2ed3`. Browser semantics, the locked
  workspace suite, the production real-browser Preview/Compare and 500-pixel
  narrow checks, and the end-of-run clean-repository check passed.
- Private evidence identifier: `accessibility-64f01f0-current-surface`;
  checksummed pre-flight manifest SHA-256:
  `d54dffab283e033782b2fa18c4b6202b659570e988162493a439401c0477c1bd`.
- Named human browser/native keyboard and VoiceOver review: **NOT RUN**.
- Decision authority and final immutable report digest: absent. The generated
  report remains `incomplete` and `phase0b_gate_eligible=false`.

Current Open-surface Bevy 0.19 pre-flight at `84b10f3`:

- Automated PRE-FLIGHT: **PASS** on 2026-08-09 UTC at commit
  `84b10f3c27f4557c5e447d5ec31c3efc7c01aaaf`. Browser semantics, the locked
  workspace suite, the production real-browser Open failure/retry and paused
  Preview launch, Preview/Compare isolation, 500-pixel narrow checks, and the
  end-of-run clean-repository check passed.
- Private evidence identifier: `accessibility-84b10f3-20260809-open-rerun`;
  checksummed pre-flight manifest SHA-256:
  `ef91a0ca0543238844553aedcda0d6952182295dc8770e5a0ea20211b0bda21e`.
- Named human browser/native keyboard and VoiceOver review: **NOT RUN**.
- Decision authority and final immutable report digest: absent. The generated
  report remains `incomplete` and `phase0b_gate_eligible=false`.

This result supersedes automated surface coverage at `64f01f0` only. It
satisfies no named human row; accessibility acceptance remains **INCOMPLETE**.

Current Open/Compare-surface Bevy 0.19 pre-flight at `228f757`:

- Automated PRE-FLIGHT: **PASS** on 2026-08-09 UTC at commit
  `228f757b154840a7fbba780576682245675a7aee`. Browser semantics, the locked
  workspace suite, real-browser invalid Primary recovery, invalid Comparison
  atomic recovery, paused Preview and Compare launches, split-pane isolation,
  cleared hidden FileLists and canvas focus, 500-pixel narrow checks, and the
  end-of-run clean-repository check passed.
- Private evidence identifier:
  `spinal-accessibility-228f757-20260809-open-compare`; checksummed pre-flight
  manifest SHA-256:
  `84ba4adc56f06c5f011d4cc1acfd9657b32ccf3c86af2c1a06cc4061ac6e9b9e`.
- Named human browser/native keyboard and VoiceOver review: **NOT RUN**.
- Decision authority and final immutable report digest: absent. The generated
  report remains `incomplete` and `phase0b_gate_eligible=false`.

This result supersedes automated surface coverage at `84b10f3` only. It
satisfies no named human row; accessibility acceptance remains **INCOMPLETE**.

The human decision is recorded in an immutable external report. A read-only
checker may emit its digest only after every required row and artifact checksum
passes; acceptance remains **INCOMPLETE** until that exact digest, tested commit,
reviewer, and date are recorded here without changing the report bytes.

Target WCAG 2.2 AA for chrome/workflow. Visual motion approval still requires a
qualified visual reviewer or agreed accommodation; diagnostics do not replace
that judgment. Thin host shells may differ, shared product logic may not.

## Phase 2: Bevy 0.19 migration

Status: migration checkpoint recorded at
`07af12de9f726b15fd66d629ae345b2e66686276`; the local automated migration
matrix and current automated accessibility pre-flight pass. Configured
CI/platform results remain pending. The workspace baseline is Bevy 0.19.0 with
Rust 1.95; dual-version support is not provided. This does not pass
representative Phase 0A or 0B, and it does not transfer the historical
accessibility result.

- The locked graph contains one Bevy 0.19 line, aligned AccessKit 0.24 and glam
  0.32 dependencies, and no Bevy 0.18 package.
- Asset loading, input focus, custom Mesh2D target/compositing keys, transient
  render phases, visibility iteration, and manual render synchronization use
  the Bevy 0.19 APIs without deprecation allowances.
- Native workspace, headless adapter, application, showcase, documentation,
  strict Clippy, and fuzz-target checks pass on the Rust 1.95 baseline. Both
  production and opt-in Phase 0B WASM modes compile and lint. Separate local
  real-Chrome/WebGL2 smokes cover the production WASM host and the generic,
  gate-ineligible Phase 0B browser capture seam.
- CI is configured to retain separate production `web` Clippy and MSRV
  coverage, validate the Phase 0B shell/CDP tooling, and run the generic browser
  smoke after the production web smoke, while also checking macOS, Windows,
  stable Rust, exact MSRV, documentation, and evidence tooling. Results for this
  revision remain pending.
- WebGL2 remains first and WASM remains single-threaded; WebGPU is a separate
  future decision.
- The frozen 0.18.1 rehearsal and accessibility artifacts remain historical.
  Representative Phase 0B still requires fresh evidence; current accessibility
  acceptance still requires the named human review despite fresh automation.

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

Before stable viewer acceptance, follow the
[Accessibility Acceptance Runbook](docs/ACCESSIBILITY-ACCEPTANCE-RUNBOOK.md).
Its real-browser automation includes a 500-CSS-pixel narrow PRE-FLIGHT, but
actual browser 200%/400% zoom/reflow, keyboard and visible-focus usability,
and VoiceOver behavior remain named human checks. Native minimum-window and
magnification review is recorded separately and is not a reflow claim. This
supports the scoped WCAG AA chrome/workflow target; it is not WCAG
certification and does not replace human visual judgment.

Activated acceptance proves exact-version capability, round-trip allowlist,
new/existing fingerprints, unchanged setup/assets, native/WASM load, and
fail-closed warning/timeout/partial/nonzero behavior. Spinal owns a
redistributable generic fixture; consumers keep private representative fixtures,
licensed renders, and evidence outside Git.

## Milestones and pivots

- **Stable viewer:** launch-only Open, Preview, Compare, Diagnostics, and check
  are trustworthy once the current automated and named human acceptance lanes
  are complete.
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

## Implementation dependencies

Read-only preparation may proceed independently without changing either Phase 0
gate:

```text
Phase 1 historical viewer checkpoint -> Phase 2 Bevy 0.19 viewer migration
Phase 0A representative adapter implemented -> review -> representative run
0B rehearsal contract -> frozen historical reference only; never gate evidence
```

The hard authorization chain is:

```text
representative 0A PASS
  -> private disposable, non-promotable Proposed copy
  -> representative 0B on the migrated viewer
  -> representative 0B PASS
  -> Phase 3A guided no-conflict native/browser slice with thin spinal serve
  -> Phase 3B evidence-based durable-job decision; implement only when yes
  -> Phase 4 visual conflict Review, acknowledgments, atomic promotion
  -> Phase 5 private acceptance and limited beta
  -> Phase 6 headless construction after beta
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

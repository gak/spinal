# Phase 0 Evidence Runbook

This runbook owns the mechanics, evidence formats, and historical calibration
record for the Phase 0A and Phase 0B gates in the
[Spinal Application Consolidation plan](../PLAN-SPINAL-APPLICATION-CONSOLIDATION.md).
The plan remains authoritative for product boundaries, implementation order,
gate consequences, and the decision to proceed or stop. A tool or report cannot
change a gate state by itself.

Private downstream project files, licensed-editor transcripts, reference
renders, and completed evidence reports stay in an owner-private directory
outside Git. Checked-in cases contain specifications and empty evidence slots,
not private artifacts or claims that a run occurred.

## Ownership and pass authority

- **Owner:** the project owner/reviewer controls the licensed Spine 4.3.23 seat,
  chooses the exact representative Current, replacement Submission, and
  new-animation Submission, and owns the private evidence directory.
- **Phase 0A runner:** the checked-in `tools/spinal-phase0a` generic binary is
  permanently gate-ineligible. A closed representative entry point,
  owner-private binding, format-v5 outer publisher, and read-only verifier were
  implemented and reviewed at `b229339` in that crate. They bind the exact
  private Current, one replacement Submission, and one new-animation Submission
  while reusing the frozen operation primitives over disposable staged copies.
  This remains an internal conformance harness, not a product command.
- **Phase 0B runner:** an owner-invoked runner under `tools/spinal-phase0b` must
  execute the checked-in semantic schedule through native and browser hosts.
  The crate now authenticates the closed case and isolated runtime bundles,
  owns the shared v1 schedule and strict semantic/event/pixel comparison, and
  provides identity-bound native semantic and event capture. `spinal-app` has an
  opt-in browser observation path whose output names both runtime identities,
  plus a generic Bevy 0.19 CDP seam that captures fresh Current/Proposed event
  windows and eight identity- and generation-bound presented PNGs after a fresh
  nonce challenge and strict presentation isolation. An outer version 3
  envelope requires both event windows and binds the screenshot receipts to
  semantic observations; the strict host parser requires both the loaded bundle
  pair and an independently retained expected nonce. A representative private
  v2 case and policy, the identity-bound two-host owner runner, independent
  references, complete browser/build/GPU provenance, publisher, and verifier
  still have to be implemented and reviewed.
- **Evidence:** each run publishes a machine-readable assertion matrix and
  digest-bound artifacts. The existing generic Phase 0A runner uses format v4;
  the representative entry point encloses that unchanged generic core in a
  format-v5 report that binds its evidence class, eligibility, and exact
  three-package mapping without reusing or relabelling generic claims. Phase 0B
  must use a versioned report that binds the case, binaries, runtimes, browsers,
  reference provenance, semantic frames, events, pixels, and diagnostics.
- **Pass authority:** the maintainer/reviewer inspects a fresh report and its
  independent references, records the result in the plan, and is the only
  authority that may mark a gate passed. Missing, skipped, degraded, stale, or
  self-generated expected evidence is a failure, regardless of process exit.

The representative gate order is fixed: the closed Phase 0A path was reviewed
at `b229339`; next run it on the exact private Current, replacement Submission,
and new-animation Submission. The Bevy dependency migration is independent
preparation and neither passes nor waives either representative gate. After
Phase 0A passes, the owner may construct one private, disposable,
non-promotable Proposed copy from fresh Current through the proven import
recipe solely for Phase 0B. Run the complete Phase 0B matrix on those exact
Current and Proposed bundles. Both representative reports must pass before
Phase 3A begins.

## Current evidence state

- The Phase 0A harness and its controlled-failure path are implemented.
- A licensed generic calibration at source revision `2a68e1f` passed all 25
  assertions. It is deliberately non-representative and does not pass Phase 0A
  for the intended workflow.
- The closed representative Phase 0A adapter, binding, outer publisher, and
  read-only verifier were implemented and reviewed at `b229339`. The existing
  generic report cannot be relabelled or promoted into representative evidence.
- The representative Phase 0A run is **NOT RUN**.
- A versioned semantic-frame contract, authenticated case/runtime-bundle
  loaders, identity-bound native semantic and event capture, strict
  outer-v3 browser-envelope parsing with required Current/Proposed event
  windows and an independently retained nonce, stable event diagnostic codes,
  and bounded semantic/event/pixel comparators exist for Phase 0B foundation
  work.
- The generic Bevy 0.19 real-Chrome smoke passes locally. It uses only
  self-authored fixtures to exercise a fresh nonce, fresh Current/Proposed event
  windows, the fixed four-sample schedule, strict presentation isolation, and
  eight original 640-by-480 RGB8/RGBA8 CDP PNGs. It is plumbing, not an
  independent oracle or evidence; its capture manifest records
  `gate_eligible = false` and parsed results are categorically gate-ineligible.
- No owner command binds one representative loaded case through both hosts.
  Independent analytical and licensed-Spine references, a representative
  private v2 case/policy, browser/build/GPU provenance, and report
  publication/verification remain unavailable.
- `tools/spinal-phase0b/cases/generic-bevy-0.18.1.toml` remains `not_run` and
  permanently `gate_eligible = false`. Its required evidence slots are empty;
  it is a frozen historical contract and cannot become Bevy 0.19 evidence.
- Representative Phase 0B is **NOT RUN**, no pass is claimed, and mutation
  remains locked.

## Representative Phase 0A candidate workflow

The implementation review is complete at `b229339`. The authoritative run
starts from a clean reviewed commit and uses exact prebuilt
binaries; `cargo run` is not the representative runner. From the clean
checkout, verify that `git status --short` is empty, record the lowercase
revision from `git rev-parse --verify HEAD`, and build both tools together:

```sh
cargo +1.95.0 build --locked -p spinal-phase0a \
  --bin spinal-phase0a-representative \
  --bin spinal-phase0a-verify
```

The build embeds the clean source revision and exact workspace `Cargo.lock`.
Do not edit, rebuild, or replace the representative runner after proposing its
binding. Create a new owner-private parent outside Git, place the final case
there, and give the case and binding mode `0600`; the parent must be accessible
only to its owner. The case must name the exact three representative packages.

Generate a proposal with the exact prebuilt runner. Proposal mode observes its
own bytes and embedded clean source revision and workspace lockfile digest. It
only prints TOML; it creates no files, evidence, or gate decision:

```sh
umask 077
mkdir -m 700 "/absolute/private/phase0a-run"
chmod 600 "/absolute/private/phase0a-run/case.toml"
just phase0a-binding-proposal \
  "/absolute/checkout/target/debug/spinal-phase0a-representative" \
  "/absolute/private/phase0a-run/case.toml" \
  > "/absolute/private/phase0a-run/representative-binding.toml"
chmod 600 "/absolute/private/phase0a-run/representative-binding.toml"
```

Review every proposed identity before continuing: evidence class and binding
ID, exact case digest, exact representative-runner digest, clean source
revision, `Cargo.lock` digest, and the role-tagged Current,
replacement-Submission, and new-animation-Submission package-tree digests.
Then invoke that exact runner through the recipe that takes its path explicitly:

```sh
just phase0a-representative \
  "/absolute/checkout/target/debug/spinal-phase0a-representative" \
  "/absolute/private/phase0a-run/representative-binding.toml" \
  "/absolute/private/phase0a-run/case.toml" \
  "/absolute/path/to/Spine" \
  "/absolute/private/new-workspace" \
  "/absolute/private/spine-editor.lock" \
  "/absolute/private/new-evidence"
```

All paths must be absolute and normalized. Workspace and evidence destinations
must be new, non-overlapping paths beneath owner-private parents. The workspace
is retained when preparation creates it. Representative admission rejects case
bytes containing unredacted `Licensed to:` text before creating any
destination, so retained generic-v4 diagnostics can include the exact bound
case without exposing license-owner text. A failed inner core is retained only
as generic-v4 diagnostics and receives no top-level format-v5 report. If
admission or publication fails, any partial destination is **UNPUBLISHED**:
retain it for diagnosis, do not repair or promote it, and use fresh workspace
and evidence paths for the next attempt. `report.json` is published last.

Verify the exact published directory with an explicitly selected prebuilt
verifier. Use the canonical evidence path printed by the runner; aliases such
as macOS `/tmp` for `/private/tmp` are intentionally rejected:

```sh
just phase0a-verify \
  "/absolute/checkout/target/debug/spinal-phase0a-verify" \
  "/absolute/private/new-evidence"
```

The verifier is read-only. It independently checks the fixed filesystem graph,
hashes, inventories, identities, cross-links, eligibility derivation, and
representative-marker coverage. It does **not** rerun Spine or the native
validator, reclassify transcripts, or rederive normalized or semantic
comparisons. Those results still require maintainer inspection. A valid
representative-v5 input is necessarily a complete passing candidate; an
unpublished diagnostic tree is rejected. A successful candidate and successful
verification still do not record PASS: only the maintainer/reviewer may update
the plan, and mutation remains locked until representative Phase 0A and Phase
0B both pass.

## Phase 0A capability preflight

Prove at runtime, rather than echoing configuration, that:

- the selected executable exists and is the approved Spine launcher;
- exact editor version 4.3.23 runs;
- the installed license is activated for the required operations;
- the launcher accepts every advanced import/export argument used by Spinal;
- the exact requested skeleton is discovered without a fallback guess;
- CLI calls return expected exit codes and warning output;
- the linked native Spinal validator executes successfully against the exact
  staged bundles; and
- Spine CLI work can be serialized safely.

Record the exact commands, executable identity, version output, exit codes,
stdout, stderr, warnings, and output checksums.

The first licensed host contract is the project owner's recorded macOS host and
architecture. Invoke the approved Mac CLI executable directly and select
exactly `--update 4.3.23` for every job; a family selector such as `4.3.xx` is
not acceptable. Record whether the command may use a prewarmed editor cache or
network, who owns the activated seat, and which opt-in CI host, if any, may run
licensed acceptance. Missing or revoked activation is a hard gate, never a
reason to fall back or expose license material in evidence.

Run every CLI probe inside its complete package context, including required
empty asset directories. A missing-path message such as `Images path not
found` is blocking even when Spine exits successfully; it is never hidden by
an allowlist or by a warning detector that only searches for the words
`warning` and `error`.

Phase 0 emits one machine-readable assertion matrix. Every required assertion
has its own result and evidence digest; the overall result is the conjunction
of all assertions. An assertion cannot accept a caller-supplied pass boolean or
cite an unrelated artifact. Its result is derived from typed,
assertion-specific evidence and the exact process operation that produced it.
Adversarial tests prove that wrong version, wrong license state, wrong
skeleton, unsupported arguments, warnings, partial output, timeout, and
nonzero exit each make the overall gate fail.

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
normalization, safe staging, orchestration, and typed assertion derivation must
be complete before Phase 0A can change from **NOT RUN** to a result.

### Fixed editor recipe

The production rehearsal is one fixed, linear recipe rather than a general job
framework. It performs exactly 22 ordered editor operations: version and
advanced-help probes; three project inventories; the two deterministic JSON
round trips; first and repeated existing-animation imports; one successful
new-animation import; one isolated duplicate-new-animation collision control
and its diagnostic export; and one final missing-`./images/` negative control.
Removing, adding, reordering, relabelling, or rebinding any operation
invalidates the run. A generic fixture rehearsal can exercise the machinery
but cannot be converted into representative gate evidence.

The collision control uses a separate disposable project that already
contains the submitted new animation. It must prove the observed Spine 4.3.23
hazard exactly: a repeated no-`--replace` import may exit zero, report a
requested-name-to-renamed-name collision, and add the renamed duplicate. That
diagnostic is accepted only as the expected result of this negative-control
operation. The ordinary new-animation import must remain diagnostic-free, and
its clean candidate and first export are never reused by the collision
control.

All editor work occurs in one fresh owner-private run directory. Preparation
stages immutable package copies, two explicit current-derived candidates, an
isolated duplicate-collision copy, the missing-path-control copy, fixed output
slots, and the checked-in export preset; then the workspace is sealed. Each
command must consume a staged file or a verified output from an earlier
successful operation and may mutate only its exact declared slot.
Descriptor-relative snapshots bind file identity, mode, owner, link count,
timestamps, size, and digest before and after every call. Hard links, path
aliases, between-operation edits, undeclared files, and same-byte replacements
fail closed. Inputs are bounded by fixed depth, entry, per-file, total-byte,
process-time, and transcript-size limits. The three original packages are
rechecked after the final editor operation.

This boundary does not claim kernel-level isolation from malicious code already
running as the same operating-system user. A temporary file created and
removed entirely during an editor call may be unobservable. The licensed editor
and host user are trusted; all persistent pre-call, post-call, and between-call
state is audited.

Generic evidence format v4 identifies every artifact by the full
`role + portable path + SHA-256` triple. Equal empty transcripts are valid when
their paths differ. Assertions and processes cite exact identities, and a
fresh `0700` evidence directory receives create-only `0600` artifacts only
after a complete privacy and integrity preflight. `report.json` is published
last. Any unhidden `Licensed to:` text blocks publication without echoing the
sensitive line.

Representative evidence format v5 is an outer, immutable composition. Its
top-level layout is `report.json`, the exact
`representative-binding.toml`, and `core/`. The complete `core/` directory is a
fresh format-v4 generic report and artifacts, enclosed without alteration: its
metadata remains `generic_rehearsal` and
`representative_gate_eligible: false`. A prior generic rehearsal cannot be
substituted. The outer report binds the exact binding and case, three
role-tagged package-tree digests, clean source revision and `Cargo.lock`, exact
prebuilt representative-runner bytes, the entire core tree, and a hashed
`SPINAL_PHASE0A_REPRESENTATIVE_BINDING_SHA256` marker in every process of a
passing candidate. Eligibility requires all 22 markers. It alone may state
that a passing candidate is representative-gate-eligible. A controlled-failure
core remains generic-v4 diagnostics beneath an **UNPUBLISHED** partial
destination. It is never enclosed by format v5 and is invalid verifier input.

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
- setup, skeleton, skins, attachments, constraints, and assets equal Current;
- every unselected animation equals Current;
- selected animation replacement is the only semantic change;
- repeating an existing-animation import with explicit `--replace` is
  semantically idempotent in its exact exported JSON even if opaque `.spine`
  bytes change; every binary identity remains recorded and chain-bound;
- a new-animation import without replacement is single-apply, while the
  isolated duplicate-name control proves and records its unsafe retry behavior;
- warnings, partial outputs, timeouts, and nonzero exits fail the operation;
  and
- the Current source package is byte-for-byte unchanged.

## Phase 0A calibration history

On 2026-08-08, a licensed generic calibration reached the old repeated-new
slot and confirmed the duplicate-name hazard: Spine 4.3.23 exited zero,
reported `gesture -> gesture2`, and exported both animations. The run stopped
before publication because the then-current success-only publisher could not
emit a failure matrix. This calibration is not representative evidence.

A fresh generic rehearsal at source revision `af029e5` correctly published a
failed format-v4 report, SHA-256
`6ef01ce4fbf1340414ef566d35fd49b8eb30fc2c9b7dae0e9199debb7a2f8fe8`.
The collision transcript also contained the ordinary `Imported animation:
gesture` line; the parser correctly failed until the reviewed contract was
updated. All three original packages remained byte-for-byte unchanged.

The next rehearsal at `4bf48e5` completed all 22 editor operations and
correctly published a failed report, SHA-256
`faefd158f002783d38db8109eb9b85110835a65a09ed9faaa64c8a8cfd31816e`.
The exact missing-images diagnostic ended in `./images/`, while the contract
had required the slashless form. The originals remained unchanged.

The following rehearsal at `8782819` admitted all transcripts and correctly
published a failed report, SHA-256
`8d0f22d005fbb870a60a7754c2133767f846bddf469e30f1bdd84b7243446dfe`.
Spine rewrote opaque `.spine` bytes during repeated existing-animation
replacement even though the two JSON exports were byte-identical. Binary
identity is therefore evidence, not the definition of semantic idempotence.

After that rule was reviewed and checked in, the fresh licensed generic
rehearsal at `2a68e1f` passed all 25 assertions. Its format-v4 report SHA-256 is
`992615694d3588dca755507c36480f807c13a5ad75ac660cae1eebb3a8733bc5`.
All 22 operations ran against Spine 4.3.23; slots 19 and 21 were the only
expected negative controls. Both reconstruction round trips and both
determinism comparisons were raw-byte identical with no observed losses. The
only approved volatile pointer remains `/skeleton/hash`. Existing-animation
replacement matched its submission and its repeat export was byte-identical;
the new import added only `gesture`; the isolated collision added only
`gesture2` with the same name-independent content fingerprint. Current,
existing, and new runtime bundles passed the native Spinal validator with no
diagnostics, all original source packages remained unchanged, and the report
binds to a clean `2a68e1f` checkout and approved launcher digest.

This generic result proves the harness and CLI premise on that host. It does
not pass Phase 0A for the production workflow and does not unlock mutation or
promotion.

## Phase 0B execution contract

The Bevy 0.18.1 rehearsal contract is frozen non-representative history. It was
not run and no evidence carries forward. The authoritative gate runs on Bevy
0.19 against the representative private Current and Proposed bundles.

The runner must:

1. load the exact checked and digest-bound case;
2. capture complete versioned semantic frames and event windows through native
   Spinal;
3. run the same bundle and schedule through the browser/WASM host;
4. compare native and browser semantics against the checked contract;
5. compare both semantics and browser pixels with independent project-owned
   analytical and licensed-Spine references;
6. collect stable diagnostics and complete build/browser/GPU provenance; and
7. publish a create-only report whose overall result is derived from all
   required assertions.

The candidate runtime bundle must load without fallback validation, decode
every atlas page and texture, evaluate every changed animation at all required
samples, expose no blocking unsupported feature, agree semantically across
native and WASM, render correctly in the browser, match tolerant independent
appearance references, support all review controls, and match independently
known bone transforms and active attachments.

Validation covers both Current and Proposed. Parser errors, degraded features,
unsupported features used by either bundle, missing pages, texture decode
failures, evaluation failures, and unlisted diagnostics are blocking.

Native/WASM agreement is parity evidence, not a correctness oracle. Use
project-owned analytical fixtures for independently known bone transforms,
attachments, slot order, vertices, colors, and events. Use images rendered by
licensed Spine 4.3.23 at recorded timestamps as the independent appearance
oracle for the representative private project. Spinal output can never
generate its own expected result.

The checked-in generic v1 case deliberately fixes one one-second `sway`
animation, four exact samples including the `alternate` skin, and one exact
event window. Its parser authenticates safe, bounded evidence references; its
bundle loader isolates Current and Proposed; native semantic capture preserves
those identities; deterministic native event capture produces strict event
documents for the same retained pair; and strict event/PNG comparators are
available. The PNG comparator accepts exact 640-by-480 static non-interlaced
RGB8 or RGBA8 inputs, expands RGB8 to opaque RGBA only in memory, and compares
the normalized buffers without rewriting the originals.

The generic Bevy 0.19 browser seam starts with a fresh driver-generated
256-bit nonce. After both sources are ready, two hidden instances created from
their exact loaded asset handles perform a fresh no-seek `sway`/Once event
window through the inclusive endpoint and are removed before the fixed
sample-major schedule begins. The browser then holds Current and Proposed at
each of the four full-viewport samples across two strict Bevy updates; the CDP
driver waits through a two-frame compositor barrier and stores the eight exact
original PNG byte strings. Screenshot receipts carry their exact lengths and
hashes, semantic and acknowledged command generations, and runtime identities.
The outer version 3 semantic document requires both strict event windows and
binds those receipts to all eight observations. Rust parsing requires the
expected nonce to have been retained independently and binds the document to the
already loaded Current/Proposed pair.

Run the separate local smoke with:

```sh
just phase0b-browser-smoke 8427
```

That real-Chrome smoke passes locally at this implementation checkpoint and
requires neither FFmpeg nor ImageMagick. It is a self-authored
`non_representative_rehearsal` with `gate_eligible = false`, not an analytical
or licensed-Spine oracle, Phase 0B evidence, or PASS. Configured CI results for
the revision are not claimed.

Before a representative run, obtain independent project-owned analytical and
licensed-Spine references; define and review the representative private v2 case
and policy; implement the identity-bound two-host owner runner; collect complete
browser/build/GPU provenance; and add the create-only publisher and independent
verifier. See
[`tools/spinal-phase0b/README.md`](../tools/spinal-phase0b/README.md) for the
current specification and the work still missing before a representative run.

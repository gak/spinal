# Phase 0B generic rehearsal foundations

`spinal-phase0b` is an internal, unpublished foundation for one deliberately
small Phase 0B input contract. It authenticates bounded inputs, retains their
exact bytes, acquires and validates the case-declared Current and Proposed
runtime bundles, compares complete semantic frames, and can execute the fixed
native sample schedule against two already loaded Bevy assets. Its public fixed
contract is also consumed by `spinal-app`'s opt-in `phase0b-rehearsal` browser
path, which drives the ordinary WASM viewer and emits Current and Proposed
semantic observations bound to each runtime manifest and content SHA-256. It
does not yet run a full case through both hosts, bind retained bundle identities
to the native Bevy handles, compare events or pixels, publish evidence, or make
a go/no-go decision.

The checked-in `cases/generic-bevy-0.18.1.toml` case is explicitly:

- `non_representative_rehearsal`;
- pinned to Spine 4.3.23 and Bevy 0.18.1;
- in `not_run` state;
- permanently `gate_eligible = false`; and
- incomplete because no fixture or independent reference artifacts have been
  created or claimed.

Parsing this file proves only that the intended rehearsal contract is closed
and internally coherent. The native smoke test proves capture plumbing against
a hand-authored in-memory fixture; it is not an independent semantic oracle.
Neither result is Phase 0B evidence or unlocks mutation, coordinator, review,
or promotion work.

## Fixed rehearsal boundary

The v1 contract requires exactly Current and Proposed runtime bundles and one
one-second animation named `sway`. Its schedule is literal: four ordered
samples named `sway-start` at 0 seconds, `sway-middle` at 0.5 seconds,
`sway-alternate-skin` at 0.75 seconds with only the `alternate` skin layer, and
`sway-end` at 1 second. It also requires the single `sway-events` event window
from 0 through 1 second. The native helper and opt-in WASM observation path both
implement the complete fixed semantic schedule, but neither has been executed
as one authenticated owner-run case. Only the browser host has an appearance
oracle in this lightweight rehearsal. Native framebuffer capture is explicitly
not required.

Expected semantic and appearance values must be project-owned analytical
references produced independently of Spinal. Spinal, this parser, and the
future rehearsal runner may never generate their own expected results. The
allowed nonblocking diagnostic set is empty; missing, skipped, or degraded
evidence cannot pass a future rehearsal.

The numeric and browser-pixel tolerances in the case are exact fixed policy,
not caller-selected limits. Changing the sample schedule, fields, features,
hosts, or tolerances requires a reviewed schema change and invalidates older
results.

## Evidence slots

Every required input is represented by an evidence slot. A slot containing
only `required = true` is deliberately unavailable. It makes the case
incomplete without pretending that a file or digest exists.

Do not add concrete references or private artifacts to the checked-in template.
For a real rehearsal, copy the TOML and the complete private fixture/oracle tree
to an owner-private external evidence directory outside Git, preserving their
relative paths. Populate `path`, `byte_length`, and `sha256` only in that
external copy, after each referenced artifact exists and its exact length and
digest have been measured. Partial metadata is invalid.

Concrete paths are relative to the case file's directory. They must be
portable normalized paths, and length and digest metadata are all-or-none.
`load_case` rejects symlinks, verifies every referenced regular file's exact
nonzero bounded length and lowercase SHA-256, and retains the authenticated
bytes so later code does not reopen them. A relative case path is anchored to
the process's absolute working directory before the first read, so a later
working-directory change cannot redirect bundle acquisition. `parse_case`
validates only the TOML contract and therefore never claims filesystem
authentication.

`load_case` checks metadata from the opened file before allocating, then reads
at most the declared length plus one byte and requires exact EOF and length. Its
security boundary assumes an owner-private local directory without hostile
concurrent writers; it is not the descriptor-relative, tamper-audited Phase 0A
boundary. If files can change concurrently, stage immutable copies first.

Hard limits are compiled into the parser: 64 KiB for the case and runtime
manifests, 64 KiB for provenance, 1 MiB per semantic reference, 256 KiB per
event reference, 4 MiB per browser PNG, and 32 MiB across all references. The
v1 schedule is not configurable beyond the exact animation, four samples, skin
selection, and event window described above.

Exactly eleven authenticated artifacts make the partial semantic execution
plan available: provenance, Current and Proposed runtime manifests, and eight
independent semantic references. This does not weaken full readiness, which
still requires all 21 evidence slots. The case remains `not_run` and
gate-ineligible in either state.

`load_case_runtime_bundles` strictly parses the two retained runtime manifests,
resolves each manifest-declared location beneath that manifest's case-relative
directory, rejects unsafe paths, links, special files, physical aliases, and
case-artifact aliases, then performs bounded exact-length and SHA-256 reads.
The two isolated byte maps must pass the shared `RuntimeBundleManifest`
validation before immutable `ValidatedRuntimeBundle` values are returned. This
loader still assumes its owner-private local case tree is quiescent; it does
not claim the descriptor-relative hostile-writer boundary required by Phase
0A.

The native capture helper always uses `sway` in `Once` mode, pauses it, applies
the exact skin selection, and seeks to each fixed timestamp. Before issuing any
command it verifies that both supplied assets contain an exact one-second
`sway` animation and the `alternate` skin. It accepts a frame only after the
capture revision advances and the runtime acknowledges the fresh play and seek
generations plus exact skin layers. The semantic comparator keeps structural
fields exact and applies only the fixed field-specific tolerances; its bounded
difference report contains no pass or gate field. The animation, schedule,
skin, event-window, and semantic-tolerance values live in one public v1
contract module shared by the native helper and current opt-in browser
observation path.

## What remains before a rehearsal

The generic fixture pair and independent analytical semantic, event, and
browser references do not exist. The opt-in browser path already emits
identity-bound semantic observations, but there is no full owner command that
loads one authenticated case through both hosts, no identity-bound
case-to-Bevy-asset native capture runner, and no event or pixel comparison,
final provenance collector, or report publisher. The next native seam must
construct Bevy assets directly from the retained `LoadedCaseRuntimeBundles`
bytes, carry both content SHA-256 identities through capture, and compare
observations with the same loaded case's authenticated references in one
bounded operation. The existing handle-level capture helper is intentionally
not evidence-capable. Add that identity-bound seam only in a later reviewed
slice. Until every required slot is backed by a real independently reviewed
artifact and a separate runner executes both hosts, the status remains **NOT
RUN**.

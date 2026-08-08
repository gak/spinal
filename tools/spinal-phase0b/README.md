# Phase 0B generic rehearsal specification

`spinal-phase0b` is an internal, unpublished parser for one deliberately
small Phase 0B input contract. It does not run Bevy, a browser, WebAssembly,
or the Spine editor; serialize frames; compare pixels; publish evidence; or
make a go/no-go decision.

The checked-in `cases/generic-bevy-0.18.1.toml` case is explicitly:

- `non_representative_rehearsal`;
- pinned to Spine 4.3.23 and Bevy 0.18.1;
- in `not_run` state;
- permanently `gate_eligible = false`; and
- incomplete because no fixture or independent reference artifacts have been
  created or claimed.

Parsing this file proves only that the intended rehearsal contract is closed
and internally coherent. It is not Phase 0B evidence and does not unlock
mutation, coordinator, review, or promotion work.

## Fixed rehearsal boundary

The v1 contract requires exactly Current and Proposed runtime bundles and one
one-second animation named `sway`. Its schedule is literal: four ordered
samples named `sway-start` at 0 seconds, `sway-middle` at 0.5 seconds,
`sway-alternate-skin` at 0.75 seconds with only the `alternate` skin layer, and
`sway-end` at 1 second. It also requires the single `sway-events` event window
from 0 through 1 second. Both native and WASM hosts must eventually produce the
complete fixed semantic frame at every sample. Only the browser host has an
appearance oracle in this lightweight rehearsal. Native framebuffer capture
is explicitly not required.

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
`load_case` rejects symlinks and verifies every referenced regular file's exact
nonzero bounded length and lowercase SHA-256. `parse_case` validates only the
TOML contract and therefore never claims filesystem authentication.

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

## What remains before a rehearsal

The generic fixture pair and independent analytical semantic, event, and
browser references do not exist. This parser also contains no runner, capture
path, browser comparison, provenance collector, or report publisher. Add them
only in later reviewed slices. Until every required slot is backed by a real
independently reviewed artifact and a separate runner executes both hosts, the
status remains **NOT RUN**.

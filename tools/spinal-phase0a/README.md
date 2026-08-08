# Spinal Phase 0A evidence harness

This internal, opt-in tool runs the closed Spine 4.3.23 round-trip and
whole-animation-import rehearsal used to develop Spinal's collaboration
workflow. Its public command can produce **generic, non-representative**
evidence only. It cannot mark a run as representative or unlock a downstream
project's Phase 0 gate.

The generic runner is implemented. The representative downstream-project run
remains **NOT RUN**.

## Run a generic rehearsal

Start from `cases/example.toml`, copy it outside the repository, and replace
the package roots, runtime atlas, animation and skeleton names, and editor
executable checksum. Operational Spine projects, private workspaces, locks,
and evidence must remain outside Git.

The exact command is:

```sh
just phase0a-generic \
  "/absolute/path/to/case.toml" \
  "/absolute/path/to/Spine" \
  "/absolute/path/to/new-workspace" \
  "/absolute/path/to/spine-editor.lock" \
  "/absolute/path/to/new-evidence"
```

This expands to:

```sh
cargo run --locked --package spinal-phase0a --bin spinal-phase0a-generic -- \
  <case.toml> \
  <spine-executable> \
  <workspace-directory> \
  <editor-lock-file> \
  <evidence-directory>
```

The five positional arguments are required in that order; missing or extra
arguments are refused. `-h` and `--help` are accepted only as the sole
argument. Every path must be absolute and normalized. The workspace and
evidence directories must not exist, while both parent directories must
already exist. The lock file may already exist in its trusted local parent.

The command admits the validated case and a fresh, non-overlapping evidence
destination before any editor launch. On success it clearly labels the run
generic and non-representative, then prints the retained workspace path,
published evidence path, and `report.json` SHA-256. A workspace that has been
created is deliberately retained for inspection; the runner does not delete
it.

Once admission succeeds, a controlled editor, workspace, analysis, runtime,
or report-assembly failure publishes an always-failing generic report when it
can do so safely. The command prints that report's identity and exits nonzero.
Failures before admission, and failures while publishing evidence itself,
cannot claim a published report and instead return only an error.

## Closed rehearsal

The runner performs exactly 22 serialized editor operations:

1. version, advanced help, and project information for all three inputs;
2. two independent JSON reconstruction round trips;
3. replacement of one existing animation, followed by the same import again;
4. one positive new-animation import plus an isolated duplicate-name collision
   control and collision export; and
5. one fixed `Images path not found: ./images/` negative control.

Every command is shell-free and pinned to Spine 4.3.23, the case-pinned
executable digest, a minimal environment, a fixed working directory, and the
embedded pretty/nonessential JSON export preset. The editor lock covers every
call. Nonzero exits, unexpected warnings, transcript-policy failures, wrong
output paths, incomplete cleanup, or identity changes fail closed.

The duplicate-name control never touches the positive new-animation candidate.
It starts from a distinct writable copy of the validated new-submission package,
where the requested animation already exists. The only accepted editor outcome
is exit 0, empty stderr, the exact request-bound import, imported-animation,
and collision lines, a changed collision-control project, and no additional
transcript text. That expected diagnostic remains a failed process assessment
and is accepted only in operation slot 19; ordinary animation imports reject
it.

Before and after the editor calls, the harness inventories immutable packages
and writable projects. Staging and snapshots reject symlinks, hard-linked
regular files, special entries, mount crossings, case-folded aliases,
nonportable paths, and configured size/depth/entry limit violations. Source
packages and the embedded preset are rechecked after the final operation.

The proof stage then checks normalized and semantic round-trip differences,
setup and per-animation fingerprints, replacement/new import isolation,
existing-import idempotence, the isolated new-animation collision hazard, and
package preservation. The collision export must preserve setup and every prior
animation, add exactly the transcript-named animation, and give that renamed
animation the same name-independent content fingerprint as the submitted
animation. Current, existing-import, and positive new-import runtime bundles
are all checked through the same strict Spinal runtime-bundle contract used by
native and WebAssembly consumers. The collision-control package is evidence,
not a runtime target. Unsupported or degraded runtime content is rejected
rather than silently accepted.

## Versioned contracts

Case manifests currently use `format_version = 2`. All tables reject unknown
keys, and a case cannot weaken these fixed policies:

- `target_spine_version` is exactly `4.3.23`;
- `runtime_atlas` is a safe package-relative `.atlas` path;
- the export preset is `pretty-nonessential-json`;
- the volatile pointer list is exactly `[/skeleton/hash]`;
- that pointer is approved only for a present string-to-different-string
  change;
- each package root is absolute and describes a complete package context;
- every project, atlas, required directory, and asset root is a safe portable
  relative path;
- every asset root is also required, so empty asset directories remain
  evidence;
- replacement and new animation names are distinct; and
- skeleton and animation names may not begin with `-`.

The current, replacement-submission, and new-submission roots may be the same
only when one complete fixture package genuinely contains multiple source
projects.

Evidence reports currently use `format_version = 4`. Report metadata fixes the
scope to `generic_rehearsal` and `representative_gate_eligible` to `false`.
It also contains closed provenance derived by the harness rather than supplied
by the caller:

- the exact harness executable bytes and a path-free stable-file-identity
  digest, observed before admission and rechecked immediately before report
  preparation;
- contextual build-checkout HEAD, dirty state and hashed Git status, the exact
  embedded workspace `Cargo.lock`, the actual `rustc -vV` identity, and Cargo
  build-host and target triples;
- runtime operating system, process architecture and kernel family;
- the exact case, three role-tagged package trees, and approved export-preset
  identities; and
- one observed Spine launcher identity that must remain identical across all
  22 process captures and match the case-pinned digest.

Build-checkout data is explicitly context only, not an attestation that the
binary came from that commit. Missing or malformed build context and changed
harness or launcher identity make a successful report impossible. Controlled
failure reports preserve typed unavailable, changed, inconsistent, or mismatch
states and remain false. Phase 0A does not fabricate Bevy, WASM, browser, or GPU
metadata; those belong to Phase 0B.

The result is derived from the complete required assertion catalog, all 22
assessed processes, exact content-addressed artifact identities, semantic
differences, runtime validations, and report-integrity checks; callers cannot
supply passing assertions or relabel the scope.

A successful evidence directory has a fixed private layout:

- `case.toml` and `package-inventories.json`;
- `native-validations.json`;
- three files beneath `comparisons/`;
- exact retained stdout and stderr evidence for each operation beneath
  `processes/`; and
- `report.json`, published only after every other file succeeds.

A controlled-failure directory uses a separate private layout beneath
`attempt/`: a machine-readable `failure.json`, an optional privacy-safe copy of
the case manifest, and optional retained stdout/stderr pairs. Unsafe raw
transcript pairs and unsafe diagnostics are withheld, while their stream
digests and omission state remain recorded. Its `report.json` always has
`passed: false`, `representative_gate_eligible: false`, and the exact required
assertion catalog with `passed`, `failed`, `missing`, `skipped`, or `degraded`
statuses derived from the typed evidence that completed before the failure.

Individual artifacts and the report are limited to 64 MiB, the complete
published bundle to 512 MiB, and license-owner text is rejected unless it is
the exact redacted line `Licensed to: <hidden>`. Directories and files are
created with private permissions on supported local macOS and Linux
filesystems.

## Safety boundary

The subprocess adapter drains both output streams with fixed memory and time
limits, hashes all observed bytes, uses a separate cleanup deadline, and
terminates the whole process group on failure. If cleanup cannot be proved,
the coordinator retains the acquired editor lock and refuses later calls
until restart.

Filesystem calls and process spawn are still operating-system blocking APIs,
so wall-clock bounds assume the required local filesystem remains responsive.
The checked path is reopened by the operating system when launching Spine;
there is therefore a documented residual same-user path race. Removing that
race requires a separately reviewed descriptor-bound launcher rather than an
unsafe fork hook in this crate.

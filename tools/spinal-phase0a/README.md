# Spinal Phase 0A evidence harness

This opt-in repository tool contains fail-closed building blocks for proving
Spine editor round trips and whole-animation imports. It is not part of the
viewer. Typed operations can invoke Spine, but the complete linear gate runner
and evidence-directory writer are not implemented yet; Phase 0A remains
**NOT RUN**.

The checked-in foundation provides:

- a strict TOML case schema pinned to Spine 4.3.23;
- full-package inventories whose SHA-256 changes when file bytes or empty
  directories change;
- rejection of symlinks and special filesystem entries;
- descriptor-relative, no-follow staging into fresh private writable copies,
  with a final source-package recheck for the end of a run;
- shell-free typed commands pinned to Spine 4.3.23 for version, capability,
  project-info, JSON export/reconstruction, negative control, and whole-
  animation import operations;
- an embedded approved pretty/nonessential export preset whose exact bytes are
  checked before and after every export;
- strict typed project-info extraction and exact one-skeleton selection;
- bounded strict JSON parsing, canonical pretty output, semantic differences,
  setup fingerprints, and independent per-animation fingerprints;
- exact-path created/updated output discovery with before/after SHA-256;
- a fixed assertion catalog where omitted evidence becomes an explicit failed
  row;
- immutable validated cases bound to the exact source TOML digest;
- atomically serialized process request, canonical executable and working
  directory identity, hashed allowlisted environment, capture, transcript
  digests, lock acquisition, cleanup, and assessment evidence;
- a report result derived from its assertions, assessed processes, validated
  content-addressed artifacts, and semantic differences;
- volatile approval derived from the immutable case policy rather than a
  caller-supplied trust flag;
- a real shell-free subprocess adapter that resolves and hashes a regular
  executable immediately before launch, uses an explicit canonical working
  directory, clears ambient environment, closes stdin, and records only hashes
  of fixed allowlisted environment values;
- one nonblocking poll loop that fairly drains stdout and stderr in bounded
  quanta, retains a fixed raw prefix, hashes every observed byte, and claims a
  full-stream digest only after EOF;
- separate execution and cleanup deadlines, whole-process-group termination,
  pre-reserved bounded cleanup capacity, and a nonblocking child reaper on
  macOS and Linux; other platforms are rejected rather than silently weakened;
- a persistent no-follow OS lock file in a canonical user-owned,
  group/world-non-writable, verified-local parent, plus a locked executor
  wrapper that binds acquisition evidence to every complete editor call and
  poisons the coordinator until restart if cleanup is ever incomplete;
- an injectable process boundary with fake and helper-process tests for
  zero/nonzero exits, diagnostics, timeouts, descendant cleanup, transcript
  limits, cwd/environment isolation, unknown lines, and missing outputs.

`cases/example.toml` documents every manifest field. Copy it outside the
repository, replace all `/external/evidence/...` package roots and the editor
checksum, and keep the operational projects and generated evidence outside
Git.

## Case contract

All tables reject unknown keys. The fixed policies cannot be weakened by a
case:

- `format_version` is `1`;
- `target_spine_version` is `4.3.23`;
- the export preset is `pretty-nonessential-json`;
- the volatile pointer list is exactly `[/skeleton/hash]`;
- that pointer is approved only for a present string-to-different-string
  change;
- package roots are absolute, while every declared package member is a safe
  portable relative path;
- every asset root is also a required directory so an empty asset root remains
  evidence;
- replacement and new animation names are distinct;
- skeleton and animation names may not begin with `-`.

The current, replacement-submission, and new-submission roots each describe a
complete package context. Roots may be the same only when a fixture genuinely
stores multiple source projects in one complete package.

The case contract remains format version `1`. Serialized evidence reports use
format version `3`, which includes canonical launch identities, hashed
environment values, retained-prefix and full-stream distinctions, termination
and cleanup status, acquired lock evidence, and a fixed expected outcome for
each recorded process. Negative controls can match only a checked-in expected
failure category; arbitrary predicates and caller-supplied diagnostic strings
are not accepted.

The retained-prefix limit is an evidence policy, not a signal to stop reading.
Overflow makes the assessment fail, while the adapter continues draining and
hashing until EOF or the execution deadline. This preserves a truthful complete
stream digest for finite output without allowing unbounded memory growth.
Requests are themselves capped at a 30-minute execution deadline, 30-second
cleanup deadline, and four MiB retained prefix per stream.

The adapter accepts only a regular non-setid executable that is root- or
effective-user-owned, not group/world-writable, and hosted with its immediate
parent on a trusted local filesystem. It records device, inode, owner, mode,
size, modification/change times, and SHA-256, then rechecks path identity after
spawn. The canonical working directory must likewise be a controlled,
effective-user-owned local directory that is not group/world-writable.

The lock is checked by name relative to an open trusted-parent descriptor
before and after acquisition, and that parent descriptor remains held for the
life of the lock. If cleanup is delegated, unavailable, or exceeds its
deadline, the coordinator deliberately retains the acquired lock and refuses
all subsequent editor executions until restart. This prevents a later Spine
launch from overlapping a child whose death has not been proved.

These deadlines bound the runner's own nonblocking poll, drain, termination,
and reaping state machine. Filesystem canonicalization/open/stat/read and the
operating system's spawn call are blocking APIs; the hard wall-clock claim
therefore assumes the already-required local filesystems remain responsive.
`Command` also reopens the checked executable and working-directory paths, so
this slice documents a residual same-user path race. Eliminating that residual
would require a separately reviewed fd-bound launcher/helper rather than
unsafe fork hooks in this crate.

The canonical lock parent is part of the same effective-user trust boundary.
Dirfd-relative no-follow checks and retaining the opened parent narrow path
replacement races, but cannot defend against a malicious process running as
that same user and able to replace entries in the trusted directory.

## Deliberately deferred

The remaining work is one private run-workspace boundary, the complete linear
round-trip/import orchestrator, assertion derivation from its closed operation
inventory, native Spinal validation, and an atomic privacy-preserving evidence
writer. The representative project has not been run. Arbitrary caller-supplied
assertion results remain intentionally unavailable, and mutation work remains
blocked until the complete gate passes.

The subprocess adapter by itself cannot claim output discovery. Only the typed
Spine runner upgrades a capture after checking exact paths. Reviewed operation
profiles reject unknown output, and unexpected license text is excluded from
serialized diagnostics.

# Spinal Phase 0A evidence harness

This opt-in repository tool contains the fail-closed contracts for proving
Spine editor round trips and whole-animation imports. It is not part of the
viewer and it does not run Spine in this first slice.

The checked-in foundation provides:

- a strict TOML case schema pinned to Spine 4.3.23;
- full-package inventories whose SHA-256 changes when file bytes or empty
  directories change;
- rejection of symlinks and special filesystem entries;
- a fixed assertion catalog where omitted evidence becomes an explicit failed
  row;
- immutable validated cases bound to the exact source TOML digest;
- atomically serialized process request, capture, transcript digests, and
  assessment evidence;
- a report result derived from its assertions, assessed processes, validated
  content-addressed artifacts, and semantic differences;
- volatile approval derived from the immutable case policy rather than a
  caller-supplied trust flag;
- an injectable process boundary with tests for zero/nonzero exits,
  diagnostics, timeouts, unknown transcript lines, and missing outputs.

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

## Deliberately deferred

The real editor adapter, exclusive cross-process lock, timeout enforcement,
race-resistant package staging, output discovery, JSON normalization and
fingerprints, evidence-directory writer, and full round-trip/import
orchestration land in later reviewed slices. Until captured 4.3.23
informational output is reviewed and checked in, the transcript policy accepts
only blank output; any unknown nonblank line fails closed.

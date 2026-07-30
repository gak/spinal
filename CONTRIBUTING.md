# Contributing to Spinal

Thank you for helping build Spinal. Correctness matters, but clean-room
provenance is a condition of every contribution.

## Before starting

1. Read [CLEANROOM.md](CLEANROOM.md) in full.
2. Check [SOURCES.toml](SOURCES.toml) for the permitted documents supporting
   the feature.
3. If a required source is not registered, add it before implementation.
4. If you have previously inspected official Spine runtime source or a
   derivative port, disclose that to a maintainer before doing implementation
   work.
5. Start with an independently written failing test derived from permitted
   documentation or a properly recorded editor export.

Do not search for or inspect reference runtime implementations to answer an
implementation question. If the public documentation and editor output are
insufficient, document the uncertainty and design a black-box editor
experiment, or leave the behavior unsupported with an explicit diagnostic.

## Fixture requirements

Normative fixtures must follow the provenance checklist in `CLEANROOM.md`.
Do not commit example artwork or exported data unless its redistribution
status is known and recorded. Prefer minimal, project-owned fixtures produced
specifically for one test.

## Pull request expectations

A contribution should:

- keep the standalone `spinal` core independent of Bevy and any renderer;
- preserve `MIT OR Apache-2.0` licensing;
- add tests before implementation for changed behavior;
- report unsupported data explicitly instead of silently producing a
  plausible but incorrect result;
- update `SOURCES.toml` when a technical source or claim changes; and
- pass formatting, linting, tests, documentation, and dependency-policy
  checks.

## Required contributor attestation

Include the following statement in every pull request that changes code,
tests, fixtures, generated data, or technical documentation:

> I attest that this contribution was created without inspecting or using
> official Spine runtime source, source derived from an official Spine
> runtime, decompiled runtime code, or material relayed from those sources. I
> derived it only from sources permitted by `CLEANROOM.md`, and I have
> registered every material technical source and fixture provenance required
> by that policy. I submit my original contribution under
> `MIT OR Apache-2.0`.

Do not provide the attestation if it is not true. Explain the provenance
concern privately to a maintainer so the affected work can be isolated or
reassigned.

## Contribution licensing

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in this project, as defined by Apache License 2.0,
is licensed under `MIT OR Apache-2.0` without additional terms or conditions.

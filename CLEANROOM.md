# Clean-room policy

Spinal is an independent implementation of the documented Spine data formats
and behavior. This policy protects that independence and applies to source
code, tests, fixtures, documentation, review comments, generated tables, and
other implementation material accepted into this repository.

This is a project contribution policy, not legal advice.

## Implementation boundary

Implementation work may use only the following categories of input:

1. Official, public, user-facing documentation published by Esoteric
   Software. Every technical document used must be recorded in
   [SOURCES.toml](SOURCES.toml).
2. Data exported by a properly licensed copy of the Spine editor, including
   JSON, binary, atlas, image, warning, and command-line output. A fixture must
   record its editor version, export settings, origin, and checksum before it
   becomes normative.
3. Black-box observation of the Spine editor's documented controls and
   exported data. Observations must be reproducible without an official
   runtime.
4. General mathematical, graphics, Rust, and game-development references that
   are independent of Spine. Record a reference when it materially determines
   an algorithm or compatibility decision.
5. This repository's original, project-owned 2022 Spinal source and history.
   It may be reused and revised because it was written under the same
   clean-room boundary. Its behavior is not evidence of 4.3.23 conformance.

Documentation describes the contract. Editor exports provide test data.
Neither permits copying an implementation from elsewhere.

## Prohibited inputs

Do not inspect or use:

- source code from any official Spine runtime;
- source code from ports, wrappers, forks, or other runtimes derived from an
  official Spine runtime;
- decompiled, disassembled, or otherwise reverse-engineered runtime binaries;
- source, comments, tests, fixtures, constants, lookup tables, control flow,
  internal names, or algorithm structure copied or translated from a
  prohibited runtime;
- generated explanations or patches produced by asking a person or tool to
  inspect prohibited material; or
- an official or derivative runtime as an executable comparison oracle.

Linking to, adapting, transliterating, or rewriting prohibited code in another
programming language does not make it a permitted clean-room input.

When researching on the web, stop if a page presents runtime implementation
source. Do not quote, summarize, save, or use that material. Record only the
permitted document actually used.

## Fixtures and conformance evidence

A normative fixture must include or be accompanied by:

- the exact Spine editor version;
- whether it is JSON or binary;
- the skeleton and atlas export settings;
- the texture-packer settings when packed images are involved;
- the origin and redistribution status of all source artwork;
- cryptographic checksums for the exported files; and
- a short statement of the feature the fixture demonstrates.

Public Spine example data may be used only when its applicable terms permit
the intended storage and redistribution. If redistribution is unclear, keep
the data out of the public repository and retain only independently written
metadata or tests that do not disclose it.

Historical sample files and exports from other Spine versions may support
regression work, but they cannot establish 4.3.23 conformance. Living web
documentation can also change after its access date, so exact 4.3.23 editor
exports remain the final wire-format evidence.

## Design and review records

For every new format feature or behavior:

1. Add or update its permitted source in `SOURCES.toml`.
2. Write an independently phrased contract or test expectation from that
   source.
3. Add the smallest fixture needed to demonstrate the behavior, with the
   provenance described above.
4. Implement the behavior without consulting prohibited material.
5. Have a reviewer check both correctness and clean-room provenance.

Reviewers must reject a change whose origin cannot be explained from the
registered sources and fixtures.

## Prior exposure

Someone who has previously seen official or derivative runtime source must
disclose that fact to a maintainer before contributing implementation work.
Exposure does not automatically exclude a contributor, but the maintainer may
reassign the feature, narrow the work to non-implementation tasks, or require
an additional independent review. Do not contribute remembered code,
structure, constants, tests, or implementation details.

If prohibited material is encountered during work, stop immediately. Tell a
maintainer what category of material was seen without copying it into an
issue, commit, or chat. The affected implementation should be isolated and
reassigned before work resumes.

## Licensing boundary

Original contributions accepted under this policy are licensed as
`MIT OR Apache-2.0`, as described in the repository license files. That
licensing choice does not grant rights to Spine software, trademarks,
artwork, examples, or editor exports.

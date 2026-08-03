# Loader fuzzing

The two byte-oriented targets exercise skeleton JSON and text-atlas parsing
through the public combined loader. Successful skeleton JSON loads also
traverse every linked public ID, sample and play every retained animation,
solve world and IK state, traverse active diagnostics, and construct
renderer-neutral draw items. Successful text-atlas loads traverse every page
and region and construct a skeleton when the combined load succeeds.

Run them with `cargo fuzz run skeleton_json` and
`cargo fuzz run text_atlas` from this directory.

Each target has a checked-in, documentation-derived valid seed so mutation
reaches schema linking rather than only syntax rejection. CI runs a bounded
seed-corpus smoke pass on every change and a coverage-guided 30-second pass per
target when CI is started manually. Longer local fuzzing remains the primary
safety tool.

Corpus inputs and dictionaries must follow `../CLEANROOM.md`. Do not seed a
target with files or behavior derived from an official or third-party runtime.
Exact editor exports need the same provenance and redistribution review as
other fixtures.

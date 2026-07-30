# External compatibility fixtures

This directory records exact-version editor exports used as clean-room
compatibility tripwires without redistributing their skeleton data, atlases, or
artwork. They are not the complete, normative supported-profile fixture set
required for a conformance claim.

The files listed in [MANIFEST.toml](MANIFEST.toml) stay outside this
repository. To run their contract test, arrange the files as:

```text
<root>/
  ess/
    spineboy-ess.json
    spineboy-ess.atlas
    spineboy-ess.png
  pro/
    spineboy-pro.json
    spineboy-pro.atlas
    spineboy-pro.png
```

Then run the checksummed standalone and Bevy compatibility checks:

```console
tools/verify-external-fixtures.sh <root>
```

The harness verifies all six checksums, exercises every animation through the
standalone loader/player/solver, and loads each JSON + atlas + PNG compound asset
through Bevy.

[COVERAGE.toml](COVERAGE.toml) tracks every first-profile feature separately.
It distinguishes available implementation evidence from the project-owned
editor exports and saved presets that are still required for production
conformance.

[PROJECT_INTAKE.md](PROJECT_INTAKE.md) is the exact handoff checklist for
José's project-owned exports. It separates immutable raw editor output from
derived test fixtures and records the evidence needed to close Stage 0 without
guessing at editor behavior.

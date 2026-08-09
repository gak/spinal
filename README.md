# Spinal

Spinal is a clean-room, renderer-independent Rust runtime core for Spine 2D
data. The active implementation is being rebuilt from this repository's
original 2022 work, using only the inputs permitted by
[CLEANROOM.md](CLEANROOM.md).

## Status

Spinal has completed the standalone **Stage 4: stateful animation and solved
frames** gate, includes the **Stage 5 Bevy adapter**, originally completed on
0.18 and now migrated whole-workspace to 0.19, and has
completed the **Stage 6 AnimationMixer** and **Stage 7 weighted mesh**
capability gates. The crates remain on the pre-release `0.1.0` development
line until the API and behavior are ready
for a maintainer-selected version. The adapter remains
provisional pending project-owned profile fixtures and a production
asset-backed visual canary. Exact 4.3.23 Spineboy Essential and Professional
exports passed the historical Bevy 0.18 external load, sample, solve, and
compound-asset checks. Those external fixtures were unavailable for this
migration, so a fresh Bevy 0.19 compound-asset run is **NOT RUN**.
Spinal is not ready for production use.
There is no release date or public stability promise.

The staged capability gates and supported production subset are tracked in
[ROADMAP.md](ROADMAP.md). Existing users can review the additive API changes
in [MIGRATING-0.4.md](MIGRATING-0.4.md).

The active `spinal` crate currently provides:

- a standalone core with no Bevy dependency;
- immutable, shareable asset data and owned skeleton instances;
- asset-scoped typed identifiers;
- checked angle, mix, and transform value types;
- JSON and multi-page text-atlas parsing and linking;
- typed retained animation data for the first profile;
- exact-tick, deterministic absolute timeline sampling;
- ordered attachment-only skin composition for independent cosmetics;
- a one-track player with exact events, absolute seek with event rebaselining,
  and interruption-safe crossfades;
- a permanent-base, ordered-override `AnimationMixer` with sparse continuous
  contributions, independent track weights, weight fades, and crossfades;
- a scoped procedural edit phase followed by authored-order IK and direct
  world-rotation transform constraints;
- skeleton-space control targets resolved through the current mixed parent
  pose before constraints;
- an allocation-free indexed draw stream for rigid regions, weighted meshes,
  unweighted meshes, and linked meshes;
- structured warnings plus active-frame degraded-feature diagnostics; and
- a fresh Bevy 0.19 compound loader, compatible one-track ECS facade,
  declarative named override tracks, hot-reload recovery, ordered indexed
  region-and-mesh renderer, track-aware owned events, and red-cross
  degradation markers.

The current parser is **not yet fully conformant with Spine 4.3.23**. Historical,
checksummed exact-version Spineboy exports passed the supported profile at the
Bevy 0.18 checkpoint, but the external-fixture matrix has not been rerun on the
current Bevy 0.19 adapter. Project-owned fixtures covering every supported
feature and a representative production export are still pending. Legacy files
in this repository are historical inputs, not 4.3.23 conformance fixtures.

The reviewed roadmap for consolidating native and browser Preview/Compare,
Diagnostics, and safe animation-update intake into one Spinal product is the
[Spinal Application Consolidation Plan](PLAN-SPINAL-APPLICATION-CONSOLIDATION.md).
It is a staged implementation plan, not a release announcement; its licensed
Spine evidence gates have not yet passed.

The plan owns product boundaries, phase order, and gate consequences. Detailed
mechanics and retained logs live in the
[Phase 0 Evidence Runbook](docs/PHASE-0-EVIDENCE-RUNBOOK.md) and the conditional
[Coordinator Recovery Runbook](docs/COORDINATOR-RECOVERY-RUNBOOK.md).

The first wire-format target is Spine 4.3.23 JSON plus text atlases. Exact
4.3.23 editor exports with recorded settings and checksums are required before
the parser can claim complete supported-profile conformance. The recommended
production settings are in [EXPORT_PROFILE.md](EXPORT_PROFILE.md).

## Architecture

- `spinal` is the renderer- and engine-independent runtime core.
- `bevy_spinal` is a fresh Bevy 0.19 adapter around that standalone core.
- `apps/spinal` is the one Spinal application. Its current read-only native and
  browser Open/Preview/Compare/Diagnostics surface shares immutable intake,
  animation, skin, loop, fixed playback-speed, absolute-timeline, and linked
  pan/zoom/Fit-view camera controls and is documented in
  [apps/spinal/web/README.md](apps/spinal/web/README.md).
  The feature-rich `bevy_spinal` `runtime_showcase` example remains an adapter
  and conformance harness only; product session, browser, Review, and
  coordinator work belongs in `apps/spinal`.
- The historical Bevy 0.8 prototype was removed rather than upgraded.

Keeping the runtime core independent makes it usable by other renderers and
engines while allowing the Bevy plugin to focus on asset loading, extraction,
rendering, and developer diagnostics.

## Open a read-only Preview

Run `just open` (or invoke `spinal` without paths) to choose one Spine JSON
export with the native system picker. Cancellation exits without opening a
window. Existing positional Preview and explicit `--compare` launches remain
available for scripts and repeatable paths. On Linux, Open uses the first
available `zenity`, `kdialog`, or `yad`; a missing or failed picker is an
explicit error rather than cancellation.

Run `just web` to open the browser's local-directory form. Select one
JSON/text-atlas/PNG export directory; Spinal enumerates and bounds the selection,
reads only the required files, fully validates an immutable in-memory bundle,
and only then starts a paused Preview. The selected bytes remain in that browser
tab and are not uploaded. Explicit authenticated browser manifests remain the
adapter for repeatable Preview and Compare launches.

This is launch-only runtime-export intake. It does not open `.spine` project
files or ZIPs, create project/Base/Submission/Proposed state, save or mutate
assets, start `spinal serve`, or authorize the later Review/promotion workflow.

## Read-only native check

The Spinal application can validate and inventory one complete JSON/atlas/PNG
export without opening a window:

```text
just check path/to/rig.spine.json
just check path/to/rig.spine.json --json
```

The command uses the same immutable intake and runtime loader as Preview. It
prints stable virtual paths, bundle hashes and sizes, inventory counts, ordered
animation and skin summaries, and bounded stable-name diagnostics. Successful
JSON contains no absolute host paths or timestamps. The command does not create
a project, candidate, sidecar, or approval record.

Automation should branch on `format_version`, `status`, and the stable
`error.code`/optional `error.reason` fields, never on human messages. Catalogs,
authored names, diagnostics, file intake, and canonical JSON output all have
fixed limits; omitted or clipped values are reported explicitly. Compatible
and degraded v1 output are protected by exact-byte golden tests.

Preview derives its contextual Diagnostics from that same inspection. The
native sidebar deliberately shows one finding plus an explicit remainder count;
the wider browser disclosure shows up to eight. `spinal check` is the expanded
inspection view, while still preserving the inspection model's own hard safety
limits and truncation sentinel.

Exit status is `0` for compatible, `1` for loadable with deliberate
degradation, `2` for invalid arguments, `3` for unavailable or rejected
input, and `4` for an internal output failure. This is a runtime compatibility
check, not a complete Spine 4.3.23 conformance claim or an evidence-gate
decision.

## Clean-room development

No official Spine runtime source, derivative runtime, translated port,
decompiled runtime, or tests copied from such a runtime may be used to
implement Spinal.

Permitted implementation inputs include official public user documentation,
outputs produced by a properly licensed Spine editor, general mathematical
references, and this repository's original project-owned 2022 source. The
complete policy is in [CLEANROOM.md](CLEANROOM.md), and the implementation
source register is [SOURCES.toml](SOURCES.toml).

Contributors must read [CONTRIBUTING.md](CONTRIBUTING.md) and provide the
clean-room attestation described there.

## Licensing

Spinal's original code is licensed under either:

- [Apache License 2.0](LICENSE-APACHE); or
- [MIT License](LICENSE-MIT),

at your option. In SPDX form: `MIT OR Apache-2.0`.

This code license does not grant rights to the Spine editor, Spine trademarks,
third-party artwork, or exported content. Contributors and users are
responsible for complying with the licenses that apply to those materials.
Historical example exports have their own [asset notices](assets/README.md)
and are excluded from packaged crates.

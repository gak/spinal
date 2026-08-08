# Spinal

Spinal is a clean-room, renderer-independent Rust runtime core for Spine 2D
data. The active implementation is being rebuilt from this repository's
original 2022 work, using only the inputs permitted by
[CLEANROOM.md](CLEANROOM.md).

## Status

Spinal has completed the standalone **Stage 4: stateful animation and solved
frames** gate, includes the fresh **Stage 5 Bevy 0.18 adapter**, and has
completed the **Stage 6 AnimationMixer** and **Stage 7 weighted mesh**
capability gates. The crates remain on the pre-release `0.1.0` development
line until the API and behavior are ready
for a maintainer-selected version. The adapter remains
provisional pending project-owned profile fixtures and a production
asset-backed visual canary. Exact 4.3.23 Spineboy Essential and Professional
exports pass the external load, sample, solve, and Bevy compound-asset checks.
Spinal is not ready for production use.

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
- a fresh Bevy 0.18 compound loader, compatible one-track ECS facade,
  declarative named override tracks, hot-reload recovery, ordered indexed
  region-and-mesh renderer, track-aware owned events, and red-cross
  degradation markers.

The current parser is **not yet fully conformant with Spine 4.3.23**. External,
checksummed exact-version Spineboy exports now pass, but project-owned
fixtures covering every supported feature and a representative production
export are still pending. Legacy files in this repository are historical
inputs, not 4.3.23 conformance fixtures.

The first wire-format target is Spine 4.3.23 JSON plus text atlases. Exact
4.3.23 editor exports with recorded settings and checksums are required before
the parser can claim complete supported-profile conformance. The recommended
production settings are in [EXPORT_PROFILE.md](EXPORT_PROFILE.md).

## Architecture

- `spinal` is the renderer- and engine-independent runtime core.
- `bevy_spinal` is a fresh Bevy 0.18 adapter around that standalone core.
- `apps/spinal-viewer` is the dedicated read-only desktop viewer for exported
  skeletons. The feature-rich `bevy_spinal` `runtime_showcase` example remains
  an advanced adapter test harness.
- The historical Bevy 0.8 prototype was removed rather than upgraded.

Keeping the runtime core independent makes it usable by other renderers and
engines while allowing the Bevy plugin to focus on asset loading, extraction,
rendering, and developer diagnostics.

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

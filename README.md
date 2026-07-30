# Spinal

Spinal is a clean-room, renderer-independent Rust runtime core for Spine 2D
data. The active implementation is being rebuilt from this repository's
original 2022 work, using only the inputs permitted by
[CLEANROOM.md](CLEANROOM.md).

## Status

Spinal has completed **Stage 4: stateful animation and solved frames** for the
provisional clean-room profile. The standalone player, procedural pose phase,
world and IK solver, and renderer-neutral draw stream have passed the Stage 4
review gate. Spinal is not ready for production use.

The staged capability gates and supported Loafstead subset are tracked in
[ROADMAP.md](ROADMAP.md).

The active `spinal` crate currently provides:

- a standalone core with no Bevy dependency;
- immutable, shareable asset data and owned skeleton instances;
- asset-scoped typed identifiers;
- checked angle, mix, and transform value types;
- JSON and multi-page text-atlas parsing and linking;
- typed retained animation data for the first profile;
- exact-tick, deterministic absolute timeline sampling;
- ordered attachment-only skin composition for independent cosmetics;
- a one-track player with exact events and interruption-safe crossfades;
- a scoped procedural edit phase followed by world transforms and basic IK;
- an allocation-free rigid-region draw stream; and
- structured warnings plus active-frame degraded-feature diagnostics.

The current parser is **not yet conformant with Spine 4.3.23**. Exact-version
fixtures are still pending. The renderer-independent Stage 4 behavior is
implemented provisionally from registered public documentation, while Bevy
rendering and Loafstead integration remain the next gate. Legacy files in this
repository are historical inputs, not 4.3.23 conformance fixtures.

The first wire-format target is Spine 4.3.23 JSON plus text atlases. Exact
4.3.23 editor exports with recorded settings and checksums are required before
the parser can claim conformance.

## Architecture

- `spinal` is the renderer- and engine-independent runtime core.
- A fresh Bevy 0.18 adapter is planned around the standalone core.
- `bevy_spinal` currently contains only the excluded historical Bevy 0.8
  prototype. It is not part of the supported workspace or the new adapter's
  architecture.

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

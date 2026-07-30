# spinal

`spinal` is the renderer-independent core of a clean-room Rust runtime for
Spine 2D data.

The crate is being rebuilt from the repository's original 2022 implementation.
Its first planned wire target is Spine 4.3.23 JSON plus the modern text atlas
format. Loading, animation evaluation, IK, and rendering integration land in
later stages; this release establishes the asset-safe public model they use.

The core has no Bevy dependency. A fresh Bevy 0.18 plugin will live in the
separate `bevy_spinal` crate.

The implementation is based only on public Spine documentation,
editor-generated exports, general animation mathematics, and original work in
this repository. Contributors must not inspect or translate official or
third-party Spine runtime source code.

The complete [roadmap](https://github.com/gak/spinal/blob/main/ROADMAP.md),
[clean-room policy](https://github.com/gak/spinal/blob/main/CLEANROOM.md), and
[contribution guide](https://github.com/gak/spinal/blob/main/CONTRIBUTING.md)
live in the workspace repository.

Spinal is dual-licensed under [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE), at your option.

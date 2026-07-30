# spinal

`spinal` is the renderer-independent core of a clean-room Rust runtime for
Spine 2D data.

The crate is being rebuilt from the repository's original 2022 implementation.
Its first wire target is Spine 4.3.23 JSON plus the modern text atlas format.
The standalone loader accepts caller-owned bytes, validates and links the
closed first-profile subset, preserves supported animation timelines for
runtime evaluation, samples local poses at exact integer-tick boundaries, and
returns structured diagnostics for safely retained unsupported data.

```rust
use spinal::{Skeleton, load_json};

# fn example(json: &[u8], atlas: &[u8]) -> Result<(), spinal::LoadError> {
let report = load_json(json, atlas)?;
let asset = report.into_asset();
let mut skeleton = Skeleton::new(asset.clone());
assert!(!skeleton.asset().bones().collect::<Vec<_>>().is_empty());
if let Some(animation) = asset.animations().next() {
    skeleton
        .sample_animation(
            animation.id(),
            std::time::Duration::ZERO,
            spinal::PlaybackMode::Once,
        )
        .expect("animation and skeleton share one asset");
}
# Ok(())
# }
```

The loader performs no filesystem, image-decoding, rendering, or engine work.
Exact 4.3.23 compatibility remains a target rather than a conformance claim
until checksummed editor-generated fixtures from that exact version are
available.

The initial demo API assumes trusted inputs whose byte and element counts are
bounded by the caller. It validates structure and recursion, but intentionally
does not yet expose allocation limits. A configurable `LoadLimits` policy is a
post-demo roadmap item.

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

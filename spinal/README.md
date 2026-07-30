# spinal

`spinal` is the renderer-independent core of a clean-room Rust runtime for
Spine 2D data.

The crate is being rebuilt from the repository's original 2022 implementation.
Its first wire target is Spine 4.3.23 JSON plus the modern text atlas format.
The standalone loader accepts caller-owned bytes, validates and links the
closed first-profile subset, preserves supported animation timelines for
runtime evaluation, and returns structured diagnostics for safely retained
unsupported data. The runtime adds exact event delivery, interruption-safe
one-track crossfades, a procedural edit phase, world transforms, basic IK, and
an allocation-free renderer-neutral rigid-region draw stream.

```rust
use std::time::Duration;

use spinal::{AnimationPlayer, PlayOptions, Skeleton, load_json};

# fn example(json: &[u8], atlas: &[u8]) -> Result<(), spinal::LoadError> {
let report = load_json(json, atlas)?;
let asset = report.into_asset();
let mut skeleton = Skeleton::new(asset.clone());
if let Some(animation) = asset.animations().next() {
    let mut player = AnimationPlayer::new(&skeleton);
    player
        .play(animation.id(), PlayOptions::looping())
        .expect("animation and skeleton share one asset");
    let frame = player
        .update(&mut skeleton, Duration::from_millis(16), &mut ())
        .expect("the player remains bound to its skeleton")
        .solve();
    for item in frame.draw_items() {
        # let _item = item;
        // Submit the renderer-neutral item to an engine adapter.
    }
}
# Ok(())
# }
```

The core performs no filesystem, image-decoding, rendering, or engine work.
External checksummed Spineboy Essential and Professional exports from 4.3.23
pass load, animation sampling, and frame solving. Complete 4.3.23
supported-profile conformance remains a target until project-owned fixtures
cover every supported feature with their complete export presets.

The initial demo API assumes trusted inputs whose byte and element counts are
bounded by the caller. It validates structure and recursion, but intentionally
does not yet expose allocation limits. A configurable `LoadLimits` policy is a
post-demo roadmap item.

The core has no Bevy dependency. The fresh Bevy 0.18 plugin lives in the
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

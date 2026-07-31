# spinal

`spinal` is the renderer-independent core of a clean-room Rust runtime for
Spine 2D data.

The crate is being rebuilt from the repository's original 2022 implementation.
Its first wire target is Spine 4.3.23 JSON plus the modern text atlas format.
The standalone loader accepts caller-owned bytes, validates and links the
closed first-profile subset, preserves supported animation timelines for
runtime evaluation, and returns structured diagnostics for safely retained
unsupported data. The runtime adds exact event delivery, a compatible
one-track player, sparse ordered override tracks, interruption-safe
crossfades, independent track-weight fades, a procedural edit phase, world
transforms, basic IK, direct world-rotation transform constraints in authored
order, and an allocation-free renderer-neutral rigid-region draw stream.

```rust
use std::time::Duration;

use spinal::{
    AnimationMixer, Mix, PlayOptions, Skeleton, TrackOptions, WeightFade,
    load_json,
};

# fn example(json: &[u8], atlas: &[u8]) -> Result<(), spinal::LoadError> {
let report = load_json(json, atlas)?;
let asset = report.into_asset();
let mut skeleton = Skeleton::new(asset.clone());
if let Some(animation) = asset.animations().next() {
    let mut mixer = AnimationMixer::new(&skeleton);
    mixer
        .base_track_mut()
        .play(animation.id(), PlayOptions::looping())
        .expect("animation and skeleton share one asset");
    if let Some(overlay) = asset.animations().nth(1) {
        let track = mixer
            .insert_track(TrackOptions::override_track().with_weight(Mix::ZERO))
            .expect("the mixer has track identity capacity");
        let mut track = mixer.track_mut(track).expect("the track remains present");
        track
            .play(overlay.id(), PlayOptions::looping())
            .expect("animation and skeleton share one asset");
        track.fade_weight(
            Mix::ONE,
            WeightFade::new(Duration::from_millis(120)),
        );
    }
    let frame = mixer
        .update(&mut skeleton, Duration::from_millis(16), &mut ())
        .expect("the mixer remains bound to its skeleton")
        .solve();
    for item in frame.draw_items() {
        # let _item = item;
        // Submit the renderer-neutral item to an engine adapter.
    }
}
# Ok(())
# }
```

`AnimationMixer` always has one base track. Override tracks run from low to
high priority and affect only continuous properties authored by their active
animation. Missing properties leave the live lower-track value untouched,
including during interrupted crossfades. v0.4 applies bone translation,
rotation, scale magnitude, and shear; slot colour; IK mix; and transform
constraint mix channels.

Attachment switches, draw order, IK bend direction, and scale-sign changes
remain base-track-only in v0.4. An override animation containing one of these
properties still loads and continues applying its supported properties.
`AnimationRef::override_compatibility()` reports the deferred properties, and
an active visible track exposes them through
`AnimationMixer::active_deferred_properties()`.

Track animation clocks use scaled playback time. Crossfades and weight fades
use wall time and continue while a track is paused. Authored events follow
playback clocks and are delivered in deterministic base-to-high-track order.
They are not suppressed by track weight; an outgoing transition source stops
emitting as soon as a new playback or stop command replaces it.
Unit speed preserves wall `Duration` exactly; other speeds use deterministic
binary-`f32` scaling with nearest-nanosecond rounding. Unrepresentable scaled
deltas return `PlayerError::TimeOverflow` before mutation rather than
silently clamping the clock.
Each player or track update preflights a fixed 65,536-occurrence event safety
ceiling. Exceeding it returns `PlayerError::EventLimitExceeded` without
changing the clock, pose, report, or event sink.

The simpler `AnimationPlayer` API remains available and source compatible for
single-track users.

The core performs no filesystem, image-decoding, rendering, or engine work.
External checksummed Spineboy Essential and Professional exports from 4.3.23
pass load, animation sampling, and frame solving. Complete 4.3.23
supported-profile conformance remains a target until project-owned fixtures
cover every supported feature with their complete export presets.

The initial demo API assumes trusted inputs whose byte and element counts are
bounded by the caller. It validates structure and recursion and enforces the
fixed event safety ceiling, but intentionally does not yet expose configurable
resource limits. A public limits policy is a post-demo roadmap item.

The core has no Bevy dependency. The fresh Bevy 0.18 plugin lives in the
separate `bevy_spinal` crate. Event-free steady-state mixer evaluation,
including active override tracks and solved frames, allocates nothing after
construction and warmup. Commands and caller-owned event storage are outside
that claim.

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

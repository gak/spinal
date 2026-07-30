# bevy_spinal

`bevy_spinal` is the fresh Bevy 0.18 adapter for the renderer-independent
`spinal` core.

The adapter owns Bevy asset loading, ECS playback intent, hot-reload recovery,
owned frame output, ordered multi-page batching, and visible degraded-feature
markers. Skeleton parsing, animation sampling, crossfades, events, world
transforms, and IK remain in `spinal`.

This crate does not upgrade or reuse the historical Bevy 0.8 prototype that
previously occupied this directory.

External checksummed Spineboy Essential and Professional exports from 4.3.23
pass the compound loader. Complete supported-profile conformance and
Loafstead's real asset-backed canary remain pending project-owned cat exports.

## Quick start

Add Bevy's asset plugin before `SpinalPlugin`, as the normal Bevy plugin
groups do:

```rust,no_run
use bevy::prelude::*;
use bevy_spinal::{SpinalAnimator, SpinalAsset, SpinalInstance, SpinalPlugin};

# fn main() {
    App::new()
        .add_plugins((DefaultPlugins, SpinalPlugin))
        .add_systems(Startup, |mut commands: Commands, assets: Res<AssetServer>| {
            let cat: Handle<SpinalAsset> = assets.load("cat.spine.json");
            commands.spawn((
                SpinalInstance::new(cat),
                SpinalAnimator::looping("idle"),
            ));
        })
        .run();
# }
```

For `cat.spine.json`, the loader infers a sibling `cat.atlas`; page names
inside the atlas resolve relative to that atlas. A typed plain `cat.json`
load is also supported through Bevy's `load_with_settings` API, which selects
`SpinalAssetLoaderSettings` explicitly. Each atlas page becomes a stable
`#page-N` labeled `Handle<Image>`.

The initial renderer contract is straight-alpha PNG pages and normal slot
blending. Data outside the documented profile remains loadable when its record
boundary is safe, but affected draw items are omitted and the instance enters
`SpinalInstanceState::Degraded`, or `DegradedNoDraws` when the current frame
has no drawable items. When Bevy's gizmo plugin is present, an obvious red
cross marks the affected bone, slot, or skeleton root.
Use the repository's
[export profile](https://github.com/gak/spinal/blob/main/EXPORT_PROFILE.md) for
the shared production settings.

## ECS API

- `SpinalInstance` selects the typed asset and requires transform, visibility,
  playback, skin, override, and observation components automatically.
- `SpinalAppearance` adds per-instance Bevy colour modulation and local-space
  horizontal or vertical facing without requiring a negative parent scale.
- `SpinalAnimator` is declarative one-track intent. Repeating `play` or
  calling `restart` always issues a new playback, so same-name restarts are
  unambiguous.
- `SpinalSkinLayers` composes attachment-only skins from low to high priority.
- `SpinalPoseOverrides` applies stable-name local bone replacements after
  animation and before IK.
- `SpinalInstanceState::DegradedNoDraws` distinguishes a live diagnostic
  runtime whose current frame has no drawable items. `has_drawable_output()`
  reports geometry, not visual completeness; consumers may require `Ready`
  before hiding a known-complete sprite fallback.
- `SpinalPlaybackState` exposes the current playback ID, animation name,
  mode, local position, loop index, completion, and transition influence.
- `SpinalAnimationEvent` and `SpinalIssue` are owned Bevy messages and can be
  retained after the frame that emitted them.
- `SpinalSet::{Prepare, Animate, Render}` are stable schedule integration
  points.

Public intent uses names deliberately. A successful hot reload atomically
rebuilds the private `Skeleton` and `AnimationPlayer`, then reapplies the
requested animation, skins, and procedural overrides by name. A failed reload
keeps the last good compound asset and frame.

After warmup, an unchanged instance reuses its resolved names, playback
observation, solved-frame storage, and draw buffers without allocating.

Disable the default `render` feature for a headless loader and ECS runtime.
The standalone `spinal` crate remains usable without Bevy. The adapter
re-exports its exact core dependency as `bevy_spinal::spinal`, so an
application using only the Bevy facade does not need to declare a duplicate
core dependency for playback and pose types.

## Viewer

Build the small inspection app with:

```text
cargo run -p bevy_spinal --example viewer --features viewer
```

The bundled fixture is project-authored from public format documentation. It
is useful for adapter smoke tests, while the untracked exact-version examples
are exercised by the external fixture tests. Pass a project-owned asset path
and animation name to inspect a production export.

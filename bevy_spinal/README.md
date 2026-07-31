# bevy_spinal

`bevy_spinal` is the fresh Bevy 0.18 adapter for the renderer-independent
`spinal` core.

The adapter owns Bevy asset loading, ECS playback intent, hot-reload recovery,
owned frame output, declarative named override tracks, ordered multi-page
batching, and visible degraded-feature markers. Skeleton parsing, animation
mixing, crossfades, events, world transforms, IK, and the supported rotation
transform-constraint subset remain in `spinal`.

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

Add a sparse override such as aim without replacing the base animator:

```rust,no_run
use std::time::Duration;

use bevy_spinal::{
    SpinalAnimationTracks,
    spinal::{Mix, PlaybackMode, Transition, WeightFade},
};

let mut tracks = SpinalAnimationTracks::default();
tracks.set_weight("aim", Mix::ZERO);
tracks.play("aim", "aim", PlaybackMode::Loop, Transition::Immediate);
tracks.fade_weight(
    "aim",
    Mix::ONE,
    WeightFade::new(Duration::from_millis(120)),
);
```

Insert `tracks` beside `SpinalInstance`. Changing `SpinalAnimator` from
`walk` to `eat` or `fall` leaves the named aim playback and its weight
untouched.

## ECS API

- `SpinalInstance` selects the typed asset and requires transform, visibility,
  playback, skin, override, and observation components automatically.
- `SpinalAppearance` adds per-instance Bevy colour modulation and local-space
  horizontal or vertical facing without requiring a negative parent scale.
  Its `world_to_skeleton_position` helper converts cursor or other world
  points through both `GlobalTransform` and the selected facing.
- `SpinalAnimator` is declarative one-track intent. Repeating `play` or
  calling `restart` always issues a new playback, so same-name restarts are
  unambiguous.
- `SpinalAnimationTracks` declares stable named override tracks from low to
  high priority. Playback commands restart deliberately; pause, speed, and
  constant-weight setters are idempotent. `move_to` changes priority without
  restarting a track. Removing and recreating a key creates a fresh track
  incarnation, even in one frame; a simultaneous hot reload cannot transfer
  the deleted track's presented weight into that replacement. Replacing the
  whole component with an independently constructed value likewise declares
  fresh tracks; mutate the existing component to issue commands against its
  live tracks.
- `SpinalTrackStates` exposes each named track's playback, presented and
  target weights, active weight-fade state, pause, and speed.
- `SpinalSkinLayers` composes attachment-only skins from low to high priority.
- `SpinalPoseOverrides` applies stable-name local bone replacements after
  animation and before ordered constraint solving.
- `SpinalControlTargets` moves named bone origins in skeleton space through
  the current mixed parent pose, after local overrides and before constraints.
  Use the appearance conversion helper first when the source point is in Bevy
  world space.
- `SpinalInstanceState::DegradedNoDraws` distinguishes a live diagnostic
  runtime whose current frame has no drawable items. `has_drawable_output()`
  reports geometry, not visual completeness; consumers may require `Ready`
  before hiding a known-complete sprite fallback.
- `SpinalPlaybackState` exposes the current playback ID, animation name,
  mode, local position, loop index, completion, and transition influence.
- `SpinalAnimationEvent` and `SpinalIssue` are owned Bevy messages and can be
  retained after the frame that emitted them. Override events and issues
  expose their stable track key.
- `SpinalSet::{Prepare, Animate, Render}` are stable schedule integration
  points.

Public intent uses names deliberately. A successful hot reload atomically
rebuilds the private `Skeleton` and `AnimationMixer`, then reapplies the base
animation, ordered named tracks, skins, local overrides, and control targets
by name. A failed reload keeps the last good compound asset and frame.

After warmup, an event- and issue-free instance reuses its resolved names,
playback observations, mixer storage, solved-frame storage, and draw buffers
without allocating. Moving an existing control target also reuses its storage.
Active diagnostic construction and caller-owned event storage are outside this
claim.

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

The viewer can keep one sparse overlay playing while the arrow keys crossfade
between base animations. It can also drive a skeleton-space control bone from
the mouse. The original Spineboy exports intentionally remain unsupported for
rendering because they use meshes and premultiplied alpha. Prepare a temporary
rigid, straight-alpha preview from the unmodified Essential and Professional
4.3.23 exports first:

```text
preview_root=$(mktemp -d)
tools/prepare-spineboy-aim-preview.sh \
  /path/to/4.3.23-fixtures/ess \
  /path/to/4.3.23-fixtures/pro \
  "$preview_root"
cargo run -p bevy_spinal --example viewer --features viewer -- \
  --asset-root "$preview_root" \
  --asset spineboy-rigid-aim.json --animation walk --overlay-animation aim \
  --scale 0.65 \
  --mouse-target crosshair
```

The helper requires `jq` and ImageMagick's `magick` command. It keeps the
supported `idle`, `walk`, `run`, and `aim` clips, uses Essential's rigid region
attachments, and does not modify either source export.

Press Left or Right to change the base animation while aim remains live.
Press `M` to pause or resume mouse tracking.

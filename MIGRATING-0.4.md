# Migrating to Spinal v0.4

v0.4 adds layered animation without replacing the existing single-track API.
Code using `AnimationPlayer`, `SpinalAnimator`, or `SpinalPlaybackState` can
continue unchanged.

## Standalone core

Use `AnimationMixer` when an animation such as `aim` should remain active
while the base changes between `walk`, `eat`, and `fall`.

```rust,ignore
let mut mixer = AnimationMixer::new(&skeleton);
mixer
    .base_track_mut()
    .play(walk, PlayOptions::looping())?;

let aim_track = mixer.insert_track(
    TrackOptions::override_track().with_weight(Mix::ZERO),
)?;
mixer
    .track_mut(aim_track)?
    .play(aim, PlayOptions::looping())?;
mixer
    .track_mut(aim_track)?
    .fade_weight(Mix::ONE, WeightFade::new(Duration::from_millis(120)));
```

`TrackId` belongs to one mixer instance. Do not persist it across asset reloads
or reconstruct it from numeric values. Use `AnimationMixer::tracks`,
`AnimationMixer::track`, and `AnimationMixer::move_track` for ordered
observation and priority changes.

`AnimationMixer::update` returns the same `EditablePose` phase used by
`AnimationPlayer`. Apply local edits or skeleton-space targets, then solve
constraints once:

```rust,ignore
let mut pose = mixer.update(&mut skeleton, delta, &mut events)?;
pose.targets().set_skeleton_position(crosshair, cursor)?;
let frame = pose.solve();
```

## Bevy adapter

`SpinalAnimator` remains the permanent base-track facade. Add
`SpinalAnimationTracks` for ordered override intent:

```rust,ignore
tracks.set_weight("aim", Mix::ZERO);
tracks.play("aim", "aim", PlaybackMode::Loop, Transition::Immediate);
tracks.fade_weight("aim", Mix::ONE, fade);
```

`play`, `restart`, `stop`, and `fade_weight` are commands. Calling them again
has an observable effect. `set_paused`, `set_speed`, and `set_weight` are
idempotent state setters. `move_to` changes priority without restarting the
track. Removing and recreating the same stable key creates a fresh track
incarnation, including when it happens in the same frame as a hot reload.
Replacing the whole component with an independently constructed value also
declares fresh tracks; mutate the existing component to retain live track
continuity.

Read `SpinalTrackStates` for per-track playback, current and target weights,
fade state, pause, and speed. `SpinalAnimationEvent::track` and
`SpinalIssue::track` return the stable override key, or `None` for the base or
an entity-wide issue.

Use `SpinalControlTargets::set_skeleton_position` for a control bone such as
`crosshair`. Existing target names update in place without allocating. The
adapter resolves the position through the current mixed parent pose after
local overrides and before authored constraints.

For a cursor or other Bevy world-space point, first call
`SpinalAppearance::world_to_skeleton_position`. It inverts the entity
`GlobalTransform` and the appearance's horizontal or vertical facing, so a
flipped character still targets the visible point.

## Override subset

v0.4 override tracks apply:

- bone translation, rotation, scale magnitude, and shear;
- slot colour;
- IK mix; and
- every retained transform-constraint mix channel.

They intentionally ignore attachment switches, draw order, IK bend direction,
and bone scale-sign changes. These records still load. Inspect
`AnimationRef::override_compatibility` before playback, or observe active
track-scoped diagnostics and red-cross markers at runtime.

Authored events follow playback clocks even when track weight is zero. Weight
fades and clip crossfades use wall time and continue while the animation clock
is paused. A new play or stop command ends event delivery from the outgoing
transition source immediately.

To prevent one extreme delta on a very short eventful loop from monopolizing a
frame, each player or track update has a fixed internal ceiling of 65,536
authored event occurrences. `PlayerError::EventLimitExceeded` is returned
during preflight, leaving clocks, poses, reports, and the event sink unchanged.
The deferred post-demo limits work will make resource policy configurable; it
is not required for ordinary trusted demo assets.

Unit-speed clocks preserve the supplied `Duration` exactly. Other finite
speeds use deterministic binary-`f32` scaling and nearest-nanosecond rounding.
If the scaled result cannot be represented by `Duration`, the whole mixer
update returns `PlayerError::TimeOverflow` before mutation.

## Hot reload

The Bevy adapter rebuilds private mixer IDs after a successful asset
replacement and reapplies base intent, named override order, skins, local
overrides, and control targets by stable name. Runtime playback starts again
from the declared intent. Never cache core IDs across reload.

## Still deferred

v0.4 does not add additive tracks, arbitrary masks, blend trees, queues,
reverse playback, weighted meshes, deform timelines, clipping, path
constraints, physics constraints, or configurable loader allocation limits.

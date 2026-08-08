# Spinal AnimationMixer plan

This plan defines the standalone and Bevy-facing contract for layered
animation in Spinal's `0.1.0` development line. It is a focused
replacement-style mixer for a production runtime, not a general animation
graph.

Status: implemented and verified on the `0.1.0` development line.

The implementation remains subject to [CLEANROOM.md](CLEANROOM.md). Its Spine
behavior is derived only from the public documents registered in
[SOURCES.toml](SOURCES.toml), especially `spine-applying-animations` and
`spine-api-reference`. No official or derivative runtime source is an
implementation input.

## Outcome

A consuming application can play a base animation such as `walk`, `eat`, or
`fall` while a higher `aim` animation continues to control only the properties
it keys. The aim track can change weight or animation without freezing the
live base pose. The final mixed local pose remains procedurally editable before world
transforms and authored constraints are solved once.

The current claim is deliberately narrow: best-in-class Rust and Bevy ergonomics,
deterministic behavior, diagnostics, and failure atomicity for this subset. It
does not claim feature parity with a general blend-tree or animation-graph
system.

## Supported mixer profile

- One permanent base track and zero or more ordered override tracks.
- Stable mixer-scoped `TrackId` values that reject foreign or removed tracks.
- One active playback per track.
- Per-track looping, pausing, nonnegative finite speed, and constant weight.
- Independent weight fades and within-track interruption-safe crossfades.
- Sparse continuous contributions for:
  - bone translation, rotation, scale, and shear;
  - slot colour;
  - IK mix; and
  - every retained transform-constraint mix channel.
- Track-aware authored events, lifecycle reports, status, and diagnostics.
- Procedural skeleton-space control targets and scoped local-pose editing after
  animation mixing and before one authored-order constraint solve.
- Allocation-free unchanged steady-state update after mixer and track
  construction.
- The existing `AnimationPlayer` and one-track Bevy components remain source
  compatible with the current development line.

The following remain outside the current mixer profile:

- additive blending;
- arbitrary caller-authored bone or property masks;
- blend trees, state machines, blend spaces, and asset-authored graphs;
- animation queues, reverse playback, and negative speed;
- attachment, draw-order, IK bend-direction, or scale-sign changes from an
  override track;
- deform timelines and other features outside the active production export
  profile. Weighted meshes were delivered separately in Roadmap Stage 7 and
  do not change the mixer property model.

An override animation containing a deferred discrete property still loads.
The track ignores that property, reports it through track compatibility and
active diagnostics, and continues applying supported continuous properties.

## Public standalone surface

The implemented common path is:

```rust,ignore
let mut mixer = AnimationMixer::new(&skeleton);

mixer
    .base_track_mut()
    .play(walk, PlayOptions::looping())?;

let aim = mixer.insert_track(TrackOptions::override_track())?;
mixer
    .track_mut(aim)?
    .play(aim_animation, PlayOptions::looping())?;
mixer
    .track_mut(aim)?
    .fade_weight(Mix::ONE, WeightFade::new(Duration::from_millis(120)));

let mut pose = mixer.update(&mut skeleton, wall_delta, &mut events)?;
pose.targets().set_skeleton_position(crosshair, mouse_position)?;
let frame = pose.solve();
```

The following API properties are acceptance requirements:

- Commands such as `play`, `restart`, and `remove_track` are explicit actions.
- Setters such as `set_paused`, `set_speed`, and `set_weight` are idempotent.
- Every fallible command validates before mutation.
- IDs identify runtime objects; stable names belong in the Bevy intent layer.
- Observation borrows where practical and does not require frame-by-frame
  allocation.
- `AnimationRef::properties()` exposes authored property metadata.
- A caller can inspect override-track compatibility before playing an
  animation.

## Composition semantics

Tracks are evaluated from low to high. The base track reconstructs a complete
pose relative to setup pose, preserving the existing one-track behavior.
Every override track then contributes only authored continuous properties
whose first key is active at the sampled time.

For one continuous property, an override contribution is:

```text
output = mix(live_lower_value, authored_value, contribution * track_weight)
```

`live_lower_value` is read after all lower tracks have been evaluated during
the same update. A missing contribution leaves the lower value unchanged.
Track order is stable and observable.

A crossfade remains in contribution space. It never freezes a whole skeleton
pose. Source and target contributions are evaluated against the same live
lower value, then mixed by the transition amount. This rule also applies
after rapid interruption, so a changing base animation continues beneath a
partially faded aim track.

Angular channels retain an explicit branch for the life of each property
transition. Scale magnitudes interpolate continuously; a sign change is a
deferred discrete override property and is therefore ignored with a
diagnostic in the current mixer profile.

Weight fades use wall-clock time, independent of animation speed and pause.
Animation clocks use scaled playback time. Crossfades use wall-clock time.
Wall-time fades and unit-speed animation clocks are observably equivalent
whether an accepted elapsed interval is supplied as one update or split into
smaller updates. Non-unit speeds use exact binary-`f32` scaling followed by
documented nearest-nanosecond rounding for each update. A scaled delta that
cannot fit Rust's `Duration` is rejected during preflight with
`PlayerError::TimeOverflow`; it is never silently clamped.

## Transactional update contract

Before mutating clocks, transitions, skeleton pose, event sinks, or reports,
an update validates:

- the bound skeleton instance;
- every active animation ID;
- loop-index arithmetic;
- the fixed authored-event safety ceiling;
- track identities and ordering.

If validation fails, no mixer state, skeleton pose, event output, or caller
observation changes. Successful events are emitted in deterministic order:
track order first, then chronological and authored source order within each
track.

As agreed for the demo, configurable load and event-count policies remain a
post-demo API. The current mixer does enforce a fixed internal ceiling of
65,536 authored event occurrences per player or track update. An update that
would exceed it returns `PlayerError::EventLimitExceeded` during preflight,
before any pose, clock, report, or event-sink mutation.

## Bevy surface

`SpinalAnimator` and `SpinalPlaybackState` remain the one-track facade.
Layered users add declarative components whose stable application keys are
independent of runtime `TrackId` values:

```rust,ignore
tracks.play("aim", "aim", PlaybackMode::Loop, transition);
tracks.fade_weight("aim", Mix::ONE, fade);
tracks.set_paused("aim", false);
tracks.restart("aim");
```

The adapter resolves names after load and hot reload, preserves intent across
asset replacement, exposes per-track observation, and routes track-aware
events and diagnostics through owned Bevy messages. Repeating an idempotent
setter does not restart playback. Repeating `play` or calling `restart` does.
`SpinalAppearance::world_to_skeleton_position` converts world points through
the entity transform and horizontal or vertical facing before they are stored
as control targets.

## Implementation stages and gates

### M1: executable property contract

- Add public property keys and `AnimationRef::properties()`.
- Precompute ordered unique property metadata at load time.
- Classify properties as base-supported, override-supported, or deferred.
- Add a deliberately slow test-only reference compositor.

Gate: metadata snapshots and compatibility reports cover every retained
timeline type, with no update-loop allocation.

### M2: sparse contribution sampler

- Split complete-pose sampling from sparse continuous contribution sampling.
- Preserve bit-for-bit one-track sampling and player behavior.
- Add per-property contribution and angular branch storage.

Gate: the base path matches all existing snapshots, and sparse sampling
changes no unkeyed property.

### M3: standalone tracks and constant-weight overrides

- Add mixer-scoped IDs, permanent base-track access, ordered insertion,
  removal, status, and validated mutation.
- Compose constant-weight override tracks over the live lower pose.
- Solve constraints once after all tracks and procedural edits.

Gate: `walk + aim` keeps the walk body motion while aim controls only its
authored properties; foreign and removed IDs fail atomically.

### M4: fades, interruption, events, and diagnostics

- Add independent weight fades and within-track crossfades.
- Collapse interrupted transitions in sparse contribution space.
- Add track-aware events, reports, active animation diagnostics, skin remap,
  and transactional overflow behavior.
- Add first-class skeleton-space control targets, including current-parent
  conversion and reflected-facing tests.

Gate: rapid `walk -> eat -> fall` interruption does not disturb the live aim
track, and moving the target remains correct through a rotated or reflected
root.

### M5: Bevy facade and hot reload

- Add stable named track intent and per-track observation components.
- Preserve the current one-track components as a compatibility facade.
- Rebuild runtime tracks from declarative names after asset replacement.
- Extend the viewer with a base-animation selector plus mouse-driven aim.

Gate: the viewer changes base animation while continuously following the
mouse, and hot reload restores the same declared tracks without stale IDs.

### M6: release verification

- Compare optimized sparse composition against a separate slow reference
  compositor over generated play, stop, restart, reorder, immediate-weight,
  weight-fade, crossfade, and delta sequences.
- Exercise every supported continuous property family directly, including
  angular branch replacement and absence/re-entry cases, and compare accepted
  single versus split updates using accumulated lifecycle pulses.
- Add allocation, deterministic replay, package-content, documentation,
  Clippy, formatting, and dependency-policy gates.
- Exercise the exact external Spineboy exports plus a derived drawable
  walk/run-and-aim preview. Exercise a project-owned representative export
  when it becomes available.
- Update crate versions, READMEs, roadmap status, and migration notes.

Gate: every mixer acceptance requirement has direct test or runtime evidence;
all repository gates pass; packages contain no restricted example assets.

The implementation gate is complete. Stable and Rust 1.89 run the core,
headless Bevy, viewer, documentation, Clippy, and all-feature test matrices.
The package allowlist, historical-asset checksums, package archive,
dependency policy, parser fuzz smokes, exact 4.3.23 Spineboy exports, and the
derived drawable walk/run-and-aim preview also pass. A project-owned
representative export remains unavailable; its intake is a separate
production-conformance and production-canary gate rather than evidence for the
mixer contract.

## Review findings incorporated

Five independent reviews shaped this contract. The principal correction was
that a property mask alone is insufficient: a crossfade must remain a sparse
contribution and must be reapplied over the current lower-track pose every
frame. The reviews also required explicit time domains, deterministic
track-aware events, stable identity rules, hot-reload reconstruction,
transactional failure behavior, and preservation of the existing one-track
facade.

Three subsequent implementation and release reviews covered the standalone
contract, Bevy integration, and verification evidence. Their medium-or-higher
findings were fixed and the final reviews returned GO with no remaining
medium-or-higher findings.

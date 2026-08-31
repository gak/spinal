//! Public contract tests for the six single-axis bone timelines
//! (`translatex`, `translatey`, `scalex`, `scaley`, `shearx`, `sheary`).
//!
//! Fixture provenance: `SINGLE_AXIS_EXPORT_JSON` below is the byte-identical
//! editor-returned export from a project-owned, project-authored skeleton
//! built for RigTogether and round-tripped once through a licensed Spine
//! 4.3.23 editor install. It contains no licensee identity, no third-party
//! artwork, and no redistributed editor assets; every number in it is
//! project-owned. See `fixtures/COVERAGE.toml` (id
//! `single-axis-timeline-project-owned`) for the ledger entry.

use std::{sync::Arc, time::Duration};

use spinal::{
    AnimationMixer, Mix, PlayOptions, PlaybackMode, Skeleton, SkeletonAsset, TrackOptions,
    load_json,
};

/// Byte-identical to the editor-returned export staged at
/// `target/spinal-fixtures/single_axis-export/single_axis.json` in the
/// RigTogether project (produced with licensed Spine 4.3.23; the authored
/// input that was round-tripped documents provenance only and is not
/// itself normative). Key objects that omit `time`/`value` rely on the
/// editor's own zero defaults, exactly as returned.
const SINGLE_AXIS_EXPORT_JSON: &[u8] = br#"{
"skeleton": {
	"hash": "xlIqC2sN7VE",
	"spine": "4.3.23",
	"x": -64,
	"y": -64,
	"width": 128,
	"height": 128,
	"images": "./images",
	"audio": "./audio"
},
"bones": [
	{ "name": "root" },
	{ "name": "spin", "parent": "root", "rotation": 30, "x": 10, "y": 5 },
	{ "name": "tip", "parent": "spin", "rotation": -15, "x": 20 },
	{ "name": "mirror", "parent": "root", "x": -10, "scaleX": -1 },
	{ "name": "leaf", "parent": "mirror", "rotation": 45, "x": 5 }
],
"slots": [
	{ "name": "back", "bone": "spin", "attachment": "back" },
	{ "name": "gap", "bone": "root" },
	{ "name": "mid", "bone": "tip", "color": "80ff40cc", "attachment": "mid" },
	{ "name": "front", "bone": "leaf", "attachment": "front" }
],
"skins": [
	{
		"name": "default",
		"attachments": {
			"back": {
				"back": { "x": 2, "y": 1, "rotation": 10, "width": 8, "height": 6 }
			},
			"front": {
				"front": { "x": 1, "y": -1, "rotation": -90, "width": 6, "height": 2 }
			},
			"mid": {
				"mid": { "color": "ff8040aa", "width": 4, "height": 2 }
			}
		}
	}
],
"animations": {
	"axis": {
		"bones": {
			"spin": {
				"translatex": [
					{
						"curve": [ 0.25, 1.75, 0.75, 5.25 ]
					},
					{ "time": 1, "value": 7 }
				],
				"translatey": [
					{},
					{ "time": 1, "value": -4 }
				]
			},
			"tip": {
				"scalex": [
					{},
					{ "time": 1, "value": 2 }
				],
				"scaley": [
					{},
					{ "time": 1, "value": 0.5 }
				],
				"shearx": [
					{},
					{ "time": 1, "value": 15 }
				],
				"sheary": [
					{},
					{ "time": 1, "value": -10 }
				]
			}
		}
	}
}
}"#;

const SINGLE_AXIS_ATLAS: &[u8] = b"\
page.png
\tsize: 128, 64
\tfilter: Linear, Linear
\trepeat: none
\tpma: false
back
\tbounds: 0, 0, 8, 6
mid
\tbounds: 0, 0, 4, 2
front
\tbounds: 0, 0, 6, 2
";

fn fixture() -> (Arc<SkeletonAsset>, Skeleton) {
    let asset = load_json(SINGLE_AXIS_EXPORT_JSON, SINGLE_AXIS_ATLAS)
        .expect("the oracle-returned single-axis export loads")
        .into_asset();
    let skeleton = Skeleton::new(Arc::clone(&asset));
    (asset, skeleton)
}

#[test]
fn the_oracle_export_loads_with_no_unsupported_timeline_diagnostics() {
    let report = load_json(SINGLE_AXIS_EXPORT_JSON, SINGLE_AXIS_ATLAS)
        .expect("the oracle-returned single-axis export loads");
    // All six single-axis names (translatex/translatey/scalex/scaley/
    // shearx/sheary) must retire the unsupported-timeline diagnostic: a
    // clean load with zero diagnostics proves none of them fell through to
    // `TimelineData::Unsupported`.
    assert_eq!(
        report.diagnostics(),
        &[],
        "single-axis bone timelines must no longer be reported unsupported"
    );
}

#[test]
fn single_axis_timelines_sample_exactly_at_authored_keys() {
    let (asset, mut skeleton) = fixture();
    let axis = asset.animation_id("axis").expect("axis animation exists");
    let spin = asset.bone_id("spin").expect("spin bone exists");
    let tip = asset.bone_id("tip").expect("tip bone exists");

    // At time 0 every timeline's first key is either an explicit 0/0 pair
    // or an omitted-field default (translate -> 0, scale -> 1, shear -> 0),
    // so every bone must land exactly on its setup transform.
    skeleton
        .sample_animation(axis, Duration::ZERO, PlaybackMode::Once)
        .expect("axis animation is asset-local");
    let spin_t0 = skeleton
        .bone_pose(spin)
        .expect("spin bone belongs to the skeleton")
        .local_transform();
    assert_eq!(
        spin_t0.translation().x,
        10.0,
        "setup x, untouched at time 0"
    );
    assert_eq!(spin_t0.translation().y, 5.0, "setup y, untouched at time 0");
    let tip_t0 = skeleton
        .bone_pose(tip)
        .expect("tip bone belongs to the skeleton")
        .local_transform();
    assert_eq!(tip_t0.scale().x, 1.0);
    assert_eq!(tip_t0.scale().y, 1.0);
    assert_eq!(tip_t0.shear().x().as_degrees(), 0.0);
    assert_eq!(tip_t0.shear().y().as_degrees(), 0.0);

    // At time 1 (the second and last key of every timeline) sampling must
    // return the authored value exactly, composed over each bone's setup
    // value on its own axis:
    //   spin.x  = setup 10 + translatex 7  = 17
    //   spin.y  = setup  5 + translatey -4 =  1
    //   tip.scaleX = setup 1 * scalex 2    =  2
    //   tip.scaleY = setup 1 * scaley 0.5  =  0.5
    //   tip.shearX = setup 0 + shearx 15   = 15 degrees
    //   tip.shearY = setup 0 + sheary -10  = -10 degrees
    skeleton
        .sample_animation(axis, Duration::from_secs(1), PlaybackMode::Once)
        .expect("axis animation is asset-local");
    let spin_t1 = skeleton
        .bone_pose(spin)
        .expect("spin bone belongs to the skeleton")
        .local_transform();
    assert!((spin_t1.translation().x - 17.0).abs() < 1.0e-4);
    assert!((spin_t1.translation().y - 1.0).abs() < 1.0e-4);
    let tip_t1 = skeleton
        .bone_pose(tip)
        .expect("tip bone belongs to the skeleton")
        .local_transform();
    assert!((tip_t1.scale().x - 2.0).abs() < 1.0e-4);
    assert!((tip_t1.scale().y - 0.5).abs() < 1.0e-4);
    assert!((tip_t1.shear().x().as_degrees() - 15.0).abs() < 1.0e-4);
    assert!((tip_t1.shear().y().as_degrees() + 10.0).abs() < 1.0e-4);
}

#[test]
fn single_axis_bezier_and_linear_curves_match_hand_computed_midpoints() {
    let (asset, mut skeleton) = fixture();
    let axis = asset.animation_id("axis").expect("axis animation exists");
    let spin = asset.bone_id("spin").expect("spin bone exists");
    let tip = asset.bone_id("tip").expect("tip bone exists");

    // spin.translatex carries the fixture's one authored curve, the
    // 4-number absolute-space bezier [0.25, 1.75, 0.75, 5.25] spanning key
    // 0 (time 0, value 0) to key 1 (time 1, value 7). Its Y control points
    // are exactly 7x its X control points (0->0, 0.25->1.75, 0.75->5.25,
    // 1->7): every authored Y_i = 7 * X_i. A cubic Bezier is a linear
    // combination of its control points, B(t) = sum(basis_i(t) * P_i), so
    // scaling every control point by a constant scales the whole curve by
    // that constant: B_y(t) = 7 * B_x(t) for every t. Spinal's evaluator
    // searches for the segment whose B_x matches the requested linear
    // fraction and returns the paired B_y there (see
    // `segmented_bezier_value_for_x` in spinal/src/animation.rs); because
    // B_y = 7 * B_x identically, that returned value is exactly 7 times
    // whatever linear fraction was requested, regardless of which of the
    // ten segments resolves the search. This authored curve is therefore
    // an affinely-linear Bezier (the editor's default, unadjusted curve
    // handles), and sampling it at time 0.2s (linear = 0.2) must give
    // exactly delta = 7 * 0.2 = 1.4; spin.x = setup 10 + 1.4 = 11.4.
    //
    // spin.translatey has no curve (defaults to linear) between key 0
    // (value 0) and key 1 (value -4), so at the same 0.2s:
    //   y = lerp(0, -4, 0.2) = -0.8; spin.y = setup 5 + -0.8 = 4.2
    skeleton
        .sample_animation(axis, Duration::from_millis(200), PlaybackMode::Once)
        .expect("axis animation is asset-local");
    let spin_pose = skeleton
        .bone_pose(spin)
        .expect("spin bone belongs to the skeleton")
        .local_transform();
    assert!(
        (spin_pose.translation().x - 11.4).abs() < 1.0e-3,
        "translatex's authored curve is affinely-linear (Y = 7X), got {}",
        spin_pose.translation().x
    );
    assert!(
        (spin_pose.translation().y - 4.2).abs() < 1.0e-4,
        "translatey has no curve and must interpolate linearly, got {}",
        spin_pose.translation().y
    );

    // tip's four axes are all plain-linear (no curve field on either key).
    // At time 0.4s (linear = 0.4):
    //   scalex value = lerp(1, 2, 0.4)   = 1.4;  tip.scaleX = 1 * 1.4 = 1.4
    //   scaley value = lerp(1, 0.5, 0.4) = 0.8;  tip.scaleY = 1 * 0.8 = 0.8
    //   shearx value = lerp(0, 15, 0.4)  = 6;    tip.shearX = 0 + 6   = 6 deg
    //   sheary value = lerp(0, -10, 0.4) = -4;   tip.shearY = 0 + -4  = -4 deg
    skeleton
        .sample_animation(axis, Duration::from_millis(400), PlaybackMode::Once)
        .expect("axis animation is asset-local");
    let tip_pose = skeleton
        .bone_pose(tip)
        .expect("tip bone belongs to the skeleton")
        .local_transform();
    assert!((tip_pose.scale().x - 1.4).abs() < 1.0e-4);
    assert!((tip_pose.scale().y - 0.8).abs() < 1.0e-4);
    assert!((tip_pose.shear().x().as_degrees() - 6.0).abs() < 1.0e-4);
    assert!((tip_pose.shear().y().as_degrees() + 4.0).abs() < 1.0e-4);
}

/// A minimal, purpose-built (non-oracle) fixture isolating the mixer-path
/// requirement that a single-axis timeline must not disturb the untouched
/// axis, even partially, when layered through an override track.
const AXIS_ISOLATION_JSON: &[u8] = br#"{
  "skeleton": { "spine": "4.3.23" },
  "bones": [
    { "name": "root" },
    { "name": "target", "parent": "root", "x": 100, "y": 200 }
  ],
  "animations": {
    "drift-x": {
      "bones": { "target": { "translatex": [ { "value": 0 }, { "time": 1, "value": 40 } ] } }
    },
    "drift-y-only": {
      "bones": { "target": { "translatey": [ { "value": 0 }, { "time": 1, "value": 80 } ] } }
    }
  }
}"#;

#[test]
fn y_only_override_leaves_x_untouched_under_partial_mixer_weight() {
    let asset = load_json(AXIS_ISOLATION_JSON, b"page.png\n\tsize: 1, 1\n")
        .expect("the axis-isolation fixture loads")
        .into_asset();
    let target = asset.bone_id("target").expect("target bone exists");
    let drift_x = asset.animation_id("drift-x").expect("drift-x exists");
    let drift_y_only = asset
        .animation_id("drift-y-only")
        .expect("drift-y-only exists");
    let mut skeleton = Skeleton::new(Arc::clone(&asset));
    let mut mixer = AnimationMixer::new(&skeleton);

    // Base track: translatex only, clamped at its final key (time 1) so
    // target.x is a live, non-setup value (100 + 40 = 140) with no
    // translatey timeline anywhere in this animation.
    mixer
        .base_track_mut()
        .play(drift_x, PlayOptions::once())
        .expect("drift-x is asset-local");
    // Override track: translatey only, at half weight, also clamped at its
    // final key. It never samples x at all.
    let half = Mix::new(0.5).expect("one half is a normalized mix");
    let track = mixer
        .insert_track(TrackOptions::override_track().with_weight(half))
        .expect("track identity remains available");
    mixer
        .track_mut(track)
        .expect("track exists")
        .play(drift_y_only, PlayOptions::once())
        .expect("drift-y-only is asset-local");

    let frame = mixer
        .update(&mut skeleton, Duration::from_secs(1), &mut ())
        .expect("both tracks sample their final key")
        .solve();
    let transform = frame
        .bone(target)
        .expect("target bone belongs to the skeleton")
        .local_transform();

    // x must be exactly the base track's live value: the y-only override
    // never contributes to `translation.x`, so it must not be dragged
    // toward setup (100) or any other placeholder, even at 50% weight.
    assert_eq!(
        transform.translation().x,
        140.0,
        "an override track with no translatex timeline must leave x exactly as the base track left it"
    );
    // y is setup 200 blended at 50% toward the override's live value
    // (200 + 80 = 280): lerp(200, 280, 0.5) = 240.
    assert!((transform.translation().y - 240.0).abs() < 1.0e-4);
}

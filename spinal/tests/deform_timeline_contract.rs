//! Public contract tests for deform timelines
//! (`animations.<clip>.attachments.<skin>.<slot>.<attachment>.deform`) on
//! both a rigid (unweighted) and a weighted mesh.
//!
//! Fixture provenance: `DEFORM_MESH_EXPORT_JSON` and `DEFORM_CURVE_EXPORT_JSON`
//! below are byte-identical editor-returned exports from project-owned,
//! project-authored skeletons built for RigTogether and round-tripped
//! through a licensed Spine 4.3.23 editor install. They contain no
//! licensee identity, no third-party artwork, and no redistributed editor
//! assets; every number in them is project-owned. See
//! `fixtures/COVERAGE.toml` (id `deform-mesh-project-owned`) for the ledger
//! entry.
//!
//! A deform key's curve encoding -- a single shared 4-number curve per key
//! (one curve for the whole vertex-delta vector, not one per component),
//! with absolute-time x values normalized within the key's own time span
//! and fraction-domain (0..1 "progress") y values -- is oracle-proven by
//! `DEFORM_CURVE_EXPORT_JSON`, not reasoned from architectural necessity
//! alone: a second round trip through the licensed 4.3.23 editor, adding
//! `"curve": [0.125, 0.2, 0.375, 0.9]` to `mid`'s first deform key (time 0,
//! next key at time 0.5), returned byte-identical, confirming exactly the
//! implemented design.

use std::{sync::Arc, time::Duration};

use spinal::{
    DiagnosticCode, DiagnosticSeverity, DrawItemRef, PlaybackMode, Skeleton, SkeletonAsset, SlotId,
    SolvedFrame, glam::Vec2, load_json,
};

/// Byte-identical to the editor-returned export staged at
/// `target/spinal-fixtures/deform_mesh-export/deform_mesh.json` in the
/// RigTogether project (produced with licensed Spine 4.3.23; the authored
/// input that was round-tripped documents provenance only and is not
/// itself normative). `mid` is a rigid (unweighted) mesh; `wmid` is a
/// weighted mesh sharing the same bone ("tip") and the same authored rest
/// geometry pattern.
///
/// This fixture oracle-proves byte-for-byte preservation of the deform
/// numbers on both mesh kinds (the round trip changes nothing). It does
/// *not*, on its own, prove which of two possible indexing layouts a
/// weighted mesh's deform delta array uses: `wmid`'s two deformed vertices
/// (0 and 1) each have exactly one bone contribution, so a "per vertex"
/// reading and the implemented "per bone contribution" reading compute the
/// same array indices there and cannot be told apart by this data alone.
/// The per-contribution layout is instead evidenced by real, multi-influence
/// editor output: see `spineboy_pro_hoverboard_deform_only_fits_the_per_influence_domain`
/// in `spinal/tests/editor_4_3_23_contract.rs`, which loads a genuine
/// Spine-exported weighted mesh whose real deform key only fits that
/// layout's bounds.
const DEFORM_MESH_EXPORT_JSON: &[u8] = br#"{
"skeleton": {
	"hash": "9atIyhowRmI",
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
	{ "name": "front", "bone": "leaf", "attachment": "front" },
	{ "name": "wmid", "bone": "tip", "attachment": "wmid" }
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
				"mid": {
					"type": "mesh",
					"uvs": [ 0, 0, 1, 0, 1, 1, 0, 1 ],
					"triangles": [ 0, 1, 2, 0, 2, 3 ],
					"vertices": [ -2.0, -1.0, 2.0, -1.0, 2.0, 1.0, -2.0, 1.0 ],
					"hull": 4,
					"edges": [ 0, 6, 0, 2, 2, 4, 4, 6 ],
					"width": 4.0,
					"height": 2.0
				}
			},
			"wmid": {
				"wmid": {
					"type": "mesh",
					"uvs": [ 0, 0, 1, 0, 1, 1, 0, 1 ],
					"triangles": [ 0, 1, 2, 0, 2, 3 ],
					"vertices": [ 1.0, 2.0, -3.0, -2.0, 1.0, 1.0, 2.0, 3.0, -2.0, 1.0, 2.0, 2.0, 3.0, 2.0, 0.6, 1.0, -8.0, 1.0, 0.4, 1.0, 2.0, -3.0, 2.0, 1.0 ],
					"hull": 4,
					"edges": [ 0, 6, 0, 2, 2, 4, 4, 6 ],
					"width": 6.0,
					"height": 4.0
				}
			}
		}
	}
],
"animations": {
	"deform": {
		"attachments": {
			"default": {
				"mid": {
					"mid": {
						"deform": [
							{},
							{
								"time": 0.5,
								"offset": 2,
								"vertices": [ 3, -2, 1, 4 ]
							},
							{ "time": 1 }
						]
					}
				},
				"wmid": {
					"wmid": {
						"deform": [
							{},
							{
								"time": 0.5,
								"vertices": [ 0.5, -0.25, 1.5, 0.75 ]
							},
							{ "time": 1 }
						]
					}
				}
			}
		}
	}
}
}"#;

const DEFORM_MESH_ATLAS: &[u8] = b"\
page.png
\tsize: 128, 64
\tfilter: Linear, Linear
\trepeat: none
\tpma: false
back
\tbounds: 0, 0, 8, 6
front
\tbounds: 0, 0, 6, 2
mid
\tbounds: 0, 0, 4, 2
wmid
\tbounds: 0, 0, 6, 4
";

/// Byte-identical to the editor-returned export staged at
/// `target/spinal-fixtures/deform_curve-export/deform_curve.json` in the
/// RigTogether project (produced with licensed Spine 4.3.23; the authored
/// input that was round-tripped documents provenance only and is not
/// itself normative). Identical to `DEFORM_MESH_EXPORT_JSON` except `mid`'s
/// first deform key (time 0) now carries `"curve": [0.125, 0.2, 0.375,
/// 0.9]` instead of being empty -- the same skeleton, oracle-round-tripped
/// a second time specifically to prove the deform curve wire encoding.
const DEFORM_CURVE_EXPORT_JSON: &[u8] = br#"{
"skeleton": {
	"hash": "cBtA0pSOY4I",
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
	{ "name": "front", "bone": "leaf", "attachment": "front" },
	{ "name": "wmid", "bone": "tip", "attachment": "wmid" }
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
				"mid": {
					"type": "mesh",
					"uvs": [ 0, 0, 1, 0, 1, 1, 0, 1 ],
					"triangles": [ 0, 1, 2, 0, 2, 3 ],
					"vertices": [ -2.0, -1.0, 2.0, -1.0, 2.0, 1.0, -2.0, 1.0 ],
					"hull": 4,
					"edges": [ 0, 6, 0, 2, 2, 4, 4, 6 ],
					"width": 4.0,
					"height": 2.0
				}
			},
			"wmid": {
				"wmid": {
					"type": "mesh",
					"uvs": [ 0, 0, 1, 0, 1, 1, 0, 1 ],
					"triangles": [ 0, 1, 2, 0, 2, 3 ],
					"vertices": [ 1.0, 2.0, -3.0, -2.0, 1.0, 1.0, 2.0, 3.0, -2.0, 1.0, 2.0, 2.0, 3.0, 2.0, 0.6, 1.0, -8.0, 1.0, 0.4, 1.0, 2.0, -3.0, 2.0, 1.0 ],
					"hull": 4,
					"edges": [ 0, 6, 0, 2, 2, 4, 4, 6 ],
					"width": 6.0,
					"height": 4.0
				}
			}
		}
	}
],
"animations": {
	"deform": {
		"attachments": {
			"default": {
				"mid": {
					"mid": {
						"deform": [
							{
								"curve": [ 0.125, 0.2, 0.375, 0.9 ]
							},
							{
								"time": 0.5,
								"offset": 2,
								"vertices": [ 3, -2, 1, 4 ]
							},
							{ "time": 1 }
						]
					}
				},
				"wmid": {
					"wmid": {
						"deform": [
							{},
							{
								"time": 0.5,
								"vertices": [ 0.5, -0.25, 1.5, 0.75 ]
							},
							{ "time": 1 }
						]
					}
				}
			}
		}
	}
}
}"#;

fn fixture() -> (Arc<SkeletonAsset>, Skeleton) {
    let asset = load_json(DEFORM_MESH_EXPORT_JSON, DEFORM_MESH_ATLAS)
        .expect("the oracle-returned deform-mesh export loads")
        .into_asset();
    let skeleton = Skeleton::new(Arc::clone(&asset));
    (asset, skeleton)
}

fn curve_fixture() -> (Arc<SkeletonAsset>, Skeleton) {
    let asset = load_json(DEFORM_CURVE_EXPORT_JSON, DEFORM_MESH_ATLAS)
        .expect("the oracle-returned deform-curve export loads")
        .into_asset();
    let skeleton = Skeleton::new(Arc::clone(&asset));
    (asset, skeleton)
}

fn mesh_positions(frame: &SolvedFrame<'_>, slot: SlotId) -> Vec<Vec2> {
    frame
        .draw_items()
        .find_map(|item| match item {
            DrawItemRef::Mesh(mesh) if mesh.slot() == slot => Some(mesh.positions().to_vec()),
            _ => None,
        })
        .expect("the mesh slot is drawn")
}

fn assert_close(actual: Vec2, expected: Vec2, label: &str) {
    assert!(
        (actual.x - expected.x).abs() < 1.0e-3 && (actual.y - expected.y).abs() < 1.0e-3,
        "{label}: expected {expected:?}, got {actual:?}"
    );
}

#[test]
fn the_oracle_export_loads_with_no_unsupported_timeline_diagnostics() {
    let report = load_json(DEFORM_MESH_EXPORT_JSON, DEFORM_MESH_ATLAS)
        .expect("the oracle-returned deform-mesh export loads");
    // The `attachments` animation section, and the rigid and weighted
    // meshes' `deform` timelines within it, must no longer fall through to
    // the generic unsupported-section/unsupported-timeline diagnostics.
    assert_eq!(
        report.diagnostics(),
        &[],
        "deform timelines must no longer be reported unsupported"
    );
}

#[test]
fn keyless_and_empty_deform_keys_leave_rest_vertices_unchanged() {
    let (asset, mut skeleton) = fixture();
    let deform = asset
        .animation_id("deform")
        .expect("deform animation exists");
    let mid = asset.slot_id("mid").expect("mid slot exists");
    let wmid = asset.slot_id("wmid").expect("wmid slot exists");

    // Both meshes' first key (time 0) and last key (time 1) are empty
    // objects with no "vertices": per the spec, an empty key means setup,
    // so sampling at either time must reproduce the authored rest
    // geometry with no offset at all, for both the rigid and the weighted
    // mesh.
    let mid_rest = [
        Vec2::new(-2.0, -1.0),
        Vec2::new(2.0, -1.0),
        Vec2::new(2.0, 1.0),
        Vec2::new(-2.0, 1.0),
    ];
    for time in [Duration::ZERO, Duration::from_secs(1)] {
        skeleton
            .sample_animation(deform, time, PlaybackMode::Once)
            .expect("deform animation is asset-local");
        let frame = skeleton.editable_pose().solve();
        let positions = mesh_positions(&frame, mid);
        let expected: Vec<Vec2> = mid_rest
            .iter()
            .map(|&local| tip_world_transform_point(local))
            .collect();
        for (index, (actual, expected)) in positions.iter().zip(&expected).enumerate() {
            assert_close(
                *actual,
                *expected,
                &format!("mid vertex {index} at {time:?}"),
            );
        }
        drop(frame);

        let frame = skeleton.editable_pose().solve();
        let wmid_positions = mesh_positions(&frame, wmid);
        // wmid shares its rest geometry's world result with a bone-weighted
        // computation; at zero deform this must match the same closed-form
        // per-vertex transform used below for the deformed case, with every
        // delta at 0. Vertex 2 splits across two bones (tip 0.6, spin 0.4);
        // the rest is single-bone (tip, weight 1.0), so its rest position
        // equals `tip_world_transform_point` directly, like the rigid mesh.
        assert_close(
            wmid_positions[0],
            tip_world_transform_point(Vec2::new(-3.0, -2.0)),
            "wmid rest vertex 0",
        );
        assert_close(
            wmid_positions[1],
            tip_world_transform_point(Vec2::new(3.0, -2.0)),
            "wmid rest vertex 1",
        );
        assert_close(
            wmid_positions[3],
            tip_world_transform_point(Vec2::new(-3.0, 2.0)),
            "wmid rest vertex 3",
        );
    }
}

/// Applies the fixture's `tip` bone world transform to one attachment-local
/// point, by hand: `tip`'s world matrix is the composition of `spin`'s
/// setup rotation (30 degrees, no scale/shear, so a pure rotation matrix)
/// with `tip`'s own setup rotation (-15 degrees, likewise pure), which
/// composes to a pure rotation of 30 + (-15) = 15 degrees; `tip`'s world
/// translation is `spin`'s rotation matrix applied to `tip`'s local
/// translation (20, 0), plus `spin`'s own world translation (10, 5).
fn tip_world_transform_point(local: Vec2) -> Vec2 {
    let spin_rotation = 30.0_f64.to_radians();
    let (spin_sin, spin_cos) = spin_rotation.sin_cos();
    let spin_translation = Vec2::new(10.0, 5.0);
    // spin's world matrix, a pure rotation since spin has no scale/shear:
    // [[cos, -sin], [sin, cos]].
    let tip_world_translation = Vec2::new(
        (spin_cos * 20.0 - spin_sin * 0.0) as f32,
        (spin_sin * 20.0 + spin_cos * 0.0) as f32,
    ) + spin_translation;

    let tip_rotation = (30.0_f64 - 15.0).to_radians();
    let (tip_sin, tip_cos) = tip_rotation.sin_cos();
    let x = tip_cos * f64::from(local.x) - tip_sin * f64::from(local.y);
    let y = tip_sin * f64::from(local.x) + tip_cos * f64::from(local.y);
    Vec2::new(x as f32, y as f32) + tip_world_translation
}

/// The bone matrix for a bone with only a setup rotation (no scale, no
/// shear): a pure rotation, `[[cos, -sin], [sin, cos]]`.
fn spin_world_transform_point(local: Vec2) -> Vec2 {
    let spin_rotation = 30.0_f64.to_radians();
    let (spin_sin, spin_cos) = spin_rotation.sin_cos();
    let x = spin_cos * f64::from(local.x) - spin_sin * f64::from(local.y);
    let y = spin_sin * f64::from(local.x) + spin_cos * f64::from(local.y);
    Vec2::new(x as f32, y as f32) + Vec2::new(10.0, 5.0)
}

#[test]
fn deform_offsets_rest_vertices_before_weighting_and_skinning() {
    let (asset, mut skeleton) = fixture();
    let deform = asset
        .animation_id("deform")
        .expect("deform animation exists");
    let mid = asset.slot_id("mid").expect("mid slot exists");
    let wmid = asset.slot_id("wmid").expect("wmid slot exists");

    // The single interior key (time 0.5) is exact (both meshes' surrounding
    // keys are 0, so sampling exactly at 0.5 lands with `linear = 0` inside
    // the [0.5, 1] span and returns key 1's values with no interpolation).
    skeleton
        .sample_animation(deform, Duration::from_millis(500), PlaybackMode::Once)
        .expect("deform animation is asset-local");
    let frame = skeleton.editable_pose().solve();

    // Rigid (unweighted) mesh `mid`: rest vertices [(-2,-1),(2,-1),(2,1),
    // (-2,1)]. The key's sparse `offset: 2, vertices: [3,-2,1,4]` writes
    // flat floats 2..6 of an 8-float (4-vertex) buffer, i.e. vertex 1's
    // delta (3,-2) and vertex 2's delta (1,4); vertices 0 and 3 get no
    // entry and stay at delta (0,0). Deform is applied before the single
    // bone (tip) transform, so the deformed local positions are:
    //   v0 = (-2,-1)+(0,0)  = (-2,-1)
    //   v1 = ( 2,-1)+(3,-2) = ( 5,-3)
    //   v2 = ( 2, 1)+(1, 4) = ( 3, 5)
    //   v3 = (-2, 1)+(0,0)  = (-2, 1)
    let mid_positions = mesh_positions(&frame, mid);
    let mid_expected_local = [
        Vec2::new(-2.0, -1.0),
        Vec2::new(5.0, -3.0),
        Vec2::new(3.0, 5.0),
        Vec2::new(-2.0, 1.0),
    ];
    for (index, (actual, local)) in mid_positions.iter().zip(mid_expected_local).enumerate() {
        assert_close(
            *actual,
            tip_world_transform_point(local),
            &format!("mid vertex {index}"),
        );
    }

    // Weighted mesh `wmid`. Its "vertices" wire data packs, per vertex, a
    // bone-count then (boneIndex, localX, localY, weight) tuples in bone
    // order [root, spin, tip, mirror, leaf] = [0..4]:
    //   vertex 0: 1x (tip,  -3, -2, 1.0)
    //   vertex 1: 1x (tip,   3, -2, 1.0)
    //   vertex 2: 2x (tip,   3,  2, 0.6), (spin, -8, 1, 0.4)
    //   vertex 3: 1x (tip,  -3,  2, 1.0)
    // giving 5 total bone contributions, so the deform buffer for this
    // attachment holds 2*5 = 10 floats (2 per contribution, in the same
    // order as the contributions above). The key's `vertices: [0.5, -0.25,
    // 1.5, 0.75]` (offset defaults to 0) therefore covers only the first
    // two contributions -- vertex 0's sole contribution gets delta
    // (0.5, -0.25) and vertex 1's sole contribution gets delta (1.5, 0.75)
    // -- leaving vertex 2's two contributions and vertex 3's contribution
    // at delta (0, 0). Deform is indexed per bone contribution and each
    // pair offsets that one contribution's own bone-local bind position
    // before it is transformed by its own bone and blended by weight (not
    // a single reconstructed per-vertex point in some shared space). This
    // fixture's own affected vertices (0 and 1) are single-influence, so
    // it cannot by itself distinguish that per-contribution indexing from
    // a per-vertex reading; see this file's module doc and
    // `spineboy_pro_hoverboard_deform_only_fits_the_per_influence_domain`
    // in `editor_4_3_23_contract.rs` for the evidence that does:
    //   v0 = tip_point(-3+0.5, -2+-0.25)               = tip_point(-2.5, -2.25)
    //   v1 = tip_point(3+1.5, -2+0.75)                 = tip_point(4.5, -1.25)
    //   v2 = 0.6 * tip_point(3, 2) + 0.4 * spin_point(-8, 1)   (no deform)
    //   v3 = tip_point(-3, 2)                          (no deform)
    let wmid_positions = mesh_positions(&frame, wmid);
    assert_close(
        wmid_positions[0],
        tip_world_transform_point(Vec2::new(-2.5, -2.25)),
        "wmid vertex 0",
    );
    assert_close(
        wmid_positions[1],
        tip_world_transform_point(Vec2::new(4.5, -1.25)),
        "wmid vertex 1",
    );
    let v2_tip = tip_world_transform_point(Vec2::new(3.0, 2.0));
    let v2_spin = spin_world_transform_point(Vec2::new(-8.0, 1.0));
    let v2_expected = v2_tip * 0.6 + v2_spin * 0.4;
    assert_close(wmid_positions[2], v2_expected, "wmid vertex 2");
    assert_close(
        wmid_positions[3],
        tip_world_transform_point(Vec2::new(-3.0, 2.0)),
        "wmid vertex 3",
    );
}

#[test]
fn deform_curve_interpolates_by_solving_x_for_the_sampled_time_fraction() {
    let (asset, mut skeleton) = curve_fixture();
    let deform = asset
        .animation_id("deform")
        .expect("deform animation exists");
    let mid = asset.slot_id("mid").expect("mid slot exists");

    // `mid`'s first deform key (time 0) now carries the oracle-returned
    // curve [0.125, 0.2, 0.375, 0.9], spanning to the next key at time 0.5.
    // Per the wire format proven above, x is absolute time within the key's
    // own span and y is a 0..1 progress fraction, so after Spinal's
    // load-time normalization by that span (0.5s) the stored control
    // points are:
    //   x1 = 0.125 / 0.5 = 0.25   y1 = 0.2  (unchanged, already a fraction)
    //   x2 = 0.375 / 0.5 = 0.75   y2 = 0.9  (unchanged, already a fraction)
    //
    // Sampling at time 0.25s gives linear = (0.25 - 0) / (0.5 - 0) = 0.5;
    // that 0.5 is a target *x* on the curve, not a Bezier parameter t. The
    // trap here (caught in an earlier draft of this suite) is plugging the
    // target fraction directly in as t and evaluating B_y(t) -- for a
    // Bezier, x(t) is generally *not* t, so that skips solving for the t
    // that actually reaches the target x. Spinal's evaluator instead walks
    // the documented 10-segment search over B_x(t) = (1-t)^3*0 +
    // 3(1-t)^2*t*x1 + 3(1-t)*t^2*x2 + t^3*1 to find the t that reaches
    // x = 0.5, then evaluates B_y at that same t:
    //   B_x(0.1) = 0.082   B_x(0.2) = 0.176   B_x(0.3) = 0.279
    //   B_x(0.4) = 0.388   B_x(0.5) = 0.500
    // None of segments 1-4 reach the target (all < 0.5); segment 5 hits it
    // exactly (0.5 <= 0.500, with zero remaining interpolation), because
    // these particular x control points are symmetric (x1 + x2 = 1) and
    // t = 0.5 happens to be its own fixed point -- not because the target
    // fraction can always be used as t. The solved parameter is therefore
    // t = 0.5, and the returned progress is B_y at that same t, using the
    // real y control points (0.2, 0.9), not (0, 1):
    //   B_y(0.5) = (1/8)*0 + (3/8)*0.2 + (3/8)*0.9 + (1/8)*1
    //            = 0.075 + 0.3375 + 0.125 = 0.5375
    //
    // That progress fraction (0.5375) then blends every deform component
    // between key 0 (all zero) and key 1 (offset 2, vertices [3,-2,1,4],
    // expanded to the zero-padded [0,0, 3,-2, 1,4, 0,0]):
    //   vertex 0: lerp(0,0, 0.5375)   = (0, 0)
    //   vertex 1: lerp((0,0),(3,-2), 0.5375) = (1.6125, -1.075)
    //   vertex 2: lerp((0,0),(1,4), 0.5375)  = (0.5375, 2.15)
    //   vertex 3: lerp(0,0, 0.5375)   = (0, 0)
    // added to `mid`'s rest vertices [(-2,-1),(2,-1),(2,1),(-2,1)] before
    // the single (tip) bone transform:
    //   v0 = (-2, -1)              v1 = (3.6125, -2.075)
    //   v2 = (2.5375, 3.15)        v3 = (-2, 1)
    skeleton
        .sample_animation(deform, Duration::from_millis(250), PlaybackMode::Once)
        .expect("deform animation is asset-local");
    let frame = skeleton.editable_pose().solve();
    let mid_positions = mesh_positions(&frame, mid);
    let mid_expected_local = [
        Vec2::new(-2.0, -1.0),
        Vec2::new(3.6125, -2.075),
        Vec2::new(2.5375, 3.15),
        Vec2::new(-2.0, 1.0),
    ];
    for (index, (actual, local)) in mid_positions.iter().zip(mid_expected_local).enumerate() {
        assert_close(
            *actual,
            tip_world_transform_point(local),
            &format!("curved mid vertex {index} at t=0.25s"),
        );
    }
}

/// A deform key targeting a non-mesh vertex attachment (here a bounding
/// box, which the wire format permits but Spinal does not evaluate deform
/// against) must degrade like any other unsupported timeline -- the whole
/// file must keep loading, not fail with a fatal schema error.
const BOUNDING_BOX_DEFORM_JSON: &[u8] = br#"{
  "skeleton": { "spine": "4.3.23" },
  "bones": [ { "name": "root" } ],
  "slots": [ { "name": "region", "bone": "root", "attachment": "region" } ],
  "skins": [
    {
      "name": "default",
      "attachments": {
        "region": {
          "region": { "type": "boundingbox", "vertexCount": 4, "vertices": [ 0, 0, 1, 0, 1, 1, 0, 1 ] }
        }
      }
    }
  ],
  "animations": {
    "shape": {
      "attachments": {
        "default": {
          "region": {
            "region": {
              "deform": [ { "vertices": [ 0.1, 0.2 ] }, { "time": 1 } ]
            }
          }
        }
      }
    }
  }
}"#;

#[test]
fn a_deform_key_targeting_a_non_mesh_attachment_degrades_the_load_continues() {
    let report = load_json(BOUNDING_BOX_DEFORM_JSON, b"page.png\n\tsize: 1, 1\n")
        .expect("a deform key on a bounding box must not fail the whole load");
    let diagnostics = report.diagnostics();
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code()
            == DiagnosticCode::UnsupportedTimelineType
            && diagnostic.severity() == DiagnosticSeverity::Degraded),
        "expected a degraded unsupported-timeline diagnostic for the bounding-box deform key, got {diagnostics:#?}"
    );
}

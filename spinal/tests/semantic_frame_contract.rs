//! Stable semantic evidence captured from solved Spine 4.3.23 frames.

use std::{sync::Arc, time::Duration};

use serde_json::Value;
use spinal::{
    PlaybackMode, SemanticBlendMode, SemanticDiagnosticCode, SemanticDiagnosticSeverity,
    SemanticDrawKind, SemanticFrame, SemanticIkTargetReach, Skeleton, load_json,
};

const ATLAS: &str = "\
fixture.png
\tsize: 64, 64
body
\tbounds: 0, 0, 8, 8
body-alt
\tbounds: 8, 0, 8, 8
mesh
\tbounds: 16, 0, 8, 8
";

const JSON: &str = r#"{
  "skeleton":{"spine":"4.3.23"},
  "bones":[
    {"name":"root","x":-0.0},
    {"name":"upper","parent":"root"},
    {"name":"lower","parent":"upper","x":10,"length":10},
    {"name":"target","parent":"root","x":10,"y":10},
    {"name":"source","parent":"root","rotation":25},
    {"name":"constrained","parent":"root","rotation":5}
  ],
  "slots":[
    {"name":"region-slot","bone":"lower","attachment":"body"},
    {"name":"mesh-slot","bone":"root","attachment":"mesh","blend":"additive"}
  ],
  "skins":[
    {
      "name":"default",
      "attachments":{
        "region-slot":{
          "body":{"path":"body","width":8,"height":8}
        },
        "mesh-slot":{
          "mesh":{
            "type":"mesh",
            "uvs":[0,0,1,0,0,1],
            "triangles":[0,1,2],
            "vertices":[0,0,8,0,0,8],
            "hull":3
          }
        }
      }
    },
    {
      "name":"outfit",
      "bones":["root"],
      "attachments":{
        "region-slot":{
          "body-alt":{
            "name":"outfit-body-alt",
            "path":"body-alt",
            "width":8,
            "height":8
          }
        }
      }
    }
  ],
  "constraints":[
    {
      "name":"paw",
      "type":"ik",
      "bones":["upper","lower"],
      "target":"target",
      "bendPositive":true
    },
    {
      "name":"copy",
      "type":"transform",
      "source":"source",
      "bones":["constrained"],
      "properties":{"rotate":{"to":{"rotate":{"max":100}}}},
      "mixRotate":1
    }
  ],
  "animations":{
    "review":{
      "bones":{
        "source":{
          "translate":[{"x":0,"y":0},{"time":1,"x":8,"y":0}]
        }
      },
      "slots":{
        "region-slot":{
          "attachment":[
            {"name":"body"},
            {"time":0.25,"name":"body-alt"}
          ],
          "rgba":[
            {"color":"00000000"},
            {"time":1,"color":"FFFFFFFF"}
          ]
        }
      },
      "drawOrder":[
        {"offsets":[{"slot":"region-slot","offset":1}]}
      ]
    }
  }
}"#;

fn animated_fixture() -> (Arc<spinal::SkeletonAsset>, Skeleton) {
    let asset = load_json(JSON.as_bytes(), ATLAS.as_bytes())
        .expect("the semantic fixture should load")
        .into_asset();
    let outfit = asset.skin_id("outfit").expect("outfit skin exists");
    let review = asset
        .animation_id("review")
        .expect("review animation exists");
    let mut skeleton = Skeleton::new(Arc::clone(&asset));
    skeleton
        .set_skin_layers(&[outfit])
        .expect("the skin belongs to this asset");
    skeleton
        .sample_animation(review, Duration::from_millis(500), PlaybackMode::Once)
        .expect("the animation belongs to this asset");

    (asset, skeleton)
}

fn capture_fixture() -> SemanticFrame {
    let (_asset, mut skeleton) = animated_fixture();
    let frame = skeleton.editable_pose().solve();
    SemanticFrame::capture(&frame).expect("the solved fixture is finite and supported")
}

#[test]
fn capture_owns_the_animated_midpoint_as_stable_renderer_neutral_evidence() {
    let captured = capture_fixture();

    assert_eq!(captured.default_skin(), Some("default"));
    assert_eq!(captured.skin_layers().collect::<Vec<_>>(), ["outfit"]);
    assert_eq!(
        captured
            .bones()
            .iter()
            .map(|bone| bone.name())
            .collect::<Vec<_>>(),
        ["root", "upper", "lower", "target", "source", "constrained"]
    );
    assert_eq!(
        captured
            .bones()
            .iter()
            .map(|bone| bone.ordinal())
            .collect::<Vec<_>>(),
        [0, 1, 2, 3, 4, 5]
    );
    assert_eq!(
        captured
            .slots()
            .iter()
            .map(|slot| slot.name())
            .collect::<Vec<_>>(),
        ["mesh-slot", "region-slot"]
    );
    assert_eq!(
        captured
            .slots()
            .iter()
            .map(|slot| slot.draw_order())
            .collect::<Vec<_>>(),
        [0, 1]
    );
    let source = captured
        .bones()
        .iter()
        .find(|bone| bone.name() == "source")
        .expect("source bone is captured");
    assert_f32s_near(source.local().translation(), [4.0, 0.0]);

    let [mesh_slot, region_slot] = captured.slots() else {
        panic!("the fixture should capture two evaluated slots");
    };
    assert_eq!(mesh_slot.name(), "mesh-slot");
    assert_eq!(region_slot.name(), "region-slot");
    let region_attachment = region_slot
        .attachment()
        .expect("the attachment timeline selects an attachment");
    assert_eq!(region_attachment.skin(), "outfit");
    assert_eq!(region_attachment.placeholder(), "body-alt");
    assert_eq!(region_attachment.name(), "outfit-body-alt");
    assert_f32s_near(region_slot.color_rgba(), [0.5; 4]);

    let [mesh, region] = captured.draw_items() else {
        panic!("the animated draw order should put the mesh before the region");
    };
    assert_eq!(region.kind(), SemanticDrawKind::Region);
    assert_eq!(region.slot(), "region-slot");
    assert_eq!(region.attachment().skin(), "outfit");
    assert_eq!(region.attachment().slot(), "region-slot");
    assert_eq!(region.attachment().placeholder(), "body-alt");
    assert_eq!(region.attachment().name(), "outfit-body-alt");
    assert_eq!(region.atlas_region().page(), "fixture.png");
    assert_eq!(region.atlas_region().region(), "body-alt");
    assert_eq!(region.blend_mode(), SemanticBlendMode::Normal);
    assert_eq!(region.positions().len(), 4);
    assert_eq!(region.uvs().expect("the atlas declares its size").len(), 4);
    assert_eq!(region.triangles(), [0, 1, 2, 0, 2, 3]);
    assert_f32s_near(region.color_rgba(), [0.5; 4]);

    assert_eq!(mesh.kind(), SemanticDrawKind::Mesh);
    assert_eq!(mesh.slot(), "mesh-slot");
    assert_eq!(mesh.attachment().skin(), "default");
    assert_eq!(mesh.attachment().slot(), "mesh-slot");
    assert_eq!(mesh.attachment().placeholder(), "mesh");
    assert_eq!(mesh.attachment().name(), "mesh");
    assert_eq!(mesh.atlas_region().page(), "fixture.png");
    assert_eq!(mesh.atlas_region().region(), "mesh");
    assert_eq!(mesh.blend_mode(), SemanticBlendMode::Additive);
    assert_eq!(mesh.positions().len(), 3);
    assert_eq!(mesh.uvs().expect("the atlas declares its size").len(), 3);
    assert_eq!(mesh.triangles(), [0, 1, 2]);

    let [ik] = captured.ik_constraints() else {
        panic!("the fixture should produce one IK status");
    };
    assert_eq!(ik.name(), "paw");
    assert!(ik.is_active());
    assert_eq!(ik.target_reach(), Some(SemanticIkTargetReach::Reachable));
    assert!(ik.issue().is_none());

    let [transform] = captured.transform_constraints() else {
        panic!("the fixture should produce one transform status");
    };
    assert_eq!(transform.name(), "copy");
    assert!(transform.is_active());
    assert!(transform.issue().is_none());

    assert!(captured.active_diagnostics().iter().any(|diagnostic| {
        diagnostic.severity() == SemanticDiagnosticSeverity::Degraded
            && diagnostic.code() == SemanticDiagnosticCode::UnsupportedBlendMode
    }));
    assert!(captured.active_diagnostics().iter().any(|diagnostic| {
        diagnostic.severity() == SemanticDiagnosticSeverity::Degraded
            && diagnostic.code() == SemanticDiagnosticCode::IgnoredSkinBones
    }));
}

#[test]
fn canonical_json_round_trips_and_is_equal_across_independent_asset_loads() {
    let first = capture_fixture();
    let second = capture_fixture();

    assert_eq!(first, second);
    let first_json = first
        .to_canonical_json()
        .expect("semantic evidence should serialize");
    let second_json = second
        .to_canonical_json()
        .expect("semantic evidence should serialize");
    assert_eq!(first_json, second_json);

    let decoded = SemanticFrame::from_json(&first_json).expect("valid evidence should parse");
    assert_eq!(decoded, first);
    assert_eq!(
        decoded
            .to_canonical_json()
            .expect("decoded evidence should serialize"),
        first_json
    );

    let value: Value = serde_json::from_slice(&first_json).expect("evidence is JSON");
    assert_has_no_runtime_id_keys(&value);
}

#[test]
fn capture_and_json_intake_canonicalize_every_signed_float_zero() {
    let (asset, mut skeleton) = animated_fixture();
    let root = asset.bone_id("root").expect("root bone exists");
    let frame = skeleton.editable_pose().solve();
    assert!(
        frame
            .bone(root)
            .expect("root belongs to the fixture")
            .local_transform()
            .translation()
            .x
            .is_sign_negative(),
        "the source fixture must exercise capture of negative zero"
    );
    let captured = SemanticFrame::capture(&frame).expect("the solved fixture is valid");
    let canonical = captured
        .to_canonical_json()
        .expect("semantic evidence should serialize");
    assert!(
        !String::from_utf8_lossy(&canonical).contains("-0.0"),
        "capture must normalize negative zero"
    );

    let mut value: Value = serde_json::from_slice(&canonical).expect("evidence is JSON");
    negate_json_float_zeroes(&mut value);
    let signed_zero_json = serde_json::to_vec(&value).expect("mutated evidence should serialize");
    assert!(
        String::from_utf8_lossy(&signed_zero_json).contains("-0.0"),
        "the mutation must exercise signed-zero intake"
    );
    let normalized = SemanticFrame::from_json(&signed_zero_json)
        .expect("negative zero remains finite and valid");
    assert_eq!(normalized, captured);
    assert_eq!(
        normalized
            .to_canonical_json()
            .expect("normalized evidence should serialize"),
        canonical
    );
}

#[test]
fn json_intake_rejects_wrong_versions_unknown_tokens_and_invalid_numeric_data() {
    let canonical = capture_fixture()
        .to_canonical_json()
        .expect("semantic evidence should serialize");
    let mut value: Value = serde_json::from_slice(&canonical).expect("evidence is JSON");

    value["format_version"] = Value::from(2);
    let wrong_version = serde_json::to_vec(&value).expect("mutated evidence should serialize");
    assert!(SemanticFrame::from_json(&wrong_version).is_err());

    let mut value: Value = serde_json::from_slice(&canonical).expect("evidence is JSON");
    value["draw_items"][0]["blend_mode"] = Value::from("invented");
    let unknown_token = serde_json::to_vec(&value).expect("mutated evidence should serialize");
    assert!(SemanticFrame::from_json(&unknown_token).is_err());

    let mut value: Value = serde_json::from_slice(&canonical).expect("evidence is JSON");
    value["draw_items"][1]["positions"]
        .as_array_mut()
        .expect("region positions are an array")
        .pop();
    let invalid_shape = serde_json::to_vec(&value).expect("mutated evidence should serialize");
    assert!(SemanticFrame::from_json(&invalid_shape).is_err());

    let mut value: Value = serde_json::from_slice(&canonical).expect("evidence is JSON");
    value["draw_items"][0]["positions"] = serde_json::json!([[0.0, 0.0], [1.0, 0.0]]);
    value["draw_items"][0]["uvs"] = serde_json::json!([[0.0, 0.0], [1.0, 0.0]]);
    value["draw_items"][0]["triangles"] = serde_json::json!([0, 1, 0]);
    let short_mesh = serde_json::to_vec(&value).expect("mutated evidence should serialize");
    assert!(SemanticFrame::from_json(&short_mesh).is_err());

    let mut value: Value = serde_json::from_slice(&canonical).expect("evidence is JSON");
    value["draw_items"][0]["triangles"] = Value::Array(Vec::new());
    let triangleless_mesh = serde_json::to_vec(&value).expect("mutated evidence should serialize");
    assert!(SemanticFrame::from_json(&triangleless_mesh).is_err());

    let mut value: Value = serde_json::from_slice(&canonical).expect("evidence is JSON");
    value["slots"][0]["draw_order"] = Value::from(9);
    let invalid_order = serde_json::to_vec(&value).expect("mutated evidence should serialize");
    assert!(SemanticFrame::from_json(&invalid_order).is_err());

    value["slots"][0]["draw_order"] = Value::from(0);
    value["slots"][0]["color_rgba"][0] = Value::from(2.0);
    let invalid_color = serde_json::to_vec(&value).expect("mutated evidence should serialize");
    assert!(SemanticFrame::from_json(&invalid_color).is_err());

    let canonical = String::from_utf8(canonical).expect("canonical JSON is UTF-8");
    let nonfinite = canonical.replacen("\"rotation_radians\":0.0", "\"rotation_radians\":1e999", 1);
    assert_ne!(
        nonfinite, canonical,
        "the fixture should contain a zero rotation"
    );
    assert!(SemanticFrame::from_json(nonfinite.as_bytes()).is_err());
}

fn assert_has_no_runtime_id_keys(value: &Value) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                assert_ne!(
                    key, "id",
                    "semantic evidence must not serialize runtime IDs"
                );
                assert!(
                    !key.ends_with("_id"),
                    "semantic evidence must not serialize runtime ID key {key:?}"
                );
                assert_has_no_runtime_id_keys(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                assert_has_no_runtime_id_keys(value);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn negate_json_float_zeroes(value: &mut Value) {
    match value {
        Value::Number(number) if number.is_f64() && number.as_f64() == Some(0.0) => {
            *number = serde_json::Number::from_f64(-0.0).expect("negative zero is a JSON number");
        }
        Value::Array(values) => {
            for value in values {
                negate_json_float_zeroes(value);
            }
        }
        Value::Object(object) => {
            for value in object.values_mut() {
                negate_json_float_zeroes(value);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn assert_f32s_near<const N: usize>(actual: [f32; N], expected: [f32; N]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 1.0e-5, "{actual} != {expected}");
    }
}

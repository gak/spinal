//! Public contract tests for deterministic Stage 3 pose sampling and skins.

use std::{sync::Arc, time::Duration};

use spinal::{BendDirection, PlaybackMode, Rgba, Skeleton, load_json};

const ATLAS: &str = "\
cat.png
	size: 64, 16
body
	bounds: 0, 0, 8, 8
orange-body
	bounds: 8, 0, 8, 8
red-hat
	bounds: 16, 0, 8, 8
blue-hat
	bounds: 24, 0, 8, 8
round-glasses
	bounds: 32, 0, 8, 8
";

const JSON: &str = r#"{
  "skeleton":{"spine":"4.3.23"},
  "bones":[
    {"name":"root"},
    {
      "name":"cat",
      "parent":"root",
      "x":5,
      "y":7,
      "rotation":10,
      "scaleX":2,
      "scaleY":3,
      "shearX":1,
      "shearY":2
    },
    {"name":"target","parent":"root","x":20}
  ],
  "slots":[
    {
      "name":"body-slot",
      "bone":"cat",
      "attachment":"body",
      "color":"20406080"
    },
    {"name":"hat-slot","bone":"cat","attachment":"hat"},
    {"name":"glasses-slot","bone":"cat","attachment":"glasses"}
  ],
  "skins":[
    {
      "name":"default",
      "attachments":{
        "body-slot":{"body":{"path":"body","width":8,"height":8}}
      }
    },
    {
      "name":"breed/orange",
      "attachments":{
        "body-slot":{"body":{"path":"orange-body","width":8,"height":8}}
      }
    },
    {
      "name":"hat/red",
      "attachments":{
        "hat-slot":{"hat":{"path":"red-hat","width":8,"height":8}}
      }
    },
    {
      "name":"hat/blue",
      "attachments":{
        "hat-slot":{"hat":{"path":"blue-hat","width":8,"height":8}}
      }
    },
    {
      "name":"glasses/round",
      "attachments":{
        "glasses-slot":{"glasses":{"path":"round-glasses","width":8,"height":8}}
      }
    }
  ],
  "constraints":[{
    "name":"aim",
    "type":"ik",
    "bones":["cat"],
    "target":"target",
    "mix":0.25
  }],
  "events":{"step":{"int":7,"string":"soft"}},
  "animations":{
    "action":{
      "bones":{
        "cat":{
          "rotate":[{"value":0},{"time":1,"value":90}],
          "translate":[{"x":0,"y":0},{"time":1,"x":10,"y":-4}],
          "scale":[
            {"x":1,"y":1,"curve":"stepped"},
            {"time":1,"x":2,"y":0.5}
          ],
          "shear":[{"x":0,"y":0},{"time":1,"x":10,"y":20}]
        }
      },
      "slots":{
        "body-slot":{
          "rgba":[
            {
              "color":"00000000",
              "curve":[
                0,0,1,1,
                0,0,1,1,
                0,0,1,1,
                0,0,1,1
              ]
            },
            {"time":1,"color":"FFFFFFFF"}
          ]
        },
        "hat-slot":{
          "attachment":[{"name":"hat"},{"time":0.75,"name":null}]
        }
      },
      "ik":{
        "aim":[
          {"mix":0.2,"bendPositive":true},
          {"time":1,"mix":0.8,"bendPositive":false}
        ]
      },
      "drawOrder":[
        {"offsets":[{"slot":"body-slot","offset":2}]},
        {"time":0.75}
      ],
      "events":[
        {"name":"step"},
        {"time":0.5,"name":"step","int":9},
        {"time":1,"name":"step","string":"hard"}
      ]
    },
    "late":{
      "bones":{
        "cat":{
          "rotate":[{"time":0.5,"value":20},{"time":1,"value":40}]
        }
      }
    },
    "tenth":{
      "bones":{
        "cat":{
          "rotate":[{"time":0.1,"value":30}]
        }
      }
    }
  }
}"#;

fn fixture() -> (Arc<spinal::SkeletonAsset>, Skeleton) {
    let asset = load_json(JSON.as_bytes(), ATLAS.as_bytes())
        .expect("the Stage 3 fixture should load")
        .into_asset();
    let skeleton = Skeleton::new(Arc::clone(&asset));
    (asset, skeleton)
}

#[test]
fn sampling_is_absolute_relative_to_setup_and_curve_aware() {
    let (asset, mut skeleton) = fixture();
    let action = asset.animation_id("action").expect("animation exists");
    let cat = asset.bone_id("cat").expect("bone exists");

    skeleton
        .sample_animation(action, Duration::from_millis(500), PlaybackMode::Once)
        .expect("animation ID belongs to this asset");

    let transform = skeleton
        .bone_pose(cat)
        .expect("bone ID belongs to this asset")
        .local_transform();
    assert_eq!(transform.translation().to_array(), [10.0, 5.0]);
    assert!((transform.rotation().as_degrees() - 55.0).abs() < 1.0e-4);
    assert_eq!(transform.scale().to_array(), [2.0, 3.0]);
    assert!((transform.shear().x().as_degrees() - 6.0).abs() < 1.0e-4);
    assert!((transform.shear().y().as_degrees() - 12.0).abs() < 1.0e-4);

    let body_slot = asset.slot_id("body-slot").expect("slot exists");
    let color = skeleton
        .slot_pose(body_slot)
        .expect("slot ID belongs to this asset")
        .color();
    assert_rgba_near(color, Rgba::new(0.5, 0.5, 0.5, 0.5).expect("valid color"));

    let aim = asset.ik_constraint_id("aim").expect("constraint exists");
    let pose = skeleton
        .ik_constraint_pose(aim)
        .expect("constraint ID belongs to this asset");
    assert!((pose.mix().get() - 0.5).abs() < 1.0e-5);
    assert_eq!(pose.bend_direction(), BendDirection::Positive);
}

#[test]
fn once_clamps_loop_wraps_and_before_the_first_key_uses_setup() {
    let (asset, mut skeleton) = fixture();
    let action = asset.animation_id("action").expect("animation exists");
    let late = asset.animation_id("late").expect("animation exists");
    let cat = asset.bone_id("cat").expect("bone exists");

    skeleton
        .sample_animation(action, Duration::from_millis(1250), PlaybackMode::Once)
        .expect("asset-local animation");
    assert!(
        (skeleton
            .bone_pose(cat)
            .expect("asset-local bone")
            .local_transform()
            .rotation()
            .as_degrees()
            - 100.0)
            .abs()
            < 1.0e-4
    );

    skeleton
        .sample_animation(action, Duration::from_millis(1250), PlaybackMode::Loop)
        .expect("asset-local animation");
    assert!(
        (skeleton
            .bone_pose(cat)
            .expect("asset-local bone")
            .local_transform()
            .rotation()
            .as_degrees()
            - 32.5)
            .abs()
            < 1.0e-4
    );

    skeleton
        .sample_animation(late, Duration::from_millis(250), PlaybackMode::Once)
        .expect("asset-local animation");
    assert!(
        (skeleton
            .bone_pose(cat)
            .expect("asset-local bone")
            .local_transform()
            .rotation()
            .as_degrees()
            - 10.0)
            .abs()
            < 1.0e-4
    );
}

#[test]
fn exact_decimal_key_and_loop_boundaries_use_integer_ticks() {
    let (asset, mut skeleton) = fixture();
    let animation = asset.animation_id("tenth").expect("animation exists");
    let cat = asset.bone_id("cat").expect("bone exists");

    skeleton
        .sample_animation(animation, Duration::from_millis(100), PlaybackMode::Once)
        .expect("asset-local animation");
    assert!(
        (skeleton
            .bone_pose(cat)
            .expect("asset-local bone")
            .local_transform()
            .rotation()
            .as_degrees()
            - 40.0)
            .abs()
            < 1.0e-4
    );

    skeleton
        .sample_animation(animation, Duration::from_millis(100), PlaybackMode::Loop)
        .expect("asset-local animation");
    assert!(
        (skeleton
            .bone_pose(cat)
            .expect("asset-local bone")
            .local_transform()
            .rotation()
            .as_degrees()
            - 10.0)
            .abs()
            < 1.0e-4
    );
}

#[test]
fn loop_positions_beyond_u64_nanoseconds_keep_their_true_phase() {
    let (asset, mut skeleton) = fixture();
    let animation = asset.animation_id("action").expect("animation exists");
    let cat = asset.bone_id("cat").expect("bone exists");

    skeleton
        .sample_animation(
            animation,
            Duration::new(u64::MAX, 250_000_000),
            PlaybackMode::Loop,
        )
        .expect("asset-local animation");

    assert!(
        (skeleton
            .bone_pose(cat)
            .expect("asset-local bone")
            .local_transform()
            .rotation()
            .as_degrees()
            - 32.5)
            .abs()
            < 1.0e-4
    );
}

#[test]
fn composite_skins_use_later_wins_precedence_then_default_fallback() {
    let (asset, mut skeleton) = fixture();
    let body_slot = asset.slot_id("body-slot").expect("slot exists");
    let hat_slot = asset.slot_id("hat-slot").expect("slot exists");
    let glasses_slot = asset.slot_id("glasses-slot").expect("slot exists");

    assert_eq!(
        asset
            .slot(hat_slot)
            .expect("asset-local slot")
            .setup_attachment_name(),
        Some("hat")
    );
    assert!(
        skeleton
            .slot_pose(hat_slot)
            .expect("slot pose")
            .attachment()
            .is_none()
    );

    let orange = asset.skin_id("breed/orange").expect("skin exists");
    let red = asset.skin_id("hat/red").expect("skin exists");
    let blue = asset.skin_id("hat/blue").expect("skin exists");
    let round = asset.skin_id("glasses/round").expect("skin exists");
    skeleton
        .set_skin_layers(&[orange, red, round])
        .expect("all skins belong to the asset");

    assert_eq!(
        attachment_path(&asset, &skeleton, body_slot),
        Some("orange-body")
    );
    assert_eq!(
        attachment_path(&asset, &skeleton, hat_slot),
        Some("red-hat")
    );
    assert_eq!(
        attachment_path(&asset, &skeleton, glasses_slot),
        Some("round-glasses")
    );

    skeleton
        .set_skin_layers(&[red, blue])
        .expect("all skins belong to the asset");
    assert_eq!(attachment_path(&asset, &skeleton, body_slot), Some("body"));
    assert_eq!(
        attachment_path(&asset, &skeleton, hat_slot),
        Some("blue-hat")
    );
    assert_eq!(skeleton.skin_layers().collect::<Vec<_>>(), [red, blue]);

    skeleton
        .set_skin_layers(&[red, blue, red])
        .expect("all skins belong to the asset");
    assert_eq!(skeleton.skin_layers().collect::<Vec<_>>(), [blue, red]);
    assert_eq!(
        attachment_path(&asset, &skeleton, hat_slot),
        Some("red-hat")
    );
}

#[test]
fn incrementally_equipping_and_removing_a_cosmetic_refreshes_setup_slots() {
    let (asset, mut skeleton) = fixture();
    let orange = asset.skin_id("breed/orange").expect("skin exists");
    let round = asset.skin_id("glasses/round").expect("skin exists");
    let glasses_slot = asset.slot_id("glasses-slot").expect("slot exists");

    skeleton
        .set_skin_layers(&[orange])
        .expect("skin belongs to the asset");
    assert_eq!(attachment_path(&asset, &skeleton, glasses_slot), None);

    skeleton
        .set_skin_layers(&[orange, round])
        .expect("skins belong to the asset");
    assert_eq!(
        attachment_path(&asset, &skeleton, glasses_slot),
        Some("round-glasses")
    );

    skeleton
        .set_skin_layers(&[orange])
        .expect("skin belongs to the asset");
    assert_eq!(attachment_path(&asset, &skeleton, glasses_slot), None);
}

#[test]
fn foreign_skin_layer_changes_fail_without_mutating_the_outfit() {
    let (asset, mut skeleton) = fixture();
    let red = asset.skin_id("hat/red").expect("skin exists");
    let hat_slot = asset.slot_id("hat-slot").expect("slot exists");
    skeleton.set_skin_layers(&[red]).expect("asset-local skin");
    let before = attachment_path(&asset, &skeleton, hat_slot);

    let (foreign_asset, _foreign_skeleton) = fixture();
    let foreign = foreign_asset
        .skin_id("glasses/round")
        .expect("foreign skin exists");
    assert!(skeleton.set_skin_layers(&[red, foreign]).is_err());

    assert_eq!(skeleton.skin_layers().collect::<Vec<_>>(), [red]);
    assert_eq!(attachment_path(&asset, &skeleton, hat_slot), before);
}

#[test]
fn attachment_and_draw_order_keys_are_sampled() {
    let (asset, mut skeleton) = fixture();
    let action = asset.animation_id("action").expect("animation exists");
    let red = asset.skin_id("hat/red").expect("skin exists");
    let hat_slot = asset.slot_id("hat-slot").expect("slot exists");
    skeleton.set_skin_layers(&[red]).expect("asset-local skin");

    skeleton
        .sample_animation(action, Duration::from_millis(500), PlaybackMode::Once)
        .expect("asset-local animation");
    assert_eq!(
        attachment_path(&asset, &skeleton, hat_slot),
        Some("red-hat")
    );
    assert_eq!(
        skeleton
            .draw_order()
            .map(|slot| { asset.slot(slot.id()).expect("asset-local draw slot").name() })
            .collect::<Vec<_>>(),
        ["hat-slot", "glasses-slot", "body-slot"]
    );

    skeleton
        .sample_animation(action, Duration::from_millis(800), PlaybackMode::Once)
        .expect("asset-local animation");
    assert!(
        skeleton
            .slot_pose(hat_slot)
            .expect("slot pose")
            .attachment()
            .is_none()
    );
    assert_eq!(
        skeleton
            .draw_order()
            .map(|slot| { asset.slot(slot.id()).expect("asset-local draw slot").name() })
            .collect::<Vec<_>>(),
        ["body-slot", "hat-slot", "glasses-slot"]
    );
}

#[test]
fn steady_state_sampling_and_skin_composition_allocate_nothing() {
    let (asset, mut skeleton) = fixture();
    let action = asset.animation_id("action").expect("animation exists");
    let red = asset.skin_id("hat/red").expect("skin exists");
    let round = asset.skin_id("glasses/round").expect("skin exists");
    let red_only = [red];
    let outfit = [red, round];

    let allocations = allocation_counter::measure(|| {
        for step in 0..128 {
            skeleton
                .sample_animation(action, Duration::from_millis(step * 13), PlaybackMode::Loop)
                .expect("asset-local animation");
            skeleton.reset_to_setup_pose();
            skeleton
                .set_skin_layers(if step % 2 == 0 {
                    &outfit[..]
                } else {
                    &red_only[..]
                })
                .expect("asset-local skins");
            skeleton.reset_slot_attachments_to_setup_pose();
        }
    });

    assert_eq!(allocations.count_total, 0);
    assert_eq!(allocations.bytes_total, 0);
}

fn attachment_path<'a>(
    asset: &'a spinal::SkeletonAsset,
    skeleton: &Skeleton,
    slot: spinal::SlotId,
) -> Option<&'a str> {
    let attachment = skeleton.slot_pose(slot).ok()?.attachment()?;
    asset
        .attachment(attachment)
        .ok()?
        .as_region()?
        .attachment()
        .atlas_path()
}

fn assert_rgba_near(actual: Rgba, expected: Rgba) {
    for (actual, expected) in actual.to_array().into_iter().zip(expected.to_array()) {
        assert!((actual - expected).abs() < 1.0e-4, "{actual} != {expected}");
    }
}

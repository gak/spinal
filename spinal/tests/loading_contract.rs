//! Public contract tests for the documentation-derived Stage 2 loaders.

use std::{sync::Arc, time::Duration};

use spinal::{
    AlphaEncoding, AttachmentKind, BendDirection, DiagnosticCode, DiagnosticScope,
    DiagnosticSeverity, LoadDocument, LoadErrorKind, PixelRect, PixelSize, Rgba8, Skeleton,
    SlotBlendMode, TextureFilter, TransformMix, load_json,
};

const MINIMAL_ATLAS: &str = "\
cat.png
	size: 128, 64
	filter: Linear, Linear
	repeat: none
	pma: false
cat/body
	bounds: 4, 8, 64, 32
";

const MINIMAL_JSON: &str = r#"{
  "skeleton": { "spine": "4.3.23" },
  "bones": [
    { "name": "root" },
    {
      "name": "body",
      "parent": "root",
      "length": 12,
      "x": 3,
      "y": 4,
      "rotation": 30,
      "scaleX": 2,
      "scaleY": 0.5,
      "shearX": 5,
      "shearY": -7
    }
  ],
  "slots": [
    {
      "name": "body-slot",
      "bone": "body",
      "attachment": "body",
      "color": "10203040"
    }
  ],
  "skins": [
    {
      "name": "default",
      "attachments": {
        "body-slot": {
          "body": {
            "path": "cat/body",
            "x": 1,
            "y": 2,
            "rotation": 15,
            "width": 64,
            "height": 32,
            "color": "FFEEDDCC"
          }
        }
      }
    }
  ],
  "animations": {
    "idle": {
      "bones": {
        "body": {
          "rotate": [
            { "value": 0 },
            { "time": 0.75, "value": 10 }
          ]
        }
      }
    }
  }
}"#;

#[test]
fn exact_target_loads_into_a_linked_renderer_independent_asset() {
    let report = load_json(MINIMAL_JSON.as_bytes(), MINIMAL_ATLAS.as_bytes())
        .expect("the closed target subset should load");
    assert_eq!(report.asset().spine_version(), "4.3.23");
    assert!(report.diagnostics().is_empty());
    assert!(!report.has_degradations());

    let asset = report.asset();
    assert_eq!(
        asset.bones().map(|bone| bone.name()).collect::<Vec<_>>(),
        ["root", "body"]
    );
    let root = asset.bone_id("root").expect("root exists");
    let body = asset
        .bone(asset.bone_id("body").expect("body exists"))
        .expect("ID belongs to the asset");
    assert_eq!(body.parent(), Some(root));
    assert_eq!(body.length(), 12.0);
    assert_eq!(body.setup_transform().translation().to_array(), [3.0, 4.0]);
    assert!((body.setup_transform().rotation().as_degrees() - 30.0).abs() < 1.0e-5);
    assert_eq!(body.setup_transform().scale().to_array(), [2.0, 0.5]);
    assert!((body.setup_transform().shear().x().as_degrees() - 5.0).abs() < 1.0e-5);
    assert!((body.setup_transform().shear().y().as_degrees() + 7.0).abs() < 1.0e-5);

    let slot = asset.slots().next().expect("one slot");
    assert_eq!(slot.bone(), body.id());
    assert_eq!(slot.color(), Rgba8::new(0x10, 0x20, 0x30, 0x40));

    let setup_name = slot
        .setup_attachment_name()
        .expect("slot has a setup attachment placeholder");
    let attachment_id = asset
        .default_skin()
        .expect("fixture has a default skin")
        .attachment(slot.id(), setup_name)
        .expect("slot belongs to this asset")
        .expect("default skin supplies the setup attachment");
    let attachment = asset
        .attachment(attachment_id)
        .expect("ID belongs to the asset");
    assert_eq!(attachment.name(), "body");
    assert_eq!(attachment.atlas_path(), Some("cat/body"));
    assert_eq!(attachment.kind(), AttachmentKind::Region);
    assert_eq!(attachment.slot(), slot.id());
    let rigid = attachment.as_region().expect("rigid region attachment");
    assert_eq!(rigid.color(), Rgba8::new(0xFF, 0xEE, 0xDD, 0xCC));
    assert_eq!(rigid.size(), PixelSize::new(64, 32));

    let region = asset
        .atlas_region(rigid.atlas_region())
        .expect("ID belongs to the asset");
    assert_eq!(region.name(), "cat/body");
    assert_eq!(region.bounds(), PixelRect::new(4, 8, 64, 32));
    let page = asset
        .atlas_page(region.page())
        .expect("ID belongs to the asset");
    assert_eq!(page.name(), "cat.png");
    assert_eq!(page.size(), PixelSize::new(128, 64));
    assert_eq!(page.min_filter(), TextureFilter::Linear);
    assert_eq!(page.mag_filter(), TextureFilter::Linear);
    assert_eq!(page.alpha_encoding(), AlphaEncoding::Straight);

    let idle = asset
        .animation(asset.animation_id("idle").expect("idle exists"))
        .expect("ID belongs to the asset");
    assert_eq!(idle.duration(), Duration::from_millis(750));

    let shared = report.into_asset();
    let instance = Skeleton::new(Arc::clone(&shared));
    assert!(Arc::ptr_eq(instance.asset_handle(), &shared));
}

#[test]
fn attachment_placeholder_actual_name_and_region_view_are_distinct() {
    let json = br#"{
      "skeleton":{"spine":"4.3.23"},
      "bones":[{"name":"root"}],
      "slots":[{"name":"head-slot","bone":"root","attachment":"head"}],
      "skins":[{
        "name":"default",
        "attachments":{
          "head-slot":{
            "head":{"name":"blue/head","width":16,"height":8}
          }
        }
      }]
    }"#;
    let report = load_json(
        json,
        b"cat.png\n\tsize:16,8\nblue/head\n\tbounds:0,0,16,8\n",
    )
    .expect("actual attachment name should resolve the atlas region");
    let asset = report.asset();
    let slot = asset.slots().next().expect("one slot");
    let attachment = asset
        .attachment(
            asset
                .default_skin()
                .expect("fixture has a default skin")
                .attachment(
                    slot.id(),
                    slot.setup_attachment_name()
                        .expect("slot has a setup attachment placeholder"),
                )
                .expect("slot belongs to this asset")
                .expect("default skin supplies the setup attachment"),
        )
        .expect("asset-local attachment ID");

    assert_eq!(attachment.placeholder_name(), "head");
    assert_eq!(attachment.name(), "blue/head");
    assert_eq!(attachment.atlas_path(), None);
    let region = attachment.as_region().expect("rigid region view");
    assert_eq!(region.attachment().id(), attachment.id());
    assert_eq!(region.size(), PixelSize::new(16, 8));
    assert_eq!(region.color(), Rgba8::WHITE);
    assert_eq!(
        asset
            .atlas_region(region.atlas_region())
            .expect("linked atlas region")
            .name(),
        "blue/head"
    );

    let skin = asset.skins().next().expect("default skin");
    assert_eq!(
        skin.attachment(slot.id(), "head")
            .expect("asset-local slot")
            .expect("placeholder lookup"),
        attachment.id()
    );
}

#[test]
fn setup_placeholders_may_be_supplied_only_by_an_optional_skin() {
    let json = br#"{
      "skeleton":{"spine":"4.3.23"},
      "bones":[{"name":"root"}],
      "slots":[{"name":"hat-slot","bone":"root","attachment":"hat"}],
      "skins":[{
        "name":"hat/red",
        "attachments":{
          "hat-slot":{"hat":{"path":"red-hat","width":8,"height":8}}
        }
      }]
    }"#;
    let report = load_json(json, b"cat.png\n\tsize:8,8\nred-hat\n\tbounds:0,0,8,8\n")
        .expect("a non-default skin may supply a setup placeholder");
    let slot = report.asset().slots().next().expect("one slot");
    assert_eq!(slot.setup_attachment_name(), Some("hat"));
    assert!(report.asset().default_skin().is_none());

    let missing = br#"{
      "skeleton":{"spine":"4.3.23"},
      "bones":[{"name":"root"}],
      "slots":[{"name":"hat-slot","bone":"root","attachment":"hat"}]
    }"#;
    let error = load_json(missing, b"cat.png\n")
        .expect_err("an unknown setup placeholder remains a fatal reference error");
    assert_eq!(error.kind(), LoadErrorKind::UnresolvedReference);
    assert_eq!(error.location().path(), Some("/slots/0/attachment"));
}

#[test]
fn atlas_pages_regions_defaults_and_duplicate_names_preserve_source_order() {
    let json = r#"{
      "skeleton": { "spine": "4.3.23" },
      "bones": [{ "name": "root" }]
    }"#;
    let atlas = "\u{feff}\r\n\
first.png\r\n\
\tsize: 16, 8\r\n\
spark\r\n\
\tindex: 0\r\n\
\tbounds: 1, 2, 3, 4\r\n\
\r\n\
empty.png\r\n\
\r\n\
third.png\r\n\
spark\r\n\
\tindex: 1\r\n";

    let report = load_json(json.as_bytes(), atlas.as_bytes()).expect("valid multi-page atlas");
    let asset = report.asset();

    assert_eq!(
        asset
            .atlas_pages()
            .map(|page| page.name())
            .collect::<Vec<_>>(),
        ["first.png", "empty.png", "third.png"]
    );
    let regions = asset.atlas_regions().collect::<Vec<_>>();
    assert_eq!(regions.len(), 2);
    assert_eq!(regions[0].name(), "spark");
    assert_eq!(regions[0].index(), Some(0));
    assert_eq!(regions[1].name(), "spark");
    assert_eq!(regions[1].index(), Some(1));
    assert_eq!(regions[1].bounds(), PixelRect::new(0, 0, 0, 0));
    assert_eq!(regions[1].trim().original_size(), PixelSize::new(0, 0));
    assert_eq!(
        asset
            .atlas_regions_named("spark")
            .map(|region| region.index())
            .collect::<Vec<_>>(),
        [Some(0), Some(1)]
    );
}

#[test]
fn one_and_two_bone_ik_are_linked_in_evaluation_order() {
    let json = r#"{
      "skeleton": { "spine": "4.3.23" },
      "bones": [
        { "name": "root" },
        { "name": "upper", "parent": "root" },
        { "name": "lower", "parent": "upper" },
        { "name": "target", "parent": "root" }
      ],
      "constraints": [
        {
          "name": "aim",
          "type": "ik",
          "order": 2,
          "target": "target",
          "bones": ["upper"],
          "mix": 0.25
        },
        {
          "name": "paw",
          "type": "ik",
          "order": 5,
          "target": "target",
          "bones": ["upper", "lower"],
          "bendPositive": false
        }
      ]
    }"#;

    let report = load_json(json.as_bytes(), b"page.png\n").expect("supported IK should load");
    let asset = report.asset();
    let constraints = asset.ik_constraints().collect::<Vec<_>>();
    assert_eq!(constraints.len(), 2);
    assert_eq!(constraints[0].name(), "aim");
    assert_eq!(constraints[0].order(), 2);
    assert_eq!(constraints[0].mix().get(), 0.25);
    assert_eq!(constraints[0].bend_direction(), BendDirection::Positive);
    assert_eq!(
        constraints[0]
            .bones()
            .map(|id| asset.bone(id).expect("linked ID").name())
            .collect::<Vec<_>>(),
        ["upper"]
    );
    assert_eq!(
        asset
            .bone(constraints[0].target())
            .expect("linked target")
            .name(),
        "target"
    );
    assert_eq!(constraints[1].bend_direction(), BendDirection::Negative);
    assert_eq!(constraints[1].bones().count(), 2);
}

#[test]
fn unified_constraints_without_orders_follow_source_order() {
    let json = br#"{
      "skeleton":{"spine":"4.3.23"},
      "bones":[
        {"name":"root"},
        {"name":"upper","parent":"root"},
        {"name":"lower","parent":"upper"},
        {"name":"target","parent":"root"}
      ],
      "constraints":[
        {
          "name":"aim",
          "type":"ik",
          "bones":["upper"],
          "target":"target"
        },
        {
          "name":"paw",
          "type":"ik",
          "bones":["upper","lower"],
          "target":"target"
        }
      ]
    }"#;

    let report = load_json(json, b"page.png\n").expect("supported IK should load");
    let asset = report.asset();
    let constraints = asset.constraints().collect::<Vec<_>>();
    assert_eq!(constraints[0].name(), "aim");
    assert_eq!(constraints[0].order(), 0);
    assert_eq!(constraints[1].name(), "paw");
    assert_eq!(constraints[1].order(), 1);

    let ik = asset.ik_constraints().collect::<Vec<_>>();
    assert_eq!(ik[0].name(), "aim");
    assert_eq!(ik[0].order(), 0);
    assert_eq!(ik[1].name(), "paw");
    assert_eq!(ik[1].order(), 1);
}

#[test]
fn separate_constraint_arrays_are_retained_and_bridge_to_typed_ik() {
    let json = br#"{
      "skeleton":{"spine":"4.3.23"},
      "bones":[
        {"name":"root"},
        {"name":"upper","parent":"root"},
        {"name":"target","parent":"root"}
      ],
      "ik":[{
        "name":"aim",
        "order":1,
        "bones":["upper"],
        "target":"target"
      }],
      "transform":[{
        "name":"follow",
        "bones":["upper"],
        "target":"target",
        "mixX":0,
        "mixScaleX":0,
        "mixShearY":0
      }]
    }"#;
    let report =
        load_json(json, b"page.png\n").expect("supported transform constraint records are linked");
    let asset = report.asset();
    let constraints = asset.constraints().collect::<Vec<_>>();
    assert_eq!(
        constraints
            .iter()
            .map(|constraint| constraint.name())
            .collect::<Vec<_>>(),
        ["aim", "follow"]
    );
    assert_eq!(constraints[0].source_type(), "ik");
    assert_eq!(constraints[1].source_type(), "transform");
    assert_eq!(constraints[0].order(), 1);
    assert_eq!(constraints[1].order(), 0);

    let ik = constraints[0].as_ik().expect("typed IK bridge");
    assert_eq!(ik.name(), "aim");
    assert_eq!(ik.constraint().id(), constraints[0].id());
    assert!(constraints[1].as_ik().is_none());
    let transform = constraints[1]
        .as_transform()
        .expect("typed transform bridge");
    assert_eq!(transform.name(), "follow");
    assert_eq!(transform.constraint().id(), constraints[1].id());
    assert_eq!(transform.source(), asset.bone_id("target").unwrap());
    assert_eq!(
        transform.bones().collect::<Vec<_>>(),
        [asset.bone_id("upper").unwrap()]
    );
    assert!(transform.copies_rotation());
    assert_eq!(transform.rotation_offset().as_degrees(), 0.0);
    assert_eq!(transform.setup_pose().mix_rotate(), TransformMix::ONE);
    assert!(report.diagnostics().is_empty());
}

#[test]
fn spine_4_3_rotation_transform_constraints_and_timelines_are_typed() {
    let json = br#"{
      "skeleton":{"spine":"4.3.23"},
      "bones":[
        {"name":"root"},
        {"name":"source","parent":"root","rotation":20},
        {"name":"torso","parent":"root","rotation":100}
      ],
      "constraints":[{
        "type":"transform",
        "name":"aim-torso-transform",
        "source":"source",
        "bones":["torso"],
        "rotation":69.5,
        "properties":{"rotate":{"to":{"rotate":{"max":100}}}},
        "mixRotate":0
      }],
      "animations":{
        "aim":{
          "transform":{
            "aim-torso-transform":[
              {"mixRotate":0.25},
              {"time":1,"mixRotate":0.75}
            ]
          }
        }
      }
    }"#;

    let report =
        load_json(json, b"page.png\n").expect("Spine 4.3 rotation constraints should load");
    assert!(report.diagnostics().is_empty());
    let asset = report.asset();
    let id = asset
        .transform_constraint_id("aim-torso-transform")
        .expect("typed name lookup");
    let constraint = asset
        .transform_constraint(id)
        .expect("ID belongs to this asset");
    assert_eq!(constraint.order(), 0);
    assert_eq!(constraint.source(), asset.bone_id("source").unwrap());
    assert_eq!(
        constraint.bones().collect::<Vec<_>>(),
        [asset.bone_id("torso").unwrap()]
    );
    assert!((constraint.rotation_offset().as_degrees() - 69.5).abs() < 1.0e-5);
    assert_eq!(constraint.setup_pose().mix_rotate(), TransformMix::ZERO);

    let mut skeleton = Skeleton::new(Arc::clone(asset));
    let aim = asset.animation_id("aim").expect("aim animation exists");
    skeleton
        .sample_animation(aim, Duration::from_millis(500), spinal::PlaybackMode::Once)
        .expect("animation belongs to the asset");
    let pose = skeleton
        .transform_constraint_pose(id)
        .expect("constraint belongs to the asset");
    assert!((pose.mix_rotate().get() - 0.5).abs() < 1.0e-5);
}

#[test]
fn unsupported_profile_features_have_individual_bounded_loader_tripwires() {
    let json = br#"{
      "skeleton":{"spine":"4.3.23"},
      "bones":[{"name":"root"}],
      "slots":[{"name":"body","bone":"root","dark":"102030"}],
      "constraints":[
        {"name":"follow-path","type":"path","order":0},
        {"name":"jiggle","type":"physics","order":1}
      ],
      "skins":[{
        "name":"default",
        "constraints":["follow-path"]
      }]
    }"#;
    let report = load_json(json, b"page.png\n")
        .expect("known unsupported profile records must remain bounded");
    let asset = report.asset();
    let constraints = asset.constraints().collect::<Vec<_>>();
    assert_eq!(
        constraints
            .iter()
            .map(|constraint| constraint.source_type())
            .collect::<Vec<_>>(),
        ["path", "physics"]
    );

    for constraint in &constraints {
        assert!(report.diagnostics().iter().any(|diagnostic| {
            diagnostic.code() == DiagnosticCode::UnsupportedConstraintType
                && diagnostic.scope() == DiagnosticScope::Constraint(constraint.id())
        }));
    }
    let slot = asset.slots().next().expect("the tripwire has one slot");
    assert!(report.diagnostics().iter().any(|diagnostic| {
        diagnostic.code() == DiagnosticCode::UnsupportedTwoColourTint
            && diagnostic.scope() == DiagnosticScope::Slot(slot.id())
    }));
    let skin = asset.skins().next().expect("the tripwire has one skin");
    assert!(report.diagnostics().iter().any(|diagnostic| {
        diagnostic.code() == DiagnosticCode::IgnoredSkinConstraints
            && diagnostic.scope() == DiagnosticScope::Skin(skin.id())
    }));
}

#[test]
fn compatible_but_untested_patch_and_active_unsupported_data_are_structured() {
    let json = r#"{
      "skeleton": { "spine": "4.3.24" },
      "bones": [
        { "name": "root" },
        { "name": "odd", "parent": "root", "transform": "onlyTranslation" }
      ],
      "slots": [
        { "name": "mesh-slot", "bone": "odd", "attachment": "mesh", "blend": "future-light" }
      ],
      "skins": [
        {
          "name": "default",
          "attachments": {
            "mesh-slot": {
              "mesh": {
                "type": "mesh",
                "uvs": [0, 0, 1, 0, 1, 1],
                "triangles": [0, 1, 2],
                "vertices": [0, 0, 1, 0, 1, 1]
              }
            }
          }
        }
      ]
    }"#;
    let atlas = "page.png\n\tpma: true\n";

    let report = load_json(json.as_bytes(), atlas.as_bytes())
        .expect("known unsupported records should load coherently");
    assert!(report.has_degradations());

    let diagnostics = report.diagnostics();
    let slot = report.asset().slots().next().expect("one slot");
    assert_eq!(slot.blend_mode(), SlotBlendMode::Unknown);
    assert_eq!(slot.blend_token(), "future-light");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code() == DiagnosticCode::UntestedPatchVersion
            && diagnostic.severity() == DiagnosticSeverity::Warning
            && diagnostic.scope() == DiagnosticScope::Asset
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code() == DiagnosticCode::UnsupportedBoneTransformMode
            && matches!(diagnostic.scope(), DiagnosticScope::Bone(_))
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code() == DiagnosticCode::UnsupportedBlendMode
            && matches!(diagnostic.scope(), DiagnosticScope::Slot(_))
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code() == DiagnosticCode::UnsupportedAttachmentType
            && matches!(diagnostic.scope(), DiagnosticScope::Attachment(_))
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code() == DiagnosticCode::AlphaEncodingMismatch
            && matches!(diagnostic.scope(), DiagnosticScope::AtlasPage(_))
    }));
}

#[test]
fn safely_bounded_unsupported_records_load_as_precise_sentinels() {
    let json = br#"{
      "skeleton": { "spine": "4.3.23" },
      "bones": [{ "name": "root" }],
      "slots": [
        { "name": "visual", "bone": "root", "attachment": "cycle" },
        { "name": "hitbox", "bone": "root", "attachment": "bounds" }
      ],
      "skins": [{
        "name": "default",
        "attachments": {
          "visual": {
            "cycle": {
              "width": 8,
              "height": 8,
              "sequence": { "count": 3 }
            }
          },
          "hitbox": {
            "bounds": {
              "type": "boundingbox",
              "vertexCount": 4,
              "vertices": [0, 0, 8, 0, 8, 8, 0, 8]
            }
          }
        }
      }],
      "futureSection": { "enabled": true }
    }"#;

    let report = load_json(json, b"page.png\n")
        .expect("records with clear boundaries should survive unsupported features");
    let asset = report.asset();
    let visual_slot = asset.slots().next().expect("visual slot");
    let cycle = asset
        .attachment(
            asset
                .default_skin()
                .expect("fixture has a default skin")
                .attachment(
                    visual_slot.id(),
                    visual_slot
                        .setup_attachment_name()
                        .expect("slot has a setup attachment placeholder"),
                )
                .expect("slot belongs to this asset")
                .expect("unsupported sentinel remains linked"),
        )
        .expect("linked attachment");
    assert_eq!(cycle.kind(), AttachmentKind::Unsupported);
    assert_eq!(cycle.unsupported_type(), Some("region"));
    assert!(report.diagnostics().iter().any(|diagnostic| {
        diagnostic.code() == DiagnosticCode::UnsupportedAttachmentType
            && diagnostic.severity() == DiagnosticSeverity::Degraded
            && diagnostic.scope() == DiagnosticScope::Attachment(cycle.id())
    }));
    assert!(report.diagnostics().iter().any(|diagnostic| {
        diagnostic.code() == DiagnosticCode::UnknownField
            && diagnostic.scope() == DiagnosticScope::Asset
    }));

    let bounds = asset
        .attachments()
        .find(|attachment| attachment.name() == "bounds")
        .expect("bounding box retained");
    assert_eq!(bounds.kind(), AttachmentKind::BoundingBox);
    assert!(report.diagnostics().iter().any(|diagnostic| {
        diagnostic.code() == DiagnosticCode::UnsupportedAttachmentType
            && diagnostic.severity() == DiagnosticSeverity::Warning
            && diagnostic.scope() == DiagnosticScope::Attachment(bounds.id())
    }));
}

#[test]
fn unknown_skeleton_metadata_is_never_silent() {
    let report = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23","futureScale":2},
          "bones":[{"name":"root"}]
        }"#,
        b"page.png\n",
    )
    .expect("bounded unknown metadata should degrade rather than fail");
    assert!(report.diagnostics().iter().any(|diagnostic| {
        diagnostic.code() == DiagnosticCode::UnknownField
            && diagnostic.scope() == DiagnosticScope::Asset
            && diagnostic.message().contains("futureScale")
    }));
}

#[test]
fn fatal_json_errors_have_stable_categories_paths_and_locations() {
    let syntax_error = load_json(
        b"{\n  \"skeleton\": { \"spine\": \"4.3.23\" },\n  \"bones\": [}",
        b"page.png\n",
    )
    .expect_err("invalid JSON must fail");
    assert_eq!(syntax_error.kind(), LoadErrorKind::Syntax);
    let location = syntax_error.location();
    assert_eq!(location.document(), LoadDocument::SkeletonJson);
    assert_eq!(location.line(), Some(3));
    assert!(location.column().is_some_and(|column| column > 0));

    let wrong_version = load_json(
        br#"{"skeleton":{"spine":"4.2.99"},"bones":[{"name":"root"}]}"#,
        b"page.png\n",
    )
    .expect_err("a different major/minor schema must fail");
    assert_eq!(wrong_version.kind(), LoadErrorKind::UnsupportedVersion);
    assert_eq!(wrong_version.path(), Some("/skeleton/spine"));

    let duplicate = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[{"name":"root"},{"name":"root"}]
        }"#,
        b"page.png\n",
    )
    .expect_err("duplicate required names must fail");
    assert_eq!(duplicate.kind(), LoadErrorKind::DuplicateName);
    assert_eq!(duplicate.path(), Some("/bones/1/name"));

    let bad_parent_order = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[{"name":"child","parent":"root"},{"name":"root"}]
        }"#,
        b"page.png\n",
    )
    .expect_err("parents must precede children");
    assert_eq!(bad_parent_order.kind(), LoadErrorKind::InvalidTopology);
    assert_eq!(bad_parent_order.path(), Some("/bones/0/parent"));
}

#[test]
fn curve_coordinate_conversion_rejects_unrepresentable_results_with_real_paths() {
    let absolute_x_overflow = br#"{
      "skeleton":{"spine":"4.3.23"},
      "bones":[{"name":"root"}],
      "animations":{
        "tiny":{
          "bones":{
            "root":{
              "rotate":[
                {"value":0,"curve":[3e38,0,-3e38,0]},
                {"time":0.000000002,"value":1}
              ]
            }
          }
        }
      }
    }"#;
    let error = load_json(absolute_x_overflow, b"page.png\n")
        .expect_err("normalizing huge absolute time handles must not overflow f32");
    assert_eq!(error.kind(), LoadErrorKind::NonFiniteNumber);
    assert_eq!(
        error.path(),
        Some("/animations/tiny/bones/root/rotate/0/curve/0")
    );

    let compact_y_overflow = br#"{
      "skeleton":{"spine":"4.3.23"},
      "bones":[{"name":"root"}],
      "animations":{
        "extreme":{
          "bones":{
            "root":{
              "rotate":[
                {"value":3.4e38,"curve":0.5,"c2":3.4e38,"c3":0.5,"c4":0},
                {"time":1,"value":-3.4e38}
              ]
            }
          }
        }
      }
    }"#;
    let error = load_json(compact_y_overflow, b"page.png\n")
        .expect_err("denormalizing huge compact value handles must not overflow f32");
    assert_eq!(error.kind(), LoadErrorKind::NonFiniteNumber);
    assert_eq!(
        error.path(),
        Some("/animations/extreme/bones/root/rotate/0/c2")
    );
}

#[test]
fn fatal_atlas_and_link_errors_are_never_silent() {
    let malformed_atlas = load_json(
        br#"{"skeleton":{"spine":"4.3.23"},"bones":[{"name":"root"}]}"#,
        b"page.png\n\tsize: 10, nope\n",
    )
    .expect_err("malformed known atlas values must fail");
    assert_eq!(malformed_atlas.kind(), LoadErrorKind::Syntax);
    let location = malformed_atlas.location();
    assert_eq!(location.document(), LoadDocument::Atlas);
    assert_eq!(location.line(), Some(2));

    let missing_region = load_json(MINIMAL_JSON.as_bytes(), b"page.png\n")
        .expect_err("a required region must be linked");
    assert_eq!(missing_region.kind(), LoadErrorKind::MissingAtlasRegion);
    assert_eq!(
        missing_region.path(),
        Some("/skins/0/attachments/body-slot/body")
    );

    let empty_actual_name = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[{"name":"root"}],
          "slots":[{"name":"slot","bone":"root"}],
          "skins":[{
            "name":"default",
            "attachments":{"slot":{"placeholder":{"name":"","width":1,"height":1}}}
          }]
        }"#,
        b"page.png\nplaceholder\n\tbounds:0,0,1,1\n",
    )
    .expect_err("explicit attachment identities must not be empty");
    assert_eq!(empty_actual_name.kind(), LoadErrorKind::SchemaViolation);
    assert_eq!(
        empty_actual_name.path(),
        Some("/skins/0/attachments/slot/placeholder/name")
    );
}

#[test]
fn invalid_ik_target_ancestry_and_draw_order_offsets_are_fatal() {
    let descendant_target = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[
            {"name":"root"},
            {"name":"upper","parent":"root"},
            {"name":"target","parent":"upper"}
          ],
          "ik":[{
            "name":"invalid",
            "bones":["upper"],
            "target":"target"
          }]
        }"#,
        b"page.png\n",
    )
    .expect_err("an IK target cannot descend from a constrained bone");
    assert_eq!(descendant_target.kind(), LoadErrorKind::InvalidTopology);
    assert_eq!(descendant_target.path(), Some("/ik/0/target"));

    let invalid_draw_order = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[{"name":"root"}],
          "slots":[
            {"name":"back","bone":"root"},
            {"name":"front","bone":"root"}
          ],
          "animations":{
            "invalid":{
              "drawOrder":[{
                "offsets":[
                  {"slot":"back","offset":1},
                  {"slot":"front","offset":0}
                ]
              }]
            }
          }
        }"#,
        b"page.png\n",
    )
    .expect_err("two moved slots cannot occupy one draw-order destination");
    assert_eq!(invalid_draw_order.kind(), LoadErrorKind::InvalidOrder);
    assert_eq!(
        invalid_draw_order.path(),
        Some("/animations/invalid/drawOrder/0/offsets/1/offset")
    );
}

#[test]
fn skeleton_has_exactly_one_root_bone() {
    let error = load_json(
        br#"{
          "skeleton":{"spine":"4.3.23"},
          "bones":[{"name":"root"},{"name":"other-root"}]
        }"#,
        b"page.png\n",
    )
    .expect_err("a skeleton cannot contain multiple root bones");
    assert_eq!(error.kind(), LoadErrorKind::InvalidTopology);
    assert_eq!(error.path(), Some("/bones/1/parent"));
}

#[test]
fn loader_does_not_panic_on_arbitrary_bytes() {
    for bytes in [
        &[][..],
        &[0xFF, 0xFE, 0xFD],
        b"\0\0\0",
        b"{\"skeleton\":null}",
        b"[]",
    ] {
        let result = std::panic::catch_unwind(|| load_json(bytes, bytes));
        assert!(result.is_ok(), "loader panicked for {bytes:?}");
    }
}

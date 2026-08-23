//! Normative project-owned fixture gate driven by an external manifest.

use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use serde_json::{Map, Value};
use spinal::{
    AlphaEncoding, AnimationPlayer, Diagnostic, DiagnosticCode, DiagnosticScope,
    DiagnosticSeverity, LoadErrorKind, PlayOptions, PlaybackMode, Skeleton, SkeletonAsset,
    TextureFilter, TextureFormat, WrapMode, load_json,
};

const FIXTURE_ROOT_ENV: &str = "SPINAL_4_3_23_PROJECT_FIXTURES";
const COVERAGE: &str = include_str!("../../fixtures/COVERAGE.toml");

#[test]
#[ignore = "requires project-owned fixtures; see github.com/gak/spinal/blob/main/fixtures/PROJECT_INTAKE.md"]
fn project_owned_profile_exports_are_normative_4_3_23_evidence() {
    let root = fixture_root();
    let manifest = read_json(&root.join("MANIFEST.json"));
    assert_eq!(required_u64(&manifest, "format_version"), 1);
    assert_eq!(required_str(&manifest, "target_spine_version"), "4.3.23");
    let source_projects = required_array(&manifest, "source_projects");
    assert!(
        !source_projects.is_empty(),
        "at least one project-owned .spine source project is required"
    );
    for source in source_projects {
        let path = checked_file(
            &root,
            source
                .as_str()
                .expect("source project paths must be strings"),
        );
        assert_eq!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("spine"),
            "source project evidence must preserve .spine files"
        );
    }
    let project_provenance = required_object(&manifest, "project_provenance");
    for field in ["origin", "owner", "license", "redistribution_status"] {
        assert!(
            !project_provenance
                .get(field)
                .and_then(Value::as_str)
                .unwrap_or_else(|| {
                    panic!("manifest field `project_provenance.{field}` must be a string")
                })
                .trim()
                .is_empty(),
            "project provenance field `{field}` must not be empty"
        );
    }
    validate_manifest_lineage(&root, &manifest);
    validate_artwork_provenance(&root, &manifest);

    let positive = required_array(&manifest, "positive");
    assert!(
        !positive.is_empty(),
        "at least one positive export is required"
    );
    let mut positive_assets = BTreeMap::new();
    for case in positive {
        let id = required_str(case, "id");
        let asset = load_nonfatal_case(&root, case);
        assert!(
            asset.diagnostics().is_empty(),
            "positive case `{id}` has profile diagnostics: {:#?}",
            asset.diagnostics()
        );
        exercise_every_animation(Arc::clone(&asset), id);
        assert!(
            positive_assets
                .insert(
                    id.to_owned(),
                    (
                        asset,
                        read_case_json(&root, case),
                        read_case_atlas(&root, case),
                    ),
                )
                .is_none(),
            "duplicate positive case id `{id}`"
        );
    }

    let requirements = coverage_requirements();
    let mappings = required_array(&manifest, "coverage");
    let mut mapped_positive = BTreeSet::new();
    let mut mapped_tripwires = BTreeSet::new();
    let mut mapped_scale = BTreeSet::new();
    let mut tripwire_locations = BTreeMap::new();
    for mapping in mappings {
        let coverage_id = required_str(mapping, "id");
        let artifact = required_str(mapping, "artifact");
        let location = required_str(mapping, "location");
        assert!(
            !location.trim().is_empty(),
            "coverage `{coverage_id}` needs an exact JSON pointer, atlas record, or test location"
        );
        if requirements.positive.contains(coverage_id) {
            let (asset, json, atlas) = positive_assets.get(artifact).unwrap_or_else(|| {
                panic!("positive coverage `{coverage_id}` references unknown artifact `{artifact}`")
            });
            assert!(
                observes_supported_feature(coverage_id, asset, json, atlas),
                "artifact `{artifact}` does not observably exercise `{coverage_id}` at {location}"
            );
            assert_supported_evidence_location(coverage_id, location, asset, json, atlas);
            assert!(
                mapped_positive.insert(coverage_id.to_owned()),
                "positive coverage `{coverage_id}` is mapped more than once"
            );
        } else if requirements.tripwires.contains(coverage_id) {
            assert_eq!(
                artifact, coverage_id,
                "isolated tripwire artifacts use their COVERAGE.toml id"
            );
            assert!(
                mapped_tripwires.insert(coverage_id.to_owned()),
                "tripwire coverage `{coverage_id}` is mapped more than once"
            );
            tripwire_locations.insert(coverage_id.to_owned(), location.to_owned());
        } else if requirements.scale.contains(coverage_id) {
            assert_eq!(
                artifact, "scale-probe",
                "the scale/Nonessential quartet uses the `scale-probe` artifact id"
            );
            assert_eq!(
                location, "scale-probe",
                "the scale/Nonessential coverage location is `scale-probe`"
            );
            assert!(
                mapped_scale.insert(coverage_id.to_owned()),
                "scale coverage `{coverage_id}` is mapped more than once"
            );
        } else {
            panic!("coverage mapping `{coverage_id}` is not a required project-owned observation");
        }
    }
    assert_eq!(
        mapped_positive, requirements.positive,
        "positive manifest coverage must map every required supported wire feature exactly once"
    );
    assert_eq!(
        mapped_scale, requirements.scale,
        "the scale probe needs an exact manifest coverage mapping"
    );

    let tripwires = required_array(&manifest, "tripwires");
    let mut observed_tripwires = BTreeSet::new();
    let mut tripwire_artifacts = BTreeSet::new();
    for case in tripwires {
        let coverage_id = required_str(case, "coverage_id");
        assert!(
            requirements.tripwires.contains(coverage_id),
            "unknown or non-tripwire coverage id `{coverage_id}`"
        );
        assert!(
            observed_tripwires.insert(coverage_id.to_owned()),
            "duplicate tripwire case `{coverage_id}`"
        );
        if matches!(
            coverage_id,
            "non-quarter-atlas-rotation" | "unknown-atlas-page-setting"
        ) {
            assert_eq!(
                required_str(case, "source_kind"),
                "derived",
                "{coverage_id} must trace its edited atlas record to a raw editor export"
            );
        }
        validate_case_evidence(&root, case);
        let artifact_identity = if case.get("binary").is_some() {
            format!("binary:{}", required_str(case, "binary"))
        } else {
            format!(
                "json:{}|atlas:{}",
                required_str(case, "json"),
                required_str(case, "atlas")
            )
        };
        assert!(
            tripwire_artifacts.insert(artifact_identity),
            "each isolated tripwire needs a distinct exported or derived artifact"
        );
        let inseparable = required_array(case, "inseparable_with")
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .expect("inseparable_with entries must be coverage IDs")
                    .to_owned()
            })
            .collect::<BTreeSet<_>>();
        if coverage_id == "binary-skeleton" {
            assert!(
                inseparable.is_empty(),
                "binary-skeleton has no inseparable JSON-profile companion"
            );
            let binary = checked_file(&root, required_str(case, "binary"));
            assert!(
                binary
                    .extension()
                    .is_some_and(|extension| extension == "skel"),
                "binary-skeleton must preserve the editor's .skel export"
            );
            let binary_bytes = fs::read(&binary).expect("binary editor export is readable");
            assert!(
                !binary_bytes.is_empty(),
                "binary-skeleton must preserve a nonempty editor export"
            );
            let atlas = fs::read(checked_file(&root, required_str(case, "atlas")))
                .expect("binary tripwire atlas is readable");
            assert!(
                load_json(&binary_bytes, &atlas).is_err(),
                "the JSON-only standalone entry point must reject binary skeleton bytes"
            );
            assert_eq!(required_str(case, "expected"), "not-accepted");
            let expected_location = format!("binary:{}", required_str(case, "binary"));
            assert_eq!(
                tripwire_locations.get(coverage_id).map(String::as_str),
                Some(expected_location.as_str()),
                "binary coverage location must identify the exact .skel artifact"
            );
            continue;
        }

        let asset = load_nonfatal_case(&root, case);
        let raw_json = read_case_json(&root, case);
        let raw_atlas = read_case_atlas(&root, case);
        let actual_features =
            observed_tripwire_features(&requirements.tripwires, &raw_json, &raw_atlas, &asset);
        let mut expected_features = inseparable;
        expected_features.insert(coverage_id.to_owned());
        assert_eq!(
            actual_features, expected_features,
            "tripwire `{coverage_id}` must contain only its named feature and declared unavoidable companions"
        );
        assert_tripwire_evidence_location(
            coverage_id,
            tripwire_locations
                .get(coverage_id)
                .expect("tripwire mapping was checked"),
            &raw_json,
            &raw_atlas,
        );
        let mut actual = asset
            .diagnostics()
            .iter()
            .map(diagnostic_signature)
            .collect::<Vec<_>>();
        actual.sort();
        let mut expected = expected_features
            .iter()
            .map(|feature| {
                tripwire_expectation(feature).unwrap_or_else(|| {
                    panic!("tripwire `{feature}` needs an authoritative diagnostic contract")
                })
            })
            .collect::<Vec<_>>();
        expected.sort();
        assert_eq!(
            actual, expected,
            "tripwire `{coverage_id}` must produce the authoritative diagnostic count, severity, code, and scope"
        );
        exercise_every_animation(asset, coverage_id);
    }
    assert_eq!(
        observed_tripwires, requirements.tripwires,
        "manifest must contain exactly one case for every required unsupported tripwire"
    );
    assert_eq!(
        mapped_tripwires, requirements.tripwires,
        "every tripwire needs an exact artifact and source-record coverage mapping"
    );

    validate_fatal_cases(&root, &manifest);
    validate_scale_probe(&root, &manifest);
}

fn assert_supported_evidence_location(
    id: &str,
    location: &str,
    asset: &SkeletonAsset,
    json: &Value,
    atlas: &str,
) {
    if !location.starts_with("json:") {
        assert!(
            observes_supported_atlas_record(id, location, asset),
            "supported atlas coverage `{id}` is not demonstrated by `{location}`"
        );
        assert_evidence_location(location, json, atlas);
        return;
    }

    let pointer = location
        .strip_prefix("json:")
        .expect("JSON location prefix was checked");
    let expected_fragment = match id {
        "json-4-3-23" => "/skeleton/spine",
        "normal-bone-inheritance" => "/bones/",
        "rigid-region-attachment"
        | "weighted-mesh-attachment"
        | "unweighted-mesh-attachment"
        | "linked-mesh-attachment"
        | "attachment-only-skins" => "/skins/",
        "setup-slots" | "setup-draw-order" => "/slots",
        "one-bone-ik"
        | "two-bone-ik"
        | "ik-target"
        | "ik-order"
        | "ik-setup-mix"
        | "ik-setup-bend-direction"
        | "transform-rotation-constraint" => "/constraints",
        _timeline => "/animations/",
    };
    assert!(
        pointer.contains(expected_fragment),
        "supported coverage `{id}` must point inside `{expected_fragment}`"
    );
    let exact_marker = match id {
        "bone-rotate-timeline" => Some("/rotate"),
        "bone-translate-timeline" => Some("/translate"),
        "bone-scale-timeline" => Some("/scale"),
        "bone-shear-timeline" => Some("/shear"),
        "ik-mix-timeline" | "ik-bend-direction-timeline" => Some("/ik/"),
        "transform-mix-timeline" => Some("/transform/"),
        "slot-attachment-timeline" | "attachment-switching" => Some("/attachment"),
        "slot-colour-timeline" => Some("/rgba"),
        "draw-order-timeline" => Some("/drawOrder"),
        "events" => Some("/events"),
        _other => None,
    };
    if let Some(marker) = exact_marker {
        assert!(
            pointer.contains(marker),
            "supported coverage `{id}` must select its `{marker}` record"
        );
    }
    let selected = json
        .pointer(pointer)
        .unwrap_or_else(|| panic!("JSON evidence location `{location}` does not resolve"));
    assert!(
        observes_supported_json_value(id, selected),
        "supported coverage `{id}` is not demonstrated by the selected JSON record"
    );
    assert_evidence_location(location, json, atlas);
}

fn assert_tripwire_evidence_location(id: &str, location: &str, json: &Value, atlas: &str) {
    if matches!(
        id,
        "premultiplied-alpha" | "non-quarter-atlas-rotation" | "unknown-atlas-page-setting"
    ) {
        let line = location
            .strip_prefix("atlas:")
            .unwrap_or_else(|| panic!("tripwire `{id}` must point at an atlas record"));
        let (key, value) = line
            .split_once(':')
            .unwrap_or_else(|| panic!("tripwire `{id}` atlas location must be a property line"));
        match id {
            "premultiplied-alpha" => {
                assert_eq!((key.trim(), value.trim()), ("pma", "true"));
            }
            "non-quarter-atlas-rotation" => {
                assert_eq!(key.trim(), "rotate");
                assert!(value.trim().parse::<f32>().is_ok_and(|rotation| {
                    rotation.is_finite() && !matches!(rotation, 0.0 | 90.0 | 180.0 | 270.0 | 360.0)
                }));
            }
            "unknown-atlas-page-setting" => {
                assert!(
                    ![
                        "size", "format", "filter", "repeat", "pma", "scale", "bounds", "offsets",
                        "rotate", "index", "split", "pad",
                    ]
                    .contains(&key.trim()),
                    "unknown atlas setting location must identify the unknown property"
                );
            }
            _other => unreachable!(),
        }
    } else {
        let pointer = location
            .strip_prefix("json:")
            .unwrap_or_else(|| panic!("tripwire `{id}` needs a JSON pointer"));
        let expected_fragment = match id {
            "clipping-attachment"
            | "attachment-sequence"
            | "bounding-box-attachment"
            | "point-attachment"
            | "skin-specific-bones"
            | "skin-specific-constraints" => "/skins/",
            "path-constraint"
            | "unsupported-transform-constraint-option"
            | "physics-constraint" => "/constraints/",
            "two-colour-tint" | "non-normal-blend-mode" => "/slots/",
            "non-normal-bone-inheritance" => "/bones/",
            "deform-timeline"
            | "ik-softness-timeline"
            | "ik-compress-timeline"
            | "ik-stretch-timeline" => "/animations/",
            "ik-softness-setup"
            | "ik-compress-option"
            | "ik-stretch-option"
            | "ik-uniform-scaling-option" => "/constraints/",
            _other => panic!("tripwire `{id}` has no JSON location contract"),
        };
        assert!(
            pointer.contains(expected_fragment),
            "tripwire `{id}` must point inside `{expected_fragment}`"
        );
        let exact_marker = match id {
            "deform-timeline" => Some("/deform"),
            "ik-softness-timeline" | "ik-compress-timeline" | "ik-stretch-timeline" => Some("/ik/"),
            _other => None,
        };
        if let Some(marker) = exact_marker {
            assert!(
                pointer.contains(marker),
                "tripwire `{id}` must select its `{marker}` record"
            );
        }
        let selected = json
            .pointer(pointer)
            .unwrap_or_else(|| panic!("JSON evidence location `{location}` does not resolve"));
        assert!(
            observes_tripwire_json_value(id, selected),
            "tripwire `{id}` is not demonstrated by the selected JSON record"
        );
    }
    assert_evidence_location(location, json, atlas);
}

fn observes_supported_atlas_record(id: &str, location: &str, asset: &SkeletonAsset) -> bool {
    if let Some(ordinal) = location.strip_prefix("atlas-page:") {
        let Ok(ordinal) = ordinal.parse::<usize>() else {
            return false;
        };
        let Some(page) = asset.atlas_pages().nth(ordinal) else {
            return false;
        };
        return match id {
            "text-atlas" => true,
            "multi-page-atlas" => asset.atlas_pages().len() > 1,
            "straight-alpha-png" => page.alpha_encoding() == AlphaEncoding::Straight,
            "atlas-rgba8888-format" => page.format() == TextureFormat::Rgba8888,
            "atlas-linear-filter" => {
                page.min_filter() == TextureFilter::Linear
                    && page.mag_filter() == TextureFilter::Linear
            }
            "atlas-clamp-wrap" => page.wrap() == WrapMode::CLAMP,
            "atlas-positive-scale" => page.scale().is_finite() && page.scale() > 0.0,
            _other => false,
        };
    }
    if let Some(ordinal) = location.strip_prefix("atlas-region:") {
        let Ok(ordinal) = ordinal.parse::<usize>() else {
            return false;
        };
        let Some(region) = asset.atlas_regions().nth(ordinal) else {
            return false;
        };
        return match id {
            "atlas-packed-bounds" => region.bounds().width() > 0 && region.bounds().height() > 0,
            "atlas-indices" => region.index().is_some(),
            "atlas-whitespace-trim-offsets" => {
                region.trim().left() > 0 || region.trim().bottom() > 0
            }
            "atlas-original-size" => {
                let packed = region.bounds().size();
                let original = region.trim().original_size();
                packed.width() != original.width() || packed.height() != original.height()
            }
            "atlas-quarter-turn-rotation" => region.rotation().as_degrees() == 90.0,
            _other => false,
        };
    }
    false
}

fn observes_supported_json_value(id: &str, selected: &Value) -> bool {
    match id {
        "json-4-3-23" => selected.as_str() == Some("4.3.23"),
        "normal-bone-inheritance" => selected.as_object().is_some_and(|bone| {
            bone.get("name").and_then(Value::as_str).is_some()
                && bone
                    .get("inherit")
                    .and_then(Value::as_str)
                    .is_none_or(|inherit| inherit == "normal")
        }),
        "rigid-region-attachment" => selected.as_object().is_some_and(|attachment| {
            attachment
                .get("type")
                .and_then(Value::as_str)
                .is_none_or(|kind| kind == "region")
        }),
        "weighted-mesh-attachment" => selected.as_object().is_some_and(|attachment| {
            attachment.get("type").and_then(Value::as_str) == Some("mesh")
                && array_len(selected, "vertices") > array_len(selected, "uvs")
        }),
        "unweighted-mesh-attachment" => selected.as_object().is_some_and(|attachment| {
            attachment.get("type").and_then(Value::as_str) == Some("mesh")
                && array_len(selected, "vertices") > 0
                && array_len(selected, "vertices") == array_len(selected, "uvs")
        }),
        "linked-mesh-attachment" => selected.as_object().is_some_and(|attachment| {
            attachment.get("type").and_then(Value::as_str) == Some("linkedmesh")
                && attachment.get("parent").and_then(Value::as_str).is_some()
        }),
        "setup-slots" => selected.as_array().is_some_and(|slots| !slots.is_empty()),
        "setup-draw-order" => selected.as_array().is_some_and(|slots| slots.len() > 1),
        "attachment-switching" | "slot-attachment-timeline" => {
            selected.as_array().is_some_and(|frames| {
                !frames.is_empty()
                    && (id == "slot-attachment-timeline"
                        || distinct_frame_values(frames, "name") > 1)
            })
        }
        "attachment-only-skins" => selected.as_object().is_some_and(|skin| {
            skin.get("name").and_then(Value::as_str) != Some("default")
                && skin
                    .get("attachments")
                    .and_then(Value::as_object)
                    .is_some_and(|attachments| !attachments.is_empty())
        }),
        "one-bone-ik" => selected.as_object().is_some_and(|constraint| {
            is_ik_constraint(constraint) && array_len(selected, "bones") == 1
        }),
        "two-bone-ik" => selected.as_object().is_some_and(|constraint| {
            is_ik_constraint(constraint) && array_len(selected, "bones") == 2
        }),
        "ik-target" => selected.as_object().is_some_and(|constraint| {
            is_ik_constraint(constraint) && constraint.get("target").is_some()
        }),
        "ik-order" => selected.as_object().is_some_and(|constraint| {
            is_ik_constraint(constraint) && constraint.get("order").is_some()
        }),
        "ik-setup-mix" => selected.as_object().is_some_and(|constraint| {
            is_ik_constraint(constraint) && constraint.get("mix").is_some()
        }),
        "ik-setup-bend-direction" => selected.as_array().is_some_and(|constraints| {
            constraints
                .iter()
                .filter_map(Value::as_object)
                .filter(|constraint| is_ik_constraint(constraint))
                .map(|constraint| {
                    constraint
                        .get("bendPositive")
                        .and_then(Value::as_bool)
                        .unwrap_or(true)
                })
                .collect::<BTreeSet<_>>()
                == BTreeSet::from([false, true])
        }),
        "transform-rotation-constraint" => is_supported_rotation_transform_constraint(selected),
        "bone-rotate-timeline"
        | "bone-translate-timeline"
        | "bone-scale-timeline"
        | "bone-shear-timeline"
        | "slot-colour-timeline"
        | "draw-order-timeline"
        | "events" => selected.as_array().is_some_and(|frames| !frames.is_empty()),
        "ik-mix-timeline" => selected
            .as_array()
            .is_some_and(|frames| frames.iter().any(|frame| frame.get("mix").is_some())),
        "ik-bend-direction-timeline" => selected.as_array().is_some_and(|frames| {
            frames
                .iter()
                .map(|frame| {
                    frame
                        .get("bendPositive")
                        .and_then(Value::as_bool)
                        .unwrap_or(true)
                })
                .collect::<BTreeSet<_>>()
                == BTreeSet::from([false, true])
        }),
        "transform-mix-timeline" => selected
            .as_array()
            .is_some_and(|frames| frames.iter().any(|frame| frame.get("mixRotate").is_some())),
        "linear-interpolation" => selected
            .as_array()
            .is_some_and(|frames| frames_have_transition(frames, |curve| curve.is_none())),
        "stepped-interpolation" => selected.as_array().is_some_and(|frames| {
            frames_have_transition(frames, |curve| {
                curve.and_then(Value::as_str) == Some("stepped")
            })
        }),
        "bezier-interpolation" => selected.as_array().is_some_and(|frames| {
            frames_have_transition(frames, |curve| curve.is_some_and(Value::is_array))
        }),
        _other => false,
    }
}

fn is_ik_constraint(constraint: &Map<String, Value>) -> bool {
    constraint.get("type").and_then(Value::as_str) == Some("ik")
}

fn distinct_frame_values(frames: &[Value], key: &str) -> usize {
    frames
        .iter()
        .map(|frame| frame.get(key).and_then(Value::as_str))
        .collect::<BTreeSet<_>>()
        .len()
}

fn frames_have_transition(frames: &[Value], predicate: impl Fn(Option<&Value>) -> bool) -> bool {
    frames.len() > 1
        && frames[..frames.len() - 1].iter().any(|frame| {
            frame.as_object().is_some_and(|frame| {
                ["value", "x", "y", "rgba", "mix"]
                    .iter()
                    .any(|key| frame.contains_key(*key))
                    && predicate(frame.get("curve"))
            })
        })
}

fn observes_tripwire_json_value(id: &str, selected: &Value) -> bool {
    let objects = selected
        .as_array()
        .map(|values| values.iter().collect::<Vec<_>>())
        .unwrap_or_else(|| vec![selected]);
    match id {
        "deform-timeline" => {
            selected.as_array().is_some_and(|value| !value.is_empty())
                || selected.as_object().is_some_and(|value| !value.is_empty())
        }
        "clipping-attachment" => objects
            .iter()
            .any(|value| value.get("type").and_then(Value::as_str) == Some("clipping")),
        "path-constraint" | "physics-constraint" => {
            let expected = id.trim_end_matches("-constraint");
            objects
                .iter()
                .any(|value| value.get("type").and_then(Value::as_str) == Some(expected))
        }
        "unsupported-transform-constraint-option" => objects
            .iter()
            .any(|value| has_unsupported_transform_constraint_option(value)),
        "skin-specific-bones" => objects.iter().any(|value| nonempty_array(value, "bones")),
        "skin-specific-constraints" => objects.iter().any(|value| {
            ["ik", "transform", "path", "physics", "constraints"]
                .iter()
                .any(|field| nonempty_array(value, field))
        }),
        "attachment-sequence" => objects.iter().any(|value| value.get("sequence").is_some()),
        "two-colour-tint" => objects.iter().any(|value| {
            value.get("dark").is_some()
                || value.get("darkColor").is_some()
                || value.get("darkColour").is_some()
        }),
        "non-normal-blend-mode" => objects.iter().any(|value| {
            value
                .get("blend")
                .and_then(Value::as_str)
                .is_some_and(|blend| blend != "normal")
        }),
        "non-normal-bone-inheritance" => objects.iter().any(|value| {
            value
                .get("inherit")
                .and_then(Value::as_str)
                .is_some_and(|inherit| inherit != "normal")
        }),
        "ik-softness-setup" => objects.iter().any(|value| {
            value.as_object().is_some_and(is_ik_constraint)
                && value
                    .get("softness")
                    .and_then(Value::as_f64)
                    .is_some_and(|softness| softness != 0.0)
        }),
        "ik-softness-timeline" => objects.iter().any(|value| {
            value
                .get("softness")
                .and_then(Value::as_f64)
                .is_some_and(|softness| softness != 0.0)
        }),
        "ik-compress-timeline" => objects
            .iter()
            .any(|value| value.get("compress").and_then(Value::as_bool) == Some(true)),
        "ik-stretch-timeline" => objects
            .iter()
            .any(|value| value.get("stretch").and_then(Value::as_bool) == Some(true)),
        "ik-compress-option" | "ik-stretch-option" | "ik-uniform-scaling-option" => {
            let field = match id {
                "ik-compress-option" => "compress",
                "ik-stretch-option" => "stretch",
                "ik-uniform-scaling-option" => "uniform",
                _other => unreachable!(),
            };
            objects.iter().any(|value| {
                value.as_object().is_some_and(is_ik_constraint)
                    && value.get(field).and_then(Value::as_bool) == Some(true)
            })
        }
        "bounding-box-attachment" => objects
            .iter()
            .any(|value| value.get("type").and_then(Value::as_str) == Some("boundingbox")),
        "point-attachment" => objects
            .iter()
            .any(|value| value.get("type").and_then(Value::as_str) == Some("point")),
        _other => false,
    }
}

fn observed_tripwire_features(
    requirements: &BTreeSet<String>,
    json: &Value,
    atlas: &str,
    asset: &SkeletonAsset,
) -> BTreeSet<String> {
    requirements
        .iter()
        .filter(|id| id.as_str() != "binary-skeleton")
        .filter(|id| observes_tripwire_feature(id, json, atlas, asset))
        .cloned()
        .collect()
}

fn observes_tripwire_feature(id: &str, json: &Value, atlas: &str, asset: &SkeletonAsset) -> bool {
    let constraints = json
        .get("constraints")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let ik = constraints
        .iter()
        .filter(|constraint| constraint.get("type").and_then(Value::as_str) == Some("ik"))
        .collect::<Vec<_>>();
    match id {
        "deform-timeline" => {
            recursive_nonempty_key(json.get("animations").unwrap_or(&Value::Null), "deform")
        }
        "clipping-attachment" => has_attachment_type(json, "clipping"),
        "path-constraint" => has_constraint_type(json, "path"),
        "unsupported-transform-constraint-option" => constraints
            .iter()
            .any(has_unsupported_transform_constraint_option),
        "physics-constraint" => has_constraint_type(json, "physics"),
        "skin-specific-bones" => json
            .get("skins")
            .and_then(Value::as_array)
            .is_some_and(|skins| skins.iter().any(|skin| nonempty_array(skin, "bones"))),
        "skin-specific-constraints" => {
            json.get("skins")
                .and_then(Value::as_array)
                .is_some_and(|skins| {
                    skins.iter().any(|skin| {
                        ["ik", "transform", "path", "physics", "constraints"]
                            .iter()
                            .any(|field| nonempty_array(skin, field))
                    })
                })
        }
        "attachment-sequence" => {
            json_attachments(json).any(|attachment| attachment.get("sequence").is_some())
        }
        "two-colour-tint" => json
            .get("slots")
            .and_then(Value::as_array)
            .is_some_and(|slots| {
                slots.iter().any(|slot| {
                    slot.get("dark").is_some()
                        || slot.get("darkColor").is_some()
                        || slot.get("darkColour").is_some()
                })
            }),
        "non-normal-blend-mode" => {
            json.get("slots")
                .and_then(Value::as_array)
                .is_some_and(|slots| {
                    slots.iter().any(|slot| {
                        slot.get("blend")
                            .and_then(Value::as_str)
                            .is_some_and(|blend| blend != "normal")
                    })
                })
        }
        "non-normal-bone-inheritance" => {
            json.get("bones")
                .and_then(Value::as_array)
                .is_some_and(|bones| {
                    bones.iter().any(|bone| {
                        bone.get("inherit")
                            .and_then(Value::as_str)
                            .is_some_and(|inherit| inherit != "normal")
                    })
                })
        }
        "ik-softness-setup" => ik.iter().any(|constraint| {
            constraint
                .get("softness")
                .and_then(Value::as_f64)
                .is_some_and(|softness| softness != 0.0)
        }),
        "ik-softness-timeline" => ik_timeline_frames(json).iter().any(|frame| {
            frame
                .get("softness")
                .and_then(Value::as_f64)
                .is_some_and(|softness| softness != 0.0)
        }),
        "ik-compress-timeline" => ik_timeline_frames(json)
            .iter()
            .any(|frame| frame.get("compress").and_then(Value::as_bool) == Some(true)),
        "ik-stretch-timeline" => ik_timeline_frames(json)
            .iter()
            .any(|frame| frame.get("stretch").and_then(Value::as_bool) == Some(true)),
        "ik-compress-option" => ik
            .iter()
            .any(|constraint| constraint.get("compress").and_then(Value::as_bool) == Some(true)),
        "ik-stretch-option" => ik
            .iter()
            .any(|constraint| constraint.get("stretch").and_then(Value::as_bool) == Some(true)),
        "ik-uniform-scaling-option" => ik
            .iter()
            .any(|constraint| constraint.get("uniform").and_then(Value::as_bool) == Some(true)),
        "premultiplied-alpha" => atlas.lines().any(|line| {
            line.split_once(':')
                .is_some_and(|(key, value)| key.trim() == "pma" && value.trim() == "true")
        }),
        "non-quarter-atlas-rotation" => atlas.lines().any(|line| {
            let Some((key, value)) = line.split_once(':') else {
                return false;
            };
            if key.trim() != "rotate" {
                return false;
            }
            value.trim().parse::<f32>().is_ok_and(|rotation| {
                rotation.is_finite() && !matches!(rotation, 0.0 | 90.0 | 180.0 | 270.0 | 360.0)
            })
        }),
        "unknown-atlas-page-setting" => asset
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == DiagnosticCode::UnsupportedAtlasSetting),
        "bounding-box-attachment" => has_attachment_type(json, "boundingbox"),
        "point-attachment" => has_attachment_type(json, "point"),
        _other => false,
    }
}

fn has_attachment_type(json: &Value, expected: &str) -> bool {
    json_attachments(json)
        .any(|attachment| attachment.get("type").and_then(Value::as_str) == Some(expected))
}

fn has_constraint_type(json: &Value, expected: &str) -> bool {
    json.get("constraints")
        .and_then(Value::as_array)
        .is_some_and(|constraints| {
            constraints
                .iter()
                .any(|constraint| constraint.get("type").and_then(Value::as_str) == Some(expected))
        })
        || json
            .get(expected)
            .and_then(Value::as_array)
            .is_some_and(|constraints| !constraints.is_empty())
}

fn recursive_nonempty_key(value: &Value, expected: &str) -> bool {
    match value {
        Value::Object(object) => {
            object.get(expected).is_some_and(|value| {
                value.as_array().is_some_and(|value| !value.is_empty())
                    || value.as_object().is_some_and(|value| !value.is_empty())
            }) || object
                .values()
                .any(|value| recursive_nonempty_key(value, expected))
        }
        Value::Array(array) => array
            .iter()
            .any(|value| recursive_nonempty_key(value, expected)),
        _other => false,
    }
}

fn assert_evidence_location(location: &str, json: &Value, atlas: &str) {
    if let Some(pointer) = location.strip_prefix("json:") {
        assert!(
            pointer.starts_with('/') && json.pointer(pointer).is_some(),
            "JSON evidence location `{location}` does not resolve"
        );
    } else if let Some(line) = location.strip_prefix("atlas:") {
        assert!(
            !line.is_empty() && atlas.lines().any(|candidate| candidate.trim() == line),
            "atlas evidence location `{location}` does not match an exact trimmed record line"
        );
    } else if let Some(ordinal) = location
        .strip_prefix("atlas-page:")
        .or_else(|| location.strip_prefix("atlas-region:"))
    {
        assert!(
            ordinal.parse::<usize>().is_ok(),
            "atlas record location needs a source-order ordinal"
        );
    } else {
        panic!(
            "evidence location `{location}` must start with `json:`, `atlas:`, `atlas-page:`, or `atlas-region:`"
        );
    }
}

fn validate_artwork_provenance(root: &Path, manifest: &Value) {
    let mut exported_pages = BTreeSet::new();
    for section in ["positive", "tripwires", "fatal"] {
        for case in required_array(manifest, section) {
            exported_pages.extend(case_page_paths(case));
        }
    }
    let probe = required_object(manifest, "scale_probe");
    for name in ["a", "b", "c", "d"] {
        exported_pages.extend(case_page_paths(
            probe
                .get(name)
                .unwrap_or_else(|| panic!("scale_probe requires case `{name}`")),
        ));
    }
    assert!(
        !exported_pages.is_empty(),
        "project fixture manifest must list exported atlas pages"
    );

    let mut traced_pages = BTreeSet::new();
    for artwork in required_array(manifest, "artwork") {
        for field in ["origin", "owner", "license", "redistribution_status"] {
            assert!(
                !required_str(artwork, field).trim().is_empty(),
                "artwork provenance field `{field}` must not be empty"
            );
        }
        let sources = required_array(artwork, "source_files");
        assert!(
            !sources.is_empty(),
            "each artwork provenance row needs at least one preserved source file"
        );
        for source in sources {
            checked_file(
                root,
                source
                    .as_str()
                    .expect("artwork source file paths must be strings"),
            );
        }
        for page in required_array(artwork, "derived_pages") {
            let page = page
                .as_str()
                .expect("artwork derived page paths must be strings");
            assert!(
                exported_pages.contains(page),
                "artwork provenance references undeclared atlas page `{page}`"
            );
            traced_pages.insert(page.to_owned());
        }
    }
    assert_eq!(
        traced_pages, exported_pages,
        "every exported atlas page must trace to inventoried source artwork"
    );
}

fn case_page_paths(case: &Value) -> BTreeSet<String> {
    case.get("pages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|page| {
            page.as_str()
                .expect("atlas page paths must be strings")
                .to_owned()
        })
        .collect()
}

struct CoverageRequirements {
    positive: BTreeSet<String>,
    tripwires: BTreeSet<String>,
    scale: BTreeSet<String>,
}

fn coverage_requirements() -> CoverageRequirements {
    let mut positive = BTreeSet::new();
    let mut tripwires = BTreeSet::new();
    let mut scale = BTreeSet::new();
    for record in COVERAGE.split("[[coverage]]").skip(1) {
        let id = scalar(record, "id").expect("every coverage row has an id");
        if scalar(record, "production_observation").as_deref() == Some("runtime-only") {
            continue;
        }
        match scalar(record, "production_fixture").as_deref() {
            Some("production-profile-positive") => {
                positive.insert(id);
            }
            Some("production-profile-tripwires") => {
                tripwires.insert(id);
            }
            Some("production-scale-nonessential-probe") => {
                scale.insert(id);
            }
            _other => {}
        }
    }
    CoverageRequirements {
        positive,
        tripwires,
        scale,
    }
}

fn scalar(record: &str, key: &str) -> Option<String> {
    record.lines().find_map(|line| {
        let value = line.trim().strip_prefix(key)?.trim_start();
        value
            .strip_prefix('=')?
            .trim()
            .strip_prefix('"')?
            .strip_suffix('"')
            .map(ToOwned::to_owned)
    })
}

fn fixture_root() -> PathBuf {
    let root = env::var_os(FIXTURE_ROOT_ENV).unwrap_or_else(|| {
        panic!(
            "{FIXTURE_ROOT_ENV} must point at the project fixture root; \
             see https://github.com/gak/spinal/blob/main/fixtures/PROJECT_INTAKE.md"
        )
    });
    fs::canonicalize(&root)
        .unwrap_or_else(|error| panic!("failed to resolve {}: {error}", Path::new(&root).display()))
}

fn load_nonfatal_case(root: &Path, case: &Value) -> Arc<SkeletonAsset> {
    validate_case_evidence(root, case);
    let id = case_label(case);
    let json = fs::read(checked_file(root, required_str(case, "json")))
        .unwrap_or_else(|error| panic!("failed to read JSON for `{id}`: {error}"));
    let atlas = fs::read(checked_file(root, required_str(case, "atlas")))
        .unwrap_or_else(|error| panic!("failed to read atlas for `{id}`: {error}"));
    let report = load_json(&json, &atlas)
        .unwrap_or_else(|error| panic!("project fixture `{id}` must load: {error}"));
    let asset = report.into_asset();
    assert_eq!(asset.spine_version(), "4.3.23", "{id}");
    validate_declared_pages(root, case, &asset);
    asset
}

fn validate_declared_pages(root: &Path, case: &Value, asset: &SkeletonAsset) {
    let atlas_relative = Path::new(required_str(case, "atlas"));
    let atlas_parent = atlas_relative.parent().unwrap_or_else(|| Path::new(""));
    let actual = asset
        .atlas_pages()
        .map(|page| {
            let relative = atlas_parent.join(page.name());
            checked_file(
                root,
                relative
                    .to_str()
                    .expect("atlas page dependency path is UTF-8"),
            )
        })
        .collect::<BTreeSet<_>>();
    let declared = required_array(case, "pages")
        .iter()
        .map(|page| {
            checked_file(
                root,
                page.as_str().expect("atlas page paths must be strings"),
            )
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        declared,
        actual,
        "{} must list exactly the atlas page dependencies",
        case_label(case)
    );
}

fn validate_case_evidence(root: &Path, case: &Value) {
    let id = case_label(case);
    for field in ["export_preset", "texture_packer_preset", "warnings"] {
        let path = checked_file(root, required_str(case, field));
        assert!(
            fs::metadata(&path)
                .unwrap_or_else(|error| panic!("failed to inspect {}: {error}", path.display()))
                .len()
                > 0,
            "{id} evidence `{field}` must not be empty"
        );
    }
    validate_case_settings(case);
    if case.get("json").is_some() {
        let json = checked_file(root, required_str(case, "json"));
        assert!(
            json.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".spine.json")),
            "{id} JSON must use the Bevy-loadable .spine.json compound extension"
        );
    }
    if case.get("atlas").is_some() {
        checked_file(root, required_str(case, "atlas"));
        let pages = required_array(case, "pages");
        assert!(!pages.is_empty(), "{id} must list every atlas page");
        for page in pages {
            checked_file(
                root,
                page.as_str()
                    .unwrap_or_else(|| panic!("{id} page paths must be strings")),
            );
        }
        validate_declared_pages_from_atlas(root, case);
    }
}

fn validate_case_settings(case: &Value) {
    let id = case_label(case);
    let settings = required_object(case, "settings");
    assert_eq!(
        settings.get("editor_version").and_then(Value::as_str),
        Some("4.3.23"),
        "{id} settings must record the exact editor version"
    );
    let expected_format = if case.get("binary").is_some() {
        "binary"
    } else {
        "json"
    };
    assert_eq!(
        settings.get("format").and_then(Value::as_str),
        Some(expected_format),
        "{id} settings record the actual skeleton format"
    );
    assert_eq!(
        settings.get("animation_cleanup").and_then(Value::as_bool),
        Some(false),
        "{id} must not use Animation clean up"
    );
    assert_eq!(
        settings.get("warnings").and_then(Value::as_bool),
        Some(true),
        "{id} must export with warnings enabled"
    );
    assert_eq!(
        settings.get("warning_count").and_then(Value::as_u64),
        Some(0),
        "{id} must preserve an explicit zero-warning export result"
    );
    let expected_nonessential = case
        .get("nonessential")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    assert_eq!(
        settings.get("nonessential").and_then(Value::as_bool),
        Some(expected_nonessential),
        "{id} settings must match its declared Nonessential state"
    );
    assert_eq!(
        settings.get("pack_atlas").and_then(Value::as_bool),
        Some(true),
        "{id} must preserve its texture-pack run"
    );

    let texture = settings
        .get("texture")
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("{id} settings need a texture snapshot"));
    for (field, expected) in [
        ("format", "RGBA8888"),
        ("min_filter", "Linear"),
        ("mag_filter", "Linear"),
        ("wrap_x", "ClampToEdge"),
        ("wrap_y", "ClampToEdge"),
    ] {
        assert_eq!(
            texture.get(field).and_then(Value::as_str),
            Some(expected),
            "{id} texture setting `{field}` is outside the profile"
        );
    }
    for field in [
        "strip_whitespace_x",
        "strip_whitespace_y",
        "edge_padding",
        "rotation",
    ] {
        assert_eq!(
            texture.get(field).and_then(Value::as_bool),
            Some(true),
            "{id} texture setting `{field}` must be enabled"
        );
    }
    assert!(
        texture
            .get("padding_x")
            .and_then(Value::as_u64)
            .is_some_and(|padding| padding >= 2)
            && texture
                .get("padding_y")
                .and_then(Value::as_u64)
                .is_some_and(|padding| padding >= 2),
        "{id} needs at least two pixels of texture padding"
    );
    assert_eq!(
        texture.get("scale").and_then(Value::as_f64),
        Some(1.0),
        "{id} texture scale must be 1"
    );
    let pma_tripwire =
        case.get("coverage_id").and_then(Value::as_str) == Some("premultiplied-alpha");
    assert_eq!(
        texture.get("pma").and_then(Value::as_bool),
        Some(pma_tripwire),
        "{id} PMA setting must match its profile role"
    );
    if !pma_tripwire {
        assert_eq!(
            texture.get("bleed").and_then(Value::as_bool),
            Some(true),
            "{id} straight-alpha pages require bleed"
        );
    }
}

fn validate_case_lineage(root: &Path, case: &Value) {
    let id = case_label(case);
    match required_str(case, "source_kind") {
        "raw-editor-export" => {
            let archive = required_str(case, "raw_archive");
            checked_file(root, archive);
            let checksum = required_str(case, "raw_archive_sha256");
            assert!(
                checksum.len() == 64
                    && checksum
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
                "{id} raw archive checksum must be a lowercase SHA-256 hex digest"
            );
            let recorded = checksum_ledger(root).remove(archive).unwrap_or_else(|| {
                panic!("{id} raw archive `{archive}` is missing from SHA256SUMS")
            });
            assert_eq!(
                checksum, recorded,
                "{id} raw archive checksum must match immutable intake evidence"
            );
        }
        "derived" => {
            let source = required_str(case, "derived_from");
            checked_file(root, source);
            let expected = required_str(case, "derived_from_sha256");
            let recorded = checksum_ledger(root)
                .remove(source)
                .unwrap_or_else(|| panic!("{id} source `{source}` is missing from SHA256SUMS"));
            assert_eq!(
                expected, recorded,
                "{id} derived source checksum must match immutable intake evidence"
            );
            assert!(
                !required_str(case, "derivation").trim().is_empty(),
                "{id} needs a reproducible derivation description"
            );
        }
        other => panic!("{id} has unsupported source_kind `{other}`"),
    }
}

fn validate_manifest_lineage(root: &Path, manifest: &Value) {
    let cases = manifest_cases(manifest);
    let declared_projects = required_array(manifest, "source_projects")
        .iter()
        .map(|project| {
            project
                .as_str()
                .expect("source project paths must be strings")
        })
        .collect::<BTreeSet<_>>();
    let ledger = checksum_ledger(root);
    let declared_outputs = cases
        .iter()
        .flat_map(|case| case_output_paths(case))
        .collect::<BTreeSet<_>>();
    let mut raw_outputs = BTreeSet::new();

    for case in &cases {
        validate_case_lineage(root, case);
        if required_str(case, "source_kind") != "raw-editor-export" {
            continue;
        }

        let archive = required_str(case, "raw_archive");
        let archive_path = Path::new(archive);
        assert!(
            archive_path.starts_with("raw") && is_archive_path(archive_path),
            "{} raw archive must be a .zip file under raw/",
            case_label(case)
        );
        let archive_file = checked_file(root, archive);
        let archive_members = read_zip_members(&archive_file);
        assert!(
            !declared_outputs.contains(archive),
            "{} raw archive must be distinct from every extracted fixture output",
            case_label(case)
        );
        validate_raw_archive_members(root, case, &archive_members);

        let source_project = required_str(case, "source_project");
        assert!(
            declared_projects.contains(source_project),
            "{} source_project must name one of the top-level source_projects",
            case_label(case)
        );
        let source_checksum = required_str(case, "source_project_sha256");
        let recorded_checksum = ledger.get(source_project).unwrap_or_else(|| {
            panic!(
                "{} source project `{source_project}` is missing from SHA256SUMS",
                case_label(case)
            )
        });
        assert_eq!(
            source_checksum,
            recorded_checksum,
            "{} source project checksum must identify the exact editor-project revision",
            case_label(case)
        );
        raw_outputs.extend(case_output_paths(case));
    }

    for case in &cases {
        if required_str(case, "source_kind") != "derived" {
            continue;
        }
        let source = required_str(case, "derived_from");
        assert!(
            raw_outputs.contains(source),
            "{} derived_from must name an exported artifact belonging to a declared raw editor case",
            case_label(case)
        );
        assert!(
            !case_output_paths(case).contains(source),
            "{} derived fixture cannot name one of its own outputs as its raw source",
            case_label(case)
        );
    }
}

fn manifest_cases(manifest: &Value) -> Vec<&Value> {
    let mut cases = Vec::new();
    for section in ["positive", "tripwires", "fatal"] {
        cases.extend(required_array(manifest, section));
    }
    let probe = required_object(manifest, "scale_probe");
    for name in ["a", "b", "c", "d"] {
        cases.push(
            probe
                .get(name)
                .unwrap_or_else(|| panic!("scale_probe requires case `{name}`")),
        );
    }
    cases
}

fn case_output_paths(case: &Value) -> BTreeSet<&str> {
    let mut paths = BTreeSet::new();
    for field in ["json", "binary", "atlas"] {
        if let Some(path) = case.get(field).and_then(Value::as_str) {
            paths.insert(path);
        }
    }
    paths.extend(
        case.get("pages")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|page| page.as_str().expect("atlas page paths must be strings")),
    );
    paths
}

fn is_archive_path(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    name.ends_with(".zip")
}

struct ZipMemberEvidence {
    bytes: Vec<u8>,
}

fn validate_raw_archive_members(
    root: &Path,
    case: &Value,
    archive_members: &BTreeMap<String, ZipMemberEvidence>,
) {
    let mut expected_paths = case_output_paths(case);
    expected_paths.extend([
        required_str(case, "source_project"),
        required_str(case, "export_preset"),
        required_str(case, "texture_packer_preset"),
        required_str(case, "warnings"),
    ]);
    let mapping = required_object(case, "raw_archive_members");
    let mapped_paths = mapping.keys().map(String::as_str).collect::<BTreeSet<_>>();
    assert_eq!(
        mapped_paths,
        expected_paths,
        "{} raw_archive_members must map every extracted source, export, atlas page, preset, and warning artifact exactly once",
        case_label(case)
    );

    let mut used_members = BTreeSet::new();
    for (extracted, member) in mapping {
        let member = member.as_str().unwrap_or_else(|| {
            panic!(
                "{} raw archive member names must be strings",
                case_label(case)
            )
        });
        assert!(
            used_members.insert(member),
            "{} raw archive member `{member}` is mapped more than once",
            case_label(case)
        );
        let evidence = archive_members.get(member).unwrap_or_else(|| {
            panic!(
                "{} raw archive does not contain mapped member `{member}`",
                case_label(case)
            )
        });
        let bytes = fs::read(checked_file(root, extracted)).unwrap_or_else(|error| {
            panic!("failed to read extracted archive member `{extracted}`: {error}")
        });
        assert_eq!(
            evidence.bytes,
            bytes,
            "{} extracted `{extracted}` does not exactly match decompressed archive member `{member}`",
            case_label(case)
        );
    }
}

fn read_zip_members(path: &Path) -> BTreeMap<String, ZipMemberEvidence> {
    let bytes = fs::read(path)
        .unwrap_or_else(|error| panic!("failed to read raw archive {}: {error}", path.display()));
    assert!(bytes.len() >= 22, "raw archive is too short to be ZIP");
    let search_start = bytes.len().saturating_sub(65_557);
    let eocd = (search_start..=bytes.len() - 4)
        .rev()
        .find(|offset| bytes[*offset..*offset + 4] == [0x50, 0x4b, 0x05, 0x06])
        .expect("raw archive has a ZIP end-of-central-directory record");
    assert_eq!(
        read_u16(&bytes, eocd + 4),
        0,
        "multi-disk ZIP is unsupported"
    );
    assert_eq!(
        read_u16(&bytes, eocd + 6),
        0,
        "multi-disk ZIP is unsupported"
    );
    let entry_count = usize::from(read_u16(&bytes, eocd + 10));
    let central_size = usize::try_from(read_u32(&bytes, eocd + 12)).expect("ZIP size fits usize");
    let mut cursor = usize::try_from(read_u32(&bytes, eocd + 16)).expect("ZIP offset fits usize");
    let central_end = cursor
        .checked_add(central_size)
        .expect("ZIP central directory size does not overflow");
    assert!(
        central_end <= eocd,
        "ZIP central directory lies outside the archive"
    );

    let mut members = BTreeMap::new();
    for _entry in 0..entry_count {
        assert!(
            cursor + 46 <= central_end && bytes[cursor..cursor + 4] == [0x50, 0x4b, 0x01, 0x02],
            "ZIP central directory entry is malformed"
        );
        let flags = read_u16(&bytes, cursor + 8);
        assert_eq!(flags & 1, 0, "encrypted raw ZIP members are unsupported");
        let compression = read_u16(&bytes, cursor + 10);
        assert!(
            matches!(compression, 0 | 8),
            "raw ZIP member uses unsupported compression method {compression}"
        );
        let expected_crc32 = read_u32(&bytes, cursor + 16);
        let compressed_size =
            usize::try_from(read_u32(&bytes, cursor + 20)).expect("ZIP member size fits usize");
        let uncompressed_size = read_u32(&bytes, cursor + 24);
        let name_len = usize::from(read_u16(&bytes, cursor + 28));
        let extra_len = usize::from(read_u16(&bytes, cursor + 30));
        let comment_len = usize::from(read_u16(&bytes, cursor + 32));
        let local_offset =
            usize::try_from(read_u32(&bytes, cursor + 42)).expect("ZIP member offset fits usize");
        let name_start = cursor + 46;
        let name_end = name_start + name_len;
        assert!(name_end <= central_end, "ZIP member name is truncated");
        let name = std::str::from_utf8(&bytes[name_start..name_end])
            .expect("raw ZIP member names must be UTF-8")
            .to_owned();
        assert!(!name.is_empty(), "raw ZIP member name must not be empty");
        assert!(
            local_offset + 30 <= bytes.len()
                && bytes[local_offset..local_offset + 4] == [0x50, 0x4b, 0x03, 0x04],
            "ZIP member `{name}` has no matching local header"
        );
        assert_eq!(
            read_u16(&bytes, local_offset + 6),
            flags,
            "ZIP member `{name}` local and central flags differ"
        );
        assert_eq!(
            read_u16(&bytes, local_offset + 8),
            compression,
            "ZIP member `{name}` local and central compression methods differ"
        );
        if flags & 0x08 == 0 {
            assert_eq!(
                read_u32(&bytes, local_offset + 14),
                expected_crc32,
                "ZIP member `{name}` local and central CRC values differ"
            );
            assert_eq!(
                read_u32(&bytes, local_offset + 18),
                u32::try_from(compressed_size).expect("ZIP compressed size fits u32"),
                "ZIP member `{name}` local and central compressed sizes differ"
            );
            assert_eq!(
                read_u32(&bytes, local_offset + 22),
                uncompressed_size,
                "ZIP member `{name}` local and central uncompressed sizes differ"
            );
        }
        let local_name_len = usize::from(read_u16(&bytes, local_offset + 26));
        let local_extra_len = usize::from(read_u16(&bytes, local_offset + 28));
        let local_name_start = local_offset + 30;
        let local_name_end = local_name_start + local_name_len;
        assert!(
            local_name_end <= bytes.len()
                && bytes[local_name_start..local_name_end] == bytes[name_start..name_end],
            "ZIP member `{name}` local and central names differ"
        );
        let payload_start = local_offset + 30 + local_name_len + local_extra_len;
        let payload_end = payload_start
            .checked_add(compressed_size)
            .filter(|end| *end <= bytes.len())
            .unwrap_or_else(|| panic!("ZIP member `{name}` payload is truncated"));
        let compressed = &bytes[payload_start..payload_end];
        let decompressed = match compression {
            0 => compressed.to_vec(),
            8 => {
                let mut output = Vec::new();
                flate2::read::DeflateDecoder::new(compressed)
                    .read_to_end(&mut output)
                    .unwrap_or_else(|error| {
                        panic!("ZIP member `{name}` DEFLATE payload is invalid: {error}")
                    });
                output
            }
            _other => unreachable!(),
        };
        assert_eq!(
            decompressed.len() as u64,
            u64::from(uncompressed_size),
            "ZIP member `{name}` uncompressed size is invalid"
        );
        assert_eq!(
            crc32(&decompressed),
            expected_crc32,
            "ZIP member `{name}` payload CRC is invalid"
        );
        assert!(
            members
                .insert(
                    name.clone(),
                    ZipMemberEvidence {
                        bytes: decompressed,
                    },
                )
                .is_none(),
            "raw ZIP has duplicate member `{name}`"
        );
        cursor = name_end + extra_len + comment_len;
    }
    assert_eq!(
        cursor, central_end,
        "ZIP central directory size is inconsistent"
    );
    assert!(!members.is_empty(), "raw ZIP archive must contain members");
    members
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    let encoded = bytes
        .get(offset..offset + 2)
        .expect("ZIP integer is truncated");
    u16::from_le_bytes([encoded[0], encoded[1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    let encoded = bytes
        .get(offset..offset + 4)
        .expect("ZIP integer is truncated");
    u32::from_le_bytes([encoded[0], encoded[1], encoded[2], encoded[3]])
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _bit in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

fn checksum_ledger(root: &Path) -> BTreeMap<String, String> {
    let contents = fs::read_to_string(root.join("SHA256SUMS"))
        .expect("the project fixture checksum ledger is readable");
    contents
        .lines()
        .map(|line| {
            let split = line
                .find(char::is_whitespace)
                .expect("SHA256SUMS uses standard checksum lines");
            let checksum = &line[..split];
            let path = line[split..]
                .trim_start()
                .trim_start_matches('*')
                .trim_start_matches("./");
            (path.to_owned(), checksum.to_owned())
        })
        .collect()
}

fn validate_declared_pages_from_atlas(root: &Path, case: &Value) {
    const MINIMAL_JSON: &[u8] = br#"{"skeleton":{"spine":"4.3.23"},"bones":[{"name":"root"}]}"#;
    let atlas_path = checked_file(root, required_str(case, "atlas"));
    let atlas = fs::read(&atlas_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", atlas_path.display()));
    let asset = load_json(MINIMAL_JSON, &atlas)
        .unwrap_or_else(|error| {
            panic!(
                "{} atlas must parse independently: {error}",
                case_label(case)
            )
        })
        .into_asset();
    validate_declared_pages(root, case, &asset);
}

#[test]
fn every_project_owned_coverage_row_has_a_machine_gate() {
    const POSITIVE_MACHINE_CHECKS: &[&str] = &[
        "json-4-3-23",
        "text-atlas",
        "multi-page-atlas",
        "straight-alpha-png",
        "atlas-rgba8888-format",
        "atlas-linear-filter",
        "atlas-clamp-wrap",
        "atlas-positive-scale",
        "atlas-packed-bounds",
        "atlas-indices",
        "atlas-whitespace-trim-offsets",
        "atlas-original-size",
        "atlas-quarter-turn-rotation",
        "normal-bone-inheritance",
        "rigid-region-attachment",
        "weighted-mesh-attachment",
        "unweighted-mesh-attachment",
        "linked-mesh-attachment",
        "setup-slots",
        "setup-draw-order",
        "attachment-switching",
        "attachment-only-skins",
        "one-bone-ik",
        "two-bone-ik",
        "ik-target",
        "ik-order",
        "ik-setup-mix",
        "ik-setup-bend-direction",
        "transform-rotation-constraint",
        "bone-rotate-timeline",
        "bone-translate-timeline",
        "bone-scale-timeline",
        "bone-shear-timeline",
        "ik-mix-timeline",
        "ik-bend-direction-timeline",
        "transform-mix-timeline",
        "linear-interpolation",
        "stepped-interpolation",
        "bezier-interpolation",
        "slot-attachment-timeline",
        "slot-colour-timeline",
        "draw-order-timeline",
        "events",
    ];
    let requirements = coverage_requirements();
    assert_eq!(
        requirements.positive,
        POSITIVE_MACHINE_CHECKS
            .iter()
            .map(|id| (*id).to_owned())
            .collect(),
        "adding a supported wire-format row requires a corresponding observation check"
    );
    for id in [
        "ik-compress-timeline",
        "ik-stretch-timeline",
        "binary-skeleton",
        "premultiplied-alpha",
    ] {
        assert!(
            requirements.tripwires.contains(id),
            "unsupported project observation `{id}` remains gated"
        );
    }
    for id in &requirements.tripwires {
        if id != "binary-skeleton" {
            assert!(
                tripwire_expectation(id).is_some(),
                "tripwire `{id}` needs an authoritative diagnostic signature"
            );
        }
    }
    assert_eq!(
        requirements.scale,
        BTreeSet::from(["skeleton-reference-scale-nonessential-off-on".to_owned()])
    );
}

#[test]
fn scale_probe_diffing_tracks_structural_paths_without_assuming_field_names() {
    let a = serde_json::json!({"skeleton": {"spine": "4.3.23"}, "bones": [{"x": 1}]});
    let b = serde_json::json!({
        "skeleton": {"spine": "4.3.23", "images": "./images"},
        "bones": [{"x": 1}]
    });
    let c = serde_json::json!({"skeleton": {"spine": "4.3.23"}, "bones": [{"x": 2}]});
    let d = serde_json::json!({
        "skeleton": {"spine": "4.3.23", "images": "./images"},
        "bones": [{"x": 2}]
    });
    assert_eq!(
        differing_paths(&a, &b),
        BTreeSet::from(["/skeleton/images".to_owned()])
    );
    assert_eq!(
        differing_paths(&a, &c),
        BTreeSet::from(["/bones/0/x".to_owned()])
    );
    assert_eq!(
        differing_paths(&b, &d),
        BTreeSet::from(["/bones/0/x".to_owned()])
    );
}

fn read_case_json(root: &Path, case: &Value) -> Value {
    read_json(&checked_file(root, required_str(case, "json")))
}

fn read_case_atlas(root: &Path, case: &Value) -> String {
    let path = checked_file(root, required_str(case, "atlas"));
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {} as UTF-8: {error}", path.display()))
}

fn validate_fatal_cases(root: &Path, manifest: &Value) {
    let fatal = required_array(manifest, "fatal");
    assert!(
        !fatal.is_empty(),
        "at least one fatal-load fixture is required"
    );
    for case in fatal {
        assert_eq!(
            required_str(case, "source_kind"),
            "derived",
            "fatal fixtures must trace their deliberate damage to a raw editor export"
        );
        validate_case_evidence(root, case);
        let id = case_label(case);
        let json = fs::read(checked_file(root, required_str(case, "json")))
            .unwrap_or_else(|error| panic!("failed to read fatal JSON `{id}`: {error}"));
        let atlas = fs::read(checked_file(root, required_str(case, "atlas")))
            .unwrap_or_else(|error| panic!("failed to read fatal atlas `{id}`: {error}"));
        let error = load_json(&json, &atlas)
            .unwrap_err_or_else(|| panic!("fatal fixture `{id}` unexpectedly loaded"));
        assert_eq!(
            load_error_name(error.kind()),
            required_str(case, "expected_error"),
            "fatal fixture `{id}` returned the wrong stable error category"
        );
    }
}

fn validate_scale_probe(root: &Path, manifest: &Value) {
    let probe = required_object(manifest, "scale_probe");
    let non_default_scale = required_f64_object(probe, "non_default_reference_scale");
    assert!(
        non_default_scale.is_finite()
            && non_default_scale > 0.0
            && (non_default_scale - 1.0).abs() > f64::EPSILON,
        "scale probe must record one finite positive non-default reference scale"
    );

    let a = scale_case(root, probe, "a");
    let b = scale_case(root, probe, "b");
    let c = scale_case(root, probe, "c");
    let d = scale_case(root, probe, "d");
    let revision = |name| {
        let case = probe
            .get(name)
            .unwrap_or_else(|| panic!("scale_probe requires case `{name}`"));
        (
            required_str(case, "source_project"),
            required_str(case, "source_project_sha256"),
        )
    };
    let scale_one_revision = revision("a");
    let scaled_revision = revision("c");
    assert_eq!(
        scale_one_revision,
        revision("b"),
        "scale cases A/B must share the exact scale-1 .spine revision"
    );
    assert_eq!(
        scaled_revision,
        revision("d"),
        "scale cases C/D must share the exact non-default-scale .spine revision"
    );
    assert_ne!(
        scale_one_revision, scaled_revision,
        "changing the skeleton Reference scale must produce a separately preserved .spine revision"
    );
    let derivation = probe
        .get("source_revision_derivation")
        .and_then(Value::as_object)
        .expect("scale_probe needs source_revision_derivation");
    let derivation_str = |field| {
        derivation
            .get(field)
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("source_revision_derivation.{field} must be a string"))
    };
    assert_eq!(derivation_str("from_project"), scale_one_revision.0);
    assert_eq!(derivation_str("from_sha256"), scale_one_revision.1);
    assert_eq!(derivation_str("to_project"), scaled_revision.0);
    assert_eq!(derivation_str("to_sha256"), scaled_revision.1);
    assert_eq!(
        derivation_str("changed_property"),
        "skeleton.reference_scale"
    );
    assert_eq!(
        derivation.get("from_value").and_then(Value::as_f64),
        Some(1.0)
    );
    assert_eq!(
        derivation.get("to_value").and_then(Value::as_f64),
        Some(non_default_scale)
    );
    assert!(
        !derivation_str("procedure").trim().is_empty(),
        "scale source-revision derivation needs a reproducible procedure"
    );
    assert_eq!(a.scale, 1.0);
    assert_eq!(b.scale, 1.0);
    assert_eq!(c.scale, non_default_scale);
    assert_eq!(d.scale, non_default_scale);
    assert!(!a.nonessential && b.nonessential);
    assert!(!c.nonessential && d.nonessential);

    let nonessential_at_one = differing_paths(&a.json, &b.json);
    let nonessential_at_scale = differing_paths(&c.json, &d.json);
    assert!(
        !nonessential_at_one.is_empty(),
        "Nonessential on/off must produce observable export evidence"
    );
    let scale_without_nonessential = differing_paths(&a.json, &c.json);
    let scale_with_nonessential = differing_paths(&b.json, &d.json);
    assert!(
        !scale_without_nonessential.is_empty(),
        "the non-default Skeleton Reference scale must change exported data"
    );
    for (field, observed) in [
        ("nonessential_paths_at_scale_1", nonessential_at_one),
        (
            "nonessential_paths_at_non_default_scale",
            nonessential_at_scale,
        ),
        (
            "scale_paths_with_nonessential_off",
            scale_without_nonessential,
        ),
        ("scale_paths_with_nonessential_on", scale_with_nonessential),
    ] {
        assert_eq!(
            observed,
            required_string_set_object(probe, field),
            "scale probe's observed path set must be recorded exactly in `{field}`"
        );
    }
}

struct ScaleCase {
    json: Value,
    scale: f64,
    nonessential: bool,
}

fn scale_case(root: &Path, probe: &Map<String, Value>, name: &str) -> ScaleCase {
    let case = probe
        .get(name)
        .unwrap_or_else(|| panic!("scale_probe requires case `{name}`"));
    validate_case_evidence(root, case);
    let asset = load_nonfatal_case(root, case);
    assert!(
        asset
            .diagnostics()
            .iter()
            .all(|diagnostic| !diagnostic.is_degraded()),
        "scale probe `{name}` must remain loadable without degraded output"
    );
    ScaleCase {
        json: read_case_json(root, case),
        scale: required_f64(case, "reference_scale"),
        nonessential: required_bool(case, "nonessential"),
    }
}

fn differing_paths(left: &Value, right: &Value) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    collect_differences(left, right, "", &mut paths);
    paths
}

fn collect_differences(left: &Value, right: &Value, path: &str, out: &mut BTreeSet<String>) {
    match (left, right) {
        (Value::Object(left), Value::Object(right)) => {
            let keys = left.keys().chain(right.keys()).collect::<BTreeSet<_>>();
            for key in keys {
                let child = format!("{path}/{}", key.replace('~', "~0").replace('/', "~1"));
                match (left.get(key), right.get(key)) {
                    (Some(left), Some(right)) => collect_differences(left, right, &child, out),
                    _other => {
                        out.insert(child);
                    }
                }
            }
        }
        (Value::Array(left), Value::Array(right)) if left.len() == right.len() => {
            for (index, (left, right)) in left.iter().zip(right).enumerate() {
                collect_differences(left, right, &format!("{path}/{index}"), out);
            }
        }
        _other if left != right => {
            out.insert(if path.is_empty() {
                "/".to_owned()
            } else {
                path.to_owned()
            });
        }
        _other => {}
    }
}

fn observes_supported_feature(id: &str, asset: &SkeletonAsset, json: &Value, atlas: &str) -> bool {
    let animations = json.get("animations").and_then(Value::as_object);
    let constraints = json
        .get("constraints")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let ik = constraints
        .iter()
        .filter(|constraint| constraint.get("type").and_then(Value::as_str) == Some("ik"))
        .collect::<Vec<_>>();
    match id {
        "json-4-3-23" => asset.spine_version() == "4.3.23",
        "text-atlas" => asset.atlas_pages().len() > 0,
        "multi-page-atlas" => asset.atlas_pages().len() > 1,
        "straight-alpha-png" => asset
            .atlas_pages()
            .all(|page| page.alpha_encoding() == AlphaEncoding::Straight),
        "atlas-rgba8888-format" => asset
            .atlas_pages()
            .all(|page| page.format() == TextureFormat::Rgba8888),
        "atlas-linear-filter" => asset.atlas_pages().all(|page| {
            page.min_filter() == TextureFilter::Linear && page.mag_filter() == TextureFilter::Linear
        }),
        "atlas-clamp-wrap" => asset
            .atlas_pages()
            .all(|page| page.wrap() == WrapMode::CLAMP),
        "atlas-positive-scale" => {
            asset
                .atlas_pages()
                .all(|page| page.scale().is_finite() && page.scale() > 0.0)
                && atlas
                    .lines()
                    .any(|line| line.trim_start().starts_with("scale:"))
        }
        "atlas-packed-bounds" => {
            asset.atlas_regions().len() > 0
                && asset
                    .atlas_regions()
                    .all(|region| region.bounds().width() > 0 && region.bounds().height() > 0)
        }
        "atlas-indices" => asset.atlas_regions().any(|region| region.index().is_some()),
        "atlas-whitespace-trim-offsets" => asset
            .atlas_regions()
            .any(|region| region.trim().left() > 0 || region.trim().bottom() > 0),
        "atlas-original-size" => asset.atlas_regions().any(|region| {
            let packed = region.bounds().size();
            let original = region.trim().original_size();
            packed.width() != original.width() || packed.height() != original.height()
        }),
        "atlas-quarter-turn-rotation" => asset
            .atlas_regions()
            .any(|region| region.rotation().as_degrees() == 90.0),
        "normal-bone-inheritance" => {
            nonempty_array(json, "bones")
                && required_array(json, "bones").iter().all(|bone| {
                    bone.get("inherit")
                        .and_then(Value::as_str)
                        .is_none_or(|inherit| inherit == "normal")
                })
        }
        "rigid-region-attachment" => json_attachments(json).any(|attachment| {
            attachment
                .get("type")
                .and_then(Value::as_str)
                .is_none_or(|kind| kind == "region")
        }),
        "weighted-mesh-attachment" => {
            json_attachments(json).any(|attachment| {
                attachment.get("type").and_then(Value::as_str) == Some("mesh")
                    && array_len(attachment, "vertices") > array_len(attachment, "uvs")
            }) && asset
                .attachments()
                .filter_map(|attachment| attachment.as_mesh())
                .any(|mesh| mesh.is_weighted())
        }
        "unweighted-mesh-attachment" => {
            json_attachments(json).any(|attachment| {
                attachment.get("type").and_then(Value::as_str) == Some("mesh")
                    && array_len(attachment, "vertices") > 0
                    && array_len(attachment, "vertices") == array_len(attachment, "uvs")
            }) && asset
                .attachments()
                .filter_map(|attachment| attachment.as_mesh())
                .any(|mesh| !mesh.is_weighted())
        }
        "linked-mesh-attachment" => {
            has_attachment_type(json, "linkedmesh")
                && asset
                    .attachments()
                    .filter_map(|attachment| attachment.as_mesh())
                    .any(|mesh| mesh.source_mesh().is_some())
        }
        "setup-slots" => nonempty_array(json, "slots"),
        "setup-draw-order" => required_array(json, "slots").len() > 1,
        "attachment-switching" => slot_attachment_switches(json),
        "slot-attachment-timeline" => has_timeline(animations, "slots", "attachment"),
        "attachment-only-skins" => {
            json.get("skins")
                .and_then(Value::as_array)
                .is_some_and(|skins| {
                    skins.iter().any(|skin| {
                        skin.get("name").and_then(Value::as_str) != Some("default")
                            && skin
                                .get("attachments")
                                .and_then(Value::as_object)
                                .is_some_and(|attachments| !attachments.is_empty())
                    })
                })
        }
        "one-bone-ik" => ik
            .iter()
            .any(|constraint| array_len(constraint, "bones") == 1),
        "two-bone-ik" => ik
            .iter()
            .any(|constraint| array_len(constraint, "bones") == 2),
        "ik-target" => ik
            .iter()
            .any(|constraint| constraint.get("target").is_some()),
        "ik-order" => ik
            .iter()
            .any(|constraint| constraint.get("order").is_some()),
        "ik-setup-mix" => ik.iter().any(|constraint| constraint.get("mix").is_some()),
        "ik-setup-bend-direction" => {
            let values = ik
                .iter()
                .map(|constraint| {
                    constraint
                        .get("bendPositive")
                        .and_then(Value::as_bool)
                        .unwrap_or(true)
                })
                .collect::<BTreeSet<_>>();
            values == BTreeSet::from([false, true])
        }
        "transform-rotation-constraint" => constraints
            .iter()
            .any(is_supported_rotation_transform_constraint),
        "bone-rotate-timeline" => has_timeline(animations, "bones", "rotate"),
        "bone-translate-timeline" => has_timeline(animations, "bones", "translate"),
        "bone-scale-timeline" => has_timeline(animations, "bones", "scale"),
        "bone-shear-timeline" => has_timeline(animations, "bones", "shear"),
        "ik-mix-timeline" => ik_timeline_frames(json)
            .iter()
            .any(|frame| frame.get("mix").is_some()),
        "ik-bend-direction-timeline" => {
            ik_timeline_frames(json)
                .iter()
                .map(|frame| {
                    frame
                        .get("bendPositive")
                        .and_then(Value::as_bool)
                        .unwrap_or(true)
                })
                .collect::<BTreeSet<_>>()
                == BTreeSet::from([false, true])
        }
        "transform-mix-timeline" => transform_timeline_frames(json)
            .iter()
            .any(|frame| frame.get("mixRotate").is_some()),
        "linear-interpolation" => has_transition_curve(json, |curve| curve.is_none()),
        "stepped-interpolation" => has_transition_curve(json, |curve| {
            curve.and_then(Value::as_str) == Some("stepped")
        }),
        "bezier-interpolation" => {
            has_transition_curve(json, |curve| curve.is_some_and(Value::is_array))
        }
        "slot-colour-timeline" => has_timeline(animations, "slots", "rgba"),
        "draw-order-timeline" => has_animation_section(animations, "drawOrder"),
        "events" => has_animation_section(animations, "events"),
        _other => false,
    }
}

fn slot_attachment_switches(json: &Value) -> bool {
    if let Some(animations) = json.get("animations").and_then(Value::as_object) {
        for animation in animations.values() {
            if let Some(slots) = animation.get("slots").and_then(Value::as_object) {
                for slot in slots.values() {
                    if let Some(frames) = slot.get("attachment").and_then(Value::as_array) {
                        let mut names = BTreeSet::new();
                        for frame in frames {
                            names.insert(
                                frame
                                    .get("name")
                                    .and_then(Value::as_str)
                                    .map(ToOwned::to_owned),
                            );
                        }
                        if names.len() > 1 {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

fn ik_timeline_frames(json: &Value) -> Vec<&Map<String, Value>> {
    let mut frames = Vec::new();
    if let Some(animations) = json.get("animations").and_then(Value::as_object) {
        for animation in animations.values() {
            if let Some(constraints) = animation.get("ik").and_then(Value::as_object) {
                for timeline in constraints.values().filter_map(Value::as_array) {
                    frames.extend(timeline.iter().filter_map(Value::as_object));
                }
            }
        }
    }
    frames
}

fn transform_timeline_frames(json: &Value) -> Vec<&Map<String, Value>> {
    let mut frames = Vec::new();
    if let Some(animations) = json.get("animations").and_then(Value::as_object) {
        for animation in animations.values() {
            if let Some(constraints) = animation.get("transform").and_then(Value::as_object) {
                for timeline in constraints.values().filter_map(Value::as_array) {
                    frames.extend(timeline.iter().filter_map(Value::as_object));
                }
            }
        }
    }
    frames
}

fn is_supported_rotation_transform_constraint(value: &Value) -> bool {
    let Some(constraint) = value.as_object() else {
        return false;
    };
    if constraint.get("type").and_then(Value::as_str) != Some("transform")
        || constraint.get("source").and_then(Value::as_str).is_none()
        || array_len(value, "bones") == 0
        || ["localSource", "localTarget", "additive", "clamp"]
            .iter()
            .any(|field| constraint.get(*field).and_then(Value::as_bool) == Some(true))
    {
        return false;
    }
    constraint
        .get("properties")
        .and_then(Value::as_object)
        .and_then(|properties| properties.get("rotate"))
        .and_then(|source| source.get("to"))
        .and_then(Value::as_object)
        .is_some_and(|destinations| destinations.contains_key("rotate"))
}

fn has_unsupported_transform_constraint_option(value: &Value) -> bool {
    let Some(constraint) = value.as_object() else {
        return false;
    };
    if constraint.get("type").and_then(Value::as_str) != Some("transform") {
        return false;
    }
    if ["localSource", "localTarget", "additive", "clamp"]
        .iter()
        .any(|field| constraint.get(*field).and_then(Value::as_bool) == Some(true))
    {
        return true;
    }
    constraint
        .get("properties")
        .and_then(Value::as_object)
        .is_some_and(|properties| {
            properties.iter().any(|(source_name, source)| {
                source
                    .get("to")
                    .and_then(Value::as_object)
                    .is_some_and(|destinations| {
                        destinations.iter().any(|(target_name, settings)| {
                            source_name != "rotate"
                                || target_name != "rotate"
                                || settings.as_object().is_some_and(|settings| {
                                    settings.keys().any(|setting| setting != "max")
                                })
                        })
                    })
            })
        })
}

fn has_transition_curve(json: &Value, predicate: impl Fn(Option<&Value>) -> bool + Copy) -> bool {
    fn visit(value: &Value, predicate: impl Fn(Option<&Value>) -> bool + Copy) -> bool {
        match value {
            Value::Array(values) => {
                (values.len() > 1
                    && values[..values.len() - 1].iter().any(|frame| {
                        frame.as_object().is_some_and(|frame| {
                            ["value", "x", "y", "rgba", "mix"]
                                .iter()
                                .any(|key| frame.contains_key(*key))
                                && predicate(frame.get("curve"))
                        })
                    }))
                    || values.iter().any(|value| visit(value, predicate))
            }
            Value::Object(object) => object.values().any(|value| visit(value, predicate)),
            _other => false,
        }
    }
    json.get("animations")
        .is_some_and(|animations| visit(animations, predicate))
}

fn has_animation_section(animations: Option<&Map<String, Value>>, section: &str) -> bool {
    animations.is_some_and(|animations| {
        animations.values().any(|animation| {
            animation.get(section).is_some_and(|value| {
                value.as_object().is_some_and(|value| !value.is_empty())
                    || value.as_array().is_some_and(|value| !value.is_empty())
            })
        })
    })
}

fn has_timeline(animations: Option<&Map<String, Value>>, section: &str, timeline: &str) -> bool {
    animations.is_some_and(|animations| {
        animations.values().any(|animation| {
            animation
                .get(section)
                .and_then(Value::as_object)
                .is_some_and(|targets| {
                    targets.values().any(|target| {
                        target
                            .get(timeline)
                            .and_then(Value::as_array)
                            .is_some_and(|frames| !frames.is_empty())
                    })
                })
        })
    })
}

fn json_attachments(json: &Value) -> impl Iterator<Item = &Value> {
    let mut attachments = Vec::new();
    if let Some(skins) = json.get("skins").and_then(Value::as_array) {
        for skin in skins {
            if let Some(slots) = skin.get("attachments").and_then(Value::as_object) {
                for slot in slots.values().filter_map(Value::as_object) {
                    attachments.extend(slot.values());
                }
            }
        }
    }
    attachments.into_iter()
}

fn exercise_every_animation(asset: Arc<SkeletonAsset>, fixture_name: &str) {
    for animation in asset.animations() {
        let mut skeleton = Skeleton::new(Arc::clone(&asset));
        for position in [
            Duration::ZERO,
            animation.duration() / 2,
            animation.duration(),
        ] {
            skeleton
                .sample_animation(animation.id(), position, PlaybackMode::Once)
                .unwrap_or_else(|error| {
                    panic!(
                        "{fixture_name} animation `{}` failed at {position:?}: {error}",
                        animation.name()
                    )
                });
            let frame = skeleton.editable_pose().solve();
            assert!(
                frame.bones().all(|bone| {
                    let world = bone.world_transform();
                    world.translation().is_finite()
                        && world.x_axis().is_finite()
                        && world.y_axis().is_finite()
                }),
                "{fixture_name} animation `{}` produced a non-finite pose",
                animation.name()
            );
        }
        let mut skeleton = Skeleton::new(Arc::clone(&asset));
        let mut player = AnimationPlayer::new(&skeleton);
        player
            .play(animation.id(), PlayOptions::once())
            .expect("the animation belongs to the project fixture player");
        let frame = player
            .update(&mut skeleton, animation.duration(), &mut ())
            .unwrap_or_else(|error| {
                panic!(
                    "{fixture_name} animation `{}` failed in the stateful player: {error}",
                    animation.name()
                )
            })
            .solve();
        assert!(
            frame.bones().all(|bone| {
                let world = bone.world_transform();
                world.translation().is_finite()
                    && world.x_axis().is_finite()
                    && world.y_axis().is_finite()
            }),
            "{fixture_name} animation `{}` produced a non-finite player pose",
            animation.name()
        );
    }
}

fn diagnostic_signature(diagnostic: &Diagnostic) -> String {
    format!(
        "{}:{}:{}",
        severity_name(diagnostic.severity()),
        diagnostic_code_name(diagnostic.code()),
        diagnostic_scope_name(diagnostic.scope())
    )
}

fn tripwire_expectation(id: &str) -> Option<String> {
    let signature = match id {
        "clipping-attachment" | "attachment-sequence" => {
            "degraded:unsupported-attachment-type:attachment"
        }
        "bounding-box-attachment" | "point-attachment" => {
            "warning:unsupported-attachment-type:attachment"
        }
        "deform-timeline"
        | "ik-softness-timeline"
        | "ik-compress-timeline"
        | "ik-stretch-timeline" => "degraded:unsupported-timeline-type:animation",
        "path-constraint" | "physics-constraint" => {
            "degraded:unsupported-constraint-type:constraint"
        }
        "unsupported-transform-constraint-option" => {
            "degraded:unsupported-constraint-option:constraint"
        }
        "skin-specific-bones" => "degraded:ignored-skin-bones:skin",
        "skin-specific-constraints" => "degraded:ignored-skin-constraints:skin",
        "two-colour-tint" => "degraded:unsupported-two-colour-tint:slot",
        "non-normal-blend-mode" => "degraded:unsupported-blend-mode:slot",
        "non-normal-bone-inheritance" => "degraded:unsupported-bone-transform-mode:bone",
        "ik-softness-setup"
        | "ik-compress-option"
        | "ik-stretch-option"
        | "ik-uniform-scaling-option" => "degraded:unsupported-constraint-option:ik-constraint",
        "premultiplied-alpha" => "degraded:alpha-encoding-mismatch:atlas-page",
        "non-quarter-atlas-rotation" => "degraded:unsupported-atlas-rotation:atlas-region",
        "unknown-atlas-page-setting" => "degraded:unsupported-atlas-setting:atlas-page",
        "binary-skeleton" => return None,
        _other => return None,
    };
    Some(signature.to_owned())
}

fn severity_name(severity: DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::Warning => "warning",
        DiagnosticSeverity::Degraded => "degraded",
        _future => "future",
    }
}

fn diagnostic_scope_name(scope: DiagnosticScope) -> &'static str {
    match scope {
        DiagnosticScope::Asset => "asset",
        DiagnosticScope::Bone(_bone) => "bone",
        DiagnosticScope::Slot(_slot) => "slot",
        DiagnosticScope::Skin(_skin) => "skin",
        DiagnosticScope::Animation(_animation) => "animation",
        DiagnosticScope::Event(_event) => "event",
        DiagnosticScope::Attachment(_attachment) => "attachment",
        DiagnosticScope::IkConstraint(_constraint) => "ik-constraint",
        DiagnosticScope::Constraint(_constraint) => "constraint",
        DiagnosticScope::AtlasPage(_page) => "atlas-page",
        DiagnosticScope::AtlasRegion(_region) => "atlas-region",
        _future => "future",
    }
}

fn diagnostic_code_name(code: DiagnosticCode) -> &'static str {
    match code {
        DiagnosticCode::UnsupportedAttachmentType => "unsupported-attachment-type",
        DiagnosticCode::UnsupportedConstraintType => "unsupported-constraint-type",
        DiagnosticCode::UnsupportedConstraintOption => "unsupported-constraint-option",
        DiagnosticCode::UnsupportedBoneTransformMode => "unsupported-bone-transform-mode",
        DiagnosticCode::UnsupportedTimelineType => "unsupported-timeline-type",
        DiagnosticCode::UnsupportedBlendMode => "unsupported-blend-mode",
        DiagnosticCode::UnsupportedTwoColourTint => "unsupported-two-colour-tint",
        DiagnosticCode::IgnoredSkinBones => "ignored-skin-bones",
        DiagnosticCode::IgnoredSkinConstraints => "ignored-skin-constraints",
        DiagnosticCode::UnknownField => "unknown-field",
        DiagnosticCode::UntestedPatchVersion => "untested-patch-version",
        DiagnosticCode::AlphaEncodingMismatch => "alpha-encoding-mismatch",
        DiagnosticCode::UnsupportedAtlasSetting => "unsupported-atlas-setting",
        DiagnosticCode::UnsupportedAtlasRotation => "unsupported-atlas-rotation",
        DiagnosticCode::DiagnosticsTruncated => "diagnostics-truncated",
        _future => "future",
    }
}

fn load_error_name(kind: LoadErrorKind) -> &'static str {
    match kind {
        LoadErrorKind::InvalidUtf8 => "invalid-utf8",
        LoadErrorKind::Syntax => "syntax",
        LoadErrorKind::SchemaViolation => "schema-violation",
        LoadErrorKind::InvalidVersion => "invalid-version",
        LoadErrorKind::UnsupportedVersion => "unsupported-version",
        LoadErrorKind::NonFiniteNumber => "non-finite-number",
        LoadErrorKind::DuplicateField => "duplicate-field",
        LoadErrorKind::DuplicateName => "duplicate-name",
        LoadErrorKind::InvalidOrder => "invalid-order",
        LoadErrorKind::InvalidTopology => "invalid-topology",
        LoadErrorKind::UnresolvedReference => "unresolved-reference",
        LoadErrorKind::MissingAtlasRegion => "missing-atlas-region",
        LoadErrorKind::AmbiguousAtlasRegion => "ambiguous-atlas-region",
        LoadErrorKind::UnsupportedData => "unsupported-data",
        LoadErrorKind::CapacityExceeded => "capacity-exceeded",
        _future => "future",
    }
}

fn read_json(path: &Path) -> Value {
    let bytes =
        fs::read(path).unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()))
}

fn checked_file(root: &Path, relative: &str) -> PathBuf {
    let relative = Path::new(relative);
    assert!(
        !relative.is_absolute(),
        "fixture paths must be relative: {}",
        relative.display()
    );
    let path = fs::canonicalize(root.join(relative)).unwrap_or_else(|error| {
        panic!(
            "failed to resolve fixture path {}: {error}",
            relative.display()
        )
    });
    assert!(
        path.starts_with(root),
        "fixture path escapes the intake root: {}",
        relative.display()
    );
    assert!(
        path.is_file(),
        "fixture path is not a file: {}",
        path.display()
    );
    path
}

fn required_str<'a>(value: &'a Value, key: &str) -> &'a str {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("manifest field `{key}` must be a string"))
}

fn required_array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_else(|| panic!("manifest field `{key}` must be an array"))
}

fn required_object<'a>(value: &'a Value, key: &str) -> &'a Map<String, Value> {
    value
        .get(key)
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("manifest field `{key}` must be an object"))
}

fn required_u64(value: &Value, key: &str) -> u64 {
    value
        .get(key)
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("manifest field `{key}` must be an unsigned integer"))
}

fn required_f64(value: &Value, key: &str) -> f64 {
    value
        .get(key)
        .and_then(Value::as_f64)
        .unwrap_or_else(|| panic!("manifest field `{key}` must be a number"))
}

fn required_f64_object(value: &Map<String, Value>, key: &str) -> f64 {
    value
        .get(key)
        .and_then(Value::as_f64)
        .unwrap_or_else(|| panic!("manifest field `scale_probe.{key}` must be a number"))
}

fn required_string_set_object(value: &Map<String, Value>, key: &str) -> BTreeSet<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("manifest field `scale_probe.{key}` must be an array"))
        .iter()
        .map(|value| {
            value
                .as_str()
                .unwrap_or_else(|| {
                    panic!("manifest field `scale_probe.{key}` must contain strings")
                })
                .to_owned()
        })
        .collect()
}

fn required_bool(value: &Value, key: &str) -> bool {
    value
        .get(key)
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("manifest field `{key}` must be a boolean"))
}

fn nonempty_array(value: &Value, key: &str) -> bool {
    value
        .get(key)
        .and_then(Value::as_array)
        .is_some_and(|array| !array.is_empty())
}

fn array_len(value: &Value, key: &str) -> usize {
    value.get(key).and_then(Value::as_array).map_or(0, Vec::len)
}

fn case_label(case: &Value) -> &str {
    case.get("id")
        .or_else(|| case.get("coverage_id"))
        .and_then(Value::as_str)
        .unwrap_or("<unnamed>")
}

trait ResultExt<T, E> {
    fn unwrap_err_or_else(self, loaded: impl FnOnce() -> E) -> E;
}

impl<T, E> ResultExt<T, E> for Result<T, E> {
    fn unwrap_err_or_else(self, loaded: impl FnOnce() -> E) -> E {
        match self {
            Ok(_value) => loaded(),
            Err(error) => error,
        }
    }
}

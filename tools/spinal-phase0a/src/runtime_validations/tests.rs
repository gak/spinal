use super::*;
use crate::{
    case::{LoadedCase, parse_case},
    package::{CasePackageInventories, EntryKind, PackageInventory, TreeEntry},
    phase0_analysis::{Phase0JsonSources, analyze_phase0},
};
use serde_json::{Map, Value, json};

const ATLAS: &[u8] =
    b"page.png\nsize: 1, 1\nformat: RGBA8888\nfilter: Linear, Linear\nrepeat: none\npma: false\n";
const BLUE_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 96, 96, 248, 255, 31, 0, 3,
    2, 1, 255, 230, 119, 11, 174, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];
const RED_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240, 31, 0,
    5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

struct Fixture {
    analysis: CompletedPhase0Analysis,
    current: Vec<u8>,
    existing: Vec<u8>,
    new: Vec<u8>,
}

impl Fixture {
    fn valid() -> Self {
        let case = test_case();
        let current = project(
            "current-hash",
            [("idle", animation(0)), ("walk", animation(1))],
        );
        let replacement = project(
            "replacement-hash",
            [("idle", animation(10)), ("walk", animation(1))],
        );
        let new_submission = project(
            "new-submission-hash",
            [
                ("gesture", animation(20)),
                ("idle", animation(0)),
                ("walk", animation(1)),
            ],
        );
        let mut reconstructed = current.clone();
        reconstructed["skeleton"]["hash"] = json!("reconstructed-hash");
        let mut existing = current.clone();
        existing["skeleton"]["hash"] = json!("existing-hash");
        existing["animations"]["idle"] = animation(10);
        let mut new = current.clone();
        new["skeleton"]["hash"] = json!("new-candidate-hash");
        new["animations"]["gesture"] = animation(20);
        let mut collision = new_submission.clone();
        collision["skeleton"]["hash"] = json!("new-collision-control-hash");
        collision["animations"]["gesture2"] = animation(20);

        let current = json_bytes(&current);
        let reconstructed = json_bytes(&reconstructed);
        let existing = json_bytes(&existing);
        let new = json_bytes(&new);
        let sources = Phase0JsonSources {
            current_a: current.clone(),
            replacement_submission: json_bytes(&replacement),
            new_submission: json_bytes(&new_submission),
            reconstructed_a: reconstructed.clone(),
            current_b: current.clone(),
            reconstructed_b: reconstructed,
            existing_first: existing.clone(),
            existing_repeat: existing.clone(),
            new_first: new.clone(),
            new_collision_control: json_bytes(&collision),
            new_animation_collision: crate::process::NewAnimationCollisionEvidence::for_test(
                "gesture", "gesture2",
            ),
        };
        let inventories = CasePackageInventories {
            current: inventory("character.spine", "current-project"),
            replacement_submission: inventory("replacement.spine", "replacement-project"),
            new_submission: inventory("new.spine", "new-project"),
        };
        let analysis = analyze_phase0(&case, sources, &inventories).expect("valid analysis");
        Self {
            analysis,
            current,
            existing,
            new,
        }
    }

    fn targets(&self) -> (ValidatedTarget, ValidatedTarget, ValidatedTarget) {
        (
            bundle(RuntimeValidationRole::CurrentB, &self.current, BLUE_PNG),
            bundle(
                RuntimeValidationRole::ExistingRepeat,
                &self.existing,
                BLUE_PNG,
            ),
            bundle(RuntimeValidationRole::NewFirst, &self.new, BLUE_PNG),
        )
    }
}

fn test_case() -> LoadedCase {
    parse_case(
        r#"
format_version = 2
case_id = "runtime-validations"
target_spine_version = "4.3.23"
runtime_atlas = "character.atlas"

[editor]
expected_executable_sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[packages.current]
root = "/external/current"
project = "character.spine"
required_directories = ["images"]
asset_roots = ["images"]

[packages.replacement_submission]
root = "/external/replacement"
project = "replacement.spine"
required_directories = ["images"]
asset_roots = ["images"]

[packages.new_submission]
root = "/external/new"
project = "new.spine"
required_directories = ["images"]
asset_roots = ["images"]

[skeletons]
current = "Character"
replacement_submission = "Character"
new_submission = "Character"

[animations]
replacement = "idle"
new = "gesture"

[export]
preset = "pretty-nonessential-json"

[volatile]
approved_json_pointers = ["/skeleton/hash"]
"#,
    )
    .expect("valid test case")
}

fn project<const N: usize>(hash: &str, values: [(&str, Value); N]) -> Value {
    let animations = values
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect::<Map<_, _>>();
    json!({
        "skeleton": {"hash": hash, "spine": "4.3.23", "x": 0, "y": 0},
        "bones": [{"name": "root"}],
        "slots": [{"name": "body", "bone": "root"}],
        "skins": [{"name": "default", "attachments": {}}],
        "animations": animations,
    })
}

fn animation(value: i64) -> Value {
    json!({"bones": {"root": {"rotate": [{"time": 0, "value": value}]}}})
}

fn json_bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec_pretty(value).expect("serialize fixture JSON")
}

fn inventory(project: &str, project_bytes: &str) -> PackageInventory {
    let mut entries = vec![
        TreeEntry {
            path: ".".to_owned(),
            kind: EntryKind::Directory,
            size: 0,
            sha256: None,
        },
        TreeEntry {
            path: "images".to_owned(),
            kind: EntryKind::Directory,
            size: 0,
            sha256: None,
        },
        TreeEntry {
            path: "images/cat.png".to_owned(),
            kind: EntryKind::File,
            size: 7,
            sha256: Some(sha256_bytes(b"texture")),
        },
        TreeEntry {
            path: project.to_owned(),
            kind: EntryKind::File,
            size: u64::try_from(project_bytes.len()).expect("test length fits u64"),
            sha256: Some(sha256_bytes(project_bytes.as_bytes())),
        },
    ];
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    PackageInventory {
        tree_sha256: sha256_bytes(format!("tree:{project}:{project_bytes}").as_bytes()),
        entries,
    }
}

fn bundle(role: RuntimeValidationRole, json: &[u8], page: &[u8]) -> ValidatedTarget {
    let json_path = PathBuf::from("review/rig.json");
    let atlas_path = PathBuf::from("review/character.atlas");
    let files = BTreeMap::from([
        (json_path.clone(), json.to_vec()),
        (atlas_path.clone(), ATLAS.to_vec()),
        (PathBuf::from("review/page.png"), page.to_vec()),
    ]);
    let (_manifest, bundle) =
        RuntimeBundleManifest::build(role.manifest_label(), &json_path, &atlas_path, files)
            .expect("valid runtime bundle");
    ValidatedTarget {
        bundle,
        bindings: Vec::new(),
    }
}

fn require_failure(
    result: Result<CompletedRuntimeValidations, RuntimeValidationsError>,
) -> RuntimeValidationsError {
    match result {
        Ok(_completed) => panic!("runtime validation unexpectedly succeeded"),
        Err(error) => error,
    }
}

#[test]
fn completed_gate_is_deterministic_and_records_fixed_file_identities() {
    let fixture = Fixture::valid();
    let (current, existing, new) = fixture.targets();
    let completed =
        bind_validated_targets(&fixture.analysis, current, existing, new).expect("valid runtimes");

    let first = completed.artifact_bytes().expect("artifact bytes");
    let second = completed.artifact_bytes().expect("artifact bytes");
    assert_eq!(first, second);
    serde_json::to_vec(completed.artifact_view()).expect("serializable artifact view");

    let text = std::str::from_utf8(&first).expect("artifact UTF-8");
    assert!(text.contains("\"role\": \"current_b\""));
    assert!(text.contains("\"role\": \"existing_repeat\""));
    assert!(text.contains("\"role\": \"new_first\""));
    assert!(text.contains("\"manifest_sha256\""));
    assert!(text.contains("\"byte_length\""));
    assert!(!text.contains("\"passed\""));
}

#[test]
fn swapped_runtime_roles_cannot_bind_to_analysis() {
    let fixture = Fixture::valid();
    let current = bundle(RuntimeValidationRole::CurrentB, &fixture.existing, BLUE_PNG);
    let existing = bundle(
        RuntimeValidationRole::ExistingRepeat,
        &fixture.current,
        BLUE_PNG,
    );
    let new = bundle(RuntimeValidationRole::NewFirst, &fixture.new, BLUE_PNG);

    let error = require_failure(bind_validated_targets(
        &fixture.analysis,
        current,
        existing,
        new,
    ));
    assert!(matches!(
        error,
        RuntimeValidationsError::JsonIdentityMismatch {
            role: RuntimeValidationRole::CurrentB
        }
    ));
}

#[test]
fn semantically_equivalent_but_byte_modified_json_is_rejected() {
    let fixture = Fixture::valid();
    let mut modified = fixture.current.clone();
    modified.push(b'\n');
    let current = bundle(RuntimeValidationRole::CurrentB, &modified, BLUE_PNG);
    let existing = bundle(
        RuntimeValidationRole::ExistingRepeat,
        &fixture.existing,
        BLUE_PNG,
    );
    let new = bundle(RuntimeValidationRole::NewFirst, &fixture.new, BLUE_PNG);

    let error = require_failure(bind_validated_targets(
        &fixture.analysis,
        current,
        existing,
        new,
    ));
    assert!(matches!(
        error,
        RuntimeValidationsError::JsonIdentityMismatch {
            role: RuntimeValidationRole::CurrentB
        }
    ));
}

#[test]
fn atlas_page_identity_mismatch_is_rejected() {
    let fixture = Fixture::valid();
    let current = bundle(RuntimeValidationRole::CurrentB, &fixture.current, BLUE_PNG);
    let existing = bundle(
        RuntimeValidationRole::ExistingRepeat,
        &fixture.existing,
        BLUE_PNG,
    );
    let new = bundle(RuntimeValidationRole::NewFirst, &fixture.new, RED_PNG);

    let error = require_failure(bind_validated_targets(
        &fixture.analysis,
        current,
        existing,
        new,
    ));
    assert!(matches!(
        error,
        RuntimeValidationsError::AssetIdentityMismatch {
            role: RuntimeValidationRole::NewFirst
        }
    ));
}

#[test]
fn degraded_native_runtime_is_rejected_before_analysis_binding() {
    let fixture = Fixture::valid();
    let mut degraded: Value =
        serde_json::from_slice(&fixture.current).expect("fixture JSON parses");
    degraded["bones"][0]["transform"] = json!("onlyTranslation");
    let current = bundle(
        RuntimeValidationRole::CurrentB,
        &json_bytes(&degraded),
        BLUE_PNG,
    );
    let existing = bundle(
        RuntimeValidationRole::ExistingRepeat,
        &fixture.existing,
        BLUE_PNG,
    );
    let new = bundle(RuntimeValidationRole::NewFirst, &fixture.new, BLUE_PNG);

    let error = require_failure(bind_validated_targets(
        &fixture.analysis,
        current,
        existing,
        new,
    ));
    assert!(matches!(
        error,
        RuntimeValidationsError::Native {
            role: RuntimeValidationRole::CurrentB,
            source: NativeValidationError::Degraded
        }
    ));
}

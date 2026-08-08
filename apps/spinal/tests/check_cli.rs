//! Process-boundary acceptance tests for the headless checker.

use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde_json::Value;
use tempfile::TempDir;

const PIXEL_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240, 31, 0,
    5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];
const ATLAS: &[u8] = br#"rig.png
	size: 1, 1
	format: RGBA8888
	filter: Linear, Linear
	repeat: none
	pma: false
shape
	bounds: 0, 0, 1, 1
"#;
const COMPATIBLE_V1_GOLDEN: &[u8] = include_bytes!("goldens/check-compatible-v1.json");
const DEGRADED_V1_GOLDEN: &[u8] = include_bytes!("goldens/check-degraded-v1.json");

fn prepare(directory: &Path, degraded: bool) -> PathBuf {
    let extra_attachment = if degraded {
        r#", "unsupported": {
          "type": "clipping",
          "vertexCount": 3,
          "vertices": [0, 0, 12, 0, 12, 12]
        }"#
    } else {
        ""
    };
    let json = format!(
        r#"{{
  "skeleton": {{"spine": "4.3.23"}},
  "bones": [{{"name": "root"}}],
  "slots": [{{"name": "body", "bone": "root", "attachment": "shape"}}],
  "skins": [{{
    "name": "default",
    "attachments": {{
      "body": {{
        "shape": {{"width": 1, "height": 1}}{extra_attachment}
      }}
    }}
  }}],
  "animations": {{
    "idle": {{
      "bones": {{
        "root": {{"rotate": [{{}}, {{"time": 1, "value": 5}}]}}
      }}
    }}
  }}
}}"#
    );
    fs::create_dir_all(directory).expect("create fixture directory");
    let json_path = directory.join("rig.spine.json");
    fs::write(&json_path, json).expect("write JSON fixture");
    fs::write(directory.join("rig.atlas"), ATLAS).expect("write atlas fixture");
    fs::write(directory.join("rig.png"), PIXEL_PNG).expect("write PNG fixture");
    json_path
}

fn spinal(arguments: &[&OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_spinal"))
        .args(arguments)
        .output()
        .expect("run the Spinal binary")
}

fn snapshot(directory: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fs::read_dir(directory)
        .expect("read fixture directory")
        .map(|entry| {
            let path = entry.expect("read fixture entry").path();
            let name = path
                .file_name()
                .expect("fixture entry has a filename")
                .into();
            (name, fs::read(path).expect("read fixture entry"))
        })
        .collect()
}

fn json_stdout(output: &Output) -> Value {
    assert!(output.stderr.is_empty(), "unexpected stderr: {output:?}");
    serde_json::from_slice(&output.stdout).expect("stdout is one JSON document")
}

#[test]
fn compatible_json_is_repeatable_host_independent_and_read_only() {
    let first = TempDir::new().expect("first temporary root");
    let second = TempDir::new().expect("second temporary root");
    let first_json = prepare(first.path(), false);
    let second_json = prepare(second.path(), false);
    let before = snapshot(first.path());

    let first_output = spinal(&[
        OsStr::new("check"),
        first_json.as_os_str(),
        OsStr::new("--json"),
    ]);
    let repeated = spinal(&[
        OsStr::new("check"),
        first_json.as_os_str(),
        OsStr::new("--json"),
    ]);
    let relocated = spinal(&[
        OsStr::new("check"),
        second_json.as_os_str(),
        OsStr::new("--json"),
    ]);

    assert_eq!(first_output.status.code(), Some(0));
    assert_eq!(repeated.status.code(), Some(0));
    assert_eq!(relocated.status.code(), Some(0));
    assert_eq!(first_output.stdout, repeated.stdout);
    assert_eq!(first_output.stdout, relocated.stdout);
    assert_eq!(first_output.stdout, COMPATIBLE_V1_GOLDEN);
    assert_eq!(snapshot(first.path()), before);

    let value = json_stdout(&first_output);
    assert_eq!(value["format_version"], 1);
    assert_eq!(value["status"], "compatible");
    assert_eq!(value["source"]["json_path"], "rig.spine.json");
    assert_eq!(value["source"]["atlas_path"], "rig.atlas");
    assert_eq!(value["inventory"]["animations"][0]["name"], "idle");
    assert!(
        !String::from_utf8_lossy(&first_output.stdout)
            .contains(first.path().to_string_lossy().as_ref())
    );
}

#[test]
fn degraded_usage_and_missing_source_have_distinct_stable_exits() {
    let directory = TempDir::new().expect("temporary root");
    let degraded_json = prepare(directory.path(), true);

    let degraded = spinal(&[
        OsStr::new("check"),
        degraded_json.as_os_str(),
        OsStr::new("--json"),
    ]);
    assert_eq!(degraded.status.code(), Some(1));
    assert_eq!(degraded.stdout, DEGRADED_V1_GOLDEN);
    let degraded_value = json_stdout(&degraded);
    assert_eq!(degraded_value["status"], "degraded");
    assert_eq!(
        degraded_value["diagnostics"][0]["code"],
        "unsupported_attachment_type"
    );

    let usage = spinal(&[
        OsStr::new("check"),
        OsStr::new("--fps=24"),
        degraded_json.as_os_str(),
    ]);
    assert_eq!(usage.status.code(), Some(2));
    assert!(usage.stdout.is_empty());
    assert!(String::from_utf8_lossy(&usage.stderr).contains("unknown option"));

    let missing_path = directory.path().join("missing.json");
    let missing = spinal(&[
        OsStr::new("check"),
        missing_path.as_os_str(),
        OsStr::new("--json"),
    ]);
    assert_eq!(missing.status.code(), Some(3));
    assert_eq!(
        json_stdout(&missing),
        serde_json::json!({
            "format_version": 1,
            "status": "error",
            "error": {
                "code": "source_unavailable",
                "message": "one or more source files could not be read"
            }
        })
    );
}

#[test]
fn check_help_returns_without_launching_a_window() {
    let output = spinal(&[OsStr::new("check"), OsStr::new("--help")]);

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("Spinal — Check"));
}

#[test]
fn human_report_uses_readable_numbering_units_and_diagnostics() {
    let directory = TempDir::new().expect("temporary root");
    let json = prepare(directory.path(), true);

    let output = spinal(&[OsStr::new("check"), json.as_os_str()]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).expect("human report is UTF-8");
    assert!(text.contains("Inventory: 1 bone, 1 slot, 1 skin, 2 attachments, 1 animation"));
    assert!(text.contains("  1. \"idle\" — 1 s"));
    assert!(text.contains("degraded/unsupported_attachment_type at attachment \"unsupported\""));
    assert!(!text.contains("\"severity\":"));
    assert!(!text.contains(" — 1000000000 ns"));
}

#[test]
fn rejected_sources_report_actionable_path_free_codes() {
    let wrong_version_root = TempDir::new().expect("wrong-version root");
    let wrong_version_json = prepare(wrong_version_root.path(), false);
    let json = fs::read_to_string(&wrong_version_json).expect("read JSON fixture");
    fs::write(&wrong_version_json, json.replace("4.3.23", "4.3.24"))
        .expect("write wrong-version fixture");

    let wrong_version = spinal(&[
        OsStr::new("check"),
        wrong_version_json.as_os_str(),
        OsStr::new("--json"),
    ]);
    assert_eq!(wrong_version.status.code(), Some(3));
    let wrong_version_value = json_stdout(&wrong_version);
    assert_eq!(
        wrong_version_value["error"],
        serde_json::json!({
            "code": "spine_version_mismatch",
            "message": "the export does not target Spinal's required Spine editor version",
            "expected": "4.3.23",
            "actual": "4.3.24"
        })
    );

    let unsupported_version_root = TempDir::new().expect("unsupported-version root");
    let unsupported_version_json = prepare(unsupported_version_root.path(), false);
    let json = fs::read_to_string(&unsupported_version_json).expect("read JSON fixture");
    fs::write(&unsupported_version_json, json.replace("4.3.23", "4.2.0"))
        .expect("write unsupported-version fixture");
    let unsupported_version = spinal(&[
        OsStr::new("check"),
        unsupported_version_json.as_os_str(),
        OsStr::new("--json"),
    ]);
    assert_eq!(unsupported_version.status.code(), Some(3));
    let unsupported_version_value = json_stdout(&unsupported_version);
    assert_eq!(unsupported_version_value["error"]["code"], "export_invalid");
    assert_eq!(
        unsupported_version_value["error"]["reason"],
        "unsupported_version"
    );

    let corrupt_texture_root = TempDir::new().expect("corrupt-texture root");
    let corrupt_texture_json = prepare(corrupt_texture_root.path(), false);
    fs::write(corrupt_texture_root.path().join("rig.png"), b"not a PNG")
        .expect("write corrupt texture fixture");
    let corrupt_texture = spinal(&[
        OsStr::new("check"),
        corrupt_texture_json.as_os_str(),
        OsStr::new("--json"),
    ]);
    assert_eq!(corrupt_texture.status.code(), Some(3));
    assert_eq!(
        json_stdout(&corrupt_texture)["error"]["code"],
        "texture_invalid"
    );

    for output in [&wrong_version, &unsupported_version, &corrupt_texture] {
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(!stdout.contains(wrong_version_root.path().to_string_lossy().as_ref()));
        assert!(!stdout.contains(unsupported_version_root.path().to_string_lossy().as_ref()));
        assert!(!stdout.contains(corrupt_texture_root.path().to_string_lossy().as_ref()));
    }
}

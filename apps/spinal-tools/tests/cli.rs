//! End-to-end contracts for the installed `spinal` command.

use std::{
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

use serde_json::Value;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

const COSMETIC_SKINS: &[&str] = &[
    "item/hat_red_beret",
    "item/hat_flower_crown",
    "item/hat_straw_sunhat",
    "item/collar_red",
    "item/collar_bell",
    "item/collar_founder",
    "item/glasses_round",
    "item/glasses_heart",
    "item/glasses_star",
];

const REQUIRED_ANIMATIONS: &[&str] = &["walk", "jump", "eat", "sit", "sleep", "loaf", "falling"];

#[test]
fn loafstead_demo_profile_passes_a_complete_export() {
    let fixture = Fixture::complete("complete export");
    let output = run(&[
        "check",
        "--profile",
        "loafstead-demo",
        fixture.json.to_str().expect("UTF-8 fixture path"),
    ]);

    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    assert!(stdout.contains("PASS  loafstead-demo"), "{stdout}");
    assert!(stdout.contains("7/7 required animations"), "{stdout}");
    assert!(stdout.contains("3 hats, 3 collars, 3 glasses"), "{stdout}");
}

#[test]
fn cosmetics_that_replace_one_another_fail_the_demo_profile() {
    let fixture = Fixture::colliding_cosmetics("colliding cosmetics");
    let output = run(&[
        "check",
        "--profile",
        "loafstead-demo",
        "--format",
        "json",
        fixture.json.to_str().expect("UTF-8 fixture path"),
    ]);

    assert_eq!(output.status.code(), Some(1), "{}", describe(&output));
    let report: Value = serde_json::from_slice(&output.stdout).expect("valid JSON report");
    assert!(finding_codes(&report).contains(&"cosmetic-composition-invisible"));
}

#[test]
fn a_cosmetic_cannot_replace_the_body_while_also_drawing_its_own_item() {
    let fixture = Fixture::body_overwriting_hat("body-overwriting hat");
    let output = run(&[
        "check",
        "--profile",
        "loafstead-demo",
        "--format",
        "json",
        fixture.json.to_str().expect("UTF-8 fixture path"),
    ]);

    assert_eq!(output.status.code(), Some(1), "{}", describe(&output));
    let report: Value = serde_json::from_slice(&output.stdout).expect("valid JSON report");
    assert!(finding_codes(&report).contains(&"cosmetic-composition-invisible"));
}

#[test]
fn a_transparent_keyed_decoration_does_not_invalidate_a_drawable_clip() {
    let fixture = Fixture::faded_decoration("faded decoration");
    let output = run(&[
        "check",
        "--profile",
        "loafstead-demo",
        "--format",
        "json",
        fixture.json.to_str().expect("UTF-8 fixture path"),
    ]);

    assert_success(&output);
    let report: Value = serde_json::from_slice(&output.stdout).expect("valid JSON report");
    assert!(!finding_codes(&report).contains(&"animation-frame-invalid"));
}

#[test]
fn zero_area_setup_art_is_not_considered_drawable() {
    let fixture = Fixture::zero_area("zero area");
    let output = run(&[
        "check",
        "--profile",
        "loafstead-demo",
        "--format",
        "json",
        fixture.json.to_str().expect("UTF-8 fixture path"),
    ]);

    assert_eq!(output.status.code(), Some(1), "{}", describe(&output));
    let report: Value = serde_json::from_slice(&output.stdout).expect("valid JSON report");
    assert!(finding_codes(&report).contains(&"setup-pose-not-drawable"));
}

#[test]
fn fully_transparent_setup_art_is_not_considered_drawable() {
    let fixture = Fixture::transparent("transparent");
    let output = run(&[
        "check",
        "--profile",
        "loafstead-demo",
        "--format",
        "json",
        fixture.json.to_str().expect("UTF-8 fixture path"),
    ]);

    assert_eq!(output.status.code(), Some(1), "{}", describe(&output));
    let report: Value = serde_json::from_slice(&output.stdout).expect("valid JSON report");
    assert!(finding_codes(&report).contains(&"setup-pose-not-drawable"));
}

#[test]
fn profile_failure_is_machine_readable_and_uses_exit_one() {
    let fixture = Fixture::incomplete("jose style first export", true);
    let output = run(&[
        "check",
        "--profile",
        "loafstead-demo",
        "--format",
        "json",
        fixture.json.to_str().expect("UTF-8 fixture path"),
    ]);

    assert_eq!(output.status.code(), Some(1), "{}", describe(&output));
    assert!(output.stderr.is_empty(), "{}", describe(&output));
    let report: Value = serde_json::from_slice(&output.stdout).expect("valid JSON report");
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["profile"]["name"], "loafstead-demo");
    assert_eq!(report["profile"]["version"], 1);
    assert_eq!(report["status"], "fail");
    assert_eq!(report["summary"]["errors"], 17);
    assert!(report["findings"].as_array().is_some_and(|findings| {
        findings.iter().any(|finding| {
            finding["code"] == "atlas-premultiplied-alpha" && finding["severity"] == "error"
        })
    }));
    assert_eq!(report["unverified"].as_array().map(Vec::len), Some(7));
}

#[test]
fn diagnostic_multiplicity_has_bounded_report_output() {
    let fixture = Fixture::complete("many diagnostics");
    let mut document: Value =
        serde_json::from_slice(&fs::read(&fixture.json).expect("read fixture JSON"))
            .expect("parse fixture JSON");
    let root = document.as_object_mut().expect("fixture root object");
    for ordinal in 0..400 {
        root.insert(format!("unknown-{ordinal:03}"), Value::Null);
    }
    fs::write(
        &fixture.json,
        serde_json::to_vec(&document).expect("serialize diagnostic-multiplicity fixture"),
    )
    .expect("write diagnostic-multiplicity fixture");

    let output = run(&[
        "check",
        "--profile",
        "loafstead-demo",
        "--format",
        "json",
        fixture.json.to_str().expect("UTF-8 fixture path"),
    ]);
    assert_eq!(output.status.code(), Some(1), "{}", describe(&output));
    let report: Value = serde_json::from_slice(&output.stdout).expect("bounded JSON report");
    let findings = report["findings"].as_array().expect("findings array");
    assert!(
        findings.len() < 300,
        "report was not bounded: {}",
        findings.len()
    );
    assert!(finding_codes(&report).contains(&"runtime-diagnostics-truncated"));
}

#[test]
fn atlas_line_multiplicity_is_rejected_before_runtime_loading() {
    let fixture = Fixture::complete("many atlas lines");
    fs::write(fixture.root.join("cat.atlas"), "\n".repeat(70_000))
        .expect("write atlas line-multiplicity fixture");

    let output = run(&[
        "check",
        "--profile",
        "loafstead-demo",
        "--format",
        "json",
        fixture.json.to_str().expect("UTF-8 fixture path"),
    ]);
    assert_eq!(output.status.code(), Some(2), "{}", describe(&output));
    let report: Value = serde_json::from_slice(&output.stdout).expect("source-error JSON");
    assert_eq!(report["findings"][0]["code"], "source-atlas-line-limit");
}

#[test]
fn directory_input_never_guesses_between_json_exports() {
    let fixture = Fixture::complete("ambiguous");
    fs::write(
        fixture.root.join("stub.json"),
        br#"{"skeleton":{"spine":"4.3.23"},"bones":[{"name":"root"}]}"#,
    )
    .expect("write second JSON");

    let output = run(&[
        "check",
        "--profile",
        "loafstead-demo",
        fixture.root.to_str().expect("UTF-8 fixture path"),
    ]);

    assert_eq!(output.status.code(), Some(2), "{}", describe(&output));
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 stderr");
    assert!(stderr.contains("multiple skeleton JSON files"), "{stderr}");
    assert!(stderr.contains("cat.json"), "{stderr}");
    assert!(stderr.contains("stub.json"), "{stderr}");
}

#[test]
fn help_documents_the_installed_command_and_exit_codes() {
    let output = run(&["check", "--help"]);

    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    assert!(
        stdout.contains("spinal check --profile loafstead-demo"),
        "{stdout}"
    );
    assert!(stdout.contains("0  Profile passed"), "{stdout}");
    assert!(stdout.contains("1  Export failed"), "{stdout}");
    assert!(stdout.contains("2  Command or source error"), "{stdout}");
}

#[test]
fn json_format_is_total_for_command_and_source_errors() {
    let command = run(&[
        "check",
        "--profile",
        "loafstead-demo",
        "--format",
        "json",
        "--bogus",
    ]);
    assert_eq!(command.status.code(), Some(2), "{}", describe(&command));
    let command_report: Value =
        serde_json::from_slice(&command.stdout).expect("command-error JSON");
    assert_eq!(command_report["schema_version"], 1);
    assert_eq!(command_report["status"], "command-error");
    assert_eq!(command_report["summary"]["errors"], 1);
    assert!(command_report["findings"].is_array());

    let missing = unique_temp_dir("missing").join("cat.json");
    let source = run_os(&[
        OsStr::new("check"),
        OsStr::new("--profile"),
        OsStr::new("loafstead-demo"),
        OsStr::new("--format"),
        OsStr::new("json"),
        missing.as_os_str(),
    ]);
    assert_eq!(source.status.code(), Some(2), "{}", describe(&source));
    let source_report: Value = serde_json::from_slice(&source.stdout).expect("source-error JSON");
    assert_eq!(source_report["schema_version"], 1);
    assert_eq!(source_report["status"], "source-error");
    assert_eq!(source_report["profile"]["name"], "loafstead-demo");
    assert!(source_report["readiness"].is_object());
    assert!(source_report["findings"].is_array());
}

#[test]
fn unsafe_or_nonportable_page_references_are_source_errors() {
    for reference in [
        "/tmp/page.png",
        "https://example.test/page.png",
        "page#label.png",
        "../page.png",
        "nested\\page.png",
        "C:/page.png",
        "CON.png",
        "page.png:stream",
        "trailing./page.png",
    ] {
        let fixture = Fixture::complete(&format!("unsafe-page-{}", reference.len()));
        let atlas = atlas_text(false).replacen("cat.png", reference, 1);
        fs::write(fixture.root.join("cat.atlas"), atlas).expect("replace atlas page name");
        let output = run(&[
            "check",
            "--profile",
            "loafstead-demo",
            "--format",
            "json",
            fixture.json.to_str().expect("UTF-8 fixture path"),
        ]);
        assert_eq!(
            output.status.code(),
            Some(2),
            "reference {reference:?}: {}",
            describe(&output)
        );
        let report: Value = serde_json::from_slice(&output.stdout).expect("source-error JSON");
        assert_eq!(report["status"], "source-error", "{reference:?}");
        assert!(report["source"].is_object(), "{reference:?}: {report}");
        assert_eq!(report["spine_version"], "4.3.23", "{reference:?}");
        assert!(report["inventory"].is_object(), "{reference:?}");
        assert_eq!(
            report["unverified"].as_array().map(Vec::len),
            Some(7),
            "{reference:?}"
        );
        assert_eq!(
            report["findings"][0]["code"], "source-unsafe-page-reference",
            "{reference:?}"
        );
    }
}

#[test]
fn windows_case_colliding_page_names_are_rejected_on_every_host() {
    let fixture = Fixture::complete("case collision");
    let atlas = format!("{}\nCAT.PNG\n\tsize: 2, 2\n", atlas_text(false));
    fs::write(fixture.root.join("cat.atlas"), atlas).expect("write colliding pages");
    let output = run(&[
        "check",
        "--profile",
        "loafstead-demo",
        "--format",
        "json",
        fixture.json.to_str().expect("UTF-8 fixture path"),
    ]);
    assert_eq!(output.status.code(), Some(2), "{}", describe(&output));
    let report: Value = serde_json::from_slice(&output.stdout).expect("source-error JSON");
    assert_eq!(report["findings"][0]["code"], "source-page-name-collision");
}

#[test]
fn corrupt_non_rgba_and_wrong_size_pngs_fail_with_stable_codes() {
    for (label, mutate, expected) in [
        (
            "corrupt",
            write_corrupt_png as fn(&Path),
            "atlas-page-unreadable",
        ),
        ("grayscale", write_grayscale_png, "atlas-page-not-rgba8"),
        (
            "wrong-size",
            write_wrong_size_png,
            "atlas-page-dimension-mismatch",
        ),
        (
            "wrong-size-header-only",
            write_wrong_size_header_only_png,
            "atlas-page-dimension-mismatch",
        ),
        (
            "huge-header",
            write_huge_header_png,
            "atlas-page-unreadable",
        ),
    ] {
        let fixture = Fixture::complete(label);
        mutate(&fixture.root.join("cat.png"));
        let output = run(&[
            "check",
            "--profile",
            "loafstead-demo",
            "--format",
            "json",
            fixture.json.to_str().expect("UTF-8 fixture path"),
        ]);
        assert_eq!(
            output.status.code(),
            Some(1),
            "{label}: {}",
            describe(&output)
        );
        let report: Value = serde_json::from_slice(&output.stdout).expect("profile JSON");
        assert!(
            finding_codes(&report).contains(&expected),
            "{label}: {report}"
        );
    }
}

#[test]
fn missing_atlas_page_is_an_actionable_profile_failure() {
    let fixture = Fixture::complete("missing page");
    fs::remove_file(fixture.root.join("cat.png")).expect("remove atlas page");
    let output = run(&[
        "check",
        "--profile",
        "loafstead-demo",
        "--format",
        "json",
        fixture.json.to_str().expect("UTF-8 fixture path"),
    ]);
    assert_eq!(output.status.code(), Some(1), "{}", describe(&output));
    let report: Value = serde_json::from_slice(&output.stdout).expect("profile JSON");
    assert!(finding_codes(&report).contains(&"atlas-page-unreadable"));
}

#[test]
fn ambiguous_atlases_are_sorted_and_never_guessed() {
    let fixture = Fixture::complete("ambiguous atlas");
    let conventional = fixture.root.join("cat.atlas");
    fs::rename(&conventional, fixture.root.join("z.atlas")).expect("rename conventional atlas");
    fs::write(fixture.root.join("a.atlas"), atlas_text(false)).expect("write second atlas");
    let output = run(&[
        "check",
        "--profile",
        "loafstead-demo",
        fixture.json.to_str().expect("UTF-8 fixture path"),
    ]);
    assert_eq!(output.status.code(), Some(2), "{}", describe(&output));
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 stderr");
    let a = stderr.find("a.atlas").expect("a.atlas in error");
    let z = stderr.find("z.atlas").expect("z.atlas in error");
    assert!(a < z, "{stderr}");
}

#[test]
fn ambiguous_directory_diagnostics_retain_only_a_bounded_sample() {
    let fixture = Fixture::complete("many json files");
    for ordinal in 0..39 {
        fs::write(
            fixture.root.join(format!("candidate-{ordinal:02}.json")),
            b"{}",
        )
        .expect("write ambiguous JSON candidate");
    }

    let output = run(&[
        "check",
        "--profile",
        "loafstead-demo",
        fixture.root.to_str().expect("UTF-8 fixture path"),
    ]);
    assert_eq!(output.status.code(), Some(2), "{}", describe(&output));
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 stderr");
    assert!(stderr.contains("showing 16 of 40"), "{stderr}");
    assert!(!stderr.contains("candidate-38.json"), "{stderr}");
    assert!(
        stderr.len() < 4096,
        "diagnostic is unexpectedly large: {stderr}"
    );
}

#[cfg(unix)]
#[test]
fn non_unicode_json_paths_do_not_panic() {
    use std::os::unix::ffi::OsStringExt as _;

    let fixture = Fixture::complete("non unicode");
    let non_unicode = fixture
        .root
        .join(OsString::from_vec(b"cat-\xff.json".to_vec()));
    let output = run_os(&[
        OsStr::new("check"),
        OsStr::new("--profile"),
        OsStr::new("loafstead-demo"),
        non_unicode.as_os_str(),
    ]);
    assert_ne!(output.status.code(), Some(101), "{}", describe(&output));
    assert_eq!(output.status.code(), Some(2), "{}", describe(&output));
}

#[test]
fn human_output_escapes_export_control_characters() {
    let fixture = Fixture::complete("safe\nERROR [forged]\u{1b}[2J");
    let output = run_os(&[
        OsStr::new("check"),
        OsStr::new("--profile"),
        OsStr::new("loafstead-demo"),
        fixture.json.as_os_str(),
    ]);
    assert!(output.status.success(), "{}", describe(&output));
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    assert!(!stdout.contains('\u{1b}'), "{stdout}");
    assert!(!stdout.contains("safe\nERROR [forged]"), "{stdout}");
    assert!(
        stdout.contains("safe\\nERROR-[forged]\\u{001b}[2J"),
        "{stdout}"
    );
}

#[cfg(unix)]
#[test]
fn atlas_page_symlinks_cannot_escape_the_atlas_directory() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::complete("symlink escape");
    let atlas_directory = fixture.root.join("atlas");
    let outside_directory = fixture.root.join("outside");
    fs::create_dir_all(&atlas_directory).expect("create atlas directory");
    fs::create_dir_all(&outside_directory).expect("create outside directory");
    fs::rename(
        fixture.root.join("cat.atlas"),
        atlas_directory.join("cat.atlas"),
    )
    .expect("move atlas");
    fs::rename(
        fixture.root.join("cat.png"),
        outside_directory.join("cat.png"),
    )
    .expect("move page outside atlas directory");
    symlink(
        outside_directory.join("cat.png"),
        atlas_directory.join("cat.png"),
    )
    .expect("create escaping symlink");
    let output = run_os(&[
        OsStr::new("check"),
        OsStr::new("--profile"),
        OsStr::new("loafstead-demo"),
        OsStr::new("--format"),
        OsStr::new("json"),
        OsStr::new("--atlas"),
        atlas_directory.join("cat.atlas").as_os_str(),
        fixture.json.as_os_str(),
    ]);
    assert_eq!(output.status.code(), Some(2), "{}", describe(&output));
    let report: Value = serde_json::from_slice(&output.stdout).expect("source-error JSON");
    assert_eq!(
        report["findings"][0]["code"],
        "source-unsafe-page-reference"
    );
}

fn run(arguments: &[&str]) -> Output {
    let arguments = arguments.iter().map(OsStr::new).collect::<Vec<_>>();
    run_os(&arguments)
}

fn run_os(arguments: &[&OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_spinal"))
        .args(arguments.iter().copied())
        .output()
        .expect("run spinal")
}

fn assert_success(output: &Output) {
    assert!(output.status.success(), "{}", describe(output));
}

fn describe(output: &Output) -> String {
    format!(
        "status: {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn finding_codes(report: &Value) -> Vec<&str> {
    report["findings"]
        .as_array()
        .expect("findings array")
        .iter()
        .filter_map(|finding| finding["code"].as_str())
        .collect()
}

struct Fixture {
    root: PathBuf,
    json: PathBuf,
}

#[derive(Default)]
struct FixtureOptions {
    pma: bool,
    complete: bool,
    colliding: bool,
    body_overwrite: bool,
    zero_area: bool,
    transparent: bool,
    faded_decoration: bool,
}

impl Fixture {
    fn complete(label: &str) -> Self {
        Self::write(
            label,
            FixtureOptions {
                complete: true,
                ..Default::default()
            },
        )
    }

    fn incomplete(label: &str, pma: bool) -> Self {
        Self::write(
            label,
            FixtureOptions {
                pma,
                ..Default::default()
            },
        )
    }

    fn colliding_cosmetics(label: &str) -> Self {
        Self::write(
            label,
            FixtureOptions {
                complete: true,
                colliding: true,
                ..Default::default()
            },
        )
    }

    fn body_overwriting_hat(label: &str) -> Self {
        Self::write(
            label,
            FixtureOptions {
                complete: true,
                body_overwrite: true,
                ..Default::default()
            },
        )
    }

    fn faded_decoration(label: &str) -> Self {
        Self::write(
            label,
            FixtureOptions {
                complete: true,
                faded_decoration: true,
                ..Default::default()
            },
        )
    }

    fn zero_area(label: &str) -> Self {
        Self::write(
            label,
            FixtureOptions {
                complete: true,
                zero_area: true,
                ..Default::default()
            },
        )
    }

    fn transparent(label: &str) -> Self {
        Self::write(
            label,
            FixtureOptions {
                complete: true,
                transparent: true,
                ..Default::default()
            },
        )
    }

    fn write(label: &str, options: FixtureOptions) -> Self {
        let FixtureOptions {
            pma,
            complete,
            colliding,
            body_overwrite,
            zero_area,
            transparent,
            faded_decoration,
        } = options;
        let root = unique_temp_dir(label);
        fs::create_dir_all(&root).expect("create fixture directory");
        let json = root.join("cat.json");
        let atlas = root.join("cat.atlas");
        fs::write(
            &json,
            skeleton_json(
                complete,
                colliding,
                body_overwrite,
                zero_area,
                transparent,
                faded_decoration,
            ),
        )
        .expect("write skeleton JSON");
        fs::write(&atlas, atlas_text(pma)).expect("write atlas");
        write_png(&root.join("cat.png"));
        Self { root, json }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _result = fs::remove_dir_all(&self.root);
    }
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let ordinal = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "spinal-check-{}-{}-{ordinal}",
        std::process::id(),
        label.replace(' ', "-")
    ))
}

fn skeleton_json(
    complete: bool,
    colliding: bool,
    body_overwrite: bool,
    zero_area: bool,
    transparent: bool,
    faded_decoration: bool,
) -> Vec<u8> {
    let size = if zero_area { 0 } else { 1 };
    let default_skin = if faded_decoration {
        skin_json_with_two_keys(
            "default",
            ("body-slot", "body", "cat/body"),
            ("blink-slot", "blink", "cat/blink"),
            size,
        )
    } else {
        skin_json_with_key("default", "body-slot", "body", "cat/body", size)
    };
    let skins = if complete {
        let cosmetics = COSMETIC_SKINS
            .iter()
            .map(|name| skin_json(name, colliding, body_overwrite, size))
            .collect::<Vec<_>>()
            .join(",");
        format!("{default_skin},{cosmetics}")
    } else {
        default_skin
    };
    let color = if transparent {
        r#", "color":"FFFFFF00""#
    } else {
        ""
    };
    let slots = if complete && !colliding {
        let blink_slot = if faded_decoration {
            format!(r#",{{"name":"blink-slot","bone":"decoration","attachment":"blink"{color}}}"#)
        } else {
            String::new()
        };
        format!(
            r#"{{"name":"body-slot","bone":"body","attachment":"body"{color}}},
    {{"name":"hat-slot","bone":"body","attachment":"hat"{color}}},
    {{"name":"collar-slot","bone":"body","attachment":"collar"{color}}},
    {{"name":"glasses-slot","bone":"body","attachment":"glasses"{color}}}{blink_slot}"#
        )
    } else {
        format!(r#"{{"name":"body-slot","bone":"body","attachment":"body"{color}}}"#)
    };
    let animations = if complete {
        let faded_bone_timeline = if faded_decoration {
            r#", "decoration":{"scale":[{"x":0,"y":0},{"time":1,"x":0,"y":0}]}"#
        } else {
            ""
        };
        let faded_slot_timeline = if faded_decoration {
            r#", "slots":{"blink-slot":{"rgba":[{"color":"FFFFFF00"},{"time":1,"color":"FFFFFF00"}]}}"#
        } else {
            ""
        };
        REQUIRED_ANIMATIONS
            .iter()
            .map(|name| {
                format!(
                    r#""{name}":{{"bones":{{"body":{{"rotate":[{{"value":0}},{{"time":1,"value":5}}]}}{faded_bone_timeline}}}{faded_slot_timeline}}}"#
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    } else {
        r#""animation":{}"#.to_owned()
    };
    let decoration_bone = if faded_decoration {
        r#",{"name":"decoration","parent":"body"}"#
    } else {
        ""
    };
    format!(
        r#"{{
  "skeleton":{{"spine":"4.3.23"}},
  "bones":[{{"name":"root"}},{{"name":"body","parent":"root"}}{decoration_bone}],
  "slots":[{slots}],
  "skins":[{skins}],
  "animations":{{{animations}}}
}}"#
    )
    .into_bytes()
}

fn skin_json(name: &str, colliding: bool, body_overwrite: bool, size: u32) -> String {
    if colliding {
        return skin_json_with_key(name, "body-slot", "body", "cat/body", size);
    }
    let (slot, placeholder, path) = if name.starts_with("item/hat_") {
        ("hat-slot", "hat", "cat/hat")
    } else if name.starts_with("item/collar_") {
        ("collar-slot", "collar", "cat/collar")
    } else {
        ("glasses-slot", "glasses", "cat/glasses")
    };
    if body_overwrite && name == "item/hat_red_beret" {
        skin_json_with_two_keys(
            name,
            (slot, placeholder, path),
            ("body-slot", "body", "cat/body-alt"),
            size,
        )
    } else {
        skin_json_with_key(name, slot, placeholder, path, size)
    }
}

fn skin_json_with_key(name: &str, slot: &str, placeholder: &str, path: &str, size: u32) -> String {
    format!(
        r#"{{"name":"{name}","attachments":{{"{slot}":{{"{placeholder}":{{"path":"{path}","width":{size},"height":{size}}}}}}}}}"#
    )
}

fn skin_json_with_two_keys(
    name: &str,
    first: (&str, &str, &str),
    second: (&str, &str, &str),
    size: u32,
) -> String {
    let mut attachments = serde_json::Map::new();
    for (slot, placeholder, path) in [first, second] {
        let mut placeholders = serde_json::Map::new();
        placeholders.insert(
            placeholder.to_owned(),
            serde_json::json!({"path": path, "width": size, "height": size}),
        );
        attachments.insert(slot.to_owned(), Value::Object(placeholders));
    }
    serde_json::json!({"name": name, "attachments": attachments}).to_string()
}

fn atlas_text(pma: bool) -> String {
    format!(
        "cat.png\n\tsize: 2, 2\n\tformat: RGBA8888\n\tfilter: Linear, Linear\n\trepeat: none\n\tpma: {pma}\n\tscale: 1\ncat/body\n\tbounds: 0, 0, 1, 1\ncat/hat\n\tbounds: 1, 0, 1, 1\ncat/collar\n\tbounds: 0, 1, 1, 1\ncat/glasses\n\tbounds: 1, 1, 1, 1\ncat/blink\n\tbounds: 0, 0, 1, 1\ncat/body-alt\n\tbounds: 0, 0, 1, 1\n"
    )
}

fn write_png(path: &Path) {
    let file = fs::File::create(path).expect("create PNG");
    let mut encoder = png::Encoder::new(file, 2, 2);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("write PNG header");
    writer.write_image_data(&[255; 16]).expect("write PNG data");
}

fn write_corrupt_png(path: &Path) {
    fs::write(path, b"not a PNG").expect("write corrupt PNG");
}

fn write_grayscale_png(path: &Path) {
    let file = fs::File::create(path).expect("create grayscale PNG");
    let mut encoder = png::Encoder::new(file, 2, 2);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("write grayscale PNG header");
    writer
        .write_image_data(&[255; 4])
        .expect("write grayscale PNG data");
}

fn write_wrong_size_png(path: &Path) {
    let file = fs::File::create(path).expect("create wrong-size PNG");
    let mut encoder = png::Encoder::new(file, 1, 1);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("write wrong-size PNG header");
    writer
        .write_image_data(&[255; 4])
        .expect("write wrong-size PNG data");
}

fn write_wrong_size_header_only_png(path: &Path) {
    let file = fs::File::create(path).expect("create wrong-size header-only PNG");
    let mut encoder = png::Encoder::new(file, 1, 1);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let _writer = encoder.write_header().expect("write wrong-size PNG header");
}

fn write_huge_header_png(path: &Path) {
    let file = fs::File::create(path).expect("create huge-header PNG");
    let mut encoder = png::Encoder::new(file, 9000, 1);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let _writer = encoder.write_header().expect("write huge PNG header");
}

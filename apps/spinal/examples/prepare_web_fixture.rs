//! Writes the self-authored browser comparison fixture to an explicit directory.

use bevy_spinal::spinal::RuntimeBundleManifest;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, fs, io, path::Path, path::PathBuf, process::ExitCode};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FixtureProfile {
    ProductionSmoke,
    Phase0bRehearsal,
}

const OUTPUT_NAMES: [&str; 10] = [
    "manifest.json",
    "preview.manifest.json",
    "current.manifest.json",
    "viewer.spine.json",
    "viewer.atlas",
    "viewer.png",
    "proposed.manifest.json",
    "proposed.spine.json",
    "proposed.atlas",
    "proposed.png",
];

const CURRENT_JSON: &[u8] = br#"{
  "skeleton": {
    "spine": "4.3.23",
    "hash": "spinal-self-authored-current-browser-fixture",
    "width": 180,
    "height": 120
  },
  "bones": [
    { "name": "root" }
  ],
  "slots": [
    { "name": "shape-slot", "bone": "root", "attachment": "shape" }
  ],
  "skins": [
    {
      "name": "default",
      "attachments": {
        "shape-slot": {
          "shape": { "width": 180, "height": 120 }
        }
      }
    }
  ],
  "animations": {
    "sway": {
      "bones": {
        "root": {
          "rotate": [
            { "value": -8 },
            { "time": 0.5, "value": 8 },
            { "time": 1, "value": -8 }
          ]
        }
      }
    }
  }
}
"#;

const PROPOSED_JSON: &[u8] = br#"{
  "skeleton": {
    "spine": "4.3.23",
    "hash": "spinal-self-authored-proposed-browser-fixture",
    "width": 180,
    "height": 120
  },
  "bones": [
    { "name": "root" }
  ],
  "slots": [
    { "name": "shape-slot", "bone": "root", "attachment": "shape" }
  ],
  "skins": [
    {
      "name": "default",
      "attachments": {
        "shape-slot": {
          "shape": { "width": 180, "height": 120 }
        }
      }
    }
  ],
  "animations": {
    "proposed-only": {
      "bones": {
        "root": {
          "rotate": [
            { "value": -8 },
            { "time": 0.5, "value": 8 },
            { "time": 1, "value": -8 }
          ]
        }
      }
    }
  }
}
"#;

const CURRENT_ATLAS: &[u8] = b"viewer.png\n\tsize: 1, 1\n\tformat: RGBA8888\n\tfilter: Linear, Linear\n\trepeat: none\n\tpma: false\nshape\n\tbounds: 0, 0, 1, 1\n";
const PROPOSED_ATLAS: &[u8] = b"proposed.png\n\tsize: 1, 1\n\tformat: RGBA8888\n\tfilter: Linear, Linear\n\trepeat: none\n\tpma: false\nshape\n\tbounds: 0, 0, 1, 1\n";

const RED_PIXEL_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240, 31, 0,
    5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

const BLUE_PIXEL_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 96, 96, 248, 255, 31, 0, 3,
    2, 1, 255, 230, 119, 11, 174, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("prepare Spinal web fixture: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let (profile, destination) = parse_arguments(env::args_os().skip(1))?;
    let destination = PathBuf::from(destination);
    prepare_destination(&destination)?;

    let (current_json, proposed_json) = fixture_json(profile);

    let current_manifest = runtime_manifest(
        "Current",
        "viewer.spine.json",
        &current_json,
        "viewer.atlas",
        CURRENT_ATLAS,
        "viewer.png",
        RED_PIXEL_PNG,
    );
    let proposed_manifest = runtime_manifest(
        "Proposed",
        "proposed.spine.json",
        &proposed_json,
        "proposed.atlas",
        PROPOSED_ATLAS,
        "proposed.png",
        BLUE_PIXEL_PNG,
    );

    for (name, bytes) in [
        ("viewer.spine.json", current_json.as_slice()),
        ("viewer.atlas", CURRENT_ATLAS),
        ("viewer.png", RED_PIXEL_PNG),
        ("proposed.spine.json", proposed_json.as_slice()),
        ("proposed.atlas", PROPOSED_ATLAS),
        ("proposed.png", BLUE_PIXEL_PNG),
        ("current.manifest.json", current_manifest.as_bytes()),
        ("proposed.manifest.json", proposed_manifest.as_bytes()),
    ] {
        write_if_changed(&destination.join(name), bytes)?;
    }

    // The review manifest is written last so an interrupted refresh cannot
    // advertise child manifests before all digest-pinned dependencies exist.
    let preview_manifest = preview_manifest(current_manifest.as_bytes());
    write_if_changed(
        &destination.join("preview.manifest.json"),
        preview_manifest.as_bytes(),
    )?;
    let review_manifest =
        review_manifest(current_manifest.as_bytes(), proposed_manifest.as_bytes());
    write_if_changed(
        &destination.join("manifest.json"),
        review_manifest.as_bytes(),
    )?;
    println!("prepared {}", destination.display());
    Ok(())
}

fn parse_arguments(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<(FixtureProfile, std::ffi::OsString), Box<dyn std::error::Error>> {
    let arguments = arguments.into_iter().collect::<Vec<_>>();
    match arguments.as_slice() {
        [destination] => Ok((FixtureProfile::ProductionSmoke, destination.clone())),
        [flag, destination] if flag == "--phase0b" => {
            Ok((FixtureProfile::Phase0bRehearsal, destination.clone()))
        }
        _other => Err(
            "usage: prepare_web_fixture [--phase0b] DESTINATION_DIRECTORY"
                .to_owned()
                .into(),
        ),
    }
}

fn fixture_json(profile: FixtureProfile) -> (Vec<u8>, Vec<u8>) {
    match profile {
        FixtureProfile::ProductionSmoke => (CURRENT_JSON.to_vec(), PROPOSED_JSON.to_vec()),
        FixtureProfile::Phase0bRehearsal => (
            phase0b_json("current", -8, 8, 10),
            phase0b_json("proposed", -5, 12, 20),
        ),
    }
}

fn phase0b_json(
    label: &str,
    start_rotation: i32,
    middle_rotation: i32,
    event_base: i32,
) -> Vec<u8> {
    format!(
        r#"{{
  "skeleton": {{
    "spine": "4.3.23",
    "hash": "spinal-self-authored-{label}-phase0b-fixture",
    "width": 180,
    "height": 120
  }},
  "bones": [
    {{ "name": "root" }}
  ],
  "slots": [
    {{ "name": "shape-slot", "bone": "root", "attachment": "shape" }}
  ],
  "skins": [
    {{
      "name": "default",
      "attachments": {{
        "shape-slot": {{
          "shape": {{ "width": 180, "height": 120 }}
        }}
      }}
    }},
    {{
      "name": "alternate",
      "attachments": {{
        "shape-slot": {{
          "shape": {{ "path": "shape", "width": 160, "height": 100 }}
        }}
      }}
    }}
  ],
  "events": {{
    "start": {{ "int": {event_base} }},
    "middle": {{ "int": {}, "float": 1.25, "string": "middle" }},
    "end": {{ "int": {}, "volume": 0.5, "balance": -0.25 }}
  }},
  "animations": {{
    "sway": {{
      "bones": {{
        "root": {{
          "rotate": [
            {{ "value": {start_rotation} }},
            {{ "time": 0.5, "value": {middle_rotation} }},
            {{ "time": 1, "value": {start_rotation} }}
          ]
        }}
      }},
      "events": [
        {{ "name": "start" }},
        {{ "time": 0.5, "name": "middle" }},
        {{ "time": 1, "name": "end" }}
      ]
    }}
  }}
}}
"#,
        event_base + 1,
        event_base + 2,
    )
    .into_bytes()
}

fn write_if_changed(path: &Path, bytes: &[u8]) -> io::Result<bool> {
    match fs::read(path) {
        Ok(existing) if existing == bytes => Ok(false),
        Ok(_) => {
            fs::write(path, bytes)?;
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::write(path, bytes)?;
            Ok(true)
        }
        Err(error) => Err(error),
    }
}

fn prepare_destination(destination: &Path) -> Result<(), Box<dyn std::error::Error>> {
    match fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(format!(
                "fixture destination `{}` must be a real directory",
                destination.display()
            )
            .into());
        }
        Ok(_metadata) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(destination)?;
        }
        Err(error) => return Err(error.into()),
    }

    for entry in fs::read_dir(destination)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Err(format!(
                "fixture destination `{}` contains a non-UTF-8 entry",
                destination.display()
            )
            .into());
        };
        if !OUTPUT_NAMES.contains(&name) || !entry.file_type()?.is_file() {
            return Err(format!(
                "fixture destination `{}` contains unexpected entry `{name}`; refusing to publish residue",
                destination.display()
            )
            .into());
        }
    }
    Ok(())
}

fn runtime_manifest(
    label: &str,
    json_name: &str,
    json: &[u8],
    atlas_name: &str,
    atlas: &[u8],
    image_name: &str,
    image: &[u8],
) -> String {
    let files = BTreeMap::from([
        (PathBuf::from(json_name), json.to_vec()),
        (PathBuf::from(atlas_name), atlas.to_vec()),
        (PathBuf::from(image_name), image.to_vec()),
    ]);
    let (bytes, _validated) =
        RuntimeBundleManifest::build(label, Path::new(json_name), Path::new(atlas_name), files)
            .expect("the self-authored browser fixture must satisfy the shared contract");
    String::from_utf8(bytes).expect("canonical manifest is UTF-8 JSON")
}

fn review_manifest(current_manifest: &[u8], proposed_manifest: &[u8]) -> String {
    format!(
        concat!(
            "{{\n",
            "  \"format_version\": 1,\n",
            "  \"primary\": {{\n",
            "    \"url\": \"current.manifest.json\",\n",
            "    \"byte_length\": {},\n",
            "    \"sha256\": \"{}\"\n",
            "  }},\n",
            "  \"comparison\": {{\n",
            "    \"url\": \"proposed.manifest.json\",\n",
            "    \"byte_length\": {},\n",
            "    \"sha256\": \"{}\"\n",
            "  }}\n",
            "}}\n"
        ),
        current_manifest.len(),
        sha256_hex(current_manifest),
        proposed_manifest.len(),
        sha256_hex(proposed_manifest),
    )
}

fn preview_manifest(current_manifest: &[u8]) -> String {
    format!(
        concat!(
            "{{\n",
            "  \"format_version\": 1,\n",
            "  \"primary\": {{\n",
            "    \"url\": \"current.manifest.json\",\n",
            "    \"byte_length\": {},\n",
            "    \"sha256\": \"{}\"\n",
            "  }}\n",
            "}}\n"
        ),
        current_manifest.len(),
        sha256_hex(current_manifest),
    )
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_fixture_has_distinct_supported_current_and_proposed_bundles() {
        for (json, atlas, animation_name) in [
            (CURRENT_JSON, CURRENT_ATLAS, "sway"),
            (PROPOSED_JSON, PROPOSED_ATLAS, "proposed-only"),
        ] {
            let skeleton = bevy_spinal::spinal::load_json(json, atlas)
                .expect("valid fixture")
                .into_asset();
            assert_eq!(skeleton.spine_version(), "4.3.23");
            assert_eq!(
                skeleton
                    .animations()
                    .map(|animation| animation.name())
                    .collect::<Vec<_>>(),
                [animation_name]
            );
            assert_eq!(skeleton.atlas_pages().len(), 1);
        }

        let current = runtime_manifest(
            "Current",
            "viewer.spine.json",
            CURRENT_JSON,
            "viewer.atlas",
            CURRENT_ATLAS,
            "viewer.png",
            RED_PIXEL_PNG,
        );
        let proposed = runtime_manifest(
            "Proposed",
            "proposed.spine.json",
            PROPOSED_JSON,
            "proposed.atlas",
            PROPOSED_ATLAS,
            "proposed.png",
            BLUE_PIXEL_PNG,
        );
        assert_ne!(current, proposed);
        assert_eq!(
            RuntimeBundleManifest::parse(current.as_bytes())
                .expect("current manifest")
                .label(),
            "Current"
        );
        assert_eq!(
            RuntimeBundleManifest::parse(proposed.as_bytes())
                .expect("proposed manifest")
                .label(),
            "Proposed"
        );
        assert_ne!(RED_PIXEL_PNG, BLUE_PIXEL_PNG);
    }

    #[test]
    fn phase0b_profile_has_the_exact_shared_animation_skin_and_event_window() {
        let (current, proposed) = fixture_json(FixtureProfile::Phase0bRehearsal);
        for (json, expected_event_base) in [(&current, 10), (&proposed, 20)] {
            let asset = bevy_spinal::spinal::load_json(json, CURRENT_ATLAS)
                .expect("valid Phase 0B fixture")
                .into_asset();
            assert_eq!(asset.spine_version(), "4.3.23");
            assert_eq!(
                asset
                    .animations()
                    .map(|animation| (animation.name(), animation.duration()))
                    .collect::<Vec<_>>(),
                [("sway", std::time::Duration::from_secs(1))]
            );
            assert_eq!(
                asset.skins().map(|skin| skin.name()).collect::<Vec<_>>(),
                ["default", "alternate"]
            );
            let text = std::str::from_utf8(json).expect("fixture JSON is UTF-8");
            assert!(text.contains(&format!(r#""start": {{ "int": {expected_event_base} }}"#)));
        }
        assert_ne!(current, proposed);
    }

    #[test]
    fn phase0b_mode_is_an_explicit_disjoint_cli_profile() {
        let destination = std::ffi::OsString::from("fixture");
        assert_eq!(
            parse_arguments([destination.clone()]).expect("production args"),
            (FixtureProfile::ProductionSmoke, destination.clone())
        );
        assert_eq!(
            parse_arguments([std::ffi::OsString::from("--phase0b"), destination.clone()])
                .expect("Phase 0B args"),
            (FixtureProfile::Phase0bRehearsal, destination)
        );
        assert!(parse_arguments(Vec::<std::ffi::OsString>::new()).is_err());
        assert!(
            parse_arguments([
                std::ffi::OsString::from("--unknown"),
                std::ffi::OsString::from("fixture")
            ])
            .is_err()
        );
    }

    #[test]
    fn review_manifest_pins_both_child_manifests() {
        let current = b"current child";
        let proposed = b"proposed child";
        let review = review_manifest(current, proposed);

        assert!(review.contains("\"format_version\": 1"));
        assert!(review.contains("\"primary\""));
        assert!(review.contains("\"url\": \"current.manifest.json\""));
        assert!(review.contains(&format!("\"byte_length\": {}", current.len())));
        assert!(review.contains(&sha256_hex(current)));
        assert!(review.contains("\"comparison\""));
        assert!(review.contains("\"url\": \"proposed.manifest.json\""));
        assert!(review.contains(&format!("\"byte_length\": {}", proposed.len())));
        assert!(review.contains(&sha256_hex(proposed)));
    }

    #[test]
    fn preview_manifest_pins_only_the_current_child() {
        let current = b"current child";
        let preview = preview_manifest(current);

        assert!(preview.contains("\"format_version\": 1"));
        assert!(preview.contains("\"primary\""));
        assert!(preview.contains("\"url\": \"current.manifest.json\""));
        assert!(preview.contains(&format!("\"byte_length\": {}", current.len())));
        assert!(preview.contains(&sha256_hex(current)));
        assert!(!preview.contains("\"comparison\""));
        assert!(!preview.contains("proposed.manifest.json"));
    }

    #[test]
    fn destination_residue_is_rejected_without_deletion() {
        let temporary = tempfile::tempdir().expect("temporary fixture root");
        let destination = temporary.path().join("bundle");
        fs::create_dir(&destination).expect("bundle directory");
        let stale = destination.join("stale.secret");
        fs::write(&stale, b"private residue").expect("stale file");

        let error = prepare_destination(&destination).expect_err("residue must fail closed");
        assert!(error.to_string().contains("unexpected entry"));
        assert_eq!(
            fs::read(&stale).expect("residue is never deleted"),
            b"private residue"
        );
    }

    #[test]
    fn identical_fixture_bytes_do_not_rewrite_watched_files() {
        let temporary = tempfile::tempdir().expect("temporary fixture root");
        let path = temporary.path().join("viewer.atlas");
        fs::write(&path, CURRENT_ATLAS).expect("initial fixture");

        assert!(!write_if_changed(&path, CURRENT_ATLAS).expect("compare fixture"));
        assert_eq!(fs::read(path).expect("fixture remains"), CURRENT_ATLAS);
    }

    #[cfg(unix)]
    #[test]
    fn known_output_symlink_is_rejected_without_following_it() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().expect("temporary fixture root");
        let destination = temporary.path().join("bundle");
        fs::create_dir(&destination).expect("bundle directory");
        let outside = temporary.path().join("outside.json");
        fs::write(&outside, b"outside").expect("outside file");
        symlink(&outside, destination.join("manifest.json")).expect("fixture symlink");

        prepare_destination(&destination).expect_err("known symlink must fail closed");
        assert_eq!(fs::read(outside).expect("outside file remains"), b"outside");
    }
}

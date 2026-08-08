//! Writes the self-authored browser smoke fixture to an explicit directory.

use bevy_spinal::spinal::RuntimeBundleManifest;
use std::{collections::BTreeMap, env, fs, io, path::Path, path::PathBuf, process::ExitCode};

const OUTPUT_NAMES: [&str; 4] = [
    "manifest.json",
    "viewer.spine.json",
    "viewer.atlas",
    "viewer.png",
];

const JSON: &[u8] = br#"{
  "skeleton": {
    "spine": "4.3.23",
    "hash": "spinal-self-authored-browser-fixture",
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

const ATLAS: &[u8] = b"viewer.png\n\tsize: 1, 1\n\tformat: RGBA8888\n\tfilter: Linear, Linear\n\trepeat: none\n\tpma: false\nshape\n\tbounds: 0, 0, 1, 1\n";

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
    let mut arguments = env::args_os().skip(1);
    let destination = arguments.next().ok_or("missing destination directory")?;
    if arguments.next().is_some() {
        return Err("expected exactly one destination directory".into());
    }
    let destination = PathBuf::from(destination);
    prepare_destination(&destination)?;
    for (name, bytes) in [
        ("viewer.spine.json", JSON),
        ("viewer.atlas", ATLAS),
        ("viewer.png", BLUE_PIXEL_PNG),
    ] {
        write_if_changed(&destination.join(name), bytes)?;
    }
    // The manifest is written last so an interrupted refresh cannot advertise
    // new bytes before every digest-pinned dependency is present.
    write_if_changed(&destination.join("manifest.json"), manifest().as_bytes())?;
    println!("prepared {}", destination.display());
    Ok(())
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

fn manifest() -> String {
    let files = BTreeMap::from([
        (PathBuf::from("viewer.spine.json"), JSON.to_vec()),
        (PathBuf::from("viewer.atlas"), ATLAS.to_vec()),
        (PathBuf::from("viewer.png"), BLUE_PIXEL_PNG.to_vec()),
    ]);
    let (bytes, _validated) = RuntimeBundleManifest::build(
        "Spinal self-authored browser fixture",
        Path::new("viewer.spine.json"),
        Path::new("viewer.atlas"),
        files,
    )
    .expect("the self-authored browser fixture must satisfy the shared contract");
    String::from_utf8(bytes).expect("canonical manifest is UTF-8 JSON")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_fixture_matches_the_supported_runtime_profile() {
        let skeleton = bevy_spinal::spinal::load_json(JSON, ATLAS)
            .expect("valid fixture")
            .into_asset();
        assert_eq!(skeleton.spine_version(), "4.3.23");
        assert_eq!(
            skeleton
                .animations()
                .map(|animation| animation.name())
                .collect::<Vec<_>>(),
            ["sway"]
        );
        assert_eq!(skeleton.atlas_pages().len(), 1);
        assert!(BLUE_PIXEL_PNG.starts_with(b"\x89PNG\r\n\x1a\n"));
        let manifest = manifest();
        let parsed =
            RuntimeBundleManifest::parse(manifest.as_bytes()).expect("canonical manifest parses");
        let page = parsed
            .files()
            .iter()
            .find(|file| file.virtual_path() == Path::new("viewer.png"))
            .expect("page declaration");
        assert_eq!(page.expected_bytes(), BLUE_PIXEL_PNG.len());
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
        fs::write(&path, ATLAS).expect("initial fixture");

        assert!(!write_if_changed(&path, ATLAS).expect("compare fixture"));
        assert_eq!(fs::read(path).expect("fixture remains"), ATLAS);
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

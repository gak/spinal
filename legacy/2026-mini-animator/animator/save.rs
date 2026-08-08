use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use atomic_write_file::AtomicWriteFile;
use spinal::{DrawItemRef, IkTargetReach, PlaybackMode, Skeleton};
use thiserror::Error;

use super::{
    json_document::{
        JsonEditError, TranslationTimeline, encode_translation_animation, upsert_animation,
    },
    rig::RigBinding,
    walk::{WALK_SAMPLES, WalkParameters},
};

const VALIDATION_SUBSTEPS: usize = 4;

#[derive(Debug)]
pub(crate) struct SaveState {
    pub(crate) source_path: PathBuf,
    pub(crate) original: String,
    pub(crate) backup_path: Option<PathBuf>,
}

#[derive(Debug)]
pub(crate) struct SaveReceipt {
    pub(crate) backup_path: PathBuf,
}

#[derive(Debug, Error)]
pub(crate) enum SaveError {
    #[error("could not read `{path}`: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "the JSON changed on disk after this animator opened it; reload instead of overwriting it"
    )]
    ChangedOnDisk,
    #[error(transparent)]
    Edit(#[from] JsonEditError),
    #[error("the generated JSON did not reload through Spinal: {0}")]
    InvalidCandidate(String),
    #[error("the generated animation `{0}` was not retained after validation")]
    MissingAnimation(String),
    #[error("the generated animation has an invalid solved frame at {time:.3}s: {reason}")]
    InvalidFrame { time: f32, reason: String },
    #[error("could not create backup `{path}`: {source}")]
    Backup {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not write temporary file `{path}`: {source}")]
    Temporary {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not atomically replace `{path}`: {source}")]
    Replace {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub(crate) fn save_walk(
    state: &mut SaveState,
    atlas: &[u8],
    animation_name: &str,
    binding: &RigBinding,
    parameters: WalkParameters,
) -> Result<SaveReceipt, SaveError> {
    if read_source(&state.source_path)? != state.original.as_bytes() {
        return Err(SaveError::ChangedOnDisk);
    }
    let source_permissions = fs::metadata(&state.source_path)
        .map_err(|source| SaveError::Read {
            path: state.source_path.clone(),
            source,
        })?
        .permissions();

    let candidate = build_candidate(&state.original, atlas, animation_name, binding, parameters)?;
    let backup_path = match &state.backup_path {
        Some(path) => path.clone(),
        None => {
            let path = unique_sibling(&state.source_path, "spinal-backup");
            write_new_file(&path, state.original.as_bytes(), &source_permissions).map_err(
                |source| SaveError::Backup {
                    path: path.clone(),
                    source,
                },
            )?;
            state.backup_path = Some(path.clone());
            path
        }
    };

    let mut replacement =
        AtomicWriteFile::open(&state.source_path).map_err(|source| SaveError::Temporary {
            path: state.source_path.clone(),
            source,
        })?;
    replacement
        .write_all(candidate.as_bytes())
        .and_then(|()| replacement.set_permissions(source_permissions))
        .and_then(|()| replacement.sync_all())
        .map_err(|source| SaveError::Temporary {
            path: state.source_path.clone(),
            source,
        })?;
    commit_if_unchanged(&state.source_path, state.original.as_bytes(), replacement)?;
    // The replacement itself has succeeded and the file was synced. A parent
    // directory sync is an extra durability step, not grounds to report that
    // the already-completed save failed.
    let _parent_sync = sync_parent(&state.source_path);
    state.original = candidate;
    Ok(SaveReceipt { backup_path })
}

pub(crate) fn build_candidate(
    source: &str,
    atlas: &[u8],
    animation_name: &str,
    binding: &RigBinding,
    parameters: WalkParameters,
) -> Result<String, SaveError> {
    let keys = parameters.keys();
    let timelines = binding
        .controls
        .iter()
        .map(|control| control.name.as_ref())
        .chain(std::iter::once(binding.body.name.as_ref()))
        .zip(keys.iter())
        .map(|(bone, keys)| TranslationTimeline { bone, keys })
        .collect::<Vec<_>>();
    let animation = encode_translation_animation(&timelines)?;
    let candidate = upsert_animation(source, animation_name, &animation)?;
    validate_candidate(&candidate, atlas, animation_name, parameters)?;
    Ok(candidate)
}

fn validate_candidate(
    candidate: &str,
    atlas: &[u8],
    animation_name: &str,
    parameters: WalkParameters,
) -> Result<(), SaveError> {
    let report = spinal::load_json(candidate.as_bytes(), atlas)
        .map_err(|error| SaveError::InvalidCandidate(error.to_string()))?;
    let asset = report.into_asset();
    let animation = asset
        .animation_id(animation_name)
        .ok_or_else(|| SaveError::MissingAnimation(animation_name.to_owned()))?;
    let mut skeleton = Skeleton::new(Arc::clone(&asset));
    let validation_samples = WALK_SAMPLES * VALIDATION_SUBSTEPS;
    for sample in 0..=validation_samples {
        let time = parameters.duration() * sample as f32 / validation_samples as f32;
        skeleton
            .sample_animation(animation, Duration::from_secs_f32(time), PlaybackMode::Once)
            .map_err(|error| SaveError::InvalidFrame {
                time,
                reason: error.to_string(),
            })?;
        let frame = skeleton.editable_pose().solve();
        if let Some((_constraint, status)) = frame.ik_statuses().find(|(_constraint, status)| {
            status.issue().is_some() || status.target_reach() == Some(IkTargetReach::BeyondReach)
        }) {
            return Err(SaveError::InvalidFrame {
                time,
                reason: match (status.issue(), status.target_reach()) {
                    (Some(issue), _reach) => format!("IK {issue:?}"),
                    (None, Some(IkTargetReach::BeyondReach)) => {
                        "an IK target is beyond the leg's reach".to_owned()
                    }
                    (_issue, _reach) => "IK solve failed".to_owned(),
                },
            });
        }
        if let Some((_constraint, status)) = frame
            .transform_statuses()
            .find(|(_constraint, status)| status.issue().is_some())
        {
            return Err(SaveError::InvalidFrame {
                time,
                reason: status.issue().map_or_else(
                    || "transform constraint solve failed".to_owned(),
                    |issue| format!("transform constraint {issue:?}"),
                ),
            });
        }
        if frame.bones().any(|bone| {
            let world = bone.world_transform();
            !world.translation().is_finite()
                || !world.x_axis().is_finite()
                || !world.y_axis().is_finite()
        }) {
            return Err(SaveError::InvalidFrame {
                time,
                reason: "a bone transform was not finite".to_owned(),
            });
        }
        for draw in frame.draw_items() {
            let valid = match draw {
                DrawItemRef::Region(region) => region
                    .positions()
                    .into_iter()
                    .all(|position| position.is_finite()),
                DrawItemRef::Mesh(mesh) => {
                    mesh.positions().iter().all(|position| position.is_finite())
                }
                _other => true,
            };
            if !valid {
                return Err(SaveError::InvalidFrame {
                    time,
                    reason: "draw geometry was not finite".to_owned(),
                });
            }
        }
    }
    Ok(())
}

fn read_source(path: &Path) -> Result<Vec<u8>, SaveError> {
    fs::read(path).map_err(|source| SaveError::Read {
        path: path.to_owned(),
        source,
    })
}

fn write_new_file(
    path: &Path,
    bytes: &[u8],
    permissions: &fs::Permissions,
) -> Result<(), std::io::Error> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.set_permissions(permissions.clone())?;
    file.sync_all()
}

fn commit_if_unchanged(
    source_path: &Path,
    expected: &[u8],
    replacement: AtomicWriteFile,
) -> Result<(), SaveError> {
    let current = match read_source(source_path) {
        Ok(current) => current,
        Err(error) => {
            let _cleanup = replacement.discard();
            return Err(error);
        }
    };
    if current != expected {
        let _cleanup = replacement.discard();
        return Err(SaveError::ChangedOnDisk);
    }
    replacement.commit().map_err(|source| SaveError::Replace {
        path: source_path.to_owned(),
        source,
    })
}

fn unique_sibling(source: &Path, role: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let file_name = source
        .file_name()
        .map_or_else(|| "skeleton.json".into(), |name| name.to_string_lossy());
    source.with_file_name(format!(
        "{file_name}.{role}-{}-{timestamp}",
        std::process::id()
    ))
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> Result<(), std::io::Error> {
    File::open(path.parent().unwrap_or_else(|| Path::new(".")))?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rig;

    fn temporary_directory(role: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "spinal-animator-{role}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    #[test]
    fn direct_save_backs_up_once_and_refuses_an_external_change() {
        let source = String::from_utf8(rig::TEST_JSON.to_vec()).expect("fixture is UTF-8");
        let asset = spinal::load_json(source.as_bytes(), rig::TEST_ATLAS)
            .expect("test cat loads")
            .into_asset();
        let binding = rig::discover(&asset).expect("test walk controls are discoverable");
        let temporary = temporary_directory("save-test");
        fs::create_dir(&temporary).expect("unique test directory is created");
        let source_path = temporary.join("cat.json");
        fs::write(&source_path, &source).expect("test source is written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&source_path, fs::Permissions::from_mode(0o640))
                .expect("fixture permissions are set");
        }
        let mut state = SaveState {
            source_path: source_path.clone(),
            original: source.clone(),
            backup_path: None,
        };

        let first = save_walk(
            &mut state,
            rig::TEST_ATLAS,
            "walk",
            &binding,
            WalkParameters::default(),
        )
        .expect("first save succeeds");
        assert_eq!(fs::read_to_string(&first.backup_path).unwrap(), source);
        let second = save_walk(
            &mut state,
            rig::TEST_ATLAS,
            "walk",
            &binding,
            WalkParameters {
                stride: 22.0,
                ..WalkParameters::default()
            },
        )
        .expect("second save succeeds");
        assert_eq!(second.backup_path, first.backup_path);
        assert_eq!(fs::read_to_string(&first.backup_path).unwrap(), source);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                fs::metadata(&source_path)
                    .expect("saved file metadata is readable")
                    .permissions()
                    .mode()
                    & 0o777,
                0o640
            );
        }

        fs::write(&source_path, format!("{}\n", state.original))
            .expect("external edit is simulated");
        assert!(matches!(
            save_walk(
                &mut state,
                rig::TEST_ATLAS,
                "walk",
                &binding,
                WalkParameters::default()
            ),
            Err(SaveError::ChangedOnDisk)
        ));
        fs::remove_dir_all(&temporary).expect("test directory is removed");
    }

    #[test]
    fn final_compare_preserves_a_late_external_edit() {
        let temporary = temporary_directory("late-change-test");
        fs::create_dir(&temporary).expect("unique test directory is created");
        let source_path = temporary.join("cat.json");
        fs::write(&source_path, b"original export").expect("original contents are written");
        let mut replacement =
            AtomicWriteFile::open(&source_path).expect("atomic replacement is opened");
        replacement
            .write_all(b"animator draft")
            .expect("draft contents are written");
        fs::write(&source_path, b"external export").expect("external contents are written");

        assert!(matches!(
            commit_if_unchanged(&source_path, b"original export", replacement),
            Err(SaveError::ChangedOnDisk)
        ));
        assert_eq!(fs::read(&source_path).unwrap(), b"external export");
        fs::remove_dir_all(&temporary).expect("test directory is removed");
    }
}

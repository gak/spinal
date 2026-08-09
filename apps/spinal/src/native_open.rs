//! Native Open adapter for selecting and fully preparing one export.

use std::{error::Error, fmt, path::PathBuf};

#[cfg(target_os = "linux")]
use std::{
    ffi::OsString,
    io::Read,
    os::unix::ffi::OsStringExt,
    process::{Command, ExitStatus, Stdio},
};

use crate::source::{PrepareError, PreparedSource};

/// The smallest injectable boundary around the platform file picker.
pub(crate) trait JsonFilePicker {
    fn pick_json(&mut self) -> Result<Option<PathBuf>, Box<str>>;
}

impl<F> JsonFilePicker for F
where
    F: FnMut() -> Result<Option<PathBuf>, Box<str>>,
{
    fn pick_json(&mut self) -> Result<Option<PathBuf>, Box<str>> {
        self()
    }
}

/// Production picker for one Spine JSON export.
pub(crate) struct NativeJsonFilePicker;

impl JsonFilePicker for NativeJsonFilePicker {
    fn pick_json(&mut self) -> Result<Option<PathBuf>, Box<str>> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            native_dialog::DialogBuilder::file()
                .set_title("Open Preview")
                .add_filter("Spine JSON export", ["json"])
                .open_single_file()
                .show()
                .map_err(|error| error.to_string().into_boxed_str())
        }
        #[cfg(target_os = "linux")]
        {
            pick_json_linux()
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            Err("the native file picker is unavailable on this operating system".into())
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LinuxPickerBackend {
    Zenity,
    KDialog,
    Yad,
}

#[cfg(target_os = "linux")]
impl LinuxPickerBackend {
    const fn program(self) -> &'static str {
        match self {
            Self::Zenity => "zenity",
            Self::KDialog => "kdialog",
            Self::Yad => "yad",
        }
    }

    fn command(self) -> Command {
        let mut command = Command::new(self.program());
        command.env_remove("YAD_OPTIONS");
        match self {
            Self::Zenity | Self::Yad => {
                command.args([
                    "--file-selection",
                    "--title=Open Preview",
                    "--file-filter=Spine JSON export | *.json",
                ]);
            }
            Self::KDialog => {
                command.args([
                    "--title",
                    "Open Preview",
                    "--getopenfilename",
                    ".",
                    "*.json|Spine JSON export",
                ]);
            }
        }
        command
    }
}

#[cfg(target_os = "linux")]
fn pick_json_linux() -> Result<Option<PathBuf>, Box<str>> {
    let kde_first = std::env::var("XDG_CURRENT_DESKTOP")
        .ok()
        .is_some_and(|desktop| desktop.to_ascii_uppercase().contains("KDE"));
    let backends = if kde_first {
        [
            LinuxPickerBackend::KDialog,
            LinuxPickerBackend::Zenity,
            LinuxPickerBackend::Yad,
        ]
    } else {
        [
            LinuxPickerBackend::Zenity,
            LinuxPickerBackend::KDialog,
            LinuxPickerBackend::Yad,
        ]
    };
    for backend in backends {
        match run_linux_picker(backend) {
            Ok(result) => return result,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_error) => {
                return Err(
                    format!("the {} file picker could not start", backend.program())
                        .into_boxed_str(),
                );
            }
        }
    }
    Err("no supported Linux file picker was found (expected zenity, kdialog, or yad)".into())
}

#[cfg(target_os = "linux")]
fn run_linux_picker(
    backend: LinuxPickerBackend,
) -> std::io::Result<Result<Option<PathBuf>, Box<str>>> {
    const MAX_PICKER_OUTPUT_BYTES: usize = 16 * 1024;

    let mut command = backend.command();
    command.stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = command.spawn()?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(std::io::Error::other("file picker stdout was unavailable"));
    };
    let mut bytes = Vec::with_capacity(MAX_PICKER_OUTPUT_BYTES + 1);
    let read_result = stdout
        .take((MAX_PICKER_OUTPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes);
    if let Err(error) = read_result {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    if bytes.len() > MAX_PICKER_OUTPUT_BYTES {
        let _ = child.kill();
        let _ = child.wait();
        return Err(std::io::Error::other(
            "file picker output exceeded its fixed bound",
        ));
    }
    let status = child.wait()?;
    Ok(interpret_linux_picker_output(backend, status, bytes))
}

#[cfg(target_os = "linux")]
fn interpret_linux_picker_output(
    backend: LinuxPickerBackend,
    status: ExitStatus,
    stdout: Vec<u8>,
) -> Result<Option<PathBuf>, Box<str>> {
    match status.code() {
        Some(0) => selected_linux_path(stdout),
        Some(1) => Ok(None),
        Some(252) if backend == LinuxPickerBackend::Yad => Ok(None),
        Some(code) => Err(format!(
            "the {} file picker failed with status {code}",
            backend.program()
        )
        .into_boxed_str()),
        None => {
            Err(format!("the {} file picker was terminated", backend.program()).into_boxed_str())
        }
    }
}

#[cfg(target_os = "linux")]
fn selected_linux_path(mut bytes: Vec<u8>) -> Result<Option<PathBuf>, Box<str>> {
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
    }
    if bytes.is_empty()
        || bytes.len() > 16 * 1024
        || bytes
            .iter()
            .any(|byte| matches!(byte, b'\0' | b'\n' | b'\r'))
    {
        return Err("the Linux file picker returned an invalid path".into());
    }
    Ok(Some(PathBuf::from(OsString::from_vec(bytes))))
}

/// A picker result after the selected export has completed preflight.
#[derive(Debug)]
pub(crate) enum OpenResolution {
    Cancelled,
    Prepared(Box<PreparedSource>),
}

/// Failure before the viewer has been launched.
#[derive(Debug)]
pub(crate) enum OpenError {
    Picker(Box<str>),
    Prepare(PrepareError),
}

impl fmt::Display for OpenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Picker(detail) => write!(formatter, "could not open the file picker: {detail}"),
            Self::Prepare(error) => error.fmt(formatter),
        }
    }
}

impl Error for OpenError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Picker(_detail) => None,
            Self::Prepare(error) => Some(error),
        }
    }
}

/// Selects one JSON file and snapshots its JSON, atlas, and referenced pages.
///
/// No launch action is represented until `PreparedSource` has completed the
/// ordinary bounded, read-only native validation path.
pub(crate) fn resolve(picker: &mut impl JsonFilePicker) -> Result<OpenResolution, OpenError> {
    let Some(json_path) = picker.pick_json().map_err(OpenError::Picker)? else {
        return Ok(OpenResolution::Cancelled);
    };
    PreparedSource::load_single(&json_path, None, None)
        .map(Box::new)
        .map(OpenResolution::Prepared)
        .map_err(OpenError::Prepare)
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;
    use crate::bundle::TEST_BLUE_PIXEL_PNG;

    #[cfg(target_os = "linux")]
    use std::{os::unix::process::ExitStatusExt, process::ExitStatus};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TempDirectory(PathBuf);

    impl TempDirectory {
        fn new() -> Self {
            let ordinal = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "spinal-native-open-{}-{ordinal}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create isolated test directory");
            Self(path)
        }

        fn write(&self, relative: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> PathBuf {
            let path = self.0.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create fixture directory");
            }
            fs::write(&path, bytes).expect("write fixture");
            path
        }
    }

    impl Drop for TempDirectory {
        fn drop(&mut self) {
            let _ignored = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn cancel_resolves_without_preparing_a_source() {
        let mut calls = 0;
        let mut picker = || -> Result<_, Box<str>> {
            calls += 1;
            Ok(None)
        };

        assert!(matches!(
            resolve(&mut picker).expect("cancel is not an error"),
            OpenResolution::Cancelled
        ));
        assert_eq!(calls, 1);
    }

    #[test]
    fn invalid_selection_fails_before_a_prepared_source_exists() {
        let directory = TempDirectory::new();
        let json = directory.write("invalid.json", b"not JSON");
        let mut picker = || Ok(Some(json.clone()));

        assert!(resolve(&mut picker).is_err());
    }

    #[test]
    fn valid_selection_uses_the_complete_bounded_source_preflight() {
        let directory = TempDirectory::new();
        let json = directory.write(
            "export/rig.json",
            br#"{"skeleton":{"spine":"4.3.23"},"bones":[{"name":"root"}]}"#,
        );
        directory.write(
            "export/rig.atlas",
            b"rig.png\n\tsize: 1, 1\n\tformat: RGBA8888\n\tfilter: Linear, Linear\n\trepeat: none\n\tpma: false\n",
        );
        directory.write("export/rig.png", TEST_BLUE_PIXEL_PNG);
        let mut picker = || Ok(Some(json.clone()));

        let OpenResolution::Prepared(prepared) =
            resolve(&mut picker).expect("valid selected export completes preflight")
        else {
            panic!("selection was not cancelled");
        };
        assert_eq!(prepared.json_path(), json.canonicalize().unwrap());
        assert_eq!(
            prepared
                .bundle()
                .file_paths()
                .map(Path::to_owned)
                .collect::<Vec<_>>(),
            [
                PathBuf::from("rig.atlas"),
                PathBuf::from("rig.json"),
                PathBuf::from("rig.png"),
            ]
        );
    }

    #[test]
    fn picker_failure_is_not_misreported_as_cancel() {
        let mut picker = || Err("picker backend unavailable".into());

        let error = resolve(&mut picker).expect_err("picker failure must remain explicit");
        assert!(matches!(error, OpenError::Picker(_)));
        assert_eq!(
            error.to_string(),
            "could not open the file picker: picker backend unavailable"
        );
    }

    #[cfg(target_os = "linux")]
    fn linux_status(exit_code: i32) -> ExitStatus {
        ExitStatus::from_raw(exit_code << 8)
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_picker_accepts_only_documented_cancel_statuses() {
        for backend in [
            LinuxPickerBackend::Zenity,
            LinuxPickerBackend::KDialog,
            LinuxPickerBackend::Yad,
        ] {
            assert_eq!(
                interpret_linux_picker_output(backend, linux_status(1), Vec::new())
                    .expect("status 1 is user cancellation"),
                None
            );
            assert!(
                interpret_linux_picker_output(backend, linux_status(5), b"private".to_vec())
                    .is_err()
            );
        }
        assert_eq!(
            interpret_linux_picker_output(LinuxPickerBackend::Yad, linux_status(252), Vec::new(),)
                .expect("Yad escape/window close is cancellation"),
            None
        );
        assert!(
            interpret_linux_picker_output(
                LinuxPickerBackend::KDialog,
                linux_status(254),
                b"private".to_vec(),
            )
            .is_err()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_picker_path_is_single_bounded_and_preserves_non_utf8() {
        assert_eq!(
            selected_linux_path(b"/tmp/rig.json\n".to_vec())
                .expect("one path")
                .expect("selection"),
            PathBuf::from("/tmp/rig.json")
        );
        assert!(selected_linux_path(Vec::new()).is_err());
        assert!(selected_linux_path(b"/tmp/one\n/tmp/two\n".to_vec()).is_err());
        assert!(selected_linux_path(vec![b'x'; 16 * 1024 + 1]).is_err());
        assert!(
            selected_linux_path(vec![b'/', b't', b'm', b'p', b'/', 0xff])
                .expect("non-UTF-8 paths are valid on Linux")
                .is_some()
        );
    }
}

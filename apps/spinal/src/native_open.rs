//! Native Open adapter for selecting a Primary and optional Comparison export.

use std::{
    error::Error,
    fmt,
    path::{Path, PathBuf},
};

#[cfg(target_os = "linux")]
use std::{
    ffi::OsString,
    io::Read,
    os::unix::ffi::OsStringExt,
    process::{Command, ExitStatus, Stdio},
};

use crate::source::{ComparisonPrepareError, PrepareError, PreparedSource};

/// Semantic source slot assigned before any platform picker is opened.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OpenSourceRole {
    Primary,
    Comparison,
}

impl OpenSourceRole {
    const fn name(self) -> &'static str {
        match self {
            Self::Primary => "Primary",
            Self::Comparison => "Comparison",
        }
    }

    const fn picker_title(self) -> &'static str {
        match self {
            Self::Primary => "Open Primary runtime export",
            Self::Comparison => "Optional Comparison — Cancel for Preview",
        }
    }
}

/// The smallest injectable boundary around the platform file picker.
pub(crate) trait JsonFilePicker {
    fn pick_json(&mut self, role: OpenSourceRole) -> Result<Option<PathBuf>, Box<str>>;
}

impl<F> JsonFilePicker for F
where
    F: FnMut(OpenSourceRole) -> Result<Option<PathBuf>, Box<str>>,
{
    fn pick_json(&mut self, role: OpenSourceRole) -> Result<Option<PathBuf>, Box<str>> {
        self(role)
    }
}

/// Production picker for one Spine JSON export.
pub(crate) struct NativeJsonFilePicker;

impl JsonFilePicker for NativeJsonFilePicker {
    fn pick_json(&mut self, role: OpenSourceRole) -> Result<Option<PathBuf>, Box<str>> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            native_dialog::DialogBuilder::file()
                .set_title(role.picker_title())
                .add_filter("Spine JSON export", ["json"])
                .open_single_file()
                .show()
                .map_err(|error| error.to_string().into_boxed_str())
        }
        #[cfg(target_os = "linux")]
        {
            pick_json_linux(role)
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

    fn command(self, role: OpenSourceRole) -> Command {
        let mut command = Command::new(self.program());
        command.env_remove("YAD_OPTIONS");
        match self {
            Self::Zenity | Self::Yad => {
                command
                    .arg("--file-selection")
                    .arg(format!("--title={}", role.picker_title()));
                command.arg("--file-filter=Spine JSON export | *.json");
            }
            Self::KDialog => {
                command.args([
                    "--title",
                    role.picker_title(),
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
fn pick_json_linux(role: OpenSourceRole) -> Result<Option<PathBuf>, Box<str>> {
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
        match run_linux_picker(backend, role) {
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
    role: OpenSourceRole,
) -> std::io::Result<Result<Option<PathBuf>, Box<str>>> {
    const MAX_PICKER_OUTPUT_BYTES: usize = 16 * 1024;

    let mut command = backend.command(role);
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
    Prepared {
        primary: Box<PreparedSource>,
        comparison: Option<Box<PreparedSource>>,
    },
}

/// Failure before the viewer has been launched.
#[derive(Debug)]
pub(crate) enum OpenError {
    Picker {
        role: OpenSourceRole,
        detail: Box<str>,
    },
    Primary(PrepareError),
    Comparison(ComparisonPrepareError),
}

impl fmt::Display for OpenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Picker { role, detail } => write!(
                formatter,
                "could not open the {} file picker: {detail}",
                role.name()
            ),
            Self::Primary(source) => write!(formatter, "Primary export: {source}"),
            Self::Comparison(source) => source.fmt(formatter),
        }
    }
}

impl Error for OpenError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Picker { .. } => None,
            Self::Primary(source) => Some(source),
            Self::Comparison(source) => Some(source),
        }
    }
}

/// Selects and snapshots one Primary plus an optional Comparison.
///
/// No launch action is represented until `PreparedSource` has completed the
/// ordinary bounded, read-only native validation path.
pub(crate) fn resolve(picker: &mut impl JsonFilePicker) -> Result<OpenResolution, OpenError> {
    let Some(primary_path) = pick_json(picker, OpenSourceRole::Primary)? else {
        return Ok(OpenResolution::Cancelled);
    };
    let primary = prepare_source(OpenSourceRole::Primary, &primary_path)?;
    let comparison = pick_json(picker, OpenSourceRole::Comparison)?
        .map(|comparison_path| prepare_source(OpenSourceRole::Comparison, &comparison_path))
        .transpose()?;
    Ok(OpenResolution::Prepared {
        primary: Box::new(primary),
        comparison: comparison.map(Box::new),
    })
}

fn pick_json(
    picker: &mut impl JsonFilePicker,
    role: OpenSourceRole,
) -> Result<Option<PathBuf>, OpenError> {
    picker
        .pick_json(role)
        .map_err(|detail| OpenError::Picker { role, detail })
}

fn prepare_source(role: OpenSourceRole, json_path: &Path) -> Result<PreparedSource, OpenError> {
    match role {
        OpenSourceRole::Primary => {
            PreparedSource::load_single(json_path, None, None).map_err(OpenError::Primary)
        }
        OpenSourceRole::Comparison => PreparedSource::load_single(json_path, None, None)
            .map_err(ComparisonPrepareError::new)
            .map_err(OpenError::Comparison),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;
    use crate::bundle::{TEST_BLUE_PIXEL_PNG, TEST_RED_PIXEL_PNG};

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

    fn write_valid_export(directory: &TempDirectory, relative_json: &str, page: &[u8]) -> PathBuf {
        let json_path = directory.write(
            relative_json,
            br#"{"skeleton":{"spine":"4.3.23"},"bones":[{"name":"root"}]}"#,
        );
        let parent = Path::new(relative_json).parent().unwrap_or(Path::new(""));
        directory.write(
            parent.join("rig.atlas"),
            b"rig.png\n\tsize: 1, 1\n\tformat: RGBA8888\n\tfilter: Linear, Linear\n\trepeat: none\n\tpma: false\n",
        );
        directory.write(parent.join("rig.png"), page);
        json_path
    }

    #[test]
    fn primary_cancel_makes_one_role_correct_call_and_prepares_nothing() {
        let mut roles = Vec::new();
        let mut picker = |role| -> Result<_, Box<str>> {
            roles.push(role);
            Ok(None)
        };

        assert!(matches!(
            resolve(&mut picker).expect("cancel is not an error"),
            OpenResolution::Cancelled
        ));
        assert_eq!(roles, [OpenSourceRole::Primary]);
    }

    #[test]
    fn invalid_primary_never_opens_the_comparison_picker() {
        let directory = TempDirectory::new();
        let json = directory.write("invalid.json", b"not JSON");
        let mut roles = Vec::new();
        let mut picker = |role| {
            roles.push(role);
            Ok(Some(json.clone()))
        };

        let error = resolve(&mut picker).expect_err("invalid Primary must fail");
        assert!(matches!(error, OpenError::Primary(_)));
        assert!(error.to_string().starts_with("Primary export: "));
        assert_eq!(roles, [OpenSourceRole::Primary]);
    }

    #[test]
    fn comparison_cancel_prepares_one_primary_for_preview() {
        let directory = TempDirectory::new();
        let json = write_valid_export(&directory, "export/rig.json", TEST_BLUE_PIXEL_PNG);
        let mut responses = [Some(json.clone()), None].into_iter();
        let mut roles = Vec::new();
        let mut picker = |role| -> Result<_, Box<str>> {
            roles.push(role);
            Ok(responses.next().expect("one response per role"))
        };

        let OpenResolution::Prepared {
            primary,
            comparison,
        } = resolve(&mut picker).expect("valid selected export completes preflight")
        else {
            panic!("Primary selection was not cancelled");
        };
        assert!(comparison.is_none());
        assert_eq!(primary.json_path(), json.canonicalize().unwrap());
        assert_eq!(
            primary
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
        assert_eq!(roles, [OpenSourceRole::Primary, OpenSourceRole::Comparison]);
    }

    #[test]
    fn valid_comparison_prepares_two_isolated_sources() {
        let directory = TempDirectory::new();
        let primary = write_valid_export(&directory, "primary/rig.json", TEST_RED_PIXEL_PNG);
        let comparison = write_valid_export(&directory, "comparison/rig.json", TEST_BLUE_PIXEL_PNG);
        let mut responses = [Some(primary), Some(comparison)].into_iter();
        let mut roles = Vec::new();
        let mut picker = |role| -> Result<_, Box<str>> {
            roles.push(role);
            Ok(responses.next().expect("one response per role"))
        };

        let OpenResolution::Prepared {
            primary,
            comparison: Some(comparison),
        } = resolve(&mut picker).expect("both exports complete preflight")
        else {
            panic!("expected a paired launch");
        };
        assert_eq!(
            primary.bundle().file(Path::new("rig.png")),
            Some(TEST_RED_PIXEL_PNG)
        );
        assert_eq!(
            comparison.bundle().file(Path::new("rig.png")),
            Some(TEST_BLUE_PIXEL_PNG)
        );
        assert_eq!(roles, [OpenSourceRole::Primary, OpenSourceRole::Comparison]);
    }

    #[test]
    fn invalid_comparison_is_fatal_and_role_attributed() {
        let directory = TempDirectory::new();
        let primary = write_valid_export(&directory, "primary/rig.json", TEST_RED_PIXEL_PNG);
        let comparison = directory.write("comparison/invalid.json", b"not JSON");
        let mut responses = [Some(primary), Some(comparison)].into_iter();
        let mut roles = Vec::new();
        let mut picker = |role| -> Result<_, Box<str>> {
            roles.push(role);
            Ok(responses.next().expect("one response per role"))
        };

        let error = resolve(&mut picker).expect_err("invalid Comparison must reject launch");
        assert!(matches!(error, OpenError::Comparison(_)));
        assert!(error.to_string().starts_with("comparison export: "));
        assert_eq!(roles, [OpenSourceRole::Primary, OpenSourceRole::Comparison]);
    }

    #[test]
    fn missing_comparison_atlas_uses_exact_comparison_copy() {
        let directory = TempDirectory::new();
        let primary = write_valid_export(&directory, "primary/rig.json", TEST_RED_PIXEL_PNG);
        let comparison = directory.write(
            "comparison/missing.json",
            br#"{"skeleton":{"spine":"4.3.23"},"bones":[{"name":"root"}]}"#,
        );
        let mut responses = [Some(primary), Some(comparison.clone())].into_iter();
        let mut picker =
            |_role| -> Result<_, Box<str>> { Ok(responses.next().expect("one response per role")) };

        let error = resolve(&mut picker).expect_err("missing Comparison atlas must fail");
        let canonical_json = comparison
            .canonicalize()
            .expect("canonical Comparison JSON");
        let expected_atlas = canonical_json.with_file_name("missing.atlas");
        assert!(matches!(error, OpenError::Comparison(_)));
        let message = error.to_string();
        assert_eq!(
            message,
            format!(
                "comparison export: no text atlas was found beside `{}` (looked for `{}`); pass --compare-atlas FILE.atlas",
                canonical_json.display(),
                expected_atlas.display()
            )
        );
        assert!(!message.contains("pass --atlas "));
    }

    #[test]
    fn ambiguous_comparison_atlas_uses_exact_comparison_copy() {
        let directory = TempDirectory::new();
        let primary = write_valid_export(&directory, "primary/rig.json", TEST_RED_PIXEL_PNG);
        let comparison = directory.write(
            "comparison/ambiguous.json",
            br#"{"skeleton":{"spine":"4.3.23"},"bones":[{"name":"root"}]}"#,
        );
        let first_atlas = directory.write("comparison/a.atlas", b"not read");
        let second_atlas = directory.write("comparison/z.atlas", b"not read");
        let mut responses = [Some(primary), Some(comparison.clone())].into_iter();
        let mut picker =
            |_role| -> Result<_, Box<str>> { Ok(responses.next().expect("one response per role")) };

        let error = resolve(&mut picker).expect_err("ambiguous Comparison atlas must fail");
        let canonical_json = comparison
            .canonicalize()
            .expect("canonical Comparison JSON");
        let first_atlas = first_atlas.canonicalize().expect("canonical first atlas");
        let second_atlas = second_atlas.canonicalize().expect("canonical second atlas");
        assert!(matches!(error, OpenError::Comparison(_)));
        let message = error.to_string();
        assert_eq!(
            message,
            format!(
                "comparison export: more than one text atlas was found beside `{}`; pass --compare-atlas with one of:\n  {}\n  {}",
                canonical_json.display(),
                first_atlas.display(),
                second_atlas.display()
            )
        );
        assert!(!message.contains("pass --atlas "));
    }

    #[test]
    fn picker_failures_are_fatal_and_role_attributed() {
        let mut primary_picker = |_role| Err("picker backend unavailable".into());

        let error = resolve(&mut primary_picker).expect_err("picker failure must remain explicit");
        assert!(matches!(
            error,
            OpenError::Picker {
                role: OpenSourceRole::Primary,
                ..
            }
        ));
        assert_eq!(
            error.to_string(),
            "could not open the Primary file picker: picker backend unavailable"
        );

        let directory = TempDirectory::new();
        let primary = write_valid_export(&directory, "primary/rig.json", TEST_RED_PIXEL_PNG);
        let mut picker = |role| match role {
            OpenSourceRole::Primary => Ok(Some(primary.clone())),
            OpenSourceRole::Comparison => Err("comparison picker unavailable".into()),
        };

        let error = resolve(&mut picker).expect_err("Comparison picker failure must be fatal");
        assert!(matches!(
            error,
            OpenError::Picker {
                role: OpenSourceRole::Comparison,
                ..
            }
        ));
        assert_eq!(
            error.to_string(),
            "could not open the Comparison file picker: comparison picker unavailable"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_picker_commands_use_exact_role_specific_titles() {
        for role in [OpenSourceRole::Primary, OpenSourceRole::Comparison] {
            for backend in [
                LinuxPickerBackend::Zenity,
                LinuxPickerBackend::KDialog,
                LinuxPickerBackend::Yad,
            ] {
                let arguments = backend
                    .command(role)
                    .get_args()
                    .map(|argument| argument.to_string_lossy().into_owned())
                    .collect::<Vec<_>>();
                let title = role.picker_title();
                let has_title = match backend {
                    LinuxPickerBackend::Zenity | LinuxPickerBackend::Yad => {
                        arguments.contains(&format!("--title={title}"))
                    }
                    LinuxPickerBackend::KDialog => {
                        arguments.windows(2).any(|pair| pair == ["--title", title])
                    }
                };
                assert!(has_title, "missing {title:?} in {backend:?}: {arguments:?}");
            }
        }
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

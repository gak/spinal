//! Native command-line and filesystem host for the viewer.

use std::ffi::{OsStr, OsString};

use bevy::app::AppExit;
use bevy_spinal::spinal::AlphaEncoding;

use crate::{
    app::{self, LaunchConfig, LaunchSource},
    check,
    native_open::{self, JsonFilePicker, NativeJsonFilePicker, OpenResolution},
    source::{self, Options, ParseResult, PreparedSource},
};

/// Runs Preview, Compare, or the read-only headless checker.
pub fn run<I, A>(arguments: I) -> AppExit
where
    I: IntoIterator<Item = A>,
    A: Into<OsString>,
{
    let arguments = arguments
        .into_iter()
        .map(Into::into)
        .collect::<Vec<OsString>>();
    if arguments
        .first()
        .is_some_and(|argument| argument == OsStr::new("check"))
    {
        return check::run(arguments.into_iter().skip(1));
    }

    let arguments = match utf8_arguments(arguments) {
        Ok(arguments) => arguments,
        Err(argument) => {
            eprintln!(
                "spinal: command arguments must be valid UTF-8; rejected `{}`",
                argument.to_string_lossy()
            );
            return AppExit::from_code(check::USAGE_EXIT_CODE);
        }
    };
    run_viewer(arguments)
}

fn utf8_arguments<I, A>(arguments: I) -> Result<Vec<String>, OsString>
where
    I: IntoIterator<Item = A>,
    A: Into<OsString>,
{
    arguments
        .into_iter()
        .map(|argument| argument.into().into_string())
        .collect()
}

fn run_viewer(arguments: impl IntoIterator<Item = String>) -> AppExit {
    let mut picker = NativeJsonFilePicker;
    run_viewer_with(arguments, &mut picker, app::run)
}

fn run_viewer_with(
    arguments: impl IntoIterator<Item = String>,
    picker: &mut impl JsonFilePicker,
    launch: impl FnOnce(LaunchConfig) -> AppExit,
) -> AppExit {
    let parsed = match Options::parse(arguments) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("spinal: {error}\n\n{}", source::HELP);
            return AppExit::error();
        }
    };
    let (primary, comparison) = match parsed {
        ParseResult::Open => match native_open::resolve(picker) {
            Ok(OpenResolution::Cancelled) => return AppExit::Success,
            Ok(OpenResolution::Prepared(primary)) => (*primary, None),
            Err(error) => {
                eprintln!("spinal: {error}");
                return AppExit::error();
            }
        },
        ParseResult::Run(options) => {
            let primary = match PreparedSource::load(options.clone()) {
                Ok(primary) => primary,
                Err(error) => {
                    eprintln!("spinal: {error}");
                    return AppExit::error();
                }
            };
            let comparison = match PreparedSource::load_comparison(&options) {
                Ok(comparison) => comparison,
                Err(error) => {
                    eprintln!("spinal: {error}");
                    return AppExit::error();
                }
            };
            (primary, comparison)
        }
        ParseResult::Help => {
            print!("{}", source::HELP);
            return AppExit::Success;
        }
    };
    launch(launch_config(&primary, comparison.as_ref()))
}

/// Keeps the source/preflight contract isolated from the Bevy application.
fn launch_config(primary: &PreparedSource, comparison: Option<&PreparedSource>) -> LaunchConfig {
    if let Some(comparison) = comparison {
        debug_assert_eq!(primary.preview_rate(), comparison.preview_rate());
    }
    LaunchConfig {
        primary: launch_source(primary),
        comparison: comparison.map(launch_source),
        preview_rate: primary.preview_rate(),
    }
}

fn launch_source(prepared: &PreparedSource) -> LaunchSource {
    debug_assert_eq!(prepared.preview_fps(), prepared.preview_rate().fps());
    debug_assert_eq!(
        prepared.skeleton().spine_version(),
        prepared.bundle().skeleton().spine_version()
    );
    debug_assert!(
        prepared
            .pages()
            .iter()
            .zip(prepared.bundle().skeleton().atlas_pages())
            .all(|(prepared, bundled)| {
                prepared.name() == bundled.name()
                    && prepared.alpha_encoding() == bundled.alpha_encoding()
            })
    );
    debug_assert_eq!(
        prepared.premultiplied_pages().count(),
        prepared
            .bundle()
            .skeleton()
            .atlas_pages()
            .filter(|page| page.alpha_encoding() == AlphaEncoding::Premultiplied)
            .count()
    );
    LaunchSource::new(
        prepared.bundle().clone(),
        format!(
            "{} ({})",
            prepared.json_name(),
            prepared.json_path().display()
        ),
        prepared.atlas_path().display().to_string(),
    )
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        env, fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;
    use crate::bundle::{TEST_BLUE_PIXEL_PNG, TEST_RED_PIXEL_PNG};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TempDirectory(PathBuf);

    impl TempDirectory {
        fn new() -> Self {
            let ordinal = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "spinal-viewer-native-{}-{ordinal}",
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

    fn write_valid_export(directory: &TempDirectory, relative_json: &str) -> PathBuf {
        let json = r#"{"skeleton":{"spine":"4.3.23"},"bones":[{"name":"root"}]}"#;
        let atlas = b"shared.png\n\tsize: 1, 1\n\tformat: RGBA8888\n\tfilter: Linear, Linear\n\trepeat: none\n\tpma: false\n";
        let json_path = directory.write(relative_json, json);
        let parent = Path::new(relative_json).parent().unwrap_or(Path::new(""));
        directory.write(parent.join("shared.atlas"), atlas);
        directory.write(parent.join("shared.png"), TEST_BLUE_PIXEL_PNG);
        json_path
    }

    #[test]
    fn open_cancel_exits_success_without_launching_bevy() {
        let launches = Cell::new(0);
        let mut picker = || Ok(None);

        let exit = run_viewer_with(Vec::<String>::new(), &mut picker, |_config| {
            launches.set(launches.get() + 1);
            AppExit::error()
        });

        assert_eq!(exit, AppExit::Success);
        assert_eq!(launches.get(), 0);
    }

    #[test]
    fn open_picker_failure_exits_with_error_without_launching_bevy() {
        let launches = Cell::new(0);
        let mut picker = || Err("picker backend unavailable".into());

        let exit = run_viewer_with(Vec::<String>::new(), &mut picker, |_config| {
            launches.set(launches.get() + 1);
            AppExit::Success
        });

        assert_eq!(exit, AppExit::error());
        assert_eq!(launches.get(), 0);
    }

    #[test]
    fn invalid_open_selection_exits_without_launching_bevy() {
        let directory = TempDirectory::new();
        let json = directory.write(
            "broken.json",
            r#"{"skeleton":{"spine":"4.3.23"},"bones":[{"name":"root"}]}"#,
        );
        directory.write(
            "broken.atlas",
            b"missing.png\n\tsize: 1, 1\n\tformat: RGBA8888\n\tfilter: Linear, Linear\n\trepeat: none\n\tpma: false\n",
        );
        let launches = Cell::new(0);
        let mut picker = || Ok(Some(json.clone()));

        let exit = run_viewer_with(Vec::<String>::new(), &mut picker, |_config| {
            launches.set(launches.get() + 1);
            AppExit::Success
        });

        assert_eq!(exit, AppExit::error());
        assert_eq!(launches.get(), 0);
    }

    #[test]
    fn valid_open_selection_launches_once_after_complete_preflight() {
        let directory = TempDirectory::new();
        let json = write_valid_export(&directory, "export/primary.json");
        let launches = Cell::new(0);
        let mut picker = || Ok(Some(json.clone()));

        let exit = run_viewer_with(Vec::<String>::new(), &mut picker, |config| {
            launches.set(launches.get() + 1);
            assert_eq!(config.preview_rate.fps(), 30);
            assert!(config.comparison.is_none());
            assert_eq!(
                config.primary.bundle.file(Path::new("shared.png")),
                Some(TEST_BLUE_PIXEL_PNG)
            );
            AppExit::Success
        });

        assert_eq!(exit, AppExit::Success);
        assert_eq!(launches.get(), 1);
    }

    #[test]
    fn positional_preview_bypasses_the_open_picker() {
        let directory = TempDirectory::new();
        let json = write_valid_export(&directory, "export/primary.json");
        let picker_calls = Cell::new(0);
        let launches = Cell::new(0);
        let mut picker = || {
            picker_calls.set(picker_calls.get() + 1);
            Ok(None)
        };

        let exit = run_viewer_with([json.display().to_string()], &mut picker, |_config| {
            launches.set(launches.get() + 1);
            AppExit::Success
        });

        assert_eq!(exit, AppExit::Success);
        assert_eq!(picker_calls.get(), 0);
        assert_eq!(launches.get(), 1);
    }

    #[test]
    fn launch_config_contains_two_independent_prepared_bundles() {
        let directory = TempDirectory::new();
        let json = r#"{"skeleton":{"spine":"4.3.23"},"bones":[{"name":"root"}]}"#;
        let atlas = b"shared.png\n\tsize: 1, 1\n\tformat: RGBA8888\n\tfilter: Linear, Linear\n\trepeat: none\n\tpma: false\n";
        let primary_json = directory.write("primary/shared.json", json);
        directory.write("primary/shared.atlas", atlas);
        directory.write("primary/shared.png", TEST_RED_PIXEL_PNG);
        let comparison_json = directory.write("comparison/shared.json", json);
        directory.write("comparison/shared.atlas", atlas);
        directory.write("comparison/shared.png", TEST_BLUE_PIXEL_PNG);

        let ParseResult::Run(options) = Options::parse([
            primary_json.display().to_string(),
            "--compare".to_owned(),
            comparison_json.display().to_string(),
            "--fps=24".to_owned(),
        ])
        .expect("valid native comparison arguments") else {
            panic!("expected run options");
        };
        let primary = PreparedSource::load(options.clone()).expect("prepare primary");
        let comparison = PreparedSource::load_comparison(&options)
            .expect("prepare comparison")
            .expect("comparison requested");

        let config = launch_config(&primary, Some(&comparison));
        let comparison = config.comparison.expect("comparison launch source");
        assert_eq!(config.preview_rate.fps(), 24);
        assert_eq!(
            config.primary.bundle.file(Path::new("shared.png")),
            Some(TEST_RED_PIXEL_PNG)
        );
        assert_eq!(
            comparison.bundle.file(Path::new("shared.png")),
            Some(TEST_BLUE_PIXEL_PNG)
        );
    }

    #[test]
    fn check_help_dispatches_without_constructing_the_viewer() {
        assert_eq!(run(["check", "--help"]), AppExit::Success);
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_arguments_are_rejected_without_panicking() {
        use std::os::unix::ffi::OsStringExt;

        let invalid = OsString::from_vec(vec![0xff]);
        assert_eq!(utf8_arguments([invalid.clone()]), Err(invalid));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_check_source_paths_are_source_errors() {
        use std::os::unix::ffi::OsStringExt;

        let invalid_path = OsString::from_vec(b"rig\xff.json".to_vec());
        assert_eq!(
            run([OsString::from("check"), invalid_path]),
            AppExit::from_code(3)
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_check_options_are_usage_errors() {
        use std::os::unix::ffi::OsStringExt;

        let invalid_option = OsString::from_vec(vec![b'-', b'-', 0xff]);
        assert_eq!(
            run([OsString::from("check"), invalid_option]),
            AppExit::from_code(check::USAGE_EXIT_CODE)
        );
    }
}

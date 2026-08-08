//! Shared read-only Spinal viewer application for native and future web hosts.

mod app;
mod bundle;
mod clock;
mod command;
mod preview;
mod session;
mod source;
mod ui;

use std::sync::Arc;

use bevy::app::AppExit;
use bevy_spinal::spinal::AlphaEncoding;

use app::LaunchConfig;
use source::{Options, ParseResult, PreparedSource};

/// Parses viewer arguments, prepares the selected export, and runs the app.
pub fn run(arguments: impl IntoIterator<Item = String>) -> AppExit {
    let options = match Options::parse(arguments) {
        Ok(ParseResult::Run(options)) => options,
        Ok(ParseResult::Help) => {
            print!("{}", source::HELP);
            return AppExit::Success;
        }
        Err(error) => {
            eprintln!("spinal viewer: {error}\n\n{}", source::HELP);
            return AppExit::error();
        }
    };
    let prepared = match PreparedSource::load(options) {
        Ok(prepared) => prepared,
        Err(error) => {
            eprintln!("spinal viewer: {error}");
            return AppExit::error();
        }
    };
    app::run(launch_config(&prepared))
}

/// Keeps the source/preflight contract isolated from the Bevy application.
fn launch_config(prepared: &PreparedSource) -> LaunchConfig {
    debug_assert_eq!(prepared.preview_fps(), prepared.preview_rate().fps());
    let premultiplied_pages = prepared
        .premultiplied_pages()
        .map(|page| {
            debug_assert_eq!(page.alpha_encoding(), AlphaEncoding::Premultiplied);
            Box::<str>::from(page.name())
        })
        .collect();
    LaunchConfig {
        bundle: prepared.bundle().clone(),
        display_path: format!(
            "{} ({})",
            prepared.json_name(),
            prepared.json_path().display()
        ),
        atlas_display_path: prepared.atlas_path().display().to_string(),
        atlas_page_count: prepared.pages().len(),
        premultiplied_pages,
        preflight_skeleton: Arc::clone(prepared.skeleton()),
        preview_rate: prepared.preview_rate(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_remains_read_only_by_construction() {
        let help = source::HELP;
        assert!(help.contains("SKELETON.json"));
        assert!(!help.contains("save"));
        assert!(!help.contains("write"));
    }
}

//! Read-only Bevy preview application for Spine 4.3.23 JSON exports.

mod app;
mod command;
mod preview;
mod source;
mod ui;

use std::{env, sync::Arc};

use bevy::app::AppExit;
use bevy_spinal::spinal::AlphaEncoding;

use app::LaunchConfig;
use source::{Options, ParseResult, PreparedSource};

fn main() -> AppExit {
    let options = match Options::parse(env::args().skip(1)) {
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
        asset_root: prepared.asset_root().to_owned(),
        asset_path: prepared.json_asset_path().to_owned(),
        atlas_path: Some(prepared.atlas_reference().to_owned()),
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
    fn binary_remains_read_only_by_construction() {
        let help = source::HELP;
        assert!(help.contains("SKELETON.json"));
        assert!(!help.contains("save"));
        assert!(!help.contains("write"));
    }
}

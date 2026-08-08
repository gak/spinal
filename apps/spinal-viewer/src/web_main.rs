//! Web entry point for the read-only Spinal viewer.

use bevy::app::AppExit;

fn main() -> AppExit {
    spinal_viewer::run_web()
}

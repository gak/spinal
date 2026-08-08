//! Native entry point for the read-only Spinal viewer.

use std::env;

use bevy::app::AppExit;

fn main() -> AppExit {
    spinal_viewer::run(env::args().skip(1))
}

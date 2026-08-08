//! Native entry point for Spinal's read-only Preview and Compare surface.

use std::env;

use bevy::app::AppExit;

fn main() -> AppExit {
    spinal_app::run(env::args().skip(1))
}

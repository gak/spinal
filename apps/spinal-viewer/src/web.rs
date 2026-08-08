//! Thin browser canvas host for the shared Spinal runtime.

use bevy::{app::AppExit, prelude::*};
use bevy_spinal::SpinalPlugin;

/// Runs one Bevy application and renderer in the page's Spinal canvas.
pub fn run() -> AppExit {
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.025, 0.030, 0.041)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Spinal animation viewer".into(),
                canvas: Some("#spinal-canvas".into()),
                fit_canvas_to_parent: true,
                prevent_default_event_handling: false,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(SpinalPlugin)
        .add_systems(Startup, setup_canvas)
        .run()
}

fn setup_canvas(mut commands: Commands) {
    commands.spawn(Camera2d);
}

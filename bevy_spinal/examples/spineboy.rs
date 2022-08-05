use bevy::prelude::*;
use bevy_spinal::{SpinalBundle, SpinalPlugin};

fn main() {
    App::new().add_plugins(DefaultPlugins)
        .add_plugin(SpinalPlugin {})
        .add_startup_system(init)
        .run();
}

fn init(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn_bundle(Camera2dBundle {
        ..Default::default()
    });

    todo!();
    // commands.spawn_bundle(SpinalBundle {
    //
    // });
}
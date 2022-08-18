use bevy::asset::AssetServerSettings;
use bevy::prelude::*;
use bevy_egui::EguiPlugin;
use bevy_inspector_egui::WorldInspectorPlugin;
use bevy_spinal::{SpinalBundle, SpinalPlugin, SpinalState};

fn main() {
    App::new()
        .insert_resource(AssetServerSettings {
            asset_folder: "../assets".to_string(),
            ..Default::default()
        })
        .add_plugins(DefaultPlugins)
        .add_plugin(SpinalPlugin)
        .add_plugin(EguiPlugin)
        .add_plugin(WorldInspectorPlugin::new())
        .add_startup_system(init)
        .run();
}

fn init(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn_bundle(Camera2dBundle {
        transform: Transform::from_scale(Vec3::splat(2.0)),
        ..Default::default()
    });

    commands.spawn_bundle(SpinalBundle {
        skeleton: asset_server.load("spineboy-ess-4.1/spineboy-ess.skel"),
        state: SpinalState::animate("run"),
        // skeleton: asset_server.load("test/test.skel"),
        ..Default::default()
    });
}

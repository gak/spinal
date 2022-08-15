use bevy::asset::AssetServerSettings;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::render::render_resource::VertexAttribute;
use bevy::sprite::{MaterialMesh2dBundle, Mesh2dHandle};
use bevy_egui::{egui, EguiContext, EguiPlugin};
use bevy_spinal::{SpinalBundle, SpinalPlugin};

fn main() {
    App::new()
        .insert_resource(AssetServerSettings {
            asset_folder: "../assets".to_string(),
            ..Default::default()
        })
        .add_plugins(DefaultPlugins)
        .add_plugin(SpinalPlugin::default())
        .add_plugin(EguiPlugin)
        .add_startup_system(init)
        .run();
}

fn init(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn_bundle(Camera2dBundle {
        transform: Transform::from_scale(Vec3::splat(4.0)),
        ..Default::default()
    });

    commands.spawn_bundle(SpinalBundle {
        skeleton: asset_server.load("spineboy-ess-4.1/spineboy-ess.skel"),
        ..Default::default()
    });
}

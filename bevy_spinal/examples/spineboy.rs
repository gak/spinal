use bevy::asset::AssetServerSettings;
use bevy::prelude::*;
use bevy::render::mesh::PrimitiveTopology;
use bevy::sprite::MaterialMesh2dBundle;
use bevy_spinal::{SpinalBundle, SpinalPlugin};

fn main() {
    App::new()
        .insert_resource(AssetServerSettings {
            asset_folder: "../assets".to_string(),
            ..Default::default()
        })
        .add_plugins(DefaultPlugins)
        .add_plugin(SpinalPlugin::default())
        .add_startup_system(init)
        .run();
}

fn init(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    commands.spawn_bundle(Camera2dBundle {
        ..Default::default()
    });

    // commands.spawn_bundle(SpriteBundle {
    //     texture: asset_server.load("spineboy-pro-4.1/spineboy-pro.png"),
    //     ..default()
    // });


    let handle = meshes.add(Mesh::from(shape::Quad::default()));

    let pt = PrimitiveTopology{}
    let handle = meshes.add(Mesh::new(pt));

    commands.spawn_bundle(MaterialMesh2dBundle {
        mesh: handle.into(),
        transform: Transform::default().with_scale(Vec3::splat(128.)),
        material: materials.add(ColorMaterial::from(Color::PURPLE)),
        ..default()
    });

    commands.spawn_bundle(SpinalBundle {
        skeleton: asset_server.load("spineboy-pro-4.1/spineboy-pro.json"),
        ..Default::default()
    });
}

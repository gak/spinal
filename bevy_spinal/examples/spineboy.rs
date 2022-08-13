use bevy::asset::AssetServerSettings;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::render::render_resource::VertexAttribute;
use bevy::sprite::{MaterialMesh2dBundle, Mesh2dHandle};
use bevy_egui::{egui, EguiContext, EguiPlugin};
use bevy_spinal::{SpinalBundle, SpinalPlugin};
use slowchop::two_dee::{MouseScreenPosition, MouseWorldPosition};

fn main() {
    App::new()
        .insert_resource(AssetServerSettings {
            asset_folder: "../assets".to_string(),
            ..Default::default()
        })
        .add_plugins(DefaultPlugins)
        .add_plugin(SpinalPlugin::default())
        .add_plugin(EguiPlugin)
        .add_plugin(slowchop::two_dee::MousePositionPlugin)
        .add_startup_system(init)
        // .add_system(change_mesh)
        .add_system(debug_ui)
        .run();
}

fn init(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    commands.spawn_bundle(Camera2dBundle {
        transform: Transform::from_scale(Vec3::splat(2.0)),
        ..Default::default()
    });

    // commands.spawn_bundle(SpriteBundle {
    //     texture: asset_server.load("spineboy-pro-4.1/spineboy-pro.png"),
    //     ..default()
    // });

    let handle = meshes.add(Mesh::from(shape::Quad::default()));

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList);
    mesh.set_indices(Some(Indices::U32(vec![0, 1, 2, 2, 3, 0])));
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-2.5, -0.5, 0.0],
            [0.5, -0.5, 0.0],
            [0.5, 0.5, 0.0],
            [-0.5, 0.5, 0.0],
        ],
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        vec![
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
        ],
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
    );
    let handle = meshes.add(mesh);

    // commands.spawn_bundle(MaterialMesh2dBundle {
    //     mesh: handle.into(),
    //     transform: Transform::default().with_scale(Vec3::splat(128.)),
    //     material: materials.add(ColorMaterial::from(Color::PURPLE)),
    //     ..default()
    // });

    commands.spawn_bundle(SpinalBundle {
        skeleton: asset_server.load("test/skeleton.skel"),
        ..Default::default()
    });
}

fn change_mesh(
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut query: Query<&mut Mesh2dHandle>,
) {
    for handle in query.iter_mut() {
        let mesh = meshes.get_mut(&handle.0).unwrap();
        let pos = mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION).unwrap();
        if let VertexAttributeValues::Float32x3(ref mut pos) = pos {
            pos[0][0] = (time.seconds_since_startup().cos() * 10.) as f32;
        }
    }
}

fn debug_ui(mut egui_context: ResMut<EguiContext>, mouse_position: Res<MouseWorldPosition>) {
    egui::Window::new("Hello").show(egui_context.ctx_mut(), |ui| {
        ui.label(format!("{:?}", mouse_position));
    });
}

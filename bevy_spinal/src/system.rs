use crate::component::{Ready, SkeletonReady};
use crate::{MaterialMesh2dBundle, SkeletonAsset};
use bevy::prelude::*;
use bevy::utils::HashSet;

pub fn instance(
    mut commands: Commands,
    mut asset_events: EventReader<AssetEvent<SkeletonAsset>>,
    asset_server: Res<AssetServer>,
    query: Query<(Entity, &Handle<SkeletonAsset>), Without<Ready>>,
) {
    let mut changed = HashSet::new();
    for ev in asset_events.iter() {
        match ev {
            AssetEvent::Created { handle } => {
                changed.insert(handle);
            }
            _ => {}
        }
    }

    for handle in changed {
        for (entity, query_handle) in query.iter() {
            // let skeleton = asset_server.get(&handle).unwrap();
            if handle == query_handle {
                commands.entity(entity).insert(SkeletonReady);
                println!("found~!");
            }
        }
    }
}

pub fn setup(
    mut commands: Commands,
    skeletons: Res<Assets<SkeletonAsset>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut query: Query<(Entity, &Handle<SkeletonAsset>), With<SkeletonReady>>,
) {
    for (entity, handle) in query.iter() {
        let skeleton = skeletons.get(&handle).unwrap();

        for bone in &skeleton.0.bones {
            // Bones
            commands.spawn_bundle(MaterialMesh2dBundle {
                mesh: meshes.add(Mesh::from(shape::Quad::default())).into(),
                transform: Transform::default().with_scale(Vec3::splat(1.)),
                material: materials.add(ColorMaterial::from(Color::PURPLE)),
                ..default()
            });
        }

        commands.entity(entity).remove::<SkeletonReady>();
        // println!("found~!");
    }
}

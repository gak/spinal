use crate::component::{Ready, SkeletonReady};
use crate::SkeletonAsset;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::utils::HashSet;
use bevy_prototype_lyon::prelude::*;
use spinal::SkeletonState;

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
            if handle == query_handle {
                println!("instance {:?}", handle.id);
                commands.entity(entity).insert(SkeletonReady);
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
        dbg!(&skeleton);
        let mut state = SkeletonState::new(&skeleton.0);
        state.pose();

        for (bone, bone_state) in state.bones() {
            // Bones
            // let mut bone_mesh = Mesh::new(PrimitiveTopology::TriangleStrip);
            // bone_mesh.set_indices(Some(Indices::U32(vec![0, 2, 1, 0, 3, 2])));
            // bone_mesh.insert_attribute()
            // let mesh = meshes.add()

            //

            let color: Vec4 = bone.color.vec4();
            let color: Color = color.into();
            let rotation = Quat::from_rotation_z(bone_state.rotation);
            let translation = bone_state.affinity.translation;

            let shape = shapes::Line(
                translation,
                translation + Vec2::from_angle(bone_state.rotation) * bone.length,
            );
            commands.spawn_bundle(GeometryBuilder::build_as(
                &shape,
                DrawMode::Stroke(StrokeMode::new(color, 10.0)),
                Transform::default(),
            ));

            // commands.spawn_bundle(MaterialMesh2dBundle {
            //     mesh: meshes.add(Mesh::from(shape::Quad::default())).into(),
            //     transform: Transform::default()
            //         .with_translation(translation)
            //         // .with_scale(Vec3::splat(10.)),
            //         .with_rotation(rotation)
            //         .with_scale(Vec3::new(bone.length, 5., 1.)),
            //     material: materials.add(ColorMaterial::from(color)),
            //     ..default()
            // });
        }

        commands.entity(entity).remove::<SkeletonReady>();
        println!("setup~!");
    }
}

use crate::component::{Ready, SkeletonReady};
use crate::SkeletonAsset;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::utils::HashSet;
use bevy_prototype_lyon::prelude::*;
use spinal::skeleton::{Attachment, AttachmentData};
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
    mut query: Query<(Entity, &Handle<SkeletonAsset>), With<SkeletonReady>>,
) {
    for (entity, handle) in query.iter() {
        let skeleton = skeletons.get(&handle).unwrap();
        dbg!(&skeleton);
        let mut state = SkeletonState::new(&skeleton.0);
        state.pose();

        for (bone, bone_state) in state.bones() {
            let color: Vec4 = bone.color.vec4();
            let color: Color = color.into();
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
        }

        for (bone_state, attachment) in state.attachments {
            println!("{:?} {:?}", bone_state, attachment);

            match &attachment.data {
                AttachmentData::Region(region) => {
                    let position = bone_state.affinity.translation + region.position;
                    dbg!(bone_state.rotation, region.rotation.to_radians());
                    let rotation = bone_state.rotation + region.rotation.to_radians();
                    let rotation = Quat::from_rotation_z(rotation);
                    let mut color: Color = region.color.vec4().into();
                    color.set_a(0.5);

                    let shape = shapes::Rectangle {
                        extents: region.size,
                        origin: RectangleOrigin::Center,
                    };
                    commands.spawn_bundle(GeometryBuilder::build_as(
                        &shape,
                        DrawMode::Fill(FillMode::color(color)),
                        Transform::from_translation(position.extend(0.)).with_rotation(rotation),
                    ));
                }
                _ => continue,
            }
        }

        commands.entity(entity).remove::<SkeletonReady>();
        println!("setup~!");
    }
}

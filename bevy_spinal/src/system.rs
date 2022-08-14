use crate::component::{Ready, SkeletonReady};
use crate::SkeletonAsset;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::Texture;
use bevy::sprite::{Anchor, Rect};
use bevy::utils::{HashMap, HashSet};
use bevy_prototype_lyon::prelude::*;
use spinal::skeleton::{Attachment, AttachmentData};
use spinal::{AtlasParser, Bounds, SkeletonState};
use std::f32::consts::TAU;

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

fn atlas_bounds_to_rect(b: &Bounds) -> Rect {
    Rect {
        min: b.position,
        max: b.position + b.size,
    }
}

pub fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    skeletons: Res<Assets<SkeletonAsset>>,
    mut texture_atlases: ResMut<Assets<TextureAtlas>>,
    mut query: Query<(Entity, &Handle<SkeletonAsset>), With<SkeletonReady>>,
) {
    for (entity, handle) in query.iter() {
        let skeleton = skeletons.get(&handle).unwrap();
        dbg!(&skeleton);
        let mut state = SkeletonState::new(&skeleton.0);
        state.pose();

        // XXX: Lots of hacks below. Beware!

        let atlas = AtlasParser::parse(include_str!("../../assets/test/test.atlas")).unwrap();

        let texture_handle = asset_server.load("test/test.png");
        let mut texture_atlas = TextureAtlas::new_empty(texture_handle, atlas.pages[0].header.size);
        let mut name_to_atlas = HashMap::new();
        for (index, region) in atlas.pages[0].regions.iter().enumerate() {
            let rect = atlas_bounds_to_rect(&region.bounds.as_ref().unwrap());
            texture_atlas.add_texture(rect);
            name_to_atlas.insert(region.name.as_str(), (index, region));
        }
        let texture_atlas_handle = texture_atlases.add(texture_atlas);

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
                DrawMode::Stroke(StrokeMode::new(color, 4.0)),
                Transform::default(),
            ));
        }

        for (bone, bone_state, attachment) in state.attachments {
            if bone.name != "head" {
                // continue;
            }

            match &attachment.data {
                AttachmentData::Region(region_attachment) => {
                    let bone_position = bone_state.affinity.translation;
                    let angle = bone_state.rotation + region_attachment.rotation.to_radians();

                    let (index, atlas_region) = name_to_atlas[attachment.name.as_str()];
                    let texture_radians = atlas_region.rotate.unwrap_or(0.).to_radians();
                    let anchor = Vec2::from_angle(angle) * region_attachment.position
                        / region_attachment.size;
                    let sprite_position = bone_position + region_attachment.position;
                    dbg!(&anchor);
                    let mut sprite_transform =
                        Transform::from_translation(sprite_position.extend(0.))
                            .with_rotation(Quat::from_rotation_z(angle - texture_radians))
                            .with_scale(bone_state.scale.extend(1.));

                    // Draw a transparent rect where the image should be.
                    if false {
                        sprite_transform.translation.z = -10.0; // Don't think this works!
                        let mut color: Color = region_attachment.color.vec4().into();
                        color.set_a(0.2);
                        let shape = shapes::Rectangle {
                            extents: region_attachment.size,
                            origin: RectangleOrigin::Center,
                        };
                        commands.spawn_bundle(GeometryBuilder::build_as(
                            &shape,
                            DrawMode::Fill(FillMode::color(color)),
                            sprite_transform,
                        ));
                    }

                    if true {
                        commands
                            .spawn_bundle(SpriteSheetBundle {
                                texture_atlas: texture_atlas_handle.clone(),
                                transform: sprite_transform,
                                sprite: TextureAtlasSprite {
                                    index,
                                    anchor: Anchor::Custom(anchor),
                                    ..Default::default()
                                },
                                ..Default::default()
                            })
                            .insert(Testing);
                    }
                }
                _ => continue,
            }
        }

        commands.entity(entity).remove::<SkeletonReady>();
        println!("setup~!");
    }
}

#[derive(Component)]
pub struct Testing;

pub fn testing(time: Res<Time>, mut query: Query<&mut Transform, With<Testing>>) {
    for mut transform in query.iter_mut() {
        // transform.rotation =
        //     (Quat::from_rotation_z(time.time_since_startup().as_secs_f32() * 0.1 * TAU));
    }
}

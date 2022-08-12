use crate::component::{Ready, SkeletonReady};
use crate::SkeletonAsset;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::Texture;
use bevy::sprite::Rect;
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

        let atlas = AtlasParser::parse(include_str!(
            "../../assets/spineboy-ess-4.1/spineboy-ess.atlas"
        ))
        .unwrap();

        let texture_handle = asset_server.load("spineboy-ess-4.1/spineboy-ess.png");
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
            println!("{:?} {:?}", bone_state, attachment);
            if bone.name != "head" {
                // continue;
            }

            match &attachment.data {
                AttachmentData::Region(region) => {
                    let position = bone_state.affinity.translation
                        + Vec2::from_angle(region.rotation.to_radians()) * region.position;
                    let radians = bone_state.rotation + region.rotation.to_radians();
                    let rotation = Quat::from_rotation_z(radians);
                    let mut color: Color = region.color.vec4().into();
                    color.set_a(0.2);
                    let bone_transform =
                        Transform::from_translation(position.extend(0.)).with_rotation(rotation);

                    if false {
                        let shape = shapes::Rectangle {
                            extents: region.size,
                            origin: RectangleOrigin::Center,
                        };
                        commands.spawn_bundle(GeometryBuilder::build_as(
                            &shape,
                            DrawMode::Fill(FillMode::color(color)),
                            bone_transform,
                        ));
                    }

                    let (index, region) = name_to_atlas[attachment.name.as_str()];
                    let texture_radians = region.rotate.unwrap_or(0.).to_radians();
                    dbg!(texture_radians);
                    let sprite_transform = bone_transform; //.with_rotation(Quat::from_rotation_z(-texture_radians));

                    commands.spawn_bundle(SpriteSheetBundle {
                        texture_atlas: texture_atlas_handle.clone(),
                        transform: sprite_transform,
                        sprite: TextureAtlasSprite {
                            index,
                            ..Default::default()
                        },
                        ..Default::default()
                    });
                }
                _ => continue,
            }
        }

        commands.entity(entity).remove::<SkeletonReady>();
        println!("setup~!");
    }
}

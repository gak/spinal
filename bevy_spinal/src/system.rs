use crate::component::{Ready, SkeletonReady};
use crate::SkeletonAsset;
use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::Texture;
use bevy::sprite::{Anchor, Rect};
use bevy::utils::{HashMap, HashSet};
use bevy_prototype_lyon::prelude::*;
use spinal::skeleton::{Attachment, AttachmentData};
use spinal::{Atlas, AtlasPage, AtlasParser, AtlasRegion, SkeletonState};
use std::f32::consts::TAU;
use std::mem::swap;

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

fn atlas_to_bevy_rect(page: &AtlasPage, r: &AtlasRegion) -> Rect {
    let mut bounds = r.bounds.as_ref().unwrap().clone();

    // WTF: When rotated 90 degrees the width and height are flipped.
    if r.rotate == 90. {
        swap(&mut bounds.size.x, &mut bounds.size.y);
    }

    let rect = Rect {
        min: bounds.position,
        max: bounds.position + bounds.size,
    };

    rect
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
        // dbg!(&skeleton);
        let mut state = SkeletonState::new(&skeleton.0);
        state.pose();

        // XXX: Lots of hacks below. Beware!

        // let atlas = AtlasParser::parse(include_str!("../../assets/test/test.atlas")).unwrap();
        let atlas = AtlasParser::parse(include_str!(
            "../../assets/spineboy-ess-4.1/spineboy-ess.atlas"
        ))
        .unwrap();

        // let texture_handle = asset_server.load("test/test.png");
        let texture_handle = asset_server.load("spineboy-ess-4.1/spineboy-ess.png");
        // TODO: Support multiple pages
        let page = &atlas.pages[0];
        let mut texture_atlas = TextureAtlas::new_empty(texture_handle, page.header.size);
        let mut name_to_atlas = HashMap::new();
        for (index, region) in page.regions.iter().enumerate() {
            let rect = atlas_to_bevy_rect(&page, &region);
            texture_atlas.add_texture(rect);
            dbg!(region.name.as_str(), &region.bounds, &region.offsets); // <--- WRONG! read-foot is pointing to a muzzle
            name_to_atlas.insert(region.name.as_str(), (index, region));
        }
        let texture_atlas_handle = texture_atlases.add(texture_atlas);

        for (bone, bone_state) in state.bones() {
            let color: Vec4 = bone.color.vec4();
            let color: Color = color.into();
            let translation = bone_state.affinity.translation;

            let shape = shapes::Line(
                translation.truncate(),
                translation.truncate() + Vec2::from_angle(bone_state.rotation) * bone.length,
            );
            commands.spawn_bundle(GeometryBuilder::build_as(
                &shape,
                DrawMode::Stroke(StrokeMode::new(color, 4.0)),
                Transform::default(),
            ));
        }

        for (slot_idx, bone, bone_state, slot, attachment) in &state.slots {
            dbg!(&slot.name);
            match &attachment.data {
                AttachmentData::Region(region_attachment) => {
                    let (index, atlas_region) = name_to_atlas[attachment.placeholder_name.as_str()];
                    // dbg!(index, atlas_region);
                    let atlas_region_affinity = Affine3A::from_scale_rotation_translation(
                        region_attachment.scale.extend(1.),
                        Quat::from_rotation_z(region_attachment.rotation.to_radians()),
                        region_attachment
                            .position
                            .extend(-10. + (*slot_idx as f32) / 100.),
                    );
                    let transform = bone_state.affinity
                        * atlas_region_affinity
                        * Affine3A::from_rotation_z(-atlas_region.rotate.to_radians());
                    let sprite_transform = Transform::from_matrix(transform.into());

                    commands
                        .spawn_bundle(SpriteSheetBundle {
                            texture_atlas: texture_atlas_handle.clone(),
                            transform: sprite_transform,
                            sprite: TextureAtlasSprite {
                                index,
                                ..Default::default()
                            },
                            ..Default::default()
                        })
                        .insert(Testing);
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

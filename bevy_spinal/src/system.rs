use crate::component::SpinalChild;
use crate::loader::SpinalProject;
use crate::SpinalState;
use bevy::asset::Asset;
use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy::reflect::TypeData;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::Texture;
use bevy::sprite::{Anchor, Rect};
use bevy::utils::{HashMap, HashSet};
use bevy_prototype_lyon::prelude::*;
use spinal::skeleton::{Attachment, AttachmentData};
use spinal::{Atlas, AtlasPage, AtlasParser, AtlasRegion, DetachedSkeletonState, SkeletonState};
use std::mem::swap;

/// Scan for skeletons that have just finished loading and set their state to pose.
pub fn set_state_to_post_on_init(
    mut commands: Commands,
    mut asset_events: EventReader<AssetEvent<SpinalProject>>,
    asset_server: Res<AssetServer>,
    spinal_skeletons: Res<Assets<SpinalProject>>,
    mut query: Query<(&Handle<SpinalProject>, &mut SpinalState)>,
) {
    let mut changed = HashSet::new();
    for ev in asset_events.iter() {
        match ev {
            AssetEvent::Created { handle } => {
                println!("skeleton ready (created)");
                changed.insert(handle);
            }
            AssetEvent::Modified { handle } => {
                println!("skeleton ready (modified)");
                changed.insert(handle);
            }
            _ => {}
        }
    }

    for handle in changed {
        for (skeleton_handle, mut state) in query.iter_mut() {
            if handle != skeleton_handle {
                continue;
            }

            let skeleton = spinal_skeletons.get(skeleton_handle).unwrap();
            state.state.pose(&skeleton.project.skeleton);
        }
    }
}

/// Create and destroy entities as needed if there's differences in visibility. Reposition any
/// entities that have changed.
pub fn ensure_and_transform(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    skeleton_assets: Res<Assets<SpinalProject>>,
    query: Query<(&SpinalState, &Handle<SpinalProject>)>,
    children: Query<&SpinalChild>,
) {
    if children.iter().count() > 0 {
        return;
    }

    for (state, skeleton_handle) in query.iter() {
        let spinal_project = match skeleton_assets.get(skeleton_handle) {
            Some(skeleton) => skeleton,
            None => continue,
        };
        let state: &SpinalState = state;

        for slot_info in &state.state.slots(&spinal_project.project) {
            let transform = Transform::from_matrix(slot_info.affinity.into());
            commands
                .spawn_bundle(SpriteSheetBundle {
                    texture_atlas: spinal_project.atlas.clone(),
                    transform,
                    sprite: TextureAtlasSprite {
                        index: slot_info.atlas_index,
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .insert(SpinalChild {});
        }
    }
}

// /// Create the texture atlas and sprite sheets for each attachment.
// ///
// /// These won't be in the correct position until the update fn which should run straight after
// /// this system.
// pub fn setup(
//     mut commands: Commands,
//     asset_server: Res<AssetServer>,
//     skeletons: Res<Assets<SpinalSkeleton>>,
//     spinal_atlases: Res<Assets<SpinalAtlas>>,
//     mut texture_atlases: ResMut<Assets<TextureAtlas>>,
//     mut query: Query<
//         (Entity, &Handle<SpinalSkeleton>, &Handle<SpinalAtlas>),
//         Without<SkeletonReady>,
//     >,
// ) {
//     for (entity, skeleton_handle, spinal_atlas_handle) in query.iter() {
//         let skeleton = skeletons.get(&skeleton_handle).unwrap();
//         let atlas = spinal_atlases.get(&spinal_atlas_handle).unwrap();
//         dbg!(skeleton, atlas);
//
//         let state = DetachedSkeletonState::new();
//         commands.spawn().insert(SkeletonStateComponent {
//             skeleton_handle: skeleton_handle.clone(),
//             state,
//         });
//     }
// }
//
// pub fn setup__(
//     mut commands: Commands,
//     asset_server: Res<AssetServer>,
//     skeletons: Res<Assets<SpinalSkeleton>>,
//     mut texture_atlases: ResMut<Assets<TextureAtlas>>,
//     mut query: Query<(Entity, &Handle<SpinalSkeleton>), With<SkeletonReady>>,
// ) {
//     for (entity, handle) in query.iter() {
//         // let skeleton = skeletons.get(&handle).unwrap();
//         // let mut state = DetachedSkeletonState::new();
//         // state.pose(&skeleton.0);
//
//         // XXX: Lots of hacks below. Beware!
//
//         // let atlas = AtlasParser::parse(include_str!("../../assets/test/test.atlas")).unwrap();
//         // let atlas = AtlasParser::parse(include_str!(
//         //     // "../../assets/spineboy-ess-4.1/spineboy-ess.atlas"
//         //     "../../assets/raptor-pro-4.1/raptor-pro.atlas"
//         // ))
//         // .unwrap();
//
//         // let texture_handle = asset_server.load("test/test.png");
//         // let texture_handle = asset_server.load("spineboy-ess-4.1/spineboy-ess.png");
//         // let texture_handle = asset_server.load("raptor-pro-4.1/raptor-pro.png");
//         // // TODO: Support multiple pages
//         // let page = &atlas.pages[0];
//         // let mut texture_atlas = TextureAtlas::new_empty(texture_handle, page.header.size);
//         // let mut name_to_atlas = HashMap::new();
//         // for (index, region) in page.regions.iter().enumerate() {
//         //     let rect = atlas_to_bevy_rect(&page, &region);
//         //     texture_atlas.add_texture(rect);
//         //     dbg!(region.name.as_str(), &region.bounds, &region.offsets);
//         //     name_to_atlas.insert(region.name.as_str(), (index, region));
//         // }
//         // let texture_atlas_handle = texture_atlases.add(texture_atlas);
//         //
//         // for (bone, bone_state) in state.bones(&skeleton.0) {
//         //     let color: Vec4 = bone.color.vec4();
//         //     let color: Color = color.into();
//         //     let translation = bone_state.affinity.translation;
//         //
//         //     let shape = shapes::Line(
//         //         translation.truncate(),
//         //         translation.truncate() + Vec2::from_angle(bone_state.rotation) * bone.length,
//         //     );
//         //     commands.spawn_bundle(GeometryBuilder::build_as(
//         //         &shape,
//         //         DrawMode::Stroke(StrokeMode::new(color, 4.0)),
//         //         Transform::default(),
//         //     ));
//         // }
//         //
//         // for (slot_idx, bone, bone_state, slot_dx, attachment) in &state.slots {
//         //     let slot = &skeleton.0.slots[*slot_idx];
//         //
//         //     dbg!(&slot.name);
//         //     match &attachment.data {
//         //         AttachmentData::Region(region_attachment) => {
//         //             let (index, atlas_region) = name_to_atlas[attachment.placeholder_name.as_str()];
//         //             // dbg!(index, atlas_region);
//         //             let atlas_region_affinity = Affine3A::from_scale_rotation_translation(
//         //                 region_attachment.scale.extend(1.),
//         //                 Quat::from_rotation_z(region_attachment.rotation.to_radians()),
//         //                 region_attachment
//         //                     .position
//         //                     .extend(-10. + (*slot_idx as f32) / 100.),
//         //             );
//         //             let transform = bone_state.affinity
//         //                 * atlas_region_affinity
//         //                 * Affine3A::from_rotation_z(-atlas_region.rotate.to_radians());
//         //             let sprite_transform = Transform::from_matrix(transform.into());
//         //
//         //             commands
//         //                 .spawn_bundle(SpriteSheetBundle {
//         //                     texture_atlas: texture_atlas_handle.clone(),
//         //                     transform: sprite_transform,
//         //                     sprite: TextureAtlasSprite {
//         //                         index,
//         //                         ..Default::default()
//         //                     },
//         //                     ..Default::default()
//         //                 })
//         //                 .insert(Testing);
//         //         }
//         //         _ => continue,
//         //     }
//         // }
//
//         commands.entity(entity).remove::<SkeletonReady>();
//         println!("setup~!");
//     }
// }
//

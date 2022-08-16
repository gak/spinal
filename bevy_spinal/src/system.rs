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
pub fn set_state_to_pose_on_init(
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
    skeleton_assets: Res<Assets<SpinalProject>>,
    mut query: Query<(&mut SpinalState, &Handle<SpinalProject>)>,
    mut children: Query<&mut Transform, With<SpinalChild>>,
) {
    for (mut state, skeleton_handle) in query.iter_mut() {
        let spinal_project = match skeleton_assets.get(skeleton_handle) {
            Some(skeleton) => skeleton,
            None => continue,
        };

        let mut state: &mut SpinalState = &mut state;
        state.state.set_bone_rotation()

        let mut updates = Vec::new();
        let slots = &state.state.slots(&spinal_project.project);
        for slot_info in slots {
            let transform = Transform::from_matrix(slot_info.affinity.into());

            match state.children.get(&slot_info.slot.name) {
                Some(child_entity) => {
                    println!("existing");
                    let mut child_transform = children.get_mut(*child_entity).unwrap();
                    *child_transform = transform;
                }
                None => {
                    println!("new");
                    let child_entity = commands
                        .spawn_bundle(SpriteSheetBundle {
                            texture_atlas: spinal_project.atlas.clone(),
                            transform,
                            sprite: TextureAtlasSprite {
                                index: slot_info.atlas_index,
                                ..Default::default()
                            },
                            ..Default::default()
                        })
                        .insert(SpinalChild)
                        .id();

                    updates.push((slot_info.slot.name.to_string(), child_entity));
                }
            }
        }

        for update in updates {
            state.children.insert(update.0, update.1);
        }
    }
}

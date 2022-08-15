use crate::skeleton::{Attachment, Bone, ParentTransform, Skeleton, Slot};
use bevy_math::{Affine3A, Quat, Vec2};
use bevy_utils::HashMap;
use tracing::{trace, warn};

#[derive(Debug, Clone)]
pub struct SkeletonState<'a> {
    skeleton: &'a Skeleton,

    internal: DetachedSkeletonState,
}

impl<'a> SkeletonState<'a> {
    pub fn new(skeleton: &'a Skeleton) -> Self {
        Self {
            skeleton,
            internal: DetachedSkeletonState::default(),
        }
    }

    pub fn pose(&mut self) {
        self.internal.pose(self.skeleton)
    }

    pub fn bones(&'a self) -> Vec<(&'a Bone, &'a BoneState)> {
        self.internal.bones(self.skeleton)
    }
}

/// A state manager when you can't use a lifetime reference to a `Skeleton`.
///
/// Instead you must pass in a reference to the skeleton when calling methods on here.
///
/// Care is needed to make sure it's the correct `Skeleton` instance, otherwise there will be errors.
#[derive(Debug, Clone, Default)]
pub struct DetachedSkeletonState {
    bones: HashMap<usize, BoneState>,
    pub slots: Vec<(usize, usize, BoneState, usize, usize)>,
}

impl DetachedSkeletonState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bones<'a>(&'a self, skeleton: &'a Skeleton) -> Vec<(&'a Bone, &'a BoneState)> {
        self.bones
            .iter()
            .map(|(id, state)| (&skeleton.bones[*id], state))
            .collect()
    }

    pub fn pose(&mut self, skeleton: &Skeleton) {
        if skeleton.bones.len() == 0 {
            warn!("No bones in skeleton.");
            return;
        };

        self.pose_bone(skeleton, 0, BoneState::default());

        //

        self.slots.clear();
        for (slot_idx, slot) in skeleton.slots.iter().enumerate() {
            let bone = &skeleton.bones[slot.bone];
            let bone_state = match self.bones.get(&slot.bone) {
                Some(b) => b.clone(),
                None => {
                    warn!("Slot bone not found in bones.");
                    continue;
                }
            };
            let skin = &skeleton.skins[0]; // TODO: support multiple skins
            let skin_slot = &skin.slots.iter().find(|s| s.slot == slot_idx).unwrap();
            let slot_attachment_name = match slot.attachment.as_ref() {
                Some(s) => s,
                None => {
                    warn!("Slot attachment name not set. Assumed no attachment for set up pose.");
                    continue;
                }
            };

            let slot_attachment = skin_slot
                .attachments
                .iter()
                .find(|attachment| &attachment.attachment_name == slot_attachment_name);

            if let Some(attachment) = slot_attachment {
                self.slots.push((
                    slot_idx,
                    slot.bone,
                    bone_state,
                    slot_idx,
                    todo!(), /*attachment_idx*/
                ));
            } else {
                warn!("Slot attachment not found in skin.");
                continue;
            }

            dbg!(self.slots.len());
        }
    }

    fn pose_bone(&mut self, skeleton: &Skeleton, bone_idx: usize, parent_state: BoneState) {
        let bone = &skeleton.bones[bone_idx];
        let (affinity, rotation) = match bone.transform {
            ParentTransform::Normal => (
                Affine3A::from_scale_rotation_translation(
                    bone.scale.extend(1.),
                    Quat::from_rotation_z(bone.rotation.to_radians()),
                    bone.position.extend(0.),
                ),
                bone.rotation.to_radians(),
            ),
            _ => {
                // TODO: handle different parent transforms
                warn!(
                    "Unhandled transform: {:?} in bone: {}",
                    bone.transform, bone.name
                );
                return;
            }
        };
        let bone_state = BoneState {
            affinity: parent_state.affinity * affinity,
            rotation: parent_state.rotation + rotation,
        };

        self.bones.insert(bone_idx, bone_state.clone());
        trace!("Bone: {} {:?}", bone_idx, bone.name);

        if let Some(children) = skeleton.bones_tree.get(&bone_idx) {
            for child_idx in children {
                self.pose_bone(skeleton, *child_idx, bone_state);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BoneState {
    pub affinity: Affine3A,

    /// Global rotation of the bone.
    // I don't know how to extract rotation out of an Affine3A, so I'm just tracking this separately.
    pub rotation: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BinaryParser;
    use test_log::test;

    #[test]
    fn spineboy() {
        let b = include_bytes!("../../assets/spineboy-pro-4.1/spineboy-pro.skel");
        let skeleton = BinaryParser::parse(b).unwrap();
        let mut state = SkeletonState::new(&skeleton);
        state.pose();
    }
}

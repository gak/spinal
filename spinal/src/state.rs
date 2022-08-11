use crate::skeleton::{Attachment, Bone, ParentTransform, Skeleton};
use bevy_math::Affine2;
use bevy_utils::HashMap;
use tracing::warn;

#[derive(Debug, Clone)]
pub struct SkeletonState<'a> {
    skeleton: &'a Skeleton,

    bones: HashMap<usize, BoneState>,
    pub attachments: Vec<(BoneState, &'a Attachment)>,
}

impl<'a> SkeletonState<'a> {
    pub fn new(skeleton: &'a Skeleton) -> Self {
        Self {
            skeleton,
            bones: HashMap::new(),
            attachments: Vec::new(),
        }
    }

    pub fn bones(&'a self) -> Vec<(&'a Bone, &'a BoneState)> {
        self.bones
            .iter()
            .map(|(id, state)| (&self.skeleton.bones[*id], state))
            .collect()
    }

    pub fn pose(&mut self) {
        if self.skeleton.bones.len() == 0 {
            warn!("No bones in skeleton.");
            return;
        };

        self.pose_bone(0, BoneState::default());

        // We need to find the attachment for each bone.
        //
        // The data is currently saved like this:
        //  * skin_slot.slot (as part of skin/attachments)
        //  * slots[x].bone
        //
        // We should probably make a structure in skeleton like this:
        //  * bone -> attachment
        // but.. this will ignore slot ordering... so for now do it the long way (above).

        self.attachments = Vec::new();
        for skin_slot in &self.skeleton.skins[0].slots {
            // Only grab the first attachment for now.
            let attachment = &skin_slot.attachments[0];

            // Find out the bone.
            let slot = &self.skeleton.slots[skin_slot.slot];
            let bone = self.bones[&slot.bone].clone();
            self.attachments.push((bone, attachment));
        }
    }

    fn pose_bone(&mut self, bone_idx: usize, parent_state: BoneState) {
        let bone = &self.skeleton.bones[bone_idx];
        if bone.shear.x != 0.0 || bone.shear.y != 0.0 {
            warn!("Shearing is not supported yet.");
        }
        let (affinity, rotation) = match bone.transform {
            ParentTransform::Normal => (
                Affine2::from_scale_angle_translation(
                    bone.scale,
                    bone.rotation.to_radians(),
                    bone.position,
                ),
                bone.rotation.to_radians(),
            ),
            _ => return, // TODO!
        };
        let bone_state = BoneState {
            affinity: parent_state.affinity * affinity,
            rotation: parent_state.rotation + rotation,
        };

        self.bones.insert(bone_idx, bone_state.clone());
        println!("Bone: {} {:?} {:?}", bone_idx, bone.name, &bone_state);

        if let Some(children) = self.skeleton.bones_tree.get(&bone_idx) {
            for child_idx in children {
                self.pose_bone(*child_idx, bone_state);
            }
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct BoneState {
    pub affinity: Affine2,

    /// Global rotation of the bone.
    // I don't know how to extra rotation out of an Affine2, so I'm just tracking this separately.
    pub rotation: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BinaryParser;

    #[test]
    fn spineboy() {
        let b = include_bytes!("../../assets/spineboy-pro-4.1/spineboy-pro.skel");
        let skeleton = BinaryParser::parse(b).unwrap();
        let mut state = SkeletonState::new(&skeleton);
        state.pose();
        todo!();
    }
}

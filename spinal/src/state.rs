use crate::skeleton::{Attachment, Bone, ParentTransform, Skeleton, Slot};
use bevy_math::{Affine3A, Quat, Vec2};
use bevy_utils::HashMap;
use tracing::{trace, warn};

#[derive(Debug, Clone)]
pub struct SkeletonState<'a> {
    skeleton: &'a Skeleton,

    bones: HashMap<usize, BoneState>,
    pub attachments: Vec<(&'a Bone, BoneState, &'a Attachment)>,
    pub slots: Vec<(&'a Bone, BoneState, &'a Slot, &'a Attachment)>,
}

impl<'a> SkeletonState<'a> {
    pub fn new(skeleton: &'a Skeleton) -> Self {
        Self {
            skeleton,
            bones: HashMap::new(),
            attachments: Vec::new(),
            slots: Vec::new(),
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

        //

        self.slots.clear();
        for (slot_idx, slot) in self.skeleton.slots.iter().enumerate() {
            trace!("--------------");
            trace!(?slot);
            let bone = &self.skeleton.bones[slot.bone];
            trace!(bone_name = ?bone.name);
            let bone_state = match self.bones.get(&slot.bone) {
                Some(b) => b.clone(),
                None => {
                    warn!("Slot bone not found in bones.");
                    continue;
                }
            };
            let skin = &self.skeleton.skins[0]; // TODO: support multiple skins
            let skin_slot = &skin.slots.iter().find(|s| s.slot == slot_idx).unwrap();
            dbg!(&skin_slot
                .attachments
                .iter()
                .map(|a| &a.attachment_name)
                .collect::<Vec<_>>());

            let slot_attachment_name = match slot.attachment.as_ref() {
                Some(s) => s,
                None => {
                    warn!("Slot attachment name not set. Assumed no attachment for set up pose.");
                    continue;
                }
            };

            dbg!(slot_attachment_name);
            let slot_attachment = skin_slot
                .attachments
                .iter()
                .find(|attachment| &attachment.attachment_name == slot_attachment_name);

            if let Some(attachment) = slot_attachment {
                self.slots.push((bone, bone_state, slot, attachment));
            } else {
                warn!("Slot attachment not found in skin.");
                continue;
            }
            dbg!(self.slots.len());

            // let attachment = slot.attachment_name;
            // let attachment = skin
            //     .attachments
            //     .iter()
            //     .find(|s| s.slot == slot_idx)
            //     .unwrap(); // TODO: error
            // self.slots.push((bone, bone_state, slot, attachment));
        }

        // We need to find the attachment for each bone.
        //
        // The data is currently saved like this:
        //  * skin_slot.slot (as part of skin/attachments)
        //  * slots[x].bone
        //
        // We should probably make a structure in skeleton like this:
        //  * bone -> attachment
        // but.. this will ignore slot ordering... so for now do it the long way (above).

        // self.attachments = Vec::new();
        // for skin_slot in &self.skeleton.skins[0].slots {
        //     // Only grab the first attachment for now.
        //     let attachment = &skin_slot.attachments[0];
        //
        //     // Find out the bone.
        //     let slot = &self.skeleton.slots[skin_slot.slot];
        //     let bone = &self.skeleton.bones[slot.bone];
        //     trace!(
        //         "slot.bone:{:?} skin_slot.slot:{:?} bone.name:{:?}",
        //         slot.bone,
        //         skin_slot.slot,
        //         bone.name
        //     );
        //     let bone_state = self.bones.get(&slot.bone);
        //     let bone_state = match bone_state {
        //         Some(bs) => bs,
        //         None => {
        //             warn!(
        //                 "Could not find bone state for bone: {} {:?}, Skipping...",
        //                 slot.bone, bone.name,
        //             );
        //             continue;
        //         }
        //     };
        //     self.attachments.push((bone, *bone_state, attachment));
        // }
    }

    fn pose_bone(&mut self, bone_idx: usize, parent_state: BoneState) {
        let bone = &self.skeleton.bones[bone_idx];
        if bone.shear.x != 0.0 || bone.shear.y != 0.0 {
            warn!("Shearing is not supported yet.");
        }
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

        if let Some(children) = self.skeleton.bones_tree.get(&bone_idx) {
            for child_idx in children {
                self.pose_bone(*child_idx, bone_state);
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BoneState {
    pub affinity: Affine3A,

    /// Global rotation of the bone.
    // I don't know how to extract rotation out of an Affine2, so I'm just tracking this separately.
    pub rotation: f32,
}

impl Default for BoneState {
    fn default() -> Self {
        Self {
            affinity: Affine3A::IDENTITY,
            rotation: 0.0,
        }
    }
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

    #[test]
    fn api_brainstorm() {
        let b = include_bytes!("../../assets/spineboy-ess-4.1/spineboy-ess.skel");
        let skeleton = BinaryParser::parse(b).unwrap();
        let mut state = SkeletonState::new(&skeleton);
        state.pose();

        //
        for (bone, bone_state, slot, attachment) in state.slots {
            println!("{:?} {:?} {:?} {:?}", bone, bone_state, slot, attachment);
        }
    }
}

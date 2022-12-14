use crate::skeleton::animation::{BoneKeyframe, BoneKeyframeData};
use crate::skeleton::{
    Attachment, AttachmentData, Bone, ParentTransform, Skeleton, SkinSlot, Slot,
};
use crate::{Angle, Project};
use bevy_math::{Affine3A, Quat, Vec2};
use bevy_utils::HashMap;
use tracing::{instrument, trace, warn};

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

    // pub fn bones(&'a self) -> Vec<(&'a Bone, &'a BoneState)> {
    //     self.internal.bones(self.skeleton)
    // }

    pub fn bone(&'a self, bone_name: &str) -> Option<&'a BoneState> {
        self.internal.bone(bone_name)
    }

    pub fn bone_rotation(&mut self, bone_name: &str, angle: Angle) {
        self.internal.bone_rotation(bone_name, angle)
    }

    // pub fn clear_bone_rotation(&self) {
    //     self.internal.clear_bone_rotation()
    // }
}

#[derive(Debug, Clone, Default)]
pub struct BoneModification {
    pub rotation: Angle,
    pub translation: Vec2,
    pub scale: Vec2,
}

impl BoneModification {
    pub fn apply(&mut self, other: &BoneModification) {
        self.translation += other.translation;
        self.scale *= other.scale;
        self.rotation += other.rotation
    }

    pub fn from_rotation(rotation: Angle) -> BoneModification {
        BoneModification {
            rotation,
            ..Default::default()
        }
    }

    pub fn from_translation(translation: Vec2) -> BoneModification {
        BoneModification {
            translation,
            ..Default::default()
        }
    }

    pub fn from_scale(scale: Vec2) -> BoneModification {
        BoneModification {
            scale,
            ..Default::default()
        }
    }
}

/// A state manager when you can't use a lifetime reference to a `Skeleton`.
///
/// Instead you must pass in a reference to the skeleton when calling methods on here.
///
/// Care is needed to make sure it's the correct `Skeleton` instance, otherwise there will be errors.
#[derive(Debug, Clone, Default)]
pub struct DetachedSkeletonState {
    pub calculated_bones: HashMap<String, BoneState>,
    pub animation_modifications: HashMap<String, BoneModification>,
    pub user_modifications: HashMap<String, BoneModification>,
    pub slots: Vec<(usize, usize, BoneState, usize, usize)>,

    pub time: f32,

    pub animation: Option<String>,
    // TODO: A queue of animations? Blending between multiple animations etc.
    pub animation_time: f32, // This has to manually set by the game engine.
}

impl DetachedSkeletonState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, project: &Project, time: f32) {
        self.time = time;
        if self.since_first_frame() > 1.0 {
            self.animation_time = self.time;
        }
        println!(
            "{} {} {}",
            time,
            self.animation_time,
            self.since_first_frame()
        );
        self.calculate_bone_animations(project);
        self.pose(&project.skeleton);
    }

    pub fn animate(&mut self, name: &str) {
        self.animation = Some(name.to_string());
        self.animation_time = self.time;
    }

    fn since_first_frame(&self) -> f32 {
        self.time - self.animation_time
    }

    pub fn interpolate(
        &self,
        frame_1: Option<&BoneKeyframe>,
        frame_2: Option<&BoneKeyframe>,
    ) -> Option<BoneModification> {
        let f1 = if let Some(f1) = frame_1 {
            f1
        } else {
            return None;
        };

        let f1_rotation = if let BoneKeyframeData::BoneRotate(f1_rotation, _) = f1.data {
            f1_rotation.to_radians()
        } else {
            return None;
        };

        if let Some(f2) = frame_2 {
            let since_last_frame = self.since_first_frame() - f1.time;
            // trace!(?since_last_frame, "since_last_frame");
            let duration = f2.time - f1.time;
            let fraction = since_last_frame / duration;
            trace!(?fraction);
            todo!();
            // let f2_rotation = if let BoneKeyframeData::BoneRotate(f2_rotation, _) = f2.keyframe {
            //     f2_rotation.to_radians()
            // } else {
            //     return None;
            // };

            // let rotation = (f2_rotation - f1_rotation) * fraction + f1_rotation; // Linear slerp
            // Some(BoneModification {
            //     rotation: Angle::radians(rotation),
            // })
        } else {
            // Some(BoneModification {
            //     rotation: Angle::radians(f1_rotation),
            // })
            todo!()
        }
    }

    pub fn calculate_bone_animations(&mut self, project: &Project) {
        self.animation_modifications.clear();
        let animation = match &self.animation {
            None => {
                return;
            }
            Some(animation) => &project.skeleton.animations_by_name[animation.as_str()],
        };

        for animated_bone in &animation.bones {
            // TODO: Debugging only one bone.
            if animated_bone.bone_index != 3 {
                return;
            }

            let bone = &project.skeleton.bones[animated_bone.bone_index];

            // Iterate over the same timeline type (e.g. rotation) for this bone.
            let mut modifications = BoneModification::default();
            for timeline in &animated_bone.timelines {
                let keyframe_idx = timeline
                    .frames
                    .iter()
                    .position(|keyframe| keyframe.time >= self.since_first_frame());

                trace!(?keyframe_idx);

                let keyframe_idx = match keyframe_idx {
                    None => {
                        warn!(
                            "Keyframe not found for bone {}, timeline: {:?}, delta: {:?}",
                            bone.name,
                            timeline,
                            self.since_first_frame()
                        );
                        continue;
                    }
                    Some(i) => i,
                };

                let keyframe = &timeline.frames[keyframe_idx];
                let this_frame = keyframe.data.to_bone_modification();
                modifications.apply(&this_frame);

                // let frame_1 = animated_bone.keyframes.get(*bone_info_idx);
                // let frame_2 = animated_bone.keyframes.get(bone_info_idx + 1);

                // trace!(?bone_info_idx, ?frame_1, ?frame_2);

                // if let Some(bone_modification) = self.interpolate(frame_1, frame_2) {
                //     self.animation_modifications
                //         .insert(bone.name.clone(), bone_modification);
                // } else {
                //     return;
                // };
            }
            // dbg!(&modifications);

            self.animation_modifications
                .insert(bone.name.clone(), modifications);
        }
    }

    pub fn bone_state_by_name(&self, bone_name: &str) -> Option<&BoneState> {
        self.calculated_bones.get(bone_name)
    }

    pub fn bone(&self, bone_name: &str) -> Option<&BoneState> {
        self.bone_state_by_name(bone_name)
    }

    pub fn bone_rotation(&mut self, bone_name: &str, rotation: Angle) {
        self.user_modifications.insert(
            bone_name.to_string(),
            BoneModification::from_rotation(rotation),
        );
    }

    pub fn slots<'a>(&'a self, project: &'a Project) -> Vec<SlotInfo<'a>> {
        let skeleton = &project.skeleton;
        let regions = &project.atlas.pages[0].regions;
        self.slots
            .iter()
            .map(
                |(slot_id, bone_id, bone_state, skin_slot_id, attachment_id)| {
                    let slot = &skeleton.slots[*slot_id];
                    let bone = &skeleton.bones[*bone_id];
                    let skin = &skeleton.skins[0];
                    let skin_slot = skin.slots.iter().find(|s| &s.slot == skin_slot_id).unwrap(); // TODO: error

                    // TODO: Only grab the first attachment for now.
                    let attachment = &skin_slot.attachments[0];

                    let atlas_region = regions.get(&attachment.placeholder_name).unwrap(); // TODO: error

                    let atlas_region_affinity = match &attachment.data {
                        AttachmentData::Region(region_attachment) => {
                            Affine3A::from_scale_rotation_translation(
                                region_attachment.scale.extend(1.),
                                Quat::from_rotation_z(region_attachment.rotation.to_radians()),
                                region_attachment
                                    .position
                                    // The extend here is to order slots correct via the z axis.
                                    .extend(-10. + (*slot_id as f32) / 100.), // TODO: Make this less hacky.
                            )
                        }
                        _ => todo!(),
                    };

                    let affinity = bone_state.affinity
                        * atlas_region_affinity
                        * Affine3A::from_rotation_z(-atlas_region.rotate.to_radians());

                    SlotInfo {
                        slot,
                        bone,
                        bone_state,
                        skin_slot,
                        attachment,
                        atlas_index: atlas_region.order,
                        affinity,
                    }
                },
            )
            .collect()
    }

    pub fn pose(&mut self, skeleton: &Skeleton) {
        if skeleton.bones.len() == 0 {
            warn!("No bones in skeleton.");
            return;
        };

        self.pose_bone(skeleton, 0, BoneState::default());

        self.slots.clear();
        for (slot_idx, slot) in skeleton.slots.iter().enumerate() {
            let bone = &skeleton.bones[slot.bone];
            let bone_state = match self.calculated_bones.get(&bone.name) {
                Some(b) => b.clone(),
                None => {
                    warn!("Slot bone not found in bones.");
                    continue;
                }
            };
            let skin = &skeleton.skins[0]; // TODO: support multiple skins
            let skin_slot = &skin.slots.iter().find(|s| s.slot == slot_idx).unwrap(); // TODO: Skeleton HashMap
            let slot_attachment_name = match slot.attachment.as_ref() {
                Some(s) => s,
                None => {
                    // warn!("Slot attachment name not set. Assumed no attachment for set up pose.");
                    continue;
                }
            };

            let slot_attachment = skin_slot
                .attachments
                .iter()
                .enumerate()
                .find(|(_, attachment)| &attachment.attachment_name == slot_attachment_name);

            if let Some((attachment_idx, _)) = slot_attachment {
                self.slots
                    .push((slot_idx, slot.bone, bone_state, slot_idx, attachment_idx));
            } else {
                warn!("Slot attachment not found in skin.");
                continue;
            }
        }
    }

    #[instrument(skip(self, skeleton, bone_idx, parent_state))]
    fn pose_bone(&mut self, skeleton: &Skeleton, bone_idx: usize, parent_state: BoneState) {
        let bone = &skeleton.bones[bone_idx];

        let default_bone_modification = BoneModification::default();
        let bone_modification = match self.user_modifications.get(&bone.name) {
            Some(modification) => modification,
            None => &default_bone_modification,
        };

        let animated_bone_modifications = match self.animation_modifications.get(&bone.name) {
            Some(b) => b,
            None => &default_bone_modification,
        };

        let rotation = bone.rotation.to_radians()
            + bone_modification.rotation.to_radians()
            + animated_bone_modifications.rotation.to_radians();

        let (affinity, rotation) = match bone.transform {
            ParentTransform::Normal => (
                Affine3A::from_scale_rotation_translation(
                    bone.scale.extend(1.),
                    Quat::from_rotation_z(rotation),
                    bone.position.extend(0.),
                ),
                rotation,
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

        self.calculated_bones
            .insert(bone.name.to_string(), bone_state.clone());

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
    // I don't know how to extract rotation out of an Affine3A, so I'm just tracking this
    // separately.
    pub rotation: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct SlotInfo<'a> {
    pub slot: &'a Slot,
    pub bone: &'a Bone,
    pub bone_state: &'a BoneState,
    pub skin_slot: &'a SkinSlot,
    pub attachment: &'a Attachment,
    pub atlas_index: usize,
    pub affinity: Affine3A,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BinarySkeletonParser;
    use test_log::test;

    #[test]
    fn brainstorm_api() {
        let b = include_bytes!("../../assets/spineboy-ess-4.1/spineboy-ess.skel");
        let skeleton = BinarySkeletonParser::parse(b).unwrap();
        let mut state = SkeletonState::new(&skeleton);
        state.internal.animate("walk");
    }

    #[test]
    fn spineboy() {
        let b = include_bytes!("../../assets/spineboy-pro-4.1/spineboy-pro.skel");
        let skeleton = BinarySkeletonParser::parse(b).unwrap();
        let mut state = SkeletonState::new(&skeleton);
        state.pose();
        let head = state.bone("head").unwrap();
        // assert_eq!(head.rotation, 95.47044_f32.to_radians()); // TODO: Should be this??
        assert_eq!(head.rotation, 1.7179791);
        assert_eq!(head.affinity.translation.x, -23.454243);
        assert_eq!(head.affinity.translation.y, 402.30496);

        state.bone_rotation("head", Angle::Degrees(1800.));
        state.pose();
        let head = state.bone("head").unwrap();
        assert_eq!(head.rotation, 72.28667_f32.to_radians());
    }
}

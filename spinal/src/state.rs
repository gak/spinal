use crate::skeleton::{Bone, ParentTransform, Skeleton};
use bevy_math::Affine2;
use bevy_utils::HashMap;
use tracing::warn;

#[derive(Debug, Clone)]
pub struct SkeletonState<'a> {
    skeleton: &'a Skeleton,
    bones: HashMap<usize, BoneState>,
}

impl<'a> SkeletonState<'a> {
    pub fn new(skeleton: &'a Skeleton) -> Self {
        Self {
            skeleton,
            bones: HashMap::new(),
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

use crate::skeleton::{ParentTransform, Skeleton};
use bevy_math::{Affine2, Mat2, Mat3, Vec2};
use bevy_utils::HashMap;
use tracing::warn;

#[derive(Debug, Clone)]
struct SkeletonState<'a> {
    skeleton: &'a Skeleton,
    bones: HashMap<usize, BoneState>,
}

impl<'a> SkeletonState<'a> {
    fn new(skeleton: &'a Skeleton) -> Self {
        Self {
            skeleton,
            bones: HashMap::new(),
        }
    }

    pub fn pose(&mut self) {
        if self.skeleton.bones.len() == 0 {
            warn!("No bones in skeleton.");
            return;
        };

        self.pose_bone(0, Affine2::IDENTITY);
    }

    fn pose_bone(&mut self, bone_idx: usize, parent_affinity: Affine2) {
        let bone = &self.skeleton.bones[bone_idx];
        if bone.shear.x != 0.0 || bone.shear.y != 0.0 {
            warn!("Shearing is not supported yet.");
        }
        let affinity = match bone.transform {
            ParentTransform::Normal => Affine2::from_scale_angle_translation(
                bone.scale,
                bone.rotation.to_radians(),
                bone.position,
            ),
            _ => return, // TODO!
        };
        let affinity = parent_affinity * affinity;
        self.bones.insert(bone_idx, BoneState { affinity });
        println!("Bone: {} {:?} {:?}", bone_idx, bone.name, affinity);

        if let Some(children) = self.skeleton.bones_tree.get(&bone_idx) {
            for child_idx in children {
                self.pose_bone(*child_idx, affinity);
            }
        }
    }
}

#[derive(Debug, Default, Clone)]
struct BoneState {
    affinity: Affine2,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{json, BinaryParser};

    #[test]
    fn spineboy() {
        let b = include_bytes!("../../assets/spineboy-pro-4.1/spineboy-pro.skel");
        let skeleton = BinaryParser::parse(b).unwrap();
        let mut state = SkeletonState::new(&skeleton);
        state.pose();
        todo!();
    }
}

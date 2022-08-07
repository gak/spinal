use crate::skeleton::Skeleton;
use bevy_math::Mat2;
use bevy_utils::HashMap;

struct SkeletonState {
    bones: HashMap<usize, BoneState>,
}

impl SkeletonState {
    fn new() -> Self {
        Self {
            bones: HashMap::new(),
        }
    }

    fn calculate(&mut self, skeleton: &Skeleton) {
        if skeleton.bones.len() == 0 {
            return;
        };

        self.calculate_bone(skeleton, 0);
    }

    fn calculate_bone(&mut self, skeleton: &Skeleton, bone_idx: usize) {
        // for child_bone in skeleton.bones.iter().filter(|b| b.parent == bone_idx) {
        //     dbg!(child_bone);
        // }
        todo!()
    }
}

struct BoneState {
    transform: Mat2,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    // #[test]
    // fn spineboy() {
    //     let b = include_bytes!("../../assets/spineboy-pro-4.1/spineboy-pro.json");
    //     let skel = json::parse(b).unwrap();
    //     let mut state = SkeletonState::new();
    //     state.calculate(&skel);
    //     todo!();
    // }
}

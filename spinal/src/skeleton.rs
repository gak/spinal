mod animation;
mod attachment;
mod bone;
mod event;
mod ik;
mod info;
mod path;
mod skin;
mod slot;
mod transform;

pub use animation::{
    AnimatedBone, AnimatedSlot, Animation, BezierCurve, BoneKeyframe, BoneKeyframeData,
    BoneKeyframeType, Curve, OptionCurve, SlotKeyframe,
};
pub use attachment::*;
use bevy_utils::HashMap;
pub use bone::{Bone, ParentTransform};
pub use event::Event;
pub use ik::Ik;
pub use info::Info;
pub use path::{Path, PathPositionMode, PathRotateMode, PathSpacingMode};
pub use skin::{Skin, SkinSlot};
pub use slot::{Blend, Slot};
pub use transform::Transform;

#[derive(Debug, Default)]
pub struct Skeleton {
    pub info: Info,
    pub strings: Vec<String>,
    pub bones: Vec<Bone>,
    /// <ParentBone, Vec<ChildBone>>
    pub bones_tree: HashMap<usize, Vec<usize>>,
    pub bone_by_name: HashMap<String, usize>,
    pub slots: Vec<Slot>,
    pub ik: Vec<Ik>,
    pub transforms: Vec<Transform>,
    pub paths: Vec<Path>,
    pub skins: Vec<Skin>,
    pub events: Vec<Event>,
    pub animations: Vec<Animation>,
    pub animations_by_name: HashMap<String, Animation>,
}

impl Skeleton {
    pub fn build_bones_tree(&self) -> HashMap<usize, Vec<usize>> {
        let mut bones_tree = HashMap::with_capacity(self.bones.len());
        for (index, bone) in self.bones.iter().enumerate() {
            if let Some(parent_index) = bone.parent {
                bones_tree
                    .entry(parent_index)
                    .or_insert_with(Vec::new)
                    .push(index);
            }
        }
        bones_tree
    }
}

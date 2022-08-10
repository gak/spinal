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

pub use animation::Animation;
pub use attachment::*;
pub use bone::{Bone, ParentTransform};
pub use event::Event;
pub use ik::Ik;
pub use info::Info;
pub use path::{Path, PathPositionMode, PathRotateMode, PathSpacingMode};
pub use skin::Skin;
pub use slot::{Blend, Slot};
pub use transform::Transform;

#[derive(Debug, Default)]
pub struct Skeleton {
    pub info: Info,
    pub strings: Vec<String>,
    pub bones: Vec<Bone>,
    pub slots: Vec<Slot>,
    pub ik: Vec<Ik>,
    pub transforms: Vec<Transform>,
    pub paths: Vec<Path>,
    pub skins: Vec<Skin>,
    pub events: Vec<Event>,
    pub animations: Vec<Animation>,
}

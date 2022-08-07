mod attachment;
mod bone;
mod ik;
mod info;
mod skin;
mod slot;

pub use attachment::Attachment;
pub use bone::Bone;
pub use ik::Ik;
pub use info::Info;
pub use skin::Skin;
pub use slot::Slot;

#[derive(Debug)]
pub struct Skeleton {
    pub info: Info,
    pub bones: Vec<Bone>,
    pub slots: Vec<Slot>,
    pub ik: Vec<Ik>,
    pub skins: Vec<Skin>,
}

pub use info::Info;
pub use bone::Bone;

mod info;
mod bone;

pub struct Skeleton {
    pub skeleton: Info,
    pub bones: Vec<Bone>,
}


use bevy_math;
use skeleton::Skeleton;

mod skeleton;

pub struct Spinal {
    pub skeletons: Vec<Skeleton>,
    pub name: String,
}

pub enum SpineVersion {
    V4_1,
}

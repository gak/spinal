use crate::bone::Bone;
use crate::info::Info;

mod json;
mod binary;
mod info;
mod bone;

// #[derive(thiserror::Error)]
struct SpinalError {

}

struct Skeleton {
    skeleton: Info,
    bones: Vec<Bone>
}



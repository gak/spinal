use std::string::FromUtf8Error;
use crate::bone::Bone;
use crate::info::Info;

mod json;
mod binary;
mod info;
mod bone;

#[derive(thiserror::Error, Debug)]
pub enum SpinalError {
    #[error("Invalid UTF8 String.")]
    InvalidUtf8String(#[source] FromUtf8Error),
}

struct Skeleton {
    skeleton: Info,
    bones: Vec<Bone>
}



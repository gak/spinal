use crate::bone::Bone;
use crate::info::Info;
use std::string::FromUtf8Error;

mod binary;
mod bone;
mod info;
mod json;

#[derive(thiserror::Error, Debug)]
pub enum SpinalError {
    // #[error("Failed to parse binary skeleton file.")]
    // BinaryParseError(#[source] nom::Err),
    #[error("Invalid UTF8 String.")]
    InvalidUtf8String(#[source] FromUtf8Error),
}

struct Skeleton {
    skeleton: Info,
    bones: Vec<Bone>,
}

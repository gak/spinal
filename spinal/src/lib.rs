use crate::bone::Bone;
use crate::info::Info;
use serde::Deserialize;
use std::string::FromUtf8Error;

mod binary;
mod bone;
mod info;

#[derive(thiserror::Error, Debug)]
pub enum SpinalError {
    // #[error("Failed to parse binary skeleton file.")]
    // BinaryParseError(#[source] nom::Err),
    #[error("Invalid UTF8 String.")]
    InvalidUtf8String(#[source] FromUtf8Error),
}

#[derive(Debug, Deserialize)]
pub struct Skeleton {
    #[serde(rename = "skeleton")]
    info: Info,
    bones: Vec<Bone>,
}

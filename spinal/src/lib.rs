use crate::bone::{Bone, Color};
use crate::info::Info;
use serde::Deserialize;
use slot::Slot;
use std::string::FromUtf8Error;

mod binary;
mod bone;
mod info;
mod json;
mod slot;

#[derive(thiserror::Error, Debug)]
pub enum SpinalError {
    // #[error("Failed to parse binary skeleton file.")]
    // BinaryParseError(#[source] nom::Err),
    #[error("Invalid UTF8 String.")]
    InvalidUtf8String(#[source] FromUtf8Error),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Skeleton {
    #[serde(rename = "skeleton")]
    info: Info,
    bones: Vec<Bone>,
    slots: Vec<Slot>,
}

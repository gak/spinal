pub mod binary;
mod color;
pub mod json;
pub mod skeleton;
mod state;

use bevy_math::Mat2;
use bevy_utils::HashMap;
pub use skeleton::Skeleton;
use std::string::FromUtf8Error;

#[derive(thiserror::Error, Debug)]
pub enum SpinalError {
    // #[error("Failed to parse binary skeleton file.")]
    // BinaryParseError(#[source] nom::Err),
    #[error("Invalid UTF8 String.")]
    InvalidUtf8String(#[source] FromUtf8Error),

    /// When a bone is referencing a bone that doesn't exist.
    #[error("Bad bone reference: {0}")]
    BadBoneReference(String),

    #[error("Bad attachment string reference: {0}")]
    BadAttachmentStringReference(usize),
}

mod binary;
mod color;
mod json;
pub mod skeleton;
mod state;

pub use binary::BinaryParser;
pub use skeleton::Skeleton;
pub use state::SkeletonState;
use std::string::FromUtf8Error;

#[derive(thiserror::Error, Debug)]
pub enum SpinalError {
    // #[error("Failed to parse binary skeleton file.")]
    // BinaryParseError(#[source] nom::Err),
    #[error("Invalid UTF8 String.")]
    InvalidUtf8String(#[source] FromUtf8Error),

    /// When a bone is referencing a bone that doesn't exist.
    #[error("Invalid bone reference: {0}")]
    InvalidBoneReference(String),

    #[error("Invalid attachment string reference: {0}")]
    InvalidAttachmentStringReference(usize),

    #[error("Invalid string index: {0}")]
    InvalidStringIndex(usize),
}

mod atlas;
mod binary;
mod color;
mod json;
mod project;
pub mod skeleton;
mod state;

pub use project::Project;
pub use atlas::parser::AtlasParser;
pub use atlas::{Atlas, AtlasPage, AtlasRegion, Rect};
pub use binary::BinarySkeletonParser;
pub use skeleton::Skeleton;
pub use state::{DetachedSkeletonState, SkeletonState};
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

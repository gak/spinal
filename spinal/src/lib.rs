mod atlas;
mod binary;
mod color;
mod json;
mod project;
pub mod skeleton;
mod state;

use std::ops::AddAssign;
pub use atlas::parser::AtlasParser;
pub use atlas::{Atlas, AtlasPage, AtlasRegion, Rect};
pub use binary::BinarySkeletonParser;
pub use project::Project;
pub use skeleton::Skeleton;
pub use state::{BoneModification, DetachedSkeletonState, SkeletonState};
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

#[derive(Debug, Clone, Copy)]
pub enum Angle {
    Radians(f32),
    Degrees(f32),
}

impl Default for Angle {
    fn default() -> Self {
        Angle::Radians(0.0)
    }
}

impl Angle {
    pub fn radians(a: f32) -> Self {
        Angle::Radians(a)
    }

    pub fn degrees(a: f32) -> Self {
        Angle::Degrees(a)
    }

    pub fn to_degrees(&self) -> f32 {
        match self {
            Angle::Degrees(degrees) => *degrees,
            Angle::Radians(radians) => radians.to_degrees(),
        }
    }

    pub fn to_radians(&self) -> f32 {
        match self {
            Angle::Degrees(degrees) => degrees.to_radians(),
            Angle::Radians(radians) => *radians,
        }
    }
}

impl AddAssign<Angle> for Angle {
    fn add_assign(&mut self, rhs: Angle) {
        match (&self, rhs) {
            (Angle::Degrees(a), Angle::Degrees(b)) => *self = Angle::Degrees(*a + b),
            (Angle::Degrees(a), Angle::Radians(b)) => *self = Angle::Degrees(*a + b.to_degrees()),
            (Angle::Radians(a), Angle::Degrees(b)) => *self = Angle::Radians(*a + b.to_radians()),
            (Angle::Radians(a), Angle::Radians(b)) => *self = Angle::Radians(*a + b),
        }
    }
}
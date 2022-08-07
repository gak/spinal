mod attachment;
pub mod binary;
mod bone;
mod ik;
mod info;
pub mod json;
mod skin;
mod slot;
mod state;

use crate::skin::Skin;
use attachment::Attachment;
use bevy_math::Mat2;
use bevy_utils::HashMap;
use bone::{Bone, Color};
use ik::Ik;
use info::Info;
use serde::Deserialize;
use slot::Slot;
use std::string::FromUtf8Error;

#[derive(thiserror::Error, Debug)]
pub enum SpinalError {
    // #[error("Failed to parse binary skeleton file.")]
    // BinaryParseError(#[source] nom::Err),
    #[error("Invalid UTF8 String.")]
    InvalidUtf8String(#[source] FromUtf8Error),
}

#[derive(Debug, Deserialize)]
// #[serde(deny_unknown_fields)]
pub struct Skeleton {
    #[serde(rename = "skeleton")]
    pub info: Info,
    pub bones: Vec<Bone>,
    pub slots: Vec<Slot>,
    pub ik: Vec<Ik>,
    pub skins: Vec<Skin>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Reference {
    Index(usize),
    Name(String),
}

/// A helper for serde default.
pub(crate) fn f32_one() -> f32 {
    1.0
}

/// A helper for serde default.
pub(crate) fn default_true() -> bool {
    true
}

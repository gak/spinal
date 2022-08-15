use crate::{Atlas, AtlasParser, BinarySkeletonParser, Skeleton, SpinalError};
use std::path::Path;

#[derive(Debug)]
pub struct Project {
    pub skeleton: Skeleton,
    pub atlas: Atlas,
}

impl Project {
    pub fn new(skeleton: Skeleton, atlas: Atlas) -> Self {
        Self { skeleton, atlas }
    }
}

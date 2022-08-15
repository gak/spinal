use bevy::prelude::*;

#[derive(Component)]
pub struct SkeletonRoot(pub Entity);

#[derive(Component, Default, Debug)]
pub struct SpinalState(pub spinal::DetachedSkeletonState);

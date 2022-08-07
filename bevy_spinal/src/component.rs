use bevy::prelude::*;

/// Attached to an entity when the skeleton is ready to be set up.
///
/// It should be removed once the entity is ready to run the main system.
#[derive(Component)]
pub struct SkeletonReady;

#[derive(Component)]
pub struct Ready;

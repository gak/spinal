use bevy::prelude::*;
use bevy::utils::HashMap;

#[derive(Component, Default, Debug)]
pub struct SpinalState {
    pub state: spinal::DetachedSkeletonState,
    pub children: HashMap<String, Entity>,
}

#[derive(Component)]
pub struct SpinalChild;

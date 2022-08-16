use bevy::prelude::*;

#[derive(Component, Default, Debug)]
pub struct SpinalState {
    pub state: spinal::DetachedSkeletonState,
    pub slots: Vec<Entity>,
}

#[derive(Component)]
pub struct SpinalChild {
    // slot: String,
}

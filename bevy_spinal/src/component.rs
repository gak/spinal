use bevy::prelude::*;
use bevy::utils::HashMap;

#[derive(Component, Default, Debug)]
pub struct SpinalState {
    pub state: spinal::DetachedSkeletonState,
    pub children: HashMap<String, Entity>,
}

impl SpinalState {
    pub fn animate(name: &str) -> Self {
        let mut state = spinal::DetachedSkeletonState::new();
        state.animate(name);
        Self {
            state,
            ..Default::default()
        }
    }
}

#[derive(Component)]
pub struct SpinalChild;

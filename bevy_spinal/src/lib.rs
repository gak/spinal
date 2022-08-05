use bevy::prelude::*;
use spinal::Spinal;

pub struct SpinalPlugin {

}

impl Plugin for SpinalPlugin {
    fn build(&self, app: &mut App) {
        todo!()
    }

    fn name(&self) -> &str {
        "SpinalPlugin"
    }
}

pub struct SpinalBundle {
    pub spinal: Spinal,
    pub transform: Transform,
    pub global_transform: GlobalTransform,
}
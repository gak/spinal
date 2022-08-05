use bevy::prelude::*;
use spinal::Project;

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
    pub spinal: Project,
    pub transform: Transform,
    pub global_transform: GlobalTransform,
}
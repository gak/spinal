use crate::loader::SpinalBinaryLoader;
pub use crate::loader::SpinalProject;
use crate::system::{ensure_and_transform, set_state_to_pose_on_init};
use bevy::prelude::*;
use bevy_prototype_lyon::plugin::ShapePlugin;
pub use component::SpinalState;

mod component;
mod loader;
mod system;

pub struct SpinalPlugin;

impl Plugin for SpinalPlugin {
    fn build(&self, app: &mut App) {
        // bevy_prototype_lyon for rendering bones
        // TODO: feature
        app.add_plugin(ShapePlugin);

        app.add_asset_loader(SpinalBinaryLoader {})
            .add_asset::<SpinalProject>();

        app.add_system(set_state_to_pose_on_init)
            .add_system(ensure_and_transform);
    }

    fn name(&self) -> &str {
        "SpinalPlugin"
    }
}

#[derive(Debug, Default, Bundle)]
pub struct SpinalBundle {
    pub skeleton: Handle<SpinalProject>,
    pub state: SpinalState,
    pub transform: Transform,
    pub global_transform: GlobalTransform,
}

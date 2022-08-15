use crate::component::SpinalState;
use crate::loader::{SpinalBinaryLoader, SpinalSkeleton};
use bevy::prelude::*;
use bevy::sprite::Rect;
use bevy_prototype_lyon::plugin::ShapePlugin;
use spinal::AtlasRegion;
use std::mem::swap;

mod component;
mod loader;
mod system;

pub struct SpinalPlugin {}

impl Default for SpinalPlugin {
    fn default() -> Self {
        Self {}
    }
}

impl Plugin for SpinalPlugin {
    fn build(&self, app: &mut App) {
        // bevy_prototype_lyon for rendering bones
        app.add_plugin(ShapePlugin);

        app.add_asset_loader(SpinalBinaryLoader {});
        app.add_asset::<SpinalSkeleton>();
    }

    fn name(&self) -> &str {
        "SpinalPlugin"
    }
}

#[derive(Debug, Default, Bundle)]
pub struct SpinalBundle {
    pub skeleton: Handle<SpinalSkeleton>,
    pub state: SpinalState,
    pub transform: Transform,
    pub global_transform: GlobalTransform,
}

fn atlas_to_bevy_rect(atlas_region: &AtlasRegion) -> Rect {
    let mut bounds = atlas_region.bounds.as_ref().unwrap().clone();

    // When rotated, the width and height are flipped to the final size, not the size in the atlas.
    if atlas_region.rotate == 90. {
        swap(&mut bounds.size.x, &mut bounds.size.y);
    }

    Rect {
        min: bounds.position,
        max: bounds.position + bounds.size,
    }
}

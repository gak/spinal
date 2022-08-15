use crate::loader::{SpinalAtlas, SpinalAtlasLoader, SpinalBinaryLoader, SpinalSkeleton};
use crate::system::{set_skeletons_to_ready, setup, testing};
use bevy::asset::{AssetLoader, BoxedFuture, Error, LoadContext, LoadedAsset};
use bevy::ecs::component::{ComponentId, Components};
use bevy::ecs::storage::Storages;
use bevy::prelude::*;
use bevy::ptr::OwningPtr;
use bevy::sprite::{MaterialMesh2dBundle, Rect};
use bevy_prototype_lyon::plugin::ShapePlugin;
use spinal::{AtlasRegion, Skeleton};
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

        app.add_asset_loader(SpinalAtlasLoader {});
        app.add_asset::<SpinalAtlas>();

        app.add_system(set_skeletons_to_ready);
        app.add_system(setup);
    }

    fn name(&self) -> &str {
        "SpinalPlugin"
    }
}

#[derive(Debug, Default, Bundle)]
pub struct SpinalBundle {
    pub skeleton: Handle<SpinalSkeleton>,
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

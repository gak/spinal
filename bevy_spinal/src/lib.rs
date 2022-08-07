use crate::system::instance;
use bevy::asset::{AssetLoader, BoxedFuture, Error, LoadContext, LoadedAsset};
use bevy::ecs::component::{ComponentId, Components};
use bevy::ecs::storage::Storages;
use bevy::prelude::*;
use bevy::ptr::OwningPtr;
use bevy::reflect::TypeUuid;
use bevy::sprite::MaterialMesh2dBundle;
use loader::SpinalJsonLoader;
use spinal::Skeleton;

mod loader;
mod system;

/// Newtype `spinal::Skeleton` so we can use it as a Bevy asset.
#[derive(Debug, TypeUuid)]
#[uuid = "1127f13d-56a3-4471-a565-bb3bac35ba0a"]
pub struct SkeletonAsset(Skeleton);

pub struct SpinalPlugin {
    json_extension: String,
    binary_extension: String,
    atlas_extension: String,
    png_extension: String,
}

impl Default for SpinalPlugin {
    fn default() -> Self {
        Self {
            json_extension: "json".to_string(),
            binary_extension: "skel".to_string(),
            atlas_extension: "atlas".to_string(),
            png_extension: "png".to_string(),
        }
    }
}

impl Plugin for SpinalPlugin {
    fn build(&self, app: &mut App) {
        app.add_asset_loader(SpinalJsonLoader {
            extension: self.json_extension.clone(),
        })
        .add_asset::<SkeletonAsset>()
        // .add_system(instance);
        ;
    }

    fn name(&self) -> &str {
        "SpinalPlugin"
    }
}

#[derive(Debug, Default, Bundle)]
pub struct SpinalBundle {
    pub skeleton: Handle<SkeletonAsset>,
    pub transform: Transform,
    pub global_transform: GlobalTransform,
}

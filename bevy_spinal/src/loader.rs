use crate::{AssetLoader, BoxedFuture, LoadContext, LoadedAsset};
use anyhow::{Context, Error};
use bevy::asset::AssetPath;
use bevy::reflect::TypeUuid;
use spinal::{Atlas, AtlasParser, Skeleton};

/// Newtype `spinal::Skeleton` so we can use it as a Bevy asset.
#[derive(Debug, TypeUuid)]
#[uuid = "1127f13d-56a3-4471-a565-bb3bac35ba0a"]
pub struct SpinalSkeleton(pub Skeleton);

pub struct SpinalBinaryLoader {}

impl AssetLoader for SpinalBinaryLoader {
    fn load<'a>(
        &'a self,
        bytes: &'a [u8],
        load_context: &'a mut LoadContext,
    ) -> BoxedFuture<'a, anyhow::Result<(), Error>> {
        Box::pin(async move {
            let skeleton = spinal::BinaryParser::parse(bytes)
                .with_context(|| format!("Failed to load skeleton: {:?}", load_context.path()))?;

            let atlas_path = AssetPath::from(load_context.path().with_extension("atlas"));
            let asset = LoadedAsset::new(SpinalSkeleton(skeleton)).with_dependency(atlas_path);
            load_context.set_default_asset(asset);

            Ok(())
        })
    }

    fn extensions(&self) -> &[&str] {
        &["skel"]
    }
}

#[derive(Debug, TypeUuid)]
#[uuid = "a33de84b-593d-4dc1-b7f8-63f8727603fd"]
pub struct SpinalAtlas(pub Atlas);

pub struct SpinalAtlasLoader {}

impl AssetLoader for SpinalAtlasLoader {
    fn load<'a>(
        &'a self,
        bytes: &'a [u8],
        load_context: &'a mut LoadContext,
    ) -> BoxedFuture<'a, anyhow::Result<(), Error>> {
        Box::pin(async move {
            println!("Loading atlas as dep!");
            let ctx = || format!("Loading {:?}", load_context.path());
            let s = std::str::from_utf8(bytes).with_context(ctx)?;
            let atlas = AtlasParser::parse(s).with_context(ctx)?;
            let atlas = LoadedAsset::new(SpinalAtlas(atlas));
            load_context.set_default_asset(atlas);
            Ok(())
        })
    }

    fn extensions(&self) -> &[&str] {
        &["atlas"]
    }
}

use crate::atlas_to_bevy_rect;
use anyhow::{Context, Error};
use bevy::asset::{AssetLoader, AssetPath, LoadContext, LoadedAsset};
use bevy::prelude::*;
use bevy::reflect::TypeUuid;
use bevy::utils::{BoxedFuture, HashMap};
use spinal::{Atlas, AtlasParser, AtlasRegion, Skeleton};
use std::path::Path;

/// Newtype `spinal::Skeleton` so we can use it as a Bevy asset.
#[derive(Debug, TypeUuid)]
#[uuid = "1127f13d-56a3-4471-a565-bb3bac35ba0a"]
pub struct SpinalSkeleton {
    pub skeleton: Skeleton,
    pub atlas: Handle<TextureAtlas>,
    pub lookup: HashMap<String, (usize, AtlasRegion)>,
}

pub struct SpinalBinaryLoader {}

impl AssetLoader for SpinalBinaryLoader {
    fn load<'a>(
        &'a self,
        bytes: &'a [u8],
        load_context: &'a mut LoadContext,
    ) -> BoxedFuture<'a, anyhow::Result<(), Error>> {
        Box::pin(async move { load(load_context, bytes).await })
    }

    fn extensions(&self) -> &[&str] {
        &["skel"]
    }
}

async fn load<'a, 'b>(
    load_context: &'a mut LoadContext<'b>,
    bytes: &'a [u8],
) -> anyhow::Result<(), Error> {
    let skeleton = spinal::BinaryParser::parse(bytes)
        .with_context(|| format!("Failed to load skeleton: {:?}", load_context.path()))?;

    let atlas_path = load_context.path().with_extension("atlas");
    let (lookup, atlas) = load_atlas(load_context, &atlas_path)
        .await
        .with_context(|| format!("{:?}", atlas_path))?;

    let spinal_skeleton = SpinalSkeleton {
        skeleton,
        atlas,
        lookup,
    };
    load_context.set_default_asset(LoadedAsset::new(spinal_skeleton));

    Ok(())
}

// .atlas file will be loaded then discarded after loading the TextureAtlas.
async fn load_atlas(
    load_context: &mut LoadContext<'_>,
    path: &Path,
) -> anyhow::Result<(HashMap<String, (usize, AtlasRegion)>, Handle<TextureAtlas>), Error> {
    let bytes = load_context.read_asset_bytes(path).await?;
    let s = std::str::from_utf8(bytes.as_slice())?;
    let atlas = AtlasParser::parse(s)?;

    let texture_path = AssetPath::new(path.with_extension("png"), None); // TODO: Label
    let texture_handle = load_context.get_handle(texture_path);

    // TODO: Support multiple pages
    let page = &atlas.pages[0];
    let mut texture_atlas = TextureAtlas::new_empty(texture_handle, page.header.size);
    let mut name_to_atlas = HashMap::new();
    for (index, region) in page.regions.iter().enumerate() {
        let rect = atlas_to_bevy_rect(&region);
        texture_atlas.add_texture(rect);
        dbg!(region.name.as_str(), &region.bounds, &region.offsets);
        name_to_atlas.insert(region.name.clone(), (index, region.clone()));
    }

    Ok((
        name_to_atlas,
        load_context.set_labeled_asset("atlas", LoadedAsset::new(texture_atlas)),
    ))
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

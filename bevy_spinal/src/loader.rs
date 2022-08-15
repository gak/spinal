use anyhow::{Context, Error};
use bevy::asset::{AssetLoader, AssetPath, LoadContext, LoadedAsset};
use bevy::prelude::*;
use bevy::reflect::TypeUuid;
use bevy::sprite::Rect;
use bevy::utils::{BoxedFuture, HashMap};
use spinal::{AtlasParser, AtlasRegion};
use std::mem::swap;
use std::path::Path;

/// Newtype `spinal::Project` so we can use it as a Bevy asset.
///
/// It also includes a loaded [TextureAtlas] for rendering with [Sprite]`s.
#[derive(Debug, TypeUuid)]
#[uuid = "1127f13d-56a3-4471-a565-bb3bac35ba0a"]
pub struct SpinalProject {
    pub project: spinal::Project,
    pub atlas: Handle<TextureAtlas>,
}

pub struct SpinalBinaryLoader;

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
    let skeleton = spinal::BinarySkeletonParser::parse(bytes)
        .with_context(|| format!("Failed to load skeleton: {:?}", load_context.path()))?;

    let atlas_path = load_context.path().with_extension("atlas");
    let (spinal_atlas, bevy_atlas) = load_atlas(load_context, &atlas_path)
        .await
        .with_context(|| format!("{:?}", atlas_path))?;
    let project = spinal::Project::new(skeleton, spinal_atlas);

    let spinal_skeleton = SpinalProject {
        project,
        atlas: bevy_atlas,
    };
    load_context.set_default_asset(LoadedAsset::new(spinal_skeleton));

    Ok(())
}

async fn load_atlas(
    load_context: &mut LoadContext<'_>,
    path: &Path,
) -> anyhow::Result<(spinal::Atlas, Handle<TextureAtlas>), Error> {
    let bytes = load_context.read_asset_bytes(path).await?;
    let s = std::str::from_utf8(bytes.as_slice())?;
    let atlas = AtlasParser::parse(s)?;

    let texture_path = AssetPath::new(path.with_extension("png"), None); // TODO: Label
    let texture_handle = load_context.get_handle(texture_path);

    // TODO: Support multiple pages
    let page = &atlas.pages[0];
    let mut texture_atlas = TextureAtlas::new_empty(texture_handle, page.header.size);
    for region in page.regions.values() {
        let rect = spinal_to_bevy_rect(&region);
        texture_atlas.add_texture(rect);
    }

    Ok((
        atlas,
        load_context.set_labeled_asset("atlas", LoadedAsset::new(texture_atlas)),
    ))
}

fn spinal_to_bevy_rect(atlas_region: &AtlasRegion) -> Rect {
    let mut bounds = atlas_region.bounds.as_ref().unwrap().clone(); // TODO: error

    // When rotated, the width and height are flipped to the final size, not the size in the atlas.
    if atlas_region.rotate == 90. {
        swap(&mut bounds.size.x, &mut bounds.size.y);
    }

    Rect {
        min: bounds.position,
        max: bounds.position + bounds.size,
    }
}

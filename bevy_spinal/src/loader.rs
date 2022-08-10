use crate::{AssetLoader, BoxedFuture, LoadContext, LoadedAsset, SkeletonAsset};
use anyhow::Error;

pub struct SpinalBinaryLoader {
    pub extension: String,
}

impl AssetLoader for SpinalBinaryLoader {
    fn load<'a>(
        &'a self,
        bytes: &'a [u8],
        load_context: &'a mut LoadContext,
    ) -> BoxedFuture<'a, anyhow::Result<(), Error>> {
        Box::pin(async move {
            let skeleton = spinal::BinaryParser::parse(bytes)?;
            load_context.set_default_asset(LoadedAsset::new(SkeletonAsset(skeleton)));
            Ok(())
        })
    }

    fn extensions(&self) -> &[&str] {
        // TODO: Use settings.
        &["skel"]
    }
}

pub struct SpinalJsonLoader {
    pub extension: String,
}

impl AssetLoader for SpinalJsonLoader {
    fn load<'a>(
        &'a self,
        bytes: &'a [u8],
        load_context: &'a mut LoadContext,
    ) -> BoxedFuture<'a, anyhow::Result<(), Error>> {
        Box::pin(async move {
            let skeleton = spinal::json::parse(bytes)?;
            load_context.set_default_asset(LoadedAsset::new(SkeletonAsset(skeleton)));
            Ok(())
        })
    }

    fn extensions(&self) -> &[&str] {
        // TODO: Use settings.
        &["json"]
    }
}

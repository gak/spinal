use bevy::{
    app::{App, Plugin, Update},
    asset::{AssetApp, AssetServer},
    ecs::schedule::{IntoScheduleConfigs, SystemSet},
    image::ImagePlugin,
};

use crate::{
    SpinalAnimationEvent, SpinalAsset, SpinalAssetLoader, SpinalIssue, SpinalRuntimeConfig,
    runtime::{cleanup_removed_instances, prepare_instances, update_instances},
};

/// Stable integration points around Spinal's public update pipeline.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, SystemSet)]
pub enum SpinalSet {
    /// Creates or atomically rebuilds runtime instances from loaded assets.
    Prepare,
    /// Applies public intent, advances playback, solves, and owns frame data.
    Animate,
    /// Synchronizes solved frame data with the active rendering backend.
    Render,
}

/// Asset, playback, diagnostics, and optional rendering integration for Bevy
/// 0.18.
#[derive(Clone, Copy, Debug, Default)]
pub struct SpinalPlugin;

impl Plugin for SpinalPlugin {
    fn build(&self, app: &mut App) {
        assert!(
            app.world().contains_resource::<AssetServer>(),
            "SpinalPlugin requires Bevy's AssetPlugin to be added first"
        );

        if !app.is_plugin_added::<ImagePlugin>() {
            app.add_plugins(ImagePlugin::default());
        }

        app.init_asset::<SpinalAsset>()
            .register_asset_loader(SpinalAssetLoader)
            .init_resource::<SpinalRuntimeConfig>()
            .add_message::<SpinalAnimationEvent>()
            .add_message::<SpinalIssue>()
            .configure_sets(
                Update,
                (SpinalSet::Prepare, SpinalSet::Animate, SpinalSet::Render).chain(),
            )
            .add_systems(
                Update,
                (
                    (cleanup_removed_instances, prepare_instances)
                        .chain()
                        .in_set(SpinalSet::Prepare),
                    update_instances.in_set(SpinalSet::Animate),
                ),
            );

        #[cfg(feature = "render")]
        crate::render::install_render(app);
    }
}

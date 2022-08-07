use crate::SkeletonAsset;
use bevy::prelude::*;

pub fn instance(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    // mut asset_loader: ResMut<AssetsLoading>,
    mut asset_events: EventReader<SkeletonAsset>,
) {
}

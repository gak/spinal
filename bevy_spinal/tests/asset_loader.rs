//! Compound asset loading and hot-reload integration tests.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

use bevy::{
    app::TaskPoolPlugin,
    asset::{
        AssetApp, AssetPlugin, AssetServer, Assets, LoadState,
        io::{
            AssetSourceBuilder, AssetSourceEvent, AssetSourceId, AssetWatcher,
            memory::{Dir, MemoryAssetReader},
        },
    },
    image::{Image, ImageAddressMode, ImageFilterMode, ImagePlugin, ImageSampler},
    prelude::App,
};
use bevy_spinal::{SpinalAsset, SpinalAssetLoader, SpinalAssetLoaderSettings};

const SKELETON_JSON: &str = r#"{
  "skeleton": { "spine": "4.3.23" },
  "bones": [{ "name": "root" }]
}"#;

const NEAREST_ATLAS: &str = "\
cat.png
	size: 1, 1
	filter: Nearest, Linear
	repeat: x
";

const LINEAR_ATLAS: &str = "\
cat.png
	size: 1, 1
	filter: Linear, Linear
	repeat: xy
";

const PIXEL_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 4, 0,
    0, 0, 181, 28, 12, 2, 0, 0, 0, 11, 73, 68, 65, 84, 120, 218, 99, 100, 248, 15, 0, 1, 5, 1, 1,
    39, 24, 227, 102, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

struct TestWatcher;

impl AssetWatcher for TestWatcher {}

#[test]
fn typed_plain_json_load_can_select_an_explicit_relative_atlas() {
    let files = Dir::default();
    files.insert_asset_text(Path::new("plain.json"), SKELETON_JSON);
    files.insert_asset_text(
        Path::new("styles/plain.atlas"),
        "\
../plain.png
	size: 1, 1
",
    );
    files.insert_asset(Path::new("plain.png"), PIXEL_PNG.to_vec());

    let memory_reader = MemoryAssetReader { root: files };
    let mut app = App::new();
    app.register_asset_source(
        AssetSourceId::Default,
        AssetSourceBuilder::new(move || Box::new(memory_reader.clone())),
    )
    .add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin {
            watch_for_changes_override: Some(false),
            use_asset_processor_override: Some(false),
            ..Default::default()
        },
        ImagePlugin::default(),
    ))
    .init_asset::<SpinalAsset>()
    .register_asset_loader(SpinalAssetLoader);

    let asset_server = app.world().resource::<AssetServer>().clone();
    let handle = asset_server.load_with_settings::<SpinalAsset, SpinalAssetLoaderSettings>(
        "plain.json",
        |settings| {
            settings.atlas_path = Some("styles/plain.atlas".to_owned());
        },
    );

    update_until(&mut app, |app| match asset_server.load_state(&handle) {
        LoadState::Failed(error) => {
            panic!("typed plain JSON load failed: {error}")
        }
        LoadState::Loaded => app
            .world()
            .resource::<Assets<SpinalAsset>>()
            .get(&handle)
            .is_some(),
        LoadState::NotLoaded | LoadState::Loading => false,
    });

    let assets = app.world().resource::<Assets<SpinalAsset>>();
    let asset = assets.get(&handle).expect("plain JSON asset is retained");
    assert_eq!(
        asset
            .page(0)
            .expect("one page")
            .source_path()
            .map(ToString::to_string)
            .as_deref(),
        Some("plain.png")
    );
}

#[test]
fn compound_load_and_atlas_hot_reload_are_atomic_and_keep_page_handles_stable() {
    let files = Dir::default();
    files.insert_asset_text(Path::new("cat.spine.json"), SKELETON_JSON);
    files.insert_asset_text(Path::new("cat.atlas"), NEAREST_ATLAS);
    files.insert_asset(Path::new("cat.png"), PIXEL_PNG.to_vec());

    let memory_reader = MemoryAssetReader {
        root: files.clone(),
    };
    let (watcher_sender, watcher_receiver) = mpsc::sync_channel(1);

    let mut app = App::new();
    app.register_asset_source(
        AssetSourceId::Default,
        AssetSourceBuilder::new(move || Box::new(memory_reader.clone())).with_watcher(
            move |source_events| {
                watcher_sender
                    .send(source_events)
                    .expect("test watcher receiver remains alive");
                Some(Box::new(TestWatcher))
            },
        ),
    )
    .add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin {
            watch_for_changes_override: Some(true),
            use_asset_processor_override: Some(false),
            ..Default::default()
        },
        ImagePlugin::default(),
    ))
    .init_asset::<SpinalAsset>()
    .register_asset_loader(SpinalAssetLoader);

    let source_events = watcher_receiver
        .recv_timeout(Duration::from_secs(1))
        .expect("asset watcher should be installed");
    let asset_server = app.world().resource::<AssetServer>().clone();
    let handle = asset_server.load::<SpinalAsset>("cat.spine.json");

    update_until(&mut app, |app| match asset_server.load_state(&handle) {
        LoadState::Failed(error) => {
            panic!("initial compound load failed: {error}")
        }
        LoadState::Loaded => app
            .world()
            .resource::<Assets<SpinalAsset>>()
            .get(&handle)
            .is_some(),
        LoadState::NotLoaded | LoadState::Loading => false,
    });

    let (page_handle, initial_skeleton) = {
        let assets = app.world().resource::<Assets<SpinalAsset>>();
        let asset = assets.get(&handle).expect("loaded asset is retained");
        assert_eq!(asset.skeleton().spine_version(), "4.3.23");
        assert_eq!(asset.pages().len(), 1);
        assert_eq!(asset.page(0).expect("one page").name(), "cat.png");
        assert_eq!(
            asset
                .page(0)
                .expect("one page")
                .image()
                .path()
                .map(ToString::to_string)
                .as_deref(),
            Some("cat.spine.json#page-0")
        );
        (
            asset.page(0).expect("one page").image().clone(),
            Arc::clone(asset.skeleton()),
        )
    };

    assert_sampler(
        app.world()
            .resource::<Assets<Image>>()
            .get(&page_handle)
            .expect("page image is loaded before the compound root"),
        ImageFilterMode::Nearest,
        ImageFilterMode::Linear,
        ImageAddressMode::Repeat,
        ImageAddressMode::ClampToEdge,
    );

    files.insert_asset_text(Path::new("cat.atlas"), LINEAR_ATLAS);
    source_events
        .send_blocking(AssetSourceEvent::ModifiedAsset(PathBuf::from("cat.atlas")))
        .expect("test source event channel remains alive");

    update_until(&mut app, |app| {
        let images = app.world().resource::<Assets<Image>>();
        let Some(image) = images.get(&page_handle) else {
            return false;
        };
        sampler_descriptor(image).is_some_and(|sampler| {
            sampler.min_filter == ImageFilterMode::Linear
                && sampler.address_mode_v == ImageAddressMode::Repeat
        })
    });

    let successful_skeleton = {
        let assets = app.world().resource::<Assets<SpinalAsset>>();
        let asset = assets
            .get(&handle)
            .expect("successful reload replaces the retained asset");
        assert_eq!(
            asset.page(0).expect("one page").image().id(),
            page_handle.id(),
            "stable labels preserve the page handle across reloads"
        );
        assert!(
            !Arc::ptr_eq(&initial_skeleton, asset.skeleton()),
            "a successful atlas reload rebuilds the linked skeleton"
        );
        Arc::clone(asset.skeleton())
    };

    files.insert_asset_text(
        Path::new("cat.atlas"),
        "\
cat.png
	size: 1, 1

missing.png
	size: 1, 1
",
    );
    source_events
        .send_blocking(AssetSourceEvent::ModifiedAsset(PathBuf::from("cat.atlas")))
        .expect("test source event channel remains alive");

    update_until(&mut app, |_app| {
        matches!(asset_server.load_state(&handle), LoadState::Failed(_))
    });

    let assets = app.world().resource::<Assets<SpinalAsset>>();
    let retained = assets
        .get(&handle)
        .expect("failed hot reload keeps the last good compound asset");
    assert!(Arc::ptr_eq(&successful_skeleton, retained.skeleton()));
    assert_eq!(
        retained.page(0).expect("one page").image().id(),
        page_handle.id()
    );
}

fn assert_sampler(
    image: &Image,
    min_filter: ImageFilterMode,
    mag_filter: ImageFilterMode,
    address_mode_u: ImageAddressMode,
    address_mode_v: ImageAddressMode,
) {
    let sampler = sampler_descriptor(image).expect("loader installs an authored sampler");
    assert_eq!(sampler.min_filter, min_filter);
    assert_eq!(sampler.mag_filter, mag_filter);
    assert_eq!(sampler.address_mode_u, address_mode_u);
    assert_eq!(sampler.address_mode_v, address_mode_v);
}

fn sampler_descriptor(image: &Image) -> Option<&bevy::image::ImageSamplerDescriptor> {
    match &image.sampler {
        ImageSampler::Descriptor(sampler) => Some(sampler),
        ImageSampler::Default => None,
    }
}

fn update_until(app: &mut App, mut predicate: impl FnMut(&mut App) -> bool) {
    for _attempt in 0..5_000 {
        app.update();
        if predicate(app) {
            return;
        }
        thread::sleep(Duration::from_millis(1));
    }
    panic!("asset operation did not settle before the test timeout");
}

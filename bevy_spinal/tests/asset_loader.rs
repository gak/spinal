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
        AssetApp, AssetPlugin, AssetServer, Assets, LoadState, RenderAssetUsages,
        io::{
            AssetSourceBuilder, AssetSourceEvent, AssetSourceId, AssetWatcher,
            memory::{Dir, MemoryAssetReader},
        },
    },
    image::{
        CompressedImageFormats, Image, ImageAddressMode, ImageFilterMode, ImagePlugin,
        ImageSampler, ImageType,
    },
    prelude::{App, MinimalPlugins},
};
use bevy_spinal::{
    SpinalAsset, SpinalAssetLoader, SpinalAssetLoaderSettings, SpinalInstance, SpinalInstanceState,
    SpinalPlugin,
};
use spinal::DiagnosticCode;

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

#[test]
#[ignore = "requires external fixtures; see github.com/gak/spinal/blob/main/fixtures/README.md"]
fn exact_editor_exports_load_as_complete_bevy_assets() {
    let root = std::env::var_os("SPINAL_4_3_23_FIXTURES").unwrap_or_else(|| {
        panic!(
            "SPINAL_4_3_23_FIXTURES must point at the external fixture root; \
             see https://github.com/gak/spinal/blob/main/fixtures/README.md"
        )
    });
    let root = Path::new(&root);
    let files = Dir::default();
    for (directory, stem) in [("ess", "spineboy-ess"), ("pro", "spineboy-pro")] {
        let source = root.join(directory);
        let target = PathBuf::from(directory);
        files.insert_asset(
            &target.join(format!("{stem}.json")),
            std::fs::read(source.join(format!("{stem}.json"))).expect("fixture JSON is readable"),
        );
        files.insert_asset(
            &target.join(format!("{stem}.atlas")),
            std::fs::read(source.join(format!("{stem}.atlas"))).expect("fixture atlas is readable"),
        );
        files.insert_asset(
            &target.join(format!("{stem}.png")),
            std::fs::read(source.join(format!("{stem}.png"))).expect("fixture image is readable"),
        );
    }

    let memory_reader = MemoryAssetReader { root: files };
    let mut app = App::new();
    app.register_asset_source(
        AssetSourceId::Default,
        AssetSourceBuilder::new(move || Box::new(memory_reader.clone())),
    )
    .add_plugins((
        MinimalPlugins,
        AssetPlugin {
            watch_for_changes_override: Some(false),
            use_asset_processor_override: Some(false),
            ..Default::default()
        },
        SpinalPlugin,
    ));

    let asset_server = app.world().resource::<AssetServer>().clone();
    let essential = asset_server.load::<SpinalAsset>("ess/spineboy-ess.json");
    let professional = asset_server.load::<SpinalAsset>("pro/spineboy-pro.json");
    for handle in [&essential, &professional] {
        update_until(&mut app, |app| match asset_server.load_state(handle) {
            LoadState::Failed(error) => panic!("exact editor export failed to load: {error}"),
            LoadState::Loaded => app
                .world()
                .resource::<Assets<SpinalAsset>>()
                .get(handle)
                .is_some(),
            LoadState::NotLoaded | LoadState::Loading => false,
        });
    }

    {
        let assets = app.world().resource::<Assets<SpinalAsset>>();
        let images = app.world().resource::<Assets<Image>>();
        for (handle, expected_size) in [(&essential, (1_349, 275)), (&professional, (789, 1_044))] {
            let asset = assets.get(handle).expect("compound asset is retained");
            assert_eq!(asset.skeleton().spine_version(), "4.3.23");
            assert_eq!(asset.pages().len(), 1);
            assert!(
                asset.diagnostics().iter().any(|diagnostic| {
                    diagnostic.code() == DiagnosticCode::AlphaEncodingMismatch
                })
            );
            let image = images
                .get(asset.page(0).expect("one atlas page").image())
                .expect("atlas page image is retained");
            assert_eq!((image.width(), image.height()), expected_size);
        }
    }

    let entities = [essential, professional]
        .map(|handle| app.world_mut().spawn(SpinalInstance::new(handle)).id());
    for _attempt in 0..10 {
        app.update();
    }
    for entity in entities {
        let instance = app
            .world()
            .entity(entity)
            .get::<SpinalInstance>()
            .expect("instance exists");
        let asset = app
            .world()
            .resource::<Assets<SpinalAsset>>()
            .get(instance.asset())
            .expect("runtime asset remains loaded");
        let image_handle = asset.page(0).expect("one page").image();
        assert!(
            app.world()
                .resource::<Assets<Image>>()
                .get(image_handle)
                .is_some(),
            "runtime page image remains loaded"
        );
        let state = app
            .world()
            .entity(entity)
            .get::<SpinalInstanceState>()
            .expect("runtime state exists");
        assert_eq!(state, &SpinalInstanceState::DegradedNoDraws);
        assert!(state.is_usable());
        assert!(!state.has_drawable_output());
    }
}

#[test]
#[ignore = "requires the derived straight-alpha Professional preview; see github.com/gak/spinal/blob/main/fixtures/README.md"]
fn prepared_professional_weighted_preview_has_drawable_output() {
    let root = std::env::var_os("SPINAL_SPINEBOY_WEIGHTED_PREVIEW").unwrap_or_else(|| {
        panic!(
            "SPINAL_SPINEBOY_WEIGHTED_PREVIEW must point at a preview produced by \
             tools/prepare-spineboy-weighted-preview.sh"
        )
    });
    let root = Path::new(&root);
    let files = Dir::default();
    for extension in ["json", "atlas", "png"] {
        let name = format!("spineboy-pro.{extension}");
        let source = root.join(&name);
        let bytes = std::fs::read(&source)
            .unwrap_or_else(|error| panic!("{} is readable: {error}", source.display()));
        files.insert_asset(Path::new(&name), bytes);
    }

    let memory_reader = MemoryAssetReader { root: files };
    let mut app = App::new();
    app.register_asset_source(
        AssetSourceId::Default,
        AssetSourceBuilder::new(move || Box::new(memory_reader.clone())),
    )
    .add_plugins((
        MinimalPlugins,
        AssetPlugin {
            watch_for_changes_override: Some(false),
            use_asset_processor_override: Some(false),
            ..Default::default()
        },
        SpinalPlugin,
    ));

    let asset_server = app.world().resource::<AssetServer>().clone();
    let handle = asset_server.load::<SpinalAsset>("spineboy-pro.json");
    update_until(&mut app, |app| match asset_server.load_state(&handle) {
        LoadState::Failed(error) => panic!("weighted preview failed to load: {error}"),
        LoadState::Loaded => app
            .world()
            .resource::<Assets<SpinalAsset>>()
            .get(&handle)
            .is_some(),
        LoadState::NotLoaded | LoadState::Loading => false,
    });

    {
        let assets = app.world().resource::<Assets<SpinalAsset>>();
        let asset = assets.get(&handle).expect("weighted preview is retained");
        let meshes = asset
            .skeleton()
            .attachments()
            .filter_map(|attachment| attachment.as_mesh())
            .collect::<Vec<_>>();
        assert_eq!(meshes.len(), 12);
        assert_eq!(meshes.iter().filter(|mesh| mesh.is_weighted()).count(), 10);
    }

    let entity = app.world_mut().spawn(SpinalInstance::new(handle)).id();
    for _attempt in 0..10 {
        app.update();
    }
    let state = app
        .world()
        .entity(entity)
        .get::<SpinalInstanceState>()
        .expect("weighted preview runtime state exists");
    assert!(state.is_usable());
    assert!(
        state.has_drawable_output(),
        "straight-alpha Professional export must retain visible weighted mesh draws"
    );
}

#[test]
#[ignore = "requires the exact Essential export and derived aiming preview; see github.com/gak/spinal/blob/main/fixtures/README.md"]
fn prepared_preview_straight_alpha_reconstructs_source_pma_in_linear_light() {
    let fixture_root = std::env::var_os("SPINAL_4_3_23_FIXTURES")
        .expect("SPINAL_4_3_23_FIXTURES points at the external fixture root");
    let preview_root = std::env::var_os("SPINAL_SPINEBOY_AIM_PREVIEW")
        .expect("SPINAL_SPINEBOY_AIM_PREVIEW points at the derived preview root");
    let source = decode_png(Path::new(&fixture_root).join("ess/spineboy-ess.png"));
    let prepared = decode_png(Path::new(&preview_root).join("spineboy-ess.png"));
    assert_eq!(source.texture_descriptor, prepared.texture_descriptor);

    let source = source.data.expect("the decoded source keeps CPU pixels");
    let prepared = prepared.data.expect("the decoded preview keeps CPU pixels");
    assert_eq!(source.len(), prepared.len());
    let mut worst_error = 0.0_f32;
    for (source, prepared) in source.chunks_exact(4).zip(prepared.chunks_exact(4)) {
        assert_eq!(source[3], prepared[3], "alpha must remain byte-exact");
        let alpha = f32::from(source[3]) / 255.0;
        if alpha == 0.0 {
            continue;
        }
        for channel in 0..3 {
            let source_linear = srgb_to_linear(f32::from(source[channel]) / 255.0);
            let prepared_linear = srgb_to_linear(f32::from(prepared[channel]) / 255.0);
            worst_error = worst_error.max((source_linear - prepared_linear * alpha).abs());
        }
    }
    assert!(
        worst_error < 0.005,
        "straight-alpha preview must reconstruct the source PMA colour in linear light; worst error was {worst_error}"
    );
}

fn decode_png(path: PathBuf) -> Image {
    Image::from_buffer(
        &std::fs::read(&path)
            .unwrap_or_else(|error| panic!("{} is readable: {error}", path.display())),
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::Default,
        RenderAssetUsages::default(),
    )
    .unwrap_or_else(|error| panic!("{} decodes: {error}", path.display()))
}

fn srgb_to_linear(channel: f32) -> f32 {
    if channel <= 0.040_45 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

#[test]
#[ignore = "requires project-owned fixtures; see github.com/gak/spinal/blob/main/fixtures/PROJECT_INTAKE.md"]
fn project_owned_nonfatal_exports_load_as_complete_bevy_assets() {
    let root = std::env::var_os("SPINAL_4_3_23_PROJECT_FIXTURES").unwrap_or_else(|| {
        panic!(
            "SPINAL_4_3_23_PROJECT_FIXTURES must point at the project fixture root; \
             see https://github.com/gak/spinal/blob/main/fixtures/PROJECT_INTAKE.md"
        )
    });
    let root = std::fs::canonicalize(root).expect("project fixture root resolves");
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("MANIFEST.json")).expect("project manifest is readable"),
    )
    .expect("project manifest is valid JSON");
    let mut cases = manifest["positive"]
        .as_array()
        .expect("project manifest has positive cases")
        .iter()
        .map(|case| {
            (
                case["id"]
                    .as_str()
                    .expect("positive case has an id")
                    .to_owned(),
                case["json"]
                    .as_str()
                    .expect("positive case has a JSON path")
                    .to_owned(),
            )
        })
        .collect::<Vec<_>>();
    cases.extend(
        manifest["tripwires"]
            .as_array()
            .expect("project manifest has tripwire cases")
            .iter()
            .filter(|case| case["coverage_id"] != "binary-skeleton")
            .map(|case| {
                (
                    case["coverage_id"]
                        .as_str()
                        .expect("tripwire case has a coverage id")
                        .to_owned(),
                    case["json"]
                        .as_str()
                        .expect("nonbinary tripwire has a JSON path")
                        .to_owned(),
                )
            }),
    );
    cases.extend(["a", "b", "c", "d"].map(|name| {
        let case = &manifest["scale_probe"][name];
        (
            case["id"]
                .as_str()
                .unwrap_or_else(|| panic!("scale probe `{name}` has an id"))
                .to_owned(),
            case["json"]
                .as_str()
                .unwrap_or_else(|| panic!("scale probe `{name}` has a JSON path"))
                .to_owned(),
        )
    }));

    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: root
                .to_str()
                .expect("project fixture root is UTF-8")
                .to_owned(),
            watch_for_changes_override: Some(false),
            use_asset_processor_override: Some(false),
            ..Default::default()
        },
        SpinalPlugin,
    ));

    let asset_server = app.world().resource::<AssetServer>().clone();
    let handles = cases
        .iter()
        .map(|(_id, path)| asset_server.load::<SpinalAsset>(path.clone()))
        .collect::<Vec<_>>();
    for ((id, _path), handle) in cases.iter().zip(&handles) {
        update_until(&mut app, |app| match asset_server.load_state(handle) {
            LoadState::Failed(error) => panic!("project export `{id}` failed to load: {error}"),
            LoadState::Loaded => app
                .world()
                .resource::<Assets<SpinalAsset>>()
                .get(handle)
                .is_some(),
            LoadState::NotLoaded | LoadState::Loading => false,
        });
    }

    let assets = app.world().resource::<Assets<SpinalAsset>>();
    let images = app.world().resource::<Assets<Image>>();
    for ((id, _path), handle) in cases.iter().zip(&handles) {
        let asset = assets
            .get(handle)
            .unwrap_or_else(|| panic!("project export `{id}` remains retained"));
        assert_eq!(asset.skeleton().spine_version(), "4.3.23", "{id}");
        assert!(!asset.pages().is_empty(), "{id} has atlas pages");
        for page in asset.pages() {
            assert!(
                images.get(page.image()).is_some(),
                "project export `{id}` page `{}` remains loaded",
                page.name()
            );
        }
    }

    let pma_index = cases
        .iter()
        .position(|(id, _path)| id == "premultiplied-alpha")
        .expect("project manifest has a PMA tripwire");
    let pma_entity = app
        .world_mut()
        .spawn(SpinalInstance::new(handles[pma_index].clone()))
        .id();
    for _attempt in 0..10 {
        app.update();
    }
    let pma_state = app
        .world()
        .entity(pma_entity)
        .get::<SpinalInstanceState>()
        .expect("PMA tripwire instance has a runtime state");
    assert_eq!(pma_state, &SpinalInstanceState::DegradedNoDraws);
    assert!(pma_state.is_usable());
    assert!(!pma_state.has_drawable_output());

    let binary_path = manifest["tripwires"]
        .as_array()
        .expect("project manifest has tripwire cases")
        .iter()
        .find(|case| case["coverage_id"] == "binary-skeleton")
        .and_then(|case| case["binary"].as_str())
        .expect("project manifest has a binary rejection tripwire");
    let binary = asset_server.load::<SpinalAsset>(binary_path.to_owned());
    update_until(&mut app, |_app| match asset_server.load_state(&binary) {
        LoadState::Failed(_error) => true,
        LoadState::Loaded => panic!("Bevy adapter unexpectedly accepted binary skeleton data"),
        LoadState::NotLoaded | LoadState::Loading => false,
    });
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

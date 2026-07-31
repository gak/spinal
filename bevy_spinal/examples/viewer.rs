//! Small interactive viewer for the fresh Bevy adapter.
//!
//! Run the bundled, self-authored fixture:
//!
//! ```text
//! cargo run -p bevy_spinal --example viewer --features viewer
//! ```
//!
//! Or select a typed Spine JSON asset below an asset root:
//!
//! ```text
//! cargo run -p bevy_spinal --example viewer --features viewer -- \
//!   --asset-root exports --asset cat.spine.json --animation idle \
//!   --skins collar/red,glasses/round --mouse-target crosshair
//! ```

use std::{env, sync::Arc, time::Duration};

use bevy::{
    asset::{AssetPlugin, AssetServer, Assets, RenderAssetUsages},
    ecs::message::MessageReader,
    image::{Image, ImageSampler},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    window::PrimaryWindow,
};
use bevy_spinal::{
    BoneOverride, SpinalAnimationEvent, SpinalAnimator, SpinalAsset, SpinalAtlasPage,
    SpinalInstance, SpinalInstanceState, SpinalIssue, SpinalPlaybackState, SpinalPlugin,
    SpinalPoseOverrides, SpinalSet, SpinalSkinLayers,
};
use spinal::{Angle, BoneTransform, Crossfade, MixCurve, PlaybackMode, Transition};

const FIXTURE_JSON: &[u8] = include_bytes!("assets/viewer.spine.json");
const FIXTURE_ATLAS: &[u8] = include_bytes!("assets/viewer.atlas");
const CROSSFADE: Duration = Duration::from_millis(180);
const TRIPWIRE_SKIN: &str = "tripwire/unsupported";
const OVERRIDE_BONE: &str = "head";
const OVERRIDE_STEP_DEGREES: f32 = 5.0;
const BASE_PIXELS: [u8; 64] = [
    218, 143, 84, 255, // body
    190, 112, 67, 255, // trimmed tail, warm end
    145, 75, 47, 255, // trimmed tail, dark end
    0, 0, 0, 0, // unused
    0, 0, 0, 0, // unused
    0, 0, 0, 0, // unused
    0, 0, 0, 0, // unused
    0, 0, 0, 0, // unused
    0, 0, 0, 0, // unused second row
    0, 0, 0, 0, // unused
    0, 0, 0, 0, // unused
    0, 0, 0, 0, // unused
    0, 0, 0, 0, // unused
    0, 0, 0, 0, // unused
    0, 0, 0, 0, // unused
    0, 0, 0, 0, // unused
];
const DETAILS_PIXELS: [u8; 64] = [
    245, 178, 114, 255, // quarter-turn head, face half
    29, 37, 54, 255, // open eye
    52, 44, 49, 255, // closed eye
    214, 62, 82, 255, // collar
    54, 192, 199, 255, // glasses
    104, 115, 226, 255, // hat
    201, 126, 78, 255, // upper foreleg
    181, 104, 65, 255, // lower foreleg
    224, 131, 86, 255, // quarter-turn head, ear half
    0, 0, 0, 0, // unused second row
    0, 0, 0, 0, // unused
    0, 0, 0, 0, // unused
    0, 0, 0, 0, // unused
    0, 0, 0, 0, // unused
    0, 0, 0, 0, // unused
    0, 0, 0, 0, // unused
];
const SKIN_KEYS: [KeyCode; 9] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];

fn main() {
    let Some(options) = parse_options() else {
        return;
    };
    let asset_root = options.asset_root.clone();

    App::new()
        .insert_resource(ClearColor(Color::srgb(0.055, 0.065, 0.085)))
        .insert_resource(options)
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: asset_root,
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Spinal viewer | starting".into(),
                        resolution: (760, 520).into(),
                        resizable: true,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(SpinalPlugin)
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                refresh_catalog
                    .after(SpinalSet::Prepare)
                    .before(SpinalSet::Animate),
                handle_controls
                    .after(SpinalSet::Prepare)
                    .before(SpinalSet::Animate),
                follow_mouse_target
                    .after(refresh_catalog)
                    .after(handle_controls)
                    .before(SpinalSet::Animate),
                observe_messages.after(SpinalSet::Animate),
                update_window_title.after(SpinalSet::Animate),
            ),
        )
        .run();
}

#[derive(Clone, Debug, Resource)]
struct ViewerOptions {
    asset_root: String,
    asset: Option<String>,
    animation: Option<Box<str>>,
    skins: Vec<Box<str>>,
    scale: f32,
    tripwire: bool,
    mouse_target: Option<Box<str>>,
}

impl Default for ViewerOptions {
    fn default() -> Self {
        Self {
            asset_root: ".".to_owned(),
            asset: None,
            animation: None,
            skins: Vec::new(),
            scale: 3.0,
            tripwire: false,
            mouse_target: None,
        }
    }
}

fn parse_options() -> Option<ViewerOptions> {
    match ViewerOptions::parse(env::args().skip(1)) {
        Ok(ParseResult::Run(options)) => Some(options),
        Ok(ParseResult::Help) => {
            print_help();
            None
        }
        Err(error) => {
            eprintln!("spinal viewer: {error}\n");
            print_help();
            None
        }
    }
}

enum ParseResult {
    Run(ViewerOptions),
    Help,
}

impl ViewerOptions {
    fn parse(arguments: impl IntoIterator<Item = String>) -> Result<ParseResult, String> {
        let mut options = Self::default();
        let mut arguments = arguments.into_iter();

        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "-h" | "--help" => return Ok(ParseResult::Help),
                "--asset-root" => {
                    options.asset_root = next_value(&mut arguments, "--asset-root")?;
                }
                "--asset" => {
                    options.asset = Some(next_value(&mut arguments, "--asset")?);
                }
                "--animation" => {
                    options.animation =
                        Some(Box::<str>::from(next_value(&mut arguments, "--animation")?));
                }
                "--skins" => {
                    options.skins = next_value(&mut arguments, "--skins")?
                        .split(',')
                        .filter(|name| !name.is_empty())
                        .map(Box::<str>::from)
                        .collect();
                }
                "--scale" => {
                    let value = next_value(&mut arguments, "--scale")?;
                    options.scale = value
                        .parse::<f32>()
                        .map_err(|error| format!("invalid --scale `{value}`: {error}"))?;
                    if !options.scale.is_finite() || options.scale <= 0.0 {
                        return Err("--scale must be finite and greater than zero".to_owned());
                    }
                }
                "--tripwire" => options.tripwire = true,
                "--mouse-target" => {
                    options.mouse_target = Some(Box::<str>::from(next_value(
                        &mut arguments,
                        "--mouse-target",
                    )?));
                }
                unknown => return Err(format!("unknown argument `{unknown}`")),
            }
        }

        Ok(ParseResult::Run(options))
    }
}

fn next_value(
    arguments: &mut impl Iterator<Item = String>,
    option: &str,
) -> Result<String, String> {
    arguments
        .next()
        .ok_or_else(|| format!("{option} requires a value"))
}

fn print_help() {
    println!(
        "\
Spinal Bevy viewer

USAGE:
    viewer [--asset-root DIR] [--asset PATH] [--animation NAME]
           [--skins NAME,NAME] [--scale NUMBER] [--tripwire]
           [--mouse-target BONE]

Without --asset, the viewer uses a self-authored documentation-derived fixture.
External asset paths are resolved below --asset-root and loaded as
Handle<SpinalAsset>.

CONTROLS:
    Left / Right   crossfade between animations
    1 through 9    toggle the listed attachment-only skin layers
    Space          pause or resume
    R              restart the current animation
    S              crossfade to setup pose
    O              toggle the procedural head-bone override
    Up / Down      adjust the head override by 5 degrees
    M              pause or resume mouse target tracking
    U              toggle the intentionally unsupported mesh tripwire
"
    );
}

#[derive(Debug, Resource)]
struct ViewerCatalog {
    entity: Entity,
    skeleton: Option<Arc<spinal::SkeletonAsset>>,
    animations: Vec<Box<str>>,
    animation_index: Option<usize>,
    skins: Vec<Box<str>>,
    active_skins: Vec<bool>,
    unresolved_skins: Vec<Box<str>>,
    requested_animation: Option<Box<str>>,
    requested_skins: Vec<Box<str>>,
    tripwire_active: bool,
    head_override_degrees: Option<f32>,
    mouse_target: Option<Box<str>>,
    mouse_follow_enabled: bool,
    mouse_target_available: Option<bool>,
    mouse_target_parent_setup: Option<spinal::WorldTransform>,
    last_issue: Option<Box<str>>,
    last_title: String,
}

fn setup(
    mut commands: Commands<'_, '_>,
    options: Res<'_, ViewerOptions>,
    asset_server: Res<'_, AssetServer>,
    mut assets: ResMut<'_, Assets<SpinalAsset>>,
    mut images: ResMut<'_, Assets<Image>>,
) {
    commands.spawn(Camera2d);

    let asset = options.asset.as_ref().map_or_else(
        || bundled_fixture(&mut assets, &mut images),
        |path| asset_server.load::<SpinalAsset>(path.clone()),
    );
    let entity = commands
        .spawn((
            SpinalInstance::new(asset),
            SpinalSkinLayers::new(options.skins.iter().cloned()),
            Transform::from_scale(Vec3::splat(options.scale)),
        ))
        .id();

    commands.insert_resource(ViewerCatalog {
        entity,
        skeleton: None,
        animations: Vec::new(),
        animation_index: None,
        skins: Vec::new(),
        active_skins: Vec::new(),
        unresolved_skins: Vec::new(),
        requested_animation: options.animation.clone(),
        requested_skins: options.skins.clone(),
        tripwire_active: options.tripwire,
        head_override_degrees: None,
        mouse_target: options.mouse_target.clone(),
        mouse_follow_enabled: options.mouse_target.is_some(),
        mouse_target_available: None,
        mouse_target_parent_setup: None,
        last_issue: None,
        last_title: String::new(),
    });
}

fn bundled_fixture(
    assets: &mut Assets<SpinalAsset>,
    images: &mut Assets<Image>,
) -> Handle<SpinalAsset> {
    let skeleton = spinal::load_json(FIXTURE_JSON, FIXTURE_ATLAS)
        .expect("the bundled documentation-derived viewer fixture must remain valid")
        .into_asset();
    let base = images.add(generated_page(8, 2, BASE_PIXELS.to_vec()));
    let details = images.add(generated_page(8, 2, DETAILS_PIXELS.to_vec()));
    let asset = SpinalAsset::new(
        skeleton,
        vec![
            SpinalAtlasPage::new("base.png", base),
            SpinalAtlasPage::new("details.png", details),
        ],
    )
    .expect("the generated atlas pages must match the bundled fixture");
    assets.add(asset)
}

fn generated_page(width: u32, height: u32, pixels: Vec<u8>) -> Image {
    let expected_len = usize::try_from(width)
        .expect("viewer pages fit usize")
        .checked_mul(usize::try_from(height).expect("viewer pages fit usize"))
        .and_then(|pixels| pixels.checked_mul(4))
        .expect("viewer page byte length fits usize");
    assert_eq!(
        pixels.len(),
        expected_len,
        "generated viewer page dimensions and RGBA pixels must agree"
    );
    let mut image = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::linear();
    image
}

fn refresh_catalog(
    assets: Res<'_, Assets<SpinalAsset>>,
    mut catalog: ResMut<'_, ViewerCatalog>,
    mut instances: Query<
        '_,
        '_,
        (
            &SpinalInstance,
            &mut SpinalAnimator,
            &mut SpinalSkinLayers,
            &mut SpinalPoseOverrides,
        ),
    >,
) {
    let Ok((instance, mut animator, mut skin_layers, mut pose_overrides)) =
        instances.get_mut(catalog.entity)
    else {
        return;
    };
    let Some(asset) = assets.get(instance.asset()) else {
        return;
    };
    if catalog
        .skeleton
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(current, asset.skeleton()))
    {
        return;
    }

    let previous_animation = catalog
        .animation_index
        .and_then(|index| catalog.animations.get(index))
        .cloned()
        .or_else(|| catalog.requested_animation.take());
    let mut previous_skins = if catalog.skins.is_empty() {
        std::mem::take(&mut catalog.requested_skins)
    } else {
        let mut names = catalog
            .skins
            .iter()
            .zip(&catalog.active_skins)
            .filter(|(_name, active)| **active)
            .map(|(name, _active)| name.clone())
            .collect::<Vec<_>>();
        names.extend(catalog.unresolved_skins.iter().cloned());
        names
    };
    if previous_skins
        .iter()
        .any(|name| name.as_ref() == TRIPWIRE_SKIN)
    {
        catalog.tripwire_active = true;
        previous_skins.retain(|name| name.as_ref() != TRIPWIRE_SKIN);
    }

    catalog.animations = asset
        .skeleton()
        .animations()
        .map(|animation| Box::<str>::from(animation.name()))
        .collect();
    catalog.skins = asset
        .skeleton()
        .skins()
        .map(|skin| Box::<str>::from(skin.name()))
        .filter(|name| !matches!(name.as_ref(), "default" | TRIPWIRE_SKIN))
        .collect();
    catalog.animation_index = previous_animation.as_deref().map_or_else(
        || (!catalog.animations.is_empty()).then_some(0),
        |name| {
            catalog
                .animations
                .iter()
                .position(|candidate| **candidate == *name)
        },
    );
    catalog.active_skins = catalog
        .skins
        .iter()
        .map(|name| previous_skins.iter().any(|active| active == name))
        .collect();
    catalog.unresolved_skins = previous_skins
        .into_iter()
        .filter(|name| !catalog.skins.contains(name))
        .collect();
    let skeleton = Arc::clone(asset.skeleton());
    catalog.mouse_target_parent_setup = catalog
        .mouse_target
        .as_deref()
        .and_then(|name| skeleton.bone_id(name))
        .and_then(|target| skeleton.bone(target).ok())
        .and_then(|target| target_parent_setup_world(&skeleton, target));
    catalog.skeleton = Some(skeleton);
    catalog.mouse_target_available = None;

    let animation =
        previous_animation.or_else(|| catalog.current_animation().map(Box::<str>::from));
    if let Some(animation) = animation {
        animator.play(animation, PlaybackMode::Loop, Transition::Immediate);
    }
    apply_skin_layers(&catalog, &mut skin_layers);
    apply_head_override(&catalog, &mut pose_overrides);
    print_catalog(&catalog);
}

fn handle_controls(
    keys: Res<'_, ButtonInput<KeyCode>>,
    mut catalog: ResMut<'_, ViewerCatalog>,
    mut instances: Query<
        '_,
        '_,
        (
            &mut SpinalAnimator,
            &mut SpinalSkinLayers,
            &mut SpinalPoseOverrides,
        ),
    >,
) {
    let Ok((mut animator, mut skin_layers, mut pose_overrides)) = instances.get_mut(catalog.entity)
    else {
        return;
    };

    let animation_step = i8::from(keys.just_pressed(KeyCode::ArrowRight))
        - i8::from(keys.just_pressed(KeyCode::ArrowLeft));
    if animation_step != 0 && !catalog.animations.is_empty() {
        let current = catalog.animation_index.unwrap_or(0);
        let len = catalog.animations.len();
        let next = if animation_step > 0 {
            (current + 1) % len
        } else {
            (current + len - 1) % len
        };
        catalog.animation_index = Some(next);
        let animation = catalog.animations[next].clone();
        animator.play(animation.clone(), PlaybackMode::Loop, smooth_crossfade());
        println!("animation: {animation}");
    }

    if keys.just_pressed(KeyCode::Space) {
        let paused = !animator.is_paused();
        animator.set_paused(paused);
        println!("playback: {}", if paused { "paused" } else { "running" });
    }
    if keys.just_pressed(KeyCode::KeyR) {
        animator.restart();
        println!("playback: restarted");
    }
    if keys.just_pressed(KeyCode::KeyS) {
        animator.stop(smooth_crossfade());
        println!("playback: setup pose");
    }

    let mut skins_changed = false;
    for (index, key) in SKIN_KEYS.into_iter().enumerate() {
        if index < catalog.active_skins.len() && keys.just_pressed(key) {
            catalog.active_skins[index] = !catalog.active_skins[index];
            skins_changed = true;
        }
    }
    if skins_changed {
        apply_skin_layers(&catalog, &mut skin_layers);
        let active = active_skin_label(&catalog);
        println!("skin layers: {active}");
    }

    if keys.just_pressed(KeyCode::KeyU) {
        catalog.tripwire_active = !catalog.tripwire_active;
        apply_skin_layers(&catalog, &mut skin_layers);
        println!(
            "unsupported tripwire: {}",
            if catalog.tripwire_active {
                "active (expect Degraded + red X)"
            } else {
                "inactive"
            }
        );
    }

    let override_step = i8::from(keys.just_pressed(KeyCode::ArrowUp))
        - i8::from(keys.just_pressed(KeyCode::ArrowDown));
    let override_toggled = keys.just_pressed(KeyCode::KeyO);
    if override_toggled {
        catalog.head_override_degrees = catalog
            .head_override_degrees
            .map_or(Some(0.0), |_angle| None);
    }
    if override_step != 0 {
        *catalog.head_override_degrees.get_or_insert(0.0) +=
            f32::from(override_step) * OVERRIDE_STEP_DEGREES;
    }
    if override_toggled || override_step != 0 {
        apply_head_override(&catalog, &mut pose_overrides);
        match catalog.head_override_degrees {
            Some(degrees) => println!("procedural {OVERRIDE_BONE} override: {degrees:+.0} degrees"),
            None => println!("procedural {OVERRIDE_BONE} override: inactive"),
        }
    }

    if keys.just_pressed(KeyCode::KeyM) {
        if let Some(target) = catalog.mouse_target.clone() {
            catalog.mouse_follow_enabled = !catalog.mouse_follow_enabled;
            if !catalog.mouse_follow_enabled {
                pose_overrides.remove(&target);
            }
            println!(
                "mouse target {target}: {}",
                if catalog.mouse_follow_enabled {
                    "tracking"
                } else {
                    "paused"
                }
            );
        } else {
            println!("mouse target: no --mouse-target bone configured");
        }
    }
}

fn follow_mouse_target(
    windows: Query<'_, '_, &Window, With<PrimaryWindow>>,
    cameras: Query<'_, '_, (&Camera, &GlobalTransform)>,
    mut catalog: ResMut<'_, ViewerCatalog>,
    mut instances: Query<'_, '_, (&GlobalTransform, &mut SpinalPoseOverrides)>,
) {
    let Some(target_name) = catalog.mouse_target.clone() else {
        return;
    };
    let Ok((instance_transform, mut overrides)) = instances.get_mut(catalog.entity) else {
        return;
    };
    if !catalog.mouse_follow_enabled {
        overrides.remove(&target_name);
        return;
    }
    let Some(asset) = catalog.skeleton.clone() else {
        return;
    };
    let Some(target_id) = asset.bone_id(&target_name) else {
        report_mouse_target_availability(&mut catalog, false);
        overrides.remove(&target_name);
        return;
    };
    let target = asset
        .bone(target_id)
        .expect("a name-resolved bone belongs to its asset");
    let Some(parent_setup) = catalog.mouse_target_parent_setup else {
        report_mouse_target_availability(&mut catalog, false);
        overrides.remove(&target_name);
        return;
    };
    report_mouse_target_availability(&mut catalog, true);

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok((camera, camera_transform)) = cameras.single() else {
        return;
    };
    let Ok(world) = camera.viewport_to_world_2d(camera_transform, cursor) else {
        return;
    };
    let Some(skeleton_point) = world_to_skeleton_point(instance_transform, world) else {
        return;
    };
    let Some(parent_point) = parent_setup.try_inverse_point(skeleton_point) else {
        return;
    };
    let setup = target.setup_transform();
    let transform =
        BoneTransform::new(parent_point, setup.rotation(), setup.scale(), setup.shear())
            .expect("camera and entity transforms produce a finite mouse target");
    overrides.set(BoneOverride::new(target_name, transform));
}

fn target_parent_setup_world(
    asset: &Arc<spinal::SkeletonAsset>,
    target: spinal::BoneRef<'_>,
) -> Option<spinal::WorldTransform> {
    let Some(parent) = target.parent() else {
        return Some(spinal::WorldTransform::IDENTITY);
    };
    if asset.bone(parent).ok()?.parent().is_some() {
        return None;
    }

    let mut skeleton = spinal::Skeleton::new(Arc::clone(asset));
    let frame = skeleton.editable_pose().solve();
    frame.bone(parent).ok().map(|bone| bone.world_transform())
}

fn report_mouse_target_availability(catalog: &mut ViewerCatalog, available: bool) {
    if catalog.mouse_target_available == Some(available) {
        return;
    }
    catalog.mouse_target_available = Some(available);
    let target = catalog.mouse_target.as_deref().unwrap_or("<none>");
    if available {
        println!("mouse target {target}: ready");
    } else {
        eprintln!(
            "mouse target {target}: unavailable \
             (bone must be the root or a direct child of the root)"
        );
    }
}

fn world_to_skeleton_point(transform: &GlobalTransform, world: Vec2) -> Option<Vec2> {
    let matrix = transform.to_matrix();
    let determinant = matrix.determinant();
    if !determinant.is_finite() || determinant.abs() <= f32::EPSILON {
        return None;
    }
    let point = matrix
        .inverse()
        .transform_point3(world.extend(transform.translation().z))
        .truncate();
    point.is_finite().then_some(point)
}

fn smooth_crossfade() -> Transition {
    Transition::Crossfade(Crossfade::new(CROSSFADE).with_curve(MixCurve::SmoothStep))
}

impl ViewerCatalog {
    fn current_animation(&self) -> Option<&str> {
        self.animation_index
            .and_then(|index| self.animations.get(index))
            .map(AsRef::as_ref)
    }
}

fn apply_skin_layers(catalog: &ViewerCatalog, layers: &mut SpinalSkinLayers) {
    layers.set(
        catalog
            .skins
            .iter()
            .zip(&catalog.active_skins)
            .filter(|(_name, active)| **active)
            .map(|(name, _active)| name.clone())
            .chain(catalog.unresolved_skins.iter().cloned())
            .chain(
                catalog
                    .tripwire_active
                    .then(|| Box::<str>::from(TRIPWIRE_SKIN)),
            ),
    );
}

fn apply_head_override(catalog: &ViewerCatalog, overrides: &mut SpinalPoseOverrides) {
    let Some(degrees) = catalog.head_override_degrees else {
        overrides.remove(OVERRIDE_BONE);
        return;
    };
    let transform = catalog
        .skeleton
        .as_ref()
        .and_then(|asset| {
            asset
                .bone_id(OVERRIDE_BONE)
                .and_then(|bone| asset.bone(bone).ok())
        })
        .map_or(BoneTransform::IDENTITY, |bone| {
            offset_rotation(bone.setup_transform(), degrees)
        });
    overrides.set(BoneOverride::new(OVERRIDE_BONE, transform));
}

fn offset_rotation(transform: BoneTransform, degrees: f32) -> BoneTransform {
    let rotation = Angle::from_degrees(transform.rotation().as_degrees() + degrees)
        .expect("finite viewer control input keeps the bone rotation finite");
    BoneTransform::new(
        transform.translation(),
        rotation,
        transform.scale(),
        transform.shear(),
    )
    .expect("reusing a valid setup transform with a finite rotation remains valid")
}

fn print_catalog(catalog: &ViewerCatalog) {
    println!(
        "animations: {}",
        catalog
            .animations
            .iter()
            .enumerate()
            .map(|(index, name)| format!("{}={name}", index + 1))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!(
        "skin layers: {}",
        catalog
            .skins
            .iter()
            .enumerate()
            .map(|(index, name)| format!("{}={name}", index + 1))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!(
        "controls: Left/Right animation, 1-9 skins, Space pause, R restart, S setup, \
         O + Up/Down override, M mouse target, U unsupported tripwire"
    );
}

fn observe_messages(
    mut issues: MessageReader<'_, '_, SpinalIssue>,
    mut events: MessageReader<'_, '_, SpinalAnimationEvent>,
    mut catalog: ResMut<'_, ViewerCatalog>,
) {
    let entity = catalog.entity;
    for issue in issues.read().filter(|issue| issue.entity() == entity) {
        let detail = format!("{:?}: {}", issue.kind(), issue.message());
        eprintln!("issue: {detail}");
        catalog.last_issue = Some(detail.into());
    }
    for event in events.read().filter(|event| event.entity() == entity) {
        println!(
            "event: {}:{} at {:.3}s, loop {}",
            event.animation(),
            event.event(),
            event.local_time().as_secs_f64(),
            event.loop_index()
        );
    }
}

fn update_window_title(
    mut windows: Query<'_, '_, &mut Window, With<PrimaryWindow>>,
    instance: Query<
        '_,
        '_,
        (
            &SpinalInstanceState,
            &SpinalPlaybackState,
            &SpinalSkinLayers,
        ),
    >,
    mut catalog: ResMut<'_, ViewerCatalog>,
) {
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    let Ok((state, playback, layers)) = instance.get(catalog.entity) else {
        return;
    };
    if state.is_ready() {
        catalog.last_issue = None;
    }

    let playback_label = match (playback.animation(), playback.position()) {
        (Some(animation), Some(position)) => {
            format!("{animation} {:.2}s", position.as_secs_f32())
        }
        (Some(animation), None) => animation.to_owned(),
        (None, _position) if playback.is_stopping() => "setup crossfade".to_owned(),
        (None, _position) => "setup pose".to_owned(),
    };
    let skins = if layers.is_empty() {
        "default".to_owned()
    } else {
        layers.iter().collect::<Vec<_>>().join("+")
    };
    let issue = catalog
        .last_issue
        .as_deref()
        .map(|detail| format!(" | ISSUE: {detail}"))
        .unwrap_or_default();
    let mouse = catalog
        .mouse_target
        .as_deref()
        .filter(|_target| catalog.mouse_follow_enabled)
        .map(|target| format!(" | mouse:{target}"))
        .unwrap_or_default();
    let title = format!("Spinal viewer | {state} | {playback_label} | {skins}{mouse}{issue}");

    if title != catalog.last_title {
        window.title.clone_from(&title);
        catalog.last_title = title;
    }
}

fn active_skin_label(catalog: &ViewerCatalog) -> String {
    let names = catalog
        .skins
        .iter()
        .zip(&catalog.active_skins)
        .filter(|(_name, active)| **active)
        .map(|(name, _active)| name.as_ref())
        .chain(catalog.unresolved_skins.iter().map(AsRef::as_ref))
        .chain(catalog.tripwire_active.then_some(TRIPWIRE_SKIN))
        .collect::<Vec<_>>();
    if names.is_empty() {
        "default".to_owned()
    } else {
        names.join("+")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::image::ImageFilterMode;
    use spinal::{
        AlphaEncoding, AnimationPlayer, BendDirection, DiagnosticCode, DrawItemRef, PlayOptions,
        Skeleton, TextureFilter, TextureFormat, WrapMode,
    };

    fn fixture() -> Arc<spinal::SkeletonAsset> {
        spinal::load_json(FIXTURE_JSON, FIXTURE_ATLAS)
            .expect("the viewer fixture should remain loadable")
            .into_asset()
    }

    fn assert_near(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 1.0e-4,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn fixture_links_the_full_viewer_profile_and_inactive_tripwire() {
        let report = spinal::load_json(FIXTURE_JSON, FIXTURE_ATLAS)
            .expect("the viewer fixture should remain loadable");
        assert_eq!(report.asset().spine_version(), "4.3.23");
        assert_eq!(
            report
                .asset()
                .animations()
                .map(|animation| animation.name())
                .collect::<Vec<_>>(),
            ["idle", "eat", "fall"]
        );
        assert_eq!(
            report
                .asset()
                .skins()
                .map(|skin| skin.name())
                .collect::<Vec<_>>(),
            [
                "default",
                "collar/red",
                "glasses/round",
                "hat/blue",
                TRIPWIRE_SKIN,
            ]
        );
        assert_eq!(
            report
                .asset()
                .atlas_pages()
                .map(|page| page.name())
                .collect::<Vec<_>>(),
            ["base.png", "details.png"]
        );
        for page in report.asset().atlas_pages() {
            assert_eq!(page.alpha_encoding(), AlphaEncoding::Straight);
            assert_eq!(page.format(), TextureFormat::Rgba8888);
            assert_eq!(page.min_filter(), TextureFilter::Linear);
            assert_eq!(page.mag_filter(), TextureFilter::Linear);
            assert_eq!(page.wrap(), WrapMode::CLAMP);
            assert_eq!(page.scale(), 1.0);
        }
        assert_eq!(
            report
                .asset()
                .ik_constraints()
                .map(|constraint| (constraint.name(), constraint.bones().len()))
                .collect::<Vec<_>>(),
            [("look", 1), ("paw", 2)]
        );

        let head = report
            .asset()
            .atlas_regions_named("head")
            .next()
            .expect("head atlas region");
        assert_eq!(head.rotation().as_degrees(), 90.0);
        assert!(head.rotation().is_quarter_turn());
        let tail = report
            .asset()
            .atlas_regions_named("tail")
            .next()
            .expect("tail atlas region");
        assert_eq!(tail.index(), Some(0));
        assert_eq!(tail.trim().left(), 1);
        assert_eq!(tail.trim().original_size().width(), 4);
        assert!(
            report.diagnostics().iter().any(|diagnostic| {
                diagnostic.code() == DiagnosticCode::UnsupportedAttachmentType
            })
        );
    }

    #[test]
    fn bundled_adapter_asset_links_both_generated_images_in_source_order() {
        let mut assets = Assets::<SpinalAsset>::default();
        let mut images = Assets::<Image>::default();
        let handle = bundled_fixture(&mut assets, &mut images);
        let asset = assets.get(&handle).expect("bundled asset was inserted");

        assert_eq!(
            asset
                .pages()
                .iter()
                .map(SpinalAtlasPage::name)
                .collect::<Vec<_>>(),
            ["base.png", "details.png"]
        );
        for page in asset.pages() {
            let image = images
                .get(page.image())
                .expect("bundled page image was inserted");
            assert_eq!((image.width(), image.height()), (8, 2));
            let ImageSampler::Descriptor(sampler) = &image.sampler else {
                panic!("the bundled atlas profile installs an explicit sampler");
            };
            assert_eq!(sampler.min_filter, ImageFilterMode::Linear);
            assert_eq!(sampler.mag_filter, ImageFilterMode::Linear);
        }
    }

    #[test]
    fn default_frame_uses_two_pages_trim_rotation_and_both_ik_solvers() {
        let asset = fixture();
        let mut skeleton = Skeleton::new(Arc::clone(&asset));
        let frame = skeleton.editable_pose().solve();

        assert!(!frame.has_degradations());
        assert!(frame.active_diagnostics().next().is_none());
        for constraint in asset.ik_constraints() {
            let status = frame
                .ik_status(constraint.id())
                .expect("fixture constraint belongs to its frame");
            assert!(status.is_active());
            assert_eq!(status.issue(), None);
        }

        let mut pages = Vec::new();
        let mut tail_positions = None;
        let mut head_uvs = None;
        for draw in frame.draw_items() {
            let region = match draw {
                DrawItemRef::Region(region) => region,
                _other => continue,
            };
            let page = frame
                .asset()
                .atlas_page(region.atlas_page())
                .expect("draw page belongs to the fixture");
            if !pages.contains(&page.name()) {
                pages.push(page.name());
            }
            let atlas_region = frame
                .asset()
                .atlas_region(region.atlas_region())
                .expect("draw region belongs to the fixture");
            match atlas_region.name() {
                "tail" => tail_positions = Some(region.positions()),
                "head" => head_uvs = region.uvs(),
                _other => {}
            }
        }
        pages.sort_unstable();
        assert_eq!(pages, ["base.png", "details.png"]);

        let tail_positions = tail_positions.expect("trimmed tail is visible");
        assert_near(tail_positions[0].distance(tail_positions[3]), 21.0);
        let head_uvs = head_uvs.expect("quarter-turn head has normalized UVs");
        let expected_uvs = [
            Vec2::new(0.125, 1.0),
            Vec2::new(0.0, 1.0),
            Vec2::new(0.0, 0.0),
            Vec2::new(0.125, 0.0),
        ];
        for (actual, expected) in head_uvs.into_iter().zip(expected_uvs) {
            assert_near(actual.x, expected.x);
            assert_near(actual.y, expected.y);
        }
    }

    #[test]
    fn fixture_sampling_exercises_every_supported_timeline_and_curve_family() {
        let asset = fixture();
        let mut skeleton = Skeleton::new(Arc::clone(&asset));
        let idle = asset.animation_id("idle").expect("idle animation");
        let body = asset.bone_id("body").expect("body bone");
        let tail = asset.bone_id("tail").expect("tail bone");

        skeleton
            .sample_animation(idle, Duration::from_millis(250), PlaybackMode::Once)
            .expect("idle belongs to the fixture");
        let body_transform = skeleton
            .bone_pose(body)
            .expect("body belongs to the fixture")
            .local_transform();
        assert_near(body_transform.translation().y, 1.5);
        assert_near(body_transform.scale().x, 1.02);
        assert_near(body_transform.scale().y, 0.98);
        assert_near(body_transform.shear().x().as_degrees(), 1.0);
        assert_near(body_transform.shear().y().as_degrees(), -1.0);
        assert_near(body_transform.rotation().as_degrees(), -1.2);
        let head_slot = asset.slot_id("head-slot").expect("head slot");
        let head_colour = skeleton
            .slot_pose(head_slot)
            .expect("head slot belongs to the fixture")
            .color();
        assert_near(head_colour.red(), 1.0);
        assert_near(head_colour.green(), 0.923_529_4);
        assert_near(head_colour.blue(), 0.860_784_3);
        assert_near(head_colour.alpha(), 1.0);
        let look = asset.ik_constraint_id("look").expect("look constraint");
        assert_near(
            skeleton
                .ik_constraint_pose(look)
                .expect("look belongs to the fixture")
                .mix()
                .get(),
            0.31,
        );
        assert_near(
            skeleton
                .bone_pose(tail)
                .expect("tail belongs to the fixture")
                .local_transform()
                .rotation()
                .as_degrees(),
            153.0,
        );

        skeleton
            .sample_animation(idle, Duration::from_millis(500), PlaybackMode::Once)
            .expect("idle belongs to the fixture");
        let eye_slot = asset.slot_id("eye-slot").expect("eye slot");
        let eye = skeleton
            .slot_pose(eye_slot)
            .expect("eye slot belongs to the fixture")
            .attachment()
            .and_then(|attachment| asset.attachment(attachment).ok())
            .expect("the blink selects an attachment");
        assert_eq!(eye.name(), "eye-closed");
        let paw = asset.ik_constraint_id("paw").expect("paw constraint");
        assert_near(
            skeleton
                .ik_constraint_pose(look)
                .expect("look belongs to the fixture")
                .mix()
                .get(),
            0.5,
        );
        let paw_pose = skeleton
            .ik_constraint_pose(paw)
            .expect("paw belongs to the fixture");
        assert_near(paw_pose.mix().get(), 0.85);
        assert_eq!(paw_pose.bend_direction(), BendDirection::Negative);

        let eat = asset.animation_id("eat").expect("eat animation");
        skeleton
            .sample_animation(eat, Duration::from_millis(300), PlaybackMode::Once)
            .expect("eat belongs to the fixture");
        let body_slot = asset.slot_id("body-slot").expect("body slot");
        let body_color = skeleton
            .slot_pose(body_slot)
            .expect("body slot belongs to the fixture")
            .color();
        assert!(body_color.green() < 1.0);
        assert!(body_color.alpha() < 1.0);
        let eye = skeleton
            .slot_pose(eye_slot)
            .expect("eye slot belongs to the fixture")
            .attachment()
            .and_then(|attachment| asset.attachment(attachment).ok())
            .expect("the eat animation selects an attachment");
        assert_eq!(eye.name(), "eye-closed");
        let draw_order = skeleton
            .draw_order()
            .map(|slot| {
                asset
                    .slot(slot.id())
                    .expect("draw slot belongs to the fixture")
                    .name()
            })
            .collect::<Vec<_>>();
        assert_eq!(draw_order[4], "tail-slot");
    }

    #[test]
    fn event_crossfade_override_and_opt_in_tripwire_are_observable() {
        let asset = fixture();
        let mut skeleton = Skeleton::new(Arc::clone(&asset));
        let mut player = AnimationPlayer::new(&skeleton);
        let idle = asset.animation_id("idle").expect("idle animation");
        player
            .play(idle, PlayOptions::looping())
            .expect("idle belongs to the fixture");
        let mut events = Vec::new();
        let event_frame = player
            .update(
                &mut skeleton,
                Duration::from_millis(460),
                &mut |event: spinal::AnimationEvent<'_>| {
                    events.push(event.definition().name().to_owned());
                },
            )
            .expect("player update succeeds")
            .solve();
        assert!(!event_frame.has_degradations());
        assert_eq!(events, ["blink"]);

        let fall = asset.animation_id("fall").expect("fall animation");
        player
            .play(
                fall,
                PlayOptions::once().with_transition(smooth_crossfade()),
            )
            .expect("fall belongs to the fixture");
        let mixed = player
            .update(&mut skeleton, CROSSFADE / 2, &mut ())
            .expect("crossfade update succeeds")
            .solve();
        assert!(!mixed.has_degradations());
        assert_near(
            player
                .status()
                .transition_mix()
                .expect("crossfade remains active at its midpoint")
                .get(),
            0.5,
        );

        let head = asset
            .bone(asset.bone_id(OVERRIDE_BONE).expect("head bone"))
            .expect("head belongs to the fixture");
        let setup = head.setup_transform();
        let replaced = offset_rotation(setup, 25.0);
        assert_eq!(replaced.translation(), setup.translation());
        assert_eq!(replaced.scale(), setup.scale());
        assert_eq!(replaced.shear(), setup.shear());
        assert_near(
            replaced.rotation().as_degrees(),
            setup.rotation().as_degrees() + 25.0,
        );

        let tripwire = asset.skin_id(TRIPWIRE_SKIN).expect("tripwire skin");
        skeleton
            .set_skin_layers(&[tripwire])
            .expect("tripwire belongs to the fixture");
        skeleton.reset_to_setup_pose();
        let degraded = skeleton.editable_pose().solve();
        assert!(degraded.has_degradations());
        assert!(
            degraded.active_diagnostics().any(|diagnostic| {
                diagnostic.code() == DiagnosticCode::UnsupportedAttachmentType
            })
        );
        assert_eq!(degraded.draw_items().count(), 6);
    }

    #[test]
    fn arguments_select_typed_asset_animation_skins_and_scale() {
        let ParseResult::Run(options) = ViewerOptions::parse(
            [
                "--asset-root",
                "exports",
                "--asset",
                "cat.spine.json",
                "--animation",
                "eat",
                "--skins",
                "collar/red,hat/blue",
                "--scale",
                "2.5",
                "--mouse-target",
                "crosshair",
                "--tripwire",
            ]
            .map(str::to_owned),
        )
        .expect("valid viewer arguments") else {
            panic!("valid arguments should run the viewer");
        };

        assert_eq!(options.asset_root, "exports");
        assert_eq!(options.asset.as_deref(), Some("cat.spine.json"));
        assert_eq!(options.animation.as_deref(), Some("eat"));
        assert_eq!(
            options.skins.iter().map(AsRef::as_ref).collect::<Vec<_>>(),
            ["collar/red", "hat/blue"]
        );
        assert_eq!(options.scale, 2.5);
        assert_eq!(options.mouse_target.as_deref(), Some("crosshair"));
        assert!(options.tripwire);
    }

    #[test]
    fn world_mouse_position_is_inverted_through_the_instance_transform() {
        let transform = GlobalTransform::from(
            Transform::from_xyz(10.0, 20.0, 0.0).with_scale(Vec3::splat(2.0)),
        );
        let point = world_to_skeleton_point(&transform, Vec2::new(14.0, 26.0))
            .expect("finite nonsingular transform");
        assert_near(point.x, 2.0);
        assert_near(point.y, 3.0);

        let singular = GlobalTransform::from(Transform::from_scale(Vec3::ZERO));
        assert!(world_to_skeleton_point(&singular, Vec2::ZERO).is_none());
    }

    #[test]
    fn mouse_target_accounts_for_a_nonidentity_root_setup_transform() {
        let asset = spinal::load_json(
            br#"{
                "skeleton": { "spine": "4.3.23" },
                "bones": [
                    { "name": "root", "x": 10, "y": 20, "rotation": 90 },
                    { "name": "crosshair", "parent": "root" },
                    { "name": "nested", "parent": "crosshair" }
                ]
            }"#,
            b"page.png\nsize:1,1\n",
        )
        .expect("mouse target fixture should load")
        .into_asset();
        let target = asset
            .bone(asset.bone_id("crosshair").expect("crosshair bone"))
            .expect("crosshair belongs to the fixture");
        let parent = target_parent_setup_world(&asset, target)
            .expect("a direct child of a rotated root is supported");
        let local = Vec2::new(2.0, 3.0);
        let skeleton = parent.transform_point(local);
        let recovered = parent
            .try_inverse_point(skeleton)
            .expect("the root transform is nonsingular");
        assert_near(recovered.x, local.x);
        assert_near(recovered.y, local.y);

        let nested = asset
            .bone(asset.bone_id("nested").expect("nested bone"))
            .expect("nested belongs to the fixture");
        assert!(target_parent_setup_world(&asset, nested).is_none());
    }
}

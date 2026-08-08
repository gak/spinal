//! A deliberately small procedural walk editor for Spine JSON exports.
//!
//! The editor changes only one animation inside the selected JSON file. It
//! does not modify the Spine project file.

#[path = "animator/json_document.rs"]
mod json_document;
#[path = "animator/rig.rs"]
mod rig;
#[path = "animator/rig_debug.rs"]
mod rig_debug;
#[path = "animator/save.rs"]
mod save;
#[path = "animator/walk.rs"]
mod walk;

use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use accesskit::{Action, Node as Accessible, Role, Toggled};
use bevy::{
    a11y::{AccessibilityNode, ActionRequest},
    asset::{AssetPlugin, AssetServer, Assets, LoadState},
    input_focus::{
        InputDispatchPlugin, InputFocus,
        tab_navigation::{TabGroup, TabIndex, TabNavigationPlugin},
    },
    prelude::*,
    transform::TransformSystems,
    window::WindowResizeConstraints,
};
use bevy_spinal::{
    BoneOverride, SpinalAsset, SpinalAtlasPage, SpinalInstance, SpinalInstanceState, SpinalIssue,
    SpinalPlugin, SpinalPoseOverrides, SpinalSet,
};
use spinal::{AlphaEncoding, BoneTransform, Skeleton, SkeletonAsset};

use rig::RigBinding;
use rig_debug::{MarkerKind, SegmentKind};
use save::{SaveState, save_walk};
use walk::WalkParameters;

const NORMAL_BUTTON: Color = Color::srgb(0.16, 0.18, 0.23);
const HOVERED_BUTTON: Color = Color::srgb(0.24, 0.28, 0.36);
const PRESSED_BUTTON: Color = Color::srgb(0.23, 0.55, 0.38);
const PANEL: Color = Color::srgba(0.055, 0.065, 0.085, 0.96);
const TEXT: Color = Color::srgb(0.90, 0.92, 0.96);
const MUTED_TEXT: Color = Color::srgb(0.65, 0.69, 0.76);
const SUCCESS: Color = Color::srgb(0.40, 0.90, 0.58);
const ERROR: Color = Color::srgb(1.0, 0.38, 0.42);
const RIG_BONE: Color = Color::srgb(0.20, 0.88, 0.95);
const RIG_PARENT: Color = Color::srgba(0.50, 0.57, 0.68, 0.60);
const RIG_IK: Color = Color::srgb(1.0, 0.69, 0.18);
const RIG_CONTROL: Color = Color::srgb(0.30, 1.0, 0.48);
const RIG_TARGET: Color = Color::srgb(1.0, 0.35, 0.72);
const RIG_BODY: Color = Color::srgb(1.0, 0.87, 0.30);
const RIG_TRANSFORM: Color = Color::srgb(0.72, 0.50, 1.0);

fn main() -> AppExit {
    let options = match Options::parse(env::args().skip(1)) {
        Ok(ParseResult::Run(options)) => options,
        Ok(ParseResult::Help) => {
            print_help();
            return AppExit::Success;
        }
        Err(error) => {
            eprintln!("spinal animator: {error}\n");
            print_help();
            return AppExit::error();
        }
    };
    let prepared = match PreparedSource::load(options) {
        Ok(prepared) => prepared,
        Err(error) => {
            eprintln!("spinal animator: {error}");
            return AppExit::error();
        }
    };
    let asset_root = prepared
        .atlas_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_string_lossy()
        .into_owned();

    App::new()
        .insert_resource(ClearColor(Color::srgb(0.035, 0.042, 0.056)))
        .insert_resource(prepared)
        .init_resource::<InputFocus>()
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: asset_root,
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Spinal mini animator".into(),
                        resolution: (1100, 720).into(),
                        resizable: true,
                        resize_constraints: WindowResizeConstraints {
                            min_width: 860.0,
                            min_height: 680.0,
                            ..default()
                        },
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins((SpinalPlugin, InputDispatchPlugin, TabNavigationPlugin))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                button_hover,
                handle_buttons,
                handle_accessibility_actions,
                handle_shortcuts,
                advance_preview,
                apply_preview_pose,
            )
                .chain()
                .before(SpinalSet::Animate),
        )
        .add_systems(
            Update,
            (observe_preview, update_labels, update_focus_outline)
                .chain()
                .after(SpinalSet::Animate),
        )
        .add_systems(
            PostUpdate,
            draw_rig_overlay.after(TransformSystems::Propagate),
        )
        .add_systems(Last, restore_accessibility_labels)
        .run()
}

#[derive(Debug)]
struct Options {
    json_path: PathBuf,
    atlas_path: Option<PathBuf>,
    animation_name: Box<str>,
}

enum ParseResult {
    Run(Options),
    Help,
}

impl Options {
    fn parse(arguments: impl IntoIterator<Item = String>) -> Result<ParseResult, String> {
        let mut arguments = arguments.into_iter();
        let mut json_path = None;
        let mut atlas_path = None;
        let mut animation_name: Box<str> = "walk".into();
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "-h" | "--help" => return Ok(ParseResult::Help),
                "--atlas" => {
                    atlas_path = Some(PathBuf::from(next_value(&mut arguments, "--atlas")?));
                }
                "--animation" => {
                    animation_name = next_value(&mut arguments, "--animation")?.into();
                    if animation_name.is_empty() {
                        return Err("--animation must not be empty".to_owned());
                    }
                }
                option if option.starts_with('-') => {
                    return Err(format!("unknown option `{option}`"));
                }
                path if json_path.is_none() => json_path = Some(PathBuf::from(path)),
                path => return Err(format!("unexpected second JSON path `{path}`")),
            }
        }
        let json_path = json_path.ok_or_else(|| "a skeleton JSON path is required".to_owned())?;
        Ok(ParseResult::Run(Options {
            json_path,
            atlas_path,
            animation_name,
        }))
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
Spinal mini animator

USAGE:
    animator SKELETON.json [--atlas FILE.atlas] [--animation NAME]

The atlas is found automatically when the JSON directory contains exactly one
.atlas file. The generated clip defaults to `walk`.

CONTROLS:
    Space             play or pause when no button is focused
    Tab, Enter/Space  select and activate a button
    Cmd/Ctrl+S        save the walk into the JSON
"
    );
}

#[derive(Resource)]
struct PreparedSource {
    json_path: PathBuf,
    atlas_path: PathBuf,
    source: String,
    atlas: Vec<u8>,
    skeleton: Arc<SkeletonAsset>,
    binding: RigBinding,
    animation_name: Box<str>,
    parameters: WalkParameters,
    existing_draft: bool,
}

impl PreparedSource {
    fn load(options: Options) -> Result<Self, String> {
        let json_path = fs::canonicalize(&options.json_path).map_err(|error| {
            format!("could not open `{}`: {error}", options.json_path.display())
        })?;
        let atlas_path = match options.atlas_path {
            Some(path) => fs::canonicalize(&path)
                .map_err(|error| format!("could not open `{}`: {error}", path.display()))?,
            None => discover_atlas(&json_path)?,
        };
        let source = fs::read_to_string(&json_path)
            .map_err(|error| format!("could not read `{}`: {error}", json_path.display()))?;
        let atlas = fs::read(&atlas_path)
            .map_err(|error| format!("could not read `{}`: {error}", atlas_path.display()))?;
        let skeleton = spinal::load_json(source.as_bytes(), &atlas)
            .map_err(|error| format!("the export did not load: {error}"))?
            .into_asset();
        ensure_preview_alpha(&skeleton)?;
        let binding = rig::discover(&skeleton)
            .map_err(|error| format!("this is not the expected four-legged cat rig: {error}"))?;
        let existing = walk::parameters_from_source(&source, &options.animation_name, &binding)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            json_path,
            atlas_path,
            source,
            atlas,
            skeleton,
            binding,
            animation_name: options.animation_name,
            parameters: existing.unwrap_or_default(),
            existing_draft: existing.is_some(),
        })
    }
}

fn ensure_preview_alpha(skeleton: &SkeletonAsset) -> Result<(), String> {
    let Some(page) = skeleton
        .atlas_pages()
        .find(|page| page.alpha_encoding() != AlphaEncoding::Straight)
    else {
        return Ok(());
    };
    Err(format!(
        "atlas page `{}` uses premultiplied alpha (`pma: true`), which this preview cannot draw. Re-export with Premultiply alpha off and Bleed on, or pass a prepared straight-alpha atlas with --atlas. The JSON was not changed.",
        page.name()
    ))
}

fn discover_atlas(json_path: &Path) -> Result<PathBuf, String> {
    let conventional = json_path.with_extension("atlas");
    if conventional.is_file() {
        return fs::canonicalize(&conventional).map_err(|error| error.to_string());
    }
    let parent = json_path.parent().unwrap_or_else(|| Path::new("."));
    let mut candidates = fs::read_dir(parent)
        .map_err(|error| format!("could not inspect `{}`: {error}", parent.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "atlas")
        })
        .collect::<Vec<_>>();
    candidates.sort();
    match candidates.as_slice() {
        [atlas] => fs::canonicalize(atlas).map_err(|error| error.to_string()),
        [] => Err(format!(
            "no .atlas file was found beside `{}`; pass --atlas",
            json_path.display()
        )),
        _many => Err(format!(
            "more than one .atlas file was found beside `{}`; pass --atlas",
            json_path.display()
        )),
    }
}

#[derive(Resource)]
struct EditorState {
    entity: Entity,
    binding: RigBinding,
    save: SaveState,
    atlas: Vec<u8>,
    animation_name: Box<str>,
    parameters: WalkParameters,
    playing: bool,
    position: f32,
    show_rig: bool,
    status: String,
    status_color: Color,
    preview_issue: Option<String>,
    preview_status: String,
    preview_status_color: Color,
    page_images: Vec<(Box<str>, Handle<Image>)>,
}

#[derive(Resource)]
struct RigDebugSkeleton(Skeleton);

#[derive(Clone, Copy, Component)]
enum EditorAction {
    TogglePlay,
    ToggleRig,
    Stride(f32),
    Lift(f32),
    Bob(f32),
    Reset,
    Save,
}

impl EditorAction {
    const fn accessible_label(self) -> &'static str {
        match self {
            Self::TogglePlay => "Pause walk preview",
            Self::ToggleRig => "Show rig",
            Self::Stride(delta) if delta < 0.0 => "Decrease stride",
            Self::Stride(_delta) => "Increase stride",
            Self::Lift(delta) if delta < 0.0 => "Decrease paw lift",
            Self::Lift(_delta) => "Increase paw lift",
            Self::Bob(delta) if delta < 0.0 => "Decrease body bob",
            Self::Bob(_delta) => "Increase body bob",
            Self::Reset => "Reset walk parameters",
            Self::Save => "Save walk to JSON",
        }
    }

    const fn accessible_role(self) -> Role {
        match self {
            Self::ToggleRig => Role::CheckBox,
            _other => Role::Button,
        }
    }
}

#[derive(Component)]
struct PlayLabel;

#[derive(Clone, Copy, Component)]
enum ValueLabel {
    Stride,
    Lift,
    Bob,
}

#[derive(Component)]
struct StatusLabel;

#[derive(Component)]
struct PreviewLabel;

#[derive(Component)]
struct RigToggleLabel;

#[derive(Component)]
struct RigDetails;

type ButtonInteractions<'world, 'state> = Query<
    'world,
    'state,
    (&'static Interaction, &'static mut BackgroundColor),
    (Changed<Interaction>, With<Button>),
>;
type PlayLabels<'world, 'state> = Query<
    'world,
    'state,
    &'static mut Text,
    (
        With<PlayLabel>,
        Without<ValueLabel>,
        Without<StatusLabel>,
        Without<PreviewLabel>,
        Without<RigToggleLabel>,
    ),
>;
type ValueLabels<'world, 'state> = Query<
    'world,
    'state,
    (&'static ValueLabel, &'static mut Text),
    (
        Without<PlayLabel>,
        Without<StatusLabel>,
        Without<PreviewLabel>,
        Without<RigToggleLabel>,
    ),
>;
type StatusLabels<'world, 'state> = Query<
    'world,
    'state,
    (&'static mut Text, &'static mut TextColor),
    (
        With<StatusLabel>,
        Without<PlayLabel>,
        Without<ValueLabel>,
        Without<PreviewLabel>,
        Without<RigToggleLabel>,
    ),
>;
type PreviewLabels<'world, 'state> = Query<
    'world,
    'state,
    (&'static mut Text, &'static mut TextColor),
    (
        With<PreviewLabel>,
        Without<PlayLabel>,
        Without<ValueLabel>,
        Without<StatusLabel>,
        Without<RigToggleLabel>,
    ),
>;
type RigToggleLabels<'world, 'state> = Query<
    'world,
    'state,
    &'static mut Text,
    (
        With<RigToggleLabel>,
        Without<PlayLabel>,
        Without<ValueLabel>,
        Without<StatusLabel>,
        Without<PreviewLabel>,
    ),
>;

fn setup(
    mut commands: Commands<'_, '_>,
    prepared: Res<'_, PreparedSource>,
    asset_server: Res<'_, AssetServer>,
    mut assets: ResMut<'_, Assets<SpinalAsset>>,
) {
    commands.spawn(Camera2d);
    let mut page_images = Vec::new();
    let pages = prepared
        .skeleton
        .atlas_pages()
        .map(|page| {
            let handle = asset_server.load::<Image>(page.name().to_owned());
            page_images.push((page.name().into(), handle.clone()));
            SpinalAtlasPage::new(page.name(), handle)
        })
        .collect::<Vec<_>>();
    let asset = SpinalAsset::new(Arc::clone(&prepared.skeleton), pages)
        .expect("the prepared page table comes from the same loaded skeleton");
    let entity = commands
        .spawn((
            SpinalInstance::new(assets.add(asset)),
            Transform::from_xyz(-135.0, -180.0, 0.0).with_scale(Vec3::splat(1.2)),
        ))
        .id();

    commands.insert_resource(RigDebugSkeleton(Skeleton::new(Arc::clone(
        &prepared.skeleton,
    ))));
    commands.insert_resource(EditorState {
        entity,
        binding: prepared.binding.clone(),
        save: SaveState {
            source_path: prepared.json_path.clone(),
            original: prepared.source.clone(),
            backup_path: None,
        },
        atlas: prepared.atlas.clone(),
        animation_name: prepared.animation_name.clone(),
        parameters: prepared.parameters,
        playing: true,
        position: 0.0,
        show_rig: false,
        status: if prepared.existing_draft {
            "Loaded existing mini-animator walk".to_owned()
        } else {
            "Draft is not saved yet".to_owned()
        },
        status_color: MUTED_TEXT,
        preview_issue: None,
        preview_status: "Preview: loading textures".to_owned(),
        preview_status_color: MUTED_TEXT,
        page_images,
    });
    spawn_ui(
        &mut commands,
        &prepared.animation_name,
        &prepared.skeleton,
        &prepared.binding,
    );
}

fn spawn_ui(
    commands: &mut Commands<'_, '_>,
    animation_name: &str,
    skeleton: &SkeletonAsset,
    binding: &RigBinding,
) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(16),
                right: px(16),
                width: px(300),
                padding: UiRect::all(px(18)),
                flex_direction: FlexDirection::Column,
                row_gap: px(12),
                border_radius: BorderRadius::all(px(12)),
                ..default()
            },
            BackgroundColor(PANEL),
            TabGroup::new(0),
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new("Mini walk animator"),
                TextFont::from_font_size(25.0),
                TextColor(TEXT),
            ));
            panel.spawn((
                Text::new(format!("Editing animations.{animation_name}")),
                TextFont::from_font_size(14.0),
                TextColor(MUTED_TEXT),
            ));
            panel.spawn((
                Text::new("Preview: loading textures"),
                TextFont::from_font_size(13.0),
                TextColor(MUTED_TEXT),
                PreviewLabel,
            ));
            panel
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(8),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn(button_bundle(EditorAction::TogglePlay))
                        .with_child((
                            Text::new("Pause"),
                            TextFont::from_font_size(16.0),
                            TextColor(TEXT),
                            PlayLabel,
                        ));
                    row.spawn(button_bundle(EditorAction::Reset)).with_child((
                        Text::new("Reset"),
                        TextFont::from_font_size(16.0),
                        TextColor(TEXT),
                    ));
                });
            panel
                .spawn(button_bundle(EditorAction::ToggleRig))
                .with_child((
                    Text::new(rig_toggle_label(false)),
                    TextFont::from_font_size(16.0),
                    TextColor(TEXT),
                    RigToggleLabel,
                ));
            panel.spawn((
                Text::new(rig_details(skeleton, binding)),
                TextFont::from_font_size(12.0),
                TextColor(MUTED_TEXT),
                Node {
                    display: Display::None,
                    ..default()
                },
                RigDetails,
            ));
            spawn_parameter_row(
                panel,
                "Stride",
                ValueLabel::Stride,
                EditorAction::Stride(-2.0),
                EditorAction::Stride(2.0),
            );
            spawn_parameter_row(
                panel,
                "Paw lift",
                ValueLabel::Lift,
                EditorAction::Lift(-1.0),
                EditorAction::Lift(1.0),
            );
            spawn_parameter_row(
                panel,
                "Body bob",
                ValueLabel::Bob,
                EditorAction::Bob(-0.5),
                EditorAction::Bob(0.5),
            );
            panel.spawn((
                Text::new(
                    "The body is lowered 10 px so the nearly straight knees have room to bend.",
                ),
                TextFont::from_font_size(13.0),
                TextColor(MUTED_TEXT),
            ));
            panel.spawn(button_bundle(EditorAction::Save)).with_child((
                Text::new("Save walk to JSON"),
                TextFont::from_font_size(17.0),
                TextColor(TEXT),
            ));
            panel.spawn((
                Text::new("Draft is not saved yet"),
                TextFont::from_font_size(14.0),
                TextColor(MUTED_TEXT),
                StatusLabel,
            ));
            panel.spawn((
                Text::new(
                    "This changes the exported JSON only. The source .spine project is untouched.",
                ),
                TextFont::from_font_size(12.0),
                TextColor(MUTED_TEXT),
            ));
        });
}

fn spawn_parameter_row(
    panel: &mut ChildSpawnerCommands<'_>,
    name: &str,
    marker: ValueLabel,
    decrease: EditorAction,
    increase: EditorAction,
) {
    panel
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(8),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new(name),
                TextFont::from_font_size(16.0),
                TextColor(TEXT),
                Node {
                    width: px(100),
                    ..default()
                },
            ));
            row.spawn(button_bundle(decrease)).with_child((
                Text::new("-"),
                TextFont::from_font_size(18.0),
                TextColor(TEXT),
            ));
            row.spawn((
                Text::new("0"),
                TextFont::from_font_size(16.0),
                TextColor(TEXT),
                Node {
                    width: px(48),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                marker,
            ));
            row.spawn(button_bundle(increase)).with_child((
                Text::new("+"),
                TextFont::from_font_size(18.0),
                TextColor(TEXT),
            ));
        });
}

fn button_bundle(action: EditorAction) -> impl Bundle {
    let mut accessible = Accessible::new(action.accessible_role());
    accessible.set_label(action.accessible_label());
    accessible.add_action(Action::Click);
    if matches!(action, EditorAction::ToggleRig) {
        accessible.set_toggled(Toggled::False);
    }
    (
        Button,
        action,
        TabIndex(0),
        AccessibilityNode(accessible),
        Node {
            min_width: px(42),
            height: px(36),
            padding: UiRect::horizontal(px(11)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border_radius: BorderRadius::all(px(7)),
            ..default()
        },
        BackgroundColor(NORMAL_BUTTON),
    )
}

const fn rig_toggle_label(show_rig: bool) -> &'static str {
    if show_rig {
        "[x] Show rig"
    } else {
        "[ ] Show rig"
    }
}

fn rig_details(skeleton: &SkeletonAsset, binding: &RigBinding) -> String {
    const ROLES: [&str; 4] = ["Hind near", "Fore near", "Hind far", "Fore far"];
    let mut details = format!(
        "{} bones | {} IK | {} transform\nCyan bones | amber IK | green controls | pink targets | yellow body",
        skeleton.bones().len(),
        skeleton.ik_constraints().len(),
        skeleton.transform_constraints().len(),
    );
    for (role, control) in ROLES.into_iter().zip(&binding.controls) {
        let target = skeleton.bone_id(&control.name).and_then(|control_id| {
            skeleton
                .ik_constraints()
                .find(|constraint| {
                    skeleton
                        .bone(constraint.target())
                        .is_ok_and(|target| target.parent() == Some(control_id))
                })
                .and_then(|constraint| skeleton.bone(constraint.target()).ok())
        });
        if let Some(target) = target {
            details.push_str(&format!("\n{role}: {} -> {}", control.name, target.name()));
        }
    }
    details
}

fn button_hover(mut buttons: ButtonInteractions<'_, '_>) {
    for (interaction, mut color) in &mut buttons {
        *color = match interaction {
            Interaction::Pressed => PRESSED_BUTTON,
            Interaction::Hovered => HOVERED_BUTTON,
            Interaction::None => NORMAL_BUTTON,
        }
        .into();
    }
}

fn handle_buttons(
    interactions: Query<'_, '_, (Entity, &Interaction, &EditorAction), Changed<Interaction>>,
    mut editor: ResMut<'_, EditorState>,
    mut focus: ResMut<'_, InputFocus>,
) {
    for (entity, interaction, action) in &interactions {
        if *interaction == Interaction::Pressed {
            focus.0 = Some(entity);
            apply_action(*action, &mut editor);
        }
    }
}

fn handle_accessibility_actions(
    mut requests: MessageReader<'_, '_, ActionRequest>,
    actions: Query<'_, '_, &EditorAction>,
    mut editor: ResMut<'_, EditorState>,
    mut focus: ResMut<'_, InputFocus>,
) {
    for request in requests.read() {
        if request.action != Action::Click {
            continue;
        }
        let Some(entity) = Entity::try_from_bits(request.target.0) else {
            continue;
        };
        let Ok(action) = actions.get(entity) else {
            continue;
        };
        focus.0 = Some(entity);
        apply_action(*action, &mut editor);
    }
}

fn handle_shortcuts(
    keys: Res<'_, ButtonInput<KeyCode>>,
    focus: Res<'_, InputFocus>,
    actions: Query<'_, '_, &EditorAction>,
    mut editor: ResMut<'_, EditorState>,
) {
    let focused_action = focus.0.and_then(|entity| actions.get(entity).ok()).copied();
    let activate_focused = keys.just_pressed(KeyCode::Enter)
        || (focused_action.is_some() && keys.just_pressed(KeyCode::Space));
    if activate_focused {
        if let Some(action) = focused_action {
            apply_action(action, &mut editor);
        }
    } else if keys.just_pressed(KeyCode::Space) {
        apply_action(EditorAction::TogglePlay, &mut editor);
    }
    let modifier = keys.any_pressed([
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
    ]);
    if modifier && keys.just_pressed(KeyCode::KeyS) {
        apply_action(EditorAction::Save, &mut editor);
    }
}

fn update_focus_outline(
    mut commands: Commands<'_, '_>,
    focus: Res<'_, InputFocus>,
    buttons: Query<'_, '_, Entity, With<Button>>,
) {
    if !focus.is_changed() {
        return;
    }
    for button in &buttons {
        if focus.0 == Some(button) {
            commands.entity(button).insert(Outline {
                color: Color::WHITE,
                width: px(2),
                offset: px(2),
            });
        } else {
            commands.entity(button).remove::<Outline>();
        }
    }
}

fn apply_action(action: EditorAction, editor: &mut EditorState) {
    match action {
        EditorAction::TogglePlay => editor.playing = !editor.playing,
        EditorAction::ToggleRig => editor.show_rig = !editor.show_rig,
        EditorAction::Stride(delta) => {
            editor.parameters.stride += delta;
            editor.parameters.clamp();
            mark_changed(editor);
        }
        EditorAction::Lift(delta) => {
            editor.parameters.lift += delta;
            editor.parameters.clamp();
            mark_changed(editor);
        }
        EditorAction::Bob(delta) => {
            editor.parameters.bob += delta;
            editor.parameters.clamp();
            mark_changed(editor);
        }
        EditorAction::Reset => {
            editor.parameters = WalkParameters::default();
            editor.position = 0.0;
            mark_changed(editor);
        }
        EditorAction::Save => {
            match save_walk(
                &mut editor.save,
                &editor.atlas,
                &editor.animation_name,
                &editor.binding,
                editor.parameters,
            ) {
                Ok(receipt) => {
                    editor.status = format!(
                        "Saved. Backup: {}",
                        receipt.backup_path.file_name().map_or_else(
                            || receipt.backup_path.display().to_string(),
                            |name| name.to_string_lossy().into_owned()
                        )
                    );
                    editor.status_color = SUCCESS;
                }
                Err(error) => {
                    editor.status = format!("Save failed: {error}");
                    editor.status_color = ERROR;
                }
            }
        }
    }
}

fn mark_changed(editor: &mut EditorState) {
    editor.status = "Unsaved changes".to_owned();
    editor.status_color = MUTED_TEXT;
}

fn advance_preview(time: Res<'_, Time>, mut editor: ResMut<'_, EditorState>) {
    if editor.playing {
        editor.position =
            (editor.position + time.delta_secs()).rem_euclid(editor.parameters.duration());
    }
}

fn apply_preview_pose(
    editor: Res<'_, EditorState>,
    mut instances: Query<'_, '_, &mut SpinalPoseOverrides>,
) {
    let Ok(mut overrides) = instances.get_mut(editor.entity) else {
        return;
    };
    let pose = editor.parameters.sample_preview(editor.position);
    for (control, offset) in editor.binding.controls.iter().zip(pose.controls) {
        overrides.set(BoneOverride::new(
            control.name.clone(),
            translated(control.setup, offset),
        ));
    }
    overrides.set(BoneOverride::new(
        editor.binding.body.name.clone(),
        translated(editor.binding.body.setup, pose.body),
    ));
}

fn observe_preview(
    mut issues: MessageReader<'_, '_, SpinalIssue>,
    states: Query<'_, '_, &SpinalInstanceState>,
    asset_server: Res<'_, AssetServer>,
    mut editor: ResMut<'_, EditorState>,
) {
    let entity = editor.entity;
    for issue in issues.read().filter(|issue| issue.entity() == entity) {
        editor.preview_issue = Some(format!("{:?}: {}", issue.kind(), issue.message()));
    }
    if let Some((page, error)) = editor.page_images.iter().find_map(|(page, handle)| {
        match asset_server.load_state(handle.id()) {
            LoadState::Failed(error) => Some((page.as_ref(), error)),
            LoadState::NotLoaded | LoadState::Loading | LoadState::Loaded => None,
        }
    }) {
        editor.preview_status = format!("Preview failed to load `{page}`: {error}");
        editor.preview_status_color = ERROR;
        return;
    }
    let Ok(state) = states.get(entity) else {
        return;
    };
    let (status, color) = match state {
        SpinalInstanceState::Loading => ("Preview: loading textures".to_owned(), MUTED_TEXT),
        SpinalInstanceState::Ready => {
            editor.preview_issue = None;
            ("Preview: ready".to_owned(), SUCCESS)
        }
        SpinalInstanceState::ReadyNoDraws => ("Preview: no drawable attachments".to_owned(), ERROR),
        SpinalInstanceState::Degraded => (
            format!(
                "Preview warning: {}",
                editor
                    .preview_issue
                    .as_deref()
                    .unwrap_or("unsupported data is marked with a red cross")
            ),
            ERROR,
        ),
        SpinalInstanceState::DegradedNoDraws => (
            format!(
                "Preview unavailable: {}",
                editor
                    .preview_issue
                    .as_deref()
                    .unwrap_or("unsupported data produced no drawable attachments")
            ),
            ERROR,
        ),
        SpinalInstanceState::Failed => (
            format!(
                "Preview failed: {}",
                editor
                    .preview_issue
                    .as_deref()
                    .unwrap_or("asset load failed")
            ),
            ERROR,
        ),
        _other => ("Preview: unknown runtime state".to_owned(), ERROR),
    };
    editor.preview_status = status;
    editor.preview_status_color = color;
}

fn translated(setup: BoneTransform, offset: Vec2) -> BoneTransform {
    BoneTransform::new(
        setup.translation() + offset,
        setup.rotation(),
        setup.scale(),
        setup.shear(),
    )
    .expect("finite editor parameters preserve a finite setup transform")
}

fn update_labels(
    editor: Res<'_, EditorState>,
    mut play: PlayLabels<'_, '_>,
    mut values: ValueLabels<'_, '_>,
    mut status: StatusLabels<'_, '_>,
    mut preview: PreviewLabels<'_, '_>,
    mut rig_toggle: RigToggleLabels<'_, '_>,
    mut rig_details: Query<'_, '_, &mut Node, With<RigDetails>>,
) {
    if let Ok(mut label) = play.single_mut() {
        **label = if editor.playing { "Pause" } else { "Play" }.to_owned();
    }
    for (value, mut label) in &mut values {
        **label = match value {
            ValueLabel::Stride => format!("{:.0} px", editor.parameters.stride),
            ValueLabel::Lift => format!("{:.0} px", editor.parameters.lift),
            ValueLabel::Bob => format!("{:.1} px", editor.parameters.bob),
        };
    }
    if let Ok((mut label, mut color)) = status.single_mut() {
        **label = editor.status.clone();
        color.0 = editor.status_color;
    }
    if let Ok((mut label, mut color)) = preview.single_mut() {
        **label = editor.preview_status.clone();
        color.0 = editor.preview_status_color;
    }
    if let Ok(mut label) = rig_toggle.single_mut() {
        **label = rig_toggle_label(editor.show_rig).to_owned();
    }
    if let Ok(mut node) = rig_details.single_mut() {
        node.display = if editor.show_rig {
            Display::Flex
        } else {
            Display::None
        };
    }
}

fn draw_rig_overlay(
    editor: Res<'_, EditorState>,
    mut debug: ResMut<'_, RigDebugSkeleton>,
    transforms: Query<'_, '_, &GlobalTransform>,
    mut gizmos: Gizmos<'_, '_>,
) {
    if !editor.show_rig {
        return;
    }
    let Ok(transform) = transforms.get(editor.entity) else {
        return;
    };
    let geometry = rig_debug::solve_geometry(
        true,
        &mut debug.0,
        &editor.binding,
        editor.parameters,
        editor.position,
    );
    let to_world = |point: Vec2| transform.transform_point(point.extend(0.0)).truncate();

    for segment in geometry.segments {
        let color = match segment.kind {
            SegmentKind::Bone => RIG_BONE,
            SegmentKind::ParentLink => RIG_PARENT,
            SegmentKind::IkChain | SegmentKind::IkLink => RIG_IK,
            SegmentKind::TransformConstraint => RIG_TRANSFORM,
        };
        let start = to_world(segment.start);
        let end = to_world(segment.end);
        if start.is_finite() && end.is_finite() {
            gizmos.line_2d(start, end, color);
        }
    }

    for marker in geometry.markers {
        let position = to_world(marker.position);
        if !position.is_finite() {
            continue;
        }
        match marker.kind {
            MarkerKind::Joint => draw_diamond(&mut gizmos, position, 1.7, RIG_BONE),
            MarkerKind::IkControl => draw_square(&mut gizmos, position, 4.0, RIG_CONTROL),
            MarkerKind::IkTarget => draw_ring(&mut gizmos, position, 5.0, RIG_TARGET),
            MarkerKind::BodyControl => draw_diamond(&mut gizmos, position, 5.0, RIG_BODY),
            MarkerKind::Problem => draw_cross(&mut gizmos, position, 6.0, ERROR),
        }
    }
}

fn draw_square(gizmos: &mut Gizmos<'_, '_>, center: Vec2, radius: f32, color: Color) {
    gizmos.lineloop_2d(
        [
            center + Vec2::new(-radius, -radius),
            center + Vec2::new(radius, -radius),
            center + Vec2::new(radius, radius),
            center + Vec2::new(-radius, radius),
        ],
        color,
    );
}

fn draw_diamond(gizmos: &mut Gizmos<'_, '_>, center: Vec2, radius: f32, color: Color) {
    gizmos.lineloop_2d(
        [
            center + Vec2::Y * radius,
            center + Vec2::X * radius,
            center - Vec2::Y * radius,
            center - Vec2::X * radius,
        ],
        color,
    );
}

fn draw_ring(gizmos: &mut Gizmos<'_, '_>, center: Vec2, radius: f32, color: Color) {
    const SIDES: usize = 12;
    gizmos.lineloop_2d(
        (0..SIDES).map(|index| {
            let angle = std::f32::consts::TAU * index as f32 / SIDES as f32;
            center + Vec2::from_angle(angle) * radius
        }),
        color,
    );
}

fn draw_cross(gizmos: &mut Gizmos<'_, '_>, center: Vec2, radius: f32, color: Color) {
    let diagonal = Vec2::splat(radius);
    gizmos.line_2d(center - diagonal, center + diagonal, color);
    let other = Vec2::new(radius, -radius);
    gizmos.line_2d(center - other, center + other, color);
}

fn restore_accessibility_labels(
    editor: Res<'_, EditorState>,
    mut buttons: Query<'_, '_, (&EditorAction, &mut AccessibilityNode)>,
) {
    for (action, mut accessibility) in &mut buttons {
        let label = match action {
            EditorAction::TogglePlay if !editor.playing => "Play walk preview",
            _other => action.accessible_label(),
        };
        accessibility.set_role(action.accessible_role());
        accessibility.set_label(label);
        accessibility.add_action(Action::Click);
        if matches!(action, EditorAction::ToggleRig) {
            accessibility.set_toggled(Toggled::from(editor.show_rig));
        } else {
            accessibility.clear_toggled();
        }
    }
}

#[cfg(test)]
mod app_tests {
    use super::*;

    #[test]
    fn positional_json_and_optional_values_parse() {
        let ParseResult::Run(options) = Options::parse([
            "cat.json".to_owned(),
            "--atlas".to_owned(),
            "cat.atlas".to_owned(),
            "--animation".to_owned(),
            "walk-draft".to_owned(),
        ])
        .expect("arguments parse") else {
            panic!("expected run options");
        };
        assert_eq!(options.json_path, Path::new("cat.json"));
        assert_eq!(options.atlas_path.as_deref(), Some(Path::new("cat.atlas")));
        assert_eq!(options.animation_name.as_ref(), "walk-draft");
    }

    #[test]
    fn premultiplied_atlas_is_rejected_before_opening_a_blank_preview() {
        let pma_atlas = String::from_utf8(rig::TEST_ATLAS.to_vec())
            .expect("test atlas is UTF-8")
            .replace("pma:false", "pma:true");
        let skeleton = spinal::load_json(rig::TEST_JSON, pma_atlas.as_bytes())
            .expect("premultiplied alpha is a retained diagnostic")
            .into_asset();

        let error = ensure_preview_alpha(&skeleton).expect_err("PMA preview is rejected");
        assert!(error.contains("Premultiply alpha off"));
        assert!(error.contains("JSON was not changed"));
    }

    #[test]
    fn rig_toggle_uses_checkbox_semantics_and_clear_visible_states() {
        let action = EditorAction::ToggleRig;

        assert_eq!(action.accessible_role(), Role::CheckBox);
        assert_eq!(action.accessible_label(), "Show rig");
        assert_eq!(rig_toggle_label(false), "[ ] Show rig");
        assert_eq!(rig_toggle_label(true), "[x] Show rig");
        assert_eq!(EditorAction::TogglePlay.accessible_role(), Role::Button);
    }

    #[test]
    fn assistive_technology_click_toggles_rig_without_editing_the_walk() {
        let asset = spinal::load_json(rig::TEST_JSON, rig::TEST_ATLAS)
            .expect("fixture loads")
            .into_asset();
        let binding = rig::discover(&asset).expect("fixture rig is discovered");
        let parameters = WalkParameters::default();
        let mut app = App::new();
        app.add_message::<ActionRequest>()
            .init_resource::<InputFocus>()
            .insert_resource(EditorState {
                entity: Entity::PLACEHOLDER,
                binding,
                save: SaveState {
                    source_path: PathBuf::new(),
                    original: String::new(),
                    backup_path: None,
                },
                atlas: Vec::new(),
                animation_name: "walk".into(),
                parameters,
                playing: true,
                position: 0.0,
                show_rig: false,
                status: "Unchanged".to_owned(),
                status_color: MUTED_TEXT,
                preview_issue: None,
                preview_status: String::new(),
                preview_status_color: MUTED_TEXT,
                page_images: Vec::new(),
            })
            .add_systems(Update, handle_accessibility_actions);
        let checkbox = app.world_mut().spawn(EditorAction::ToggleRig).id();

        app.world_mut()
            .write_message(ActionRequest(accesskit::ActionRequest {
                action: Action::Click,
                target: accesskit::NodeId(checkbox.to_bits()),
                data: None,
            }));
        app.update();

        let editor = app.world().resource::<EditorState>();
        assert!(editor.show_rig);
        assert_eq!(editor.parameters, parameters);
        assert_eq!(editor.status, "Unchanged");
        assert_eq!(app.world().resource::<InputFocus>().0, Some(checkbox));
    }
}

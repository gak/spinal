//! Private Bevy UI for the read-only animation viewer.

use std::time::Duration;

use accesskit::{Action, Node as Accessible, Role, Toggled};
use bevy::{
    a11y::AccessibilityNode,
    input_focus::tab_navigation::{TabGroup, TabIndex},
    prelude::*,
    ui::{InteractionDisabled, RelativeCursorPosition},
    ui_widgets::{Slider, SliderRange, SliderStep, SliderThumb, SliderValue, TrackClick},
};

use crate::{
    clock::PlaybackSpeed,
    command::{
        CameraNavigationCommand, SkinSelection, StepDirection, ViewerCommand, ZoomDirection,
    },
    diagnostics::{DiagnosticsPresentation, DiagnosticsTone},
    runtime::{ViewerRuntime, source_slot_label},
    session::SourceSlot,
};

pub(crate) const SIDEBAR_WIDTH: f32 = 360.0;
pub(crate) const PREVIEW_PADDING: f32 = 36.0;

pub(crate) const PANEL: Color = Color::srgba(0.045, 0.052, 0.068, 0.98);
pub(crate) const TEXT: Color = Color::srgb(0.91, 0.93, 0.97);
pub(crate) const MUTED_TEXT: Color = Color::srgb(0.63, 0.67, 0.75);
pub(crate) const SUCCESS: Color = Color::srgb(0.43, 0.89, 0.60);
pub(crate) const WARNING: Color = Color::srgb(1.0, 0.72, 0.30);
pub(crate) const ERROR: Color = Color::srgb(1.0, 0.39, 0.44);
pub(crate) const NORMAL_BUTTON: Color = Color::srgb(0.15, 0.18, 0.24);
pub(crate) const HOVERED_BUTTON: Color = Color::srgb(0.23, 0.28, 0.37);
pub(crate) const PRESSED_BUTTON: Color = Color::srgb(0.25, 0.54, 0.42);
pub(crate) const SELECTED_BUTTON: Color = Color::srgb(0.20, 0.42, 0.64);
pub(crate) const DISABLED_BUTTON: Color = Color::srgb(0.09, 0.10, 0.13);
pub(crate) const TIMELINE_STEP: f32 = 0.01;

/// One fixed native playback-speed choice.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlaybackSpeedChoice {
    pub(crate) multiplier: f32,
    pub(crate) label: &'static str,
}

/// The fixed speed choices shared with the browser Preview/Compare controls.
pub(crate) const PLAYBACK_SPEED_CHOICES: [PlaybackSpeedChoice; 5] = [
    PlaybackSpeedChoice {
        multiplier: 0.25,
        label: "0.25×",
    },
    PlaybackSpeedChoice {
        multiplier: 0.5,
        label: "0.5×",
    },
    PlaybackSpeedChoice {
        multiplier: 1.0,
        label: "1×",
    },
    PlaybackSpeedChoice {
        multiplier: 1.5,
        label: "1.5×",
    },
    PlaybackSpeedChoice {
        multiplier: 2.0,
        label: "2×",
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NativeModeCopy {
    pub(crate) window_title: &'static str,
    pub(crate) sidebar_title: &'static str,
    pub(crate) sidebar_subtitle: &'static str,
}

pub(crate) const fn native_mode_copy(has_comparison: bool) -> NativeModeCopy {
    if has_comparison {
        NativeModeCopy {
            window_title: "Spinal — Compare",
            sidebar_title: "Spinal — Compare",
            sidebar_subtitle: "Read-only synchronized comparison",
        }
    } else {
        NativeModeCopy {
            window_title: "Spinal — Preview",
            sidebar_title: "Spinal — Preview",
            sidebar_subtitle: "Read-only preview",
        }
    }
}

#[derive(Clone, Component, Debug, Eq, PartialEq)]
pub(crate) struct ViewerAction(pub(crate) ViewerCommand);

#[derive(Clone, Copy, Component, Debug, Eq, PartialEq)]
pub(crate) enum ViewerLabel {
    Source,
    Version,
    Current,
    Time,
    Frame,
    CameraView,
    RuntimeState,
    LoadStatus,
    LatestIssue,
    IssueHistory,
}

#[derive(Component)]
pub(crate) struct AnimationList;

#[derive(Component)]
pub(crate) struct SkinList;

#[derive(Component)]
pub(crate) struct SidebarScroll;

#[derive(Component)]
pub(crate) struct ViewerViewportFocus;

/// A sidebar control that participates in focus outlining and reveal-on-focus.
#[derive(Component)]
pub(crate) struct SidebarFocusable;

#[derive(Component)]
pub(crate) struct AnimationButtonLabel {
    animation: Box<str>,
    label: Box<str>,
}

impl AnimationButtonLabel {
    pub(crate) fn text(&self, selected_animation: Option<&str>) -> String {
        let mark = if selected_animation == Some(self.animation.as_ref()) {
            "[x]"
        } else {
            "[ ]"
        };
        format!("{mark} {}", self.label)
    }
}

#[derive(Component)]
pub(crate) struct SkinButtonLabel {
    selection: SkinSelection,
    label: Box<str>,
}

impl SkinButtonLabel {
    pub(crate) fn text(&self, selected_skin: &SkinSelection) -> String {
        let mark = if &self.selection == selected_skin {
            "[x]"
        } else {
            "[ ]"
        };
        format!("{mark} {}", self.label)
    }
}

#[derive(Component)]
pub(crate) struct PauseButtonLabel;

/// Visible state label owned by the native loop-toggle button.
#[derive(Component)]
pub(crate) struct LoopButtonLabel;

impl LoopButtonLabel {
    pub(crate) const fn text(looping: bool) -> &'static str {
        if looping { "[x] Loop" } else { "[ ] Loop" }
    }
}

/// Visible state label owned by a fixed playback-speed button.
#[derive(Component)]
pub(crate) struct PlaybackSpeedButtonLabel {
    speed: PlaybackSpeed,
    label: Box<str>,
}

impl PlaybackSpeedButtonLabel {
    pub(crate) fn text(&self, playback_speed: PlaybackSpeed) -> String {
        let mark = if self.speed == playback_speed {
            "[x]"
        } else {
            "[ ]"
        };
        format!("{mark} {}", self.label)
    }
}

/// The normalized native timeline slider.
#[derive(Component)]
pub(crate) struct TimelineControl;

#[derive(Component)]
pub(crate) struct ViewerButton;

#[derive(Clone, Copy, Component, Debug, Eq, PartialEq)]
pub(crate) struct SourceStatusLabel(pub(crate) SourceSlot);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PauseActionCopy {
    pub(crate) accessible_label: &'static str,
    pub(crate) visible_label: &'static str,
}

pub(crate) const fn pause_action_copy(paused: bool) -> PauseActionCopy {
    if paused {
        PauseActionCopy {
            accessible_label: "Resume animation",
            visible_label: "Resume",
        }
    } else {
        PauseActionCopy {
            accessible_label: "Pause animation",
            visible_label: "Pause",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LoopActionCopy {
    pub(crate) accessible_label: &'static str,
    pub(crate) visible_label: &'static str,
}

pub(crate) const fn loop_action_copy(looping: bool) -> LoopActionCopy {
    if looping {
        LoopActionCopy {
            accessible_label: "Loop animation",
            visible_label: LoopButtonLabel::text(true),
        }
    } else {
        LoopActionCopy {
            accessible_label: "Loop animation",
            visible_label: LoopButtonLabel::text(false),
        }
    }
}

pub(crate) fn timeline_fraction(position: Duration, duration: Duration) -> f32 {
    if duration.is_zero() {
        return 0.0;
    }
    (position.min(duration).as_secs_f64() / duration.as_secs_f64()) as f32
}

pub(crate) fn timeline_position_for_fraction(
    fraction: f32,
    duration: Duration,
) -> Option<Duration> {
    if !fraction.is_finite() {
        return None;
    }
    let fraction = fraction.clamp(0.0, 1.0);
    if fraction <= 0.0 || duration.is_zero() {
        return Some(Duration::ZERO);
    }
    if fraction >= 1.0 {
        return Some(duration);
    }
    Duration::try_from_secs_f64(duration.as_secs_f64() * f64::from(fraction)).ok()
}

pub(crate) fn timeline_accessibility(position: Duration, duration: Duration) -> Accessible {
    let position = position.min(duration);
    let mut accessible = Accessible::new(Role::Slider);
    accessible.set_label("Timeline");
    accessible.set_numeric_value(position.as_secs_f64());
    accessible.set_min_numeric_value(0.0);
    accessible.set_max_numeric_value(duration.as_secs_f64());
    if !duration.is_zero() {
        accessible.set_numeric_value_step(duration.as_secs_f64() * f64::from(TIMELINE_STEP));
    }
    accessible.add_action(Action::Decrement);
    accessible.add_action(Action::Increment);
    accessible.add_action(Action::SetValue);
    accessible
}

pub(crate) fn spawn(commands: &mut Commands<'_, '_>, ui_camera: Entity, runtime: &ViewerRuntime) {
    let has_comparison = runtime.has_comparison();
    let mode_copy = native_mode_copy(has_comparison);
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::FlexEnd,
                ..default()
            },
            UiTargetCamera(ui_camera),
            TabGroup::new(0),
        ))
        .with_children(|root| {
            let mut viewport_accessibility = Accessible::new(Role::Group);
            let initial_camera = if has_comparison {
                "Linked view · 100% zoom"
            } else {
                "View · 100% zoom"
            };
            viewport_accessibility.set_label(viewport_accessibility_label(initial_camera));
            root.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    right: px(SIDEBAR_WIDTH),
                    top: px(0),
                    bottom: px(0),
                    ..default()
                },
                TabIndex(0),
                AccessibilityNode(viewport_accessibility),
                Outline::new(px(3), px(-3), Color::NONE),
                ViewerViewportFocus,
            ));
            spawn_source_status(root, SourceSlot::Primary, has_comparison);
            if has_comparison {
                spawn_source_status(root, SourceSlot::Comparison, true);
            }
            root.spawn((
                Node {
                    width: px(SIDEBAR_WIDTH),
                    height: percent(100),
                    padding: UiRect::all(px(18)),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(10),
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
                BackgroundColor(PANEL),
                TabGroup::new(1),
                ScrollPosition::default(),
                RelativeCursorPosition::default(),
                SidebarScroll,
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new(mode_copy.sidebar_title),
                    TextFont::from_font_size(22.0),
                    TextColor(TEXT),
                ));
                panel.spawn((
                    Text::new(mode_copy.sidebar_subtitle),
                    TextFont::from_font_size(13.0),
                    TextColor(MUTED_TEXT),
                ));

                spawn_info(panel, ViewerLabel::Source, "File: preparing source");
                spawn_info(panel, ViewerLabel::Version, "Spine version: -");
                spawn_info(panel, ViewerLabel::Current, "Animation: -");
                spawn_info(panel, ViewerLabel::Time, "Time: 0.000 / 0.000 s");
                spawn_info(panel, ViewerLabel::Frame, "Frame: 0 @ 30 FPS");
                spawn_info(panel, ViewerLabel::CameraView, "View: 100% zoom");
                spawn_info(panel, ViewerLabel::RuntimeState, "Runtime state: loading");
                spawn_info(panel, ViewerLabel::LoadStatus, "Load status: loading");
                panel.spawn((
                    Text::new("Playback"),
                    TextFont::from_font_size(17.0),
                    TextColor(TEXT),
                ));
                panel
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: px(6),
                        flex_wrap: FlexWrap::Wrap,
                        ..default()
                    })
                    .with_children(|row| {
                        spawn_button(
                            row,
                            ViewerCommand::Step(StepDirection::Backward),
                            "Previous frame",
                            "<",
                            false,
                        );
                        let pause_copy =
                            pause_action_copy(runtime.model().transport().is_paused());
                        spawn_button(
                            row,
                            ViewerCommand::TogglePause,
                            pause_copy.accessible_label,
                            pause_copy.visible_label,
                            true,
                        );
                        spawn_button(
                            row,
                            ViewerCommand::Step(StepDirection::Forward),
                            "Next frame",
                            ">",
                            false,
                        );
                        spawn_button(
                            row,
                            ViewerCommand::Restart,
                            "Restart animation",
                            "Restart",
                            false,
                        );
                    });

                let looping = runtime.model().transport().is_looping();
                let loop_copy = loop_action_copy(looping);
                panel
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: px(5),
                        flex_wrap: FlexWrap::Wrap,
                        ..default()
                    })
                    .with_children(|row| {
                        spawn_button(
                            row,
                            ViewerCommand::SetLooping(!looping),
                            loop_copy.accessible_label,
                            loop_copy.visible_label,
                            false,
                        );
                        for choice in PLAYBACK_SPEED_CHOICES {
                            let command = ViewerCommand::set_playback_speed(choice.multiplier)
                                .expect("fixed playback speeds are positive and finite");
                            spawn_button(
                                row,
                                command,
                                &format!("Set playback speed to {}", choice.label),
                                choice.label,
                                false,
                            );
                        }
                    });

                let duration = runtime
                    .selected_entry()
                    .map_or(Duration::ZERO, |(_index, _name, duration)| duration);
                spawn_timeline(
                    panel,
                    runtime.model().transport().position(),
                    duration,
                );

                panel.spawn((
                    Text::new("Camera controls"),
                    TextFont::from_font_size(17.0),
                    TextColor(TEXT),
                ));
                let mut camera_group = Accessible::new(Role::Group);
                camera_group.set_label("Camera controls");
                panel
                    .spawn((
                        Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: px(6),
                            flex_wrap: FlexWrap::Wrap,
                            ..default()
                        },
                        AccessibilityNode(camera_group),
                    ))
                    .with_children(|row| {
                        spawn_button(
                            row,
                            ViewerCommand::Navigate(CameraNavigationCommand::Zoom(
                                ZoomDirection::Out,
                            )),
                            "Zoom out",
                            "−",
                            false,
                        );
                        spawn_button(
                            row,
                            ViewerCommand::Refit,
                            "Fit and reset view",
                            "Fit view",
                            false,
                        );
                        spawn_button(
                            row,
                            ViewerCommand::Navigate(CameraNavigationCommand::Zoom(
                                ZoomDirection::In,
                            )),
                            "Zoom in",
                            "+",
                            false,
                        );
                    });

                panel.spawn((
                    Text::new("Skin (shared)"),
                    TextFont::from_font_size(17.0),
                    TextColor(TEXT),
                ));
                let mut skin_group = Accessible::new(Role::Group);
                skin_group.set_label("Shared skin choices");
                panel
                    .spawn((
                        skin_list_node(),
                        ScrollPosition::default(),
                        RelativeCursorPosition::default(),
                        AccessibilityNode(skin_group),
                        SkinList,
                    ))
                    .with_children(|list| spawn_skin_buttons(list, &[]));

                panel.spawn((
                    Text::new("Animations"),
                    TextFont::from_font_size(17.0),
                    TextColor(TEXT),
                ));
                panel.spawn((
                    Node {
                        width: percent(100),
                        min_height: px(80),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: px(5),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    ScrollPosition::default(),
                    RelativeCursorPosition::default(),
                    AnimationList,
                ));

                panel.spawn((
                    Text::new("Latest runtime issue (history, not active status): none"),
                    TextFont::from_font_size(13.0),
                    TextColor(MUTED_TEXT),
                    ViewerLabel::LatestIssue,
                ));
                panel.spawn((
                    Text::new("Issue history (observations, newest first): none"),
                    TextFont::from_font_size(12.0),
                    TextColor(MUTED_TEXT),
                    ViewerLabel::IssueHistory,
                    Node {
                        max_height: px(92),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                ));
                spawn_diagnostics(panel, runtime);
                panel.spawn((
                    Text::new(
                        "1-9,0 select | Space play/pause | Left/Right step | R restart | F fit view\nTimeline focus: Left/Right scrub | Home/End endpoints\nFocus the viewport: arrows pan | +/- zoom\nTab moves focus; Enter or Space activates the focused button",
                    ),
                    TextFont::from_font_size(11.0),
                    TextColor(MUTED_TEXT),
                ));
            });
        });
}

pub(crate) fn viewport_accessibility_label(camera_summary: &str) -> String {
    format!(
        "Animation viewport. {camera_summary}. Drag to pan. Use the wheel or pinch to zoom. When focused, arrow keys pan, plus and minus zoom, and F fits the view."
    )
}

fn spawn_diagnostics(panel: &mut ChildSpawnerCommands<'_>, runtime: &ViewerRuntime) {
    panel.spawn((
        Text::new("Diagnostics"),
        TextFont::from_font_size(17.0),
        TextColor(TEXT),
    ));
    let has_comparison = runtime.has_comparison();
    let presentations = runtime
        .sources()
        .iter()
        .map(|source| {
            (
                source_slot_label(source.slot(), has_comparison),
                DiagnosticsPresentation::capture(source.inspection()),
            )
        })
        .collect::<Vec<_>>();
    for (label, presentation) in presentations {
        let compact = presentation.compact_text();
        let text = format!("{label}: {compact}");
        let mut accessible = Accessible::new(Role::Group);
        accessible.set_label(format!("{label} source diagnostics. {compact}"));
        panel.spawn((
            Text::new(text),
            TextFont::from_font_size(12.0),
            TextColor(match presentation.tone() {
                DiagnosticsTone::Compatible => SUCCESS,
                DiagnosticsTone::Warning => WARNING,
                DiagnosticsTone::Degraded => ERROR,
            }),
            AccessibilityNode(accessible),
        ));
    }
}

fn spawn_source_status(
    root: &mut ChildSpawnerCommands<'_>,
    slot: SourceSlot,
    has_comparison: bool,
) {
    let (title, accessible_title) = match (slot, has_comparison) {
        (SourceSlot::Primary, true) => ("Primary — loading", "Primary"),
        (SourceSlot::Comparison, true) => ("Comparison — loading", "Comparison"),
        (SourceSlot::Primary, false) => ("Preview — loading", "Preview"),
        (SourceSlot::Comparison, false) => return,
    };
    let mut accessible = Accessible::new(Role::Status);
    accessible.set_label(format!("{accessible_title} status: loading"));
    root.spawn((
        Text::new(title),
        TextFont::from_font_size(13.0),
        TextColor(TEXT),
        SourceStatusLabel(slot),
        AccessibilityNode(accessible),
        Node {
            position_type: PositionType::Absolute,
            left: px(12),
            top: px(12),
            min_height: px(30),
            padding: UiRect::axes(px(10), px(6)),
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.045, 0.052, 0.068, 0.92)),
    ));
}

fn spawn_info(panel: &mut ChildSpawnerCommands<'_>, marker: ViewerLabel, text: &str) {
    panel.spawn((
        Text::new(text),
        TextFont::from_font_size(13.0),
        TextColor(MUTED_TEXT),
        marker,
    ));
}

fn spawn_timeline(panel: &mut ChildSpawnerCommands<'_>, position: Duration, duration: Duration) {
    let position = position.min(duration);
    let fraction = timeline_fraction(position, duration);
    panel
        .spawn((
            Slider {
                track_click: TrackClick::Snap,
                ..default()
            },
            SliderValue(fraction),
            SliderRange::new(0.0, 1.0),
            SliderStep(TIMELINE_STEP),
            TimelineControl,
            SidebarFocusable,
            InteractionDisabled,
            TabIndex(0),
            AccessibilityNode(timeline_accessibility(position, duration)),
            timeline_node(),
            BackgroundColor(DISABLED_BUTTON),
            Outline::new(px(2), px(2), Color::NONE),
        ))
        .with_children(|timeline| {
            timeline.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(10),
                    right: px(10),
                    top: px(20),
                    height: px(4),
                    border_radius: BorderRadius::all(px(2)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.26, 0.29, 0.36)),
            ));
            timeline
                .spawn(Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    right: px(12),
                    top: px(0),
                    bottom: px(0),
                    ..default()
                })
                .with_children(|travel| {
                    travel.spawn((
                        SliderThumb,
                        Node {
                            position_type: PositionType::Absolute,
                            left: percent(fraction * 100.0),
                            top: px(12),
                            width: px(12),
                            height: px(20),
                            border_radius: BorderRadius::all(px(6)),
                            ..default()
                        },
                        BackgroundColor(TEXT),
                    ));
                });
        });
}

fn timeline_node() -> Node {
    Node {
        width: percent(100),
        height: px(44),
        min_height: px(44),
        border_radius: BorderRadius::all(px(6)),
        ..default()
    }
}

pub(crate) fn rebuild_animation_list(
    commands: &mut Commands<'_, '_>,
    list: Entity,
    animations: &[Box<str>],
) {
    commands.entity(list).despawn_children();
    commands.entity(list).with_children(|parent| {
        if animations.is_empty() {
            parent.spawn((
                Text::new("This export contains no animations."),
                TextFont::from_font_size(13.0),
                TextColor(MUTED_TEXT),
            ));
            return;
        }
        for (index, name) in animations.iter().enumerate() {
            let shortcut = match index {
                0..=8 => format!("{}", index + 1),
                9 => "0".to_owned(),
                _other => " ".to_owned(),
            };
            let visible = format!("{shortcut:>2}  {name}");
            let accessible = format!("Select animation {}: {name}", index + 1);
            spawn_button(
                parent,
                ViewerCommand::SelectAnimation(name.clone()),
                &accessible,
                &visible,
                false,
            );
        }
    });
}

pub(crate) fn rebuild_skin_list(commands: &mut Commands<'_, '_>, list: Entity, skins: &[Box<str>]) {
    commands.entity(list).despawn_children();
    commands.entity(list).insert(ScrollPosition::default());
    commands
        .entity(list)
        .with_children(|parent| spawn_skin_buttons(parent, skins));
}

fn spawn_skin_buttons(parent: &mut ChildSpawnerCommands<'_>, skins: &[Box<str>]) {
    for selection in skin_selections(skins) {
        let (accessible, visible) = match &selection {
            SkinSelection::Default => ("Select default skin".to_owned(), "Default".to_owned()),
            SkinSelection::Named(name) => (format!("Select skin: {name}"), name.to_string()),
        };
        spawn_button(
            parent,
            ViewerCommand::SelectSkin(selection),
            &accessible,
            &format!("[ ] {visible}"),
            false,
        );
    }
}

fn skin_list_node() -> Node {
    Node {
        width: percent(100),
        height: px(44),
        max_height: px(44),
        flex_direction: FlexDirection::Row,
        flex_wrap: FlexWrap::NoWrap,
        column_gap: px(5),
        overflow: Overflow::scroll_x(),
        ..default()
    }
}

fn skin_selections(skins: &[Box<str>]) -> impl Iterator<Item = SkinSelection> + '_ {
    std::iter::once(SkinSelection::Default).chain(skins.iter().cloned().map(SkinSelection::Named))
}

fn spawn_button(
    parent: &mut ChildSpawnerCommands<'_>,
    command: ViewerCommand,
    accessible_label: &str,
    visible_label: &str,
    pause_label: bool,
) {
    let initial_looping = match &command {
        ViewerCommand::SetLooping(next_looping) => Some(!next_looping),
        _other => None,
    };
    let playback_speed = match &command {
        ViewerCommand::SetPlaybackSpeed(speed) => Some(*speed),
        _other => None,
    };
    let animation_label = match &command {
        ViewerCommand::SelectAnimation(animation) => Some(AnimationButtonLabel {
            animation: animation.clone(),
            label: visible_label.into(),
        }),
        _other => None,
    };
    let skin_label = match &command {
        ViewerCommand::SelectSkin(selection) => Some(SkinButtonLabel {
            selection: selection.clone(),
            label: selection.name().unwrap_or("Default").into(),
        }),
        _other => None,
    };
    let playback_speed_label = playback_speed.map(|speed| PlaybackSpeedButtonLabel {
        speed,
        label: visible_label.into(),
    });
    let initial_visible_label = animation_label.as_ref().map_or_else(
        || {
            playback_speed_label.as_ref().map_or_else(
                || visible_label.to_owned(),
                |label| label.text(PlaybackSpeed::NORMAL),
            )
        },
        |label| label.text(None),
    );
    let mut accessible = button_accessibility(&command, accessible_label);
    if let Some(looping) = initial_looping {
        accessible.set_toggled(if looping {
            Toggled::True
        } else {
            Toggled::False
        });
    }
    if let Some(speed) = playback_speed {
        accessible.set_toggled(if speed == PlaybackSpeed::NORMAL {
            Toggled::True
        } else {
            Toggled::False
        });
    }
    let mut entity = parent.spawn((
        Button,
        ViewerButton,
        SidebarFocusable,
        ViewerAction(command),
        InteractionDisabled,
        TabIndex(0),
        AccessibilityNode(accessible),
        Node {
            min_width: px(38),
            min_height: px(44),
            padding: UiRect::axes(px(10), px(6)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BackgroundColor(NORMAL_BUTTON),
        Outline::new(px(2), px(2), Color::NONE),
    ));
    entity.with_children(|button| {
        let mut label = button.spawn((
            Text::new(initial_visible_label),
            TextFont::from_font_size(13.0),
            TextColor(TEXT),
        ));
        if pause_label {
            label.insert(PauseButtonLabel);
        }
        if initial_looping.is_some() {
            label.insert(LoopButtonLabel);
        }
        if let Some(playback_speed_label) = playback_speed_label {
            label.insert(playback_speed_label);
        }
        if let Some(skin_label) = skin_label {
            label.insert(skin_label);
        }
        if let Some(animation_label) = animation_label {
            label.insert(animation_label);
        }
    });
}

fn button_accessibility(command: &ViewerCommand, label: &str) -> Accessible {
    let mut accessible = Accessible::new(Role::Button);
    accessible.set_label(label);
    accessible.add_action(Action::Click);
    if matches!(
        command,
        ViewerCommand::SelectAnimation(_)
            | ViewerCommand::SelectSkin(_)
            | ViewerCommand::SetLooping(_)
            | ViewerCommand::SetPlaybackSpeed(_)
    ) {
        accessible.set_toggled(Toggled::False);
    }
    accessible
}

pub(crate) fn command_is_available<'a, 'b>(
    command: &ViewerCommand,
    loaded: bool,
    animations: impl IntoIterator<Item = &'a str>,
    skins: impl IntoIterator<Item = &'b str>,
) -> bool {
    if !loaded {
        return false;
    }
    match command {
        ViewerCommand::Refit | ViewerCommand::Navigate(_) => true,
        ViewerCommand::SelectAnimation(name) => animations
            .into_iter()
            .any(|candidate| candidate == name.as_ref()),
        ViewerCommand::SelectSkin(SkinSelection::Default) => true,
        ViewerCommand::SelectSkin(SkinSelection::Named(name)) => skins
            .into_iter()
            .any(|candidate| candidate == name.as_ref()),
        ViewerCommand::SetLooping(_)
        | ViewerCommand::SetPlaybackSpeed(_)
        | ViewerCommand::SeekAbsolute(_)
        | ViewerCommand::TogglePause
        | ViewerCommand::Restart
        | ViewerCommand::Step(_) => animations.into_iter().next().is_some(),
    }
}

pub(crate) fn command_is_selected(
    command: &ViewerCommand,
    selected_animation: Option<&str>,
    selected_skin: &SkinSelection,
) -> bool {
    match command {
        ViewerCommand::SelectAnimation(name) => selected_animation == Some(name.as_ref()),
        ViewerCommand::SelectSkin(selection) => selected_skin == selection,
        ViewerCommand::SetLooping(_)
        | ViewerCommand::SetPlaybackSpeed(_)
        | ViewerCommand::SeekAbsolute(_)
        | ViewerCommand::TogglePause
        | ViewerCommand::Restart
        | ViewerCommand::Refit
        | ViewerCommand::Navigate(_)
        | ViewerCommand::Step(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_mode_copy_distinguishes_preview_and_compare_everywhere() {
        assert_eq!(
            native_mode_copy(false),
            NativeModeCopy {
                window_title: "Spinal — Preview",
                sidebar_title: "Spinal — Preview",
                sidebar_subtitle: "Read-only preview",
            }
        );
        assert_eq!(
            native_mode_copy(true),
            NativeModeCopy {
                window_title: "Spinal — Compare",
                sidebar_title: "Spinal — Compare",
                sidebar_subtitle: "Read-only synchronized comparison",
            }
        );
    }

    #[test]
    fn pause_copy_names_the_action_that_activation_will_take() {
        assert_eq!(pause_action_copy(false).visible_label, "Pause");
        assert_eq!(pause_action_copy(false).accessible_label, "Pause animation");
        assert_eq!(pause_action_copy(true).visible_label, "Resume");
        assert_eq!(pause_action_copy(true).accessible_label, "Resume animation");
    }

    #[test]
    fn loop_copy_names_state_and_the_next_action_without_color() {
        assert_eq!(loop_action_copy(false).visible_label, "[ ] Loop");
        assert_eq!(loop_action_copy(false).accessible_label, "Loop animation");
        assert_eq!(loop_action_copy(true).visible_label, "[x] Loop");
        assert_eq!(loop_action_copy(true).accessible_label, "Loop animation");
    }

    #[test]
    fn native_speed_choices_match_the_browser_contract_and_validate() {
        assert_eq!(
            PLAYBACK_SPEED_CHOICES.map(|choice| choice.multiplier),
            [0.25, 0.5, 1.0, 1.5, 2.0]
        );
        assert_eq!(
            PLAYBACK_SPEED_CHOICES.map(|choice| choice.label),
            ["0.25×", "0.5×", "1×", "1.5×", "2×"]
        );
        for choice in PLAYBACK_SPEED_CHOICES {
            assert!(ViewerCommand::set_playback_speed(choice.multiplier).is_ok());
        }

        let label = PlaybackSpeedButtonLabel {
            speed: PlaybackSpeed::new(1.5).unwrap(),
            label: "1.5×".into(),
        };
        assert_eq!(label.text(PlaybackSpeed::NORMAL), "[ ] 1.5×");
        assert_eq!(label.text(PlaybackSpeed::new(1.5).unwrap()), "[x] 1.5×");
    }

    #[test]
    fn normalized_timeline_mapping_is_bounded_and_preserves_exact_endpoints() {
        let duration = Duration::from_secs(2);
        assert_eq!(timeline_fraction(Duration::ZERO, duration), 0.0);
        assert_eq!(timeline_fraction(Duration::from_secs(1), duration), 0.5);
        assert_eq!(timeline_fraction(Duration::from_secs(3), duration), 1.0);
        assert_eq!(
            timeline_fraction(Duration::from_secs(1), Duration::ZERO),
            0.0
        );

        assert_eq!(
            timeline_position_for_fraction(-1.0, duration),
            Some(Duration::ZERO)
        );
        assert_eq!(
            timeline_position_for_fraction(0.5, duration),
            Some(Duration::from_secs(1))
        );
        assert_eq!(
            timeline_position_for_fraction(2.0, duration),
            Some(duration)
        );
        assert_eq!(timeline_position_for_fraction(f32::NAN, duration), None);
    }

    #[test]
    fn timeline_accessibility_exposes_one_seconds_based_numeric_value() {
        let accessible = timeline_accessibility(Duration::from_millis(750), Duration::from_secs(2));
        assert_eq!(accessible.role(), Role::Slider);
        assert_eq!(accessible.label(), Some("Timeline"));
        assert_eq!(accessible.value(), None);
        assert_eq!(accessible.numeric_value(), Some(0.75));
        assert_eq!(accessible.min_numeric_value(), Some(0.0));
        assert_eq!(accessible.max_numeric_value(), Some(2.0));
        assert_eq!(
            accessible.numeric_value_step(),
            Some(2.0 * f64::from(TIMELINE_STEP))
        );
        assert!(accessible.supports_action(Action::Decrement));
        assert!(accessible.supports_action(Action::Increment));
        assert!(accessible.supports_action(Action::SetValue));

        let node = timeline_node();
        assert_eq!(node.width, percent(100));
        assert_eq!(node.height, px(44));
        assert_eq!(node.min_height, px(44));
    }

    #[test]
    fn loading_and_failed_views_disable_every_command() {
        let animations = ["idle", "walk", "jump"];
        for command in [
            ViewerCommand::TogglePause,
            ViewerCommand::SetLooping(false),
            ViewerCommand::set_playback_speed(1.5).unwrap(),
            ViewerCommand::SeekAbsolute(Duration::from_millis(500)),
            ViewerCommand::Step(StepDirection::Backward),
            ViewerCommand::Restart,
            ViewerCommand::Refit,
            ViewerCommand::Navigate(CameraNavigationCommand::Zoom(ZoomDirection::In)),
            ViewerCommand::SelectAnimation("idle".into()),
            ViewerCommand::SelectSkin(SkinSelection::Default),
            ViewerCommand::SelectSkin(SkinSelection::Named("hat".into())),
        ] {
            assert!(!command_is_available(
                &command,
                false,
                animations.iter().copied(),
                ["plain", "hat"]
            ));
        }
    }

    #[test]
    fn an_empty_loaded_export_enables_safe_refit_and_skin_selection() {
        assert!(command_is_available(
            &ViewerCommand::Refit,
            true,
            std::iter::empty(),
            std::iter::empty()
        ));
        assert!(!command_is_available(
            &ViewerCommand::TogglePause,
            true,
            std::iter::empty(),
            std::iter::empty()
        ));
        assert!(!command_is_available(
            &ViewerCommand::SelectAnimation("idle".into()),
            true,
            std::iter::empty(),
            std::iter::empty()
        ));
        assert!(command_is_available(
            &ViewerCommand::SelectSkin(SkinSelection::Default),
            true,
            std::iter::empty(),
            std::iter::empty()
        ));
    }

    #[test]
    fn selection_availability_uses_animation_identity() {
        let animations = ["walk", "idle"];

        assert!(command_is_available(
            &ViewerCommand::SelectAnimation("idle".into()),
            true,
            animations.iter().copied(),
            std::iter::empty()
        ));
        assert!(!command_is_available(
            &ViewerCommand::SelectAnimation("missing".into()),
            true,
            animations.iter().copied(),
            std::iter::empty()
        ));
    }

    #[test]
    fn skin_choices_always_start_with_synthetic_default() {
        let skins = [Box::<str>::from("plain"), Box::<str>::from("hat")];

        assert_eq!(
            skin_selections(&skins).collect::<Vec<_>>(),
            [
                SkinSelection::Default,
                SkinSelection::Named("plain".into()),
                SkinSelection::Named("hat".into()),
            ]
        );
    }

    #[test]
    fn selected_animation_and_skin_text_are_not_color_dependent() {
        let animation_label = AnimationButtonLabel {
            animation: "walk".into(),
            label: " 1  walk".into(),
        };
        assert_eq!(animation_label.text(Some("idle")), "[ ]  1  walk");
        assert_eq!(animation_label.text(Some("walk")), "[x]  1  walk");

        let skin_label = SkinButtonLabel {
            selection: SkinSelection::Named("hat".into()),
            label: "hat".into(),
        };

        assert_eq!(skin_label.text(&SkinSelection::Default), "[ ] hat");
        assert_eq!(
            skin_label.text(&SkinSelection::Named("hat".into())),
            "[x] hat"
        );

        let animation = button_accessibility(
            &ViewerCommand::SelectAnimation("walk".into()),
            "Select animation 1: walk",
        );
        assert_eq!(animation.role(), Role::Button);
        assert_eq!(animation.toggled(), Some(Toggled::False));

        let skin = button_accessibility(
            &ViewerCommand::SelectSkin(SkinSelection::Default),
            "Select default skin",
        );
        assert_eq!(skin.role(), Role::Button);
        assert_eq!(skin.toggled(), Some(Toggled::False));
        let restart = button_accessibility(&ViewerCommand::Restart, "Restart animation");
        assert_eq!(restart.role(), Role::Button);
        assert_eq!(restart.toggled(), None);
    }

    #[test]
    fn viewport_accessibility_names_linked_camera_state_without_a_live_role() {
        assert_eq!(
            viewport_accessibility_label("Linked view · 125% zoom · panned"),
            "Animation viewport. Linked view · 125% zoom · panned. Drag to pan. Use the wheel or pinch to zoom. When focused, arrow keys pan, plus and minus zoom, and F fits the view."
        );
    }

    #[test]
    fn large_skin_catalog_is_kept_in_one_bounded_scroll_row() {
        let node = skin_list_node();

        assert_eq!(node.height, px(44));
        assert_eq!(node.max_height, px(44));
        assert_eq!(node.flex_wrap, FlexWrap::NoWrap);
        assert_eq!(node.overflow, Overflow::scroll_x());
    }

    #[test]
    fn skin_selection_uses_the_skin_catalog_without_requiring_animations() {
        let skins = ["plain", "hat"];

        assert!(command_is_available(
            &ViewerCommand::SelectSkin(SkinSelection::Named("hat".into())),
            true,
            std::iter::empty(),
            skins
        ));
        assert!(!command_is_available(
            &ViewerCommand::SelectSkin(SkinSelection::Named("missing".into())),
            true,
            std::iter::empty(),
            skins
        ));
    }

    #[test]
    fn animation_and_skin_selection_have_independent_highlights() {
        let selected_skin = SkinSelection::Named("hat".into());

        assert!(command_is_selected(
            &ViewerCommand::SelectAnimation("walk".into()),
            Some("walk"),
            &selected_skin
        ));
        assert!(command_is_selected(
            &ViewerCommand::SelectSkin(SkinSelection::Named("hat".into())),
            None,
            &selected_skin
        ));
        assert!(!command_is_selected(
            &ViewerCommand::SelectSkin(SkinSelection::Default),
            None,
            &selected_skin
        ));
        assert!(!command_is_selected(
            &ViewerCommand::Restart,
            Some("walk"),
            &selected_skin
        ));
    }
}

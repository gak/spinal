//! Private Bevy UI for the read-only animation viewer.

use accesskit::{Action, Node as Accessible, Role, Toggled};
use bevy::{
    a11y::AccessibilityNode,
    input_focus::tab_navigation::{TabGroup, TabIndex},
    prelude::*,
    ui::{InteractionDisabled, RelativeCursorPosition},
};

use crate::{
    command::{SkinSelection, StepDirection, ViewerCommand},
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

#[derive(Clone, Component, Debug, Eq, PartialEq)]
pub(crate) struct ViewerAction(pub(crate) ViewerCommand);

#[derive(Clone, Copy, Component, Debug, Eq, PartialEq)]
pub(crate) enum ViewerLabel {
    Source,
    Version,
    Current,
    Time,
    Frame,
    RuntimeState,
    LoadStatus,
    Compatibility,
    LatestIssue,
    IssueHistory,
}

#[derive(Component)]
pub(crate) struct AnimationList;

#[derive(Component)]
pub(crate) struct SkinList;

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

#[derive(Component)]
pub(crate) struct ViewerButton;

#[derive(Clone, Copy, Component, Debug, Eq, PartialEq)]
pub(crate) struct SourceStatusLabel(pub(crate) SourceSlot);

pub(crate) fn spawn(commands: &mut Commands<'_, '_>, ui_camera: Entity, has_comparison: bool) {
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
        ))
        .with_children(|root| {
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
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(PANEL),
                TabGroup::default(),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new("Spinal — Preview"),
                    TextFont::from_font_size(22.0),
                    TextColor(TEXT),
                ));
                panel.spawn((
                    Text::new("Read-only preview"),
                    TextFont::from_font_size(13.0),
                    TextColor(MUTED_TEXT),
                ));

                spawn_info(panel, ViewerLabel::Source, "File: preparing source");
                spawn_info(panel, ViewerLabel::Version, "Spine version: -");
                spawn_info(panel, ViewerLabel::Current, "Animation: -");
                spawn_info(panel, ViewerLabel::Time, "Time: 0.000 / 0.000 s");
                spawn_info(panel, ViewerLabel::Frame, "Frame: 0 @ 30 FPS");
                spawn_info(panel, ViewerLabel::RuntimeState, "Runtime state: loading");
                spawn_info(panel, ViewerLabel::LoadStatus, "Load status: loading");
                spawn_info(
                    panel,
                    ViewerLabel::Compatibility,
                    "Source compatibility: checking",
                );

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
                        spawn_button(
                            row,
                            ViewerCommand::TogglePause,
                            "Pause or resume preview",
                            "Pause",
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
                        spawn_button(
                            row,
                            ViewerCommand::Refit,
                            "Fit skeleton to preview",
                            "Fit",
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
                panel.spawn((
                    Text::new(
                        "1-9,0 select | Space play/pause | Left/Right step | R restart | F fit\nTab moves focus; Enter or Space activates the focused button",
                    ),
                    TextFont::from_font_size(11.0),
                    TextColor(MUTED_TEXT),
                ));
            });
        });
}

fn spawn_source_status(
    root: &mut ChildSpawnerCommands<'_>,
    slot: SourceSlot,
    has_comparison: bool,
) {
    let (title, accessible_title) = match (slot, has_comparison) {
        (SourceSlot::Primary, true) => ("Current — loading", "Current"),
        (SourceSlot::Comparison, true) => ("Proposed — loading", "Proposed"),
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
        height: px(42),
        max_height: px(42),
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
    let skin_label = match &command {
        ViewerCommand::SelectSkin(selection) => Some(SkinButtonLabel {
            selection: selection.clone(),
            label: selection.name().unwrap_or("Default").into(),
        }),
        _other => None,
    };
    let accessible = button_accessibility(&command, accessible_label);
    let mut entity = parent.spawn((
        Button,
        ViewerButton,
        ViewerAction(command),
        InteractionDisabled,
        TabIndex(0),
        AccessibilityNode(accessible),
        Node {
            min_width: px(38),
            min_height: px(34),
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
            Text::new(visible_label),
            TextFont::from_font_size(13.0),
            TextColor(TEXT),
        ));
        if pause_label {
            label.insert(PauseButtonLabel);
        }
        if let Some(skin_label) = skin_label {
            label.insert(skin_label);
        }
    });
}

fn button_accessibility(command: &ViewerCommand, label: &str) -> Accessible {
    let mut accessible = Accessible::new(Role::Button);
    accessible.set_label(label);
    accessible.add_action(Action::Click);
    if matches!(command, ViewerCommand::SelectSkin(_)) {
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
        ViewerCommand::Refit => true,
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
        | ViewerCommand::Step(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loading_and_failed_views_disable_every_command() {
        let animations = ["idle", "walk", "jump"];
        for command in [
            ViewerCommand::TogglePause,
            ViewerCommand::Step(StepDirection::Backward),
            ViewerCommand::Restart,
            ViewerCommand::Refit,
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
    fn selected_skin_text_is_not_color_dependent() {
        let label = SkinButtonLabel {
            selection: SkinSelection::Named("hat".into()),
            label: "hat".into(),
        };

        assert_eq!(label.text(&SkinSelection::Default), "[ ] hat");
        assert_eq!(label.text(&SkinSelection::Named("hat".into())), "[x] hat");

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
    fn large_skin_catalog_is_kept_in_one_bounded_scroll_row() {
        let node = skin_list_node();

        assert_eq!(node.height, px(42));
        assert_eq!(node.max_height, px(42));
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

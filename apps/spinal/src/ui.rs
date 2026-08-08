//! Private Bevy UI for the read-only animation viewer.

use accesskit::{Action, Node as Accessible, Role};
use bevy::{
    a11y::AccessibilityNode,
    input_focus::tab_navigation::{TabGroup, TabIndex},
    prelude::*,
    ui::InteractionDisabled,
};

use crate::{
    command::{StepDirection, ViewerCommand},
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

fn spawn_button(
    parent: &mut ChildSpawnerCommands<'_>,
    command: ViewerCommand,
    accessible_label: &str,
    visible_label: &str,
    pause_label: bool,
) {
    let mut accessible = Accessible::new(Role::Button);
    accessible.set_label(accessible_label);
    accessible.add_action(Action::Click);
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
    });
}

pub(crate) fn command_is_available<'a>(
    command: &ViewerCommand,
    loaded: bool,
    animations: impl IntoIterator<Item = &'a str>,
) -> bool {
    if !loaded {
        return false;
    }
    match command {
        ViewerCommand::Refit => true,
        ViewerCommand::SelectAnimation(name) => animations
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
        ] {
            assert!(!command_is_available(
                &command,
                false,
                animations.iter().copied()
            ));
        }
    }

    #[test]
    fn an_empty_loaded_export_only_enables_safe_refit() {
        assert!(command_is_available(
            &ViewerCommand::Refit,
            true,
            std::iter::empty()
        ));
        assert!(!command_is_available(
            &ViewerCommand::TogglePause,
            true,
            std::iter::empty()
        ));
        assert!(!command_is_available(
            &ViewerCommand::SelectAnimation("idle".into()),
            true,
            std::iter::empty()
        ));
    }

    #[test]
    fn selection_availability_uses_animation_identity() {
        let animations = ["walk", "idle"];

        assert!(command_is_available(
            &ViewerCommand::SelectAnimation("idle".into()),
            true,
            animations.iter().copied()
        ));
        assert!(!command_is_available(
            &ViewerCommand::SelectAnimation("missing".into()),
            true,
            animations.iter().copied()
        ));
    }
}

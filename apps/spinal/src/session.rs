//! Pure source and playback session state for Preview and Compare.

use std::time::Duration;

use crate::{
    command::{PlaybackCommand, SkinSelection, ViewerCommand},
    preview::{
        PlaybackEffect, PreviewEffect, PreviewRate, PreviewTimeError, PreviewTransport,
        TransportCommand,
    },
};

/// Stable source positions in a viewer session.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum SourceSlot {
    Primary,
    Comparison,
}

/// Load readiness relevant to synchronized viewer controls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SourceReadiness {
    Loading,
    Ready,
    Failed,
}

impl SourceReadiness {
    const fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }
}

#[derive(Debug)]
struct SessionSource {
    readiness: SourceReadiness,
    animations: Vec<(Box<str>, Duration)>,
    skins: Vec<Box<str>>,
}

impl SessionSource {
    fn new(
        readiness: SourceReadiness,
        animations: impl IntoIterator<Item = (Box<str>, Duration)>,
        skins: impl IntoIterator<Item = Box<str>>,
    ) -> Self {
        Self {
            readiness,
            animations: animations.into_iter().collect(),
            skins: skins.into_iter().collect(),
        }
    }

    fn duration(&self, animation: &str) -> Option<Duration> {
        self.animations
            .iter()
            .find_map(|(name, duration)| (name.as_ref() == animation).then_some(*duration))
    }

    fn has_skin(&self, skin: &str) -> bool {
        self.skins
            .iter()
            .any(|candidate| candidate.as_ref() == skin)
    }
}

/// Host-independent state for one primary and an optional comparison source.
#[derive(Debug)]
pub(crate) struct ViewerSession {
    primary: SessionSource,
    comparison: Option<SessionSource>,
    union_animations: Vec<Box<str>>,
    union_skins: Vec<Box<str>>,
    selected_skin: SkinSelection,
    transport: PreviewTransport,
}

impl ViewerSession {
    pub(crate) fn new(rate: PreviewRate) -> Self {
        Self {
            primary: SessionSource::new(SourceReadiness::Loading, [], []),
            comparison: None,
            union_animations: Vec::new(),
            union_skins: Vec::new(),
            selected_skin: SkinSelection::Default,
            transport: PreviewTransport::new(rate),
        }
    }

    /// Replaces one source's complete name-based catalog and readiness.
    #[cfg(test)]
    pub(crate) fn set_source(
        &mut self,
        slot: SourceSlot,
        readiness: SourceReadiness,
        animations: impl IntoIterator<Item = (Box<str>, Duration)>,
    ) -> Option<PreviewEffect> {
        self.set_source_with_skins(slot, readiness, animations, [])
    }

    /// Replaces one source's animation and non-default skin catalogs.
    pub(crate) fn set_source_with_skins(
        &mut self,
        slot: SourceSlot,
        readiness: SourceReadiness,
        animations: impl IntoIterator<Item = (Box<str>, Duration)>,
        skins: impl IntoIterator<Item = Box<str>>,
    ) -> Option<PreviewEffect> {
        let source = SessionSource::new(readiness, animations, skins);
        match slot {
            SourceSlot::Primary => self.primary = source,
            SourceSlot::Comparison => self.comparison = Some(source),
        }
        self.rebuild_union();
        self.synchronize_transport()
    }

    /// Updates readiness without changing a source's current catalog.
    pub(crate) fn set_readiness(
        &mut self,
        slot: SourceSlot,
        readiness: SourceReadiness,
    ) -> Option<PreviewEffect> {
        let source = match slot {
            SourceSlot::Primary => &mut self.primary,
            SourceSlot::Comparison => self
                .comparison
                .get_or_insert_with(|| SessionSource::new(SourceReadiness::Loading, [], [])),
        };
        source.readiness = readiness;
        self.synchronize_transport()
    }

    pub(crate) fn readiness(&self, slot: SourceSlot) -> Option<SourceReadiness> {
        self.source(slot).map(|source| source.readiness)
    }

    pub(crate) fn duration(&self, slot: SourceSlot, animation: &str) -> Option<Duration> {
        self.source(slot)
            .and_then(|source| source.duration(animation))
    }

    /// Source-independent names in primary order, then comparison-only order.
    pub(crate) fn animations(&self) -> &[Box<str>] {
        &self.union_animations
    }

    /// Non-default skin names in primary order, then comparison-only order.
    pub(crate) fn skins(&self) -> &[Box<str>] {
        &self.union_skins
    }

    pub(crate) const fn selected_skin(&self) -> &SkinSelection {
        &self.selected_skin
    }

    /// Returns whether one source can apply the requested synchronized skin.
    ///
    /// Default/setup is valid for every present source. A missing named skin
    /// is intentionally projected to Default by the shared runtime.
    pub(crate) fn skin_present(&self, slot: SourceSlot, skin: &SkinSelection) -> bool {
        let Some(source) = self.source(slot) else {
            return false;
        };
        match skin {
            SkinSelection::Default => true,
            SkinSelection::Named(name) => source.has_skin(name),
        }
    }

    /// Selects Default/setup or one name from the all-source union.
    fn select_skin(&mut self, skin: SkinSelection) {
        if skin == self.selected_skin
            || skin
                .name()
                .is_some_and(|name| !self.union_skins.iter().any(|item| item.as_ref() == name))
        {
            return;
        }
        self.selected_skin = skin;
    }

    /// The shared review extent for an animation across every present source.
    pub(crate) fn review_duration(&self, animation: &str) -> Option<Duration> {
        [SourceSlot::Primary, SourceSlot::Comparison]
            .into_iter()
            .filter_map(|slot| self.duration(slot, animation))
            .max()
    }

    /// Controls become ready only when every source currently present is ready.
    pub(crate) fn all_present_sources_ready(&self) -> bool {
        self.readiness(SourceSlot::Primary)
            .is_some_and(SourceReadiness::is_ready)
            && self
                .readiness(SourceSlot::Comparison)
                .is_none_or(SourceReadiness::is_ready)
    }

    /// Applies one semantic viewer command to session or transport state.
    pub(crate) fn handle(
        &mut self,
        command: ViewerCommand,
    ) -> Result<Option<PreviewEffect>, PreviewTimeError> {
        let command = match command {
            ViewerCommand::SelectSkin(skin) => {
                self.select_skin(skin);
                return Ok(None);
            }
            ViewerCommand::SelectAnimation(name) => TransportCommand::SelectAnimation(name),
            ViewerCommand::SetLooping(looping) => {
                TransportCommand::Playback(PlaybackCommand::SetLooping(looping))
            }
            ViewerCommand::SetPlaybackSpeed(speed) => {
                TransportCommand::Playback(PlaybackCommand::SetPlaybackSpeed(speed))
            }
            ViewerCommand::SeekAbsolute(position) => {
                TransportCommand::Playback(PlaybackCommand::SeekAbsolute(position))
            }
            ViewerCommand::TogglePause => TransportCommand::TogglePause,
            ViewerCommand::Step(direction) => TransportCommand::Step(direction),
            ViewerCommand::Restart => TransportCommand::Restart,
            ViewerCommand::Refit => TransportCommand::Refit,
            ViewerCommand::Navigate(_command) => return Ok(None),
        };
        self.transport.handle(command)
    }

    /// Applies one host-independent command to the single shared clock.
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "consumed by the compare renderer in the next slice"
        )
    )]
    pub(crate) fn handle_playback(
        &mut self,
        command: PlaybackCommand,
    ) -> Result<Option<PlaybackEffect>, PreviewTimeError> {
        self.transport.handle_playback(command)
    }

    /// Projects shared review time into one source's selected animation.
    ///
    /// In looping mode a shorter source wraps inside its own extent while the
    /// shared clock continues to the longest present duration. In non-looping
    /// mode a shorter source clamps at its end. A missing animation returns
    /// `None`, and a zero-duration source projects to zero.
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "consumed by the compare renderer in the next slice"
        )
    )]
    pub(crate) fn projected_position(
        &self,
        slot: SourceSlot,
    ) -> Result<Option<Duration>, PreviewTimeError> {
        let Some(animation) = self.transport.selected_animation() else {
            return Ok(None);
        };
        let Some(duration) = self.duration(slot, animation) else {
            return Ok(None);
        };
        self.transport.projected_position(duration).map(Some)
    }

    pub(crate) const fn transport(&self) -> &PreviewTransport {
        &self.transport
    }

    fn source(&self, slot: SourceSlot) -> Option<&SessionSource> {
        match slot {
            SourceSlot::Primary => Some(&self.primary),
            SourceSlot::Comparison => self.comparison.as_ref(),
        }
    }

    fn rebuild_union(&mut self) {
        self.union_animations.clear();
        self.union_skins.clear();
        for (name, _duration) in &self.primary.animations {
            push_unique_name(&mut self.union_animations, name);
        }
        for name in &self.primary.skins {
            push_unique_name(&mut self.union_skins, name);
        }
        if let Some(comparison) = &self.comparison {
            for (name, _duration) in &comparison.animations {
                push_unique_name(&mut self.union_animations, name);
            }
            for name in &comparison.skins {
                push_unique_name(&mut self.union_skins, name);
            }
        }
        if self
            .selected_skin
            .name()
            .is_some_and(|name| !self.union_skins.iter().any(|item| item.as_ref() == name))
        {
            self.selected_skin = SkinSelection::Default;
        }
    }

    fn synchronize_transport(&mut self) -> Option<PreviewEffect> {
        if !self.all_present_sources_ready() {
            self.transport.mark_unready();
            return None;
        }
        let animations = self
            .union_animations
            .iter()
            .filter_map(|name| {
                self.review_duration(name)
                    .map(|duration| (name.clone(), duration))
            })
            .collect::<Vec<_>>();
        self.transport.replace_catalog(animations)
    }
}

fn push_unique_name(names: &mut Vec<Box<str>>, candidate: &str) {
    if !names.iter().any(|name| name.as_ref() == candidate) {
        names.push(candidate.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::AdvanceBoundary;

    fn catalog(entries: &[(&str, u64)]) -> Vec<(Box<str>, Duration)> {
        entries
            .iter()
            .map(|(name, milliseconds)| ((*name).into(), Duration::from_millis(*milliseconds)))
            .collect()
    }

    #[test]
    fn reversed_catalog_order_preserves_primary_union_order_and_name_identity() {
        let mut session = ViewerSession::new(PreviewRate::default());
        session.set_source(
            SourceSlot::Primary,
            SourceReadiness::Ready,
            catalog(&[("walk", 100), ("idle", 200)]),
        );
        session.set_source(
            SourceSlot::Comparison,
            SourceReadiness::Ready,
            catalog(&[("idle", 250), ("walk", 150)]),
        );

        assert_eq!(
            session
                .animations()
                .iter()
                .map(AsRef::as_ref)
                .collect::<Vec<_>>(),
            ["walk", "idle"]
        );
        let effect = session
            .handle(ViewerCommand::SelectAnimation("idle".into()))
            .unwrap();
        assert!(matches!(
            effect,
            Some(PreviewEffect::Select(ref request)) if request.animation_name.as_ref() == "idle"
        ));
    }

    #[test]
    fn unequal_durations_are_retained_per_slot_and_use_the_longer_review_extent() {
        let mut session = ViewerSession::new(PreviewRate::default());
        session.set_source(
            SourceSlot::Primary,
            SourceReadiness::Ready,
            catalog(&[("walk", 800)]),
        );
        session.set_source(
            SourceSlot::Comparison,
            SourceReadiness::Ready,
            catalog(&[("walk", 1_250)]),
        );

        assert_eq!(
            session.duration(SourceSlot::Primary, "walk"),
            Some(Duration::from_millis(800))
        );
        assert_eq!(
            session.duration(SourceSlot::Comparison, "walk"),
            Some(Duration::from_millis(1_250))
        );
        assert_eq!(
            session.review_duration("walk"),
            Some(Duration::from_millis(1_250))
        );
    }

    #[test]
    fn one_shared_delta_wraps_the_longer_extent_and_rebases_both_projections() {
        let mut session = ViewerSession::new(PreviewRate::default());
        session.set_source(
            SourceSlot::Primary,
            SourceReadiness::Ready,
            catalog(&[("walk", 80)]),
        );
        session.set_source(
            SourceSlot::Comparison,
            SourceReadiness::Ready,
            catalog(&[("walk", 125)]),
        );

        session
            .handle_playback(PlaybackCommand::SeekAbsolute(Duration::from_millis(110)))
            .unwrap();
        assert_eq!(
            session.projected_position(SourceSlot::Primary).unwrap(),
            Some(Duration::from_millis(30))
        );
        assert_eq!(
            session.projected_position(SourceSlot::Comparison).unwrap(),
            Some(Duration::from_millis(110))
        );
        session
            .handle_playback(PlaybackCommand::SetPaused(false))
            .unwrap();

        let effect = session
            .handle_playback(PlaybackCommand::Advance(Duration::from_millis(20)))
            .unwrap()
            .expect("shared advance effect");
        assert_eq!(effect.boundary, AdvanceBoundary::Wrapped);
        assert_eq!(effect.update.position, Duration::from_millis(5));
        assert_eq!(
            session.projected_position(SourceSlot::Primary).unwrap(),
            Some(Duration::from_millis(5))
        );
        assert_eq!(
            session.projected_position(SourceSlot::Comparison).unwrap(),
            Some(Duration::from_millis(5))
        );
    }

    #[test]
    fn non_looping_shorter_source_clamps_until_shared_completion() {
        let mut session = ViewerSession::new(PreviewRate::default());
        session.set_source(
            SourceSlot::Primary,
            SourceReadiness::Ready,
            catalog(&[("walk", 80)]),
        );
        session.set_source(
            SourceSlot::Comparison,
            SourceReadiness::Ready,
            catalog(&[("walk", 125)]),
        );
        session
            .handle_playback(PlaybackCommand::SetLooping(false))
            .unwrap();
        session
            .handle_playback(PlaybackCommand::SeekAbsolute(Duration::from_millis(110)))
            .unwrap();

        assert_eq!(
            session.projected_position(SourceSlot::Primary).unwrap(),
            Some(Duration::from_millis(80))
        );
        assert_eq!(
            session.projected_position(SourceSlot::Comparison).unwrap(),
            Some(Duration::from_millis(110))
        );
        session
            .handle_playback(PlaybackCommand::SetPaused(false))
            .unwrap();

        let effect = session
            .handle_playback(PlaybackCommand::Advance(Duration::from_millis(20)))
            .unwrap()
            .expect("completion effect");
        assert_eq!(effect.boundary, AdvanceBoundary::Completed);
        assert_eq!(effect.update.position, Duration::from_millis(125));
        assert!(effect.update.paused);
        assert_eq!(
            session.projected_position(SourceSlot::Primary).unwrap(),
            Some(Duration::from_millis(80))
        );
        assert_eq!(
            session.projected_position(SourceSlot::Comparison).unwrap(),
            Some(Duration::from_millis(125))
        );
    }

    #[test]
    fn missing_and_zero_duration_source_projections_are_explicit() {
        let mut session = ViewerSession::new(PreviewRate::default());
        session.set_source(
            SourceSlot::Primary,
            SourceReadiness::Ready,
            catalog(&[("still", 0)]),
        );
        session.set_source(
            SourceSlot::Comparison,
            SourceReadiness::Ready,
            catalog(&[("other", 100)]),
        );

        let effect = session
            .handle_playback(PlaybackCommand::Advance(Duration::from_millis(20)))
            .unwrap()
            .expect("empty extent effect");
        assert_eq!(effect.boundary, AdvanceBoundary::Empty);
        assert_eq!(effect.update.position, Duration::ZERO);
        assert!(effect.update.paused);
        assert_eq!(
            session.projected_position(SourceSlot::Primary).unwrap(),
            Some(Duration::ZERO)
        );
        assert_eq!(
            session.projected_position(SourceSlot::Comparison).unwrap(),
            None
        );
    }

    #[test]
    fn comparison_only_animation_remains_selectable_with_no_primary_duration() {
        let mut session = ViewerSession::new(PreviewRate::default());
        session.set_source(
            SourceSlot::Primary,
            SourceReadiness::Ready,
            catalog(&[("walk", 800)]),
        );
        session.set_source(
            SourceSlot::Comparison,
            SourceReadiness::Ready,
            catalog(&[("walk", 800), ("jump", 400)]),
        );

        assert_eq!(session.duration(SourceSlot::Primary, "jump"), None);
        assert_eq!(
            session.duration(SourceSlot::Comparison, "jump"),
            Some(Duration::from_millis(400))
        );
        assert!(
            session
                .handle(ViewerCommand::SelectAnimation("jump".into()))
                .unwrap()
                .is_some()
        );
        assert_eq!(session.transport().selected_animation(), Some("jump"));
    }

    #[test]
    fn primary_only_animation_reports_a_missing_comparison_side() {
        let mut session = ViewerSession::new(PreviewRate::default());
        session.set_source(
            SourceSlot::Primary,
            SourceReadiness::Ready,
            catalog(&[("idle", 500), ("walk", 800)]),
        );
        session.set_source(
            SourceSlot::Comparison,
            SourceReadiness::Ready,
            catalog(&[("walk", 900)]),
        );

        assert_eq!(
            session.duration(SourceSlot::Primary, "idle"),
            Some(Duration::from_millis(500))
        );
        assert_eq!(session.duration(SourceSlot::Comparison, "idle"), None);
        assert!(
            session
                .animations()
                .iter()
                .any(|name| name.as_ref() == "idle")
        );
    }

    #[test]
    fn skins_use_a_stable_union_and_missing_sources_project_to_default() {
        let mut session = ViewerSession::new(PreviewRate::default());
        session.set_source_with_skins(
            SourceSlot::Primary,
            SourceReadiness::Ready,
            catalog(&[("walk", 800)]),
            [Box::<str>::from("coat"), Box::<str>::from("hat")],
        );
        session.set_source_with_skins(
            SourceSlot::Comparison,
            SourceReadiness::Ready,
            catalog(&[("walk", 900)]),
            [Box::<str>::from("hat"), Box::<str>::from("tail")],
        );

        assert_eq!(
            session
                .skins()
                .iter()
                .map(AsRef::as_ref)
                .collect::<Vec<_>>(),
            ["coat", "hat", "tail"]
        );
        assert_eq!(session.selected_skin(), &SkinSelection::Default);
        assert!(session.skin_present(SourceSlot::Primary, session.selected_skin()));
        assert!(session.skin_present(SourceSlot::Comparison, session.selected_skin()));

        let animation = session.transport().selected_animation().map(str::to_owned);
        let position = session.transport().position();
        let paused = session.transport().is_paused();
        let looping = session.transport().is_looping();
        let speed = session.transport().playback_speed();
        assert_eq!(
            session
                .handle(ViewerCommand::SelectSkin(SkinSelection::Named(
                    "tail".into(),
                )))
                .expect("select skin"),
            None
        );
        assert!(!session.skin_present(SourceSlot::Primary, session.selected_skin()));
        assert!(session.skin_present(SourceSlot::Comparison, session.selected_skin()));
        assert_eq!(
            session.transport().selected_animation(),
            animation.as_deref()
        );
        assert_eq!(session.transport().position(), position);
        assert_eq!(session.transport().is_paused(), paused);
        assert_eq!(session.transport().is_looping(), looping);
        assert_eq!(session.transport().playback_speed(), speed);

        assert_eq!(
            session
                .handle(ViewerCommand::SelectSkin(SkinSelection::Named(
                    "missing".into(),
                )))
                .expect("reject missing skin"),
            None
        );
        assert_eq!(
            session.selected_skin(),
            &SkinSelection::Named("tail".into())
        );
    }

    #[test]
    fn skin_selection_works_without_animations_and_resets_if_catalog_removes_it() {
        let mut session = ViewerSession::new(PreviewRate::default());
        session.set_source_with_skins(
            SourceSlot::Primary,
            SourceReadiness::Ready,
            [],
            [Box::<str>::from("hat")],
        );

        assert!(session.transport().is_ready());
        assert!(session.animations().is_empty());
        assert_eq!(
            session
                .handle(ViewerCommand::SelectSkin(SkinSelection::Named(
                    "hat".into(),
                )))
                .expect("select skin without animations"),
            None
        );
        assert_eq!(session.selected_skin(), &SkinSelection::Named("hat".into()));

        session.set_source_with_skins(
            SourceSlot::Primary,
            SourceReadiness::Ready,
            [],
            [Box::<str>::from("coat")],
        );
        assert_eq!(session.selected_skin(), &SkinSelection::Default);
        assert!(session.skin_present(SourceSlot::Primary, session.selected_skin()));
    }

    #[test]
    fn every_present_source_must_cross_the_readiness_barrier() {
        let mut session = ViewerSession::new(PreviewRate::default());
        assert!(!session.all_present_sources_ready());

        session.set_source(
            SourceSlot::Primary,
            SourceReadiness::Ready,
            catalog(&[("idle", 500)]),
        );
        assert!(session.all_present_sources_ready());
        assert!(session.transport().is_ready());
        assert_eq!(session.readiness(SourceSlot::Comparison), None);

        session.set_source(
            SourceSlot::Comparison,
            SourceReadiness::Loading,
            catalog(&[("idle", 500)]),
        );
        assert!(!session.all_present_sources_ready());
        assert!(!session.transport().is_ready());

        session.set_readiness(SourceSlot::Comparison, SourceReadiness::Ready);
        assert!(session.all_present_sources_ready());
        assert!(session.transport().is_ready());

        session.set_readiness(SourceSlot::Comparison, SourceReadiness::Failed);
        assert!(!session.all_present_sources_ready());
        assert!(!session.transport().is_ready());
    }
}

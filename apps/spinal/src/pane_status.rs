//! Shared, non-live presentation for one Preview or Compare pane.

use std::time::Duration;

use bevy_spinal::SpinalInstanceState;

use crate::{command::SkinSelection, runtime::ViewerLoadState};
#[cfg(any(feature = "native", target_arch = "wasm32"))]
use crate::{
    runtime::{RuntimeSnapshot, ViewerRuntime},
    session::SourceSlot,
};

/// Severity of one source runtime, shared by host status and pane presentation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimePresentation {
    Loading,
    Ready,
    Warning,
    BlockedLoad,
    BlockedRuntime,
    BlockedNoDraws,
}

impl RuntimePresentation {
    /// Stable non-color state exposed by browser pane markup.
    #[cfg(any(feature = "web", test))]
    pub(crate) const fn state_attribute(self) -> &'static str {
        match self {
            Self::Loading => "loading",
            Self::Ready => "ready",
            Self::Warning => "warning",
            Self::BlockedLoad | Self::BlockedRuntime | Self::BlockedNoDraws => "blocked",
        }
    }
}

pub(crate) fn classify_runtime(
    load_state: &ViewerLoadState,
    runtime_state: &SpinalInstanceState,
) -> RuntimePresentation {
    match load_state {
        ViewerLoadState::Loading => RuntimePresentation::Loading,
        ViewerLoadState::Failed(_) => RuntimePresentation::BlockedLoad,
        ViewerLoadState::Ready => match runtime_state {
            SpinalInstanceState::Loading => RuntimePresentation::Loading,
            SpinalInstanceState::Ready => RuntimePresentation::Ready,
            SpinalInstanceState::Degraded => RuntimePresentation::Warning,
            SpinalInstanceState::ReadyNoDraws | SpinalInstanceState::DegradedNoDraws => {
                RuntimePresentation::BlockedNoDraws
            }
            SpinalInstanceState::Failed => RuntimePresentation::BlockedRuntime,
            _ => RuntimePresentation::BlockedRuntime,
        },
    }
}

#[cfg(any(feature = "web", test))]
pub(crate) fn aggregate_presentations(
    presentations: impl IntoIterator<Item = RuntimePresentation>,
) -> RuntimePresentation {
    presentations
        .into_iter()
        .max_by_key(|presentation| match presentation {
            RuntimePresentation::Ready => 0_u8,
            RuntimePresentation::Warning => 1,
            RuntimePresentation::Loading => 2,
            RuntimePresentation::BlockedNoDraws => 3,
            RuntimePresentation::BlockedRuntime => 4,
            RuntimePresentation::BlockedLoad => 5,
        })
        .unwrap_or(RuntimePresentation::BlockedRuntime)
}

/// Stable, clock-free semantics for one viewer pane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PaneSemanticPresentation {
    heading: &'static str,
    state: RuntimePresentation,
    summary: Box<str>,
}

impl PaneSemanticPresentation {
    #[cfg(any(feature = "native", target_arch = "wasm32"))]
    pub(crate) fn capture(snapshot: &RuntimeSnapshot, slot: SourceSlot) -> Self {
        let has_comparison = snapshot.source(SourceSlot::Comparison).is_some();
        let heading = pane_heading(slot, has_comparison);
        let Some(source) = snapshot.source(slot) else {
            return Self {
                heading,
                state: RuntimePresentation::BlockedRuntime,
                summary: "Blocked — source is unavailable".into(),
            };
        };
        pane_semantic_presentation(PaneSemanticContext {
            heading,
            load_state: source.load_state(),
            runtime_state: source.runtime_state(),
            selected_animation: snapshot.selected_animation(),
            selected_present: source.selected_present(),
            selected_skin: snapshot.selected_skin(),
            selected_skin_present: source.selected_skin_present(),
        })
    }

    #[cfg(any(feature = "web", test))]
    pub(crate) const fn heading(&self) -> &'static str {
        self.heading
    }

    pub(crate) const fn state(&self) -> RuntimePresentation {
        self.state
    }

    #[cfg(any(feature = "web", test))]
    pub(crate) fn summary(&self) -> &str {
        &self.summary
    }

    #[cfg(feature = "native")]
    pub(crate) fn visible_text(&self) -> String {
        format!("{} — {}", self.heading, self.summary)
    }

    #[cfg(feature = "native")]
    pub(crate) fn accessible_label(&self) -> String {
        format!("{} pane: {}", self.heading, self.summary)
    }
}

#[derive(Clone, Copy)]
struct PaneSemanticContext<'a> {
    heading: &'static str,
    load_state: &'a ViewerLoadState,
    runtime_state: &'a SpinalInstanceState,
    selected_animation: Option<&'a str>,
    selected_present: bool,
    selected_skin: &'a SkinSelection,
    selected_skin_present: bool,
}

fn pane_semantic_presentation(context: PaneSemanticContext<'_>) -> PaneSemanticPresentation {
    let PaneSemanticContext {
        heading,
        load_state,
        runtime_state,
        selected_animation,
        selected_present,
        selected_skin,
        selected_skin_present,
    } = context;
    let classified = classify_runtime(load_state, runtime_state);
    let (state, summary): (RuntimePresentation, Box<str>) = match classified {
        RuntimePresentation::Loading => {
            let detail = if matches!(load_state, ViewerLoadState::Loading) {
                "Loading — preparing source"
            } else {
                "Loading — preparing runtime"
            };
            (classified, detail.into())
        }
        RuntimePresentation::BlockedLoad => {
            debug_assert!(matches!(load_state, ViewerLoadState::Failed(_)));
            (classified, "Blocked — bundle load failed".into())
        }
        RuntimePresentation::BlockedRuntime => (classified, "Blocked — runtime failed".into()),
        RuntimePresentation::BlockedNoDraws => (
            classified,
            "Blocked — runtime produced no drawable output".into(),
        ),
        RuntimePresentation::Ready | RuntimePresentation::Warning => {
            let animation_fallback = selected_animation.is_some() && !selected_present;
            let skin_fallback = selected_skin.name().is_some() && !selected_skin_present;
            let state = if classified == RuntimePresentation::Warning
                || animation_fallback
                || skin_fallback
            {
                RuntimePresentation::Warning
            } else {
                RuntimePresentation::Ready
            };
            let animation = match (selected_animation, selected_present) {
                (None, _) => "setup pose (no animation selected)".to_owned(),
                (Some(name), true) => format!("animation “{name}”"),
                (Some(name), false) => format!("animation “{name}” unavailable; setup pose"),
            };
            let skin = match selected_skin {
                SkinSelection::Default => "skin Default".to_owned(),
                SkinSelection::Named(name) if selected_skin_present => format!("skin “{name}”"),
                SkinSelection::Named(name) => {
                    format!("skin “{name}” unavailable; Default fallback")
                }
            };
            let mut summary = format!(
                "{} — {animation} • {skin}",
                if state == RuntimePresentation::Ready {
                    "Ready"
                } else {
                    "Warning"
                }
            );
            if classified == RuntimePresentation::Warning {
                summary.push_str(" • runtime warnings");
            }
            (state, summary.into_boxed_str())
        }
    };
    PaneSemanticPresentation {
        heading,
        state,
        summary,
    }
}

#[cfg(any(feature = "native", target_arch = "wasm32"))]
const fn pane_heading(slot: SourceSlot, has_comparison: bool) -> &'static str {
    match (slot, has_comparison) {
        (SourceSlot::Primary, false) => "Preview",
        (SourceSlot::Primary, true) => "Primary",
        (SourceSlot::Comparison, _) => "Comparison",
    }
}

/// Per-frame projected time kept separate from stable pane semantics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PaneTimePresentation {
    Exact {
        position: Duration,
        duration: Duration,
    },
    NotApplicable,
    Unavailable,
}

impl PaneTimePresentation {
    #[cfg(any(feature = "native", target_arch = "wasm32"))]
    pub(crate) fn capture(runtime: &ViewerRuntime, slot: SourceSlot) -> Self {
        let Some(source) = runtime.source(slot) else {
            return Self::NotApplicable;
        };
        if !matches!(
            classify_runtime(source.load_state(), source.runtime_state()),
            RuntimePresentation::Ready | RuntimePresentation::Warning
        ) || !source.selected_present()
        {
            return Self::NotApplicable;
        }
        let Some(animation) = runtime.model().transport().selected_animation() else {
            return Self::NotApplicable;
        };
        let Some(duration) = runtime.model().duration(slot, animation) else {
            return Self::Unavailable;
        };
        match runtime.model().projected_position(slot) {
            Ok(Some(position)) => Self::Exact { position, duration },
            Ok(None) | Err(_) => Self::Unavailable,
        }
    }

    pub(crate) fn visible_text(self) -> Option<String> {
        match self {
            Self::Exact { position, duration } => Some(format!(
                "{:.3} / {:.3} s",
                position.as_secs_f64(),
                duration.as_secs_f64()
            )),
            Self::NotApplicable => None,
            Self::Unavailable => Some("time unavailable".to_owned()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_classification_and_aggregation_preserve_fail_closed_order() {
        assert_eq!(
            classify_runtime(&ViewerLoadState::Loading, &SpinalInstanceState::Loading),
            RuntimePresentation::Loading
        );
        assert_eq!(
            classify_runtime(
                &ViewerLoadState::Failed("private/path".into()),
                &SpinalInstanceState::Ready
            ),
            RuntimePresentation::BlockedLoad
        );
        assert_eq!(
            classify_runtime(&ViewerLoadState::Ready, &SpinalInstanceState::Ready),
            RuntimePresentation::Ready
        );
        assert_eq!(
            classify_runtime(&ViewerLoadState::Ready, &SpinalInstanceState::Degraded),
            RuntimePresentation::Warning
        );
        assert_eq!(
            classify_runtime(
                &ViewerLoadState::Ready,
                &SpinalInstanceState::DegradedNoDraws
            ),
            RuntimePresentation::BlockedNoDraws
        );
        assert_eq!(
            classify_runtime(&ViewerLoadState::Ready, &SpinalInstanceState::Failed),
            RuntimePresentation::BlockedRuntime
        );
        assert_eq!(
            aggregate_presentations([
                RuntimePresentation::Ready,
                RuntimePresentation::BlockedLoad,
                RuntimePresentation::Warning,
            ]),
            RuntimePresentation::BlockedLoad
        );
        assert_eq!(
            aggregate_presentations([]),
            RuntimePresentation::BlockedRuntime
        );
    }

    #[test]
    fn semantic_presentation_is_clock_free_and_explicit_about_fallbacks() {
        let presentation = pane_semantic_presentation(PaneSemanticContext {
            heading: "Comparison",
            load_state: &ViewerLoadState::Ready,
            runtime_state: &SpinalInstanceState::Ready,
            selected_animation: Some("jump"),
            selected_present: false,
            selected_skin: &SkinSelection::Named("hat".into()),
            selected_skin_present: false,
        });
        assert_eq!(presentation.heading(), "Comparison");
        assert_eq!(presentation.state(), RuntimePresentation::Warning);
        assert_eq!(presentation.state().state_attribute(), "warning");
        assert_eq!(
            presentation.summary(),
            "Warning — animation “jump” unavailable; setup pose • skin “hat” unavailable; Default fallback"
        );
        assert!(!presentation.summary().contains("0.000"));
        assert!(!presentation.summary().contains("seconds"));
    }

    #[test]
    fn blocked_and_warning_copy_never_discloses_diagnostics() {
        let failed = pane_semantic_presentation(PaneSemanticContext {
            heading: "Primary",
            load_state: &ViewerLoadState::Failed("/private/atlas missing".into()),
            runtime_state: &SpinalInstanceState::Loading,
            selected_animation: None,
            selected_present: false,
            selected_skin: &SkinSelection::Default,
            selected_skin_present: true,
        });
        assert_eq!(failed.summary(), "Blocked — bundle load failed");
        assert!(!failed.summary().contains("private"));

        let warning = pane_semantic_presentation(PaneSemanticContext {
            heading: "Primary",
            load_state: &ViewerLoadState::Ready,
            runtime_state: &SpinalInstanceState::Degraded,
            selected_animation: None,
            selected_present: true,
            selected_skin: &SkinSelection::Default,
            selected_skin_present: true,
        });
        assert_eq!(
            warning.summary(),
            "Warning — setup pose (no animation selected) • skin Default • runtime warnings"
        );
    }

    #[test]
    fn pane_time_distinguishes_exact_zero_from_unavailable() {
        assert_eq!(
            PaneTimePresentation::Exact {
                position: Duration::ZERO,
                duration: Duration::ZERO,
            }
            .visible_text(),
            Some("0.000 / 0.000 s".to_owned())
        );
        assert_eq!(
            PaneTimePresentation::Unavailable.visible_text(),
            Some("time unavailable".to_owned())
        );
        assert_eq!(PaneTimePresentation::NotApplicable.visible_text(), None);
    }
}

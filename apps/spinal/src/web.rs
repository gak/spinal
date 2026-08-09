//! Thin browser host for the same immutable bundle and runtime used natively.

#[cfg(test)]
use crate::command::ViewerCommand;
#[cfg(any(target_arch = "wasm32", test))]
use crate::{command::SkinSelection, runtime::ViewerLoadState, web_command::BrowserCommandBatch};
#[cfg(any(target_arch = "wasm32", test))]
use bevy_spinal::SpinalInstanceState;

#[cfg(any(target_arch = "wasm32", test))]
const FETCH_TIMEOUT_MS: i32 = 30_000;
#[cfg(any(target_arch = "wasm32", test))]
const LAUNCH_TIMEOUT_MS: i32 = 60_000;

#[cfg(any(target_arch = "wasm32", test))]
struct ExternalCommandDisposition {
    batch: BrowserCommandBatch,
    #[cfg(feature = "phase0b-rehearsal")]
    rejected: bool,
}

#[cfg(any(target_arch = "wasm32", test))]
fn external_command_batch_for_shared_inbox(
    batch: BrowserCommandBatch,
) -> ExternalCommandDisposition {
    #[cfg(feature = "phase0b-rehearsal")]
    {
        // The opt-in harness owns the shared command inbox for the entire run.
        // Authorized browser/UI commands are still drained from the bounded
        // transport queue, but none may perturb its fixed sample schedule.
        let rejected = !batch.commands.is_empty() || batch.overflowed;
        ExternalCommandDisposition {
            batch: BrowserCommandBatch {
                commands: Vec::new(),
                overflowed: false,
            },
            rejected,
        }
    }
    #[cfg(not(feature = "phase0b-rehearsal"))]
    {
        ExternalCommandDisposition { batch }
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn bounded_request_timeout(remaining_launch_ms: f64) -> Option<i32> {
    if !remaining_launch_ms.is_finite() || remaining_launch_ms <= 0.0 {
        return None;
    }
    Some(remaining_launch_ms.ceil().min(f64::from(FETCH_TIMEOUT_MS)) as i32)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg(any(target_arch = "wasm32", test))]
enum RuntimePresentation {
    Loading,
    Ready,
    Warning,
    BlockedLoad,
    BlockedRuntime,
    BlockedNoDraws,
}

#[cfg(any(target_arch = "wasm32", test))]
fn classify_runtime(
    load_state: &ViewerLoadState,
    runtime_state: &SpinalInstanceState,
) -> RuntimePresentation {
    match load_state {
        ViewerLoadState::Loading => RuntimePresentation::Loading,
        ViewerLoadState::Failed(_error) => RuntimePresentation::BlockedLoad,
        ViewerLoadState::Ready => match runtime_state {
            SpinalInstanceState::Loading => RuntimePresentation::Loading,
            SpinalInstanceState::Ready => RuntimePresentation::Ready,
            SpinalInstanceState::Degraded => RuntimePresentation::Warning,
            SpinalInstanceState::ReadyNoDraws | SpinalInstanceState::DegradedNoDraws => {
                RuntimePresentation::BlockedNoDraws
            }
            SpinalInstanceState::Failed => RuntimePresentation::BlockedRuntime,
            _other => RuntimePresentation::BlockedRuntime,
        },
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn aggregate_presentations(
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

#[cfg(any(target_arch = "wasm32", test))]
fn missing_selection_summary<'a>(
    animation: Option<&str>,
    sources: impl IntoIterator<Item = (&'a str, bool)>,
) -> Option<String> {
    let animation = animation?;
    let missing = sources
        .into_iter()
        .filter_map(|(label, present)| (!present).then_some(label))
        .collect::<Vec<_>>();
    (!missing.is_empty()).then(|| {
        format!(
            "{} does not contain animation “{animation}”; showing setup pose in that pane.",
            missing.join(" and ")
        )
    })
}

#[cfg(any(target_arch = "wasm32", test))]
fn missing_skin_summary<'a>(
    skin: &SkinSelection,
    sources: impl IntoIterator<Item = (&'a str, bool)>,
) -> Option<String> {
    let skin = skin.name()?;
    let missing = sources
        .into_iter()
        .filter_map(|(label, present)| (!present).then_some(label))
        .collect::<Vec<_>>();
    (!missing.is_empty()).then(|| {
        format!(
            "{} does not contain skin “{skin}”; showing Default skin in that pane.",
            missing.join(" and ")
        )
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg(any(target_arch = "wasm32", test))]
struct TransportPresentation {
    animation_commands_enabled: bool,
    skin_commands_enabled: bool,
    fit_enabled: bool,
    playback_label: &'static str,
}

#[cfg(any(target_arch = "wasm32", test))]
const fn transport_presentation(
    controls_ready: bool,
    has_animation: bool,
    paused: bool,
) -> TransportPresentation {
    TransportPresentation {
        animation_commands_enabled: controls_ready && has_animation,
        skin_commands_enabled: controls_ready,
        fit_enabled: controls_ready,
        playback_label: if paused { "Play" } else { "Pause" },
    }
}

#[cfg(any(target_arch = "wasm32", test))]
const fn contextual_canvas_label(has_comparison: bool) -> &'static str {
    if has_comparison {
        "Spinal comparison viewport. Current is left; Proposed is right."
    } else {
        "Spinal preview viewport."
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn timeline_value_text(position: std::time::Duration, duration: std::time::Duration) -> String {
    format!(
        "{:.3} of {:.3} seconds",
        position.as_secs_f64(),
        duration.as_secs_f64()
    )
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use std::{
        cell::Cell,
        collections::{BTreeMap, BTreeSet},
        fmt::{self, Write as _},
        path::PathBuf,
        rc::Rc,
        sync::{Arc, Mutex},
    };

    use bevy::{
        asset::AssetPlugin,
        camera::Projection,
        prelude::*,
        winit::{EventLoopProxyWrapper, WinitUserEvent},
    };
    use bevy_spinal::SpinalInstance;
    use js_sys::{Date, Function, Reflect, Uint8Array};
    use wasm_bindgen::{JsCast, JsValue, closure::Closure};
    use wasm_bindgen_futures::{JsFuture, spawn_local};
    use web_sys::{
        AbortController, HtmlCanvasElement, HtmlInputElement, HtmlOptionElement, HtmlSelectElement,
        MessageEvent, ReadableStreamDefaultReader, Request, RequestCache, RequestCredentials,
        RequestInit, RequestMode, RequestRedirect, Response, Url,
    };

    use super::{
        LAUNCH_TIMEOUT_MS, RuntimePresentation, aggregate_presentations, bounded_request_timeout,
        classify_runtime, contextual_canvas_label, external_command_batch_for_shared_inbox,
        missing_selection_summary, missing_skin_summary, timeline_value_text,
        transport_presentation,
    };
    #[cfg(not(feature = "phase0b-rehearsal"))]
    use crate::camera_view::ViewerCameraInputPlugin;
    use crate::{
        camera_fit::{PreviewCamera, ViewerCameraFitPlugin},
        camera_view::{CameraViewState, ViewerCameraViewPlugin, ViewerCameraViewSet},
        diagnostics::{DiagnosticsPresentation, DiagnosticsTone, disclosure_summary},
        preview::PreviewRate,
        runtime::{
            self, CommandInbox, ViewerLoadState, ViewerRuntime, ViewerRuntimePlugin,
            ViewerRuntimeSet, source_slot_label,
        },
        session::SourceSlot,
        viewport::ViewerViewportPlugin,
        web_command::{BrowserCommandProtocol, BrowserCommandQueue, BrowserMessageContext},
        web_manifest::{
            BrowserManifest, BrowserManifestError, BrowserManifestReference, BrowserReviewBundles,
            BrowserReviewManifest, BrowserReviewManifestError, MAX_BROWSER_BUNDLE_BYTES,
            MAX_REVIEW_MANIFEST_BYTES, validate_manifest_location_reference,
        },
    };

    const APP_ELEMENT_ID: &str = "spinal-app";
    const CANVAS_ELEMENT_ID: &str = "spinal-canvas";
    const PREVIEW_HEADING_ELEMENT_ID: &str = "preview-heading";
    const SOURCE_LABELS_ELEMENT_ID: &str = "spinal-source-labels";
    const PRIMARY_LABEL_ELEMENT_ID: &str = "spinal-primary-label";
    const COMPARISON_LABEL_ELEMENT_ID: &str = "spinal-comparison-label";
    const MANIFEST_ATTRIBUTE: &str = "data-spinal-manifest";
    const CAPABILITY_ATTRIBUTE: &str = "data-spinal-command-capability";
    const GRAPHICS_BLOCKED_ATTRIBUTE: &str = "data-spinal-graphics-blocked";
    const PLAY_TOGGLE_ELEMENT_ID: &str = "spinal-play-toggle";
    const STEP_BACKWARD_ELEMENT_ID: &str = "spinal-step-backward";
    const STEP_FORWARD_ELEMENT_ID: &str = "spinal-step-forward";
    const RESTART_ELEMENT_ID: &str = "spinal-restart";
    const REFIT_ELEMENT_ID: &str = "spinal-refit";
    const ZOOM_IN_ELEMENT_ID: &str = "spinal-zoom-in";
    const ZOOM_OUT_ELEMENT_ID: &str = "spinal-zoom-out";
    const CAMERA_STATE_ELEMENT_ID: &str = "spinal-camera-state";
    const ANIMATION_SELECT_ELEMENT_ID: &str = "spinal-animation-select";
    const SKIN_SELECT_ELEMENT_ID: &str = "spinal-skin-select";
    const LOOPING_ELEMENT_ID: &str = "spinal-looping";
    const SPEED_ELEMENT_ID: &str = "spinal-speed";
    const TIMELINE_ELEMENT_ID: &str = "spinal-timeline";
    const TIMELINE_VALUE_ELEMENT_ID: &str = "spinal-timeline-value";
    const DIAGNOSTICS_ELEMENT_ID: &str = "spinal-diagnostics";
    const DIAGNOSTICS_SUMMARY_ELEMENT_ID: &str = "spinal-diagnostics-summary";
    const CAPABILITY_BYTES: usize = 32;

    /// Starts the asynchronous same-origin bundle loader and one Bevy app.
    pub(super) fn run() {
        signal_runtime_started();
        install_panic_status();
        #[cfg(feature = "phase0b-rehearsal")]
        if let Err(error) = crate::phase0b_rehearsal::initialize_dom() {
            set_status(
                StatusKind::Blocked,
                &format!("Viewer blocked — rehearsal output could not start: {error}"),
            );
            web_sys::console::error_1(&JsValue::from_str(&error));
            return;
        }
        set_status(StatusKind::Loading, "Loading preview…");
        spawn_local(async {
            match load_browser_launch().await {
                Ok(launch) => run_app(launch),
                Err(error) => {
                    #[cfg(feature = "phase0b-rehearsal")]
                    crate::phase0b_rehearsal::publish_external_error(
                        "launch_error",
                        &error.to_string(),
                    );
                    set_status(StatusKind::Blocked, &format!("Viewer blocked — {error}"));
                    web_sys::console::error_1(&JsValue::from_str(&error.to_string()));
                }
            }
        });
    }

    struct BrowserLaunch {
        label: Box<str>,
        window_title: &'static str,
        config: runtime::LaunchConfig,
    }

    async fn load_browser_launch() -> Result<BrowserLaunch, BrowserError> {
        let launch_deadline_ms = Date::now() + f64::from(LAUNCH_TIMEOUT_MS);
        let window = web_sys::window().ok_or_else(|| BrowserError::new("window is unavailable"))?;
        let document = window
            .document()
            .ok_or_else(|| BrowserError::new("document is unavailable"))?;
        let manifest_reference = document
            .get_element_by_id(APP_ELEMENT_ID)
            .and_then(|element| element.get_attribute(MANIFEST_ATTRIBUTE))
            .filter(|value| !value.is_empty())
            .ok_or_else(|| BrowserError::new("the page does not declare a bundle manifest"))?;
        let page_url = window
            .location()
            .href()
            .map_err(|_| BrowserError::new("the page URL is unavailable"))?;
        validate_manifest_location_reference(&manifest_reference)?;
        let manifest_url = resolve_bundle_file(&manifest_reference, &page_url, &page_url)?;
        let review_bytes = fetch_bytes(
            &manifest_url,
            MAX_REVIEW_MANIFEST_BYTES,
            None,
            launch_deadline_ms,
        )
        .await?;
        let review = BrowserReviewManifest::parse(&review_bytes)?;
        let has_comparison = review.comparison().is_some();
        configure_shell_mode(&document, has_comparison)?;
        set_status(
            StatusKind::Loading,
            if has_comparison {
                "Loading comparison…"
            } else {
                "Loading preview…"
            },
        );
        let window_title = if has_comparison {
            "Spinal — Compare"
        } else {
            "Spinal — Preview"
        };
        document.set_title(window_title);
        let mut resolved_urls = BTreeSet::from([manifest_url.clone()]);
        let primary_manifest_url = resolve_unique_child_manifest(
            review.primary(),
            &manifest_url,
            &page_url,
            &mut resolved_urls,
        )?;
        let comparison_manifest_url = review
            .comparison()
            .map(|reference| {
                resolve_unique_child_manifest(
                    reference,
                    &manifest_url,
                    &page_url,
                    &mut resolved_urls,
                )
            })
            .transpose()?;
        let primary_manifest_bytes = fetch_bytes(
            &primary_manifest_url,
            review.primary().expected_bytes(),
            Some(review.primary().expected_bytes()),
            launch_deadline_ms,
        )
        .await?;
        let comparison_manifest_bytes = match (review.comparison(), &comparison_manifest_url) {
            (Some(reference), Some(url)) => Some(
                fetch_bytes(
                    url,
                    reference.expected_bytes(),
                    Some(reference.expected_bytes()),
                    launch_deadline_ms,
                )
                .await?,
            ),
            (None, None) => None,
            _other => {
                return Err(BrowserError::new(
                    "comparison manifest state is inconsistent",
                ));
            }
        };
        let manifests = review.validate_runtime_manifests(
            &primary_manifest_bytes,
            comparison_manifest_bytes.as_deref(),
        )?;
        let (primary_manifest, comparison_manifest) = manifests.into_parts();
        let primary_label = primary_manifest.label().to_owned();
        let comparison_label = comparison_manifest
            .as_ref()
            .map(|manifest| manifest.label().to_owned());
        let mut total_bytes = 0_usize;
        let primary = download_runtime_bundle(
            primary_manifest,
            &primary_manifest_url,
            &page_url,
            &mut resolved_urls,
            &mut total_bytes,
            launch_deadline_ms,
        )
        .await?;
        let comparison = match (comparison_manifest, comparison_manifest_url) {
            (Some(manifest), Some(url)) => Some(
                download_runtime_bundle(
                    manifest,
                    &url,
                    &page_url,
                    &mut resolved_urls,
                    &mut total_bytes,
                    launch_deadline_ms,
                )
                .await?,
            ),
            (None, None) => None,
            _other => return Err(BrowserError::new("comparison bundle state is inconsistent")),
        };
        let bundles = BrowserReviewBundles::validate(primary, comparison)?;
        let (primary, comparison) = bundles.into_parts();
        let label: Box<str> = comparison_label.map_or_else(
            || primary_label.clone().into_boxed_str(),
            |comparison| {
                format!("Current: {primary_label}; Proposed: {comparison}").into_boxed_str()
            },
        );
        debug_assert_eq!(has_comparison, comparison.is_some());
        Ok(BrowserLaunch {
            label,
            window_title,
            config: runtime::LaunchConfig::from_bundles(
                primary,
                comparison,
                PreviewRate::default(),
            ),
        })
    }

    fn configure_shell_mode(
        document: &web_sys::Document,
        has_comparison: bool,
    ) -> Result<(), BrowserError> {
        let required = |id: &str| {
            document.get_element_by_id(id).ok_or_else(|| {
                BrowserError::new(format!(
                    "the viewer shell is missing required element `{id}`"
                ))
            })
        };
        let app = required(APP_ELEMENT_ID)?;
        let canvas = required(CANVAS_ELEMENT_ID)?;
        let heading = required(PREVIEW_HEADING_ELEMENT_ID)?;
        let primary_diagnostics_heading = required("spinal-primary-diagnostics-heading")?;
        let labels = required(SOURCE_LABELS_ELEMENT_ID)?;
        let primary_label = required(PRIMARY_LABEL_ELEMENT_ID)?;
        let comparison_label = required(COMPARISON_LABEL_ELEMENT_ID)?;
        let _skin_select = required(SKIN_SELECT_ELEMENT_ID)?;
        let _diagnostics = required(DIAGNOSTICS_ELEMENT_ID)?;
        let _diagnostics_summary = required(DIAGNOSTICS_SUMMARY_ELEMENT_ID)?;
        for prefix in [
            "spinal-primary-diagnostics",
            "spinal-comparison-diagnostics",
        ] {
            for suffix in [
                "",
                "-heading",
                "-compatibility",
                "-inventory",
                "-bundle",
                "-findings",
            ] {
                let _element = required(&format!("{prefix}{suffix}"))?;
            }
        }
        let (mode, heading_text, primary_text, group_label) = if has_comparison {
            (
                "compare",
                "Animation comparison",
                "Current",
                "Comparison views",
            )
        } else {
            ("preview", "Animation preview", "Preview", "Preview view")
        };
        app.set_attribute("data-spinal-mode", mode)
            .map_err(|_| BrowserError::new("could not configure the viewer shell mode"))?;
        heading.set_text_content(Some(heading_text));
        primary_label.set_text_content(Some(primary_text));
        primary_diagnostics_heading.set_text_content(Some(primary_text));
        labels
            .set_attribute("aria-label", group_label)
            .map_err(|_| BrowserError::new("could not label the viewer source panes"))?;
        canvas
            .set_attribute("aria-label", contextual_canvas_label(has_comparison))
            .map_err(|_| BrowserError::new("could not label the viewer canvas"))?;
        if has_comparison {
            comparison_label
                .remove_attribute("hidden")
                .map_err(|_| BrowserError::new("could not show the comparison label"))?;
        } else {
            comparison_label
                .set_attribute("hidden", "")
                .map_err(|_| BrowserError::new("could not hide the comparison label"))?;
        }
        Ok(())
    }

    fn resolve_unique_child_manifest(
        reference: &BrowserManifestReference,
        review_manifest_url: &str,
        page_url: &str,
        resolved_urls: &mut BTreeSet<String>,
    ) -> Result<String, BrowserError> {
        let url = resolve_bundle_file(
            reference.location_reference(),
            review_manifest_url,
            page_url,
        )?;
        if !resolved_urls.insert(url.clone()) {
            return Err(BrowserError::new(format!(
                "two viewer resources resolve to the same URL `{}`",
                redact_url(&url)
            )));
        }
        Ok(url)
    }

    async fn download_runtime_bundle(
        manifest: BrowserManifest,
        manifest_url: &str,
        page_url: &str,
        resolved_urls: &mut BTreeSet<String>,
        total_bytes: &mut usize,
        launch_deadline_ms: f64,
    ) -> Result<crate::bundle::SourceBundle, BrowserError> {
        let mut downloaded = BTreeMap::<PathBuf, Vec<u8>>::new();
        for file in manifest.files() {
            let url = resolve_bundle_file(file.location_reference(), manifest_url, page_url)?;
            if !resolved_urls.insert(url.clone()) {
                return Err(BrowserError::new(format!(
                    "two viewer resources resolve to the same URL `{}`",
                    redact_url(&url)
                )));
            }
            let remaining = MAX_BROWSER_BUNDLE_BYTES.saturating_sub(*total_bytes);
            let effective_limit = file.max_bytes().min(remaining);
            if file.expected_bytes() > effective_limit {
                return Err(BrowserError::new(format!(
                    "bundle file `{}` exceeds the remaining {remaining}-byte viewer budget",
                    file.virtual_path().display()
                )));
            }
            let bytes = fetch_bytes(
                &url,
                effective_limit,
                Some(file.expected_bytes()),
                launch_deadline_ms,
            )
            .await?;
            *total_bytes = total_bytes
                .checked_add(bytes.len())
                .ok_or_else(|| BrowserError::new("browser viewer size overflowed"))?;
            if *total_bytes > MAX_BROWSER_BUNDLE_BYTES {
                return Err(BrowserError::new(format!(
                    "browser viewer exceeds the {MAX_BROWSER_BUNDLE_BYTES}-byte total limit"
                )));
            }
            if downloaded
                .insert(file.virtual_path().to_path_buf(), bytes)
                .is_some()
            {
                return Err(BrowserError::new("duplicate downloaded virtual path"));
            }
        }
        Ok(manifest.into_bundle(downloaded)?)
    }

    fn run_app(launch: BrowserLaunch) {
        let BrowserLaunch {
            label,
            window_title,
            config,
        } = launch;
        let mut app = App::new();
        runtime::prepare_runtime(&mut app, config);
        app.insert_resource(ClearColor(Color::srgb(0.025, 0.030, 0.041)))
            .insert_resource(BrowserLabel(label))
            .init_resource::<BrowserObservation>()
            .init_resource::<BrowserTransportNotice>()
            .add_plugins(
                DefaultPlugins
                    .set(AssetPlugin {
                        watch_for_changes_override: Some(false),
                        use_asset_processor_override: Some(false),
                        ..default()
                    })
                    .set(WindowPlugin {
                        primary_window: Some(Window {
                            title: window_title.into(),
                            canvas: Some(format!("#{CANVAS_ELEMENT_ID}")),
                            fit_canvas_to_parent: true,
                            prevent_default_event_handling: false,
                            ..default()
                        }),
                        ..default()
                    }),
            );
        if let Err(error) = install_command_bridge(&mut app) {
            #[cfg(feature = "phase0b-rehearsal")]
            crate::phase0b_rehearsal::publish_external_error(
                "command_bridge_error",
                &error.to_string(),
            );
            set_status(
                StatusKind::Blocked,
                &format!("Viewer blocked — browser controls could not start: {error}"),
            );
            web_sys::console::error_1(&JsValue::from_str(&error.to_string()));
            return;
        }
        app.add_plugins((
            ViewerRuntimePlugin,
            ViewerViewportPlugin::browser(),
            ViewerCameraFitPlugin::default(),
            ViewerCameraViewPlugin,
        ))
        .add_systems(Startup, publish_diagnostics.after(ViewerRuntimeSet::Setup))
        .add_systems(
            Update,
            drain_browser_commands.before(ViewerRuntimeSet::Commands),
        )
        .add_systems(
            Update,
            (sync_status, publish_transport_notice)
                .chain()
                .after(ViewerRuntimeSet::Observe),
        )
        .add_systems(
            Update,
            publish_camera_view.after(ViewerCameraViewSet::Apply),
        );
        #[cfg(not(feature = "phase0b-rehearsal"))]
        app.add_plugins(ViewerCameraInputPlugin);
        #[cfg(feature = "phase0b-rehearsal")]
        crate::phase0b_rehearsal::install(&mut app);
        install_panic_status();
        app.run();
        // Bevy's WASM runner returns after installing its browser event loop;
        // return from `run` is not a viewer-stop signal. Launch, panic, source,
        // protocol, and bounded capture failures publish their own status.
    }

    fn publish_diagnostics(
        runtime: Res<'_, ViewerRuntime>,
        mut observation: ResMut<'_, BrowserObservation>,
    ) {
        observation.diagnostics_published = publish_diagnostics_to_dom(&runtime);
        if !observation.diagnostics_published {
            set_status(
                StatusKind::Blocked,
                "Viewer blocked — the Diagnostics surface is unavailable",
            );
        }
    }

    fn publish_diagnostics_to_dom(runtime: &ViewerRuntime) -> bool {
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return false;
        };
        let presentations = runtime
            .sources()
            .iter()
            .map(|source| {
                (
                    source.slot(),
                    DiagnosticsPresentation::capture(source.inspection()),
                )
            })
            .collect::<Vec<_>>();
        let Some(diagnostics) = document.get_element_by_id(DIAGNOSTICS_ELEMENT_ID) else {
            return false;
        };
        let Some(summary) = document.get_element_by_id(DIAGNOSTICS_SUMMARY_ELEMENT_ID) else {
            return false;
        };
        let aggregate_tone = presentations
            .iter()
            .map(|(_slot, presentation)| presentation.tone())
            .max()
            .unwrap_or(DiagnosticsTone::Compatible);
        summary.set_text_content(Some(&disclosure_summary(
            presentations
                .iter()
                .map(|(_slot, presentation)| presentation),
        )));
        if diagnostics
            .set_attribute(
                "data-tone",
                match aggregate_tone {
                    DiagnosticsTone::Compatible => "compatible",
                    DiagnosticsTone::Warning => "warning",
                    DiagnosticsTone::Degraded => "degraded",
                },
            )
            .is_err()
        {
            return false;
        }
        if aggregate_tone != DiagnosticsTone::Compatible
            && diagnostics.set_attribute("open", "").is_err()
        {
            return false;
        }

        let has_comparison = runtime.has_comparison();
        for slot in [SourceSlot::Primary, SourceSlot::Comparison] {
            let prefix = match slot {
                SourceSlot::Primary => "spinal-primary-diagnostics",
                SourceSlot::Comparison => "spinal-comparison-diagnostics",
            };
            let Some(section) = document.get_element_by_id(prefix) else {
                return false;
            };
            let Some((_slot, presentation)) = presentations
                .iter()
                .find(|(candidate, _presentation)| *candidate == slot)
            else {
                if section.set_attribute("hidden", "").is_err() {
                    return false;
                }
                continue;
            };
            if section.remove_attribute("hidden").is_err() {
                return false;
            }
            let values = [
                (
                    format!("{prefix}-heading"),
                    source_slot_label(slot, has_comparison).to_owned(),
                ),
                (
                    format!("{prefix}-compatibility"),
                    presentation.compatibility().to_owned(),
                ),
                (
                    format!("{prefix}-inventory"),
                    presentation.inventory().to_owned(),
                ),
                (format!("{prefix}-bundle"), presentation.bundle().to_owned()),
            ];
            for (id, value) in values {
                let Some(element) = document.get_element_by_id(&id) else {
                    return false;
                };
                element.set_text_content(Some(&value));
            }
            let Some(findings) = document.get_element_by_id(&format!("{prefix}-findings")) else {
                return false;
            };
            findings.set_text_content(None);
            if presentation.findings().is_empty() {
                if !append_diagnostic_item(
                    &document,
                    &findings,
                    "No source compatibility findings.",
                ) {
                    return false;
                }
            } else {
                for finding in presentation.findings() {
                    if !append_diagnostic_item(&document, &findings, finding) {
                        return false;
                    }
                }
                if presentation.omitted_finding_count() > 0
                    && !append_diagnostic_item(
                        &document,
                        &findings,
                        &format!(
                            "{} more findings omitted; run spinal check for the expanded inspection report.",
                            presentation.omitted_finding_count()
                        ),
                    )
                {
                    return false;
                }
            }
        }
        true
    }

    fn append_diagnostic_item(
        document: &web_sys::Document,
        list: &web_sys::Element,
        text: &str,
    ) -> bool {
        let Ok(item) = document.create_element("li") else {
            return false;
        };
        item.set_text_content(Some(text));
        list.append_child(&item).is_ok()
    }

    #[derive(Resource)]
    struct BrowserLabel(Box<str>);

    #[derive(Resource)]
    struct BrowserCommandQueueResource(Arc<Mutex<BrowserCommandQueue>>);

    #[derive(Default, Resource)]
    struct BrowserTransportNotice {
        overflowed: bool,
    }

    struct BrowserCommandListener {
        window: web_sys::Window,
        callback: Closure<dyn FnMut(MessageEvent)>,
    }

    impl Drop for BrowserCommandListener {
        fn drop(&mut self) {
            let _ignored = self.window.remove_event_listener_with_callback(
                "message",
                self.callback.as_ref().unchecked_ref(),
            );
        }
    }

    fn install_command_bridge(app: &mut App) -> Result<(), BrowserError> {
        let window = web_sys::window().ok_or_else(|| BrowserError::new("window is unavailable"))?;
        let document = window
            .document()
            .ok_or_else(|| BrowserError::new("document is unavailable"))?;
        let app_element = document
            .get_element_by_id(APP_ELEMENT_ID)
            .ok_or_else(|| BrowserError::new("the viewer application element is unavailable"))?;
        let page_origin = window
            .location()
            .origin()
            .map_err(|_| BrowserError::new("the page origin is unavailable"))?;
        let capability = generate_launch_capability(&window)?;
        let mut protocol = BrowserCommandProtocol::new(capability.clone())
            .map_err(|_| BrowserError::new("could not initialize browser command authorization"))?;
        let queue = Arc::new(Mutex::new(BrowserCommandQueue::default()));
        let callback_queue = Arc::clone(&queue);
        let callback_window = window.clone();
        let event_loop_proxy = app
            .world()
            .get_resource::<EventLoopProxyWrapper>()
            .map(std::ops::Deref::deref)
            .cloned()
            .ok_or_else(|| BrowserError::new("the browser event loop is unavailable"))?;
        let callback = Closure::wrap(Box::new(move |event: MessageEvent| {
            let self_source = event.source().is_some_and(|source| {
                let source: &JsValue = source.as_ref();
                let own_window: &JsValue = callback_window.as_ref();
                source == own_window
            });
            let Some(encoded) = event.data().as_string() else {
                return;
            };
            let event_origin = event.origin();
            let context = BrowserMessageContext {
                page_origin: &page_origin,
                event_origin: &event_origin,
                self_source,
            };
            let Ok(command) = protocol.authorize(&encoded, context) else {
                return;
            };
            let Ok(mut queue) = callback_queue.lock() else {
                set_status(
                    StatusKind::Blocked,
                    "Viewer blocked — browser controls became unavailable",
                );
                return;
            };
            let _accepted_or_recorded_overflow = queue.try_push(command);
            drop(queue);
            if event_loop_proxy.send_event(WinitUserEvent::WakeUp).is_err() {
                set_status(
                    StatusKind::Blocked,
                    "Viewer blocked — browser controls could not wake the viewer",
                );
            }
        }) as Box<dyn FnMut(MessageEvent)>);
        window
            .add_event_listener_with_callback("message", callback.as_ref().unchecked_ref())
            .map_err(|_| BrowserError::new("could not install the browser command listener"))?;
        if app_element
            .set_attribute(CAPABILITY_ATTRIBUTE, &capability)
            .is_err()
        {
            let _ignored = window
                .remove_event_listener_with_callback("message", callback.as_ref().unchecked_ref());
            return Err(BrowserError::new(
                "could not publish browser command authorization",
            ));
        }
        app.insert_resource(BrowserCommandQueueResource(queue));
        app.insert_non_send(BrowserCommandListener { window, callback });
        Ok(())
    }

    fn generate_launch_capability(window: &web_sys::Window) -> Result<String, BrowserError> {
        let crypto = window
            .crypto()
            .map_err(|_| BrowserError::new("secure browser randomness is unavailable"))?;
        let mut bytes = [0_u8; CAPABILITY_BYTES];
        crypto
            .get_random_values_with_u8_array(&mut bytes)
            .map_err(|_| BrowserError::new("secure browser randomness failed"))?;
        let mut capability = String::with_capacity(CAPABILITY_BYTES * 2);
        for byte in bytes {
            write!(&mut capability, "{byte:02x}")
                .map_err(|_| BrowserError::new("could not encode browser authorization"))?;
        }
        Ok(capability)
    }

    fn drain_browser_commands(
        queue: Res<'_, BrowserCommandQueueResource>,
        mut inbox: ResMut<'_, CommandInbox>,
        mut notice: ResMut<'_, BrowserTransportNotice>,
    ) {
        let batch = {
            let Ok(mut queue) = queue.0.lock() else {
                set_status(
                    StatusKind::Blocked,
                    "Viewer blocked — browser controls became unavailable",
                );
                return;
            };
            queue.drain()
        };
        let disposition = external_command_batch_for_shared_inbox(batch);
        #[cfg(feature = "phase0b-rehearsal")]
        if disposition.rejected {
            crate::phase0b_rehearsal::publish_external_error(
                "external_command",
                "an external browser command reached the isolated rehearsal transport",
            );
        }
        let batch = disposition.batch;
        for command in batch.commands {
            inbox.push(command);
        }
        notice.overflowed |= batch.overflowed;
    }

    fn publish_transport_notice(
        runtime: Res<'_, ViewerRuntime>,
        observation: Res<'_, BrowserObservation>,
        mut notice: ResMut<'_, BrowserTransportNotice>,
    ) {
        if !notice.overflowed {
            return;
        }
        let snapshot = runtime.snapshot();
        let settled = observation.published.as_ref() == Some(&snapshot);
        let presentation = aggregate_presentations(
            snapshot
                .sources()
                .iter()
                .map(|source| classify_runtime(source.load_state(), source.runtime_state())),
        );
        let drawable = matches!(
            presentation,
            RuntimePresentation::Ready | RuntimePresentation::Warning
        );
        if settled && drawable && snapshot.controls_ready() {
            notice.overflowed = false;
            set_status(
                StatusKind::Warning,
                "Viewer controls received too many commands; newest commands were ignored.",
            );
        }
    }

    fn publish_camera_view(
        runtime: Res<'_, ViewerRuntime>,
        view: Res<'_, CameraViewState>,
        cameras: Query<'_, '_, (&Transform, &Projection), With<PreviewCamera>>,
        instances: Query<'_, '_, &Transform, With<SpinalInstance>>,
    ) {
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return;
        };
        let summary = view.summary(runtime.has_comparison());
        if let Some(output) = document.get_element_by_id(CAMERA_STATE_ELEMENT_ID)
            && output.text_content().as_deref() != Some(summary.as_str())
        {
            output.set_text_content(Some(&summary));
        }
        let Some(app) = document.get_element_by_id(APP_ELEMENT_ID) else {
            return;
        };
        let synchronized = cameras.iter().count() == runtime.sources().len()
            && cameras.iter().all(|(transform, projection)| {
                let center = transform.translation.truncate();
                let scale_matches = match projection {
                    Projection::Orthographic(orthographic) => {
                        orthographic.scale == view.projection_scale()
                    }
                    _other => false,
                };
                center == view.center() && scale_matches
            });
        set_attribute_if_changed(
            &app,
            "data-spinal-camera-synchronized",
            if synchronized { "true" } else { "false" },
        );
        set_attribute_if_changed(
            &app,
            "data-spinal-camera-zoom",
            &view.zoom_percent().to_string(),
        );
        set_attribute_if_changed(
            &app,
            "data-spinal-camera-panned",
            if view.is_panned() { "true" } else { "false" },
        );
        set_attribute_if_changed(
            &app,
            "data-spinal-camera-revision",
            &view.revision().to_string(),
        );
        let fitted = runtime
            .sources()
            .iter()
            .filter_map(|source| instances.get(source.entity()).ok())
            .collect::<Vec<_>>();
        let shared_fit = fitted.len() == runtime.sources().len()
            && fitted.windows(2).all(|pair| pair[0] == pair[1]);
        set_attribute_if_changed(
            &app,
            "data-spinal-base-fit-synchronized",
            if shared_fit { "true" } else { "false" },
        );
        if let Some(transform) = fitted.first() {
            set_attribute_if_changed(
                &app,
                "data-spinal-base-fit-scale",
                &transform.scale.x.to_string(),
            );
            set_attribute_if_changed(
                &app,
                "data-spinal-base-fit-center",
                &format!("{},{}", transform.translation.x, transform.translation.y),
            );
        }
    }

    fn set_attribute_if_changed(element: &web_sys::Element, name: &str, value: &str) {
        if element.get_attribute(name).as_deref() != Some(value) {
            let _ignored = element.set_attribute(name, value);
        }
    }

    #[derive(Default, Resource)]
    struct BrowserObservation {
        diagnostics_published: bool,
        published: Option<runtime::RuntimeSnapshot>,
        pending: Option<runtime::RuntimeSnapshot>,
        stable_ready_updates: u8,
        published_catalog_revision: Option<u64>,
    }

    fn sync_status(
        runtime: Res<'_, ViewerRuntime>,
        label: Res<'_, BrowserLabel>,
        mut observation: ResMut<'_, BrowserObservation>,
    ) {
        if !observation.diagnostics_published {
            set_status(
                StatusKind::Blocked,
                "Viewer blocked — the Diagnostics surface is unavailable",
            );
            return;
        }
        let snapshot = runtime.snapshot();
        sync_source_labels(&snapshot);
        let refresh_catalog = runtime.catalog_revision() != 0
            && observation.published_catalog_revision != Some(runtime.catalog_revision());
        let controls_synced = sync_transport_controls(
            transport_presentation(
                snapshot.controls_ready(),
                snapshot.selected_animation().is_some(),
                snapshot.is_paused(),
            ),
            &runtime,
            refresh_catalog,
        );
        if !controls_synced {
            set_status(
                StatusKind::Blocked,
                "Viewer blocked — browser controls are unavailable",
            );
            return;
        }
        if refresh_catalog {
            observation.published_catalog_revision = Some(runtime.catalog_revision());
        }
        if observation.published.as_ref() == Some(&snapshot) {
            return;
        }
        if observation.pending.as_ref() != Some(&snapshot) {
            observation.pending = Some(snapshot.clone());
            observation.stable_ready_updates = 0;
        }
        let Some(primary) = snapshot.source(SourceSlot::Primary) else {
            set_status(
                StatusKind::Blocked,
                "Viewer blocked — primary source is missing",
            );
            observation.published = Some(snapshot);
            observation.pending = None;
            return;
        };
        let presentation = aggregate_presentations(
            snapshot
                .sources()
                .iter()
                .map(|source| classify_runtime(source.load_state(), source.runtime_state())),
        );
        let presentation_source = snapshot
            .sources()
            .iter()
            .find(|source| {
                classify_runtime(source.load_state(), source.runtime_state()) == presentation
            })
            .unwrap_or(primary);
        let source_label =
            source_slot_label(presentation_source.slot(), snapshot.sources().len() > 1);
        let animation = snapshot.selected_animation().map_or_else(
            || "setup pose".to_owned(),
            |name| format!("animation {name}"),
        );
        let playback = if snapshot.is_paused() {
            "Playback is paused"
        } else {
            "Playback is running"
        };
        let selection_note = [
            missing_selection_summary(
                snapshot.selected_animation(),
                snapshot.sources().iter().map(|source| {
                    (
                        source_slot_label(source.slot(), snapshot.sources().len() > 1),
                        source.selected_present(),
                    )
                }),
            ),
            missing_skin_summary(
                snapshot.selected_skin(),
                snapshot.sources().iter().map(|source| {
                    (
                        source_slot_label(source.slot(), snapshot.sources().len() > 1),
                        source.selected_skin_present(),
                    )
                }),
            ),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
        let selection_note = if selection_note.is_empty() {
            String::new()
        } else {
            format!(" {selection_note}")
        };
        let (kind, message) = match presentation {
            RuntimePresentation::Loading => (
                StatusKind::Loading,
                format!(
                    "Preparing {source_label} — runtime state: {}",
                    presentation_source.runtime_state()
                ),
            ),
            RuntimePresentation::BlockedLoad => {
                let ViewerLoadState::Failed(error) = presentation_source.load_state() else {
                    unreachable!("classification preserves the load state")
                };
                (
                    StatusKind::Blocked,
                    format!("Viewer blocked — {source_label} bundle load failed: {error}"),
                )
            }
            RuntimePresentation::BlockedRuntime => (
                StatusKind::Blocked,
                format!(
                    "Viewer blocked — {source_label} runtime failed: {}",
                    presentation_source
                        .latest_issue()
                        .unwrap_or("no diagnostic was reported")
                ),
            ),
            RuntimePresentation::BlockedNoDraws => (
                StatusKind::Blocked,
                format!(
                    "Viewer blocked — {source_label} runtime state {} produced no drawable output",
                    presentation_source.runtime_state()
                ),
            ),
            RuntimePresentation::Warning => (
                StatusKind::Warning,
                format!(
                    "Ready with warnings — {}. {animation} selected. {playback}. {}{selection_note}",
                    label.0,
                    presentation_source
                        .latest_issue()
                        .unwrap_or("A runtime fallback is active.")
                ),
            ),
            RuntimePresentation::Ready => (
                StatusKind::Ready,
                format!(
                    "Ready — {}. {animation} selected. {playback}.{selection_note}",
                    label.0
                ),
            ),
        };

        if matches!(
            presentation,
            RuntimePresentation::Ready | RuntimePresentation::Warning
        ) {
            if !canvas_has_nonzero_size() {
                set_status(StatusKind::Loading, "Preparing viewer — canvas has no size");
                return;
            }
            observation.stable_ready_updates = observation.stable_ready_updates.saturating_add(1);
            if observation.stable_ready_updates < 2 {
                set_status(
                    StatusKind::Loading,
                    "Preparing viewer — finalizing runtime state",
                );
                return;
            }
        }
        set_status(kind, &message);
        observation.published = Some(snapshot);
        observation.pending = None;
    }

    fn sync_source_labels(snapshot: &runtime::RuntimeSnapshot) {
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return;
        };
        let has_comparison = snapshot.sources().len() > 1;
        for (slot, element_id) in [
            (SourceSlot::Primary, PRIMARY_LABEL_ELEMENT_ID),
            (SourceSlot::Comparison, COMPARISON_LABEL_ELEMENT_ID),
        ] {
            let Some(source) = snapshot.source(slot) else {
                continue;
            };
            let title = source_slot_label(slot, has_comparison);
            let mut fallbacks = Vec::with_capacity(2);
            if snapshot.selected_animation().is_some() && !source.selected_present() {
                fallbacks.push("setup pose");
            }
            if snapshot.selected_skin().name().is_some() && !source.selected_skin_present() {
                fallbacks.push("Default skin");
            }
            let text = if fallbacks.is_empty() {
                title.to_owned()
            } else {
                format!("{title} — {}", fallbacks.join("; "))
            };
            if let Some(element) = document.get_element_by_id(element_id)
                && element.text_content().as_deref() != Some(text.as_str())
            {
                element.set_text_content(Some(&text));
            }
        }
    }

    fn sync_transport_controls(
        presentation: super::TransportPresentation,
        runtime: &ViewerRuntime,
        refresh_catalog: bool,
    ) -> bool {
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return false;
        };
        for id in [
            PLAY_TOGGLE_ELEMENT_ID,
            STEP_BACKWARD_ELEMENT_ID,
            STEP_FORWARD_ELEMENT_ID,
            RESTART_ELEMENT_ID,
        ] {
            set_element_enabled(&document, id, presentation.animation_commands_enabled);
        }
        set_element_enabled(&document, REFIT_ELEMENT_ID, presentation.fit_enabled);
        set_element_enabled(&document, ZOOM_IN_ELEMENT_ID, presentation.fit_enabled);
        set_element_enabled(&document, ZOOM_OUT_ELEMENT_ID, presentation.fit_enabled);
        set_element_enabled(
            &document,
            SKIN_SELECT_ELEMENT_ID,
            presentation.skin_commands_enabled,
        );
        for id in [
            ANIMATION_SELECT_ELEMENT_ID,
            LOOPING_ELEMENT_ID,
            SPEED_ELEMENT_ID,
        ] {
            set_element_enabled(&document, id, presentation.animation_commands_enabled);
        }
        let timeline_enabled = presentation.animation_commands_enabled
            && runtime
                .selected_entry()
                .is_some_and(|(_index, _name, duration)| !duration.is_zero());
        set_element_enabled(&document, TIMELINE_ELEMENT_ID, timeline_enabled);
        if let Some(play_toggle) = document.get_element_by_id(PLAY_TOGGLE_ELEMENT_ID) {
            play_toggle.set_text_content(Some(presentation.playback_label));
            let _ignored = play_toggle.set_attribute("aria-label", presentation.playback_label);
        }
        let animation_catalog_synced = sync_animation_select(&document, runtime, refresh_catalog);
        let skin_catalog_synced = sync_skin_select(&document, runtime, refresh_catalog);
        if let Some(looping) = document
            .get_element_by_id(LOOPING_ELEMENT_ID)
            .and_then(|element| element.dyn_into::<HtmlInputElement>().ok())
        {
            looping.set_checked(runtime.model().transport().is_looping());
        }
        if let Some(speed) = document
            .get_element_by_id(SPEED_ELEMENT_ID)
            .and_then(|element| element.dyn_into::<HtmlSelectElement>().ok())
        {
            speed.set_value(
                &runtime
                    .model()
                    .transport()
                    .playback_speed()
                    .multiplier()
                    .to_string(),
            );
        }
        let (position, duration) = runtime.selected_entry().map_or(
            (std::time::Duration::ZERO, std::time::Duration::ZERO),
            |_entry| (runtime.model().transport().position(), _entry.2),
        );
        if let Some(timeline) = document
            .get_element_by_id(TIMELINE_ELEMENT_ID)
            .and_then(|element| element.dyn_into::<HtmlInputElement>().ok())
        {
            timeline.set_max(&duration_milliseconds(duration).to_string());
            timeline.set_value(&duration_milliseconds(position).to_string());
            let _ignored =
                timeline.set_attribute("aria-valuetext", &timeline_value_text(position, duration));
        }
        if let Some(value) = document.get_element_by_id(TIMELINE_VALUE_ELEMENT_ID) {
            value.set_text_content(Some(&format!(
                "{:.3} / {:.3} s",
                position.as_secs_f64(),
                duration.as_secs_f64()
            )));
        }
        animation_catalog_synced && skin_catalog_synced
    }

    fn sync_animation_select(
        document: &web_sys::Document,
        runtime: &ViewerRuntime,
        refresh_catalog: bool,
    ) -> bool {
        let Some(select) = document
            .get_element_by_id(ANIMATION_SELECT_ELEMENT_ID)
            .and_then(|element| element.dyn_into::<HtmlSelectElement>().ok())
        else {
            return false;
        };
        if refresh_catalog {
            select.set_length(0);
            if runtime.model().animations().is_empty() {
                if !add_select_option(document, &select, "", "No animations") {
                    return false;
                }
            } else {
                for name in runtime.model().animations() {
                    if !add_select_option(document, &select, name, name) {
                        return false;
                    }
                }
            }
        }
        select.set_value(runtime.selected_name().unwrap_or(""));
        true
    }

    fn sync_skin_select(
        document: &web_sys::Document,
        runtime: &ViewerRuntime,
        refresh_catalog: bool,
    ) -> bool {
        let Some(select) = document
            .get_element_by_id(SKIN_SELECT_ELEMENT_ID)
            .and_then(|element| element.dyn_into::<HtmlSelectElement>().ok())
        else {
            return false;
        };
        if refresh_catalog {
            select.set_length(0);
            if !add_select_option(document, &select, "", "Default") {
                return false;
            }
            for name in runtime.model().skins() {
                if !add_select_option(document, &select, name, name) {
                    return false;
                }
            }
        }
        select.set_value(runtime.model().selected_skin().name().unwrap_or(""));
        true
    }

    fn add_select_option(
        document: &web_sys::Document,
        select: &HtmlSelectElement,
        value: &str,
        label: &str,
    ) -> bool {
        let Ok(element) = document.create_element("option") else {
            return false;
        };
        let Ok(option) = element.dyn_into::<HtmlOptionElement>() else {
            return false;
        };
        option.set_value(value);
        option.set_text(label);
        select.add_with_html_option_element(&option).is_ok()
    }

    fn duration_milliseconds(duration: std::time::Duration) -> u64 {
        u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
    }

    fn set_element_enabled(document: &web_sys::Document, id: &str, enabled: bool) {
        let Some(element) = document.get_element_by_id(id) else {
            return;
        };
        let graphics_blocked = document
            .get_element_by_id(APP_ELEMENT_ID)
            .and_then(|app| app.get_attribute(GRAPHICS_BLOCKED_ATTRIBUTE))
            .as_deref()
            == Some("true");
        if enabled && !graphics_blocked {
            let _ignored = element.remove_attribute("disabled");
        } else {
            let _ignored = element.set_attribute("disabled", "");
        }
    }

    async fn fetch_bytes(
        url: &str,
        max_bytes: usize,
        expected_bytes: Option<usize>,
        launch_deadline_ms: f64,
    ) -> Result<Vec<u8>, BrowserError> {
        if max_bytes == 0 {
            return Err(BrowserError::new("browser download budget is exhausted"));
        }
        let display_url = redact_url(url);
        let init = RequestInit::new();
        init.set_method("GET");
        init.set_mode(RequestMode::SameOrigin);
        init.set_credentials(RequestCredentials::Omit);
        init.set_cache(RequestCache::NoStore);
        init.set_redirect(RequestRedirect::Error);
        let controller = AbortController::new()
            .map_err(|_| BrowserError::new("could not create a browser request timeout"))?;
        init.set_signal(Some(&controller.signal()));
        let request = Request::new_with_str_and_init(url, &init).map_err(|_| {
            BrowserError::new(format!("could not create request for `{display_url}`"))
        })?;
        let window = web_sys::window().ok_or_else(|| BrowserError::new("window is unavailable"))?;
        let request_timeout_ms = bounded_request_timeout(launch_deadline_ms - Date::now())
            .ok_or_else(|| {
                BrowserError::new(format!(
                    "browser launch timed out after {LAUNCH_TIMEOUT_MS} ms"
                ))
            })?;
        let timed_out = Rc::new(Cell::new(false));
        let timeout_flag = Rc::clone(&timed_out);
        let abort = controller.clone();
        let timeout_callback = Closure::wrap(Box::new(move || {
            timeout_flag.set(true);
            abort.abort();
        }) as Box<dyn FnMut()>);
        let timeout_id = window
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                timeout_callback.as_ref().unchecked_ref(),
                request_timeout_ms,
            )
            .map_err(|_| BrowserError::new("could not schedule a browser request timeout"))?;

        let result = fetch_stream(
            &window,
            &request,
            url,
            max_bytes,
            expected_bytes,
            &timed_out,
            request_timeout_ms,
        )
        .await;
        if result.is_err() {
            controller.abort();
        }
        window.clear_timeout_with_handle(timeout_id);
        drop(timeout_callback);
        result
    }

    async fn fetch_stream(
        window: &web_sys::Window,
        request: &Request,
        requested_url: &str,
        max_bytes: usize,
        expected_bytes: Option<usize>,
        timed_out: &Cell<bool>,
        request_timeout_ms: i32,
    ) -> Result<Vec<u8>, BrowserError> {
        let display_url = redact_url(requested_url);
        let response_value = JsFuture::from(window.fetch_with_request(request))
            .await
            .map_err(|_| {
                if timed_out.get() {
                    BrowserError::new(format!(
                        "request for `{display_url}` timed out after {request_timeout_ms} ms"
                    ))
                } else {
                    BrowserError::new(format!("request failed for `{display_url}`"))
                }
            })?;
        let response = response_value
            .dyn_into::<Response>()
            .map_err(|_| BrowserError::new(format!("invalid response for `{display_url}`")))?;
        if response.redirected() || response.url() != requested_url {
            return Err(BrowserError::new(format!(
                "redirected response is not allowed for `{display_url}`"
            )));
        }
        if !response.ok() {
            return Err(BrowserError::new(format!(
                "request for `{display_url}` returned HTTP {}",
                response.status()
            )));
        }
        if let Some(length) = response
            .headers()
            .get("content-length")
            .map_err(|_| BrowserError::new("invalid Content-Length header"))?
        {
            let length = length
                .parse::<u64>()
                .map_err(|_| BrowserError::new("invalid Content-Length header"))?;
            if length > max_bytes as u64 {
                return Err(BrowserError::new(format!(
                    "response for `{display_url}` exceeds its {max_bytes}-byte limit"
                )));
            }
        }
        let stream = response.body().ok_or_else(|| {
            BrowserError::new(format!("response for `{display_url}` has no readable body"))
        })?;
        let reader = ReadableStreamDefaultReader::new(&stream).map_err(|_| {
            BrowserError::new(format!("could not read response for `{display_url}`"))
        })?;
        let mut bytes = Vec::with_capacity(expected_bytes.unwrap_or(0).min(max_bytes));
        loop {
            let chunk_result = JsFuture::from(reader.read()).await.map_err(|_| {
                if timed_out.get() {
                    BrowserError::new(format!(
                        "request for `{display_url}` timed out after {request_timeout_ms} ms"
                    ))
                } else {
                    BrowserError::new(format!("could not read response for `{display_url}`"))
                }
            })?;
            let done = Reflect::get(&chunk_result, &JsValue::from_str("done"))
                .ok()
                .and_then(|value| value.as_bool())
                .ok_or_else(|| BrowserError::new("browser stream returned an invalid result"))?;
            if done {
                break;
            }
            let chunk_value = Reflect::get(&chunk_result, &JsValue::from_str("value"))
                .map_err(|_| BrowserError::new("browser stream returned an invalid chunk"))?;
            if chunk_value.is_null() || chunk_value.is_undefined() {
                let _cancel = reader.cancel();
                return Err(BrowserError::new("browser stream returned an empty chunk"));
            }
            let chunk = Uint8Array::new(&chunk_value);
            let chunk_len = usize::try_from(chunk.length())
                .map_err(|_| BrowserError::new("response length does not fit this browser"))?;
            let new_len = bytes
                .len()
                .checked_add(chunk_len)
                .ok_or_else(|| BrowserError::new("response length overflowed"))?;
            if new_len > max_bytes || expected_bytes.is_some_and(|expected| new_len > expected) {
                let _cancel = reader.cancel();
                return Err(BrowserError::new(format!(
                    "response for `{display_url}` exceeds its allowed byte length"
                )));
            }
            let start = bytes.len();
            bytes.resize(new_len, 0);
            chunk.copy_to(&mut bytes[start..]);
        }
        reader.release_lock();
        if expected_bytes.is_some_and(|expected| bytes.len() != expected) {
            return Err(BrowserError::new(format!(
                "response for `{display_url}` has {} bytes; expected {}",
                bytes.len(),
                expected_bytes.expect("checked above")
            )));
        }
        Ok(bytes)
    }

    fn resolve_same_origin(
        reference: &str,
        base: &str,
        page_url: &str,
    ) -> Result<String, BrowserError> {
        let candidate = Url::new_with_base(reference, base)
            .map_err(|_| BrowserError::new("invalid bundle URL"))?;
        let page = Url::new(page_url).map_err(|_| BrowserError::new("invalid page URL"))?;
        if candidate.origin() != page.origin() {
            return Err(BrowserError::new("cross-origin bundle URL is not allowed"));
        }
        if !candidate.username().is_empty()
            || !candidate.password().is_empty()
            || !candidate.hash().is_empty()
        {
            return Err(BrowserError::new(
                "credentials and fragments are not allowed in bundle URLs",
            ));
        }
        if !matches!(candidate.protocol().as_str(), "http:" | "https:") {
            return Err(BrowserError::new("bundle URLs must use HTTP or HTTPS"));
        }
        Ok(candidate.href())
    }

    fn resolve_bundle_file(
        reference: &str,
        manifest_url: &str,
        page_url: &str,
    ) -> Result<String, BrowserError> {
        let resolved = resolve_same_origin(reference, manifest_url, page_url)?;
        let manifest = Url::new(manifest_url)
            .map_err(|_| BrowserError::new("invalid browser manifest URL"))?;
        let candidate =
            Url::new(&resolved).map_err(|_| BrowserError::new("invalid resolved bundle URL"))?;
        let manifest_path = manifest.pathname();
        let directory_end = manifest_path
            .rfind('/')
            .map_or(0, |position| position.saturating_add(1));
        let manifest_directory = &manifest_path[..directory_end];
        if !candidate.pathname().starts_with(manifest_directory) || !candidate.search().is_empty() {
            return Err(BrowserError::new(
                "bundle file URLs must stay inside the manifest directory",
            ));
        }
        Ok(resolved)
    }

    fn redact_url(value: &str) -> String {
        Url::new(value).map_or_else(
            |_error| "<invalid URL>".to_owned(),
            |url| format!("{}{}", url.origin(), url.pathname()),
        )
    }

    #[derive(Clone, Copy)]
    enum StatusKind {
        Loading,
        Ready,
        Warning,
        Blocked,
    }

    impl StatusKind {
        const fn attribute(self) -> &'static str {
            match self {
                Self::Loading => "loading",
                Self::Ready => "ready",
                Self::Warning => "warning",
                Self::Blocked => "blocked",
            }
        }
    }

    fn set_status(kind: StatusKind, message: &str) {
        let Ok(callback) = Reflect::get(
            &js_sys::global(),
            &JsValue::from_str("spinalSetShellStatus"),
        ) else {
            return;
        };
        let Some(callback) = callback.dyn_ref::<Function>() else {
            return;
        };
        let _ignored = callback.call2(
            &JsValue::NULL,
            &JsValue::from_str(kind.attribute()),
            &JsValue::from_str(message),
        );
    }

    fn canvas_has_nonzero_size() -> bool {
        let Some(element) = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.get_element_by_id(CANVAS_ELEMENT_ID))
        else {
            return false;
        };
        let Some(canvas) = element.dyn_ref::<HtmlCanvasElement>() else {
            return false;
        };
        canvas.width() > 0
            && canvas.height() > 0
            && element.client_width() > 0
            && element.client_height() > 0
    }

    fn signal_runtime_started() {
        let Ok(callback) = Reflect::get(
            &js_sys::global(),
            &JsValue::from_str("spinalRuntimeStarted"),
        ) else {
            return;
        };
        let Some(callback) = callback.dyn_ref::<Function>() else {
            return;
        };
        let _ = callback.call0(&JsValue::NULL);
    }

    fn install_panic_status() {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic| {
            #[cfg(feature = "phase0b-rehearsal")]
            crate::phase0b_rehearsal::publish_external_error(
                "viewer_panic",
                "the viewer panicked before observation completed",
            );
            set_status(
                StatusKind::Blocked,
                "Viewer blocked — the viewer stopped unexpectedly",
            );
            previous(panic);
        }));
    }

    #[derive(Clone, Debug)]
    struct BrowserError(Box<str>);

    impl BrowserError {
        fn new(message: impl Into<Box<str>>) -> Self {
            Self(message.into())
        }
    }

    impl fmt::Display for BrowserError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(&self.0)
        }
    }

    impl From<BrowserManifestError> for BrowserError {
        fn from(error: BrowserManifestError) -> Self {
            Self::new(error.to_string())
        }
    }

    impl From<BrowserReviewManifestError> for BrowserError {
        fn from(error: BrowserReviewManifestError) -> Self {
            Self::new(error.to_string())
        }
    }
}

/// Starts the browser host. Non-WASM builds report an explicit unsupported host.
pub fn run() {
    #[cfg(target_arch = "wasm32")]
    browser::run();

    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("spinal web host requires wasm32-unknown-unknown");
}

#[cfg(test)]
mod tests {
    use super::*;

    const BROWSER_SHELL_HTML: &str = include_str!("../web/index.html");

    #[test]
    fn runtime_status_mapping_never_false_greens_failed_or_empty_output() {
        assert_eq!(
            classify_runtime(&ViewerLoadState::Loading, &SpinalInstanceState::Loading),
            RuntimePresentation::Loading
        );
        assert_eq!(
            classify_runtime(
                &ViewerLoadState::Failed("bad atlas".into()),
                &SpinalInstanceState::Failed,
            ),
            RuntimePresentation::BlockedLoad
        );
        assert_eq!(
            classify_runtime(&ViewerLoadState::Ready, &SpinalInstanceState::Failed),
            RuntimePresentation::BlockedRuntime
        );
        for state in [
            SpinalInstanceState::ReadyNoDraws,
            SpinalInstanceState::DegradedNoDraws,
        ] {
            assert_eq!(
                classify_runtime(&ViewerLoadState::Ready, &state),
                RuntimePresentation::BlockedNoDraws
            );
        }
        assert_eq!(
            classify_runtime(&ViewerLoadState::Ready, &SpinalInstanceState::Degraded),
            RuntimePresentation::Warning
        );
        assert_eq!(
            classify_runtime(&ViewerLoadState::Ready, &SpinalInstanceState::Ready),
            RuntimePresentation::Ready
        );
    }

    #[test]
    fn comparison_status_never_false_greens_a_nonready_or_blocked_source() {
        assert_eq!(
            aggregate_presentations([RuntimePresentation::Ready, RuntimePresentation::Loading]),
            RuntimePresentation::Loading
        );
        assert_eq!(
            aggregate_presentations([RuntimePresentation::Ready, RuntimePresentation::Warning]),
            RuntimePresentation::Warning
        );
        for blocked in [
            RuntimePresentation::BlockedLoad,
            RuntimePresentation::BlockedRuntime,
            RuntimePresentation::BlockedNoDraws,
        ] {
            assert_eq!(
                aggregate_presentations([RuntimePresentation::Ready, blocked]),
                blocked
            );
        }
        assert_eq!(
            aggregate_presentations([]),
            RuntimePresentation::BlockedRuntime
        );
    }

    #[test]
    fn launch_deadline_bounds_every_request_and_cannot_be_extended() {
        assert_eq!(bounded_request_timeout(f64::NAN), None);
        assert_eq!(bounded_request_timeout(0.0), None);
        assert_eq!(bounded_request_timeout(-1.0), None);
        assert_eq!(bounded_request_timeout(0.1), Some(1));
        assert_eq!(bounded_request_timeout(500.1), Some(501));
        assert_eq!(
            bounded_request_timeout(f64::from(LAUNCH_TIMEOUT_MS)),
            Some(FETCH_TIMEOUT_MS)
        );
    }

    #[test]
    fn one_sided_animation_is_explained_instead_of_false_green() {
        assert_eq!(
            missing_selection_summary(Some("jump"), [("Current", false), ("Proposed", true)])
                .as_deref(),
            Some("Current does not contain animation “jump”; showing setup pose in that pane.")
        );
        assert_eq!(
            missing_selection_summary(Some("jump"), [("Current", true), ("Proposed", true)]),
            None
        );
        assert_eq!(missing_selection_summary(None, [("Current", false)]), None);
    }

    #[test]
    fn one_sided_skin_is_explained_instead_of_false_green() {
        assert_eq!(
            missing_skin_summary(
                &SkinSelection::Named("winter-coat".into()),
                [("Current", false), ("Proposed", true)],
            )
            .as_deref(),
            Some("Current does not contain skin “winter-coat”; showing Default skin in that pane.")
        );
        assert_eq!(
            missing_skin_summary(
                &SkinSelection::Named("winter-coat".into()),
                [("Current", true), ("Proposed", true)],
            ),
            None
        );
        assert_eq!(
            missing_skin_summary(&SkinSelection::Default, [("Current", false)]),
            None
        );
    }

    #[test]
    fn transport_state_is_snapshot_driven_and_playback_label_is_dynamic() {
        assert_eq!(
            transport_presentation(false, true, true),
            TransportPresentation {
                animation_commands_enabled: false,
                skin_commands_enabled: false,
                fit_enabled: false,
                playback_label: "Play",
            }
        );
        assert_eq!(
            transport_presentation(true, false, true),
            TransportPresentation {
                animation_commands_enabled: false,
                skin_commands_enabled: true,
                fit_enabled: true,
                playback_label: "Play",
            }
        );
        assert_eq!(
            transport_presentation(true, true, false),
            TransportPresentation {
                animation_commands_enabled: true,
                skin_commands_enabled: true,
                fit_enabled: true,
                playback_label: "Pause",
            }
        );
    }

    #[test]
    fn canvas_names_are_stable_context_and_timeline_values_use_seconds() {
        assert_eq!(contextual_canvas_label(false), "Spinal preview viewport.");
        assert_eq!(
            contextual_canvas_label(true),
            "Spinal comparison viewport. Current is left; Proposed is right."
        );
        assert_eq!(
            timeline_value_text(
                std::time::Duration::from_millis(500),
                std::time::Duration::from_millis(1_250)
            ),
            "0.500 of 1.250 seconds"
        );
    }

    #[test]
    fn external_commands_follow_the_compile_time_rehearsal_boundary() {
        let disposition = external_command_batch_for_shared_inbox(BrowserCommandBatch {
            commands: vec![ViewerCommand::Restart, ViewerCommand::Refit],
            overflowed: true,
        });

        #[cfg(feature = "phase0b-rehearsal")]
        {
            assert!(disposition.rejected);
            assert!(disposition.batch.commands.is_empty());
            assert!(!disposition.batch.overflowed);
        }
        #[cfg(not(feature = "phase0b-rehearsal"))]
        {
            assert_eq!(
                disposition.batch.commands,
                vec![ViewerCommand::Restart, ViewerCommand::Refit]
            );
            assert!(disposition.batch.overflowed);
        }
    }

    #[cfg(feature = "phase0b-rehearsal")]
    #[test]
    fn empty_external_batch_does_not_report_rejection() {
        let disposition = external_command_batch_for_shared_inbox(BrowserCommandBatch {
            commands: Vec::new(),
            overflowed: false,
        });
        assert!(!disposition.rejected);
        assert!(disposition.batch.commands.is_empty());
    }

    #[test]
    fn browser_shell_exposes_only_the_fixed_semantic_transport() {
        let canvas_position = BROWSER_SHELL_HTML
            .find("id=\"spinal-canvas\"")
            .expect("canvas exists");
        let transport_position = BROWSER_SHELL_HTML
            .find("id=\"spinal-transport\"")
            .expect("transport exists");
        assert!(
            canvas_position < transport_position,
            "controls follow the canvas"
        );

        let expected_controls = [
            ("spinal-restart", "restart"),
            ("spinal-step-backward", "step-backward"),
            ("spinal-play-toggle", "toggle-pause"),
            ("spinal-step-forward", "step-forward"),
            ("spinal-zoom-out", "zoom-out"),
            ("spinal-refit", "refit"),
            ("spinal-zoom-in", "zoom-in"),
        ];
        assert_eq!(
            BROWSER_SHELL_HTML.matches("data-spinal-action=\"").count(),
            expected_controls.len()
        );
        for (id, action) in expected_controls {
            let start = BROWSER_SHELL_HTML
                .find(&format!("id=\"{id}\""))
                .expect("control id exists");
            let end = BROWSER_SHELL_HTML[start..]
                .find("</button>")
                .map(|offset| start + offset)
                .expect("semantic button closes");
            let button = &BROWSER_SHELL_HTML[start..end];
            assert!(button.contains("type=\"button\""));
            assert!(button.contains(&format!("data-spinal-action=\"{action}\"")));
            assert!(button.contains("aria-controls=\"spinal-canvas\""));
            assert!(button.contains("disabled"));
        }

        assert!(BROWSER_SHELL_HTML.contains("type: \"spinal.viewer.command\""));
        assert!(BROWSER_SHELL_HTML.contains("version: 1"));
        assert!(BROWSER_SHELL_HTML.contains("window.location.origin"));
        assert!(BROWSER_SHELL_HTML.contains("data-spinal-command-capability"));
        assert!(
            BROWSER_SHELL_HTML
                .contains("<label class=\"control-field\" for=\"spinal-skin-select\">")
        );
        assert!(BROWSER_SHELL_HTML.contains(
            "<select id=\"spinal-skin-select\" aria-controls=\"spinal-canvas\" disabled>"
        ));
        assert!(BROWSER_SHELL_HTML.contains("postCommand(\"select-skin\", { selection })"));
        assert!(BROWSER_SHELL_HTML.contains("{ kind: \"default\" }"));
        assert!(BROWSER_SHELL_HTML.contains("{ kind: \"named\", name: skin.value }"));
        assert!(BROWSER_SHELL_HTML.contains("canvas?.addEventListener(\"keydown\""));
        assert!(BROWSER_SHELL_HTML.contains("ArrowLeft: \"pan-left\""));
        assert!(
            BROWSER_SHELL_HTML
                .contains("if (event.ctrlKey || event.metaKey || event.altKey) return")
        );
        assert!(BROWSER_SHELL_HTML.contains("event.preventDefault()"));
    }

    #[test]
    fn browser_shell_defaults_to_preview_and_has_a_distinct_compare_identity() {
        for expected in [
            "<title>Spinal — Preview</title>",
            "data-spinal-mode=\"preview\"",
            "class=\"identity-mode-preview\">Animation preview",
            "class=\"identity-mode-compare\">Animation comparison",
            "#spinal-app[data-spinal-mode=\"compare\"] .identity-mode-compare",
            "aria-label=\"Spinal preview viewport.\"",
            "aria-label=\"Animation controls\"",
            "Loading preview…",
        ] {
            assert!(
                BROWSER_SHELL_HTML.contains(expected),
                "missing `{expected}`"
            );
        }
        for stale in [
            "<title>Spinal — Review</title>",
            ">Animation review<",
            "aria-label=\"Review controls\"",
            "aria-label=\"Spinal review viewport.\"",
            "Loading review…",
        ] {
            assert!(
                !BROWSER_SHELL_HTML.contains(stale),
                "stale workflow copy `{stale}`"
            );
        }
    }

    #[test]
    fn browser_shell_preserves_focus_reflow_reduced_motion_and_quiet_controls() {
        assert!(BROWSER_SHELL_HTML.contains("flex-wrap: wrap"));
        assert!(BROWSER_SHELL_HTML.contains(".transport button:focus-visible"));
        assert!(BROWSER_SHELL_HTML.contains("canvas:focus-visible"));
        assert!(BROWSER_SHELL_HTML.contains("tabindex=\"0\""));
        assert!(BROWSER_SHELL_HTML.contains("role=\"group\" aria-label=\"Camera controls\""));
        assert!(BROWSER_SHELL_HTML.contains("id=\"spinal-camera-help\""));
        assert!(BROWSER_SHELL_HTML.contains("id=\"spinal-camera-state\""));
        assert!(BROWSER_SHELL_HTML.contains("@media (max-width: 48rem)"));
        assert!(BROWSER_SHELL_HTML.contains("@media (max-width: 30rem)"));
        assert!(BROWSER_SHELL_HTML.contains("grid-template-columns: minmax(0, 1fr)"));
        assert!(BROWSER_SHELL_HTML.contains("min-width: 0"));
        assert!(BROWSER_SHELL_HTML.contains("overflow-wrap: anywhere"));
        assert!(BROWSER_SHELL_HTML.contains(".camera-controls {"));
        assert!(BROWSER_SHELL_HTML.contains("flex-wrap: wrap"));
        assert!(BROWSER_SHELL_HTML.contains("flex: 1 1 100%"));
        assert!(BROWSER_SHELL_HTML.contains("@media (prefers-reduced-motion: reduce)"));
        assert_eq!(BROWSER_SHELL_HTML.matches("aria-live=").count(), 1);
        assert!(BROWSER_SHELL_HTML.contains(
            "id=\"spinal-status\"\n          role=\"status\"\n          aria-live=\"polite\""
        ));
        assert!(BROWSER_SHELL_HTML.contains("data-spinal-graphics-blocked"));
        assert!(BROWSER_SHELL_HTML.contains("effectiveKind = graphicsBlocked ? \"blocked\""));
        assert!(BROWSER_SHELL_HTML.contains("control.disabled = true"));
        assert!(BROWSER_SHELL_HTML.contains("if (status.textContent !== effectiveMessage)"));
        assert!(BROWSER_SHELL_HTML.contains("if (status.dataset.state !== effectiveKind)"));
        assert_eq!(
            BROWSER_SHELL_HTML
                .matches("canvas.setAttribute(\"aria-label\"")
                .count(),
            0,
            "status updates must not rewrite the stable canvas name"
        );
        assert!(BROWSER_SHELL_HTML.contains("aria-label=\"Spinal preview viewport.\""));
        assert!(BROWSER_SHELL_HTML.contains("aria-valuetext=\"0.000 of 0.000 seconds\""));
        assert!(
            BROWSER_SHELL_HTML.contains("timeline.setAttribute(\n              \"aria-valuetext\"")
        );
    }

    #[test]
    fn browser_shell_has_one_contextual_semantic_diagnostics_surface() {
        let transport = BROWSER_SHELL_HTML
            .find("id=\"spinal-transport\"")
            .expect("transport exists");
        let diagnostics = BROWSER_SHELL_HTML
            .find("id=\"spinal-diagnostics\"")
            .expect("diagnostics exists");
        assert!(transport < diagnostics, "Diagnostics follows the controls");
        assert_eq!(
            BROWSER_SHELL_HTML
                .matches("<details id=\"spinal-diagnostics\"")
                .count(),
            1
        );
        assert!(BROWSER_SHELL_HTML.contains(
            "id=\"spinal-diagnostics-sources\"\n            class=\"diagnostics-sources\"\n            role=\"group\"\n            aria-label=\"Source diagnostics\""
        ));
        for id in [
            "spinal-primary-diagnostics",
            "spinal-primary-diagnostics-heading",
            "spinal-primary-diagnostics-compatibility",
            "spinal-primary-diagnostics-inventory",
            "spinal-primary-diagnostics-bundle",
            "spinal-primary-diagnostics-findings",
            "spinal-comparison-diagnostics",
            "spinal-comparison-diagnostics-heading",
            "spinal-comparison-diagnostics-compatibility",
            "spinal-comparison-diagnostics-inventory",
            "spinal-comparison-diagnostics-bundle",
            "spinal-comparison-diagnostics-findings",
        ] {
            assert!(BROWSER_SHELL_HTML.contains(&format!("id=\"{id}\"")));
        }
        let diagnostics_markup = &BROWSER_SHELL_HTML[diagnostics..];
        assert!(!diagnostics_markup.contains("data-spinal-action="));
        assert!(!diagnostics_markup.contains("aria-live="));
        assert!(
            BROWSER_SHELL_HTML
                .contains("aria-describedby=\"spinal-status spinal-camera-help spinal-camera-state spinal-diagnostics-summary\"")
        );
        assert!(BROWSER_SHELL_HTML.contains("grid-template-rows: minmax(18rem, 1fr) auto auto"));
    }
}

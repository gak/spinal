//! Thin browser host for the same immutable bundle and runtime used natively.

#[cfg(any(target_arch = "wasm32", test))]
use crate::runtime::ViewerLoadState;
#[cfg(any(target_arch = "wasm32", test))]
use bevy_spinal::SpinalInstanceState;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg(any(target_arch = "wasm32", test))]
struct TransportPresentation {
    animation_commands_enabled: bool,
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
        fit_enabled: controls_ready,
        playback_label: if paused { "Play" } else { "Pause" },
    }
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

    use bevy::{asset::AssetPlugin, camera::visibility::RenderLayers, prelude::*};
    use js_sys::{Function, Reflect, Uint8Array};
    use wasm_bindgen::{JsCast, JsValue, closure::Closure};
    use wasm_bindgen_futures::{JsFuture, spawn_local};
    use web_sys::{
        AbortController, HtmlCanvasElement, HtmlInputElement, HtmlOptionElement, HtmlSelectElement,
        MessageEvent, ReadableStreamDefaultReader, Request, RequestCache, RequestCredentials,
        RequestInit, RequestMode, RequestRedirect, Response, Url,
    };

    use super::{RuntimePresentation, classify_runtime, transport_presentation};
    use crate::{
        camera_fit::{PreviewCamera, ViewerCameraFitPlugin},
        preview::PreviewRate,
        runtime::{
            self, CommandInbox, ViewerLoadState, ViewerRuntime, ViewerRuntimePlugin,
            ViewerRuntimeSet, source_render_layer,
        },
        session::SourceSlot,
        web_command::{BrowserCommandProtocol, BrowserCommandQueue, BrowserMessageContext},
        web_manifest::{
            BrowserManifest, BrowserManifestError, MAX_BROWSER_BUNDLE_BYTES, MAX_MANIFEST_BYTES,
        },
    };

    const APP_ELEMENT_ID: &str = "spinal-app";
    const CANVAS_ELEMENT_ID: &str = "spinal-canvas";
    const STATUS_ELEMENT_ID: &str = "spinal-status";
    const MANIFEST_ATTRIBUTE: &str = "data-spinal-manifest";
    const CAPABILITY_ATTRIBUTE: &str = "data-spinal-command-capability";
    const PLAY_TOGGLE_ELEMENT_ID: &str = "spinal-play-toggle";
    const STEP_BACKWARD_ELEMENT_ID: &str = "spinal-step-backward";
    const STEP_FORWARD_ELEMENT_ID: &str = "spinal-step-forward";
    const RESTART_ELEMENT_ID: &str = "spinal-restart";
    const REFIT_ELEMENT_ID: &str = "spinal-refit";
    const ANIMATION_SELECT_ELEMENT_ID: &str = "spinal-animation-select";
    const LOOPING_ELEMENT_ID: &str = "spinal-looping";
    const SPEED_ELEMENT_ID: &str = "spinal-speed";
    const TIMELINE_ELEMENT_ID: &str = "spinal-timeline";
    const TIMELINE_VALUE_ELEMENT_ID: &str = "spinal-timeline-value";
    const CAPABILITY_BYTES: usize = 32;
    const FETCH_TIMEOUT_MS: i32 = 30_000;

    /// Starts the asynchronous same-origin bundle loader and one Bevy app.
    pub(super) fn run() {
        signal_runtime_started();
        install_panic_status();
        set_status(StatusKind::Loading, "Loading preview…");
        spawn_local(async {
            match load_browser_launch().await {
                Ok(launch) => run_app(launch),
                Err(error) => {
                    set_status(StatusKind::Blocked, &format!("Preview blocked — {error}"));
                    web_sys::console::error_1(&JsValue::from_str(&error.to_string()));
                }
            }
        });
    }

    struct BrowserLaunch {
        label: Box<str>,
        config: runtime::LaunchConfig,
    }

    async fn load_browser_launch() -> Result<BrowserLaunch, BrowserError> {
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
        let manifest_url = resolve_same_origin(&manifest_reference, &page_url, &page_url)?;
        let manifest_bytes = fetch_bytes(&manifest_url, MAX_MANIFEST_BYTES, None).await?;
        let manifest = BrowserManifest::parse(&manifest_bytes)?;
        let label: Box<str> = manifest.label().into();

        let mut downloaded = BTreeMap::<PathBuf, Vec<u8>>::new();
        let mut resolved_urls = BTreeSet::new();
        let mut total_bytes = 0_usize;
        for file in manifest.files() {
            let url = resolve_bundle_file(file.location_reference(), &manifest_url, &page_url)?;
            if !resolved_urls.insert(url.clone()) {
                return Err(BrowserError::new(format!(
                    "two bundle files resolve to the same URL `{}`",
                    redact_url(&url)
                )));
            }
            let remaining = MAX_BROWSER_BUNDLE_BYTES.saturating_sub(total_bytes);
            let effective_limit = file.max_bytes().min(remaining);
            if file.expected_bytes() > effective_limit {
                return Err(BrowserError::new(format!(
                    "bundle file `{}` exceeds the remaining {remaining}-byte bundle budget",
                    file.virtual_path().display()
                )));
            }
            let bytes = fetch_bytes(&url, effective_limit, Some(file.expected_bytes())).await?;
            total_bytes = total_bytes
                .checked_add(bytes.len())
                .ok_or_else(|| BrowserError::new("browser bundle size overflowed"))?;
            if total_bytes > MAX_BROWSER_BUNDLE_BYTES {
                return Err(BrowserError::new(format!(
                    "browser bundle exceeds the {MAX_BROWSER_BUNDLE_BYTES}-byte total limit"
                )));
            }
            if downloaded
                .insert(file.virtual_path().to_path_buf(), bytes)
                .is_some()
            {
                return Err(BrowserError::new("duplicate downloaded virtual path"));
            }
        }
        let bundle = manifest.into_bundle(downloaded)?;
        document.set_title(&format!("{label} — Spinal"));
        Ok(BrowserLaunch {
            label,
            config: runtime::LaunchConfig::single(bundle, PreviewRate::default()),
        })
    }

    fn run_app(launch: BrowserLaunch) {
        let mut app = App::new();
        if let Err(error) = install_command_bridge(&mut app) {
            set_status(
                StatusKind::Blocked,
                &format!("Preview blocked — browser controls could not start: {error}"),
            );
            web_sys::console::error_1(&JsValue::from_str(&error.to_string()));
            return;
        }
        runtime::prepare_runtime(&mut app, launch.config);
        app.insert_resource(ClearColor(Color::srgb(0.025, 0.030, 0.041)))
            .insert_resource(BrowserLabel(launch.label))
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
                            title: "Spinal animation preview".into(),
                            canvas: Some(format!("#{CANVAS_ELEMENT_ID}")),
                            fit_canvas_to_parent: true,
                            prevent_default_event_handling: false,
                            ..default()
                        }),
                        ..default()
                    }),
            )
            .add_plugins((ViewerRuntimePlugin, ViewerCameraFitPlugin::default()))
            .add_systems(Startup, setup_canvas.after(ViewerRuntimeSet::Setup))
            .add_systems(
                Update,
                drain_browser_commands.before(ViewerRuntimeSet::Commands),
            )
            .add_systems(
                Update,
                (sync_status, publish_transport_notice)
                    .chain()
                    .after(ViewerRuntimeSet::Observe),
            );
        install_panic_status();
        app.run();
        set_status(
            StatusKind::Blocked,
            "Preview blocked — the viewer stopped unexpectedly",
        );
    }

    fn setup_canvas(mut commands: Commands<'_, '_>) {
        commands.spawn((
            Camera2d,
            RenderLayers::layer(source_render_layer(SourceSlot::Primary)),
            PreviewCamera(SourceSlot::Primary),
        ));
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
                    "Preview blocked — browser controls became unavailable",
                );
                return;
            };
            let _accepted_or_recorded_overflow = queue.try_push(command);
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
        app.insert_non_send_resource(BrowserCommandListener { window, callback });
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
                    "Preview blocked — browser controls became unavailable",
                );
                return;
            };
            queue.drain()
        };
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
        let drawable = snapshot.source(SourceSlot::Primary).is_some_and(|primary| {
            matches!(
                classify_runtime(primary.load_state(), primary.runtime_state()),
                RuntimePresentation::Ready | RuntimePresentation::Warning
            )
        });
        if settled && drawable && snapshot.controls_ready() {
            notice.overflowed = false;
            set_status(
                StatusKind::Warning,
                "Preview controls received too many commands; newest commands were ignored.",
            );
        }
    }

    #[derive(Default, Resource)]
    struct BrowserObservation {
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
        let snapshot = runtime.snapshot();
        let refresh_catalog = runtime.catalog_revision() != 0
            && observation.published_catalog_revision != Some(runtime.catalog_revision());
        if sync_transport_controls(
            transport_presentation(
                snapshot.controls_ready(),
                snapshot.selected_animation().is_some(),
                snapshot.is_paused(),
            ),
            &runtime,
            refresh_catalog,
        ) && refresh_catalog
        {
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
                "Preview blocked — primary source is missing",
            );
            observation.published = Some(snapshot);
            observation.pending = None;
            return;
        };
        let animation = snapshot.selected_animation().map_or_else(
            || "setup pose".to_owned(),
            |name| format!("animation {name}"),
        );
        let playback = if snapshot.is_paused() {
            "Playback is paused"
        } else {
            "Playback is running"
        };
        let presentation = classify_runtime(primary.load_state(), primary.runtime_state());
        let (kind, message) = match presentation {
            RuntimePresentation::Loading => (
                StatusKind::Loading,
                format!(
                    "Preparing preview — runtime state: {}",
                    primary.runtime_state()
                ),
            ),
            RuntimePresentation::BlockedLoad => {
                let ViewerLoadState::Failed(error) = primary.load_state() else {
                    unreachable!("classification preserves the load state")
                };
                (
                    StatusKind::Blocked,
                    format!("Preview blocked — bundle load failed: {error}"),
                )
            }
            RuntimePresentation::BlockedRuntime => (
                StatusKind::Blocked,
                format!(
                    "Preview blocked — runtime failed: {}",
                    snapshot
                        .latest_issue()
                        .unwrap_or("no diagnostic was reported")
                ),
            ),
            RuntimePresentation::BlockedNoDraws => (
                StatusKind::Blocked,
                format!(
                    "Preview blocked — runtime state {} produced no drawable output",
                    primary.runtime_state()
                ),
            ),
            RuntimePresentation::Warning => (
                StatusKind::Warning,
                format!(
                    "Ready with warnings — {}. {animation} selected. {playback}. {}",
                    label.0,
                    snapshot
                        .latest_issue()
                        .unwrap_or("A runtime fallback is active.")
                ),
            ),
            RuntimePresentation::Ready => (
                StatusKind::Ready,
                format!("Ready — {}. {animation} selected. {playback}.", label.0),
            ),
        };

        if matches!(
            presentation,
            RuntimePresentation::Ready | RuntimePresentation::Warning
        ) {
            if !canvas_has_nonzero_size() {
                set_status(
                    StatusKind::Loading,
                    "Preparing preview — canvas has no size",
                );
                return;
            }
            observation.stable_ready_updates = observation.stable_ready_updates.saturating_add(1);
            if observation.stable_ready_updates < 2 {
                set_status(
                    StatusKind::Loading,
                    "Preparing preview — finalizing runtime state",
                );
                return;
            }
        }
        set_status(kind, &message);
        observation.published = Some(snapshot);
        observation.pending = None;
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
        for id in [
            ANIMATION_SELECT_ELEMENT_ID,
            LOOPING_ELEMENT_ID,
            SPEED_ELEMENT_ID,
            TIMELINE_ELEMENT_ID,
        ] {
            set_element_enabled(&document, id, presentation.animation_commands_enabled);
        }
        if let Some(play_toggle) = document.get_element_by_id(PLAY_TOGGLE_ELEMENT_ID) {
            play_toggle.set_text_content(Some(presentation.playback_label));
            let _ignored = play_toggle.set_attribute("aria-label", presentation.playback_label);
        }
        let catalog_synced = sync_animation_select(&document, runtime, refresh_catalog);
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
        }
        if let Some(value) = document.get_element_by_id(TIMELINE_VALUE_ELEMENT_ID) {
            value.set_text_content(Some(&format!(
                "{:.3} / {:.3} s",
                position.as_secs_f64(),
                duration.as_secs_f64()
            )));
        }
        catalog_synced
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
                if !add_animation_option(document, &select, "", "No animations") {
                    return false;
                }
            } else {
                for name in runtime.model().animations() {
                    if !add_animation_option(document, &select, name, name) {
                        return false;
                    }
                }
            }
        }
        select.set_value(runtime.selected_name().unwrap_or(""));
        true
    }

    fn add_animation_option(
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
        if enabled {
            let _ignored = element.remove_attribute("disabled");
        } else {
            let _ignored = element.set_attribute("disabled", "");
        }
    }

    async fn fetch_bytes(
        url: &str,
        max_bytes: usize,
        expected_bytes: Option<usize>,
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
                FETCH_TIMEOUT_MS,
            )
            .map_err(|_| BrowserError::new("could not schedule a browser request timeout"))?;

        let result = fetch_stream(
            &window,
            &request,
            url,
            &display_url,
            max_bytes,
            expected_bytes,
            &timed_out,
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
        display_url: &str,
        max_bytes: usize,
        expected_bytes: Option<usize>,
        timed_out: &Cell<bool>,
    ) -> Result<Vec<u8>, BrowserError> {
        let response_value = JsFuture::from(window.fetch_with_request(request))
            .await
            .map_err(|_| {
                if timed_out.get() {
                    BrowserError::new(format!(
                        "request for `{display_url}` timed out after {FETCH_TIMEOUT_MS} ms"
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
                        "request for `{display_url}` timed out after {FETCH_TIMEOUT_MS} ms"
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
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return;
        };
        if let Some(status) = document.get_element_by_id(STATUS_ELEMENT_ID) {
            if status.text_content().as_deref() != Some(message) {
                status.set_text_content(Some(message));
            }
            let _ = status.set_attribute("data-state", kind.attribute());
        }
        if let Some(app) = document.get_element_by_id(APP_ELEMENT_ID) {
            let busy = if matches!(kind, StatusKind::Loading) {
                "true"
            } else {
                "false"
            };
            let _ = app.set_attribute("aria-busy", busy);
        }
        if let Some(canvas) = document.get_element_by_id(CANVAS_ELEMENT_ID) {
            let _ = canvas.set_attribute("role", "img");
            let _ = canvas.remove_attribute("tabindex");
            let _ = canvas.set_attribute("aria-label", &format!("Spinal preview. {message}"));
        }
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
            set_status(
                StatusKind::Blocked,
                "Preview blocked — the viewer stopped unexpectedly",
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
}

/// Starts the browser host. Non-WASM builds report an explicit unsupported host.
pub fn run() {
    #[cfg(target_arch = "wasm32")]
    browser::run();

    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("spinal viewer web host requires wasm32-unknown-unknown");
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
    fn transport_state_is_snapshot_driven_and_playback_label_is_dynamic() {
        assert_eq!(
            transport_presentation(false, true, true),
            TransportPresentation {
                animation_commands_enabled: false,
                fit_enabled: false,
                playback_label: "Play",
            }
        );
        assert_eq!(
            transport_presentation(true, false, true),
            TransportPresentation {
                animation_commands_enabled: false,
                fit_enabled: true,
                playback_label: "Play",
            }
        );
        assert_eq!(
            transport_presentation(true, true, false),
            TransportPresentation {
                animation_commands_enabled: true,
                fit_enabled: true,
                playback_label: "Pause",
            }
        );
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
            ("spinal-refit", "refit"),
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
        assert!(!BROWSER_SHELL_HTML.contains("keydown"));
    }

    #[test]
    fn browser_shell_preserves_focus_reflow_reduced_motion_and_quiet_controls() {
        assert!(BROWSER_SHELL_HTML.contains("flex-wrap: wrap"));
        assert!(BROWSER_SHELL_HTML.contains(".transport button:focus-visible"));
        assert!(BROWSER_SHELL_HTML.contains("@media (max-width: 48rem)"));
        assert!(BROWSER_SHELL_HTML.contains("@media (prefers-reduced-motion: reduce)"));
        assert_eq!(BROWSER_SHELL_HTML.matches("aria-live=").count(), 1);
        assert!(BROWSER_SHELL_HTML.contains(
            "id=\"spinal-status\"\n          role=\"status\"\n          aria-live=\"polite\""
        ));
    }
}

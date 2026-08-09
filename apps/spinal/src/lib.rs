//! Shared read-only Spinal viewer application for native and web hosts.

#[cfg(feature = "native")]
mod app;
#[cfg_attr(
    all(feature = "web", not(feature = "native")),
    allow(dead_code, reason = "shared model is wired into the browser host next")
)]
mod bundle;
#[cfg_attr(
    all(feature = "web", not(feature = "native"), not(target_arch = "wasm32")),
    allow(
        dead_code,
        reason = "browser camera fitting is only instantiated on wasm32"
    )
)]
mod camera_fit;
#[cfg_attr(
    all(feature = "web", not(feature = "native"), not(target_arch = "wasm32")),
    allow(
        dead_code,
        reason = "browser camera interaction is only instantiated on wasm32"
    )
)]
#[cfg_attr(
    all(target_arch = "wasm32", feature = "phase0b-rehearsal"),
    allow(
        dead_code,
        reason = "the fixed observation harness intentionally disables pointer and touch input"
    )
)]
mod camera_view;
#[cfg(feature = "native")]
mod check;
#[cfg_attr(
    all(feature = "web", not(feature = "native")),
    allow(dead_code, reason = "shared model is wired into the browser host next")
)]
mod clock;
#[cfg_attr(
    all(feature = "web", not(feature = "native")),
    allow(dead_code, reason = "shared model is wired into the browser host next")
)]
mod command;
#[cfg(any(feature = "native", feature = "web"))]
mod diagnostics;
#[cfg(any(feature = "native", feature = "web"))]
#[cfg_attr(
    all(feature = "web", not(feature = "native")),
    allow(
        dead_code,
        reason = "browser inspection is rendered only by the wasm32 host"
    )
)]
mod inspection;
#[cfg(any(feature = "native", feature = "web"))]
#[cfg_attr(
    all(feature = "web", not(feature = "native"), not(target_arch = "wasm32")),
    allow(
        dead_code,
        reason = "browser viewports are instantiated only on wasm32"
    )
)]
mod layout;
#[cfg(feature = "native")]
mod native;
#[cfg(all(feature = "phase0b-rehearsal", any(target_arch = "wasm32", test)))]
mod phase0b_rehearsal;
#[cfg_attr(
    all(feature = "web", not(feature = "native")),
    allow(dead_code, reason = "shared model is wired into the browser host next")
)]
mod preview;
#[cfg_attr(
    all(feature = "web", not(feature = "native")),
    allow(
        dead_code,
        reason = "shared runtime is wired into the browser host next"
    )
)]
mod runtime;
#[cfg_attr(
    all(feature = "web", not(feature = "native")),
    allow(dead_code, reason = "shared model is wired into the browser host next")
)]
mod session;
#[cfg(feature = "native")]
mod source;
#[cfg(feature = "native")]
mod ui;
#[cfg(any(feature = "native", feature = "web"))]
#[cfg_attr(
    all(feature = "web", not(feature = "native"), not(target_arch = "wasm32")),
    allow(dead_code, reason = "browser cameras are instantiated only on wasm32")
)]
mod viewport;
#[cfg(feature = "web")]
mod web;
#[cfg(all(feature = "web", any(target_arch = "wasm32", test)))]
mod web_command;
#[cfg(all(feature = "web", any(target_arch = "wasm32", test)))]
mod web_manifest;

/// Runs Preview, Compare, or the read-only headless checker.
#[cfg(feature = "native")]
pub use native::run;

/// Starts the thin browser host around the shared viewer runtime.
#[cfg(feature = "web")]
pub use web::run as run_web;

#[cfg(all(test, feature = "native"))]
mod tests {
    use crate::source;

    #[test]
    fn application_remains_read_only_by_construction() {
        let help = source::HELP;
        assert!(help.contains("SKELETON.json"));
        assert!(!help.contains("save"));
        assert!(!help.contains("write"));
    }
}

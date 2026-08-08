//! Shared read-only Spinal viewer application for native and web hosts.

#[cfg(feature = "native")]
mod app;
#[cfg_attr(
    all(feature = "web", not(feature = "native")),
    allow(dead_code, reason = "shared model is wired into the browser host next")
)]
mod bundle;
mod camera_fit;
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
#[cfg(feature = "native")]
mod layout;
#[cfg(feature = "native")]
mod native;
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
#[cfg(feature = "web")]
mod web;
#[cfg(all(feature = "web", any(target_arch = "wasm32", test)))]
mod web_manifest;

/// Parses viewer arguments, prepares the selected export, and runs the app.
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

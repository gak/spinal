//! Shared read-only Spinal viewer application for native and web hosts.

#[cfg(feature = "native")]
mod app;
#[cfg_attr(
    all(feature = "web", not(feature = "native")),
    allow(dead_code, reason = "shared model is wired into the browser host next")
)]
mod bundle;
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
    allow(dead_code, reason = "shared model is wired into the browser host next")
)]
mod session;
#[cfg(feature = "native")]
mod source;
#[cfg(feature = "native")]
mod ui;
#[cfg(feature = "web")]
mod web;

/// Parses viewer arguments, prepares the selected export, and runs the app.
#[cfg(feature = "native")]
pub use native::run;

/// Runs the thin browser canvas host.
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

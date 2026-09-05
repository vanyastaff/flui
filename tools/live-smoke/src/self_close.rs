//! The platform's harness self-close hook, spelled once for both modes.
//!
//! `flui-platform`'s winit backend arms a self-close deadline from
//! `FLUI_SELF_CLOSE_AFTER_MS` and takes one of two routes to close the
//! window when it fires (`FLUI_SELF_CLOSE_ROUTE`): the synthesized
//! compositor `CloseRequested` arm, or `PlatformWindow::close` — the route
//! an application closing its own window takes, which on winit hid the
//! window but never left the backend's tracking map, so the process never
//! exited (issue #919) while the compositor-route checks stayed green.
//!
//! Exit status alone cannot tell the two routes apart — a misspelled route
//! name falls back to the compositor route and still exits 0 — so every
//! self-close check also asserts the route MARKER the backend logs when the
//! deadline fires: a structured `route` field, matched on the field rather
//! than the human-readable message.

use std::path::Path;

use anyhow::{Context, Result, bail};

/// The app closes itself this long after its loop starts — long enough for
/// the window and a few presented frames to exist, so the teardown runs
/// against a live swapchain (the ordering the #713 checks pin).
pub(crate) const SELF_CLOSE_AFTER_MS: u64 = 2000;
/// The platform's deadline knob.
pub(crate) const DEADLINE_ENV: &str = "FLUI_SELF_CLOSE_AFTER_MS";
/// The platform's route knob.
pub(crate) const ROUTE_ENV: &str = "FLUI_SELF_CLOSE_ROUTE";

/// Which route the app's self-close takes — the value of [`ROUTE_ENV`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseRoute {
    /// The synthesized compositor `CloseRequested` arm.
    Compositor,
    /// `PlatformWindow::close` on the tracked window.
    Programmatic,
}

impl CloseRoute {
    /// The [`ROUTE_ENV`] value that selects this route.
    pub(crate) fn env_value(self) -> &'static str {
        match self {
            Self::Compositor => "compositor",
            Self::Programmatic => "programmatic",
        }
    }

    /// The structured field the backend logs when its deadline fires on
    /// this route (`tracing`'s fmt layer renders a `&str` field as
    /// `name="value"`).
    fn log_marker(self) -> String {
        format!("route=\"{}\"", self.env_value())
    }
}

/// Asserts the app log shows the deadline fired on `route` — the oracle
/// that makes a self-close check route-discriminating. The log is captured
/// with ANSI colour on, so it is stripped before matching.
pub(crate) fn assert_route_observed(log_path: &Path, route: CloseRoute) -> Result<()> {
    let log = std::fs::read_to_string(log_path)
        .with_context(|| format!("reading the app log {}", log_path.display()))?;
    let marker = route.log_marker();
    if strip_ansi(&log).contains(&marker) {
        return Ok(());
    }
    bail!(
        "self-close check FAILED: the app exited but its log never shows the deadline firing \
         on the {} route (no `{marker}` line) — the platform took a different route than the \
         one this check exists to exercise",
        route.env_value()
    )
}

/// Strips ANSI CSI escape sequences (the colour codes `tracing`'s fmt layer
/// emits on a tty-shaped stream) so log oracles match on content.
pub(crate) fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c2 in chars.by_ref() {
                    if ('@'..='~').contains(&c2) {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

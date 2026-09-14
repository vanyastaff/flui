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

/// The `flui.gpu` trace's structured marker for a released GPU surface
/// (`SurfaceLease::drop`, `crates/flui-engine/src/wgpu/surface_lease.rs`,
/// issue #1043's cycle-breaker) — matched on the field, not the
/// human-readable message, the same discipline `harness.rs`'s own
/// `GPU_PRESENT_MARKER`/`GPU_PRE_PRESENT_MARKER` use. Requires `flui.gpu`
/// enabled at `debug` or louder in `RUST_LOG` — the level `SurfaceLease::drop`
/// logs at.
const SURFACE_RELEASED_MARKER: &str = "event=\"surface_released\"";

/// The winit backend's own "the loop is exiting" line
/// (`WinitApp::request_exit`) — logged only after the closing window's
/// callbacks have already been cleared and the exit-policy hook has
/// confirmed no window remains, which makes it the closest thing this
/// single-window harness has to an observable "the window has now fully
/// closed" instant. Matched on the literal message: it carries no
/// structured field of its own.
const EVENT_LOOP_QUITTING_LINE: &str = "Quitting event loop";

/// Asserts the app log shows its GPU surface released strictly before the
/// event loop reports itself quitting, on whichever close route produced
/// this log.
///
/// This is the only executed evidence that `SurfaceLease`'s target (the
/// renderer's own `Arc` clone of the window, the same shape
/// `install_pre_present_hook` captures in production) was actually
/// released while the window and event loop were still alive, rather than
/// orphaned by the frame-closure cycle (window -> callback slot -> frame
/// closure -> `Arc<window>`) that only a callback-clearing call breaks —
/// see the memory note `pre-present-hook-pins-the-window-in-a-cycle`.
/// Before this check existed, `just live-smoke-wayland` asserted only the
/// exit code, which stays 0 whether the surface was released in order,
/// released late, or never released at all (the process's own exit
/// reclaims the leak either way).
pub(crate) fn assert_surface_released_before_window_close(log_path: &Path) -> Result<()> {
    let log = std::fs::read_to_string(log_path)
        .with_context(|| format!("reading the app log {}", log_path.display()))?;
    let log = strip_ansi(&log);
    let lines: Vec<&str> = log.lines().collect();

    let surface_line = lines
        .iter()
        .position(|line| line.contains(SURFACE_RELEASED_MARKER));
    let close_line = lines
        .iter()
        .position(|line| line.contains(EVENT_LOOP_QUITTING_LINE));

    match (surface_line, close_line) {
        (Some(surface_line), Some(close_line)) if surface_line < close_line => Ok(()),
        (None, _) => bail!(
            "surface-release check FAILED: no '{SURFACE_RELEASED_MARKER}' log line at all -- \
             either the flui.gpu trace target is not reaching the log (check RUST_LOG), or \
             SurfaceLease::drop never ran -- the frame-closure cycle (window -> callback slot \
             -> closure -> Arc<window>) was not broken"
        ),
        (_, None) => bail!(
            "surface-release check FAILED: the app exited 0 but its log never shows \
             '{EVENT_LOOP_QUITTING_LINE}' -- cannot order the surface release against it"
        ),
        (Some(surface_line), Some(close_line)) => bail!(
            "surface-release check FAILED: the surface release line comes AFTER the loop's own \
             quitting line -- the renderer's Arc<window> survived past the point the loop \
             believed every window was gone, the orphaned-by-the-callback-cycle shape issue \
             #1043 closes.\n  surface release (log line {surface_line}): {}\n  loop quitting \
             (log line {close_line}): {}",
            lines[surface_line],
            lines[close_line],
        ),
    }
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

//! Live-smoke E2E harness — drives a REAL windowed FLUI binary with REAL
//! platform input/teardown and asserts on the observable outcome.
//!
//! Two modes:
//!
//! - **x11** (default): real X11 input under an X server. Run:
//!   `flui-live-smoke <path-to-app-binary>` (CI: `xvfb-run -a flui-live-smoke
//!   target/debug/examples/sliver_demo`), or via `just live-smoke`.
//! - **wayland**: close-path teardown check under a headless weston
//!   compositor. Run: `flui-live-smoke <path-to-app-binary> wayland`, or via
//!   `just live-smoke-wayland`. SKIPs (exit 0, clear message) when `weston`
//!   is not on PATH. See `wayland.rs` for why this mode cannot inject input
//!   the way the X11 harness does, and what it pins instead.
//!
//! # Why this exists
//!
//! The synthetic-event test harness covers everything DOWNSTREAM of event
//! synthesis and nothing upstream of it. Three live-runtime breakages
//! shipped while every gesture test was green: the winit translation
//! stamped no held buttons (drag-moves were hovers, live drag-scrolling
//! did nothing), a redraw request set flags without waking the parked
//! event loop (125 pointer events produced 2 frames), and closing the
//! window crashed after a clean teardown. This harness exercises exactly
//! that untested band: real platform translation, the real wake chain,
//! real teardown.
//!
//! # Checks
//!
//! 1. **launch** — the app opens an X window within the timeout.
//! 2. **drag scrolls** — an XTEST press-move-release drag changes the
//!    window's pixels (the list scrolls, the bar collapses).
//! 3. **wheel scrolls** — three XTEST wheel ticks move the content (the
//!    whole pointer-scroll wire: platform translation → hit-tested signal
//!    → the scrollable's immediate scroll).
//! 4. **clean close** — a `WM_DELETE_WINDOW` client message (a real window
//!    close, no window manager required) makes the process exit `0` within
//!    the timeout: no hang, no post-teardown crash.
//!
//! The wayland mode runs check 4's equivalent only (launch + clean close,
//! several cycles) — the band where the X11-green/Wayland-segfault teardown
//! ordering bug of issue #713 lived.

#[cfg(target_os = "linux")]
mod harness;
#[cfg(target_os = "linux")]
mod wayland;

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    use anyhow::Context;

    let app_path = std::env::args()
        .nth(1)
        .context("usage: flui-live-smoke <path-to-app-binary> [x11|wayland]")?;
    match std::env::args().nth(2).as_deref() {
        None | Some("x11") => harness::run(&app_path),
        Some("wayland") => wayland::run(&app_path),
        Some(other) => anyhow::bail!("unknown mode {other:?}: expected \"x11\" or \"wayland\""),
    }
}

#[cfg(not(target_os = "linux"))]
fn main() -> std::process::ExitCode {
    eprintln!("live-smoke drives an X11 server; only the Linux target is supported");
    std::process::ExitCode::from(2)
}

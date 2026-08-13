//! Live-smoke E2E harness — drives a REAL windowed FLUI binary with REAL X11
//! input and asserts on captured pixels and the process's exit code.
//!
//! Run: `flui-live-smoke <path-to-app-binary>` under an X server (CI:
//! `xvfb-run -a flui-live-smoke target/debug/examples/sliver_demo`), or via
//! `just live-smoke`, which builds the demo and wraps in Xvfb when no
//! display is present.
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

#[cfg(target_os = "linux")]
mod harness;

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    harness::run()
}

#[cfg(not(target_os = "linux"))]
fn main() -> std::process::ExitCode {
    eprintln!("live-smoke drives an X11 server; only the Linux target is supported");
    std::process::ExitCode::from(2)
}

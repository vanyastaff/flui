//! Executable AppKit close/teardown coverage (issue #1148, AppKit half).
//!
//! Drives the REAL AppKit close route from a bundled binary's `fn main` —
//! the AppKit main thread, the exact floor libtest cannot clear (a libtest
//! `#[test]` runs on a worker thread, and AppKit requires window construction
//! on the main thread): `current_platform()` → `open_window()` →
//! [`PlatformWindow::close`] → `windowWillClose:` → `handle_close`. It then
//! asserts the teardown ordering #1148 names and that programmatic `close()`
//! bypasses the should-close veto. Emits `CLOSE_PATH_PROBE_RESULT=PASS/FAIL`
//! and exits 0/1.
//!
//! macOS-only by construction: besides the main-thread floor, unbundled
//! NSWindow construction throws `_CFBundleGetValueForInfoKey` (a foreign
//! NSException Rust cannot catch). Run it on a real Mac via
//! `cargo xtask device macos-close-path`, which stages this example into a minimal `.app`
//! (the committed `Info.plist.close_path_probe` clears the bundle floor),
//! launches it with `RUST_LOG=info`, and asserts exit 0 plus the PASS marker.
//! On every other target the binary is a compile-time no-op main so the
//! workspace stays green; nothing here runs without the bundle anyway.

#[cfg(target_os = "macos")]
mod appkit_close_path_probe {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use flui_types::geometry::px;
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

    pub(crate) fn run() {
        tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::from_default_env())
            .with(tracing_subscriber::fmt::layer())
            .init();

        let mut failures: Vec<String> = Vec::new();

        let platform = flui_platform::current_platform()
            .expect("current_platform() must succeed on the AppKit main thread");
        if platform.name() != "macOS (AppKit)" {
            failures.push(format!(
                "expected platform macOS (AppKit), got {}",
                platform.name()
            ));
        }
        tracing::info!("platform: {}", platform.name());

        // A real NSWindow, constructed on the owner/main thread. It is never
        // ordered front (`visible: false`) — the close/teardown route under
        // test is the same `windowWillClose:` → `handle_close` path an
        // on-screen close takes through its shared tail.
        let window = platform
            .open_window(flui_platform::WindowOptions {
                title: "close-path probe".to_string(),
                size: flui_types::geometry::Size::new(px(400.0), px(300.0)),
                resizable: false,
                visible: false, // never order-front; the close route is what is tested
                decorated: true,
                min_size: None,
                max_size: None,
                ..Default::default()
            })
            .expect("open_window must construct a real NSWindow on the owner/main thread");

        // The `on_close` callback #1148 obliges to fire during the close
        // route, before the wrapper is dropped.
        let closed_by_callback = Arc::new(AtomicBool::new(false));
        let cb = Arc::clone(&closed_by_callback);
        window.on_close(Box::new(move || {
            cb.store(true, Ordering::SeqCst);
        }));

        // Negative should-close assertion: programmatic `close()` BYPASSES the
        // should-close veto. The veto callback is dispatched only from
        // `handle_close_request` (the `windowShouldClose:` delegate route —
        // the red button / `performClose:`), and AppKit's `-close` never sends
        // `windowShouldClose:` (trait `close` doc). A regression that rewires
        // `close()` to `performClose:` consults this veto and fails below.
        let veto_consulted = Arc::new(AtomicBool::new(false));
        let vc = Arc::clone(&veto_consulted);
        window.on_should_close(Box::new(move || {
            vc.store(true, Ordering::SeqCst);
            true // allow the close; the assertion cares that this is never asked
        }));

        // `window_handle()` must be Ok BEFORE close (a live window): if the
        // window were not actually constructed, every later obligation would
        // be moot.
        match window.window_handle() {
            Ok(_) => tracing::info!("obligation: window_handle Ok before close ✓"),
            Err(e) => failures.push(format!(
                "window_handle should be Ok before close, got {e:?}"
            )),
        }

        // Drive the programmatic close route (the same call the app-facing
        // `PlatformWindow::close` makes).
        tracing::info!("calling PlatformWindow::close()");
        window.close();

        // After close: `window_handle()` must be `Unavailable` — the
        // `closed`-flag obligation (#1148), the opposite of a stale/dangling
        // handle. Removing `closed.store` in `handle_close` fails this.
        match window.window_handle() {
            Err(raw_window_handle::HandleError::Unavailable) => {
                tracing::info!("obligation: window_handle Unavailable after close ✓");
            }
            other => failures.push(format!(
                "window_handle should be Unavailable after close, got {other:?}"
            )),
        }

        // The `on_close` callback must have fired — the
        // `windowWillClose:` → `handle_close` dispatch ordering.
        if closed_by_callback.load(Ordering::SeqCst) {
            tracing::info!("obligation: on_close callback fired ✓");
        } else {
            failures.push("on_close callback did not fire during close".to_string());
        }

        // The should-close veto must NOT have been consulted by the
        // programmatic route.
        if veto_consulted.load(Ordering::SeqCst) {
            failures.push(
                "on_should_close was consulted during programmatic close() — close() must \
                 bypass the should-close veto"
                    .to_string(),
            );
        } else {
            tracing::info!("obligation: should-close veto NOT consulted by close() ✓");
        }

        // Drop the last wrapper: with `setReleasedWhenClosed: NO` at
        // construction plus `handle_close`'s delegate niling, Drop must not
        // IMMEDIATELY over-release the NSWindow (a crash would exit via
        // SIGABRT, i.e. a nonzero status). Scoped to immediate releases: a
        // re-release deferred into an autorelease pool is invisible here
        // because the probe exits via `std::process::exit` and never drains
        // a pool (see the plan's residuals).
        drop(window);
        drop(platform);
        tracing::info!("wrapper dropped without an immediate over-release crash ✓");

        if failures.is_empty() {
            tracing::info!("CLOSE_PATH_PROBE_RESULT=PASS");
        } else {
            for f in &failures {
                tracing::error!("FAILURE: {f}");
            }
            tracing::error!("CLOSE_PATH_PROBE_RESULT=FAIL");
        }
        // Explicit exit keeps marker reporting deterministic; an AppKit crash
        // after `main` returns would otherwise also surface as a nonzero exit,
        // but the markers must be authoritative first.
        if failures.is_empty() {
            std::process::exit(0);
        } else {
            std::process::exit(1);
        }
    }
}

#[cfg(target_os = "macos")]
fn main() {
    appkit_close_path_probe::run();
}

/// Non-macOS build placeholder: this probe needs the AppKit main thread and
/// a bundle to construct a window at all; on other targets it exists only so
/// the workspace compiles. Run it with `cargo xtask device macos-close-path` on a real Mac.
#[cfg(not(target_os = "macos"))]
fn main() {}

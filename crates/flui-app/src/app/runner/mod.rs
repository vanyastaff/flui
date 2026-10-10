//! Application runner - entry points for running FLUI apps.
//!
//! This module provides platform-agnostic entry points that delegate
//! to platform-specific implementations via flui-platform.

use flui_view::{StatelessView, View};

use super::AppConfig;

#[cfg(target_os = "android")]
mod android;
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
mod desktop;
mod device_recovery;
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
mod first_reveal;
mod fonts;
mod frame_driver;
mod frame_pacing;
mod host;
mod installed_host;
#[cfg(target_os = "ios")]
pub(super) mod ios;
mod native_bindings;
mod native_retirement;
mod native_text_sizing;
mod window_install;

mod owner_dispatch;
// Unconditional, like `device_recovery` above: the backoff's trait and
// outcome are portable and its tests are host-run, so a
// `cfg(target_os = "android")` here would hide the whole file from every gate
// this host can run.
mod retry_backoff;
mod secondary_window;
#[cfg(any(
    target_os = "ios",
    all(test, not(target_os = "android"), not(target_arch = "wasm32"))
))]
mod session_controller;
// Unconditional, like `device_recovery` above: the seam's trait and outcome
// are portable and its tests are host-run, so a `cfg(target_os = "android")`
// here would hide the whole file from every gate this host can run.
mod surface_lifecycle;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(target_os = "android")]
pub use android::{run_app_android, run_app_android_with_config};
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
use desktop::run_desktop;
pub use fonts::{FontRegistrationError, register_font};
pub(crate) use host::{OwnerHostClearGuard, install_owner_platform, with_owner_platform};
pub(in crate::app) use installed_host::InstalledHost;
#[cfg(target_os = "ios")]
pub use ios::{run_app_ios, run_app_ios_with_config};
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub use secondary_window::open_secondary_window;
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub use secondary_window::open_window;
#[cfg(target_arch = "wasm32")]
use web::run_web;

/// The presentation window for a freshly opened host window: the window
/// itself, upcast to the contract the UI runtime drives, the accessibility
/// bridge its backend fixed when it built it, and its text-store host when
/// the backend's input methods pull from the field (ADR-0135).
///
/// The one place the runner turns an `open_window` result into what a UI runtime
/// constructor takes. Reading the bridge and the host here, once, is sound
/// because every backend sets both at construction and never swaps them.
/// The host is owner-thread state, so it is read through the loop's
/// [`OwnerPlatform`](flui_platform::OwnerPlatform); without one installed
/// (no loop on this thread) the window has none.
pub(crate) fn presentation_window(
    host: std::sync::Arc<dyn flui_platform::traits::HostWindow>,
) -> crate::app::presentation::PresentationWindow {
    let accessibility = host.accessibility();
    let text_store_host = text_store_host_of(&host);
    crate::app::presentation::PresentationWindow::new(host, accessibility)
        .with_text_store_host(text_store_host)
}

fn text_store_host_of(
    window: &std::sync::Arc<dyn flui_platform::traits::HostWindow>,
) -> Option<std::rc::Rc<dyn flui_platform_api::TextStoreHost>> {
    host::with_owner_platform(|owner| owner.text_store_host(window)).flatten()
}

/// Close the window at `address` programmatically, bypassing its
/// close-request handler (issue #558).
///
/// This is how an application finishes a close it kept open: a
/// [`CloseRequestHandler`](crate::CloseRequestHandler) that answered
/// [`CloseResponse::KeepOpen`](crate::CloseResponse) in order to save
/// unsaved work calls this with the address it received in the
/// [`CloseRequest`](crate::CloseRequest) once the work is done, and the
/// window closes on the normal path — `on_close`, presentation teardown,
/// and then the exit-policy question if this was the last window.
///
/// The handler is deliberately not asked again: every backend's own
/// `PlatformWindow::close` bypasses its native close-request path for the
/// same reason (AppKit's `-close` does not send `windowShouldClose:`;
/// Win32's `DestroyWindow` does not send `WM_CLOSE`).
///
/// Owner-thread only, like every other operation on a hosted UI runtime — a
/// call from a worker thread is REFUSED with a typed error rather than
/// silently doing nothing. That matters for the deferral case above: the
/// work an application finishes before calling this often finishes on a
/// worker, and `AppRuntime` is thread-local, so a worker would otherwise
/// look at its own empty runtime and report "nothing registered". Marshal
/// back to the thread that ran the handler.
///
/// The check is thread-scoped rather than process-scoped because "the
/// owner thread" is not a process-wide fact: a process may host several
/// event loops on several threads, each with its own `AppRuntime`. Merely
/// reading this thread's slot is side-effect-free by construction (see
/// `AppRuntime::new`'s own doc) — it resolves no services and constructs
/// no singletons.
///
/// # Backend behaviour
///
/// This resolves the close fully on the **headless**, **winit**, **Win32**
/// and **AppKit** backends, whose `PlatformWindow::close` performs a real
/// close. Timing differs: on **winit** the teardown (`on_close`, window-map
/// removal, exit-policy consult) runs on the event-loop owner's next turn,
/// after this call returns — never synchronously inside it (issue #919's
/// fix) — while the **headless** double runs `on_close` synchronously
/// inside `close()`. On **web** and **Android** the veto itself is inert
/// (neither backend consults `dispatch_should_close`), so there is nothing
/// to resolve.
///
/// # Errors
///
/// [`CloseRequestError::NoHostedRuntime`](crate::CloseRequestError::NoHostedRuntime)
/// when the calling thread hosts no FLUI runtime,
/// [`CloseRequestError::WrongThread`](crate::CloseRequestError::WrongThread)
/// when it hosts one but not the presentation's,
/// [`CloseRequestError::UnknownPresentation`](crate::CloseRequestError::UnknownPresentation)
/// when no presentation is registered at `address` (never installed, or
/// already closed), and
/// [`CloseRequestError::WindowGone`](crate::CloseRequestError::WindowGone)
/// when its native window is already destroyed.
pub fn request_presentation_close(
    address: flui_foundation::PresentationAddress,
) -> Result<(), crate::app::close_request::CloseRequestError> {
    use crate::app::close_request::CloseRequestError;

    // Read the owner thread and the router in ONE visit, then leave the
    // thread-local: `PlatformWindow::close` fires the window's own
    // `on_close`, which re-enters `APP_RUNTIME` through
    // `close_this_window`. A thread that hosts no loop has `owner_thread`
    // unset — that, not an empty router, is what distinguishes "you are on
    // the wrong thread" from "that presentation is gone".
    let (owner_thread, router) = host::APP_RUNTIME.with(|slot| {
        let state = slot.borrow();
        (state.owner_thread, state.close_requests())
    });
    match owner_thread {
        None => Err(CloseRequestError::NoHostedRuntime),
        Some(owner) if owner != std::thread::current().id() => Err(CloseRequestError::WrongThread),
        Some(_) => router.request_close(address),
    }
}

/// Run a FLUI application with default configuration.
///
/// This is the internal implementation called by `run_app()`.
pub fn run_app_impl<V>(root: V)
where
    V: View + StatelessView + Clone + 'static,
{
    run_app_with_config_impl(root, AppConfig::default());
}

/// Run a FLUI application with custom configuration.
///
/// This is the internal implementation called by `run_app_with_config()`.
pub fn run_app_with_config_impl<V>(root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + 'static,
{
    // Managed startup: install FLUI's default backend only if the slot is
    // empty. An application that configured its own subscriber before calling
    // `run_app` keeps it, and a second `run_app` in one process is a no-op
    // rather than a panic.
    let _installation = super::logging::init_managed_logging(&config);

    // No frame-pacing field is logged here: `AppConfig` carries none — the
    // advisory-only `vsync`/`target_fps` fields it used to have were removed
    // rather than kept misleading. The desktop runner's steady-state pacing
    // comes entirely from the GPU-side present path
    // (`flui_engine::Renderer::render_scene`) today — the blocking Fifo
    // present on the Vulkan/Wayland path, and the platform's display-pass
    // cadence on the native AppKit backend (ADR-0058's per-backend facts).
    // The unwired `RasterOptions` DTO was deleted with no reader rather than
    // kept as a shape; a frame-pacing surface returns with the threaded lane
    // that can act on one — that wiring is #559's job, not a claim this
    // comment gets to make in the meantime.
    tracing::info!(
        title = %config.title,
        size = ?config.size,
        "Starting FLUI application"
    );

    // Run platform-specific event loop
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    {
        run_desktop(root, config);
    }

    #[cfg(target_os = "android")]
    {
        let _ = (root, config);
        panic!(
            "On Android, use flui_app::run_app_android() from android_main() \
             instead of run_app(). AndroidApp must be provided by the system."
        );
    }

    #[cfg(target_os = "ios")]
    {
        run_app_ios_with_config(root, config);
    }

    #[cfg(target_arch = "wasm32")]
    {
        run_web(root, config);
    }
}

#[cfg(test)]
mod tests {

    use super::host::APP_RUNTIME;

    // `teardown_platform_ui_runtime` is `cfg(all(not(ios), not(wasm32)))` -- neither
    // platform runs the desktop teardown path it exercises.

    use super::*;

    /// Bootstrap ordering invariant shared by `bootstrap_desktop`, `run_android`,
    /// and `run_web`: the window must be stored in `AppRuntime`'s redraw-poke
    /// slot before anything that could synchronously observe it (the initial
    /// redraw request, `Lifecycle::Started`) runs — otherwise the first such
    /// observer would silently see nothing installed.
    ///
    /// `bootstrap_desktop`/`run_android`/`run_web` themselves cannot run in a
    /// unit test: each opens its window from inside a live platform event loop
    /// (`ActiveEventLoop` is unreachable outside `Platform::run`) and creates a
    /// real GPU `Renderer`, gated behind the separate `testing` CI job
    /// (WARP), not this one. This instead drives the exact ordering invariant
    /// headlessly: `HeadlessWindow::request_redraw` (flui-platform's headless
    /// backend, used elsewhere in this crate's tests) dispatches its
    /// `on_request_frame` callback SYNCHRONOUSLY — unlike a real winit window,
    /// where a queued `RedrawRequested` would not fire until `on_ready` (and
    /// this reordering) has already returned. That is exactly why the ordering
    /// bug was invisible in a real window's actual first frame but is directly
    /// observable here.
    ///
    /// Checks a unique window *size* rather than mere `is_some()`, so this
    /// cannot pass merely because an earlier test left SOME window installed
    /// — only THIS test's window, with THIS test's unmistakable marker size,
    /// proves `set_redraw_window` ran before the callback.
    ///
    /// If reverted: swap the order of the two calls below (request the
    /// redraw, then store the window — the pre-fix shape) and this fails:
    /// `wake_frame` finds no window yet, never calls `request_redraw` on it,
    /// and the callback never fires at all.
    ///
    /// No test lock: this touches `APP_RUNTIME`, a `thread_local!`, and the
    /// standard library test harness runs each `#[test]` on its own freshly
    /// spawned thread, so a fresh `AppRuntime` (no UI runtime, no owner platform)
    /// is what this test's thread starts from regardless of what any other
    /// concurrently-running test does on ITS OWN thread — the same reasoning
    /// this file's other thread-local-only tests below rely on. The retired
    /// `AppBinding`-era version of this test carried a dedicated per-test
    /// window-identity lock, and later, briefly, `UpdateScheduler` carried a
    /// sibling per-test scheduler-phase lock; both are deleted now, not
    /// ported forward, because the state each one guarded
    /// (`AppBinding::instance()`'s active window, and the process-global
    /// half of the `UpdateScheduler` singleton respectively) no longer exists —
    /// `AppBinding` is gone entirely and every `UiRuntime` owns its own fresh
    /// `UpdateScheduler` value — and because a per-test-thread thread-local needs
    /// no cross-test lock in the first place.
    fn desktop_bootstrap_stores_the_window_before_the_first_synchronous_redraw_observes_it() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let marker_size = flui_foundation::geometry::Size::new(4001.0, 4002.0);

        let platform = flui_platform::headless_platform();
        let window = platform
            .open_window(flui_platform::traits::WindowOptions {
                size: marker_size,
                ..Default::default()
            })
            .expect("headless platform always opens a window");

        // `on_request_frame` requires `Send` on the callback; `AppRuntime` is
        // not `Send` (it holds owner-thread-affine ui_runtime state), so the
        // closure below cannot capture a specific `&AppRuntime`. Resolving
        // `APP_RUNTIME` fresh inside the closure (zero captures for the
        // runtime itself) sidesteps that entirely.
        //
        // Reads through `with_redraw_window`, NOT `wake_frame`/`request_redraw`:
        // a headless window's `request_redraw` dispatches this very callback
        // synchronously, so calling anything that re-locks the redraw-poke
        // slot from in here (the two are on the same thread, same call
        // stack) would deadlock on the slot's own non-reentrant lock.
        let saw_marker_window = Arc::new(AtomicBool::new(false));
        let saw_marker_window_cb = Arc::clone(&saw_marker_window);
        window.on_request_frame(Box::new(move || {
            let matches_marker = APP_RUNTIME
                .with(|slot| {
                    slot.borrow()
                        .with_redraw_window(|w| w.bounds().size == marker_size)
                })
                .unwrap_or(false);
            saw_marker_window_cb.store(matches_marker, Ordering::SeqCst);
        }));

        // Mirrors the FIXED order in `bootstrap_desktop`/`run_android`:
        // store the window BEFORE requesting the initial redraw. `wake_frame`
        // (not a direct `request_redraw()` on the window) clones the window
        // out from under the lock before calling through, so this call
        // cannot deadlock against the callback's own `with_redraw_window`
        // re-entry above.
        APP_RUNTIME.with(|slot| {
            let state = slot.borrow();
            state.set_redraw_window(window);
            state.wake_frame();
        });

        assert!(
            saw_marker_window.load(Ordering::SeqCst),
            "set_redraw_window must have taken effect before the initial redraw \
             fires the frame callback that could read the redraw-poke slot",
        );
        // Clean up so this test's window does not linger for whatever test
        // runs next on this pool thread.
        let released = APP_RUNTIME.with(|slot| slot.borrow().clear_redraw_window());
        drop(released);
    }

    // ========================================================================
    // Owner-platform host tests (ADR-0039 §6)
    // ========================================================================

    fn owner_platform_host_panic_in_on_ready_still_clears() {
        use flui_platform::headless_platform;

        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _clear_guard = OwnerHostClearGuard::arm();
            let platform = headless_platform();
            let _ = platform.run(Box::new(|owner| {
                install_owner_platform(owner).expect("install owner wake transport");
                panic!("exercise on_ready panic cleanup");
            }));
        }));

        assert!(unwind.is_err(), "on_ready's panic must propagate");
        assert!(
            with_owner_platform(|_| ()).is_none(),
            "a panic inside on_ready must still unwind through the clear guard \
             (armed before Platform::run, not inside on_ready) rather than \
             leaking the host onto this thread"
        );
    }

    /// A host that records what it was told to focus, from wherever the
    /// headless platform's factory built it.
    struct FocusLog(std::sync::Arc<parking_lot::Mutex<Vec<bool>>>);

    impl flui_platform_api::text_store::TextStoreHost for FocusLog {
        fn focus_store(&self, store: Option<std::rc::Rc<dyn flui_platform_api::TextStore>>) {
            self.0.lock().push(store.is_some());
        }

        fn complete_composition(
            &self,
            _: &std::rc::Rc<dyn flui_platform_api::TextStore>,
        ) -> Result<
            flui_platform_api::text_store::CompositionEnd,
            flui_platform_api::text_store::TextStoreHostError,
        > {
            Ok(flui_platform_api::text_store::CompositionEnd::Committed)
        }
    }

    /// A pull-model window's host reaches the presentation the runner builds
    /// for it: a field that attaches is focused on the host
    /// (`presentation_window`, ADR-0135).
    ///
    /// Red-check: pass `None` to `with_text_store_host` in
    /// `presentation_window` — the presentation falls back to the window's
    /// push capability and the host hears nothing.
    fn presentation_window_hands_a_pull_window_s_host_to_its_presentation() {
        use std::sync::Arc;

        use flui_platform::Platform as _;

        let heard = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let log = Arc::clone(&heard);
        let platform = flui_platform::HeadlessPlatform::new()
            .with_text_store_host(move || std::rc::Rc::new(FocusLog(Arc::clone(&log))));
        let _clear_guard = OwnerHostClearGuard::arm();
        Box::new(platform)
            .run(Box::new(|owner| {
                install_owner_platform(owner).expect("install owner wake transport");
                let window = with_owner_platform(|owner| {
                    owner.open_window(flui_platform::WindowOptions::default())
                })
                .expect("BUG: install_owner_platform just ran above")
                .and_then(flui_platform::WindowOpen::try_ready)
                .expect("headless open_window is always Ready");
                let ui_runtime = host::build_ui_runtime(
                    &host::runtime_wake_callback(),
                    presentation_window(window),
                    1.0,
                )
                .expect("ui_runtime");
                let store = flui_platform_api::text_store::InMemoryTextStore::new("");
                let _token = ui_runtime
                    .text_input_handle()
                    .attach(flui_interaction::TextInputClient::new(store))
                    .expect("the presentation takes text input");
                Ok(())
            }))
            .expect("headless run");
        assert_eq!(
            *heard.lock(),
            [true, false],
            "focused, then unfocused at teardown"
        );
    }

    #[test]
    fn runner_bootstrap_matrix() {
        crate::table_test::run_table(
            "runner_bootstrap_matrix",
            &[
                ("desktop_bootstrap_stores_the_window_before_the_first_synchronous_redraw_observes_it", desktop_bootstrap_stores_the_window_before_the_first_synchronous_redraw_observes_it as fn()),
                ("owner_platform_host_panic_in_on_ready_still_clears", owner_platform_host_panic_in_on_ready_still_clears as fn()),
                ("presentation_window_hands_a_pull_window_s_host_to_its_presentation", presentation_window_hands_a_pull_window_s_host_to_its_presentation as fn()),
            ],
        );
    }
}

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(in crate::app) mod main_window;
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(in crate::app) use main_window::run_application;

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
mod frame_pacing;
mod host;
mod lifecycle_ladder;
mod realm_dispatch;
mod secondary_window;
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
#[cfg(not(target_os = "ios"))]
pub(crate) use host::{OwnerHostClearGuard, install_owner_platform, with_owner_platform};
#[cfg(not(target_os = "ios"))]
pub(in crate::app) use realm_dispatch::{RealmTask, SurfaceApplier};
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub use secondary_window::open_secondary_window;
#[cfg(target_arch = "wasm32")]
use web::run_web;

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
    // comes entirely from the GPU-side blocking Fifo present
    // (`flui-engine::wgpu::Renderer::render_scene`) today. `flui_engine::
    // RasterOptions` exists as the shape a future frame-pacing surface would
    // take, but nothing reads it at the raster boundary yet (`RasterOwner`
    // stores its `RasterOptions` and never acts on it) — that wiring is
    // #559's job, not a claim this comment gets to make in the meantime.
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
        run_ios(config);
    }

    #[cfg(target_arch = "wasm32")]
    {
        run_web(root, config);
    }
}

// ============================================================================
// iOS Implementation
// ============================================================================

#[cfg(target_os = "ios")]
fn run_ios(_config: AppConfig) {
    // Native iOS (UIKit windowing + surface) is a Cross.P (Platform breadth)
    // deliverable — see docs/ROADMAP.md's Cross.P section. This stub exists
    // only so `#[cfg(target_os = "ios")]` builds compile; there is no
    // UIKit-backed `flui-platform` implementation to call into yet.
    tracing::info!("iOS platform - not yet implemented");
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use flui_types::geometry::px;
    use flui_view::{BuildContext, IntoView, View, ViewExt};

    use super::host::APP_RUNTIME;
    use super::realm_dispatch::{
        RealmDispatcher, dispatch_platform_realm, install_platform_realm, teardown_platform_realm,
    };
    use super::*;

    /// Trivial leaf fixture: an empty view used as the terminal node under
    /// `OwnerLocalRoot` below, and constructible on its own wherever a test
    /// needs a minimal `View + StatelessView` root.
    #[derive(Clone)]
    struct TestView;

    impl StatelessView for TestView {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            TestView.boxed()
        }
    }

    impl View for TestView {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateless(self)
        }
    }

    #[derive(Clone)]
    struct OwnerLocalRoot {
        value: Rc<Cell<usize>>,
    }

    impl StatelessView for OwnerLocalRoot {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            self.value.set(self.value.get() + 1);
            TestView.boxed()
        }
    }

    impl View for OwnerLocalRoot {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateless(self)
        }
    }

    #[test]
    fn runner_entrypoints_accept_owner_local_root_state() {
        static_assertions::assert_not_impl_any!(OwnerLocalRoot: Send, Sync);

        std::hint::black_box(run_app_impl::<OwnerLocalRoot> as fn(OwnerLocalRoot));
        std::hint::black_box(
            run_app_with_config_impl::<OwnerLocalRoot> as fn(OwnerLocalRoot, AppConfig),
        );
    }

    #[test]
    fn test_config_creation() {
        let config = AppConfig::new().with_title("Test").with_size(800, 600);

        assert_eq!(config.title, "Test");
        assert_eq!(config.size.width, px(800.0));
    }

    /// Bootstrap ordering invariant shared by `bootstrap_desktop`, `run_android`,
    /// and `run_web`: the window must be stored in `AppRuntime`'s redraw-poke
    /// slot before anything that could synchronously observe it (the initial
    /// redraw request, `Lifecycle::Started`) runs — otherwise the first such
    /// observer would silently see nothing installed.
    ///
    /// `bootstrap_desktop`/`run_android`/`run_web` themselves cannot run in a
    /// unit test: each opens its window from inside a live platform event loop
    /// (`ActiveEventLoop` is unreachable outside `Platform::run`) and creates a
    /// real GPU `Renderer`, gated behind the separate `enable-wgpu-tests` CI job
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
    /// spawned thread, so a fresh `AppRuntime` (no realm, no owner platform)
    /// is what this test's thread starts from regardless of what any other
    /// concurrently-running test does on ITS OWN thread — the same reasoning
    /// this file's other thread-local-only tests below rely on. The retired
    /// `AppBinding`-era version of this test carried a dedicated per-test
    /// window-identity lock, and later, briefly, `UpdateScheduler` carried a
    /// sibling per-test scheduler-phase lock; both are deleted now, not
    /// ported forward, because the state each one guarded
    /// (`AppBinding::instance()`'s active window, and the process-global
    /// half of the `UpdateScheduler` singleton respectively) no longer exists —
    /// `AppBinding` is gone entirely and every `UiRealm` owns its own fresh
    /// `UpdateScheduler` value — and because a per-test-thread thread-local needs
    /// no cross-test lock in the first place.
    #[test]
    fn desktop_bootstrap_stores_the_window_before_the_first_synchronous_redraw_observes_it() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let marker_size = flui_types::Size::new(px(4001.0), px(4002.0));

        let platform = flui_platform::headless_platform();
        let window = platform
            .open_window(flui_platform::traits::WindowOptions {
                size: marker_size,
                ..Default::default()
            })
            .expect("headless platform always opens a window");

        // `on_request_frame` requires `Send` on the callback; `AppRuntime` is
        // not `Send` (it holds owner-thread-affine realm state), so the
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

    #[test]
    fn owner_platform_host_installs_and_clears_around_run() {
        use flui_platform::headless_platform;

        assert!(
            with_owner_platform(|_| ()).is_none(),
            "no host installed before any on_ready has run on this thread"
        );

        // `PlatformReadyCallback` is `Box<dyn FnOnce(OwnerPlatform) + 'static>`,
        // so the closure below cannot borrow a stack-local `Cell` — `Rc`
        // gives it an owned handle instead (single-threaded: headless `run`
        // invokes `on_ready` synchronously, on this same thread).
        let seen_while_installed = Rc::new(Cell::new(false));
        let seen_while_installed_for_closure = Rc::clone(&seen_while_installed);
        {
            let _clear_guard = OwnerHostClearGuard::arm();
            let platform = headless_platform();
            let result = platform.run(Box::new(move |owner| {
                install_owner_platform(owner);
                let observed = with_owner_platform(|_owner| true);
                seen_while_installed_for_closure.set(observed == Some(true));
                Ok(())
            }));
            assert!(result.is_ok(), "on_ready returns Ok here");
        } // `_clear_guard` drops here.

        assert!(
            seen_while_installed.get(),
            "the accessor must yield Some(_) while a host is installed"
        );
        assert!(
            with_owner_platform(|_| ()).is_none(),
            "the clear guard must remove the host once its scope ends"
        );
    }

    /// `install_owner_platform` alone -- the exact path `run_direct` takes,
    /// which opens a window but never installs a `UiRealm` -- must NOT
    /// resolve `SharedEngineServices`. Only `install_platform_realm`
    /// (exercised by the realm-install tests elsewhere in this file) does
    /// that, so a backend that never hosts a realm never pays for
    /// painting/semantics/scheduler singleton construction or full
    /// system-font enumeration it cannot use.
    #[test]
    fn install_owner_platform_alone_does_not_resolve_services() {
        use flui_platform::headless_platform;

        let _clear_guard = OwnerHostClearGuard::arm();
        let platform = headless_platform();
        let result = platform.run(Box::new(|owner| {
            install_owner_platform(owner);
            assert!(
                !APP_RUNTIME.with(|slot| slot.borrow().services_resolved()),
                "install_owner_platform alone must not resolve SharedEngineServices"
            );
            Ok(())
        }));
        assert!(result.is_ok(), "on_ready returns Ok here");
    }

    #[test]
    fn owner_platform_host_panic_in_on_ready_still_clears() {
        use flui_platform::headless_platform;

        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _clear_guard = OwnerHostClearGuard::arm();
            let platform = headless_platform();
            let _ = platform.run(Box::new(|owner| {
                install_owner_platform(owner);
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

    /// `on_ready` returning `Err` must propagate all the way out of
    /// `Platform::run` — not be swallowed into
    /// a bare log while the loop keeps running a half-built app — AND the
    /// `AppRuntime.owner_platform` TLS clear guard (armed before `run`, per
    /// the existing unwind-safety contract) must still fire on this ordinary
    /// `Err` return, exactly as it does on a panic.
    #[test]
    fn owner_platform_host_on_ready_error_propagates_and_still_clears() {
        use flui_platform::headless_platform;

        let result = {
            let _clear_guard = OwnerHostClearGuard::arm();
            let platform = headless_platform();
            platform.run(Box::new(|owner| {
                install_owner_platform(owner);
                assert!(
                    with_owner_platform(|_| ()).is_some(),
                    "the host is installed while on_ready runs, even on the \
                     path that is about to fail"
                );
                Err("simulated bootstrap failure".into())
            }))
        }; // `_clear_guard` drops here -- before the assertions below.

        assert!(
            result.is_err(),
            "on_ready's Err must propagate out of Platform::run, not be \
             swallowed"
        );
        assert!(
            with_owner_platform(|_| ()).is_none(),
            "the clear guard must still remove the host on the Err path, \
             the same as it does on a panic"
        );
    }

    #[test]
    #[should_panic(
        expected = "with_owner_platform called while the installed realm's scheduler is inside"
    )]
    #[cfg_attr(
        not(debug_assertions),
        ignore = "the fence is a debug_assert!; release builds don't panic"
    )]
    fn owner_platform_accessor_fences_the_installed_realms_frame_transaction() {
        use flui_platform::headless_platform;

        let _clear_guard = OwnerHostClearGuard::arm();
        let window = headless_platform()
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create a test window");
        let dispatcher =
            install_platform_realm(super::super::ui_realm::UiRealm::for_test(), &window);

        // A clone of the installed realm's OWN scheduler -- same underlying
        // `SchedulerInner` fence (c) reads through `installed_realm_phase`.
        // Driven directly here, with the realm still resident in its slot
        // (not checked out via `dispatch_platform_realm`) -- the sibling
        // test right below this one, `..._through_dispatch`, pins the other
        // half: the realm checked OUT for a dispatched task, where
        // `installed_realm_phase` must fall back to `dispatched_scheduler`
        // instead of reading the resident slot directly.
        let (scheduler, local_post_frame) = APP_RUNTIME.with(|slot| {
            let borrowed = slot.borrow();
            let realm = borrowed
                .realms
                .get(&dispatcher.address.realm_id)
                .and_then(|realm_slot| realm_slot.realm.as_ref())
                .expect("just installed above");
            (
                realm.scheduler().clone(),
                realm.local_post_frame_lane().clone(),
            )
        });

        // `drive_frame` leaves the scheduler in `PersistentCallbacks` for the
        // duration of its `pipeline` closure -- a forbidden phase per fence
        // (c). A panicking pipeline is caught internally and resolved back
        // to `Idle` via `abort_frame()` before the panic resumes, so this
        // test's own `#[should_panic]` unwind leaves the scheduler clean.
        let now = web_time::Instant::now();
        scheduler.drive_frame_with_lane(
            now,
            flui_scheduler::IdleDeadline::far_future(now),
            || {
                let _ = with_owner_platform(|_owner| ());
            },
            &local_post_frame,
        );
    }

    /// The through-dispatch half of the fence-(c) pin above: `with_owner_platform`
    /// called from INSIDE a `RealmTask::Frame` running through
    /// `dispatch_platform_realm` -- not driven directly against a resident
    /// realm -- must still trip the debug_assert while a frame phase is
    /// active.
    ///
    /// Red before the `dispatched_scheduler` fallback existed:
    /// `dispatch_platform_realm` checks the realm's `UiRealm` OUT of its slot
    /// for the entire extent of the dispatched task (see its own doc), so
    /// `installed_realm_phase` reading only resident slots would observe
    /// `None` for this call, not `PersistentCallbacks` -- vacuously "not
    /// mid-frame",
    /// the debug_assert would pass, and this test would fail its
    /// `#[should_panic]` expectation. Green now because
    /// `dispatch_platform_realm` stashes a clone of the checked-out realm's
    /// scheduler into `dispatched_scheduler` before running the queued task,
    /// and `installed_realm_phase` falls back to it exactly when `realm`
    /// itself is empty.
    #[test]
    #[should_panic(
        expected = "with_owner_platform called while the installed realm's scheduler is inside"
    )]
    #[cfg_attr(
        not(debug_assertions),
        ignore = "the fence is a debug_assert!; release builds don't panic"
    )]
    fn owner_platform_accessor_fences_the_installed_realms_frame_transaction_through_dispatch() {
        use flui_platform::headless_platform;

        let _clear_guard = OwnerHostClearGuard::arm();
        let window = headless_platform()
            .open_window(flui_platform::WindowOptions::default())
            .expect("headless platform should create a test window");
        let dispatcher =
            install_platform_realm(super::super::ui_realm::UiRealm::for_test(), &window);

        // Unlike the sibling test above, this drives the frame from INSIDE a
        // `RealmTask::Frame` dispatched through `dispatch_platform_realm` --
        // the realm is checked out of its registry slot for the whole
        // closure below, exactly the window `dispatched_scheduler` exists to
        // cover. `drive_frame`'s `PersistentCallbacks` phase is active while
        // `with_owner_platform` is called, so the fence must trip here
        // exactly as it does when driven directly.
        let dispatch_result = dispatch_platform_realm(
            dispatcher,
            RealmTask::Frame(Box::new(|realm| {
                let now = web_time::Instant::now();
                realm.scheduler().drive_frame_with_lane(
                    now,
                    flui_scheduler::IdleDeadline::far_future(now),
                    || {
                        let _ = with_owner_platform(|_owner| ());
                    },
                    realm.local_post_frame_lane(),
                );
            })),
        );
        // Unreachable on the fence-tripping path (the debug_assert panics
        // first, unwinding out of `dispatch_platform_realm` before it can
        // return) -- kept only so a build without debug assertions (where
        // this test is `ignore`d) still type-checks the dispatch call.
        let _ = dispatch_result;
    }

    /// Hot-restart survival (ADR-0039 §6): `owner_platform`
    /// is a loop-scoped `AppRuntime` field, deliberately not cleared by
    /// `teardown_platform_realm` alongside the realm-facing fields it DOES
    /// clear (`realm`, `queue`, `owner_thread`, `address`,
    /// `surface_applier`) -- tearing down a realm on the owner thread must
    /// not strand the loop's capability, because the loop may host a fresh
    /// realm next without ever calling `Platform::run` again (hot-restart
    /// does exactly this today, `install_platform_realm`).
    #[test]
    fn owner_platform_survives_realm_teardown() {
        use flui_platform::headless_platform;

        // `Rc<Cell<_>>`, not a bare local: the `on_ready` closure below is
        // `Box<dyn FnOnce(OwnerPlatform) + 'static>`, so it cannot borrow a
        // stack local -- see the sibling install/clear test's identical
        // note. `(bool, bool)` is `Copy`, so `Cell` suffices.
        let observed = Rc::new(Cell::new((false, false)));
        let observed_for_closure = Rc::clone(&observed);

        let _clear_guard = OwnerHostClearGuard::arm();
        let platform = headless_platform();
        let result = platform.run(Box::new(move |owner| {
            install_owner_platform(owner);
            let before_teardown = with_owner_platform(|_owner| true) == Some(true);

            // Simulate hot-restart: a realm's teardown runs on this owner
            // thread while the loop keeps running (headless `run` returns
            // immediately either way, but the TLS host's contract does not
            // depend on that -- it is exercised identically whether the
            // loop is about to return or about to host another realm).
            teardown_platform_realm();

            let after_teardown = with_owner_platform(|_owner| true) == Some(true);
            observed_for_closure.set((before_teardown, after_teardown));
            Ok(())
        }));
        assert!(result.is_ok(), "on_ready returns Ok here");

        let (before_teardown, after_teardown) = observed.get();
        assert!(
            before_teardown,
            "the host must be installed before teardown runs"
        );
        assert!(
            after_teardown,
            "teardown_platform_realm must not clear AppRuntime.owner_platform -- \
             the loop may host another realm before it exits (hot-restart)"
        );
    }

    /// Regression pin for the "No host re-entry" rule on `with_owner_platform`'s
    /// own rustdoc: since `AppRuntime` folded the realm-facing state and
    /// `owner_platform` into one `RefCell`, a closure that calls back into
    /// any function touching that same cell while `with_owner_platform`
    /// still holds its immutable borrow is a guaranteed `BorrowMutError`
    /// panic. `dispatch_platform_realm` is the stand-in host op here; the
    /// same panic would fire for `install_platform_realm`,
    /// `teardown_platform_realm`, or `install_surface_applier` instead, for
    /// the identical reason (all of them `borrow_mut()` the same cell).
    #[test]
    // Substring match, not the full message: `RefCell`'s panic wording
    // ("already borrowed: BorrowMutError" vs. "already mutably borrowed:
    // BorrowError" depending on which side re-enters) has varied across
    // Rust versions and could vary again; "borrow" is the one substring
    // present in every variant, so this still fails on an unrelated panic
    // while staying stable across toolchains.
    #[should_panic(expected = "borrow")]
    fn with_owner_platform_reentering_dispatch_panics() {
        use flui_platform::headless_platform;

        let _clear_guard = OwnerHostClearGuard::arm();
        let platform = headless_platform();
        let _ = platform.run(Box::new(|owner| {
            install_owner_platform(owner);
            with_owner_platform(|_owner| {
                // Any host op re-entering here panics: `with_owner_platform`
                // still holds `APP_RUNTIME.borrow()` for the duration of
                // this closure, and `dispatch_platform_realm` immediately
                // tries `slot.borrow_mut()` on the very first line of its
                // own TLS access.
                let dispatcher = RealmDispatcher {
                    owner_thread: std::thread::current().id(),
                    address: flui_foundation::PresentationAddress {
                        realm_id: flui_foundation::RealmId::new_gen(
                            0,
                            std::num::NonZeroU32::new(1).unwrap(),
                        ),
                        presentation_id: flui_foundation::PresentationId::new_gen(
                            0,
                            std::num::NonZeroU32::new(1).unwrap(),
                        ),
                    },
                };
                let _ = dispatch_platform_realm(dispatcher, RealmTask::Frame(Box::new(|_| {})));
            });
            Ok(())
        }));
    }
}

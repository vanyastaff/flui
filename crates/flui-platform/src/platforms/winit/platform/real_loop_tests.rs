//! Tests that drive a REAL winit `EventLoop`.
//!
//! Split out of `platform.rs`'s inline test module: this is one cohesive
//! family — every test here runs `event_loop.run_app` on an ordinary test
//! thread via [`build_test_event_loop`] — and together with its helpers it
//! outgrew the budget of a 3.9k-line file (issue #923). Nothing else in the
//! crate uses these helpers, which is why all four moved rather than being
//! left behind as a shared prelude.
//!
//! Declared with `#[path]` from `platform.rs` as a sibling of its `tests`
//! module, so these tests read as `platform::real_loop_tests::*`.

use std::{
    collections::HashSet,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use flui_foundation::ClaimOutcome;
use flui_types::geometry::{Size, px};
use parking_lot::Mutex;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent as WinitWindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::WindowId as WinitWindowId,
};

use super::{
    SelfCloseRoute, WinitApp, WinitPlatform, WinitProxyTransport, WinitRunState,
    combine_shutdown_result,
};
use crate::{
    error::PlatformError,
    platforms::winit::control::control_lane,
    traits::{
        Platform, PlatformWindow, ProxySendError, WindowEvent, WindowId, WindowOptions,
        owner::ProxyTransport,
    },
};

fn options(title: impl Into<String>) -> WindowOptions {
    WindowOptions {
        title: title.into(),
        size: Size::new(px(320.0), px(240.0)),
        visible: false,
        ..WindowOptions::default()
    }
}

/// Polls (bounded, 2s) until the owner has reached `Running` — the point
/// at which `resumed()` has returned and the owner lane accepts
/// deferred, post-bootstrap requests. Real wall-clock, not simulated:
/// these lane tests drive an actual winit event loop.
fn wait_for_running(platform: &WinitPlatform) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !platform.with_state(|state| matches!(state.run_state, WinitRunState::Running { .. })) {
        assert!(
            Instant::now() < deadline,
            "owner never reached Running within 2s"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

/// Polls (bounded, 2s) until `window_id_map.len() == expected`. Used
/// both to detect that a deferred request actually landed (count goes
/// up) and that an unwind completed (count goes back down) — a
/// deterministic condition, not a blind sleep, even though the exact
/// wake-up latency is real wall-clock time.
fn wait_for_map_len(platform: &WinitPlatform, expected: usize, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let actual = platform.with_state(|state| state.window_id_map.len());
        if actual == expected {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}: window_id_map.len() = {actual}, expected {expected}"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

/// Builds a real `EventLoop` for a test running on an ordinary test
/// thread, not the process's actual main thread. winit refuses this by
/// default (a real cross-platform hazard for production code); Linux
/// and Windows offer an explicit opt-out for exactly this situation
/// (test harnesses, embedding). AppKit has no such opt-out — main-thread
/// affinity there is enforced by the OS, not just winit's own guard —
/// so these lane tests are `#[ignore]`d on macOS (see the callers).
///
/// **Only one `EventLoop` may ever exist per process** (a separate,
/// permanent winit limitation, not the main-thread one above): a second
/// `build()` call anywhere in the same process fails with
/// `RecreationAttempt`, even sequentially, even after the first loop
/// exited. Every test that calls this must run in its own process —
/// `cargo nextest run` gives each test one by default; plain
/// `cargo test` runs the whole binary in one process and WILL fail
/// whichever of these tests happens to run second. CI runs this crate's
/// suite with nextest under `xvfb-run` (see `crates/flui-platform/AGENTS.md`),
/// which is what gives these tests their X11 connection there.
#[cfg(any(target_os = "linux", target_os = "windows"))]
fn build_test_event_loop() -> EventLoop<()> {
    #[cfg(target_os = "linux")]
    {
        // `EventLoopBuilderExtWayland` adds a same-named, same-effect
        // `with_any_thread` to this same shared `EventLoopBuilder` (the
        // backend, X11 or Wayland, is a runtime choice, not this
        // builder's) -- importing both traits makes the call
        // ambiguous, so this sets the flag through X11's alone; it
        // applies regardless of which backend is actually selected at
        // runtime.
        use winit::platform::x11::EventLoopBuilderExtX11;
        let mut builder = EventLoop::builder();
        EventLoopBuilderExtX11::with_any_thread(&mut builder, true);
        builder
            .build()
            .expect("build a real winit event loop off the main thread")
    }
    #[cfg(target_os = "windows")]
    {
        use winit::platform::windows::EventLoopBuilderExtWindows;
        EventLoop::builder()
            .with_any_thread(true)
            .build()
            .expect("build a real winit event loop off the main thread")
    }
}

#[cfg(target_os = "macos")]
fn build_test_event_loop() -> EventLoop<()> {
    EventLoop::builder()
        .build()
        .expect("build a real winit event loop (main-thread only on macOS)")
}

/// Drives a REAL winit event loop (this sandbox has a live X11/Wayland
/// display) to exercise the actual owner-side path: `create_window_now`
/// needs a genuine `ActiveEventLoop`, so the drain/sweep/unwind sequence
/// (`process_control` -> `sweep_settled_replies` -> `unwind_orphan_window`)
/// cannot be exercised end-to-end with a hand-built `WinitApp` alone the
/// way the lower-level claim-slot tests in `control.rs` do.
///
/// `on_ready: None` — this test bypasses `OwnerPlatform` entirely and
/// drives the lane directly through `ControlSender`, mirroring how a
/// deferred post-bootstrap request actually reaches it (`WinitOwnerHooks`/
/// `WinitProxyTransport` are thin wrappers over exactly this).
#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "winit requires AppKit's event loop on the real main thread; \
              the test harness runs this on an ordinary test thread"
)]
fn winit_lane_dropped_after_delivery_unwinds_and_leaves_the_window_gone() {
    let platform = Arc::new(WinitPlatform::new());
    let event_loop = build_test_event_loop();
    let event_loop_proxy = event_loop.create_proxy();
    let wake_owner: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        let _ = event_loop_proxy.send_event(());
    });
    let (control, receiver) = control_lane(wake_owner);
    let owner_thread = thread::current().id();
    platform
        .install_control_lane(owner_thread, control.clone())
        .expect("first install succeeds");

    let platform_for_worker = Arc::clone(&platform);
    let control_for_worker = control.clone();
    let worker = thread::spawn(move || {
        // Catch a scenario panic (e.g. a bounded-poll timeout) so
        // `request_quit()` always runs below -- otherwise a failing
        // assertion leaves `run_app` parked forever waiting for a quit
        // that never comes, and the test hangs until nextest's
        // slow-test SIGKILL instead of failing fast.
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            wait_for_running(&platform_for_worker);

            let keys_before: HashSet<_> = platform_for_worker
                .with_state(|state| state.window_id_map.keys().copied().collect());

            let handle = control_for_worker
                .request_open_window(options("orphan-after-delivery"))
                .expect("lane accepts the request");

            // Bounded poll, not a blind sleep: the request must
            // actually be created and delivered (the map grows) before
            // we abandon it -- otherwise we would only be testing the
            // already-covered dropped-*before*-delivery skip path.
            wait_for_map_len(
                &platform_for_worker,
                keys_before.len() + 1,
                "window creation",
            );

            // Capture the orphan's platform `WindowId` (not just its
            // winit id) so we can seed and later check state keyed by
            // it: `cursor_positions`, and a synthetic drag-drop session
            // via `data_transfer.note_dropped_file`, proving
            // `unwind_orphan_window` cleans up both, not just the two
            // window-identity maps.
            let (orphan_id, data_transfer) = platform_for_worker.with_state(|state| {
                let new_winit_id = *state
                    .window_id_map
                    .keys()
                    .find(|id| !keys_before.contains(id))
                    .expect("a new winit id must have appeared");
                let orphan_id = state.window_id_map[&new_winit_id];
                state
                    .cursor_positions
                    .insert(orphan_id, winit::dpi::PhysicalPosition::new(1.0, 2.0));
                (orphan_id, Arc::clone(&state.data_transfer))
            });
            data_transfer.note_dropped_file(orphan_id, PathBuf::from("test.txt"), None);
            assert!(
                data_transfer.windows_awaiting_freeze().contains(&orphan_id),
                "the seeded drag session must be live before the unwind"
            );

            // Drop WITHOUT claiming: the claim-slot's
            // `Delivered -> Abandoned` transition (ADR-0039 §3) -- the
            // owner must unwind the window it already created for a
            // requester that vanished before reading it.
            drop(handle);

            wait_for_map_len(
                &platform_for_worker,
                keys_before.len(),
                "orphan-window unwind",
            );

            let (windows_len, cursor_positions_still_has_orphan) =
                platform_for_worker.with_state(|state| {
                    (
                        state.windows.len(),
                        state.cursor_positions.contains_key(&orphan_id),
                    )
                });
            assert_eq!(
                windows_len,
                keys_before.len(),
                "unwind_orphan_window must also remove the windows-map entry, \
                 not just window_id_map"
            );
            assert!(
                !cursor_positions_still_has_orphan,
                "unwind_orphan_window must also remove the orphan's \
                 cursor_positions entry"
            );
            assert!(
                !data_transfer.windows_awaiting_freeze().contains(&orphan_id),
                "unwind_orphan_window's data_transfer.forget_window call must \
                 retire the orphan's in-flight drag session"
            );
        }));

        control_for_worker.request_quit();
        if let Err(payload) = outcome {
            resume_unwind(payload);
        }
    });

    let mut app = WinitApp {
        platform: Arc::clone(&platform),
        on_ready: None,
        control: receiver,
        quit_notified: false,
        in_flight_replies: Vec::new(),
        bootstrap_error: None,
        self_close_deadline: None,
        self_close_route: SelfCloseRoute::default(),
    };
    event_loop
        .run_app(&mut app)
        .expect("event loop runs to completion");
    worker.join().expect("worker thread does not panic");
}

/// See the module doc on
/// [`winit_lane_dropped_after_delivery_unwinds_and_leaves_the_window_gone`]
/// for why this needs a real event loop. This test additionally proves
/// the winit `window_event` entry's existing unknown-id tolerance
/// (`tracing::warn!("Received event for unknown window"); return;`)
/// covers a straggler that names an id this lane itself just unwound —
/// not merely an id that was never registered at all.
#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "winit requires AppKit's event loop on the real main thread; \
              the test harness runs this on an ordinary test thread"
)]
fn winit_lane_post_unwind_straggler_event_is_tolerated() {
    /// Wraps the real `WinitApp` to inject one synthetic `window_event`
    /// call carrying a stale (already-unwound) `WinitWindowId`, reusing
    /// the live `&ActiveEventLoop` the wrapping callback already has —
    /// there is no other way to obtain one outside a running loop.
    struct StragglerHarness {
        inner: WinitApp,
        stale_id: Arc<Mutex<Option<WinitWindowId>>>,
        inject_now: Arc<AtomicBool>,
        injected: AtomicBool,
    }

    impl ApplicationHandler for StragglerHarness {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            self.inner.resumed(event_loop);
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            window_id: WinitWindowId,
            event: WinitWindowEvent,
        ) {
            self.inner.window_event(event_loop, window_id, event);
        }

        fn user_event(&mut self, event_loop: &ActiveEventLoop, (): ()) {
            self.inner.user_event(event_loop, ());
            if self.inject_now.load(Ordering::Acquire)
                && !self.injected.swap(true, Ordering::AcqRel)
                && let Some(stale_id) = *self.stale_id.lock()
            {
                // The straggler: an event for a window this app has
                // already fully unregistered. Must not panic -- if it
                // did, it would unwind through this real winit callback
                // and the test would fail loudly rather than silently.
                self.inner
                    .window_event(event_loop, stale_id, WinitWindowEvent::Focused(true));
            }
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            self.inner.about_to_wait(event_loop);
        }

        fn exiting(&mut self, event_loop: &ActiveEventLoop) {
            self.inner.exiting(event_loop);
        }
    }

    let platform = Arc::new(WinitPlatform::new());
    let event_loop = build_test_event_loop();
    let event_loop_proxy = event_loop.create_proxy();
    let event_loop_proxy_for_inject = event_loop.create_proxy();
    let wake_owner: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        let _ = event_loop_proxy.send_event(());
    });
    let (control, receiver) = control_lane(wake_owner);
    let owner_thread = thread::current().id();
    platform
        .install_control_lane(owner_thread, control.clone())
        .expect("first install succeeds");

    let stale_id: Arc<Mutex<Option<WinitWindowId>>> = Arc::new(Mutex::new(None));
    let inject_now = Arc::new(AtomicBool::new(false));

    let platform_for_worker = Arc::clone(&platform);
    let control_for_worker = control.clone();
    let stale_id_for_worker = Arc::clone(&stale_id);
    let inject_now_for_worker = Arc::clone(&inject_now);
    let worker = thread::spawn(move || {
        // See the sibling test's identical comment: `request_quit()`
        // must run even if a scenario assertion panics, or a failing
        // poll leaves `run_app` parked forever instead of failing fast.
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            wait_for_running(&platform_for_worker);

            let keys_before: HashSet<_> = platform_for_worker
                .with_state(|state| state.window_id_map.keys().copied().collect());

            let handle = control_for_worker
                .request_open_window(options("straggler-source"))
                .expect("lane accepts the request");

            wait_for_map_len(
                &platform_for_worker,
                keys_before.len() + 1,
                "window creation",
            );

            let new_id = platform_for_worker
                .with_state(|state| {
                    state
                        .window_id_map
                        .keys()
                        .copied()
                        .find(|id| !keys_before.contains(id))
                })
                .expect("a new winit id must have appeared");
            *stale_id_for_worker.lock() = Some(new_id);

            // Abandon without claiming -- same unwind path as the
            // sibling test, so by the time the map shrinks back,
            // `new_id` is a genuinely stale id: once valid, now
            // unregistered.
            drop(handle);
            wait_for_map_len(
                &platform_for_worker,
                keys_before.len(),
                "orphan-window unwind",
            );

            // Arm the injection and wake the loop once more so the
            // harness's `user_event` fires the straggler `window_event`.
            inject_now_for_worker.store(true, Ordering::Release);
            let _ = event_loop_proxy_for_inject.send_event(());

            // If the injected call had panicked inside that callback,
            // the real winit loop would have unwound through it and
            // this request would never complete -- proving "no panic"
            // by the process still being alive to finish a normal
            // round trip, rather than asserting a negative directly.
            let sentinel = control_for_worker
                .request_open_window(options("post-straggler-sentinel"))
                .expect("lane still accepts requests after the straggler event");
            match sentinel.wait() {
                ClaimOutcome::Delivered(result) => {
                    result.expect("a normal request still completes after the straggler event");
                }
                ClaimOutcome::AlreadyClaimed => {
                    panic!("this handle is never polled by another caller before wait")
                }
                ClaimOutcome::OwnerGone => panic!("the owner never disconnects in this test"),
            }
        }));

        control_for_worker.request_quit();
        if let Err(payload) = outcome {
            resume_unwind(payload);
        }
    });

    let mut app = StragglerHarness {
        inner: WinitApp {
            platform: Arc::clone(&platform),
            on_ready: None,
            control: receiver,
            quit_notified: false,
            in_flight_replies: Vec::new(),
            bootstrap_error: None,
            self_close_deadline: None,
            self_close_route: SelfCloseRoute::default(),
        },
        stale_id,
        inject_now,
        injected: AtomicBool::new(false),
    };
    event_loop
        .run_app(&mut app)
        .expect("event loop runs to completion");
    worker.join().expect("worker thread does not panic");
}

/// Close-path teardown-order regression (issue #713), plus the harness
/// self-close hook end-to-end. Two pins in one real-event-loop run:
///
/// 1. **Processing `CloseRequested` must drop the window's registered
///    callbacks inside the close arm itself**, while the native window
///    and the event loop are both still alive. The callbacks are the
///    embedder's owning references to per-window GPU state (the frame
///    callback owns the wgpu renderer); before the explicit
///    `callbacks().clear()` in that arm they survived inside any
///    still-held window `Arc` until the embedder's post-loop teardown,
///    which destroyed the swapchain after the `wl_surface` — the
///    post-quit Wayland SIGSEGV. The worker here deliberately keeps its
///    own window `Arc` alive until after the loop has fully exited, so
///    without the close arm's clear the drop flag is still unset at
///    assert time and this test goes red.
/// 2. **An armed self-close deadline runs that full close arm and exits
///    the loop on its own** — no `request_quit` on the success path.
///    This is the in-process trigger the Wayland live-smoke relies on
///    (`FLUI_SELF_CLOSE_AFTER_MS`), pinned here at the same
///    `window_event` entry a compositor close takes.
#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "winit requires AppKit's event loop on the real main thread; \
              the test harness runs this on an ordinary test thread"
)]
fn close_requested_drops_window_callbacks_and_self_close_exits_the_loop() {
    /// Delegating wrapper that arms the inner app's self-close deadline
    /// from the loop thread when the worker signals — once `run_app`
    /// owns the app there is no other way to reach its field.
    struct SelfCloseHarness {
        inner: WinitApp,
        arm_now: Arc<AtomicBool>,
    }

    impl ApplicationHandler for SelfCloseHarness {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            self.inner.resumed(event_loop);
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            window_id: WinitWindowId,
            event: WinitWindowEvent,
        ) {
            self.inner.window_event(event_loop, window_id, event);
        }

        fn user_event(&mut self, event_loop: &ActiveEventLoop, (): ()) {
            self.inner.user_event(event_loop, ());
            if self.arm_now.swap(false, Ordering::AcqRel) {
                self.inner.self_close_deadline = Some(Instant::now());
            }
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            self.inner.about_to_wait(event_loop);
        }

        fn exiting(&mut self, event_loop: &ActiveEventLoop) {
            self.inner.exiting(event_loop);
        }
    }

    /// Sets its flag when dropped — owned by the registered frame
    /// callback, standing in for the GPU renderer the production
    /// closure owns.
    struct DropFlag(Arc<AtomicBool>);
    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    let platform = Arc::new(WinitPlatform::new());
    let event_loop = build_test_event_loop();
    let event_loop_proxy = event_loop.create_proxy();
    let event_loop_proxy_for_arm = event_loop.create_proxy();
    let wake_owner: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        let _ = event_loop_proxy.send_event(());
    });
    let (control, receiver) = control_lane(wake_owner);
    let owner_thread = thread::current().id();
    platform
        .install_control_lane(owner_thread, control.clone())
        .expect("first install succeeds");

    let arm_now = Arc::new(AtomicBool::new(false));
    let callback_dropped = Arc::new(AtomicBool::new(false));
    // The worker parks its window Arc here; the main thread drops it
    // only AFTER the assertions, so the drop flag being set cannot be
    // this Arc's own (post-loop) drop doing the clearing.
    let kept_window: Arc<Mutex<Option<Arc<dyn PlatformWindow>>>> = Arc::new(Mutex::new(None));
    let global_close_events = record_global_close_events(&platform);

    let platform_for_worker = Arc::clone(&platform);
    let control_for_worker = control;
    let arm_now_for_worker = Arc::clone(&arm_now);
    let callback_dropped_for_worker = Arc::clone(&callback_dropped);
    let kept_window_for_worker = Arc::clone(&kept_window);
    let worker = thread::spawn(move || {
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            wait_for_running(&platform_for_worker);

            let handle = control_for_worker
                .request_open_window(options("self-close-teardown-order"))
                .expect("lane accepts the request");
            let window = match handle.wait() {
                ClaimOutcome::Delivered(result) => result.expect("window creation succeeds"),
                ClaimOutcome::AlreadyClaimed => {
                    panic!("this handle is never polled by another caller before wait")
                }
                ClaimOutcome::OwnerGone => panic!("the owner never disconnects in this test"),
            };

            let flag = DropFlag(Arc::clone(&callback_dropped_for_worker));
            window.on_request_frame(Box::new(move || {
                // Owns `flag` the way the production frame closure owns
                // the renderer.
                let _ = &flag;
            }));
            *kept_window_for_worker.lock() = Some(window);

            // Arm the self-close on the loop thread and wake it.
            arm_now_for_worker.store(true, Ordering::Release);
            let _ = event_loop_proxy_for_arm.send_event(());

            // The close arm must empty the window maps (bounded poll,
            // inside catch_unwind so the quit fallback below still
            // unparks the loop if it never happens).
            wait_for_map_len(&platform_for_worker, 0, "self-close window removal");
        }));

        // Success path: the self-close's own exit ends the loop; this
        // quit is then a no-op. Failure path: it unparks `run_app` so
        // the main thread reaches its assertions instead of hanging.
        control_for_worker.request_quit();
        if let Err(payload) = outcome {
            resume_unwind(payload);
        }
    });

    let mut app = SelfCloseHarness {
        inner: WinitApp {
            platform: Arc::clone(&platform),
            on_ready: None,
            control: receiver,
            quit_notified: false,
            in_flight_replies: Vec::new(),
            bootstrap_error: None,
            self_close_deadline: None,
            self_close_route: SelfCloseRoute::default(),
        },
        arm_now,
    };
    event_loop
        .run_app(&mut app)
        .expect("event loop runs to completion");
    worker.join().expect("worker thread does not panic");

    assert!(
        callback_dropped.load(Ordering::SeqCst),
        "CloseRequested must drop the window's registered callbacks inside the close \
         arm itself (while the window and event loop are alive) — a still-set frame \
         callback here means the renderer it owns in production would only die with \
         the embedder's last window Arc, after the event loop, destroying the wgpu \
         swapchain after its wl_surface (issue #713's post-quit SIGSEGV)"
    );
    assert!(
        platform.with_state(|state| state.windows.is_empty()),
        "the synthesized CloseRequested must remove the window from tracking"
    );
    let closed_id = kept_window.lock().as_ref().map(|window| window.id());
    assert_eq!(
        *global_close_events.lock(),
        vec![
            (
                GlobalCloseEvent::CloseRequested,
                closed_id.expect("window was opened")
            ),
            (
                GlobalCloseEvent::Closed,
                closed_id.expect("window was opened")
            ),
        ],
        "the user-initiated route reports CloseRequested (veto passed) and then Closed"
    );
    // Only now release the worker's window Arc — after the assertions
    // that prove the clearing already happened without it.
    drop(kept_window.lock().take());
}

/// Which of the two close-related global `WindowEvent`s a test saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GlobalCloseEvent {
    CloseRequested,
    Closed,
}

/// Installs a global window-event handler that records the close-related
/// events in order (other events are ignored) — the only way to pin
/// which route emits which event, since no production code installs
/// one.
fn record_global_close_events(
    platform: &WinitPlatform,
) -> Arc<Mutex<Vec<(GlobalCloseEvent, WindowId)>>> {
    let events: Arc<Mutex<Vec<(GlobalCloseEvent, WindowId)>>> = Arc::new(Mutex::new(Vec::new()));
    let events_for_handler = Arc::clone(&events);
    platform.on_window_event(Box::new(move |event| match event {
        WindowEvent::CloseRequested { window_id } => events_for_handler
            .lock()
            .push((GlobalCloseEvent::CloseRequested, window_id)),
        WindowEvent::Closed(window_id) => events_for_handler
            .lock()
            .push((GlobalCloseEvent::Closed, window_id)),
        _ => {}
    }));
    events
}

/// Programmatic close runs the owner's full close teardown and exits
/// the loop on its own (issue #919). Before the fix
/// `WinitWindow::close` hid the window and fired its callback but never
/// left the tracking map, so the exit policy — consulted only against
/// that map — never saw the last window go. Observed on the pre-fix
/// body: the worker's bounded wait for map removal times out
/// (`window_id_map.len() = 1, expected 0`), which aborts the worker
/// before the later assertions run; by construction `on_close` would
/// also have fired on the worker thread, and the loop only ended
/// because the worker's failure-path `request_quit` unparked it.
///
/// Pinned, in one real-event-loop run, from a cross-thread `close()`:
/// 1. the window leaves the tracking map and is reported not visible;
/// 2. `on_close` fires on the OWNER thread, not the calling worker —
///    an embedder's owner-affine close handling (`flui-app` rejects
///    realm dispatch off its owner thread) would otherwise silently
///    refuse it;
/// 3. the registered callbacks are cleared inside the teardown, while
///    the window and loop are alive (the #713 ordering the compositor
///    path already pins);
/// 4. the global handler sees `Closed` and NOT `CloseRequested` — a
///    programmatic close was never a request;
/// 5. the loop exits by the teardown's own exit-policy consult — the
///    worker waits for `exiting` BEFORE arming its fallback quit, so a
///    map removal without the exit is a timeout here, not a pass.
#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "winit requires AppKit's event loop on the real main thread; \
              the test harness runs this on an ordinary test thread"
)]
fn programmatic_close_runs_the_full_teardown_and_exits_the_loop() {
    /// Delegating wrapper that records when the loop reaches `exiting`,
    /// so the worker can tell "the teardown ended the loop" apart from
    /// "my own fallback quit did".
    struct ExitObserver {
        inner: WinitApp,
        exiting: Arc<AtomicBool>,
    }

    impl ApplicationHandler for ExitObserver {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            self.inner.resumed(event_loop);
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            window_id: WinitWindowId,
            event: WinitWindowEvent,
        ) {
            self.inner.window_event(event_loop, window_id, event);
        }

        fn user_event(&mut self, event_loop: &ActiveEventLoop, (): ()) {
            self.inner.user_event(event_loop, ());
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            self.inner.about_to_wait(event_loop);
        }

        fn exiting(&mut self, event_loop: &ActiveEventLoop) {
            self.exiting.store(true, Ordering::SeqCst);
            self.inner.exiting(event_loop);
        }
    }

    /// Sets its flag when dropped — owned by the registered frame
    /// callback, standing in for the GPU renderer the production
    /// closure owns.
    struct DropFlag(Arc<AtomicBool>);
    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    let platform = Arc::new(WinitPlatform::new());
    let event_loop = build_test_event_loop();
    let event_loop_proxy = event_loop.create_proxy();
    let wake_owner: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        let _ = event_loop_proxy.send_event(());
    });
    let (control, receiver) = control_lane(wake_owner);
    let owner_thread = thread::current().id();
    platform
        .install_control_lane(owner_thread, control.clone())
        .expect("first install succeeds");

    let exiting = Arc::new(AtomicBool::new(false));
    let callback_dropped = Arc::new(AtomicBool::new(false));
    let close_thread: Arc<Mutex<Option<thread::ThreadId>>> = Arc::new(Mutex::new(None));
    // Parked here so the drop flag cannot be this Arc's own drop.
    let kept_window: Arc<Mutex<Option<Arc<dyn PlatformWindow>>>> = Arc::new(Mutex::new(None));
    let global_close_events = record_global_close_events(&platform);

    let platform_for_worker = Arc::clone(&platform);
    let control_for_worker = control;
    let exiting_for_worker = Arc::clone(&exiting);
    let callback_dropped_for_worker = Arc::clone(&callback_dropped);
    let close_thread_for_worker = Arc::clone(&close_thread);
    let kept_window_for_worker = Arc::clone(&kept_window);
    let worker = thread::spawn(move || {
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            wait_for_running(&platform_for_worker);

            let handle = control_for_worker
                .request_open_window(options("programmatic-close"))
                .expect("lane accepts the request");
            let window = match handle.wait() {
                ClaimOutcome::Delivered(result) => result.expect("window creation succeeds"),
                ClaimOutcome::AlreadyClaimed => {
                    panic!("this handle is never polled by another caller before wait")
                }
                ClaimOutcome::OwnerGone => panic!("the owner never disconnects in this test"),
            };
            wait_for_map_len(&platform_for_worker, 1, "window registration");

            let flag = DropFlag(Arc::clone(&callback_dropped_for_worker));
            window.on_request_frame(Box::new(move || {
                let _ = &flag;
            }));
            let close_thread_for_callback = Arc::clone(&close_thread_for_worker);
            window.on_close(Box::new(move || {
                *close_thread_for_callback.lock() = Some(thread::current().id());
            }));
            *kept_window_for_worker.lock() = Some(Arc::clone(&window));

            // The programmatic close, from a thread that is NOT the
            // owner — the hardest case for the teardown's thread rules.
            window.close();

            wait_for_map_len(&platform_for_worker, 0, "programmatic close map removal");
            assert!(
                !window.is_visible(),
                "the teardown reports the closed window as not visible"
            );

            // The teardown's own exit-policy consult must end the loop;
            // bounded so a missing exit is a loud failure, not a hang.
            let deadline = Instant::now() + Duration::from_secs(2);
            while !exiting_for_worker.load(Ordering::SeqCst) {
                assert!(
                    Instant::now() < deadline,
                    "the window left the map but the loop did not exit: the close \
                     teardown must consult the exit policy once the map is empty"
                );
                thread::sleep(Duration::from_millis(5));
            }
        }));

        // Success path: the teardown's own exit already ended the loop
        // and this quit is a no-op. Failure path: it unparks `run_app`
        // so the main thread reaches its assertions instead of hanging.
        control_for_worker.request_quit();
        if let Err(payload) = outcome {
            resume_unwind(payload);
        }
    });

    let mut app = ExitObserver {
        inner: WinitApp {
            platform: Arc::clone(&platform),
            on_ready: None,
            control: receiver,
            quit_notified: false,
            in_flight_replies: Vec::new(),
            bootstrap_error: None,
            self_close_deadline: None,
            self_close_route: SelfCloseRoute::default(),
        },
        exiting,
    };
    event_loop
        .run_app(&mut app)
        .expect("event loop runs to completion");
    worker.join().expect("worker thread does not panic");

    assert_eq!(
        *close_thread.lock(),
        Some(owner_thread),
        "on_close must fire on the event-loop owner thread, never on the thread that \
         called close()"
    );
    assert!(
        callback_dropped.load(Ordering::SeqCst),
        "the programmatic close must clear the window's callbacks inside the teardown, \
         while the window and event loop are alive (issue #713's ordering)"
    );
    assert!(
        platform.with_state(|state| state.windows.is_empty()
            && state.window_id_map.is_empty()
            && state.cursor_positions.is_empty()
            && state.active_window.is_none()),
        "the programmatic close must leave no trace in the tracking maps"
    );
    let closed_id = kept_window.lock().as_ref().map(|window| window.id());
    assert_eq!(
        *global_close_events.lock(),
        vec![(
            GlobalCloseEvent::Closed,
            closed_id.expect("window was opened")
        )],
        "a programmatic close reports Closed only — it was never a request, so no \
         CloseRequested"
    );
    drop(kept_window.lock().take());
}

/// `resumed`'s stashed `bootstrap_error` must survive all the way to the
/// value `run_event_loop` returns. This drives a REAL winit event loop
/// through the identical sequence `run_event_loop` performs --
/// `event_loop.run_app(&mut app)`, take `bootstrap_error`,
/// `finish_shutdown`, then `combine_shutdown_result` (the exact
/// function `run_event_loop` calls) -- rather than calling
/// `run_event_loop` itself, which builds its own `EventLoop` internally
/// via a builder with no `with_any_thread` opt-out and so cannot run on
/// this non-main test thread (see `build_test_event_loop`'s doc).
#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "winit requires AppKit's event loop on the real main thread; \
              the test harness runs this on an ordinary test thread"
)]
fn on_ready_failure_propagates_through_the_combined_shutdown_result() {
    let platform = Arc::new(WinitPlatform::new());
    let event_loop = build_test_event_loop();
    let event_loop_proxy = event_loop.create_proxy();
    let wake_owner: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        let _ = event_loop_proxy.send_event(());
    });
    let (control, receiver) = control_lane(wake_owner);
    let owner_thread = thread::current().id();
    platform
        .install_control_lane(owner_thread, control)
        .expect("first install succeeds");

    let mut app = WinitApp {
        platform: Arc::clone(&platform),
        on_ready: Some(Box::new(|_owner| Err("simulated bootstrap failure".into()))),
        control: receiver,
        quit_notified: false,
        in_flight_replies: Vec::new(),
        bootstrap_error: None,
        self_close_deadline: None,
        self_close_route: SelfCloseRoute::default(),
    };

    let result = event_loop
        .run_app(&mut app)
        .map_err(|error| PlatformError::EventLoop {
            message: error.to_string(),
        });
    assert!(
        result.is_ok(),
        "on_ready's own Err requests a clean exit; the winit loop \
         itself does not error in this scenario, got: {result:?}"
    );

    let bootstrap_error = app.bootstrap_error.take();
    assert!(
        bootstrap_error.is_some(),
        "resumed() must stash on_ready's Err on WinitApp::bootstrap_error"
    );

    let combined = combine_shutdown_result(bootstrap_error, result);
    let error = combined.expect_err("the bootstrap failure must propagate as Err");
    assert!(
        format!("{error:?}").contains("simulated bootstrap failure"),
        "the root cause must be reachable from the combined error, got: {error:?}"
    );
}

/// Nothing previously asserted that winit's REAL `ProxyTransport`
/// reports `OwnerGone` -- a lane existed and its loop died -- rather
/// than `Unsupported` (reserved for backends with no lane at all,
/// `ClosedTransport`) once its loop has actually stopped.
#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "winit requires AppKit's event loop on the real main thread; \
              the test harness runs this on an ordinary test thread"
)]
fn winit_proxy_reports_owner_gone_not_unsupported_after_loop_stop() {
    let platform = Arc::new(WinitPlatform::new());
    let event_loop = build_test_event_loop();
    let event_loop_proxy = event_loop.create_proxy();
    let wake_owner: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        let _ = event_loop_proxy.send_event(());
    });
    let (control, receiver) = control_lane(wake_owner);
    let owner_thread = thread::current().id();
    platform
        .install_control_lane(owner_thread, control.clone())
        .expect("first install succeeds");

    let platform_for_worker = Arc::clone(&platform);
    let worker = thread::spawn(move || {
        wait_for_running(&platform_for_worker);
        control.request_quit();
    });

    let mut app = WinitApp {
        platform: Arc::clone(&platform),
        on_ready: None,
        control: receiver,
        quit_notified: false,
        in_flight_replies: Vec::new(),
        bootstrap_error: None,
        self_close_deadline: None,
        self_close_route: SelfCloseRoute::default(),
    };
    event_loop
        .run_app(&mut app)
        .expect("event loop runs to completion");
    worker.join().expect("worker thread does not panic");

    // The loop has now actually stopped (`finish_shutdown` ->
    // `close_owner_lane` -> `platform.mark_stopped()`, run_state ==
    // `Stopped`). A lane existed here and is now gone -- distinct from
    // a lane-less backend, which reports `Unsupported` instead.
    let transport = WinitProxyTransport {
        platform: Arc::clone(&platform),
        owner_thread,
    };
    let error = transport
        .open_window(options("post-shutdown"))
        .expect_err("no lane behind a stopped winit loop");
    assert!(
        matches!(error, ProxySendError::OwnerGone { .. }),
        "a stopped winit loop must report OwnerGone (a lane existed and \
         died), not Unsupported (no lane ever existed here) -- got {error:?}"
    );
}

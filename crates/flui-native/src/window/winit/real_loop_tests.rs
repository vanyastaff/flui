//! Tests that drive a REAL winit `EventLoop` with the winit host's windows:
//! a window the loop unwinds when its requester walks away, a window closed
//! from another thread, the AT-SPI bridge a Linux window hands out, and a
//! window on a Wayland compositor.
//!
//! Every test here runs `event_loop.run_app` on an ordinary test thread, over
//! the winit host's platform, with the loop's test support
//! (`owner::winit::testing`): the event loop a test thread may build, the
//! application a test runs it with and the waits for the loop.

use std::{
    collections::HashSet,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use flui_foundation::ClaimOutcome;
use flui_foundation::geometry::Size;
use parking_lot::Mutex;
use winit::{
    application::ApplicationHandler, event::WindowEvent as WinitWindowEvent,
    event_loop::ActiveEventLoop, window::WindowId as WinitWindowId,
};

use crate::owner::winit::WinitApp;
use crate::owner::winit::testing::{
    build_test_event_loop, install_control_lane, install_test_owner_signal, wait_for_map_len,
    wait_for_running,
};
use crate::{WinitPlatform, host_window::HostWindow, platform::Platform};
use flui_platform::{WindowEvent, WindowId, WindowOptions};

fn options(title: impl Into<String>) -> WindowOptions {
    WindowOptions::default()
        .with_title(title)
        .with_size(Size::new(320.0, 240.0))
        .with_visible(false)
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
    let host = Arc::new(WinitPlatform::new());
    let platform = host.shared_owner();
    let event_loop = build_test_event_loop();
    install_test_owner_signal(&platform, &event_loop);
    let (control, lane) = install_control_lane(&platform, &event_loop);

    let platform_for_worker = Arc::clone(&platform);
    let control_for_worker = control;
    let worker = thread::spawn(move || {
        // Catch a scenario panic (e.g. a bounded-poll timeout) so
        // `request_quit()` always runs below -- otherwise a failing
        // assertion leaves `run_app` parked forever waiting for a quit
        // that never comes, and the test hangs until nextest's
        // slow-test SIGKILL instead of failing fast.
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            wait_for_running(&platform_for_worker);

            let keys_before: HashSet<_> = platform_for_worker
                .with_state(|state| state.window_ids_for_test().keys().copied().collect());

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
                    .window_ids_for_test()
                    .keys()
                    .find(|id| !keys_before.contains(id))
                    .expect("a new winit id must have appeared");
                let orphan_id = state.window_ids_for_test()[&new_winit_id];
                state
                    .cursor_positions_for_test()
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
                        state.cursor_positions_for_test().contains_key(&orphan_id),
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

        control_for_worker.request_quit_for_test();
        if let Err(payload) = outcome {
            resume_unwind(payload);
        }
    });

    let mut app = WinitApp::for_test(Arc::clone(&platform), host, lane, None);
    event_loop
        .run_app(&mut app)
        .expect("event loop runs to completion");
    worker.join().expect("worker thread does not panic");
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
///    UI runtime dispatch off its owner thread) would otherwise silently
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
        /// Set just before the close. From then on every owner wake is
        /// preceded, in its loop turn, by a pointer move the translation
        /// holds; one of those turns runs the close, whichever wake carries
        /// it.
        move_before_close: Arc<AtomicBool>,
        /// Whether the input callback is registered.
        registered: bool,
        /// The moves held before a wake, one per wake.
        injected: Arc<AtomicUsize>,
        /// What the window's input callback saw: its moves, and each
        /// capability snapshot by whether it reports a mouse present.
        seen: Arc<Mutex<Vec<&'static str>>>,
    }

    impl ExitObserver {
        /// Register the window's input callback on the owner thread, as an
        /// application does, so its capability snapshot is delivered and
        /// input is not held behind it.
        fn register(&mut self, host: &dyn crate::owner::winit::LoopWindow) {
            if std::mem::replace(&mut self.registered, true) {
                return;
            }
            let seen = Arc::clone(&self.seen);
            flui_platform::PlatformWindow::on_input(
                host,
                Box::new(move |input| {
                    use flui_platform::input_capabilities::PointerKindSet;
                    match input {
                        flui_platform::PlatformInput::Pointer(
                            flui_platform::pointer::PointerEvent::Move(_),
                        ) => seen.lock().push("move"),
                        flui_platform::PlatformInput::Capabilities(capabilities) => {
                            seen.lock().push(
                                if capabilities.present().contains(PointerKindSet::MOUSE) {
                                    "mouse present"
                                } else {
                                    "no mouse"
                                },
                            );
                        }
                        _ => {}
                    }
                    flui_platform::DispatchEventResult::resolved(false, true)
                }),
            );
        }
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
            let (window, host) = self.inner.platform.with_state(|state| {
                (
                    state.window_ids_for_test().keys().next().copied(),
                    state.windows.values().next().cloned(),
                )
            });
            if self.move_before_close.load(Ordering::SeqCst)
                && let (Some(window), Some(host)) = (window, host)
            {
                self.register(host.as_ref());
                let moved = |x: f64| WinitWindowEvent::CursorMoved {
                    device_id: winit::event::DeviceId::dummy(),
                    position: winit::dpi::PhysicalPosition::new(x, 8.0),
                };
                if self.injected.load(Ordering::SeqCst) == 0 {
                    // The device a held move came from is announced while
                    // the move is held.
                    self.inner.window_event(event_loop, window, moved(10.0));
                    self.injected.fetch_add(1, Ordering::SeqCst);
                    self.inner.device_event(
                        event_loop,
                        winit::event::DeviceId::dummy(),
                        winit::event::DeviceEvent::Added,
                    );
                }
                // The move held when this wake runs.
                self.inner.window_event(event_loop, window, moved(12.0));
                self.injected.fetch_add(1, Ordering::SeqCst);
            }
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

    let host = Arc::new(WinitPlatform::new());
    let platform = host.shared_owner();
    let event_loop = build_test_event_loop();
    install_test_owner_signal(&platform, &event_loop);
    let (control, lane) = install_control_lane(&platform, &event_loop);
    let owner_thread = thread::current().id();

    let exiting = Arc::new(AtomicBool::new(false));
    let move_before_close = Arc::new(AtomicBool::new(false));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let injected = Arc::new(AtomicUsize::new(0));
    let callback_dropped = Arc::new(AtomicBool::new(false));
    let close_thread: Arc<Mutex<Option<thread::ThreadId>>> = Arc::new(Mutex::new(None));
    // Parked here so the drop flag cannot be this Arc's own drop.
    let kept_window: Arc<Mutex<Option<Arc<dyn HostWindow>>>> = Arc::new(Mutex::new(None));
    let global_close_events = record_global_close_events(&host);

    let platform_for_worker = Arc::clone(&platform);
    let control_for_worker = control;
    let exiting_for_worker = Arc::clone(&exiting);
    let callback_dropped_for_worker = Arc::clone(&callback_dropped);
    let close_thread_for_worker = Arc::clone(&close_thread);
    let kept_window_for_worker = Arc::clone(&kept_window);
    let move_before_close_for_worker = Arc::clone(&move_before_close);
    let injected_for_worker = Arc::clone(&injected);
    let poke = event_loop.create_proxy();
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
            window.on_request_frame(Box::new(move |_| {
                let _ = &flag;
            }));
            let close_thread_for_callback = Arc::clone(&close_thread_for_worker);
            window.on_close(Box::new(move || {
                *close_thread_for_callback.lock() = Some(thread::current().id());
            }));
            let _prev = kept_window_for_worker.lock().replace(Arc::clone(&window));

            // Every owner wake from here on holds a pointer move first; one
            // armed wake has run before the close is posted, so the wake
            // that runs the close is an armed one by construction.
            move_before_close_for_worker.store(true, Ordering::SeqCst);
            let _ = poke.send_event(());
            let deadline = Instant::now() + Duration::from_secs(2);
            while injected_for_worker.load(Ordering::SeqCst) == 0 {
                assert!(Instant::now() < deadline, "an armed wake ran");
                thread::sleep(Duration::from_millis(1));
            }

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
        control_for_worker.request_quit_for_test();
        if let Err(payload) = outcome {
            resume_unwind(payload);
        }
    });

    let mut app = ExitObserver {
        inner: WinitApp::for_test(Arc::clone(&platform), host, lane, None),
        exiting,
        move_before_close,
        registered: false,
        injected: Arc::clone(&injected),
        seen: Arc::clone(&seen),
    };
    event_loop
        .run_app(&mut app)
        .expect("event loop runs to completion");
    worker.join().expect("worker thread does not panic");

    let seen = seen.lock().clone();
    let injected = injected.load(Ordering::SeqCst);
    assert!(injected > 0, "a wake ran the close after it was armed");
    assert_eq!(
        seen.iter().filter(|seen| **seen == "move").count(),
        injected,
        "every pointer move held before a wake reaches the window, the one held \
         when that wake's control request closes the window included: {seen:?}"
    );
    let first_move = seen.iter().position(|seen| *seen == "move");
    assert_eq!(
        first_move.and_then(|at| seen[..at].last().copied()),
        Some("mouse present"),
        "the mouse a held move reveals is present before the move, even when its \
         device is announced while the move is held: {seen:?}"
    );

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
            && state.window_ids_for_test().is_empty()
            && state.cursor_positions_for_test().is_empty()
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
    let kept = kept_window.lock().take();
    drop(kept);
}

/// A winit window on Linux hands out its AT-SPI bridge with no feature beyond
/// the host's own (ADR-0151 §15): the composition root reads the bridge once,
/// and a window without one is invisible to Orca however its widgets are
/// labelled.
#[cfg(target_os = "linux")]
#[test]
fn a_linux_winit_window_hands_out_its_at_spi_bridge() {
    let host = Arc::new(WinitPlatform::new());
    let platform = host.shared_owner();
    let event_loop = build_test_event_loop();
    install_test_owner_signal(&platform, &event_loop);
    let (control, lane) = install_control_lane(&platform, &event_loop);

    let platform_for_worker = Arc::clone(&platform);
    let worker = thread::spawn(move || {
        // Caught, so the quit below always unparks `run_app`.
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            wait_for_running(&platform_for_worker);
            let handle = control
                .request_open_window(options("accessibility"))
                .expect("lane accepts the request");
            let ClaimOutcome::Delivered(window) = handle.wait() else {
                panic!("the owner delivers the window to its only claimant");
            };
            window
                .expect("window creation succeeds")
                .accessibility()
                .is_some()
        }));
        control.request_quit_for_test();
        outcome.unwrap_or_else(|payload| resume_unwind(payload))
    });

    let mut app = WinitApp::for_test(Arc::clone(&platform), host, lane, None);
    event_loop
        .run_app(&mut app)
        .expect("event loop runs to completion");
    let bridged = worker.join().expect("worker thread does not panic");
    assert!(
        bridged,
        "a winit window on Linux hands out its AT-SPI bridge"
    );
}

/// A Wayland window on a real compositor (skipped without `WAYLAND_DISPLAY`;
/// a headless weston covers the Wayland close path).
///
/// A compositor places windows and a client cannot hide, focus or place one,
/// so those commands are refused and the capabilities say so; what it does let
/// a client do (title, mode, size, constraints) is accepted.
#[cfg(target_os = "linux")]
mod wayland_window {
    use flui_foundation::geometry::Point;

    use super::*;
    use flui_platform::{WindowCapabilities, WindowCommand, WindowCommandError, WindowMode};
    use winit::event_loop::EventLoop;

    /// What a window answers for a command its capabilities leave out.
    const NOT_HERE: flui_platform::Unsupported =
        flui_platform::Unsupported::new(flui_platform::UnsupportedReason::NotOnThisPlatform);

    fn build_wayland_event_loop() -> EventLoop<()> {
        use winit::platform::wayland::EventLoopBuilderExtWayland;
        let mut builder = EventLoop::builder();
        EventLoopBuilderExtWayland::with_any_thread(&mut builder, true);
        EventLoopBuilderExtWayland::with_wayland(&mut builder);
        builder
            .build()
            .expect("build a winit event loop on Wayland")
    }

    #[test]
    fn wayland_window_refuses_what_a_compositor_does_not_let_a_client_do() {
        if std::env::var_os("WAYLAND_DISPLAY").is_none_or(|socket| socket.is_empty()) {
            eprintln!("skipped: no Wayland compositor (WAYLAND_DISPLAY is not set)");
            return;
        }
        let host = Arc::new(WinitPlatform::new());
        let platform = host.shared_owner();
        let event_loop = build_wayland_event_loop();
        install_test_owner_signal(&platform, &event_loop);
        let (control, lane) = install_control_lane(&platform, &event_loop);

        let platform_for_worker = Arc::clone(&platform);
        let host_for_worker = Arc::clone(&host);
        let control_for_worker = control;
        let worker = thread::spawn(move || {
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                wait_for_running(&platform_for_worker);
                let window = match control_for_worker
                    .request_open_window(
                        WindowOptions::default()
                            .with_title("wayland-window")
                            .with_size(Size::new(320.0, 240.0))
                            .with_position(Some(Point::new(40.0, 60.0))),
                    )
                    .expect("lane accepts the request")
                    .wait()
                {
                    ClaimOutcome::Delivered(result) => result.expect("window creation succeeds"),
                    _ => panic!("the window request is delivered"),
                };
                let capabilities = window.capabilities();
                assert!(!capabilities.sets_position && !capabilities.places_at_open);
                assert!(!capabilities.hides && !capabilities.shows && !capabilities.activates);
                assert!(!capabilities.observes_position && !capabilities.observes_minimized);
                assert!(
                    capabilities.sets_title && capabilities.sets_size && capabilities.maximizes
                );
                assert_eq!(host_for_worker.window_capabilities(), capabilities);
                assert_ne!(capabilities, WindowCapabilities::ALL);

                let state = window.state();
                assert_eq!(
                    state.outer.position, None,
                    "a client cannot read a position"
                );
                assert_eq!(state.mode, WindowMode::Normal);

                for refused in [
                    WindowCommand::SetPosition(Point::new(10.0, 10.0)),
                    WindowCommand::Hide,
                    WindowCommand::Show,
                    WindowCommand::Activate,
                ] {
                    assert_eq!(
                        window.apply(refused.clone()),
                        Err(WindowCommandError::Unsupported(NOT_HERE)),
                        "{refused:?}"
                    );
                }
                window
                    .apply(WindowCommand::SetTitle("renamed".into()))
                    .expect("a title is set");
                assert_eq!(window.state().title.as_deref(), Some("renamed"));
                window
                    .apply(WindowCommand::SetMode(WindowMode::Maximized))
                    .expect("a maximize is requested");
                // No first frame ever presents here, so there is nothing to
                // reveal and the call changes nothing.
                window.reveal_after_first_frame();
                assert!(window.state().visible);
            }));
            control_for_worker.request_quit_for_test();
            if let Err(payload) = outcome {
                resume_unwind(payload);
            }
        });

        let mut app = WinitApp::for_test(Arc::clone(&platform), host, lane, None);
        event_loop
            .run_app(&mut app)
            .expect("event loop runs to completion");
        worker.join().expect("worker thread does not panic");
    }
}

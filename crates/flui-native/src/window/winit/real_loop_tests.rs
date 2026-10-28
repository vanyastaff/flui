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
    rc::Rc,
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

use crate::WinitPlatform;
use crate::owner::winit::testing::{
    build_test_event_loop, install_control_lane, install_test_owner_signal, wait_for_map_len,
    wait_for_running,
};
use crate::owner::winit::{LoopWindow, WinitApp};
use flui_platform::WindowOptions;
use flui_testing::window_handler::RecordingWindowHandler;

/// A request for a window with `options`, whose handler, built on the owner,
/// records what the window delivers.
fn request(options: WindowOptions) -> crate::WindowRequest {
    crate::WindowRequest::new(
        options,
        || -> std::rc::Rc<dyn flui_platform::WindowHandler> { RecordingWindowHandler::new() },
    )
}

/// What a test does once the loop is ready, on its owner thread, as an
/// application handler that hears nothing else.
type ReadyStep = Box<dyn FnOnce(&crate::OwnerPlatform) -> Result<(), crate::BootstrapError>>;

struct OnReady(std::cell::RefCell<Option<ReadyStep>>);

impl OnReady {
    fn running(
        step: impl FnOnce(&crate::OwnerPlatform) -> Result<(), crate::BootstrapError> + 'static,
    ) -> std::rc::Rc<dyn crate::AppHandler> {
        std::rc::Rc::new(Self(std::cell::RefCell::new(Some(Box::new(step)))))
    }
}

impl crate::AppHandler for OnReady {
    fn ready(&self, owner: crate::OwnerPlatform) -> Result<(), crate::BootstrapError> {
        let step = self.0.borrow_mut().take();
        step.map_or(Ok(()), |step| step(&owner))
    }

    fn quit_requested(&self, _: &crate::OwnerPlatform) {}

    fn reopen(&self, _: &crate::OwnerPlatform) {}

    fn session_end(
        &self,
        _: &crate::OwnerPlatform,
        _: flui_platform::SessionEndPhase,
    ) -> flui_platform::SessionEndAnswer {
        flui_platform::SessionEndAnswer::Proceed
    }

    fn host_notice(&self, _: &crate::OwnerPlatform, _: flui_platform::SystemNotice) {}

    fn exit_policy(&self, _: &crate::OwnerPlatform) -> crate::ExitAnswer {
        crate::ExitAnswer::HostDecides
    }

    fn wake_deadline(&self, _: &crate::OwnerPlatform) -> Option<web_time::Instant> {
        None
    }
}

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
                .request_open_window(request(options("orphan-after-delivery")))
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

            let cursor_positions_still_has_orphan = platform_for_worker
                .with_state(|state| state.cursor_positions_for_test().contains_key(&orphan_id));
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
            keys_before.len()
        }));

        control_for_worker.request_quit_for_test();
        outcome.unwrap_or_else(|payload| resume_unwind(payload))
    });

    let mut app = WinitApp::for_test(Arc::clone(&platform), host, lane, None);
    event_loop
        .run_app(&mut app)
        .expect("event loop runs to completion");
    let windows_before = worker.join().expect("worker thread does not panic");
    // The windows map is the owner's: it is read here, on the thread that ran
    // the loop.
    assert_eq!(
        platform.with_state(|state| state.windows.keys().len()),
        windows_before,
        "unwind_orphan_window must also remove the windows-map entry, \
         not just window_id_map"
    );
}

/// Programmatic close runs the owner's full close teardown and exits
/// the loop on its own (issue #919). Before the fix
/// `WinitWindow::close` hid the window and fired its callback but never
/// left the tracking map, so the exit policy — consulted only against
/// that map — never saw the last window go. Observed on the pre-fix
/// body: the worker's bounded wait for map removal times out
/// (`window_id_map.len() = 1, expected 0`), which aborts the worker
/// before the later assertions run; by construction `WindowHandler::closed` would
/// also have fired on the worker thread, and the loop only ended
/// because the worker's failure-path `request_quit` unparked it.
///
/// Pinned, in one real-event-loop run, from a close requested on another thread:
/// 1. the window leaves the tracking map and is reported not visible;
/// 2. `WindowHandler::closed` fires on the OWNER thread, not the calling worker —
///    an embedder's owner-affine close handling (`flui-app` rejects
///    UI runtime dispatch off its owner thread) would otherwise silently
///    refuse it;
/// 3. the registered callbacks are cleared inside the teardown, while
///    the window and loop are alive (the #713 ordering the compositor
///    path already pins);
/// 4. the window's `WindowHandler::close_requested` is never asked — a programmatic
///    close was never a request;
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
        /// The moves held before a wake, one per wake.
        injected: Arc<AtomicUsize>,
        /// The window, parked on the owner before the close so the drop
        /// flag cannot be the window's own drop.
        kept: Option<Rc<dyn LoopWindow>>,
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
            let window = self
                .inner
                .platform
                .with_state(|state| state.window_ids_for_test().keys().next().copied());
            if self.move_before_close.load(Ordering::SeqCst)
                && let Some(window) = window
            {
                if self.kept.is_none() {
                    self.kept = self
                        .inner
                        .platform
                        .with_state(|state| state.windows.values().into_iter().next());
                }
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
    let close_asked = Arc::new(AtomicUsize::new(0));
    let close_asked_for_worker = Arc::clone(&close_asked);

    let platform_for_worker = Arc::clone(&platform);
    let control_for_worker = control;
    let exiting_for_worker = Arc::clone(&exiting);
    let callback_dropped_for_worker = Arc::clone(&callback_dropped);
    let close_thread_for_worker = Arc::clone(&close_thread);
    let move_before_close_for_worker = Arc::clone(&move_before_close);
    let injected_for_worker = Arc::clone(&injected);
    let seen_for_worker = Arc::clone(&seen);
    let poke = event_loop.create_proxy();
    let worker = thread::spawn(move || {
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            wait_for_running(&platform_for_worker);

            // The handler is built on the owner, from what this thread hands
            // the request.
            let flag = DropFlag(Arc::clone(&callback_dropped_for_worker));
            let close_thread_for_callback = Arc::clone(&close_thread_for_worker);
            let seen = Arc::clone(&seen_for_worker);
            let request = crate::WindowRequest::new(options("programmatic-close"), move || {
                let handler = RecordingWindowHandler::new();
                handler.react_to_frame(move |_| {
                    let _ = &flag;
                });
                handler.react_to_close(move || {
                    *close_thread_for_callback.lock() = Some(thread::current().id());
                });
                handler.react_to_close_request(move || {
                    close_asked_for_worker.fetch_add(1, Ordering::SeqCst);
                    flui_platform::CloseDecision::Close
                });
                handler.react_to_input(move |input| {
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
                });
                handler as std::rc::Rc<dyn flui_platform::WindowHandler>
            });
            let handle = control_for_worker
                .request_open_window(request)
                .expect("lane accepts the request");
            let window = match handle.wait() {
                ClaimOutcome::Delivered(result) => result.expect("window creation succeeds"),
                ClaimOutcome::AlreadyClaimed => {
                    panic!("this handle is never polled by another caller before wait")
                }
                ClaimOutcome::OwnerGone => panic!("the owner never disconnects in this test"),
            };
            wait_for_map_len(&platform_for_worker, 1, "window registration");

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
            control_for_worker.request_close_window(window.id());

            wait_for_map_len(&platform_for_worker, 0, "programmatic close map removal");

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
        injected: Arc::clone(&injected),
        kept: None,
    };
    event_loop
        .run_app(&mut app)
        .expect("event loop runs to completion");
    worker.join().expect("worker thread does not panic");
    let kept = app
        .kept
        .take()
        .expect("the owner parked the window before the close");
    assert!(
        !kept.state().visible,
        "the teardown reports the closed window as not visible"
    );

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
        "WindowHandler::closed must run on the event-loop owner thread, never on the thread that \
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
    assert_eq!(
        close_asked.load(Ordering::SeqCst),
        0,
        "a programmatic close was never a request, so the window is not asked \
         whether it should close"
    );
    drop(kept);
}

/// `a_surface_target_is_send_sync_and_unavailable_after_close`, on a real
/// winit loop: the target hands out the window's handle while it is open,
/// returns from `pre_present_notify` on another thread, and answers
/// `Unavailable` once the window closed.
#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "winit requires AppKit's event loop on the real main thread; \
              run on a Mac in a bundled process"
)]
fn a_winit_surface_target_answers_until_its_window_closes() {
    use raw_window_handle::{HandleError, HasWindowHandle};

    let host = Arc::new(WinitPlatform::new());
    let platform = host.shared_owner();
    let event_loop = build_test_event_loop();
    install_test_owner_signal(&platform, &event_loop);
    let (_control, lane) = install_control_lane(&platform, &event_loop);

    let kept: Rc<std::cell::RefCell<Option<Arc<dyn flui_platform::SurfaceTarget>>>> = Rc::default();
    let opened = Arc::new(AtomicBool::new(false));
    let (keep, open) = (Rc::clone(&kept), Arc::clone(&opened));
    let on_ready = OnReady::running(move |owner| {
        use flui_testing::window_commands::AdmitAndApply as _;

        let window = owner
            .open_window(options("surface target"), RecordingWindowHandler::new())?
            .try_ready()?;
        let target = window.surface_target();
        open.store(target.window_handle().is_ok(), Ordering::SeqCst);
        let presenting = Arc::clone(&target);
        thread::spawn(move || {
            presenting.pre_present_notify(flui_platform::SwapChainStatistics::None);
        })
        .join()
        .expect("pre_present_notify returns on another thread");
        *keep.borrow_mut() = Some(target);
        window
            .admit_and_apply(flui_platform::WindowCommand::Close)
            .expect("a winit window closes");
        owner.quit();
        Ok(())
    });

    let mut app = WinitApp::for_test(Arc::clone(&platform), host, lane, Some(on_ready));
    event_loop
        .run_app(&mut app)
        .expect("event loop runs to completion");
    assert!(
        opened.load(Ordering::SeqCst),
        "an open winit window's target hands out its handle"
    );
    let target = kept.borrow_mut().take().expect("the target was kept");
    assert!(
        matches!(target.window_handle(), Err(HandleError::Unavailable)),
        "a closed winit window's target hands out no handle"
    );
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
    let (_control, lane) = install_control_lane(&platform, &event_loop);

    // The window is the owner's: it is opened and read there, as the loop
    // becomes ready.
    let bridged = Arc::new(AtomicBool::new(false));
    let read = Arc::clone(&bridged);
    let on_ready = OnReady::running(move |owner| {
        let window = owner
            .open_window(options("accessibility"), RecordingWindowHandler::new())?
            .try_ready()?;
        read.store(window.accessibility().is_some(), Ordering::SeqCst);
        owner.quit();
        Ok(())
    });

    let mut app = WinitApp::for_test(Arc::clone(&platform), host, lane, Some(on_ready));
    event_loop
        .run_app(&mut app)
        .expect("event loop runs to completion");
    assert!(
        bridged.load(Ordering::SeqCst),
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
    use flui_platform::CommandRefusal;
    use flui_platform::{AdmissionError, WindowCapabilities, WindowCommand, WindowMode};
    use flui_testing::window_commands::AdmitAndApply as _;
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
        let (_control, lane) = install_control_lane(&platform, &event_loop);

        // The window is the owner's: it is opened and driven there, as the
        // loop becomes ready.
        let host_on_owner = Arc::clone(&host);
        let checked = Arc::new(AtomicBool::new(false));
        let done = Arc::clone(&checked);
        let on_ready = OnReady::running(move |owner| {
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                let window = owner
                    .open_window(
                        WindowOptions::default()
                            .with_title("wayland-window")
                            .with_size(Size::new(320.0, 240.0))
                            .with_position(Some(Point::new(40.0, 60.0))),
                        RecordingWindowHandler::new(),
                    )
                    .expect("the window opens")
                    .try_ready()
                    .expect("the window opens at once");
                let capabilities = window.state().capabilities;
                assert!(
                    !capabilities.sets_position.is_available()
                        && !capabilities.places_at_open.is_available()
                );
                assert!(
                    !capabilities.hides.is_available()
                        && !capabilities.shows.is_available()
                        && !capabilities.activates.is_available()
                );
                assert!(
                    !capabilities.observes_position.is_available()
                        && !capabilities.observes_minimized.is_available()
                );
                assert!(
                    capabilities.sets_title.is_available()
                        && capabilities.sets_size.is_available()
                        && capabilities.maximizes.is_available()
                );
                assert_eq!(
                    crate::platform::Platform::window_capabilities(&*host_on_owner),
                    capabilities
                );
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
                        window.admit_and_apply(refused.clone()),
                        Err(CommandRefusal::Admission(AdmissionError::Unsupported(
                            NOT_HERE
                        ))),
                        "{refused:?}"
                    );
                }
                window
                    .admit_and_apply(WindowCommand::SetTitle("renamed".into()))
                    .expect("a title is set");
                assert_eq!(window.state().title.as_deref(), Some("renamed"));
                window
                    .admit_and_apply(WindowCommand::SetMode(WindowMode::Maximized))
                    .expect("a maximize is requested");
                // No first frame ever presents here, so there is nothing to
                // reveal and the call changes nothing.
                window.reveal_after_first_frame();
                assert!(window.state().visible);
            }));
            // Caught, so the loop always quits; the failure resumes below.
            owner.quit();
            if let Err(payload) = outcome {
                resume_unwind(payload);
            }
            done.store(true, Ordering::SeqCst);
            Ok(())
        });

        let mut app = WinitApp::for_test(Arc::clone(&platform), host, lane, Some(on_ready));
        event_loop
            .run_app(&mut app)
            .expect("event loop runs to completion");
        assert!(
            checked.load(Ordering::SeqCst),
            "the window's checks ran on the owner"
        );
    }
}

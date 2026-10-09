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
use flui_foundation::geometry::Size;
use parking_lot::Mutex;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent as WinitWindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::WindowId as WinitWindowId,
};

use super::{SelfCloseRoute, WinitApp, WinitPlatform, WinitRunState};
use crate::{
    platforms::winit::control::control_lane,
    traits::{HostWindow, Platform, WindowEvent, WindowId, WindowOptions},
};

fn options(title: impl Into<String>) -> WindowOptions {
    WindowOptions {
        title: title.into(),
        size: Size::new(320.0, 240.0),
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
/// suite with nextest under `xvfb-run` (see `docs/testing.md`),
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

/// Native ingress needs the real event-loop owner; no test changes OS settings.
#[test]
#[cfg(target_os = "windows")]
#[allow(
    unsafe_code,
    reason = "pointer-free queued mouse messages target an owned hidden HWND"
)]
fn windows_winit_wheels_preserve_raw_units_and_observe_system_policy() {
    struct RetryObserver {
        inner: WinitApp,
        packets: Arc<Mutex<Vec<flui_platform_api::pointer::ScrollEvent>>>,
        frames: Arc<Mutex<usize>>,
        injected: bool,
    }

    impl ApplicationHandler for RetryObserver {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            self.inner.resumed(event_loop);
        }

        fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: winit::event::StartCause) {
            self.inner.new_events(event_loop, cause);
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
            // Drain the real ingress and initial frame before measuring an idle retry.
            if !self.injected && self.packets.lock().len() == 4 && *self.frames.lock() > 0 {
                self.injected = true;
                let source = self
                    .inner
                    .preference_source
                    .as_ref()
                    .expect("live native source");
                source.send_setting_change_for_test();
                assert!(
                    source.pending(),
                    "actual receiver invalidates the native sample"
                );
                let error = source.sample_with(|| {
                    Err(crate::PlatformError::Preferences {
                        message: "injected native read failure".into(),
                    })
                });
                assert!(error.is_err(), "the native read fails before publication");
                assert!(
                    source.retry_deadline().is_some(),
                    "failure retains paced delivery"
                );
                self.inner.refresh_preferences();
                let signal = self
                    .inner
                    .platform
                    .owner_signal
                    .lock()
                    .clone()
                    .expect("owner signal");
                let _ = signal.wake();
            }
            self.inner.about_to_wait(event_loop);
        }

        fn exiting(&mut self, event_loop: &ActiveEventLoop) {
            self.inner.exiting(event_loop);
        }
    }

    use flui_platform_api::{
        WheelStep,
        pointer::{PointerEvent, ScrollUnit},
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        UI::WindowsAndMessaging::{
            PostMessageW, SPI_GETWHEELSCROLLCHARS, SPI_GETWHEELSCROLLLINES,
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW, WM_MOUSEHWHEEL,
            WM_MOUSEMOVE, WM_MOUSEWHEEL,
        },
    };

    let platform = Arc::new(WinitPlatform::new());
    let event_loop = build_test_event_loop();
    let proxy = event_loop.create_proxy();
    let owner_proxy = proxy.clone();
    let signal = crate::shared::owner_signal::OwnerSignal::new(Arc::new(move || {
        owner_proxy
            .send_event(())
            .map_err(|error| crate::PlatformError::EventLoop {
                message: error.to_string(),
            })
    }));
    *platform.owner_signal.lock() = Some(signal);
    let (control, receiver) = control_lane(Arc::new(move || {
        let _ = proxy.send_event(());
    }));
    platform
        .install_control_lane(thread::current().id(), control.clone())
        .expect("install actual owner lane");
    let scrolls = Arc::new(Mutex::new(Vec::new()));
    let observations = Arc::new(Mutex::new(None));
    let frames = Arc::new(Mutex::new(0_usize));
    let deferred_frame = Arc::new(Mutex::new(None));
    let recovered = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    let timeout_done = done.clone();
    let timeout_control = control.clone();
    let timeout = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !timeout_done.load(Ordering::Acquire) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        timeout_control.request_quit();
    });
    let sink = scrolls.clone();
    let observed = observations.clone();
    let callback_done = done.clone();
    let quit = platform.clone();
    let wake_platform = platform.clone();
    let wake_frames = frames.clone();
    let wake_deferred = deferred_frame.clone();
    let wake_recovered = recovered.clone();
    let frame_sink = frames.clone();
    let mut app = WinitApp {
        platform: platform.clone(),
        preference_source: None,
        on_ready: Some(Box::new(move |owner| {
            let mut lines = 0_u32;
            let mut characters = 0_u32;
            // SAFETY: writable scalar outputs; these getters do not mutate OS policy.
            unsafe {
                SystemParametersInfoW(
                    SPI_GETWHEELSCROLLLINES,
                    0,
                    Some((&raw mut lines).cast()),
                    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
                )?;
                SystemParametersInfoW(
                    SPI_GETWHEELSCROLLCHARS,
                    0,
                    Some((&raw mut characters).cast()),
                    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
                )?;
            }
            let expected = if lines == u32::MAX {
                WheelStep::Page
            } else {
                WheelStep::Lines(lines)
            };
            let snapshot = owner.preferences()?;
            *observed.lock() = Some((snapshot.wheel().clone(), expected, characters));
            owner.on_wake(Box::new(move || match wake_platform.preferences() {
                Err(crate::PlatformError::PreferencesDeferred) => {
                    let _ = wake_deferred.lock().get_or_insert(*wake_frames.lock());
                }
                Ok(snapshot) => {
                    let before = *wake_deferred.lock();
                    if let Some(before) = before {
                        assert_eq!(
                            *wake_frames.lock(),
                            before,
                            "idle preference retry must not redraw"
                        );
                        assert_eq!(snapshot.wheel().vertical(), Some(expected));
                        assert_eq!(snapshot.wheel().horizontal_characters(), Some(characters));
                        wake_recovered.store(true, Ordering::Release);
                        callback_done.store(true, Ordering::Release);
                        quit.quit();
                    }
                }
                Err(error) => panic!("unexpected owner preference result: {error}"),
            }))?;
            let window = owner
                .open_window(options("winit-wheel-policy"))?
                .try_ready()?;
            let RawWindowHandle::Win32(handle) = window.window_handle()?.as_raw() else {
                panic!("Windows event loop must create a Win32 window");
            };
            let hwnd = HWND(handle.hwnd.get() as *mut std::ffi::c_void);
            window.on_request_frame(Box::new(move || *frame_sink.lock() += 1));
            window.request_redraw();
            window.on_input(Box::new(move |input| {
                if let flui_platform_api::PlatformInput::Pointer(PointerEvent::Scroll(scroll)) =
                    input
                {
                    sink.lock().push(scroll);
                }
                crate::DispatchEventResult::default()
            }));
            // SAFETY: this exact owned hidden HWND remains tracked until loop shutdown;
            // every queued message contains only by-value coordinates/wheel distance.
            unsafe {
                PostMessageW(Some(hwnd), WM_MOUSEMOVE, WPARAM(0), LPARAM(40 | (40 << 16)))?;
                for (message, distance) in [
                    (WM_MOUSEWHEEL, 120_i16),
                    (WM_MOUSEWHEEL, -60),
                    (WM_MOUSEHWHEEL, 120),
                    (WM_MOUSEHWHEEL, -60),
                ] {
                    PostMessageW(
                        Some(hwnd),
                        message,
                        WPARAM(usize::from(distance.cast_unsigned()) << 16),
                        LPARAM(0),
                    )?;
                }
            }
            Ok(())
        })),
        control: receiver,
        quit_notified: false,
        in_flight_replies: Vec::new(),
        bootstrap_error: None,
        self_close_deadline: None,
        self_close_route: SelfCloseRoute::default(),
    };
    let mut observer = RetryObserver {
        inner: app,
        packets: scrolls.clone(),
        frames,
        injected: false,
    };
    let result = event_loop.run_app(&mut observer);
    app = observer.inner;
    done.store(true, Ordering::Release);
    timeout.join().expect("bounded exit worker");
    result.expect("real event loop completes");
    assert!(
        app.bootstrap_error.is_none(),
        "public bootstrap failed: {:?}",
        app.bootstrap_error
    );
    assert!(
        recovered.load(Ordering::Acquire),
        "failed native read recovers through an idle owner wake"
    );
    assert!(
        platform.preferences().is_err(),
        "shutdown refuses the retired owner snapshot"
    );
    let log = scrolls.lock();
    assert_eq!(
        log.len(),
        4,
        "all actual native packets remain deliverable: {log:?}"
    );
    let mut failures = Vec::new();
    for (scroll, (x, y)) in log
        .iter()
        .zip([(0.0, -1.0), (0.0, 0.5), (1.0, 0.0), (-0.5, 0.0)])
    {
        if scroll.delta.unit() != ScrollUnit::Detents {
            failures.push(format!(
                "raw native wheel mislabeled {:?}",
                scroll.delta.unit()
            ));
        }
        assert_eq!((scroll.delta.x(), scroll.delta.y()), (x, y));
        assert_eq!(
            scroll.pointer, log[0].pointer,
            "one native mouse keeps identity across axes"
        );
    }
    let observed = observations.lock();
    let (wheel, vertical, horizontal) = observed.as_ref().expect("owner bootstrap observation");
    if wheel.vertical() != Some(*vertical) {
        failures.push(format!(
            "owner vertical policy {:?} instead of {vertical:?}",
            wheel.vertical()
        ));
    }
    if wheel.horizontal_characters() != Some(*horizontal) {
        failures.push(format!(
            "owner character policy {:?} instead of {horizontal}",
            wheel.horizontal_characters()
        ));
    }
    assert!(failures.is_empty(), "{}", failures.join("; "));
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
        #[cfg(windows)]
        preference_source: None,
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
    let kept_window: Arc<Mutex<Option<Arc<dyn HostWindow>>>> = Arc::new(Mutex::new(None));
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
            let _prev = kept_window_for_worker.lock().replace(Arc::clone(&window));

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
            #[cfg(windows)]
            preference_source: None,
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
    let kept = kept_window.lock().take();
    drop(kept);
}

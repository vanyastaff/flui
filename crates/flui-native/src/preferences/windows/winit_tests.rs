//! The Windows preference source on a REAL winit event loop: a native read
//! that fails is retried on an idle owner turn, as the winit host on Windows
//! runs it, while the wheel's raw units and the system's wheel policy reach
//! the application.
//!
//! Every test here runs `event_loop.run_app` on an ordinary test thread, over
//! the winit host's platform, with the loop's test support
//! (`owner::winit::testing`).

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use flui_foundation::geometry::Size;
use parking_lot::Mutex;
use winit::{
    application::ApplicationHandler, event::WindowEvent as WinitWindowEvent,
    event_loop::ActiveEventLoop, window::WindowId as WinitWindowId,
};

use crate::owner::winit::WinitApp;

use crate::owner::winit::testing::{build_test_event_loop, install_control_lane};
use crate::{WinitPlatform, host_window::HostWindow, platform::Platform};
use flui_platform::WindowOptions;

fn options(title: impl Into<String>) -> WindowOptions {
    WindowOptions::default()
        .with_title(title)
        .with_size(Size::new(320.0, 240.0))
        .with_visible(false)
}

/// A paint-capable native window outside the virtual desktop, without activation.
#[cfg(target_os = "windows")]
#[allow(
    unsafe_code,
    reason = "the test owns this live HWND and changes only its position and visibility"
)]
fn make_unactivated_offscreen(window: &dyn HostWindow) -> anyhow::Result<()> {
    use raw_window_handle::RawWindowHandle;
    use windows::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{
            GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
            SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos, ShowWindow,
        },
    };
    let RawWindowHandle::Win32(handle) = window.window_handle()?.as_raw() else {
        panic!("Windows event loop must create a Win32 window");
    };
    let hwnd = HWND(handle.hwnd.get() as *mut std::ffi::c_void);
    // SAFETY: position outside the virtual desktop before making this owned window
    // visible. Neither operation activates it; owner shutdown destroys it normally.
    unsafe {
        let x = GetSystemMetrics(SM_XVIRTUALSCREEN)
            .saturating_add(GetSystemMetrics(SM_CXVIRTUALSCREEN))
            .saturating_add(100);
        SetWindowPos(
            hwnd,
            None,
            x,
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            0,
            0,
            SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOZORDER,
        )?;
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    Ok(())
}

/// Native ingress needs the real event-loop owner; no test changes OS settings.
#[cfg(target_os = "windows")]
fn wake_after_native_drain(
    recovered: Arc<AtomicBool>,
    drained: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
    wake: impl FnOnce() + Send + 'static,
    quit: impl FnOnce() + Send + 'static,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut drain_at = None;
        let mut wake = Some(wake);
        while !done.load(Ordering::Acquire) && Instant::now() < deadline {
            if recovered.load(Ordering::Acquire) {
                let after =
                    *drain_at.get_or_insert_with(|| Instant::now() + Duration::from_millis(50));
                if Instant::now() >= after
                    && let Some(wake) = wake.take()
                {
                    drained.store(true, Ordering::Release);
                    wake();
                }
            }
            thread::sleep(Duration::from_millis(5));
        }
        quit();
    })
}

#[test]
#[cfg(target_os = "windows")]
#[allow(
    unsafe_code,
    reason = "pointer-free queued mouse messages target an owned hidden HWND"
)]
fn windows_winit_wheels_preserve_raw_units_and_observe_system_policy() {
    use crate::owner::windows::control::OwnerPreferences;

    struct RetryObserver {
        inner: WinitApp,
        packets: Arc<Mutex<Vec<flui_platform::pointer::ScrollEvent>>>,
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
                let source = crate::preferences::windows::kept(std::rc::Rc::clone(
                    self.inner
                        .preference_source_for_test()
                        .as_ref()
                        .expect("live native source"),
                ));
                let receiver = self
                    .inner
                    .broadcasts_for_test()
                    .expect("live broadcast receiver");
                receiver.send_with_for_test(
                    windows::Win32::UI::WindowsAndMessaging::WM_SETTINGCHANGE,
                    0,
                    0,
                );
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
                self.inner.refresh_preferences_for_test();
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

    use flui_platform::{
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

    let host = Arc::new(WinitPlatform::new());
    let platform = host.shared_owner();
    let event_loop = build_test_event_loop();
    let owner_proxy = event_loop.create_proxy();
    let signal = crate::owner::signal::OwnerSignal::new(Arc::new(move || {
        owner_proxy
            .send_event(())
            .map_err(|error| crate::PlatformError::EventLoop {
                message: error.to_string(),
            })
    }));
    *platform.owner_signal.lock() = Some(signal.clone());
    let (control, lane) = install_control_lane(&platform, &event_loop);
    let scrolls = Arc::new(Mutex::new(Vec::new()));
    let observations = Arc::new(Mutex::new(None));
    let frames = Arc::new(Mutex::new(0_usize));
    let deferred_frame = Arc::new(Mutex::new(None));
    let recovered = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    let drained = Arc::new(AtomicBool::new(false));
    let healthy_frame = Arc::new(Mutex::new(None));
    let timeout_control = control;
    let timeout = wake_after_native_drain(
        recovered.clone(),
        drained.clone(),
        done.clone(),
        move || {
            let _ = signal.wake();
        },
        move || timeout_control.request_quit_for_test(),
    );
    let sink = scrolls.clone();
    let observed = observations.clone();
    let callback_done = done.clone();
    let quit = host.clone();
    let wake_platform = host.clone();
    let wake_frames = frames.clone();
    let wake_deferred = deferred_frame;
    let wake_recovered = recovered.clone();
    let wake_drained = drained;
    let wake_healthy = healthy_frame.clone();
    let frame_healthy = healthy_frame;
    let frame_sink = frames.clone();
    let on_ready: crate::platform::PlatformReadyCallback = Box::new(move |owner| {
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
        let window = owner
            .open_window(options("winit-wheel-policy"))?
            .try_ready()?;
        let redraw_window = window.clone();
        owner.on_wake(Box::new(move || match wake_platform.preferences() {
            Err(crate::PlatformError::PreferencesDeferred) => {
                let _ = wake_deferred.lock().get_or_insert(*wake_frames.lock());
            }
            Ok(snapshot) => {
                let before = *wake_deferred.lock();
                if let Some(before) = before {
                    assert_eq!(snapshot.wheel().vertical(), Some(expected));
                    assert_eq!(snapshot.wheel().horizontal_characters(), Some(characters));
                    wake_recovered.store(true, Ordering::Release);
                    if wake_drained.load(Ordering::Acquire) {
                        assert_eq!(
                            *wake_frames.lock(),
                            before,
                            "idle preference retry must not redraw after native drain"
                        );
                        let should_request = {
                            let mut healthy = wake_healthy.lock();
                            if healthy.is_none() {
                                *healthy = Some(before + 1);
                                true
                            } else {
                                false
                            }
                        };
                        if should_request {
                            redraw_window.request_redraw();
                        }
                    }
                }
            }
            Err(error) => panic!("unexpected owner preference result: {error}"),
        }))?;
        let RawWindowHandle::Win32(handle) = window.window_handle()?.as_raw() else {
            panic!("Windows event loop must create a Win32 window");
        };
        let hwnd = HWND(handle.hwnd.get() as *mut std::ffi::c_void);
        window.on_request_frame(Box::new(move || {
            let count = {
                let mut frames = frame_sink.lock();
                *frames += 1;
                *frames
            };
            let expected = *frame_healthy.lock();
            if let Some(expected) = expected {
                assert_eq!(
                    count, expected,
                    "healthy native redraw is observable after retry"
                );
                callback_done.store(true, Ordering::Release);
                quit.quit();
            }
        }));
        make_unactivated_offscreen(window.as_ref())?;
        window.request_redraw();
        window.on_input(Box::new(move |input| {
            if let flui_platform::PlatformInput::Pointer(PointerEvent::Scroll(scroll)) = input {
                sink.lock().push(scroll);
            }
            flui_platform::DispatchEventResult::default()
        }));
        // SAFETY: this exact owned hidden HWND remains tracked until loop shutdown;
        // every queued message contains only by-value coordinates/wheel distance.
        unsafe {
            PostMessageW(
                Some(hwnd),
                WM_MOUSEMOVE,
                WPARAM(0),
                LPARAM(0x0028 | (0x0028 << 16)),
            )?;
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
    });
    let mut observer = RetryObserver {
        inner: WinitApp::for_test(platform, host.clone(), lane, Some(on_ready)),
        packets: scrolls.clone(),
        frames,
        injected: false,
    };
    let result = event_loop.run_app(&mut observer);
    let app = observer.inner;
    timeout.join().expect("bounded exit worker");
    result.expect("real event loop completes");
    assert!(
        app.bootstrap_error_for_test().is_none(),
        "public bootstrap failed: {:?}",
        app.bootstrap_error_for_test()
    );
    assert!(
        recovered.load(Ordering::Acquire),
        "failed native read recovers through an idle owner wake"
    );
    assert!(
        done.load(Ordering::Acquire),
        "drain and healthy redraw both complete"
    );
    assert!(
        host.preferences().is_err(),
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

#[test]
#[cfg(target_os = "windows")]
fn windows_winit_cold_preferences_recover_on_an_idle_owner_turn() {
    struct ColdRead {
        inner: WinitApp,
        frames: Arc<Mutex<usize>>,
        baseline: Arc<Mutex<Option<usize>>>,
    }

    impl ApplicationHandler for ColdRead {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let signal = self
                .inner
                .platform
                .owner_signal
                .lock()
                .clone()
                .expect("live owner signal");
            let source = crate::preferences::windows::PreferenceSource::new(&signal)
                .expect("native source construction");
            assert!(
                source
                    .sample_with(|| Err(crate::PlatformError::Preferences {
                        message: "injected first getter failure".into(),
                    }))
                    .is_err()
            );
            *self.inner.preference_source_for_test() = Some(std::rc::Rc::new(source));
            self.inner.resumed(event_loop);
            let _ = signal.wake();
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
            let frames = *self.frames.lock();
            if frames > 0 {
                let _ = self.baseline.lock().get_or_insert(frames);
            }
            self.inner.about_to_wait(event_loop);
        }

        fn exiting(&mut self, event_loop: &ActiveEventLoop) {
            self.inner.exiting(event_loop);
        }
    }

    let host = Arc::new(WinitPlatform::new());
    let platform = host.shared_owner();
    let event_loop = build_test_event_loop();
    let owner_proxy = event_loop.create_proxy();
    let signal = crate::owner::signal::OwnerSignal::new(Arc::new(move || {
        owner_proxy
            .send_event(())
            .map_err(|error| crate::PlatformError::EventLoop {
                message: error.to_string(),
            })
    }));
    *platform.owner_signal.lock() = Some(signal.clone());
    let (control, lane) = install_control_lane(&platform, &event_loop);
    let frames = Arc::new(Mutex::new(0_usize));
    let baseline = Arc::new(Mutex::new(None));
    let deferred = Arc::new(AtomicBool::new(false));
    let recovered = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    let drained = Arc::new(AtomicBool::new(false));
    let healthy_frame = Arc::new(Mutex::new(None));
    let timeout = wake_after_native_drain(
        recovered.clone(),
        drained.clone(),
        done.clone(),
        move || {
            let _ = signal.wake();
        },
        move || control.request_quit_for_test(),
    );
    let bootstrap_deferred = deferred.clone();
    let wake_platform = host.clone();
    let wake_frames = frames.clone();
    let wake_baseline = baseline.clone();
    let wake_recovered = recovered.clone();
    let wake_drained = drained;
    let wake_healthy = healthy_frame.clone();
    let frame_healthy = healthy_frame;
    let frame_done = done.clone();
    let frame_quit = host.clone();
    let frame_sink = frames.clone();
    let on_ready: crate::platform::PlatformReadyCallback = Box::new(move |owner| {
        assert!(matches!(
            owner.preferences(),
            Err(crate::PlatformError::PreferencesDeferred)
        ));
        bootstrap_deferred.store(true, Ordering::Release);
        let window = owner
            .open_window(options("cold-winit-preferences"))?
            .try_ready()?;
        let redraw_window = window.clone();
        owner.on_wake(Box::new(move || match wake_platform.preferences() {
            Err(crate::PlatformError::PreferencesDeferred) => {}
            Ok(snapshot) => {
                assert!(snapshot.wheel().vertical().is_some());
                assert!(snapshot.wheel().horizontal_characters().is_some());
                let before = (*wake_baseline.lock()).expect("initial frame completed");
                wake_recovered.store(true, Ordering::Release);
                if wake_drained.load(Ordering::Acquire) {
                    assert_eq!(
                        *wake_frames.lock(),
                        before,
                        "cold retry does not redraw after native drain"
                    );
                    let should_request = {
                        let mut healthy = wake_healthy.lock();
                        if healthy.is_none() {
                            *healthy = Some(before + 1);
                            true
                        } else {
                            false
                        }
                    };
                    if should_request {
                        redraw_window.request_redraw();
                    }
                }
            }
            Err(error) => panic!("unexpected recovery result: {error}"),
        }))?;
        window.on_request_frame(Box::new(move || {
            let count = {
                let mut frames = frame_sink.lock();
                *frames += 1;
                *frames
            };
            let expected = *frame_healthy.lock();
            if let Some(expected) = expected {
                assert_eq!(
                    count, expected,
                    "healthy native redraw remains observable after cold recovery"
                );
                frame_done.store(true, Ordering::Release);
                frame_quit.quit();
            }
        }));
        make_unactivated_offscreen(window.as_ref())?;
        window.request_redraw();
        Ok(())
    });
    let mut observer = ColdRead {
        inner: WinitApp::for_test(platform, host.clone(), lane, Some(on_ready)),
        frames,
        baseline,
    };
    let result = event_loop.run_app(&mut observer);
    timeout.join().expect("bounded exit worker");
    result.expect("real loop completes");
    assert!(observer.inner.bootstrap_error_for_test().is_none());
    assert!(
        deferred.load(Ordering::Acquire),
        "bootstrap remains usable without an observation"
    );
    assert!(
        recovered.load(Ordering::Acquire),
        "first failed read remains deliverable"
    );
    assert!(
        done.load(Ordering::Acquire),
        "cold drain and healthy redraw both complete"
    );
    assert!(host.preferences().is_err(), "retired owner refuses reads");
}

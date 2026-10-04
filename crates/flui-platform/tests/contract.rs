//! Contract test for the window lifecycle every `Platform` implementation
//! must honour: reported sizes are positive and consistent with the scale
//! factor, `visible: false` is respected, and windows are independent.

use flui_foundation::geometry::Size;
use flui_platform::{WindowOptions, current_platform};

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "requires an AppKit-run-loop-pumping test process (ADR-0039): the macOS platform surface asserts the owner main thread, a bare macOS cargo test cannot pump it and unbundled NSWindow construction aborts the process — these run headless on CI (FLUI_HEADLESS=1) and from an AppKit-pumping process only"
)]
fn test_window_lifecycle_contract() {
    #[cfg(target_os = "windows")]
    if native_windows::run_requested_child() {
        return;
    }
    #[cfg(target_os = "windows")]
    native_windows::run_children();
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();

    tracing::info!("Testing window lifecycle contract across platforms");

    let platform = current_platform().expect("Failed to create platform");
    let platform_name = platform.name();
    tracing::info!("Testing platform: {}", platform_name);

    // Contract 1: Platform must have a name
    assert!(
        !platform_name.is_empty(),
        "Platform name should not be empty"
    );

    // Contract 2: Platform must enumerate displays (even if empty for headless)
    let displays = platform.displays();
    tracing::info!("Platform has {} display(s)", displays.len());

    // Contract 3: Window creation should either succeed or fail gracefully
    let options = WindowOptions {
        title: "Contract Test Window".to_string(),
        size: Size::new(640.0, 480.0),
        visible: false,
        resizable: true,
        decorated: true,
        min_size: Some(Size::new(320.0, 240.0)),
        max_size: None,
        ..Default::default()
    };

    match platform.open_window(options) {
        Ok(window) => {
            tracing::info!("✓ Platform supports window creation");

            // Contract 4: Window must report valid sizes
            let logical_size = window.logical_size();
            let physical_size = window.physical_size();
            let scale_factor = window.scale_factor();

            tracing::info!(
                "Window sizes - Logical: {}x{}, Physical: {}x{}, Scale: {}",
                logical_size.width,
                logical_size.height,
                physical_size.width,
                physical_size.height,
                scale_factor
            );

            // Logical size must be positive
            assert!(
                logical_size.width > 0.0 && logical_size.height > 0.0,
                "Logical size must be positive"
            );

            // Physical size must be positive
            assert!(
                physical_size.width > 0 && physical_size.height > 0,
                "Physical size must be positive"
            );

            // Scale factor must be positive
            assert!(scale_factor > 0.0, "Scale factor must be positive");

            // Contract 5: Scale factor relationship (physical = logical * scale)
            let expected_physical_width = (logical_size.width * scale_factor) as i32;
            let expected_physical_height = (logical_size.height * scale_factor) as i32;

            let width_diff = (physical_size.width - expected_physical_width).abs();
            let height_diff = (physical_size.height - expected_physical_height).abs();

            tracing::info!(
                "Scale relationship - Expected physical: {}x{}, Actual: {}x{}, Diff: {}x{}",
                expected_physical_width,
                expected_physical_height,
                physical_size.width,
                physical_size.height,
                width_diff,
                height_diff
            );

            assert!(
                width_diff < 2 && height_diff < 2,
                "Physical size should equal logical size * scale (±2px tolerance)"
            );

            // Contract 6: Window visibility API
            let is_visible = window.is_visible();
            tracing::info!("Window visibility: {}", is_visible);
            assert!(
                !is_visible,
                "Window should not be visible with visible=false"
            );

            // Contract 7: Window focus API
            let is_focused = window.is_focused();
            tracing::info!("Window focus: {}", is_focused);

            // Contract 8: request_redraw() must not panic
            window.request_redraw();
            tracing::info!("✓ request_redraw() executed without panic");

            // Contract 9: Multiple window creation
            let options2 = WindowOptions {
                title: "Contract Test Window 2".to_string(),
                size: Size::new(400.0, 300.0),
                visible: false,
                ..Default::default()
            };

            match platform.open_window(options2) {
                Ok(window2) => {
                    tracing::info!("✓ Platform supports multiple concurrent windows");

                    let size2 = window2.logical_size();
                    assert!(
                        size2.width > 0.0 && size2.height > 0.0,
                        "Second window must have valid size"
                    );

                    // Windows must be independent (different sizes)
                    assert!(
                        (logical_size.width - size2.width).abs() > 1.0,
                        "Windows should have different sizes"
                    );
                }
                Err(e) => {
                    tracing::warn!("Platform doesn't support multiple windows: {}", e);
                }
            }

            tracing::info!(
                "✓ PASS: Window lifecycle contract validated for {}",
                platform_name
            );
        }
        Err(e) => {
            tracing::info!(
                "Platform {} doesn't support window creation: {}",
                platform_name,
                e
            );
            tracing::info!("⊘ SKIP: Platform doesn't support windows (expected for headless)");
        }
    }
}

#[cfg(target_os = "windows")]
mod native_windows {
    use flui_foundation::geometry::Size;
    use flui_platform::platforms::windows::WindowsWindow;
    use flui_platform::{HostWindow, Platform, WindowOptions, WindowsPlatform};
    use std::{
        process::{Command, Stdio},
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };
    use windows::Win32::{
        Foundation::{LPARAM, POINT, RECT, WPARAM},
        Graphics::Gdi::ClientToScreen,
        UI::WindowsAndMessaging::{
            GetClientRect, IsWindowVisible, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
            SendMessageW, SetWindowPos, WM_CLOSE,
        },
    };

    const CHILD: &str = "FLUI_NATIVE_WINDOW_CONTRACT_CHILD";
    const CASES: &[(&str, fn())] = &[
        (
            "resize_callback_preserves_large_native_dimensions",
            resize_callback_preserves_large_native_dimensions,
        ),
        (
            "move_callback_observes_full_native_coordinates",
            move_callback_observes_full_native_coordinates,
        ),
        (
            "native_close_retires_final_registry_owner",
            native_close_retires_final_registry_owner,
        ),
        (
            "closed_window_retires_tracking",
            closed_window_retires_tracking,
        ),
        (
            "close_callback_releases_external_owner",
            close_callback_releases_external_owner,
        ),
        (
            "resize_callback_observes_current_client_bounds",
            resize_callback_observes_current_client_bounds,
        ),
        (
            "hidden_popup_is_natively_hidden",
            hidden_popup_is_natively_hidden,
        ),
    ];

    pub(super) fn run_requested_child() -> bool {
        let Ok(name) = std::env::var(CHILD) else {
            return false;
        };
        let case = CASES
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .expect("known native window child")
            .1;
        case();
        true
    }

    pub(super) fn run_children() {
        let mut failures = Vec::new();
        for &(name, _) in CASES {
            let outcome = std::panic::catch_unwind(|| run_child(name));
            if outcome.is_err() {
                failures.push(name);
            }
        }
        assert!(
            failures.is_empty(),
            "native window cases failed: {failures:?}"
        );
    }

    fn run_child(name: &str) {
        let mut child = Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "contract::test_window_lifecycle_contract",
                "--nocapture",
            ])
            .env(CHILD, name)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn native window child");
        let start = Instant::now();
        while child.try_wait().expect("query child").is_none() {
            if start.elapsed() > Duration::from_secs(15) {
                child.kill().expect("kill stalled native child");
                let output = child.wait_with_output().expect("collect stalled child");
                panic!(
                    "{name} timed out: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let output = child.wait_with_output().expect("collect native child");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains("1 passed;"),
            "{name}: {:?}\n{stdout}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn open(platform: &WindowsPlatform, decorated: bool) -> Arc<dyn HostWindow> {
        platform
            .open_window(WindowOptions {
                visible: false,
                decorated,
                size: Size::new(320.0, 240.0),
                ..Default::default()
            })
            .expect("create actual hidden Win32 window")
    }

    #[expect(
        unsafe_code,
        reason = "actual owned Win32 close dispatch on its creating thread"
    )]
    fn native_close_retires_final_registry_owner() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, true);
        let weak = Arc::downgrade(&window);
        let hwnd = window
            .as_any()
            .downcast_ref::<WindowsWindow>()
            .expect("Win32 backend")
            .hwnd();
        drop(window);
        assert!(
            weak.strong_count() > 0,
            "platform owns the open native window"
        );
        // SAFETY: the platform registry still owns this HWND on its creating
        // thread. WM_CLOSE takes only by-value handle and zero message data;
        // it dereferences no caller-provided memory. Dispatch synchronously
        // retires the registry's final wrapper after clearing native userdata.
        unsafe {
            SendMessageW(hwnd, WM_CLOSE, Some(WPARAM(0)), Some(LPARAM(0)));
        }
        assert!(
            weak.upgrade().is_none(),
            "native close retained the final registry owner"
        );
        let next = open(&platform, true);
        let next_weak = Arc::downgrade(&next);
        next.close();
        drop(next);
        assert!(
            next_weak.upgrade().is_none(),
            "next close failed to retire tracking"
        );
    }

    #[expect(
        unsafe_code,
        reason = "actual owned Win32 movement and full client-origin queries"
    )]
    fn move_callback_observes_full_native_coordinates() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, true);
        let hwnd = window
            .as_any()
            .downcast_ref::<WindowsWindow>()
            .expect("Win32 backend")
            .hwnd();
        let weak = Arc::downgrade(&window);
        let observations = Arc::new(Mutex::new(Vec::new()));
        let callback_observations = Arc::clone(&observations);
        window.on_moved(Box::new(move || {
            let window = weak.upgrade().expect("live moved window");
            let hwnd = window
                .as_any()
                .downcast_ref::<WindowsWindow>()
                .expect("Win32 backend")
                .hwnd();
            let mut origin = POINT::default();
            // SAFETY: the upgraded wrapper owns this HWND on the callback's
            // creating thread; origin is a live correctly sized out-parameter.
            assert!(unsafe { ClientToScreen(hwnd, &raw mut origin) }.as_bool());
            let scale = window.scale_factor();
            let actual = flui_foundation::geometry::Point::new(
                origin.x as f64 / scale,
                origin.y as f64 / scale,
            );
            callback_observations
                .lock()
                .expect("move observations")
                .push((actual, window.bounds().origin));
        }));
        for (x, y) in [(40_000, 100), (200, 150)] {
            observations.lock().expect("move observations").clear();
            // SAFETY: this live wrapper owns the HWND on its creating thread;
            // SetWindowPos receives only by-value coordinates and flags.
            unsafe {
                SetWindowPos(
                    hwnd,
                    None,
                    x,
                    y,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                )
            }
            .expect("native move");
            let values = observations.lock().expect("move observations");
            let &(actual, observed) = values.last().expect("native move delivered");
            assert_eq!(actual, observed, "callback saw truncated or stale origin");
            assert_eq!(
                window.bounds().origin,
                actual,
                "getter changed after native move"
            );
        }
        window.close();
    }

    fn closed_window_retires_tracking() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        for _ in 0..2 {
            let window = open(&platform, true);
            let weak = Arc::downgrade(&window);
            window.close();
            drop(window);
            assert!(
                weak.upgrade().is_none(),
                "closed window retained by platform registry"
            );
        }
    }

    fn close_callback_releases_external_owner() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, true);
        let weak = Arc::downgrade(&window);
        let owner = Arc::new(Mutex::new(Some(Arc::clone(&window))));
        let callback_owner = Arc::clone(&owner);
        window.on_close(Box::new(move || {
            drop(callback_owner.lock().expect("owner slot").take());
        }));
        drop(window);
        weak.upgrade().expect("tracked window").close();
        assert!(owner.lock().expect("owner slot").is_none());
        assert!(
            weak.upgrade().is_none(),
            "callback-released window remains tracked"
        );
        let next = open(&platform, true);
        next.close();
    }

    #[expect(
        unsafe_code,
        reason = "actual owned hidden Win32 resize and client-dimension queries"
    )]
    fn resize_callback_preserves_large_native_dimensions() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, false);
        let hwnd = window
            .as_any()
            .downcast_ref::<WindowsWindow>()
            .expect("Win32 backend")
            .hwnd();
        let weak = Arc::downgrade(&window);
        let observations = Arc::new(Mutex::new(Vec::new()));
        let callback_observations = Arc::clone(&observations);
        window.on_resize(Box::new(move |size, scale| {
            let window = weak.upgrade().expect("live resize window");
            let hwnd = window
                .as_any()
                .downcast_ref::<WindowsWindow>()
                .expect("Win32 backend")
                .hwnd();
            let mut client = RECT::default();
            // SAFETY: the live wrapper owns this HWND on its creating thread;
            // client is a correctly sized, initialized native out-parameter.
            unsafe { GetClientRect(hwnd, &raw mut client) }.expect("native client size");
            let actual = Size::new(client.right - client.left, client.bottom - client.top);
            callback_observations
                .lock()
                .expect("resize observations")
                .push((
                    actual,
                    size,
                    scale,
                    window.logical_size(),
                    window.physical_size(),
                ));
        }));
        for (width, height) in [(40_000, 100), (100, 40_000), (320, 240)] {
            observations.lock().expect("resize observations").clear();
            // SAFETY: this wrapper owns the HWND on the creating thread. The
            // resize supplies only dimensions and flags, with no caller memory.
            unsafe {
                SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    width,
                    height,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                )
            }
            .expect("native hidden resize");
            let values = observations.lock().expect("resize observations");
            let &(actual, delivered, scale, observed, physical) =
                values.last().expect("native resize delivered");
            if width >= 32_768 {
                assert!(
                    actual.width >= 32_768,
                    "OS capped width; unsigned-dimension witness unavailable: {actual:?}"
                );
            }
            if height >= 32_768 {
                assert!(
                    actual.height >= 32_768,
                    "OS capped height; unsigned-dimension witness unavailable: {actual:?}"
                );
            }
            let logical = Size::new(actual.width as f64 / scale, actual.height as f64 / scale);
            assert_eq!(
                delivered, logical,
                "native dimensions corrupted in resize delivery"
            );
            assert_eq!(
                observed, logical,
                "callback getter corrupted native dimensions"
            );
            assert_eq!(physical, actual);
            assert_eq!(
                window.logical_size(),
                logical,
                "getter corrupted after resize"
            );
        }
        window.close();
    }

    fn resize_callback_observes_current_client_bounds() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, true);
        let weak = Arc::downgrade(&window);
        let observations = Arc::new(Mutex::new(Vec::new()));
        let callback_observations = Arc::clone(&observations);
        window.on_resize(Box::new(move |size, scale| {
            let window = weak.upgrade().expect("live resize window");
            callback_observations
                .lock()
                .expect("resize observations")
                .push((
                    size,
                    scale,
                    window.logical_size(),
                    window.physical_size(),
                    window.scale_factor(),
                ));
        }));
        for requested in [Size::new(500.0, 360.0), Size::new(440.0, 280.0)] {
            window.resize(requested);
            let values = observations.lock().expect("resize observations");
            let &(size, scale, observed, physical, observed_scale) =
                values.last().expect("native resize delivered");
            assert_eq!(size, observed, "getter stale inside resize callback");
            assert_eq!(scale, observed_scale);
            assert_eq!(
                window.logical_size(),
                size,
                "setter replaced native client bounds with requested outer size"
            );
            assert!((physical.width as f64 - size.width * scale).abs() <= 1.0);
            assert!((physical.height as f64 - size.height * scale).abs() <= 1.0);
        }
        window.close();
    }

    #[expect(
        unsafe_code,
        reason = "actual owned Win32 visibility query on its creating thread"
    )]
    fn hidden_popup_is_natively_hidden() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, false);
        let native = window
            .as_any()
            .downcast_ref::<WindowsWindow>()
            .expect("Win32 backend");
        // SAFETY: the live wrapper owns this HWND, queried on its creating thread.
        assert!(!unsafe { IsWindowVisible(native.hwnd()) }.as_bool());
        assert!(!window.is_visible());
        window.close();
    }
}

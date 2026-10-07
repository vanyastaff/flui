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
    use flui_platform::{
        DispatchEventResult, HostWindow, OwnerPlatform, Platform, WindowOptions, WindowsPlatform,
    };
    use std::{
        cell::RefCell,
        marker::PhantomData,
        process::{Command, Stdio},
        rc::Rc,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };
    use windows::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{ClientToScreen, UpdateWindow},
        System::Threading::GetCurrentThreadId,
        UI::Input::KeyboardAndMouse::{
            ACTIVATE_KEYBOARD_LAYOUT_FLAGS, ActivateKeyboardLayout, GetCapture, GetKeyState,
            GetKeyboardLayout, GetKeyboardState, HKL, KLF_ACTIVATE, LoadKeyboardLayoutW,
            ReleaseCapture, SetCapture, SetKeyboardState, ToUnicodeEx, VIRTUAL_KEY, VK_CONTROL,
            VK_LBUTTON, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_MBUTTON, VK_MENU, VK_RBUTTON,
            VK_RMENU, VK_SHIFT,
        },
        UI::WindowsAndMessaging::{
            CWPSTRUCT, CallNextHookEx, DispatchMessageW, GetClientRect, IsIconic, IsWindowVisible,
            MSG, PM_REMOVE, PeekMessageW, PostMessageW, SW_MINIMIZE, SWP_NOACTIVATE, SWP_NOMOVE,
            SWP_NOSIZE, SWP_NOZORDER, SendMessageW, SetWindowPos, SetWindowsHookExW, ShowWindow,
            TranslateMessage, UnhookWindowsHookEx, WH_CALLWNDPROC, WM_CHAR, WM_CLOSE,
            WM_ENTERMENULOOP, WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP,
            WM_MOUSEMOVE, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSCHAR, WM_SYSKEYDOWN, WM_SYSKEYUP,
        },
    };

    const CHILD: &str = "FLUI_NATIVE_WINDOW_CONTRACT_CHILD";
    const CASES: &[(&str, fn())] = &[
        (
            "deadline_rearms_independent_windows_without_input",
            deadline_rearms_independent_windows_without_input,
        ),
        (
            "deadline_hook_replacement_rearms_without_input",
            deadline_hook_replacement_rearms_without_input,
        ),
        (
            "deadline_query_can_close_an_independent_window",
            deadline_query_can_close_an_independent_window,
        ),
        (
            "deadline_none_allows_same_instant_readmission",
            deadline_none_allows_same_instant_readmission,
        ),
        (
            "deadline_reinstalled_by_its_frame_gets_another_frame",
            deadline_reinstalled_by_its_frame_gets_another_frame,
        ),
        (
            "deadline_queued_three_times_at_one_instant_gets_three_frames",
            deadline_queued_three_times_at_one_instant_gets_three_frames,
        ),
        (
            "deadline_no_frame_can_service_does_not_spin",
            deadline_no_frame_can_service_does_not_spin,
        ),
        (
            "deadline_window_without_frame_callback_does_not_spin",
            deadline_window_without_frame_callback_does_not_spin,
        ),
        (
            "deadline_hidden_window_without_frame_callback_does_not_spin",
            deadline_hidden_window_without_frame_callback_does_not_spin,
        ),
        (
            "deadline_query_unwind_retains_replaced_hostile_captures",
            deadline_query_unwind_retains_replaced_hostile_captures,
        ),
        (
            "deadline_reaches_minimized_window",
            deadline_reaches_minimized_window,
        ),
        (
            "deadline_reaches_hidden_window",
            deadline_reaches_hidden_window,
        ),
        (
            "unhandled_system_key_closes_window",
            unhandled_system_key_closes_window,
        ),
        (
            "allowed_system_key_closes_window",
            allowed_system_key_closes_window,
        ),
        (
            "prevented_system_key_preserves_window",
            prevented_system_key_preserves_window,
        ),
        (
            "system_key_close_honours_veto",
            system_key_close_honours_veto,
        ),
        (
            "system_key_callback_can_close_window",
            system_key_callback_can_close_window,
        ),
        (
            "deferred_system_key_preserves_window",
            deferred_system_key_preserves_window,
        ),
        ("alt_tap_keeps_next_character", alt_tap_keeps_next_character),
        ("f10_keeps_next_character", f10_keeps_next_character),
        ("dead_key_is_reported_as_dead", dead_key_is_reported_as_dead),
        (
            "system_dead_key_is_reported_as_dead",
            system_dead_key_is_reported_as_dead,
        ),
        (
            "altgr_dead_key_is_reported_as_dead",
            altgr_dead_key_is_reported_as_dead,
        ),
        (
            "dead_key_release_survives_a_layout_switch",
            dead_key_release_survives_a_layout_switch,
        ),
        (
            "dead_key_forgotten_when_focus_leaves",
            dead_key_forgotten_when_focus_leaves,
        ),
        (
            "dead_key_survives_a_transient_focus_loss",
            dead_key_survives_a_transient_focus_loss,
        ),
        (
            "consumed_alt_space_withdraws_its_system_char",
            consumed_alt_space_withdraws_its_system_char,
        ),
        (
            "unconsumed_alt_space_keeps_its_system_char",
            unconsumed_alt_space_keeps_its_system_char,
        ),
        (
            "pumping_consumer_of_alt_space_never_sees_its_system_char",
            pumping_consumer_of_alt_space_never_sees_its_system_char,
        ),
        (
            "pumping_handler_of_alt_space_keeps_its_system_char",
            pumping_handler_of_alt_space_keeps_its_system_char,
        ),
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
            "closed_window_traces_its_native_destruction",
            closed_window_traces_its_native_destruction,
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
        (
            "closing_the_last_window_ends_the_loop",
            closing_the_last_window_ends_the_loop,
        ),
        (
            "exit_policy_veto_holds_until_a_reevaluation_allows_exit",
            exit_policy_veto_holds_until_a_reevaluation_allows_exit,
        ),
        (
            "window_opened_by_the_exit_policy_keeps_the_loop",
            window_opened_by_the_exit_policy_keeps_the_loop,
        ),
        (
            "panicking_exit_policy_vetoes_and_stays_installed",
            panicking_exit_policy_vetoes_and_stays_installed,
        ),
        (
            "press_captures_the_mouse_until_the_last_release",
            press_captures_the_mouse_until_the_last_release,
        ),
        (
            "capture_taken_mid_press_cancels_the_sequence",
            capture_taken_mid_press_cancels_the_sequence,
        ),
        (
            "pointer_modifiers_are_the_message_state",
            pointer_modifiers_are_the_message_state,
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

    // DefWindowProc ignores Alt+F4 and leaves menu mode at once for a hidden
    // window, so system-key defaults are exercised on a shown one.
    fn open_shown(platform: &WindowsPlatform) -> Arc<dyn HostWindow> {
        platform
            .open_window(WindowOptions {
                visible: true,
                size: Size::new(160.0, 120.0),
                ..Default::default()
            })
            .expect("create actual shown Win32 window")
    }

    fn deadline_rearms_independent_windows_without_input() {
        run_deadline_windows("rearm");
    }

    fn deadline_hook_replacement_rearms_without_input() {
        run_deadline_windows("replace");
    }

    fn deadline_query_can_close_an_independent_window() {
        run_deadline_windows("close");
    }

    fn deadline_none_allows_same_instant_readmission() {
        use flui_platform::WindowOpen;

        let calls = Arc::new(AtomicUsize::new(0));
        let result = Arc::clone(&calls);
        Box::new(WindowsPlatform::new().expect("native Windows platform"))
            .run(Box::new(move |owner| {
                let WindowOpen::Ready(window) = owner
                    .open_window(WindowOptions {
                        visible: true,
                        size: Size::new(160.0, 120.0),
                        ..Default::default()
                    })
                    .expect("open readmission window")
                else {
                    panic!("Win32 on-ready window was deferred");
                };
                let initial = web_time::Instant::now() + Duration::from_millis(80);
                let state = Arc::new(Mutex::new((Some(initial), 0_usize)));
                let wake_state = Arc::clone(&state);
                owner
                    .on_wake(Box::new(move || {
                        let mut state = wake_state.lock().expect("readmission state");
                        if state.1 == 1 {
                            *state = (Some(initial), 2);
                        }
                    }))
                    .expect("native owner wake registration");
                let callback_state = Arc::clone(&state);
                let callback_proxy = owner.proxy();
                let weak = Arc::downgrade(&window);
                window.on_request_frame(Box::new(move || {
                    let count = {
                        let mut state = callback_state.lock().expect("readmission state");
                        if state.0.is_none_or(|due| web_time::Instant::now() < due) {
                            return;
                        }
                        let count = calls.fetch_add(1, Ordering::SeqCst) + 1;
                        *state = (None, if count == 1 { 1 } else { 3 });
                        count
                    };
                    if count == 2 {
                        weak.upgrade().expect("live readmission window").close();
                        callback_proxy
                            .request_quit()
                            .expect("quit after readmission");
                    }
                }));
                let proxy = owner.proxy();
                owner.shared().set_wake_deadline_hook(Box::new(move || {
                    let (deadline, stage) = *state.lock().expect("readmission state");
                    if stage == 1 {
                        // None must be observed by the loop before the next
                        // owner turn re-admits the same absolute instant.
                        proxy.wake().expect("readmit on the next owner turn");
                    }
                    deadline
                }));
                Ok(())
            }))
            .expect("native readmission loop returns normally");
        assert_eq!(
            result.load(Ordering::SeqCst),
            2,
            "None permits a fresh admission of the same instant"
        );
    }

    fn deadline_reinstalled_by_its_frame_gets_another_frame() {
        assert_eq!(
            same_instant_deadline_frames(2),
            2,
            "a frame that accepts new work at the instant it serviced gets a second frame"
        );
    }

    fn deadline_queued_three_times_at_one_instant_gets_three_frames() {
        // Every frame that services one obligation leaves the next due at
        // the same instant; each must get its own frame, not only the first
        // repeat of the pair.
        assert_eq!(
            same_instant_deadline_frames(3),
            3,
            "the third obligation due at one instant is stranded"
        );
    }

    fn deadline_no_frame_can_service_does_not_spin() {
        // No window is open, so a delivery runs no frame callback.
        let queries = unserviced_deadline_queries(None);
        assert!(
            queries < 16,
            "a deadline no frame can service was re-armed {queries} times"
        );
    }

    fn deadline_window_without_frame_callback_does_not_spin() {
        // A visible window paints, but no frame callback is registered.
        let queries = unserviced_deadline_queries(Some(true));
        assert!(
            queries < 16,
            "a deadline a callback-less window received was re-armed {queries} times"
        );
    }

    fn deadline_hidden_window_without_frame_callback_does_not_spin() {
        // A hidden window gets its frame request directly, and no frame
        // callback is registered to answer it.
        let queries = unserviced_deadline_queries(Some(false));
        assert!(
            queries < 16,
            "a deadline a callback-less hidden window received was re-armed {queries} times"
        );
    }

    // How many times the loop queries a hook that keeps answering one past
    // instant no frame callback services, over a fixed span a watchdog ends.
    // `window` opens one window with no frame callback, visible or hidden.
    // A delivery that ran no frame must stay delivered, so the loop parks.
    fn unserviced_deadline_queries(window: Option<bool>) -> usize {
        use flui_platform::WindowOpen;

        let queries = Arc::new(AtomicUsize::new(0));
        let result = Arc::clone(&queries);
        Box::new(WindowsPlatform::new().expect("native Windows platform"))
            .run(Box::new(move |owner| {
                if let Some(visible) = window {
                    let WindowOpen::Ready(_window) = owner
                        .open_window(WindowOptions {
                            visible,
                            size: Size::new(160.0, 120.0),
                            ..Default::default()
                        })
                        .expect("open callback-less window")
                    else {
                        panic!("Win32 on-ready window was deferred");
                    };
                }
                let watchdog = owner.proxy();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(300));
                    let _ = watchdog.request_quit();
                });
                let due = web_time::Instant::now() + Duration::from_millis(30);
                owner.shared().set_wake_deadline_hook(Box::new(move || {
                    queries.fetch_add(1, Ordering::SeqCst);
                    Some(due)
                }));
                Ok(())
            }))
            .expect("native unserviced-deadline loop returns normally");
        result.load(Ordering::SeqCst)
    }

    // Frame callbacks that run at or after one fixed instant the hook keeps
    // answering while `obligations` remain: each such frame services one,
    // leaving the next due at the same instant. A watchdog ends the loop.
    #[expect(
        unsafe_code,
        reason = "synchronous first paint of an owned Win32 window on its creating thread"
    )]
    fn same_instant_deadline_frames(obligations: usize) -> usize {
        use flui_platform::WindowOpen;

        let frames = Arc::new(AtomicUsize::new(0));
        let result = Arc::clone(&frames);
        Box::new(WindowsPlatform::new().expect("native Windows platform"))
            .run(Box::new(move |owner| {
                let WindowOpen::Ready(window) = owner
                    .open_window(WindowOptions {
                        visible: true,
                        size: Size::new(160.0, 120.0),
                        ..Default::default()
                    })
                    .expect("open same-instant window")
                else {
                    panic!("Win32 on-ready window was deferred");
                };
                let hwnd = window
                    .as_any()
                    .downcast_ref::<WindowsWindow>()
                    .expect("Win32 backend")
                    .hwnd();
                // SAFETY: the live wrapper owns this HWND on its creating
                // thread; the paint it sends runs synchronously here.
                let _ = unsafe { UpdateWindow(hwnd) };
                let watchdog = owner.proxy();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(600));
                    let _ = watchdog.request_quit();
                });
                let due = web_time::Instant::now() + Duration::from_millis(60);
                let pending = Arc::new(AtomicUsize::new(obligations));
                let callback_pending = Arc::clone(&pending);
                window.on_request_frame(Box::new(move || {
                    if callback_pending.load(Ordering::SeqCst) == 0
                        || web_time::Instant::now() < due
                    {
                        return;
                    }
                    callback_pending.fetch_sub(1, Ordering::SeqCst);
                    frames.fetch_add(1, Ordering::SeqCst);
                }));
                owner.shared().set_wake_deadline_hook(Box::new(move || {
                    (pending.load(Ordering::SeqCst) > 0).then_some(due)
                }));
                Ok(())
            }))
            .expect("native same-instant loop returns normally");
        result.load(Ordering::SeqCst)
    }

    fn deadline_query_unwind_retains_replaced_hostile_captures() {
        struct Bomb(Arc<AtomicUsize>);
        impl Drop for Bomb {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
                panic!("deadline capture drop failure");
            }
        }

        let drops = Arc::new(AtomicUsize::new(0));
        let captures = (Bomb(Arc::clone(&drops)), Bomb(Arc::clone(&drops)));
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Box::new(WindowsPlatform::new().expect("native Windows platform"))
                .run(Box::new(move |owner| {
                    let shared = owner.shared();
                    let replacement = shared.clone();
                    shared.set_wake_deadline_hook(Box::new(move || {
                        std::hint::black_box(&captures);
                        replacement.set_wake_deadline_hook(Box::new(|| None));
                        panic!("deadline query failure");
                    }));
                    Ok(())
                }))
                .expect("native query returns unless its hook panics");
        }))
        .expect_err("original hook panic propagates through the Rust run boundary");
        assert_eq!(
            failure.downcast_ref::<&'static str>().copied(),
            Some("deadline query failure")
        );
        assert_eq!(
            drops.load(Ordering::SeqCst),
            0,
            "outgoing captures retained before unwind retirement"
        );
        drop(failure);
        let platform = WindowsPlatform::new().expect("independent native owner after failed query");
        let window = open(&platform, true);
        let weak = Arc::downgrade(&window);
        window.close();
        drop(window);
        assert!(
            weak.upgrade().is_none(),
            "subsequent native owner still retires windows"
        );
    }

    fn deadline_reaches_minimized_window() {
        deadline_reaches_unpainted_window(true);
    }
    fn deadline_reaches_hidden_window() {
        deadline_reaches_unpainted_window(false);
    }

    // Whether the owned `hwnd` is in the state its row put it in: minimized,
    // or never shown.
    #[expect(unsafe_code, reason = "Win32 visibility queries of an owned HWND")]
    fn unpainted(hwnd: HWND, minimized: bool) -> bool {
        // SAFETY: queries of a live HWND the calling row owns, on its
        // creating thread.
        unsafe {
            if minimized {
                IsIconic(hwnd).as_bool()
            } else {
                !IsWindowVisible(hwnd).as_bool()
            }
        }
    }

    #[expect(
        unsafe_code,
        reason = "actual owned Win32 minimize on its creating thread"
    )]
    fn deadline_reaches_unpainted_window(minimized: bool) {
        use flui_platform::WindowOpen;

        // A minimized or hidden window paints nothing, so a due deadline must
        // reach its frame callback by another route or stay stranded until
        // input. A watchdog quits a stranded loop so the row fails instead of
        // hanging.
        let serviced = Arc::new(Mutex::new(None::<bool>));
        let result = Arc::clone(&serviced);
        Box::new(WindowsPlatform::new().expect("native Windows platform"))
            .run(Box::new(move |owner| {
                let WindowOpen::Ready(window) = owner
                    .open_window(WindowOptions {
                        visible: minimized,
                        size: Size::new(160.0, 120.0),
                        ..Default::default()
                    })
                    .expect("open unpainted window")
                else {
                    panic!("Win32 on-ready window was deferred");
                };
                let hwnd = window
                    .as_any()
                    .downcast_ref::<WindowsWindow>()
                    .expect("Win32 backend")
                    .hwnd();
                if minimized {
                    // SAFETY: the live wrapper owns this HWND, minimized on
                    // its creating thread.
                    let _ = unsafe { ShowWindow(hwnd, SW_MINIMIZE) };
                }
                assert!(unpainted(hwnd, minimized), "window minimized or hidden");
                let watchdog = owner.proxy();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(3));
                    let _ = watchdog.request_quit();
                });
                let due = web_time::Instant::now() + Duration::from_millis(60);
                let pending = Arc::new(Mutex::new(Some(due)));
                let callback_pending = Arc::clone(&pending);
                let proxy = owner.proxy();
                let raw_hwnd = hwnd.0 as isize;
                window.on_request_frame(Box::new(move || {
                    let mut pending = callback_pending.lock().expect("deadline state");
                    if pending.is_none_or(|due| web_time::Instant::now() < due) {
                        return;
                    }
                    *pending = None;
                    // The frame callback runs on the window's creating
                    // thread while its wrapper is alive.
                    let still = unpainted(HWND(raw_hwnd as *mut _), minimized);
                    *serviced.lock().expect("serviced state") = Some(still);
                    proxy.request_quit().expect("quit after unpainted deadline");
                }));
                owner.shared().set_wake_deadline_hook(Box::new(move || {
                    *pending.lock().expect("deadline state")
                }));
                Ok(())
            }))
            .expect("native unpainted deadline loop returns normally");
        assert_eq!(
            *result.lock().expect("serviced state"),
            Some(true),
            "a due deadline reaches a minimized or hidden window's frame callback"
        );
    }

    #[expect(
        unsafe_code,
        reason = "synchronous first paint of owned Win32 windows on their creating thread"
    )]
    fn run_deadline_windows(mode: &'static str) {
        use flui_platform::WindowOpen;

        // Every frame request before the first admitted deadline is a
        // delivery nothing asked for (a stale answer of a replaced hook).
        // Each window's first paint is forced before the hooks arm, so no
        // ordinary paint lands in that span.
        let early = Arc::new(AtomicUsize::new(0));
        let armed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let returned_early = Arc::clone(&early);
        let counts = Arc::new([AtomicUsize::new(0), AtomicUsize::new(0)]);
        let pending = Arc::new(AtomicUsize::new(if mode == "close" { 1 } else { 2 }));
        let deadlines = Arc::new(Mutex::new([None::<web_time::Instant>; 2]));
        let queried = Arc::new(AtomicUsize::new(0));
        let first_closed = Arc::new(AtomicUsize::new(0));
        let returned_counts = Arc::clone(&counts);
        let returned_closed = Arc::clone(&first_closed);
        Box::new(WindowsPlatform::new().expect("native Windows platform"))
            .run(Box::new(move |owner| {
                let shared = owner.shared();
                let proxy = owner.proxy();
                let mut first = None;
                let mut windows = Vec::new();
                for index in 0..2 {
                    let WindowOpen::Ready(window) = owner
                        .open_window(WindowOptions {
                            visible: true,
                            size: Size::new(160.0, 120.0),
                            ..Default::default()
                        })
                        .expect("open actual deadline window")
                    else {
                        panic!("Win32 on-ready window was deferred");
                    };
                    windows.push(Arc::clone(&window));
                    if index == 0 {
                        first = Some(Arc::downgrade(&window));
                        let closed = Arc::clone(&first_closed);
                        window.on_close(Box::new(move || {
                            closed.fetch_add(1, Ordering::SeqCst);
                        }));
                    }
                    let callback_deadlines = Arc::clone(&deadlines);
                    let callback_counts = Arc::clone(&counts);
                    let callback_pending = Arc::clone(&pending);
                    let callback_proxy = proxy.clone();
                    let callback_early = Arc::clone(&early);
                    let callback_armed = Arc::clone(&armed);
                    let weak = Arc::downgrade(&window);
                    window.on_request_frame(Box::new(move || {
                        let now = web_time::Instant::now();
                        let count = {
                            let mut deadlines = callback_deadlines.lock().expect("deadline state");
                            if deadlines[index].is_none_or(|due| now < due) {
                                if callback_armed.load(Ordering::SeqCst)
                                    && callback_counts[index].load(Ordering::SeqCst) == 0
                                    && deadlines[index].is_some()
                                {
                                    callback_early.fetch_add(1, Ordering::SeqCst);
                                }
                                return;
                            }
                            let count = callback_counts[index].fetch_add(1, Ordering::SeqCst) + 1;
                            deadlines[index] = (count < 2).then(|| now + Duration::from_millis(40));
                            count
                        };
                        if count == 2 {
                            weak.upgrade().expect("due live window").close();
                            if callback_pending.fetch_sub(1, Ordering::SeqCst) == 1 {
                                callback_proxy
                                    .request_quit()
                                    .expect("quit after both deadline rearms");
                            }
                        }
                    }));
                }
                let first = first.expect("first native window");
                for window in &windows {
                    let hwnd = window
                        .as_any()
                        .downcast_ref::<WindowsWindow>()
                        .expect("Win32 backend")
                        .hwnd();
                    // SAFETY: the live wrapper owns this HWND on its creating
                    // thread; the paint it sends runs synchronously here.
                    let _ = unsafe { UpdateWindow(hwnd) };
                }
                drop(windows);
                armed.store(true, Ordering::SeqCst);
                let initial = web_time::Instant::now() + Duration::from_millis(80);
                *deadlines.lock().expect("deadline state") = [Some(initial); 2];
                let hook_deadlines = Arc::clone(&deadlines);
                let replacement_shared = shared.clone();
                shared.set_wake_deadline_hook(Box::new(move || {
                    let first_query = queried.fetch_add(1, Ordering::SeqCst) == 0;
                    if mode == "replace" && first_query {
                        let replacement_deadlines = Arc::clone(&hook_deadlines);
                        replacement_shared.set_wake_deadline_hook(Box::new(move || {
                            replacement_deadlines
                                .lock()
                                .expect("replacement deadline state")
                                .iter()
                                .flatten()
                                .copied()
                                .min()
                        }));
                        // This answer belongs to the outgoing registration.
                        return Some(web_time::Instant::now());
                    }
                    if mode == "close" && first_query {
                        first.upgrade().expect("live first window").close();
                        hook_deadlines.lock().expect("deadline state")[0] = None;
                    }
                    hook_deadlines
                        .lock()
                        .expect("deadline state")
                        .iter()
                        .flatten()
                        .copied()
                        .min()
                }));
                Ok(())
            }))
            .expect("native deadline loop returns normally");
        assert_eq!(
            returned_early.load(Ordering::SeqCst),
            0,
            "{mode}: frame requests before any admitted deadline"
        );
        assert_eq!(
            returned_counts[0].load(Ordering::SeqCst),
            if mode == "close" { 0 } else { 2 },
            "first window's admitted deadlines"
        );
        assert_eq!(
            returned_counts[1].load(Ordering::SeqCst),
            2,
            "independent window rearms without input"
        );
        assert_eq!(
            returned_closed.load(Ordering::SeqCst),
            1,
            "first native window closed exactly once"
        );
    }

    fn unhandled_system_key_closes_window() {
        system_keyboard_close("unhandled");
    }
    fn allowed_system_key_closes_window() {
        system_keyboard_close("allowed");
    }
    fn prevented_system_key_preserves_window() {
        system_keyboard_close("prevented");
    }
    fn system_key_close_honours_veto() {
        system_keyboard_close("vetoed");
    }
    fn system_key_callback_can_close_window() {
        system_keyboard_close("closed_in_callback");
    }
    fn deferred_system_key_preserves_window() {
        system_keyboard_close("deferred");
    }

    // SetKeyboardState affects this owner thread, not physical keyboard input.
    // Keep the entire snapshot, including unrelated keys and toggle bits.
    struct ThreadKeyboardState {
        original: [u8; 256],
        armed: bool,
        owner_thread: PhantomData<Rc<()>>,
    }

    #[expect(unsafe_code, reason = "owner-thread keyboard-state fixture custody")]
    impl ThreadKeyboardState {
        fn with_alt_pressed() -> Self {
            Self::with_keys(&[(VK_MENU, true), (VK_LMENU, true)])
        }

        /// Sets each key down (`true`) or up in this thread's
        /// queue-synchronized state only; the physical (`GetAsyncKeyState`)
        /// state is untouched. Explicit ups keep a case independent of
        /// whatever the snapshot inherited from real input.
        fn with_keys(keys: &[(VIRTUAL_KEY, bool)]) -> Self {
            let mut original = [0; 256];
            // SAFETY: a complete writable snapshot on the current owner thread.
            unsafe { GetKeyboardState(&mut original) }.expect("snapshot thread keyboard state");
            let guard = Self {
                original,
                armed: true,
                owner_thread: PhantomData,
            };
            let mut state = original;
            for &(key, down) in keys {
                let slot = &mut state[usize::from(key.0)];
                *slot = if down { *slot | 0x80 } else { *slot & !0x80 };
            }
            // SAFETY: the guard already owns restoration for the current thread.
            unsafe { SetKeyboardState(&state) }.expect("set synthetic thread key state");
            for &(key, down) in keys {
                // SAFETY: reads only the current thread's logical key state.
                assert_eq!(unsafe { GetKeyState(i32::from(key.0)) } < 0, down);
            }
            guard
        }

        fn restore(&mut self) {
            // SAFETY: this non-Send guard restores the creating thread's snapshot.
            unsafe { SetKeyboardState(&self.original) }.expect("restore thread keyboard state");
            let mut restored = [0; 256];
            // SAFETY: complete writable snapshot on the same owner thread.
            unsafe { GetKeyboardState(&mut restored) }.expect("verify restored keyboard state");
            assert_eq!(
                restored, self.original,
                "complete keyboard state restoration"
            );
            self.armed = false;
        }
    }

    #[expect(
        unsafe_code,
        reason = "non-panicking owner-thread restoration on unwind"
    )]
    impl Drop for ThreadKeyboardState {
        fn drop(&mut self) {
            if self.armed {
                // SAFETY: the non-Send guard remains on its creating thread.
                // A failed OS restoration cannot replace an incoming failure.
                let _ = unsafe { SetKeyboardState(&self.original) };
            }
        }
    }

    #[expect(
        unsafe_code,
        reason = "actual owned Win32 system-key dispatch and message pumping"
    )]
    fn system_keyboard_close(mode: &'static str) {
        let platform = Arc::new(WindowsPlatform::new().expect("native Windows platform"));
        let window = open_shown(&platform);
        let original_weak = Arc::downgrade(&window);
        let replacement = Arc::new(Mutex::new(None::<Arc<dyn HostWindow>>));
        let replacement_closes = Arc::new(AtomicUsize::new(0));
        let hwnd = window
            .as_any()
            .downcast_ref::<WindowsWindow>()
            .expect("Win32 backend")
            .hwnd();
        let close_count = Arc::new(AtomicUsize::new(0));
        let close_observations = Arc::clone(&close_count);
        window.on_close(Box::new(move || {
            close_observations.fetch_add(1, Ordering::SeqCst);
        }));
        let input_count = Arc::new(AtomicUsize::new(0));
        let veto_count = Arc::new(AtomicUsize::new(0));
        if mode == "vetoed" {
            let veto_observations = Arc::clone(&veto_count);
            window.on_should_close(Box::new(move || {
                veto_observations.fetch_add(1, Ordering::SeqCst);
                false
            }));
        }
        if mode != "unhandled" {
            let input_observations = Arc::clone(&input_count);
            let weak = Arc::downgrade(&window);
            let raw_hwnd = hwnd.0 as isize;
            let callback_platform = Arc::downgrade(&platform);
            let callback_replacement = Arc::clone(&replacement);
            let callback_replacement_closes = Arc::clone(&replacement_closes);
            window.on_input(Box::new(move |event| {
                assert_eq!(
                    event.as_keyboard().expect("native key event").key,
                    keyboard_types::Key::Named(keyboard_types::NamedKey::F4)
                );
                let first = input_observations.fetch_add(1, Ordering::SeqCst) == 0;
                if mode == "deferred" && first {
                    // SAFETY: the upgraded callback's window owns this
                    // same native handle on the current thread. This nested
                    // delivery returns DEFERRED before its callback can run.
                    unsafe {
                        SendMessageW(
                            HWND(raw_hwnd as *mut _),
                            WM_SYSKEYDOWN,
                            Some(WPARAM(0x73)),
                            Some(LPARAM(1 | (0x3e << 16) | (1 << 29))),
                        );
                    }
                }
                if mode == "closed_in_callback" {
                    weak.upgrade().expect("live input window").close();
                    let replacement =
                        open(&callback_platform.upgrade().expect("live platform"), true);
                    let closes = Arc::clone(&callback_replacement_closes);
                    replacement.on_close(Box::new(move || {
                        closes.fetch_add(1, Ordering::SeqCst);
                    }));
                    *callback_replacement.lock().expect("replacement storage") = Some(replacement);
                }
                DispatchEventResult::resolved(
                    true,
                    mode == "prevented" || (mode == "deferred" && first),
                )
            }));
        }
        // Direct system-key dispatch also supplies the owner thread's logical
        // Alt state. This is a synthetic default-processing fixture, not a
        // hardware-input or GetAsyncKeyState modifier-fidelity claim.
        let mut keyboard_state = ThreadKeyboardState::with_alt_pressed();
        // SAFETY: this wrapper owns HWND on its creating thread. The message
        // carries only integer F4, scan-code and documented Alt-context data.
        unsafe {
            SendMessageW(
                hwnd,
                WM_SYSKEYDOWN,
                Some(WPARAM(0x73)),
                Some(LPARAM(1 | (0x3e << 16) | (1 << 29))),
            );
        }
        keyboard_state.restore();
        // Drain ordinary messages for this owned HWND, including any close
        // chain. Never fabricate a system command or a close notification.
        let mut drained = false;
        for _ in 0..64 {
            let mut message = MSG::default();
            // SAFETY: only the fixture's original owner-thread HWND is selected.
            if !unsafe { PeekMessageW(&raw mut message, Some(hwnd), 0, 0, PM_REMOVE) }.as_bool() {
                drained = true;
                break;
            }
            // SAFETY: dispatch the message returned by this thread's queue.
            unsafe { DispatchMessageW(&raw const message) };
        }
        assert!(drained, "{mode}: owned message drain exceeded its bound");
        assert_eq!(
            input_count.load(Ordering::SeqCst),
            if mode == "deferred" {
                2
            } else {
                usize::from(mode != "unhandled")
            },
            "{mode}: keyboard delivery"
        );
        assert_eq!(
            replacement_closes.load(Ordering::SeqCst),
            0,
            "{mode}: native default affected replacement"
        );
        let replacement = replacement.lock().expect("replacement storage").take();
        if let Some(replacement) = replacement {
            assert!(
                flui_platform::traits::PlatformWindow::window_handle(replacement.as_ref()).is_ok(),
                "replacement remains natively live"
            );
            replacement.close();
        }
        assert_eq!(
            close_count.load(Ordering::SeqCst),
            usize::from(mode != "prevented" && mode != "vetoed" && mode != "deferred"),
            "{mode}: native default close"
        );
        assert_eq!(
            veto_count.load(Ordering::SeqCst),
            usize::from(mode == "vetoed"),
            "{mode}: close veto delivery"
        );
        if mode == "vetoed" {
            window.on_should_close(Box::new(|| true));
        }
        if mode == "vetoed" || mode == "prevented" || mode == "deferred" {
            window.close();
        }
        drop(window);
        assert!(
            original_weak.upgrade().is_none(),
            "{mode}: closed window retained by registry"
        );
        let next = open(&platform, true);
        let next_weak = Arc::downgrade(&next);
        next.close();
        drop(next);
        assert!(
            next_weak.upgrade().is_none(),
            "{mode}: next close failed to retire native tracking"
        );
    }

    fn alt_tap_keeps_next_character() {
        menu_key_keeps_next_character("alt");
    }
    fn f10_keeps_next_character() {
        menu_key_keeps_next_character("f10");
    }

    // A dead key (an accent on an international layout) arrives as
    // `NamedKey::Dead`, not as its unshifted character, so a shortcut bound
    // to that character does not fire mid-composition.
    fn dead_key_is_reported_as_dead() {
        dead_key_reports_dead(DeadKeyCase::UsInternationalAcute);
    }

    // With Alt held the same key arrives as a system keydown and keyup.
    fn system_dead_key_is_reported_as_dead() {
        dead_key_reports_dead(DeadKeyCase::SystemAcute);
    }

    // French AltGr+2 is a dead tilde, while unshifted 2 types `é`: the dead
    // mapping exists only on the AltGr layer.
    fn altgr_dead_key_is_reported_as_dead() {
        dead_key_reports_dead(DeadKeyCase::FrenchAltGrTilde);
    }

    // The layout switches to plain US while the dead key is held; its release
    // keeps the identity its press reported.
    fn dead_key_release_survives_a_layout_switch() {
        dead_key_reports_dead(DeadKeyCase::LayoutSwitchWhileHeld);
    }

    // Focus leaves while the dead key is held, so its release goes elsewhere;
    // the same physical key pressed later as a plain key reports the plain
    // character on both phases.
    fn dead_key_forgotten_when_focus_leaves() {
        dead_key_reports_dead(DeadKeyCase::FocusLostWhileHeld);
    }

    // Focus leaves and comes back while the dead key is held: its release
    // arrives here and still reports Dead.
    fn dead_key_survives_a_transient_focus_loss() {
        dead_key_reports_dead(DeadKeyCase::FocusReturnsWhileHeld);
    }

    #[derive(Clone, Copy)]
    enum DeadKeyCase {
        UsInternationalAcute,
        SystemAcute,
        FrenchAltGrTilde,
        LayoutSwitchWhileHeld,
        FocusLostWhileHeld,
        FocusReturnsWhileHeld,
    }

    #[expect(unsafe_code, reason = "owned Win32 keyboard dispatch")]
    fn dead_key_reports_dead(case: DeadKeyCase) {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open_shown(&platform);
        let hwnd = window
            .as_any()
            .downcast_ref::<WindowsWindow>()
            .expect("Win32 backend")
            .hwnd();
        let keys = Arc::new(Mutex::new(Vec::<(
            keyboard_types::KeyState,
            keyboard_types::Key,
        )>::new()));
        let observed = Arc::clone(&keys);
        window.on_input(Box::new(move |event| {
            // Only the dead key under test; modifier releases the platform
            // synthesizes around a layout switch are not part of this contract.
            if let Some(keyboard) = event.as_keyboard()
                && !matches!(
                    keyboard.key,
                    keyboard_types::Key::Named(
                        keyboard_types::NamedKey::Control
                            | keyboard_types::NamedKey::Alt
                            | keyboard_types::NamedKey::AltGraph
                            | keyboard_types::NamedKey::Shift
                    )
                )
            {
                observed
                    .lock()
                    .expect("keys")
                    .push((keyboard.state, keyboard.key.clone()));
            }
            DispatchEventResult::resolved(true, false)
        }));
        // Layouts are switched for this thread only, after the window exists
        // (creating it resets the thread's layout), and restored on drop; each
        // row runs in its own child process.
        let (layout_id, vk, scan) = match case {
            DeadKeyCase::FrenchAltGrTilde => ("0000040C", 0x32_usize, 0x03_isize),
            _ => ("00020409", 0xDE, 0x28),
        };
        let layout = ThreadLayout::activate(layout_id);
        flush_dead_key_state();
        let altgr = matches!(case, DeadKeyCase::FrenchAltGrTilde).then(|| {
            ThreadKeyboardState::with_keys(&[
                (VK_CONTROL, true),
                (VK_LCONTROL, true),
                (VK_MENU, true),
                (VK_RMENU, true),
            ])
        });
        let (down, up, context) = match case {
            DeadKeyCase::SystemAcute => (WM_SYSKEYDOWN, WM_SYSKEYUP, 1 << 29),
            _ => (WM_KEYDOWN, WM_KEYUP, 0),
        };
        // Input goes through the queue and `TranslateMessage`, as the
        // platform's message loop delivers it: translation runs first and
        // updates the kernel's dead-key state before the window procedure
        // sees the keydown.
        let key_down = LPARAM(1 | (scan << 16) | context);
        let key_up = LPARAM(1 | (scan << 16) | context | (1 << 30) | (1 << 31));
        let deliver = |message: u32, lparam: LPARAM| {
            // SAFETY: integer key data for the fixture's own HWND on this
            // thread's queue.
            unsafe { PostMessageW(Some(hwnd), message, WPARAM(vk), lparam) }
                .expect("queue key message");
            pump_translated(hwnd);
        };
        deliver(down, key_down);
        let mut switched = None;
        match case {
            DeadKeyCase::LayoutSwitchWhileHeld => {
                switched = Some(ThreadLayout::activate("00000409"));
            }
            DeadKeyCase::FocusReturnsWhileHeld => {
                // SAFETY: a focus-loss notification for the fixture's HWND.
                unsafe { SendMessageW(hwnd, WM_KILLFOCUS, None, None) };
            }
            DeadKeyCase::FocusLostWhileHeld => {
                // SAFETY: a focus-loss notification for the fixture's HWND;
                // no other window is named.
                unsafe { SendMessageW(hwnd, WM_KILLFOCUS, None, None) };
                switched = Some(ThreadLayout::activate("00000409"));
                // The key-up went to the newly focused window; the same
                // physical key, now plain, goes down and up here.
                deliver(down, key_down);
            }
            _ => {}
        }
        deliver(up, key_up);
        drop(switched);
        drop(altgr);
        // Leave no composition pending for the rows that follow.
        flush_dead_key_state();
        drop(layout);
        let dead = keyboard_types::Key::Named(keyboard_types::NamedKey::Dead);
        let expected = if matches!(case, DeadKeyCase::FocusLostWhileHeld) {
            let apostrophe = keyboard_types::Key::Character("'".into());
            vec![
                (keyboard_types::KeyState::Down, dead),
                (keyboard_types::KeyState::Down, apostrophe.clone()),
                (keyboard_types::KeyState::Up, apostrophe),
            ]
        } else {
            vec![
                (keyboard_types::KeyState::Down, dead.clone()),
                (keyboard_types::KeyState::Up, dead),
            ]
        };
        assert_eq!(
            *keys.lock().expect("keys"),
            expected,
            "a dead key reports Dead on its press and on the release that \
             belongs to that press, and nothing else does"
        );
    }

    /// Clear a dead key a previous row left pending in the keyboard layout's
    /// composition state, which outlives the row's process: translate a space
    /// until it yields plain text.
    #[expect(unsafe_code, reason = "keyboard layout composition state")]
    fn flush_dead_key_state() {
        let state = [0u8; 256];
        let mut buffer = [0u16; 8];
        for _ in 0..4 {
            // SAFETY: live locals of the sizes the call requires; the layout is
            // this thread's own.
            let produced = unsafe {
                ToUnicodeEx(
                    0x20,
                    0x39,
                    &state,
                    &mut buffer,
                    0,
                    Some(GetKeyboardLayout(0)),
                )
            };
            if produced >= 0 {
                return;
            }
        }
    }

    /// Pump this HWND's queue the way the platform loop does: translate, then
    /// dispatch.
    #[expect(unsafe_code, reason = "owned Win32 message pumping")]
    fn pump_translated(hwnd: HWND) {
        for _ in 0..64 {
            let mut message = MSG::default();
            // SAFETY: only the fixture's owner-thread HWND is selected.
            if !unsafe { PeekMessageW(&raw mut message, Some(hwnd), 0, 0, PM_REMOVE) }.as_bool() {
                return;
            }
            // SAFETY: translate and dispatch the message this thread's queue
            // returned.
            unsafe {
                let _ = TranslateMessage(&raw const message);
                DispatchMessageW(&raw const message);
            }
        }
        panic!("the fixture's queue did not drain");
    }

    /// The calling thread's keyboard layout, switched for the duration of a
    /// row and restored on drop.
    struct ThreadLayout {
        previous: HKL,
    }

    impl ThreadLayout {
        #[expect(unsafe_code, reason = "thread-local keyboard layout switch")]
        fn activate(id: &str) -> Self {
            let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
            // SAFETY: `wide` is a NUL-terminated UTF-16 layout id that outlives
            // the call; the layout is activated for this thread only.
            let previous = unsafe {
                let previous = GetKeyboardLayout(0);
                LoadKeyboardLayoutW(windows::core::PCWSTR(wide.as_ptr()), KLF_ACTIVATE)
                    .expect("load the US-International layout");
                previous
            };
            Self { previous }
        }
    }

    impl Drop for ThreadLayout {
        #[expect(unsafe_code, reason = "restore the thread's keyboard layout")]
        fn drop(&mut self) {
            // SAFETY: `previous` was this thread's active layout handle.
            let _ = unsafe {
                ActivateKeyboardLayout(self.previous, ACTIVATE_KEYBOARD_LAYOUT_FLAGS::default())
            };
        }
    }

    // Counts WM_ENTERMENULOOP sent on this thread while the menu-key rows run.
    // Each row runs in its own child process, so the count is per row.
    static MENU_LOOPS: AtomicUsize = AtomicUsize::new(0);

    #[expect(unsafe_code, reason = "WH_CALLWNDPROC hook procedure")]
    unsafe extern "system" fn observe_menu_loop(
        code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // SAFETY: for a WH_CALLWNDPROC hook with a non-negative code, Win32
        // passes a valid CWPSTRUCT pointer in lparam for this call's duration.
        if code >= 0 && unsafe { (*(lparam.0 as *const CWPSTRUCT)).message } == WM_ENTERMENULOOP {
            MENU_LOOPS.fetch_add(1, Ordering::SeqCst);
        }
        // SAFETY: forwards this hook's own arguments unchanged.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    // A tapped Alt or F10 that no handler consumes reaches DefWindowProc,
    // which raises SC_KEYMENU. A window without a menu bar must not enter
    // modal menu mode, which swallows the next keystroke: no menu loop is
    // entered, the next character reaches the input callback and the window
    // stays open.
    #[expect(
        unsafe_code,
        reason = "actual owned Win32 system-key dispatch and message pumping"
    )]
    fn menu_key_keeps_next_character(key: &'static str) {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open_shown(&platform);
        let hwnd = window
            .as_any()
            .downcast_ref::<WindowsWindow>()
            .expect("Win32 backend")
            .hwnd();
        let closes = Arc::new(AtomicUsize::new(0));
        let close_observations = Arc::clone(&closes);
        window.on_close(Box::new(move || {
            close_observations.fetch_add(1, Ordering::SeqCst);
        }));
        let typed = Arc::new(Mutex::new(Vec::<String>::new()));
        let typed_observations = Arc::clone(&typed);
        window.on_input(Box::new(move |event| {
            if let Some(keyboard) = event.as_keyboard()
                && keyboard.state == keyboard_types::KeyState::Down
                && let keyboard_types::Key::Character(text) = &keyboard.key
            {
                typed_observations
                    .lock()
                    .expect("typed characters")
                    .push(text.clone());
            }
            DispatchEventResult::resolved(true, false)
        }));
        // SAFETY: a thread-local hook on the current thread, whose procedure
        // touches only an atomic; it is removed below before the row returns.
        let hook = unsafe {
            SetWindowsHookExW(
                WH_CALLWNDPROC,
                Some(observe_menu_loop),
                None,
                GetCurrentThreadId(),
            )
        }
        .expect("install menu-loop observer");
        let (vk, scan) = if key == "alt" {
            (0x12_usize, 0x38_isize)
        } else {
            (0x79, 0x44)
        };
        {
            let mut keyboard_state = (key == "alt").then(ThreadKeyboardState::with_alt_pressed);
            let context = if key == "alt" { 1 << 29 } else { 0 };
            // SAFETY: the platform owns this HWND on the current thread; the
            // message carries only integer key codes and documented key data,
            // and dispatch is synchronous on that thread.
            unsafe {
                SendMessageW(
                    hwnd,
                    WM_SYSKEYDOWN,
                    Some(WPARAM(vk)),
                    Some(LPARAM(1 | (scan << 16) | context)),
                );
            }
            if let Some(state) = keyboard_state.as_mut() {
                state.restore();
            }
        }
        // Queue the character after the key-down (whose arm drains pending
        // WM_CHARs) and before the key-up, so a menu loop entered by the
        // key-up would see it first.
        // SAFETY: as for the key-down: integer key data for this owned HWND.
        unsafe {
            PostMessageW(
                Some(hwnd),
                WM_KEYDOWN,
                WPARAM(0x41),
                LPARAM(1 | (0x1e << 16)),
            )
            .expect("queue character keydown");
            PostMessageW(Some(hwnd), WM_CHAR, WPARAM(0x61), LPARAM(1 | (0x1e << 16)))
                .expect("queue character");
        }
        // SAFETY: as above. Bits 30 and 31 mark the release of a held key.
        unsafe {
            SendMessageW(
                hwnd,
                WM_SYSKEYUP,
                Some(WPARAM(vk)),
                Some(LPARAM(1 | (scan << 16) | (1 << 30) | (1 << 31))),
            );
        }
        let mut drained = false;
        for _ in 0..64 {
            let mut message = MSG::default();
            // SAFETY: only the fixture's owner-thread HWND is selected.
            if !unsafe { PeekMessageW(&raw mut message, Some(hwnd), 0, 0, PM_REMOVE) }.as_bool() {
                drained = true;
                break;
            }
            // SAFETY: dispatch the message returned by this thread's queue.
            unsafe { DispatchMessageW(&raw const message) };
        }
        // SAFETY: the hook installed above, on this thread, removed once.
        unsafe { UnhookWindowsHookEx(hook) }.expect("remove menu-loop observer");
        assert!(drained, "{key}: owned message drain exceeded its bound");
        assert_eq!(
            MENU_LOOPS.load(Ordering::SeqCst),
            0,
            "{key}: the menu key entered modal menu mode"
        );
        assert_eq!(
            *typed.lock().expect("typed characters"),
            ["a"],
            "{key}: the character after the menu key reaches input"
        );
        assert_eq!(closes.load(Ordering::SeqCst), 0, "{key}: window stays open");
        window.close();
    }

    fn consumed_alt_space_withdraws_its_system_char() {
        alt_space_system_char(true, false);
    }
    fn unconsumed_alt_space_keeps_its_system_char() {
        alt_space_system_char(false, false);
    }
    fn pumping_consumer_of_alt_space_never_sees_its_system_char() {
        alt_space_system_char(true, true);
    }
    fn pumping_handler_of_alt_space_keeps_its_system_char() {
        alt_space_system_char(false, true);
    }

    // Alt+Space goes through the message loop's own path: the keydown is
    // queued, `TranslateMessage` queues its WM_SYSCHAR, then the keydown is
    // dispatched. That character is what `DefWindowProc` turns into the
    // system menu, so a consumed keydown must withdraw it and an unconsumed
    // one must leave it queued. The row never dispatches a WM_SYSCHAR itself:
    // one left behind is counted, not allowed to open a modal menu. With
    // `pump`, the handler pumps this window's messages before answering, as
    // a modal API does: the character must not be dispatchable then, since
    // the handler's result is not known yet.
    #[expect(
        unsafe_code,
        reason = "actual owned Win32 key translation and message pumping"
    )]
    fn alt_space_system_char(consume: bool, pump: bool) {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open_shown(&platform);
        let hwnd = window
            .as_any()
            .downcast_ref::<WindowsWindow>()
            .expect("Win32 backend")
            .hwnd();
        let keydowns = Arc::new(AtomicUsize::new(0));
        let keydown_observations = Arc::clone(&keydowns);
        let pumped = Arc::new(Mutex::new(Vec::new()));
        let pumped_observations = Arc::clone(&pumped);
        // The input callback must be `Send`; carry the handle as an address.
        let pumped_window = hwnd.0 as usize;
        window.on_input(Box::new(move |event| {
            if event
                .as_keyboard()
                .is_some_and(|key| key.state == keyboard_types::KeyState::Down)
            {
                keydown_observations.fetch_add(1, Ordering::SeqCst);
                if pump {
                    let mut message = MSG::default();
                    // SAFETY: the handler runs on the window's creating
                    // thread; only that HWND's queue is pumped. A WM_SYSCHAR
                    // the pump reaches is recorded instead of dispatched, so
                    // it cannot open a modal menu.
                    let hwnd = HWND(pumped_window as *mut _);
                    while unsafe { PeekMessageW(&raw mut message, Some(hwnd), 0, 0, PM_REMOVE) }
                        .as_bool()
                    {
                        if message.message == WM_SYSCHAR {
                            pumped_observations
                                .lock()
                                .expect("pumped characters")
                                .push(message.wParam.0);
                        } else {
                            // SAFETY: dispatches a message this thread's
                            // queue just returned.
                            unsafe { DispatchMessageW(&raw const message) };
                        }
                    }
                }
            }
            DispatchEventResult::resolved(true, consume)
        }));
        let mut keyboard_state = ThreadKeyboardState::with_alt_pressed();
        // SAFETY: integer VK_SPACE, scan-code and Alt-context data queued for
        // the platform's own HWND on its creating thread.
        unsafe {
            PostMessageW(
                Some(hwnd),
                WM_SYSKEYDOWN,
                WPARAM(0x20),
                LPARAM(1 | (0x39 << 16) | (1 << 29)),
            )
            .expect("queue Alt+Space keydown");
        }
        let mut keydown = MSG::default();
        // SAFETY: removes the message just queued for this owned HWND, then
        // translates and dispatches it as the platform's message loop does,
        // while the thread's logical Alt state is still held.
        unsafe {
            assert!(
                PeekMessageW(
                    &raw mut keydown,
                    Some(hwnd),
                    WM_SYSKEYDOWN,
                    WM_SYSKEYDOWN,
                    PM_REMOVE
                )
                .as_bool(),
                "the queued keydown"
            );
            let _ = TranslateMessage(&raw const keydown);
            DispatchMessageW(&raw const keydown);
        }
        keyboard_state.restore();
        let mut system_chars = Vec::new();
        let mut message = MSG::default();
        // SAFETY: only this owned HWND's WM_SYSCHARs are removed, undispatched.
        while unsafe {
            PeekMessageW(
                &raw mut message,
                Some(hwnd),
                WM_SYSCHAR,
                WM_SYSCHAR,
                PM_REMOVE,
            )
        }
        .as_bool()
        {
            system_chars.push(message.wParam.0);
        }
        assert_eq!(keydowns.load(Ordering::SeqCst), 1, "keydown delivery");
        let pumped = std::mem::take(&mut *pumped.lock().expect("pumped characters"));
        assert!(
            pumped.is_empty(),
            "a handler's pump reached {pumped:?} before its result was known"
        );
        if consume {
            assert!(
                system_chars.is_empty(),
                "a consumed Alt+Space left {system_chars:?} for DefWindowProc"
            );
        } else {
            assert_eq!(
                system_chars,
                [0x20],
                "an unconsumed Alt+Space keeps its system character"
            );
        }
        window.close();
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

    /// Records the `window_id` of every `native_window_destroyed` event on
    /// the `flui.platform` target.
    struct DestroyedWindows(Arc<Mutex<Vec<u64>>>);

    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for DestroyedWindows {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            #[derive(Default)]
            struct Fields {
                destroyed: bool,
                window_id: Option<u64>,
            }
            impl tracing::field::Visit for Fields {
                fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                    if field.name() == "event" {
                        self.destroyed = value == "native_window_destroyed";
                    }
                }
                fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
                    if field.name() == "window_id" {
                        self.window_id = Some(value);
                    }
                }
                fn record_debug(&mut self, _: &tracing::field::Field, _: &dyn std::fmt::Debug) {}
            }
            if event.metadata().target() != "flui.platform" {
                return;
            }
            let mut fields = Fields::default();
            event.record(&mut fields);
            if fields.destroyed {
                self.0
                    .lock()
                    .expect("recorder lock")
                    .push(fields.window_id.expect("the event names its window"));
            }
        }
    }

    // Each case runs in a process of its own and the subscriber is scoped to
    // this thread, so no other test can observe or feed the recorder.
    fn closed_window_traces_its_native_destruction() {
        use tracing_subscriber::layer::SubscriberExt as _;

        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, true);
        let sibling = open(&platform, true);
        let destroyed = Arc::new(Mutex::new(Vec::new()));
        let subscriber =
            tracing_subscriber::registry().with(DestroyedWindows(Arc::clone(&destroyed)));
        tracing::subscriber::with_default(subscriber, || window.close());
        assert_eq!(
            *destroyed.lock().expect("recorder lock"),
            [window.id().0],
            "closing one window traces its destruction, under its own identity, once"
        );
        sibling.close();
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

    const MK_LBUTTON: usize = 0x0001;
    const MK_RBUTTON: usize = 0x0002;
    const MK_SHIFT: usize = 0x0004;

    fn hwnd_of(window: &Arc<dyn HostWindow>) -> HWND {
        window
            .as_any()
            .downcast_ref::<WindowsWindow>()
            .expect("Win32 backend")
            .hwnd()
    }

    /// Every pointer event `window` delivers, in order.
    fn record_pointer(
        window: &Arc<dyn HostWindow>,
    ) -> Arc<Mutex<Vec<ui_events::pointer::PointerEvent>>> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&events);
        window.on_input(Box::new(move |input| {
            if let Some(event) = input.as_pointer() {
                recorded.lock().expect("pointer log").push(event.clone());
            }
            DispatchEventResult::default()
        }));
        events
    }

    fn kinds(events: &Mutex<Vec<ui_events::pointer::PointerEvent>>) -> Vec<&'static str> {
        use ui_events::pointer::PointerEvent;
        events
            .lock()
            .expect("pointer log")
            .iter()
            .map(|event| match event {
                PointerEvent::Down(_) => "down",
                PointerEvent::Up(_) => "up",
                PointerEvent::Move(_) => "move",
                PointerEvent::Cancel(_) => "cancel",
                _ => "other",
            })
            .collect()
    }

    /// A mouse message's client-coordinate `lParam`, negative values included.
    fn mouse_lparam(x: i16, y: i16) -> LPARAM {
        LPARAM(((y as u16 as isize) << 16) | x as u16 as isize)
    }

    /// The thread's queue-synchronized state for the keys a mouse message's
    /// `MK_*` mask reports, as retrieving that message from the queue would
    /// leave it.
    fn queue_state_for(mask: usize) -> ThreadKeyboardState {
        ThreadKeyboardState::with_keys(&[
            (VK_LBUTTON, mask & MK_LBUTTON != 0),
            (VK_RBUTTON, mask & MK_RBUTTON != 0),
            (VK_MBUTTON, false),
            (VK_SHIFT, mask & MK_SHIFT != 0),
            (VK_LSHIFT, mask & MK_SHIFT != 0),
        ])
    }

    /// Dispatches a mouse message synchronously, with the queue-synchronized
    /// key state its mask reports in place for the dispatch.
    #[expect(unsafe_code, reason = "synchronous mouse dispatch to an owned window")]
    fn send_mouse(hwnd: HWND, msg: u32, mask: usize, lparam: LPARAM) {
        let mut state = queue_state_for(mask);
        // SAFETY: the fixture's live window, on its creating thread; mouse
        // messages carry only integers and dereference no caller memory.
        unsafe { SendMessageW(hwnd, msg, Some(WPARAM(mask)), Some(lparam)) };
        state.restore();
    }

    #[expect(unsafe_code, reason = "reads the calling thread's mouse capture")]
    fn captured() -> HWND {
        // SAFETY: argument-free query of this thread's capture window.
        unsafe { GetCapture() }
    }

    /// A press captures the mouse and the last release lets go, so a drag
    /// released outside the client area still reaches the window — at the
    /// sign-extended client coordinates of that outside point.
    fn press_captures_the_mouse_until_the_last_release() {
        use ui_events::pointer::PointerEvent;
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, true);
        let hwnd = hwnd_of(&window);
        let events = record_pointer(&window);

        send_mouse(hwnd, WM_LBUTTONDOWN, MK_LBUTTON, mouse_lparam(10, 10));
        assert_eq!(captured(), hwnd, "a press captures the mouse");
        send_mouse(
            hwnd,
            WM_RBUTTONDOWN,
            MK_LBUTTON | MK_RBUTTON,
            mouse_lparam(10, 10),
        );
        send_mouse(hwnd, WM_LBUTTONUP, MK_RBUTTON, mouse_lparam(10, 10));
        assert_eq!(
            captured(),
            hwnd,
            "a release with another button held keeps the capture"
        );
        send_mouse(hwnd, WM_RBUTTONUP, 0, mouse_lparam(-40, -30));
        assert!(captured().is_invalid(), "the last release lets go");

        assert_eq!(kinds(&events), ["down", "down", "up", "up"]);
        let scale = window.scale_factor();
        let log = events.lock().expect("pointer log");
        let PointerEvent::Up(last) = log.last().expect("last release") else {
            unreachable!("kinds checked above");
        };
        assert_eq!(
            (last.state.position.x, last.state.position.y),
            (-40.0 / scale, -30.0 / scale),
            "an outside release keeps its negative client coordinates"
        );
        drop(log);
        window.close();
    }

    /// Another window taking the capture mid-press ends the sequence with a
    /// cancel; the sequence after it captures again and ends on its own,
    /// without a second cancel.
    #[expect(unsafe_code, reason = "moves the thread's mouse capture")]
    fn capture_taken_mid_press_cancels_the_sequence() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, true);
        let thief = open(&platform, true);
        let hwnd = hwnd_of(&window);
        let events = record_pointer(&window);

        send_mouse(hwnd, WM_LBUTTONDOWN, MK_LBUTTON, mouse_lparam(10, 10));
        assert_eq!(captured(), hwnd, "a press captures the mouse");
        // The button is still down when the capture moves.
        let mut held = queue_state_for(MK_LBUTTON);
        // SAFETY: the fixture's other live window, on its creating thread.
        unsafe { SetCapture(hwnd_of(&thief)) };
        held.restore();
        assert_eq!(kinds(&events), ["down", "cancel"]);

        // SAFETY: releases this thread's capture; takes no arguments.
        unsafe { ReleaseCapture() }.expect("release the thief's capture");
        send_mouse(hwnd, WM_LBUTTONDOWN, MK_LBUTTON, mouse_lparam(10, 10));
        assert_eq!(captured(), hwnd, "the next press captures again");
        send_mouse(hwnd, WM_LBUTTONUP, 0, mouse_lparam(10, 10));
        assert!(captured().is_invalid(), "the release lets go");
        assert_eq!(kinds(&events), ["down", "cancel", "down", "up"]);
        thief.close();
        window.close();
    }

    /// A pointer event carries the modifiers of its message — the
    /// queue-synchronized state — not the keyboard at processing time.
    fn pointer_modifiers_are_the_message_state() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, true);
        let hwnd = hwnd_of(&window);
        let events = record_pointer(&window);

        send_mouse(hwnd, WM_MOUSEMOVE, MK_SHIFT, mouse_lparam(5, 5));
        send_mouse(hwnd, WM_MOUSEMOVE, 0, mouse_lparam(6, 6));

        let log = events.lock().expect("pointer log");
        let modifiers: Vec<keyboard_types::Modifiers> = log
            .iter()
            .map(|event| match event {
                ui_events::pointer::PointerEvent::Move(update) => update.current.modifiers,
                _ => unreachable!("only moves were sent"),
            })
            .collect();
        assert_eq!(
            modifiers,
            [
                keyboard_types::Modifiers::SHIFT,
                keyboard_types::Modifiers::empty()
            ]
        );
        drop(log);
        window.close();
    }

    /// Runs `platform`'s message loop with `ready` as its bootstrap; returns
    /// once the loop ends. A loop that never ends is the child's timeout.
    fn run_loop(platform: WindowsPlatform, ready: impl FnOnce(OwnerPlatform) + 'static) {
        Box::new(platform)
            .run(Box::new(move |owner| {
                ready(owner);
                Ok(())
            }))
            .expect("Win32 message loop");
    }

    fn open_owned(owner: &OwnerPlatform) -> Arc<dyn HostWindow> {
        owner
            .open_window(WindowOptions {
                visible: false,
                size: Size::new(320.0, 240.0),
                ..Default::default()
            })
            .expect("create actual hidden Win32 window")
            .try_ready()
            .expect("owner-thread window opens directly")
    }

    /// Queues a native close, so the running loop delivers it as a user's
    /// close would.
    #[expect(
        unsafe_code,
        reason = "posting a native close to an owned Win32 window"
    )]
    fn post_close(window: &Arc<dyn HostWindow>) {
        let hwnd = window
            .as_any()
            .downcast_ref::<WindowsWindow>()
            .expect("Win32 backend")
            .hwnd();
        // SAFETY: the platform still tracks this live HWND; WM_CLOSE carries
        // no message data and the post dereferences no caller memory.
        unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) }
            .expect("post native close");
    }

    fn closing_the_last_window_ends_the_loop() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        run_loop(platform, |owner| post_close(&open_owned(&owner)));
    }

    fn exit_policy_veto_holds_until_a_reevaluation_allows_exit() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let answers = Arc::new(Mutex::new(Vec::new()));
        let releaser = Arc::new(Mutex::new(None));
        run_loop(platform, {
            let answers = Arc::clone(&answers);
            let releaser = Arc::clone(&releaser);
            move |owner| {
                let allow = Arc::new(AtomicBool::new(false));
                let shared = owner.shared();
                shared.set_exit_policy_hook(Box::new({
                    let allow = Arc::clone(&allow);
                    let answers = Arc::clone(&answers);
                    move || {
                        let answer = allow.load(Ordering::SeqCst);
                        answers.lock().expect("hook answers").push(answer);
                        answer
                    }
                }));
                post_close(&open_owned(&owner));
                // Once the hook has vetoed, a worker lets the loop run on,
                // then allows exit and asks for a fresh decision.
                let worker = std::thread::spawn(move || {
                    let start = Instant::now();
                    while answers.lock().expect("hook answers").is_empty() {
                        assert!(
                            start.elapsed() < Duration::from_secs(10),
                            "hook never consulted"
                        );
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    std::thread::sleep(Duration::from_millis(200));
                    allow.store(true, Ordering::SeqCst);
                    shared.request_exit_policy_reevaluation();
                });
                *releaser.lock().expect("releaser") = Some(worker);
            }
        });
        releaser
            .lock()
            .expect("releaser")
            .take()
            .expect("worker spawned")
            .join()
            .expect("worker");
        assert_eq!(
            *answers.lock().expect("hook answers"),
            [false, true],
            "the loop ended on a veto, or without the reevaluation"
        );
    }

    fn panicking_exit_policy_vetoes_and_stays_installed() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let calls = Arc::new(AtomicUsize::new(0));
        let releaser = Arc::new(Mutex::new(None));
        run_loop(platform, {
            let calls = Arc::clone(&calls);
            let releaser = Arc::clone(&releaser);
            move |owner| {
                let shared = owner.shared();
                shared.set_exit_policy_hook(Box::new({
                    let calls = Arc::clone(&calls);
                    move || {
                        // The first answer is a panic inside the owner
                        // procedure; the second allows exit.
                        assert!(
                            calls.fetch_add(1, Ordering::SeqCst) > 0,
                            "first exit-policy answer panics"
                        );
                        true
                    }
                }));
                post_close(&open_owned(&owner));
                let worker = std::thread::spawn(move || {
                    let start = Instant::now();
                    while calls.load(Ordering::SeqCst) == 0 {
                        assert!(
                            start.elapsed() < Duration::from_secs(10),
                            "hook never consulted"
                        );
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    std::thread::sleep(Duration::from_millis(200));
                    shared.request_exit_policy_reevaluation();
                });
                *releaser.lock().expect("releaser") = Some(worker);
            }
        });
        releaser
            .lock()
            .expect("releaser")
            .take()
            .expect("worker spawned")
            .join()
            .expect("worker");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "a panicking hook must veto without aborting and stay installed"
        );
    }

    thread_local! {
        static OWNER: RefCell<Option<OwnerPlatform>> = const { RefCell::new(None) };
    }

    fn window_opened_by_the_exit_policy_keeps_the_loop() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let consultations = Arc::new(AtomicUsize::new(0));
        run_loop(platform, {
            let consultations = Arc::clone(&consultations);
            move |owner| {
                // The first consultation opens (and queues the close of) a
                // replacement window yet allows exit; the loop must outlive
                // that answer and end only after the replacement closes.
                owner.shared().set_exit_policy_hook(Box::new(move || {
                    if consultations.fetch_add(1, Ordering::SeqCst) == 0 {
                        OWNER.with(|slot| {
                            let slot = slot.borrow();
                            post_close(&open_owned(slot.as_ref().expect("owner platform")));
                        });
                    }
                    true
                }));
                post_close(&open_owned(&owner));
                OWNER.with(|slot| *slot.borrow_mut() = Some(owner));
            }
        });
        drop(OWNER.with(|slot| slot.borrow_mut().take()));
        assert_eq!(
            consultations.load(Ordering::SeqCst),
            2,
            "the loop ended with the hook's replacement window still open"
        );
    }
}

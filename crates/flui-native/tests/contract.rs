//! Contract test for the window lifecycle every `Platform` implementation
//! must honour: reported sizes are positive and consistent with the scale
//! factor, `visible: false` is respected, and windows are independent.

use flui_foundation::geometry::Size;
use flui_native::current_platform;
use flui_platform::{PlatformWindow, WindowOptions};

/// The scale the window's snapshot reports with its drawable area.
fn scale_of(window: &(impl PlatformWindow + ?Sized)) -> f64 {
    window
        .state()
        .metrics
        .expect("a window with a drawable area")
        .scale()
        .get()
}

/// The drawable area in device pixels the window's snapshot reports.
fn physical_of(window: &(impl PlatformWindow + ?Sized)) -> Size<i32> {
    window
        .state()
        .metrics
        .expect("a window with a drawable area")
        .physical()
}

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

    // Contract 2: Platform must sample its displays: a snapshot (empty before a
    // winit loop runs) or a typed error, never a panic
    match platform.display_snapshot() {
        Ok(snapshot) => tracing::info!("Platform has {} display(s)", snapshot.displays().len()),
        Err(error) => tracing::info!("Platform has no display source: {error}"),
    }

    // Contract 3: Window creation should either succeed or fail gracefully
    let options = WindowOptions::default()
        .with_title("Contract Test Window".to_string())
        .with_size(Size::new(640.0, 480.0))
        .with_visible(false)
        .with_resizable(true)
        .with_decorated(true)
        .with_min_size(Some(Size::new(320.0, 240.0)))
        .with_max_size(None);

    match platform.open_window(
        options,
        flui_testing::window_handler::RecordingWindowHandler::new(),
    ) {
        Ok(window) => {
            tracing::info!("✓ Platform supports window creation");

            // Contract 4: Window must report valid sizes
            let logical_size = window.state().content_size;
            let physical_size = physical_of(&*window);
            let scale_factor = scale_of(&*window);

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
            let is_visible = window.state().visible;
            tracing::info!("Window visibility: {}", is_visible);
            assert!(
                !is_visible,
                "Window should not be visible with visible=false"
            );

            // Contract 7: Window focus API
            let is_focused = window.state().focused;
            tracing::info!("Window focus: {}", is_focused);

            // Contract 8: request_redraw() must not panic
            window.request_redraw();
            tracing::info!("✓ request_redraw() executed without panic");

            // Contract 9: a Win32 window is reachable by UI Automation in
            // every build, with no feature to turn on. Only a native window
            // can tell: the headless double always hands out its fake. The
            // Linux winit window's bridge is pinned by a real-loop test, since
            // winit opens no window outside its loop.
            #[cfg(target_os = "windows")]
            {
                use flui_native::os::windows::WindowsWindowExt as _;
                if window.win32().is_some() {
                    assert!(
                        window.accessibility().is_some(),
                        "a Win32 window hands out its UI Automation bridge"
                    );
                }
            }

            // Contract 10: Multiple window creation
            let options2 = WindowOptions::default()
                .with_title("Contract Test Window 2".to_string())
                .with_size(Size::new(400.0, 300.0))
                .with_visible(false);

            match platform.open_window(
                options2,
                flui_testing::window_handler::RecordingWindowHandler::new(),
            ) {
                Ok(window2) => {
                    tracing::info!("✓ Platform supports multiple concurrent windows");

                    let size2 = window2.state().content_size;
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
    use super::{physical_of, scale_of};
    use crate::Opened;
    use flui_foundation::geometry::Size;
    use flui_native::{HostWindow, OwnerPlatform, Platform, WindowsPlatform};
    use flui_platform::{DispatchEventResult, WindowOptions};
    use flui_testing::window_commands::AdmitAndApply as _;
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
            CWPSTRUCT, CallNextHookEx, DispatchMessageW, GetClientRect, GetQueueStatus, IsIconic,
            IsWindowVisible, MSG, PM_REMOVE, PeekMessageW, PostMessageW, QS_SENDMESSAGE,
            SW_MINIMIZE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SendMessageW,
            SetWindowPos, SetWindowsHookExW, ShowWindow, TranslateMessage, UnhookWindowsHookEx,
            WH_CALLWNDPROC, WM_CHAR, WM_CLOSE, WM_ENTERMENULOOP, WM_KEYDOWN, WM_KEYUP,
            WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_RBUTTONDOWN, WM_RBUTTONUP,
            WM_SYSCHAR, WM_SYSKEYDOWN, WM_SYSKEYUP,
        },
    };

    const CHILD: &str = "FLUI_NATIVE_WINDOW_CONTRACT_CHILD";
    const CASES: &[(&str, fn())] = &[
        (
            "touchpad_viewport_initialises_and_survives_its_messages",
            touchpad_viewport_initialises_and_survives_its_messages,
        ),
        (
            "shift_character_reaches_native_key_dispatch",
            shift_character_reaches_native_key_dispatch,
        ),
        (
            "deadline_rearms_independent_windows_without_input",
            deadline_rearms_independent_windows_without_input,
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
            "deadline_window_declining_its_frames_does_not_spin",
            deadline_window_declining_its_frames_does_not_spin,
        ),
        (
            "deadline_hidden_window_declining_its_frames_does_not_spin",
            deadline_hidden_window_declining_its_frames_does_not_spin,
        ),
        (
            "deadline_query_unwind_retains_hostile_captures",
            deadline_query_unwind_retains_hostile_captures,
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
            "a_reentrant_event_needing_an_answer_is_unhandled_and_delivered_once",
            a_reentrant_event_needing_an_answer_is_unhandled_and_delivered_once,
        ),
        ("alt_tap_keeps_next_character", alt_tap_keeps_next_character),
        ("f10_keeps_next_character", f10_keeps_next_character),
        (
            "cross_thread_sent_keydown_takes_no_queued_character",
            cross_thread_sent_keydown_takes_no_queued_character,
        ),
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
            "appearance_is_the_theme_the_chrome_follows_not_the_chrome_flag",
            appearance_is_the_theme_the_chrome_follows_not_the_chrome_flag,
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
            "reentrant_close_releases_the_surface_before_tracing_destruction",
            reentrant_close_releases_the_surface_before_tracing_destruction,
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
            "panicking_exit_policy_vetoes_and_is_asked_again",
            panicking_exit_policy_vetoes_and_is_asked_again,
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
            "focus_loss_and_minimising_end_the_held_press",
            focus_loss_and_minimising_end_the_held_press,
        ),
        (
            "focus_loss_ends_a_touch_and_silences_its_tail",
            focus_loss_ends_a_touch_and_silences_its_tail,
        ),
        (
            "pointer_modifiers_are_the_message_state",
            pointer_modifiers_are_the_message_state,
        ),
        (
            "queued_mouse_samples_keep_native_message_time",
            queued_mouse_samples_keep_native_message_time,
        ),
        (
            "queued_key_events_keep_native_message_time",
            queued_key_events_keep_native_message_time,
        ),
        (
            "extended_mouse_buttons_share_contact_and_capture",
            extended_mouse_buttons_share_contact_and_capture,
        ),
        (
            "extended_mouse_capture_loss_cancels_and_recovers",
            extended_mouse_capture_loss_cancels_and_recovers,
        ),
        (
            "unknown_native_pointer_messages_do_not_invent_contacts",
            unknown_native_pointer_messages_do_not_invent_contacts,
        ),
        (
            "synthetic_touch_and_pen_reach_native_pointer_dispatch",
            synthetic_touch_and_pen_reach_native_pointer_dispatch,
        ),
        (
            "a_captured_native_pointer_session_replays_to_what_it_delivered",
            a_captured_native_pointer_session_replays_to_what_it_delivered,
        ),
        (
            "a_captured_native_mouse_session_replays_to_what_it_delivered",
            a_captured_native_mouse_session_replays_to_what_it_delivered,
        ),
        (
            "native_mouse_wheels_keep_hover_identity_and_signed_units",
            native_mouse_wheels_keep_hover_identity_and_signed_units,
        ),
        (
            "a_pointer_routed_mouse_hovers_the_window",
            a_pointer_routed_mouse_hovers_the_window,
        ),
        (
            "a_leave_callback_that_hovers_again_keeps_the_hover",
            a_leave_callback_that_hovers_again_keeps_the_hover,
        ),
        (
            "fractional_native_wheel_packets_preserve_observed_precision_and_source",
            fractional_native_wheel_packets_preserve_observed_precision_and_source,
        ),
        (
            "repeated_samples_of_the_real_monitors_keep_their_ids",
            repeated_samples_of_the_real_monitors_keep_their_ids,
        ),
        (
            "refresh_is_the_rational_active_mode",
            refresh_is_the_rational_active_mode,
        ),
        (
            "window_display_is_a_display_of_the_snapshot",
            window_display_is_a_display_of_the_snapshot,
        ),
        (
            "a_dpi_change_publishes_the_new_scale_once",
            a_dpi_change_publishes_the_new_scale_once,
        ),
        (
            "moves_within_one_monitor_keep_the_display",
            moves_within_one_monitor_keep_the_display,
        ),
    ];

    // The monitors are keyed by their display targets, not the reusable GDI
    // names, and a sample of an unchanged topology is the same snapshot.
    fn repeated_samples_of_the_real_monitors_keep_their_ids() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let first = platform.display_snapshot().expect("first sample");
        let second = platform.display_snapshot().expect("second sample");
        let ids = |snapshot: &flui_platform::DisplaySnapshot| {
            snapshot
                .displays()
                .iter()
                .map(|display| display.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&first), ids(&second), "resampling changed a monitor id");
        let distinct: std::collections::HashSet<_> = ids(&first).into_iter().collect();
        assert_eq!(
            distinct.len(),
            first.displays().len(),
            "two monitors share an id"
        );
    }

    // Each active path's `refreshRate`, read straight from `QueryDisplayConfig`
    // and paired with its GDI source name, independently of the source under
    // test.
    #[expect(unsafe_code, reason = "reading the active display paths")]
    fn system_path_rates() -> Vec<(String, u32, u32)> {
        use windows::Win32::Devices::Display::{
            DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_MODE_INFO,
            DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME, DisplayConfigGetDeviceInfo,
            GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
        };
        let (mut paths, mut modes) = (0u32, 0u32);
        // SAFETY: both out-pointers are valid `u32`s for the call.
        let sized = unsafe {
            GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &raw mut paths, &raw mut modes)
        };
        assert_eq!(sized.0, 0, "sizing the display config");
        let mut path_info = vec![DISPLAYCONFIG_PATH_INFO::default(); paths as usize];
        let mut mode_info = vec![DISPLAYCONFIG_MODE_INFO::default(); modes as usize];
        // SAFETY: the buffers hold exactly the counts passed in; active paths
        // take no topology id.
        let queried = unsafe {
            QueryDisplayConfig(
                QDC_ONLY_ACTIVE_PATHS,
                &raw mut paths,
                path_info.as_mut_ptr(),
                &raw mut modes,
                mode_info.as_mut_ptr(),
                None,
            )
        };
        assert_eq!(queried.0, 0, "querying the display config");
        path_info.truncate(paths as usize);
        path_info
            .iter()
            .map(|path| {
                let mut name = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
                name.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
                name.header.size = std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
                name.header.adapterId = path.sourceInfo.adapterId;
                name.header.id = path.sourceInfo.id;
                // SAFETY: the header carries the request type and the struct's
                // own size; `name` outlives the call.
                let status = unsafe { DisplayConfigGetDeviceInfo(&raw mut name.header) };
                assert_eq!(status, 0, "naming the source");
                let end = name
                    .viewGdiDeviceName
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(name.viewGdiDeviceName.len());
                (
                    String::from_utf16_lossy(&name.viewGdiDeviceName[..end]),
                    path.targetInfo.refreshRate.Numerator,
                    path.targetInfo.refreshRate.Denominator,
                )
            })
            .collect()
    }

    // The snapshot's period is the target's rational rate converted exactly,
    // not a rounded whole-hertz figure.
    fn refresh_is_the_rational_active_mode() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let snapshot = platform.display_snapshot().expect("snapshot");
        let rates = system_path_rates();
        assert!(!rates.is_empty(), "the system reports an active path");
        let mut compared = 0;
        for (gdi_name, numerator, denominator) in rates {
            let Ok(expected) = flui_platform::RefreshPeriod::from_ratio(numerator, denominator)
            else {
                continue;
            };
            let display = snapshot
                .displays()
                .iter()
                .find(|display| display.name == gdi_name)
                .unwrap_or_else(|| panic!("{gdi_name} is not in {:?}", snapshot.displays()));
            assert_eq!(
                display.refresh_period,
                Some(expected),
                "{gdi_name}: {numerator}/{denominator} Hz"
            );
            compared += 1;
        }
        assert!(compared > 0, "no path carried a rate to compare");
    }

    fn window_display_is_a_display_of_the_snapshot() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, true);
        // The window reads the sample the snapshot took; it samples nothing.
        let snapshot = platform.display_snapshot().expect("snapshot");
        let id = window
            .state()
            .display
            .expect("a window on a monitor names its display");
        assert!(
            snapshot.get(id).is_some(),
            "{id:?} is not in {:?}",
            snapshot.displays()
        );
        window.close_now();
    }

    #[expect(unsafe_code, reason = "sending WM_DPICHANGED to an owned window")]
    fn a_dpi_change_publishes_the_new_scale_once() {
        use windows::Win32::UI::{HiDpi::GetDpiForWindow, WindowsAndMessaging::WM_DPICHANGED};
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, true);
        let hwnd = hwnd_of(&window);
        let rescaled = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&rescaled);
        let weak = Rc::downgrade(&window);
        let scale = std::cell::Cell::new(
            window
                .state()
                .metrics
                .map(flui_foundation::geometry::WindowMetrics::scale),
        );
        window.handler.react_to_state_change(move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            let now = window
                .state()
                .metrics
                .map(flui_foundation::geometry::WindowMetrics::scale);
            if scale.replace(now) != now {
                counted.fetch_add(1, Ordering::SeqCst);
            }
        });
        let send = |dpi: u32| {
            // SAFETY: the window is live on this thread; `rect` outlives the
            // synchronous send, which is the pointer WM_DPICHANGED's lparam
            // carries.
            unsafe {
                let mut rect = RECT::default();
                windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &raw mut rect)
                    .expect("window rect");
                SendMessageW(
                    hwnd,
                    WM_DPICHANGED,
                    Some(WPARAM(((dpi << 16) | dpi) as usize)),
                    Some(LPARAM((&raw const rect) as isize)),
                );
            }
        };
        // SAFETY: a plain query of the live window.
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        send(dpi * 3 / 2);
        assert_eq!(
            rescaled.load(Ordering::SeqCst),
            1,
            "a DPI change must publish its scale once"
        );
        send(dpi);
        window.close_now();
    }

    #[expect(unsafe_code, reason = "moving an owned window")]
    fn moves_within_one_monitor_keep_the_display() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, true);
        let hwnd = hwnd_of(&window);
        let moved = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&moved);
        let weak = Rc::downgrade(&window);
        let display = std::cell::Cell::new(window.state().display);
        window.handler.react_to_state_change(move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            let now = window.state().display;
            if display.replace(now) != now {
                counted.fetch_add(1, Ordering::SeqCst);
            }
        });
        let move_to = |x: i32, y: i32| {
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
        };
        // The first message seeds the window's monitor.
        move_to(100, 100);
        let seeded = moved.load(Ordering::SeqCst);
        for (x, y) in [(140, 120), (200, 150), (80, 90)] {
            move_to(x, y);
        }
        assert_eq!(
            moved.load(Ordering::SeqCst),
            seeded,
            "a move that stays on one monitor changed the display"
        );
        window.close_now();
    }

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

    fn open(platform: &WindowsPlatform, decorated: bool) -> Opened {
        Opened::on(
            platform,
            WindowOptions::default()
                .with_visible(false)
                .with_decorated(decorated)
                .with_size(Size::new(320.0, 240.0)),
        )
        .expect("create actual hidden Win32 window")
    }

    // The DirectManipulation viewport initialises on a real window and the
    // window reports precise scrolling and trackpad gestures as native; the
    // messages that drive it (a contact offer for a pointer that is no
    // touchpad's, the pump timer, the modal size loop) are handled with no
    // touchpad attached, and closing the window releases the viewport.
    // Whether a precision touchpad's manipulation arrives is not exercised:
    // it needs the hardware (docs/platform-parity.md).
    #[expect(unsafe_code, reason = "owned Win32 messages to the fixture's own HWND")]
    fn touchpad_viewport_initialises_and_survives_its_messages() {
        use flui_platform::Availability;
        use flui_platform::PlatformInput;
        use windows::Win32::UI::WindowsAndMessaging::{
            DM_POINTERHITTEST, WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE, WM_TIMER,
        };

        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open_shown(&platform);
        let hwnd = hwnd_of(&window);
        let Some(PlatformInput::Capabilities(capabilities)) =
            window.handler.inputs().into_iter().next()
        else {
            panic!("the window reports its capabilities first");
        };
        assert_eq!(capabilities.precise_scroll(), Availability::Native);
        assert_eq!(capabilities.trackpad_gestures(), Availability::Native);
        // SAFETY: integer message data for the fixture's own HWND on its
        // owner thread; `0x444D` is the viewport's pacing timer, and pointer
        // id `0x7fff_fff0` names no live pointer.
        unsafe {
            for (message, wparam) in [
                (WM_TIMER, 0x444D),
                (DM_POINTERHITTEST, 0x7fff_fff0),
                (WM_ENTERSIZEMOVE, 0),
                (WM_TIMER, 0x444D),
                (DM_POINTERHITTEST, 0x7fff_fff0),
                (WM_EXITSIZEMOVE, 0),
                (WM_TIMER, 0x444D),
            ] {
                SendMessageW(hwnd, message, Some(WPARAM(wparam)), Some(LPARAM(0)));
            }
        }
        let closed = Arc::new(AtomicBool::new(false));
        let close_observed = Arc::clone(&closed);
        window.handler.react_to_close(move || {
            close_observed.store(true, Ordering::SeqCst);
        });
        window.close_now();
        assert!(
            closed.load(Ordering::SeqCst),
            "closing runs the close callback"
        );
        drop(window);
        drop(platform);
    }

    // DefWindowProc ignores Alt+F4 and leaves menu mode at once for a hidden
    // window, so system-key defaults are exercised on a shown one.
    fn open_shown(platform: &WindowsPlatform) -> Opened {
        Opened::on(
            platform,
            WindowOptions::default()
                .with_visible(true)
                .with_size(Size::new(160.0, 120.0)),
        )
        .expect("create actual shown Win32 window")
    }

    struct ClosePointerTarget(Rc<dyn HostWindow>);

    impl Drop for ClosePointerTarget {
        fn drop(&mut self) {
            self.0.close_for_cleanup();
        }
    }

    #[expect(
        unsafe_code,
        reason = "keeps only the ephemeral injected-input target above unrelated host windows"
    )]
    fn open_pointer_target(platform: &WindowsPlatform) -> Option<(Opened, ClosePointerTarget)> {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, HWND_TOPMOST, SetForegroundWindow,
        };
        let window = open_shown(platform);
        let close = ClosePointerTarget(Rc::clone(&window));
        // SAFETY: the fixture's exact live owner-thread HWND. It is destroyed
        // by the RAII owner on success, early refusal, or assertion unwind;
        // no pre-existing host window's z-order or style is changed.
        unsafe {
            SetWindowPos(
                hwnd_of(&window),
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE,
            )
        }
        .expect("keep ephemeral pointer target above host windows");
        // SAFETY: only this fixture's live HWND is activated; its RAII owner
        // destroys it on failure as well. Pump activation before real input
        // so a first contact cannot be consumed merely to activate the target.
        let _ = unsafe { SetForegroundWindow(hwnd_of(&window)) };
        for _ in 0..25 {
            pump_pointer_thread();
            std::thread::sleep(Duration::from_millis(2));
        }
        // SAFETY: observes desktop activation without retaining a native handle.
        if unsafe { GetForegroundWindow() } != hwnd_of(&window) {
            eprintln!(
                "CANNOT_VERIFY native input: host foreground policy refused activation of the owned ephemeral target"
            );
            return None;
        }
        Some((window, close))
    }

    fn deadline_rearms_independent_windows_without_input() {
        run_deadline_windows("rearm");
    }

    fn deadline_query_can_close_an_independent_window() {
        run_deadline_windows("close");
    }

    fn deadline_none_allows_same_instant_readmission() {
        let calls = Arc::new(AtomicUsize::new(0));
        let result = Arc::clone(&calls);
        Box::new(WindowsPlatform::new().expect("native Windows platform"))
            .run(crate::app::with_log(move |log, owner| {
                let window = Opened::ready(
                    &owner,
                    WindowOptions::default()
                        .with_visible(true)
                        .with_size(Size::new(160.0, 120.0)),
                );
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
                let weak = Rc::downgrade(&window);
                window.handler.react_to_frame(move |_| {
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
                        weak.upgrade().expect("live readmission window").close_now();
                        callback_proxy
                            .request_quit()
                            .expect("quit after readmission");
                    }
                });
                let proxy = owner.proxy();
                log.answer_deadline_with(move || {
                    let (deadline, stage) = *state.lock().expect("readmission state");
                    if stage == 1 {
                        // None must be observed by the loop before the next
                        // owner turn re-admits the same absolute instant.
                        proxy.wake().expect("readmit on the next owner turn");
                    }
                    deadline
                });
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
        // No window is open, so a delivery runs no frame.
        let queries = unserviced_deadline_queries(None);
        assert!(
            queries < 16,
            "a deadline no frame can service was re-armed {queries} times"
        );
    }

    fn deadline_window_declining_its_frames_does_not_spin() {
        // A visible window paints, and its handler declines every frame.
        let queries = unserviced_deadline_queries(Some(true));
        assert!(
            queries < 16,
            "a deadline a declining window received was re-armed {queries} times"
        );
    }

    fn deadline_hidden_window_declining_its_frames_does_not_spin() {
        // A hidden window gets its frame request directly, and its handler
        // declines it.
        let queries = unserviced_deadline_queries(Some(false));
        assert!(
            queries < 16,
            "a deadline a declining hidden window received was re-armed {queries} times"
        );
    }

    // How many times the loop queries a hook that keeps answering one past
    // instant no frame services, over a fixed span a watchdog ends. `window`
    // opens one window, visible or hidden, whose handler declines every frame
    // (a handler with nothing to draw with yet). A delivery that ran no frame
    // must stay delivered, so the loop parks.
    fn unserviced_deadline_queries(window: Option<bool>) -> usize {
        let queries = Arc::new(AtomicUsize::new(0));
        let result = Arc::clone(&queries);
        Box::new(WindowsPlatform::new().expect("native Windows platform"))
            .run(crate::app::with_log(move |log, owner| {
                if let Some(visible) = window {
                    // No frame reaction: the recording handler declines.
                    // The platform keeps the window open for the run.
                    let _window = Opened::ready(
                        &owner,
                        WindowOptions::default()
                            .with_visible(visible)
                            .with_size(Size::new(160.0, 120.0)),
                    );
                }
                let watchdog = owner.proxy();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(300));
                    let _ = watchdog.request_quit();
                });
                let due = web_time::Instant::now() + Duration::from_millis(30);
                log.answer_deadline_with(move || {
                    queries.fetch_add(1, Ordering::SeqCst);
                    Some(due)
                });
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
        let frames = Arc::new(AtomicUsize::new(0));
        let result = Arc::clone(&frames);
        Box::new(WindowsPlatform::new().expect("native Windows platform"))
            .run(crate::app::with_log(move |log, owner| {
                let window = Opened::ready(
                    &owner,
                    WindowOptions::default()
                        .with_visible(true)
                        .with_size(Size::new(160.0, 120.0)),
                );
                let hwnd = hwnd_of(&window);
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
                window.handler.react_to_frame(move |_| {
                    if callback_pending.load(Ordering::SeqCst) == 0
                        || web_time::Instant::now() < due
                    {
                        return;
                    }
                    callback_pending.fetch_sub(1, Ordering::SeqCst);
                    frames.fetch_add(1, Ordering::SeqCst);
                });
                log.answer_deadline_with(move || {
                    (pending.load(Ordering::SeqCst) > 0).then_some(due)
                });
                Ok(())
            }))
            .expect("native same-instant loop returns normally");
        result.load(Ordering::SeqCst)
    }

    fn deadline_query_unwind_retains_hostile_captures() {
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
                .run(crate::app::with_log(move |log, _| {
                    log.answer_deadline_with(move || {
                        std::hint::black_box(&captures);
                        panic!("deadline query failure");
                    });
                    Ok(())
                }))
                .expect("native query returns unless its handler panics");
        }))
        .expect_err("the handler's panic propagates through the Rust run boundary");
        assert_eq!(
            failure.downcast_ref::<&'static str>().copied(),
            Some("deadline query failure")
        );
        assert_eq!(
            drops.load(Ordering::SeqCst),
            0,
            "the failed handler's captures are retained, not dropped into the unwind"
        );
        drop(failure);
        let platform = WindowsPlatform::new().expect("independent native owner after failed query");
        let window = open(&platform, true);
        let weak = Rc::downgrade(&window);
        window.close_now();
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
        // A minimized or hidden window paints nothing, so a due deadline must
        // reach its frame callback by another route or stay stranded until
        // input. A watchdog quits a stranded loop so the row fails instead of
        // hanging.
        let serviced = Arc::new(Mutex::new(None::<bool>));
        let result = Arc::clone(&serviced);
        Box::new(WindowsPlatform::new().expect("native Windows platform"))
            .run(crate::app::with_log(move |log, owner| {
                let window = Opened::ready(
                    &owner,
                    WindowOptions::default()
                        .with_visible(minimized)
                        .with_size(Size::new(160.0, 120.0)),
                );
                let hwnd = hwnd_of(&window);
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
                window.handler.react_to_frame(move |_| {
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
                });
                log.answer_deadline_with(move || *pending.lock().expect("deadline state"));
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
        // Every frame request before the first admitted deadline is a
        // delivery nothing asked for (an answer no deadline was due for).
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
            .run(crate::app::with_log(move |log, owner| {
                let proxy = owner.proxy();
                let mut first = None;
                let mut windows = Vec::new();
                for index in 0..2 {
                    let window = Opened::ready(
                        &owner,
                        WindowOptions::default()
                            .with_visible(true)
                            .with_size(Size::new(160.0, 120.0)),
                    );
                    windows.push(Rc::clone(&window));
                    if index == 0 {
                        first = Some(Rc::downgrade(&window));
                        let closed = Arc::clone(&first_closed);
                        window.handler.react_to_close(move || {
                            closed.fetch_add(1, Ordering::SeqCst);
                        });
                    }
                    let callback_deadlines = Arc::clone(&deadlines);
                    let callback_counts = Arc::clone(&counts);
                    let callback_pending = Arc::clone(&pending);
                    let callback_proxy = proxy.clone();
                    let callback_early = Arc::clone(&early);
                    let callback_armed = Arc::clone(&armed);
                    let weak = Rc::downgrade(&window);
                    window.handler.react_to_frame(move |_| {
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
                            weak.upgrade().expect("due live window").close_now();
                            if callback_pending.fetch_sub(1, Ordering::SeqCst) == 1 {
                                callback_proxy
                                    .request_quit()
                                    .expect("quit after both deadline rearms");
                            }
                        }
                    });
                }
                // The hook is `Send`, the window is not: the hook reaches the
                // first window through the owner thread it runs on.
                thread_local! {
                    static FIRST: RefCell<Option<std::rc::Weak<dyn HostWindow>>> =
                        const { RefCell::new(None) };
                }
                FIRST.set(Some(first.expect("first native window")));
                for window in &windows {
                    let hwnd = hwnd_of(window);
                    // SAFETY: the live wrapper owns this HWND on its creating
                    // thread; the paint it sends runs synchronously here.
                    let _ = unsafe { UpdateWindow(hwnd) };
                }
                drop(windows);
                armed.store(true, Ordering::SeqCst);
                let initial = web_time::Instant::now() + Duration::from_millis(80);
                *deadlines.lock().expect("deadline state") = [Some(initial); 2];
                let hook_deadlines = Arc::clone(&deadlines);
                log.answer_deadline_with(move || {
                    let first_query = queried.fetch_add(1, Ordering::SeqCst) == 0;
                    if mode == "close" && first_query {
                        FIRST
                            .take()
                            .and_then(|first| first.upgrade())
                            .expect("the hook runs on the owner thread, with the first window live")
                            .close_now();
                        hook_deadlines.lock().expect("deadline state")[0] = None;
                    }
                    hook_deadlines
                        .lock()
                        .expect("deadline state")
                        .iter()
                        .flatten()
                        .copied()
                        .min()
                });
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
    /// A nested Alt+F4 sent while the first one's callback runs needs its
    /// answer now: it is answered "not handled", so the native default closes
    /// the window, and the key itself is delivered once, after the running
    /// callback returns (ADR-0207 §7).
    fn a_reentrant_event_needing_an_answer_is_unhandled_and_delivered_once() {
        system_keyboard_close("reentrant");
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
        let original_weak = Rc::downgrade(&window);
        let replacement = Rc::new(RefCell::new(None::<Opened>));
        let replacement_closes = Arc::new(AtomicUsize::new(0));
        let hwnd = hwnd_of(&window);
        let close_count = Arc::new(AtomicUsize::new(0));
        let close_observations = Arc::clone(&close_count);
        window.handler.react_to_close(move || {
            close_observations.fetch_add(1, Ordering::SeqCst);
        });
        let input_count = Arc::new(AtomicUsize::new(0));
        let veto_count = Arc::new(AtomicUsize::new(0));
        if mode == "vetoed" {
            let veto_observations = Arc::clone(&veto_count);
            window.handler.react_to_close_request(move || {
                veto_observations.fetch_add(1, Ordering::SeqCst);
                flui_platform::CloseDecision::KeepOpen
            });
        }
        if mode != "unhandled" {
            let input_observations = Arc::clone(&input_count);
            let weak = Rc::downgrade(&window);
            let raw_hwnd = hwnd.0 as isize;
            let callback_platform = Arc::downgrade(&platform);
            let callback_replacement = Rc::clone(&replacement);
            let callback_replacement_closes = Arc::clone(&replacement_closes);
            window.handler.react_to_input(move |event| {
                if matches!(event, flui_platform::PlatformInput::Capabilities(_)) {
                    return flui_platform::DispatchEventResult::default();
                }
                assert_eq!(
                    event.as_keyboard().expect("native key event").key,
                    flui_platform::keyboard::Key::Named(flui_platform::keyboard::NamedKey::F4)
                );
                let first = input_observations.fetch_add(1, Ordering::SeqCst) == 0;
                if mode == "reentrant" && first {
                    // SAFETY: the upgraded callback's window owns this
                    // same native handle on the current thread. This nested
                    // delivery is answered "not handled" before its callback
                    // can run, so its native default closes the window.
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
                    weak.upgrade().expect("live input window").close_now();
                    let replacement =
                        open(&callback_platform.upgrade().expect("live platform"), true);
                    let closes = Arc::clone(&callback_replacement_closes);
                    replacement.handler.react_to_close(move || {
                        closes.fetch_add(1, Ordering::SeqCst);
                    });
                    *callback_replacement.borrow_mut() = Some(replacement);
                }
                DispatchEventResult::resolved(
                    true,
                    mode == "prevented" || (mode == "reentrant" && first),
                )
            });
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
            if mode == "reentrant" {
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
        let replacement = replacement.borrow_mut().take();
        if let Some(replacement) = replacement {
            assert!(
                raw_window_handle::HasWindowHandle::window_handle(&*replacement.surface_target())
                    .is_ok(),
                "replacement remains natively live"
            );
            replacement.close_now();
        }
        assert_eq!(
            close_count.load(Ordering::SeqCst),
            usize::from(mode != "prevented" && mode != "vetoed"),
            "{mode}: native default close"
        );
        assert_eq!(
            veto_count.load(Ordering::SeqCst),
            usize::from(mode == "vetoed"),
            "{mode}: close veto delivery"
        );
        if mode == "vetoed" {
            window
                .handler
                .react_to_close_request(|| flui_platform::CloseDecision::Close);
        }
        if mode == "vetoed" || mode == "prevented" {
            window.close_now();
        }
        drop(window);
        assert!(
            original_weak.upgrade().is_none(),
            "{mode}: closed window retained by registry"
        );
        let next = open(&platform, true);
        let next_weak = Rc::downgrade(&next);
        next.close_now();
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

    // A keydown another thread sends is delivered by the next peek of this
    // thread's queue, which can be the character drain of a translated
    // keystroke being dispatched. It owns none of the characters queued
    // here: the outer keystroke keeps its translated `é`, and the sent one
    // reports its own key.
    #[expect(unsafe_code, reason = "owned Win32 keyboard dispatch across threads")]
    fn cross_thread_sent_keydown_takes_no_queued_character() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, true);
        let hwnd = hwnd_of(&window);
        let typed = Arc::new(Mutex::new(Vec::<String>::new()));
        let typed_observations = Arc::clone(&typed);
        window.handler.react_to_input(move |event| {
            if matches!(event, flui_platform::PlatformInput::Capabilities(_)) {
                return flui_platform::DispatchEventResult::default();
            }
            if let Some(keyboard) = event.as_keyboard()
                && keyboard.state() == flui_platform::keyboard::KeyState::Down
                && let flui_platform::keyboard::Key::Character(text) = &keyboard.key
            {
                typed_observations
                    .lock()
                    .expect("typed characters")
                    .push(text.as_str().to_owned());
            }
            DispatchEventResult::resolved(true, false)
        });
        let _plain = ThreadKeyboardState::with_keys(&[
            (VK_SHIFT, false),
            (VK_CONTROL, false),
            (VK_MENU, false),
        ]);
        // The keystroke as `TranslateMessage` leaves it: the keydown, then
        // the character the layout produced for it.
        // SAFETY: integer key data for the fixture's own HWND on this thread's
        // queue.
        unsafe {
            PostMessageW(
                Some(hwnd),
                WM_KEYDOWN,
                WPARAM(0x41),
                LPARAM(1 | (0x1e << 16)),
            )
            .expect("queue keydown");
            PostMessageW(Some(hwnd), WM_CHAR, WPARAM(0xE9), LPARAM(1 | (0x1e << 16)))
                .expect("queue its character");
        }
        let mut keydown = MSG::default();
        // SAFETY: only the fixture's owner-thread HWND is selected; no sent
        // message is pending yet.
        let retrieved = unsafe {
            PeekMessageW(
                &raw mut keydown,
                Some(hwnd),
                WM_KEYDOWN,
                WM_KEYDOWN,
                PM_REMOVE,
            )
        };
        assert!(retrieved.as_bool(), "the posted keydown is queued");
        // `HWND` is a raw pointer and not `Send`; the sender gets its value.
        let target = hwnd.0 as usize;
        let sender = std::thread::spawn(move || {
            // SAFETY: the window lives on the test thread until after this
            // thread is joined; the message carries integer key data only.
            unsafe {
                SendMessageW(
                    HWND(target as *mut std::ffi::c_void),
                    WM_KEYDOWN,
                    Some(WPARAM(0x42)),
                    Some(LPARAM(1 | (0x30 << 16))),
                );
            }
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        // SAFETY: reads this thread's own queue status; dispatches nothing.
        while unsafe { GetQueueStatus(QS_SENDMESSAGE) } & QS_SENDMESSAGE.0 == 0 {
            assert!(
                Instant::now() < deadline,
                "the cross-thread keydown reached this thread's queue"
            );
            std::thread::yield_now();
        }
        // SAFETY: dispatch the keydown this thread's queue returned above.
        unsafe { DispatchMessageW(&raw const keydown) };
        sender.join().expect("sender thread");
        assert_eq!(
            *typed.lock().expect("typed characters"),
            ["b", "é"],
            "the sent keydown, delivered inside the outer drain, reports its own \
             key and the outer keystroke keeps its translated character"
        );
        window.close_now();
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
        let hwnd = hwnd_of(&window);
        let keys = Arc::new(Mutex::new(Vec::<(
            flui_platform::keyboard::KeyState,
            flui_platform::keyboard::Key,
        )>::new()));
        let observed = Arc::clone(&keys);
        window.handler.react_to_input(move |event| {
            if matches!(event, flui_platform::PlatformInput::Capabilities(_)) {
                return flui_platform::DispatchEventResult::default();
            }
            // Only the dead key under test; modifier releases the platform
            // synthesizes around a layout switch are not part of this contract.
            if let Some(keyboard) = event.as_keyboard()
                && !matches!(
                    keyboard.key,
                    flui_platform::keyboard::Key::Named(
                        flui_platform::keyboard::NamedKey::Control
                            | flui_platform::keyboard::NamedKey::Alt
                            | flui_platform::keyboard::NamedKey::AltGraph
                            | flui_platform::keyboard::NamedKey::Shift
                    )
                )
            {
                observed
                    .lock()
                    .expect("keys")
                    .push((keyboard.state(), keyboard.key.clone()));
            }
            DispatchEventResult::resolved(true, false)
        });
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
        let dead = flui_platform::keyboard::Key::Named(flui_platform::keyboard::NamedKey::Dead);
        let expected = if matches!(case, DeadKeyCase::FocusLostWhileHeld) {
            let apostrophe = flui_platform::keyboard::Key::character("'");
            vec![
                (flui_platform::keyboard::KeyState::Down, dead),
                (flui_platform::keyboard::KeyState::Down, apostrophe.clone()),
                (flui_platform::keyboard::KeyState::Up, apostrophe),
            ]
        } else {
            vec![
                (flui_platform::keyboard::KeyState::Down, dead.clone()),
                (flui_platform::keyboard::KeyState::Up, dead),
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

    thread_local! {
        /// Each pen press the thread retrieved: where the system mapped it on
        /// the screen from its himetric location and its device's rectangles.
        static PEN_PRESSES: std::cell::RefCell<Vec<(f64, f64)>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }

    /// A `WH_GETMESSAGE` hook reading each pen press while it is current, so
    /// the published point can be held against the native himetric reading.
    #[expect(unsafe_code, reason = "WH_GETMESSAGE hook procedure")]
    unsafe extern "system" fn observe_pen_press(
        code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        use windows::Win32::UI::{
            Input::Pointer::{GetPointerDeviceRects, GetPointerPenInfo, POINTER_PEN_INFO},
            WindowsAndMessaging::WM_POINTERDOWN,
        };
        // SAFETY: for a WH_GETMESSAGE hook with a non-negative code, Win32
        // passes a valid MSG pointer in lparam for this call's duration.
        let message = (code >= 0).then(|| unsafe { *(lparam.0 as *const MSG) });
        if let Some(message) = message.filter(|message| message.message == WM_POINTERDOWN) {
            let mut pen = POINTER_PEN_INFO::default();
            let (mut device, mut display) = (RECT::default(), RECT::default());
            // SAFETY: the message is the one being retrieved, so its pointer
            // id is current; every out pointer addresses a live local.
            let read = unsafe {
                GetPointerPenInfo((message.wParam.0 & 0xffff) as u32, &raw mut pen).is_ok()
                    && GetPointerDeviceRects(
                        pen.pointerInfo.sourceDevice,
                        &raw mut device,
                        &raw mut display,
                    )
                    .is_ok()
            };
            if read {
                let himetric = pen.pointerInfo.ptHimetricLocationRaw;
                let axis = |value: i32, from: (i32, i32), to: (i32, i32)| {
                    f64::from(to.0)
                        + (f64::from(value) - f64::from(from.0)) * f64::from(to.1 - to.0)
                            / f64::from(from.1 - from.0)
                };
                PEN_PRESSES.with_borrow_mut(|presses| {
                    presses.push((
                        axis(
                            himetric.x,
                            (device.left, device.right),
                            (display.left, display.right),
                        ),
                        axis(
                            himetric.y,
                            (device.top, device.bottom),
                            (display.top, display.bottom),
                        ),
                    ));
                });
            }
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
        let hwnd = hwnd_of(&window);
        let closes = Arc::new(AtomicUsize::new(0));
        let close_observations = Arc::clone(&closes);
        window.handler.react_to_close(move || {
            close_observations.fetch_add(1, Ordering::SeqCst);
        });
        let typed = Arc::new(Mutex::new(Vec::<String>::new()));
        let typed_observations = Arc::clone(&typed);
        window.handler.react_to_input(move |event| {
            if matches!(event, flui_platform::PlatformInput::Capabilities(_)) {
                return flui_platform::DispatchEventResult::default();
            }
            if let Some(keyboard) = event.as_keyboard()
                && keyboard.state() == flui_platform::keyboard::KeyState::Down
                && let flui_platform::keyboard::Key::Character(text) = &keyboard.key
            {
                typed_observations
                    .lock()
                    .expect("typed characters")
                    .push(text.as_str().to_owned());
            }
            DispatchEventResult::resolved(true, false)
        });
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
        window.close_now();
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
        let hwnd = hwnd_of(&window);
        let keydowns = Arc::new(AtomicUsize::new(0));
        let keydown_observations = Arc::clone(&keydowns);
        let pumped = Arc::new(Mutex::new(Vec::new()));
        let pumped_observations = Arc::clone(&pumped);
        // The input callback must be `Send`; carry the handle as an address.
        let pumped_window = hwnd.0 as usize;
        window.handler.react_to_input(move |event| {
            if matches!(event, flui_platform::PlatformInput::Capabilities(_)) {
                return flui_platform::DispatchEventResult::default();
            }
            if event
                .as_keyboard()
                .is_some_and(|key| key.state() == flui_platform::keyboard::KeyState::Down)
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
        });
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
        window.close_now();
    }

    #[expect(
        unsafe_code,
        reason = "actual owned Win32 close dispatch on its creating thread"
    )]
    fn native_close_retires_final_registry_owner() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, true);
        let weak = Rc::downgrade(&window);
        let hwnd = hwnd_of(&window);
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
        let next_weak = Rc::downgrade(&next);
        next.close_now();
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
        let hwnd = hwnd_of(&window);
        let weak = Rc::downgrade(&window);
        let observations = Arc::new(Mutex::new(Vec::new()));
        let callback_observations = Arc::clone(&observations);
        window.handler.react_to_state_change(move || {
            let window = weak.upgrade().expect("live moved window");
            let hwnd = hwnd_of(&window);
            let mut rect = windows::Win32::Foundation::RECT::default();
            // SAFETY: the upgraded wrapper owns this HWND on the callback's
            // creating thread; rect is a live correctly sized out-parameter.
            unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &raw mut rect) }
                .expect("window rectangle");
            let scale = scale_of(&*window);
            let actual = flui_foundation::geometry::Point::new(
                f64::from(rect.left) / scale,
                f64::from(rect.top) / scale,
            );
            callback_observations
                .lock()
                .expect("move observations")
                .push((actual, window.state().outer.position));
        });
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
            assert_eq!(
                Some(actual),
                observed,
                "callback saw truncated or stale origin"
            );
            assert_eq!(
                window.state().outer.position,
                Some(actual),
                "getter changed after native move"
            );
        }
        window.close_now();
    }

    #[expect(
        unsafe_code,
        reason = "owner-thread keyboard state and native character dispatch"
    )]
    fn shift_character_reaches_native_key_dispatch() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, true);
        let hwnd = hwnd_of(&window);
        let observed = Arc::new(Mutex::new(Vec::new()));
        let keys = observed.clone();
        window.handler.react_to_input(move |event| {
            if matches!(event, flui_platform::PlatformInput::Capabilities(_)) {
                return flui_platform::DispatchEventResult::default();
            }
            if let Some(event) = event.as_keyboard() {
                keys.lock().expect("keys").push(event.clone());
            }
            DispatchEventResult::resolved(true, false)
        });
        let _shift = ThreadKeyboardState::with_keys(&[
            (VK_SHIFT, true),
            (VK_LSHIFT, true),
            (VK_CONTROL, false),
            (VK_MENU, false),
        ]);
        // SAFETY: this platform owns the live HWND on this thread; WM_CHAR carries a scalar '+' and no caller pointers.
        unsafe {
            SendMessageW(
                hwnd,
                WM_CHAR,
                Some(WPARAM(0x2b)),
                Some(LPARAM(1 | (0x0d << 16))),
            );
        }
        assert!(
            observed.lock().expect("keys").iter().any(|event| event.key
                == flui_platform::keyboard::Key::character("+")
                && event
                    .modifiers
                    .contains(flui_platform::keyboard::Modifiers::SHIFT)),
            "native character translation retains the Shift that produced the character"
        );
        window.close_now();
    }

    fn closed_window_retires_tracking() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        for _ in 0..2 {
            let window = open(&platform, true);
            let weak = Rc::downgrade(&window);
            window.close_now();
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
        tracing::subscriber::with_default(subscriber, || {
            window.close_now();
        });
        assert_eq!(
            *destroyed.lock().expect("recorder lock"),
            [window.id().0],
            "closing one window traces its destruction, under its own identity, once"
        );
        sibling.close_now();
    }

    /// Records how many destructions were traced when it is dropped — the
    /// stand-in for the renderer surface a frame or resize callback owns.
    struct SurfaceRelease {
        destroyed: Arc<Mutex<Vec<u64>>>,
        traced_before_release: Arc<Mutex<Option<usize>>>,
    }

    impl Drop for SurfaceRelease {
        fn drop(&mut self) {
            let traced = self.destroyed.lock().expect("recorder lock").len();
            *self.traced_before_release.lock().expect("release slot") = Some(traced);
        }
    }

    /// A window closed from inside its own callback releases that callback
    /// (and the surface it owns) before its native destruction is traced,
    /// even though the release waits for the running callback to return.
    #[expect(unsafe_code, reason = "actual owned hidden Win32 resize")]
    fn reentrant_close_releases_the_surface_before_tracing_destruction() {
        use tracing_subscriber::layer::SubscriberExt as _;

        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, false);
        let hwnd = hwnd_of(&window);
        let destroyed = Arc::new(Mutex::new(Vec::new()));
        let traced_before_release = Arc::new(Mutex::new(None));
        let surface = SurfaceRelease {
            destroyed: Arc::clone(&destroyed),
            traced_before_release: Arc::clone(&traced_before_release),
        };
        /// A capture whose destructor tries to hand the handler a new
        /// reaction while the window lets go of that handler.
        struct InstallDuringRetirement {
            handler: std::rc::Weak<flui_testing::window_handler::RecordingWindowHandler>,
            probe: Option<SurfaceRelease>,
        }
        impl Drop for InstallDuringRetirement {
            fn drop(&mut self) {
                let probe = self.probe.take().expect("replacement capture");
                if let Some(handler) = self.handler.upgrade() {
                    handler.react_to_state_change(move || {
                        let _owned = &probe;
                    });
                }
            }
        }
        let replacement_release = Arc::new(Mutex::new(None));
        let install = InstallDuringRetirement {
            handler: Rc::downgrade(&window.handler),
            probe: Some(SurfaceRelease {
                destroyed: destroyed.clone(),
                traced_before_release: replacement_release.clone(),
            }),
        };
        let weak = Rc::downgrade(&window);
        window.handler.react_to_state_change(move || {
            let _owned = (&surface, &install);
            if let Some(window) = weak.upgrade() {
                window.close_now();
            }
        });
        let id = window.id().0;
        drop(window);
        let subscriber =
            tracing_subscriber::registry().with(DestroyedWindows(Arc::clone(&destroyed)));
        tracing::subscriber::with_default(subscriber, || {
            // SAFETY: the platform still tracks this window, so the HWND is
            // live on its creating thread; the resize carries no caller memory.
            unsafe {
                SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    200,
                    150,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                )
            }
            .expect("native resize");
        });
        assert_eq!(
            *traced_before_release.lock().expect("release slot"),
            Some(0),
            "the surface-owning handler is released before destruction is traced"
        );
        assert_eq!(
            *replacement_release.lock().expect("replacement release"),
            Some(0),
            "a capture that reaches for the retiring handler is released before the notice"
        );
        assert_eq!(
            *destroyed.lock().expect("recorder lock"),
            [id],
            "the reentrant close still traces its destruction once"
        );
    }

    fn close_callback_releases_external_owner() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, true);
        let weak = Rc::downgrade(&window);
        let owner = Rc::new(RefCell::new(Some(Rc::clone(&window))));
        let callback_owner = Rc::clone(&owner);
        window.handler.react_to_close(move || {
            drop(callback_owner.borrow_mut().take());
        });
        drop(window);
        weak.upgrade().expect("tracked window").close_now();
        assert!(owner.borrow().is_none());
        assert!(
            weak.upgrade().is_none(),
            "callback-released window remains tracked"
        );
        let next = open(&platform, true);
        next.close_now();
    }

    #[expect(
        unsafe_code,
        reason = "reads the DWM dark title-bar flag of an owned Win32 window"
    )]
    fn chrome_is_dark(hwnd: HWND) -> bool {
        use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmGetWindowAttribute};
        let mut dark: i32 = 0;
        // SAFETY: `hwnd` is a live window this test owns; the attribute writes a
        // 4-byte BOOL and the size states the width of the `i32` it points at.
        unsafe {
            DwmGetWindowAttribute(
                hwnd,
                DWMWINDOWATTRIBUTE(20),
                (&raw mut dark).cast(),
                size_of::<i32>() as u32,
            )
        }
        .expect("read the dark title-bar flag");
        dark != 0
    }

    #[expect(
        unsafe_code,
        reason = "inverts an owned window's DWM flag and sends it a settings broadcast"
    )]
    fn appearance_is_the_theme_the_chrome_follows_not_the_chrome_flag() {
        use flui_platform::Brightness;
        use flui_platform::WindowAppearance;
        use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmSetWindowAttribute};
        use windows::Win32::UI::WindowsAndMessaging::WM_SETTINGCHANGE;

        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, true);
        let hwnd = hwnd_of(&window);
        let reported: WindowAppearance = window.state().appearance;
        let dark = Brightness::from(reported).is_dark();
        assert_eq!(
            chrome_is_dark(hwnd),
            dark,
            "the title bar follows the theme"
        );

        // The flag is this backend's output, not the user's choice: inverting it
        // leaves the reported theme alone.
        let inverted = i32::from(!dark);
        // SAFETY: `hwnd` is a live window this test owns; the attribute reads a
        // 4-byte BOOL and the size states the width of the `i32` it points at.
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWINDOWATTRIBUTE(20),
                (&raw const inverted).cast(),
                size_of::<i32>() as u32,
            )
        }
        .expect("invert the dark title-bar flag");
        assert_eq!(
            window.state().appearance,
            reported,
            "read back the chrome flag"
        );

        // A settings broadcast that leaves the theme alone reports no change.
        let changes = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&changes);
        window.handler.react_to_state_change(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        });
        let section = windows::core::w!("ImmersiveColorSet");
        // SAFETY: a same-thread send to a live window this test owns; the
        // section name is a static NUL-terminated literal.
        unsafe {
            SendMessageW(
                hwnd,
                WM_SETTINGCHANGE,
                Some(WPARAM(0)),
                Some(LPARAM(section.as_ptr() as isize)),
            )
        };
        assert_eq!(
            changes.load(Ordering::SeqCst),
            0,
            "an unchanged theme published a change"
        );
        window.close_now();
    }

    #[expect(
        unsafe_code,
        reason = "actual owned hidden Win32 resize and client-dimension queries"
    )]
    fn resize_callback_preserves_large_native_dimensions() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, false);
        let hwnd = hwnd_of(&window);
        let weak = Rc::downgrade(&window);
        let observations = Arc::new(Mutex::new(Vec::new()));
        let callback_observations = Arc::clone(&observations);
        window.handler.react_to_state_change(move || {
            let Some(metrics) = weak.upgrade().and_then(|window| window.state().metrics) else {
                return;
            };
            let (size, scale) = (metrics.logical(), metrics.scale().get());
            let window = weak.upgrade().expect("live resize window");
            let hwnd = hwnd_of(&window);
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
                    window.state().content_size,
                    physical_of(&*window),
                ));
        });
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
                "the snapshot read in the callback corrupted native dimensions"
            );
            assert_eq!(physical, actual);
            assert_eq!(
                window.state().content_size,
                logical,
                "the snapshot corrupted after resize"
            );
        }
        window.close_now();
    }

    fn resize_callback_observes_current_client_bounds() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, true);
        let weak = Rc::downgrade(&window);
        let observations = Arc::new(Mutex::new(Vec::new()));
        let callback_observations = Arc::clone(&observations);
        window.handler.react_to_state_change(move || {
            let Some(metrics) = weak.upgrade().and_then(|window| window.state().metrics) else {
                return;
            };
            let (size, scale) = (metrics.logical(), metrics.scale().get());
            let window = weak.upgrade().expect("live resize window");
            callback_observations
                .lock()
                .expect("resize observations")
                .push((
                    size,
                    scale,
                    window.state().content_size,
                    physical_of(&*window),
                    scale_of(&*window),
                ));
        });
        for requested in [Size::new(500.0, 360.0), Size::new(440.0, 280.0)] {
            window
                .admit_and_apply(flui_platform::WindowCommand::SetSize(requested))
                .expect("the native window resizes");
            let values = observations.lock().expect("resize observations");
            let &(size, scale, observed, physical, observed_scale) =
                values.last().expect("native resize delivered");
            assert_eq!(size, observed, "the snapshot is stale inside its callback");
            assert_eq!(scale, observed_scale);
            assert!(
                (size.width - requested.width).abs() <= 1.0 / scale
                    && (size.height - requested.height).abs() <= 1.0 / scale,
                "the command sizes the client area, not the outer frame: asked {requested:?}, got {size:?}"
            );
            assert_eq!(
                window.state().content_size,
                size,
                "setter replaced native client bounds with requested outer size"
            );
            assert!((physical.width as f64 - size.width * scale).abs() <= 1.0);
            assert!((physical.height as f64 - size.height * scale).abs() <= 1.0);
        }
        window.close_now();
    }

    #[expect(
        unsafe_code,
        reason = "actual owned Win32 visibility query on its creating thread"
    )]
    fn hidden_popup_is_natively_hidden() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, false);
        // SAFETY: the live wrapper owns this HWND, queried on its creating thread.
        assert!(!unsafe { IsWindowVisible(hwnd_of(&window)) }.as_bool());
        assert!(!window.state().visible);
        window.close_now();
    }

    const MK_LBUTTON: usize = 0x0001;
    const MK_RBUTTON: usize = 0x0002;
    const MK_SHIFT: usize = 0x0004;

    fn hwnd_of(window: &Rc<dyn HostWindow>) -> HWND {
        crate::win32_handle(window.as_ref())
    }

    /// Every pointer event `window` delivers, in order.
    fn record_pointer(window: &Opened) -> Arc<Mutex<Vec<flui_platform::pointer::PointerEvent>>> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&events);
        window.handler.react_to_input(move |input| {
            if matches!(input, flui_platform::PlatformInput::Capabilities(_)) {
                return flui_platform::DispatchEventResult::default();
            }
            if let Some(event) = input.as_pointer() {
                recorded.lock().expect("pointer log").push(event.clone());
            }
            DispatchEventResult::default()
        });
        events
    }

    fn kinds(events: &Mutex<Vec<flui_platform::pointer::PointerEvent>>) -> Vec<&'static str> {
        use flui_platform::pointer::PointerEvent;
        events
            .lock()
            .expect("pointer log")
            .iter()
            .map(|event| match event {
                PointerEvent::Down(_) => "down",
                PointerEvent::Up(_) => "up",
                PointerEvent::ButtonChange(flui_platform::pointer::ButtonChange::Pressed(_)) => {
                    "button_down"
                }
                PointerEvent::ButtonChange(flui_platform::pointer::ButtonChange::Released(_)) => {
                    "button_up"
                }
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
            (
                windows::Win32::UI::Input::KeyboardAndMouse::VK_XBUTTON1,
                mask & 0x0020 != 0,
            ),
            (
                windows::Win32::UI::Input::KeyboardAndMouse::VK_XBUTTON2,
                mask & 0x0040 != 0,
            ),
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
        use flui_platform::pointer::PointerEvent;
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

        assert_eq!(kinds(&events), ["down", "button_down", "button_up", "up"]);
        let scale = scale_of(&**window);
        let log = events.lock().expect("pointer log");
        let PointerEvent::Up(last) = log.last().expect("last release") else {
            unreachable!("kinds checked above");
        };
        assert_eq!(
            (last.sample.position.get().x, last.sample.position.get().y),
            (-40.0 / scale, -30.0 / scale),
            "an outside release keeps its negative client coordinates"
        );
        drop(log);
        window.close_now();
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
        thief.close_now();
        window.close_now();
    }

    /// Losing focus or being minimised mid-press ends the sequence with a
    /// cancel, and the release that follows delivers nothing: the pointer
    /// it would name has ended.
    #[expect(
        unsafe_code,
        reason = "native focus and show commands for owned windows"
    )]
    fn focus_loss_and_minimising_end_the_held_press() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, true);
        let hwnd = hwnd_of(&window);
        let events = record_pointer(&window);

        send_mouse(hwnd, WM_LBUTTONDOWN, MK_LBUTTON, mouse_lparam(10, 10));
        // SAFETY: the fixture's live window on its owner thread; the message
        // carries no pointer.
        unsafe { SendMessageW(hwnd, WM_KILLFOCUS, None, None) };
        assert_eq!(kinds(&events), ["down", "cancel"], "focus loss cancels");
        send_mouse(hwnd, WM_LBUTTONUP, 0, mouse_lparam(10, 10));
        assert_eq!(
            kinds(&events),
            ["down", "cancel"],
            "the ended press's release delivers nothing"
        );
        assert!(captured().is_invalid(), "the release still lets go");

        events.lock().expect("pointer log").clear();
        send_mouse(hwnd, WM_LBUTTONDOWN, MK_LBUTTON, mouse_lparam(10, 10));
        assert_eq!(captured(), hwnd, "the next press captures again");
        // SAFETY: the fixture's live window; minimised without activation.
        let _ = unsafe {
            ShowWindow(
                hwnd,
                windows::Win32::UI::WindowsAndMessaging::SW_SHOWMINNOACTIVE,
            )
        };
        // SAFETY: a query of the fixture's live window.
        assert!(
            unsafe { IsIconic(hwnd) }.as_bool(),
            "the window is minimised"
        );
        assert_eq!(kinds(&events), ["down", "cancel"], "minimising cancels");
        send_mouse(hwnd, WM_LBUTTONUP, 0, mouse_lparam(10, 10));
        assert_eq!(kinds(&events), ["down", "cancel"]);
        window.close_now();
    }

    /// A touch held when the window loses focus ends with a cancel, and the
    /// moves and the lift that still arrive for it deliver nothing. Keys typed
    /// while it is held are never stamped before a reading delivered ahead of
    /// them.
    #[expect(
        unsafe_code,
        reason = "real native touch injection into an ephemeral shown test window"
    )]
    fn focus_loss_ends_a_touch_and_silences_its_tail() {
        use flui_platform::pointer::{PointerEvent, PointerKind};
        use windows::Win32::UI::{
            Controls::{
                CreateSyntheticPointerDevice, DestroySyntheticPointerDevice,
                HSYNTHETICPOINTERDEVICE, POINTER_FEEDBACK_NONE, POINTER_TYPE_INFO,
                POINTER_TYPE_INFO_0,
            },
            Input::Pointer::{
                InjectSyntheticPointerInput, POINTER_FLAG_DOWN, POINTER_FLAG_INCONTACT,
                POINTER_FLAG_INRANGE, POINTER_FLAG_UP, POINTER_FLAG_UPDATE, POINTER_FLAGS,
                POINTER_INFO, POINTER_TOUCH_INFO,
            },
            WindowsAndMessaging::{PT_TOUCH, WindowFromPoint},
        };
        struct Device(HSYNTHETICPOINTERDEVICE);
        impl Drop for Device {
            fn drop(&mut self) {
                // SAFETY: this fixture uniquely owns the device returned by Create.
                unsafe { DestroySyntheticPointerDevice(self.0) };
            }
        }
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let Some((window, _close)) = open_pointer_target(&platform) else {
            return;
        };
        let hwnd = hwnd_of(&window);
        let mut target = POINT { x: 40, y: 40 };
        // SAFETY: the live shown owner window and an initialized point.
        assert!(unsafe { ClientToScreen(hwnd, &raw mut target) }.as_bool());
        // SAFETY: a hit test of a screen point; no handle is retained.
        if unsafe { WindowFromPoint(target) } != hwnd {
            eprintln!("CANNOT_VERIFY focus-loss touch: the shown target is occluded");
            return;
        }
        // SAFETY: one ephemeral synthetic device, no borrowed native resources.
        let device = match unsafe {
            CreateSyntheticPointerDevice(PT_TOUCH, 1, POINTER_FEEDBACK_NONE)
        } {
            Ok(device) => Device(device),
            Err(error) => {
                eprintln!(
                    "CANNOT_VERIFY focus-loss touch: CreateSyntheticPointerDevice refused: {error}"
                );
                return;
            }
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        // Every timed delivery in order: a touch reading's time, or a key's.
        let timeline = Arc::new(Mutex::new(Vec::new()));
        let (recorded, timed) = (Arc::clone(&events), Arc::clone(&timeline));
        window.handler.react_to_input(move |input| {
            match &input {
                flui_platform::PlatformInput::Keyboard(key) => {
                    timed.lock().expect("timeline").push(("key", key.time));
                }
                flui_platform::PlatformInput::Pointer(event) => {
                    let time = match event {
                        PointerEvent::Down(press) => Some(press.sample.time),
                        PointerEvent::Move(update) => Some(update.current().time),
                        PointerEvent::Up(release) => Some(release.sample.time),
                        _ => None,
                    };
                    if let Some(time) = time {
                        timed.lock().expect("timeline").push(("touch", time));
                    }
                    recorded.lock().expect("pointer log").push(event.clone());
                }
                _ => {}
            }
            DispatchEventResult::default()
        });
        let place = |flags: POINTER_FLAGS, dx: i32| {
            let at = POINT {
                x: target.x + dx,
                y: target.y,
            };
            let packet = POINTER_TYPE_INFO {
                r#type: PT_TOUCH,
                Anonymous: POINTER_TYPE_INFO_0 {
                    touchInfo: POINTER_TOUCH_INFO {
                        pointerInfo: POINTER_INFO {
                            pointerType: PT_TOUCH,
                            pointerId: 1,
                            pointerFlags: flags,
                            ptPixelLocation: at,
                            ptPixelLocationRaw: at,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                },
            };
            // SAFETY: an initialized union arm matching the device's type; the
            // packet outlives the synchronous copy.
            unsafe { InjectSyntheticPointerInput(device.0, &[packet]) }
        };
        let settle = || {
            let limit = Instant::now() + Duration::from_millis(100);
            while Instant::now() < limit {
                pump_pointer_thread();
                std::thread::sleep(Duration::from_millis(2));
            }
        };
        let inject = |flags: POINTER_FLAGS, dx: i32| {
            let injected = place(flags, dx);
            settle();
            injected
        };
        let touch = || -> Vec<&'static str> {
            events
                .lock()
                .expect("pointer log")
                .iter()
                .filter_map(|event| match event {
                    PointerEvent::Down(press) if press.pointer.kind == PointerKind::Touch => {
                        Some("down")
                    }
                    PointerEvent::Move(update) if update.pointer.kind == PointerKind::Touch => {
                        Some("move")
                    }
                    PointerEvent::Up(release) if release.pointer.kind == PointerKind::Touch => {
                        Some("up")
                    }
                    PointerEvent::Cancel(cancel) if cancel.pointer.kind == PointerKind::Touch => {
                        Some("cancel")
                    }
                    _ => None,
                })
                .collect()
        };
        // The system delivers a synthetic contact's press with its next
        // frame, and lifts a contact no frame follows; the move keeps it
        // down.
        let held = POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT;
        if let Err(error) = inject(POINTER_FLAG_DOWN | held, 0) {
            eprintln!(
                "CANNOT_VERIFY focus-loss touch: InjectSyntheticPointerInput refused: {error}"
            );
            return;
        }
        inject(POINTER_FLAG_UPDATE | held, 2).expect("inject the held touch's move");
        let held_touch = touch();
        assert!(
            held_touch.first() == Some(&"down") && !held_touch.contains(&"up"),
            "the touch is held: {:?}",
            events.lock().expect("pointer log")
        );
        // A key typed right after a reading, in the same system tick, is
        // stamped from the tick's millisecond time, the reading from its
        // performance count: the key is still not placed before it.
        {
            use windows::Win32::UI::Input::KeyboardAndMouse::{
                INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
                SendInput, VK_F24,
            };
            let key = |flags| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_F24,
                        dwFlags: flags,
                        ..Default::default()
                    },
                },
            };
            // Each round: a move, the key as soon as the move is delivered,
            // likely inside the system tick the move was read in.
            for round in 0..8 {
                let delivered = timeline.lock().expect("timeline").len();
                place(POINTER_FLAG_UPDATE | held, 3 + round % 2).expect("inject a move");
                let limit = Instant::now() + Duration::from_millis(500);
                while timeline.lock().expect("timeline").len() == delivered
                    && Instant::now() < limit
                {
                    pump_pointer_thread();
                }
                // SAFETY: initialized keyboard union arms; the foreground
                // window is this row's own target.
                let sent = unsafe {
                    SendInput(
                        &[key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)],
                        size_of::<INPUT>() as i32,
                    )
                };
                assert_eq!(sent, 2, "SendInput");
                settle();
            }
            let timeline = timeline.lock().expect("timeline");
            let mut latest_touch = None;
            for &(kind, time) in timeline.iter() {
                if kind == "touch" {
                    latest_touch = latest_touch.max(Some(time));
                } else if let Some(touch) = latest_touch {
                    assert!(
                        time >= touch,
                        "a key is not stamped before the reading delivered ahead of it: {timeline:?}"
                    );
                }
            }
            assert!(
                timeline.iter().any(|(kind, _)| *kind == "key"),
                "the keys were delivered: {timeline:?}"
            );
        }
        let before = touch();
        // SAFETY: the fixture's live window on its owner thread; the message
        // carries no pointer.
        unsafe { SendMessageW(hwnd, WM_KILLFOCUS, None, None) };
        inject(POINTER_FLAG_UPDATE | held, 4).expect("inject the held touch's move");
        inject(POINTER_FLAG_UP, 4).expect("inject the touch's lift");
        let after = touch();
        assert_eq!(
            after[before.len()..],
            ["cancel"],
            "focus loss cancels the touch, and its tail is silent: {:?}",
            events.lock().expect("pointer log")
        );
        drop(device);
        window.close_now();
    }

    fn extended_mouse_buttons_share_contact_and_capture() {
        use flui_platform::pointer::{ButtonChange, PointerButton, PointerEvent};
        use windows::Win32::UI::WindowsAndMessaging::{WM_XBUTTONDOWN, WM_XBUTTONUP};
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, true);
        let hwnd = hwnd_of(&window);
        let events = record_pointer(&window);
        send_mouse(
            hwnd,
            WM_XBUTTONDOWN,
            (1 << 16) | 0x0020,
            mouse_lparam(10, 10),
        );
        assert_eq!(captured(), hwnd, "the first extended button captures");
        send_mouse(
            hwnd,
            WM_XBUTTONDOWN,
            (2 << 16) | 0x0060,
            mouse_lparam(10, 10),
        );
        send_mouse(hwnd, WM_XBUTTONUP, (1 << 16) | 0x0040, mouse_lparam(10, 10));
        assert_eq!(captured(), hwnd, "another extended button retains capture");
        send_mouse(hwnd, WM_XBUTTONUP, 2 << 16, mouse_lparam(-30, -20));
        assert!(
            captured().is_invalid(),
            "the final extended release lets go"
        );
        assert_eq!(kinds(&events), ["down", "button_down", "button_up", "up"]);
        let log = events.lock().expect("pointer log");
        let [
            PointerEvent::Down(first),
            PointerEvent::ButtonChange(ButtonChange::Pressed(second)),
            PointerEvent::ButtonChange(ButtonChange::Released(released)),
            PointerEvent::Up(last),
        ] = log.as_slice()
        else {
            panic!("the checked extended-button transitions");
        };
        assert_eq!(first.button(), PointerButton::BACK);
        assert_eq!(second.button(), PointerButton::FORWARD);
        assert!(second.buttons().contains(PointerButton::BACK));
        assert!(second.buttons().contains(PointerButton::FORWARD));
        assert!(!released.buttons().contains(PointerButton::BACK));
        assert!(released.buttons().contains(PointerButton::FORWARD));
        assert_eq!(last.button(), PointerButton::FORWARD);
        assert!(last.buttons().is_empty());
        assert_eq!(first.pointer.id, second.pointer.id);
        assert_eq!(first.pointer.id, released.pointer.id);
        assert_eq!(first.pointer.id, last.pointer.id);
        assert_eq!(first.sample.pressure, None, "a mouse reports no sensor");
        drop(log);
        window.close_now();
    }

    #[expect(
        unsafe_code,
        reason = "moves capture between actual owned hidden windows"
    )]
    fn extended_mouse_capture_loss_cancels_and_recovers() {
        use flui_platform::pointer::{CancelReason, PointerEvent};
        use windows::Win32::UI::WindowsAndMessaging::{WM_XBUTTONDOWN, WM_XBUTTONUP};
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, true);
        let thief = open(&platform, true);
        let hwnd = hwnd_of(&window);
        let events = record_pointer(&window);
        send_mouse(
            hwnd,
            WM_XBUTTONDOWN,
            (1 << 16) | 0x0020,
            mouse_lparam(10, 10),
        );
        assert_eq!(captured(), hwnd, "an extended press captures");
        let mut held = queue_state_for(0x0020);
        // SAFETY: another live window created by this fixture on this thread.
        unsafe { SetCapture(hwnd_of(&thief)) };
        held.restore();
        assert_eq!(kinds(&events), ["down", "cancel"]);
        {
            let log = events.lock().expect("pointer log");
            let PointerEvent::Cancel(cancel) = &log[1] else {
                panic!("capture loss must cancel");
            };
            assert_eq!(cancel.reason, CancelReason::CaptureLost);
        }
        // SAFETY: release this owner thread's fixture capture; no arguments.
        unsafe { ReleaseCapture() }.expect("release the thief's capture");
        send_mouse(
            hwnd,
            WM_XBUTTONDOWN,
            (2 << 16) | 0x0040,
            mouse_lparam(10, 10),
        );
        assert_eq!(captured(), hwnd, "a fresh press captures again");
        send_mouse(hwnd, WM_XBUTTONUP, 2 << 16, mouse_lparam(10, 10));
        assert_eq!(kinds(&events), ["down", "cancel", "down", "up"]);
        assert!(captured().is_invalid());
        thief.close_now();
        window.close_now();
    }

    fn unknown_native_pointer_messages_do_not_invent_contacts() {
        use windows::Win32::UI::WindowsAndMessaging::{
            WM_POINTERCAPTURECHANGED, WM_POINTERDOWN, WM_POINTERENTER, WM_POINTERLEAVE,
            WM_POINTERUP, WM_POINTERUPDATE,
        };
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, true);
        let hwnd = hwnd_of(&window);
        let events = record_pointer(&window);
        for message in [
            WM_POINTERDOWN,
            WM_POINTERUPDATE,
            WM_POINTERUP,
            WM_POINTERCAPTURECHANGED,
            WM_POINTERENTER,
            WM_POINTERLEAVE,
        ] {
            send_mouse(hwnd, message, 0xffff, mouse_lparam(10, 10));
        }
        assert!(events.lock().expect("pointer log").is_empty());
        send_mouse(hwnd, WM_LBUTTONDOWN, MK_LBUTTON, mouse_lparam(10, 10));
        send_mouse(hwnd, WM_LBUTTONUP, 0, mouse_lparam(10, 10));
        assert_eq!(kinds(&events), ["down", "up"], "ordinary input recovers");
        window.close_now();
    }

    #[expect(
        unsafe_code,
        reason = "real native pointer injection into an ephemeral shown test window"
    )]
    fn synthetic_touch_and_pen_reach_native_pointer_dispatch() {
        use flui_platform::pointer::{PenTool, PointerEvent, PointerKind};
        use windows::Win32::UI::{
            Controls::{
                CreateSyntheticPointerDevice, DestroySyntheticPointerDevice,
                HSYNTHETICPOINTERDEVICE, POINTER_FEEDBACK_NONE, POINTER_TYPE_INFO,
                POINTER_TYPE_INFO_0,
            },
            Input::Pointer::{
                InjectSyntheticPointerInput, POINTER_FLAG_DOWN, POINTER_FLAG_INCONTACT,
                POINTER_FLAG_INRANGE, POINTER_FLAG_UP, POINTER_FLAG_UPDATE, POINTER_INFO,
                POINTER_PEN_INFO, POINTER_TOUCH_INFO,
            },
            WindowsAndMessaging::{
                PEN_MASK_PRESSURE, PEN_MASK_ROTATION, PEN_MASK_TILT_X, PEN_MASK_TILT_Y, PT_PEN,
                PT_TOUCH, TOUCH_MASK_CONTACTAREA, TOUCH_MASK_PRESSURE, WindowFromPoint,
            },
        };
        struct Device(HSYNTHETICPOINTERDEVICE);
        impl Drop for Device {
            fn drop(&mut self) {
                // SAFETY: this fixture uniquely owns the device returned by Create.
                unsafe { DestroySyntheticPointerDevice(self.0) };
            }
        }
        fn unsupported_host(error: &windows::core::Error) -> bool {
            // Explicit host capability/permission refusals. Invalid arguments
            // and sequencing failures are fixture defects, not coverage skips.
            matches!(
                error.code().0 as u32,
                0x8007_0005 | 0x8007_0032 | 0x8007_0078 | 0x8000_4001
            )
        }
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let Some((window, _close)) = open_pointer_target(&platform) else {
            return;
        };
        let hwnd = hwnd_of(&window);
        pump_pointer_thread();
        let mut target = POINT { x: 40, y: 40 };
        // SAFETY: the live shown owner window and initialized point.
        assert!(unsafe { ClientToScreen(hwnd, &raw mut target) }.as_bool());
        // Injection uses actual hit testing, never hwndTarget as an override.
        if unsafe { WindowFromPoint(target) } != hwnd {
            eprintln!(
                "CANNOT_VERIFY synthetic pointer delivery: shown target is occluded or the desktop is unavailable"
            );
            window.close_now();
            return;
        }
        let events = record_pointer(&window);
        struct Hook(windows::Win32::UI::WindowsAndMessaging::HHOOK);
        impl Drop for Hook {
            fn drop(&mut self) {
                // SAFETY: the hook this row installed, removed once.
                let _ = unsafe { UnhookWindowsHookEx(self.0) };
            }
        }
        // SAFETY: a thread-local hook on the current thread whose procedure
        // reads only the message being retrieved; removed when the row ends.
        let _hook = Hook(
            unsafe {
                SetWindowsHookExW(
                    windows::Win32::UI::WindowsAndMessaging::WH_GETMESSAGE,
                    Some(observe_pen_press),
                    None,
                    GetCurrentThreadId(),
                )
            }
            .expect("install the pen press observer"),
        );
        PEN_PRESSES.with_borrow_mut(Vec::clear);
        for (native_kind, expected_kind) in [
            (PT_TOUCH, PointerKind::Touch),
            (PT_PEN, PointerKind::Pen { tool: PenTool::Tip }),
        ] {
            // SAFETY: one ephemeral synthetic device, no borrowed native resources.
            let device = match unsafe {
                CreateSyntheticPointerDevice(native_kind, 1, POINTER_FEEDBACK_NONE)
            } {
                Ok(device) => Device(device),
                Err(error) => {
                    assert!(
                        unsupported_host(&error),
                        "CreateSyntheticPointerDevice failed unexpectedly: {error}"
                    );
                    eprintln!(
                        "CANNOT_VERIFY synthetic {expected_kind:?}: CreateSyntheticPointerDevice refused: {error}"
                    );
                    continue;
                }
            };
            events.lock().expect("pointer log").clear();
            let mut supported = true;
            for (flags, pressure) in [
                (
                    POINTER_FLAG_DOWN | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT,
                    512,
                ),
                (
                    POINTER_FLAG_UPDATE | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT,
                    768,
                ),
                (POINTER_FLAG_UP, 0),
                (
                    POINTER_FLAG_DOWN | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT,
                    512,
                ),
                (POINTER_FLAG_UP, 0),
            ] {
                let mut current_origin = POINT { x: 40, y: 40 };
                // SAFETY: queries for this live ephemeral window do not redirect
                // or fabricate the native injected target.
                unsafe {
                    assert!(ClientToScreen(hwnd, &raw mut current_origin).as_bool());
                    assert_eq!(
                        (current_origin.x, current_origin.y),
                        (target.x, target.y),
                        "owned target must not move during injection"
                    );
                    assert_eq!(
                        WindowFromPoint(target),
                        hwnd,
                        "native injection must still hit the owned target"
                    );
                    assert_eq!(
                        windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow(),
                        hwnd,
                        "owned target must remain foreground during injection"
                    );
                }
                let info = POINTER_INFO {
                    pointerType: native_kind,
                    pointerId: 1,
                    pointerFlags: flags,
                    ptPixelLocation: target,
                    ptPixelLocationRaw: target,
                    ..Default::default()
                };
                let packet = if native_kind == PT_TOUCH {
                    let contact = RECT {
                        left: target.x - 10,
                        top: target.y - 15,
                        right: target.x + 10,
                        bottom: target.y + 15,
                    };
                    POINTER_TYPE_INFO {
                        r#type: native_kind,
                        Anonymous: POINTER_TYPE_INFO_0 {
                            touchInfo: POINTER_TOUCH_INFO {
                                pointerInfo: info,
                                touchMask: TOUCH_MASK_CONTACTAREA | TOUCH_MASK_PRESSURE,
                                pressure,
                                rcContact: contact,
                                rcContactRaw: contact,
                                ..Default::default()
                            },
                        },
                    }
                } else {
                    POINTER_TYPE_INFO {
                        r#type: native_kind,
                        Anonymous: POINTER_TYPE_INFO_0 {
                            penInfo: POINTER_PEN_INFO {
                                pointerInfo: info,
                                penMask: PEN_MASK_PRESSURE
                                    | PEN_MASK_ROTATION
                                    | PEN_MASK_TILT_X
                                    | PEN_MASK_TILT_Y,
                                pressure,
                                rotation: 180,
                                tiltX: 45,
                                tiltY: 0,
                                ..Default::default()
                            },
                        },
                    }
                };
                // SAFETY: initialized union arm matches type and device; the
                // packet slice remains alive for the synchronous native copy.
                if let Err(error) = unsafe { InjectSyntheticPointerInput(device.0, &[packet]) } {
                    assert!(
                        unsupported_host(&error),
                        "InjectSyntheticPointerInput failed unexpectedly: {error}"
                    );
                    eprintln!(
                        "CANNOT_VERIFY synthetic {expected_kind:?}: InjectSyntheticPointerInput refused: {error}"
                    );
                    supported = false;
                    break;
                }
                let limit = Instant::now() + Duration::from_millis(500);
                while Instant::now() < limit {
                    pump_pointer_thread();
                    std::thread::sleep(Duration::from_millis(2));
                }
                // SAFETY: read the actual hit target after native queue pumping.
                assert_eq!(
                    unsafe { WindowFromPoint(target) },
                    hwnd,
                    "owned target must remain hit-testable after native dispatch"
                );
            }
            if !supported {
                continue;
            }
            let log = events.lock().expect("pointer log");
            let downs: Vec<_> = log
                .iter()
                .filter_map(|event| {
                    if let PointerEvent::Down(press) = event {
                        (press.pointer.kind == expected_kind).then_some(press)
                    } else {
                        None
                    }
                })
                .collect();
            // Successful injection with no producer delivery is a regression,
            // not an unsupported-platform skip.
            assert_eq!(downs.len(), 2, "{expected_kind:?}: {log:?}");
            let ups: Vec<_> = log
                .iter()
                .filter_map(|event| {
                    if let PointerEvent::Up(release) = event {
                        (release.pointer.kind == expected_kind).then_some(release)
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(ups.len(), 2, "{expected_kind:?}: {log:?}");
            assert!(
                !log.iter().any(|event| match event {
                    PointerEvent::ButtonChange(flui_platform::pointer::ButtonChange::Pressed(
                        press,
                    )) => press.pointer.kind == expected_kind,
                    PointerEvent::ButtonChange(flui_platform::pointer::ButtonChange::Released(
                        release,
                    )) => release.pointer.kind == expected_kind,
                    _ => false,
                }),
                "single-button injection must not invent another press on ENTER: {log:?}"
            );
            for (down, up) in downs.iter().zip(&ups) {
                assert_eq!(down.pointer.kind, expected_kind);
                assert!(
                    down.pointer.device.is_some(),
                    "native source handle retained"
                );
                assert_eq!(down.pointer, up.pointer);
                assert!(
                    up.sample.time >= down.sample.time,
                    "native terminal sample cannot predate its admitted press: down={:?} up={:?}",
                    down.sample.time,
                    up.sample.time
                );
                assert_eq!(
                    down.sample
                        .pressure
                        .map(flui_platform::pointer::Pressure::get),
                    Some(0.5)
                );
                let point = down.sample.position.get();
                let expected = 40.0 / scale_of(&**window);
                if native_kind == PT_TOUCH {
                    assert_eq!((point.x, point.y), (expected, expected));
                } else {
                    // A pen is placed from its himetric position through its
                    // device's rectangles, which the system derived from the
                    // injected pixel: within that pixel, not on its corner.
                    let pixel = 1.0 / scale_of(&**window);
                    assert!(
                        (point.x - expected).abs() < pixel && (point.y - expected).abs() < pixel,
                        "the pen is at its injected pixel: {point:?}"
                    );
                }
            }
            if native_kind == PT_PEN {
                let mapped = PEN_PRESSES.with_borrow_mut(std::mem::take);
                assert_eq!(mapped.len(), downs.len(), "each pen press was observed");
                let mut origin = POINT::default();
                // SAFETY: the live shown owner window and an initialized point.
                assert!(unsafe { ClientToScreen(hwnd, &raw mut origin) }.as_bool());
                let scale = scale_of(&**window);
                for (down, (x, y)) in downs.iter().zip(mapped) {
                    let point = down.sample.position.get();
                    let (want_x, want_y) = (
                        (x - f64::from(origin.x)) / scale,
                        (y - f64::from(origin.y)) / scale,
                    );
                    assert!(
                        (point.x - want_x).abs() < 1e-6 && (point.y - want_y).abs() < 1e-6,
                        "the pen is at its himetric reading: {point:?}, mapped ({want_x}, {want_y})"
                    );
                }
            }
            assert_ne!(
                downs[0].pointer.id, downs[1].pointer.id,
                "fresh admission after Up"
            );
            assert!(log.iter().any(|event| matches!(event, PointerEvent::Move(movement) if movement.current().pressure.map(flui_platform::pointer::Pressure::get) == Some(0.75))), "actual update sensor reading: {log:?}");
            if native_kind == PT_TOUCH {
                let contact = downs[0]
                    .sample
                    .contact_size
                    .expect("injected contact area")
                    .get();
                assert_eq!(
                    (contact.width, contact.height),
                    (20.0 / scale_of(&**window), 30.0 / scale_of(&**window))
                );
            } else {
                let orientation = downs[0]
                    .sample
                    .orientation
                    .expect("reported both tilt axes");
                assert!(
                    (orientation.altitude().expect("altitude") - std::f64::consts::FRAC_PI_4).abs()
                        < 1e-12
                );
                assert_eq!(orientation.azimuth(), Some(0.0));
                assert!(
                    (downs[0].sample.twist.expect("rotation").radians() - std::f64::consts::PI)
                        .abs()
                        < 1e-12
                );
            }
        }
        window.close_now();
    }

    /// A touch and a pen injected into a real window under `native-capture`
    /// journal a native recording that replays, through the shared decoder,
    /// to exactly the events the window delivered, and to the shape of its
    /// golden `win32_injected_touch_and_pen.jsonl`
    /// (`native_recording::capture::hold_against_golden`).
    fn a_captured_native_pointer_session_replays_to_what_it_delivered() {
        #[cfg(all(debug_assertions, feature = "native-capture"))]
        captured_native_pointer_session();
        #[cfg(not(all(debug_assertions, feature = "native-capture")))]
        eprintln!("CANNOT_VERIFY native capture: needs a debug build with `native-capture`");
    }

    #[cfg(all(debug_assertions, feature = "native-capture"))]
    #[expect(
        unsafe_code,
        reason = "real native pointer injection into an ephemeral shown test window"
    )]
    fn captured_native_pointer_session() {
        use crate::native_recording::capture::record;
        use crate::native_recording::codec::Backend;
        use windows::Win32::UI::{
            Controls::{
                CreateSyntheticPointerDevice, DestroySyntheticPointerDevice, POINTER_FEEDBACK_NONE,
                POINTER_TYPE_INFO, POINTER_TYPE_INFO_0,
            },
            Input::Pointer::{
                InjectSyntheticPointerInput, POINTER_FLAG_DOWN, POINTER_FLAG_INCONTACT,
                POINTER_FLAG_INRANGE, POINTER_FLAG_UP, POINTER_FLAG_UPDATE, POINTER_INFO,
                POINTER_PEN_INFO, POINTER_TOUCH_INFO,
            },
            WindowsAndMessaging::{
                PEN_MASK_PRESSURE, PEN_MASK_TILT_X, PEN_MASK_TILT_Y, PT_PEN, PT_TOUCH,
                TOUCH_MASK_CONTACTAREA, TOUCH_MASK_PRESSURE, WindowFromPoint,
            },
        };
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let (delivered, recordings) = record(Backend::Win32, || {
            let (window, _close) = open_pointer_target(&platform)?;
            let hwnd = hwnd_of(&window);
            pump_pointer_thread();
            let mut target = POINT { x: 40, y: 40 };
            // SAFETY: the live shown owner window and an initialized point.
            assert!(unsafe { ClientToScreen(hwnd, &raw mut target) }.as_bool());
            // SAFETY: a hit test of a screen point; no handle is retained.
            if unsafe { WindowFromPoint(target) } != hwnd {
                eprintln!("CANNOT_VERIFY native capture: the shown target is occluded");
                return None;
            }
            let events = record_pointer(&window);
            for native_kind in [PT_TOUCH, PT_PEN] {
                // SAFETY: one ephemeral synthetic device, destroyed below.
                let device = match unsafe {
                    CreateSyntheticPointerDevice(native_kind, 1, POINTER_FEEDBACK_NONE)
                } {
                    Ok(device) => device,
                    Err(error) => {
                        eprintln!(
                            "CANNOT_VERIFY native capture: CreateSyntheticPointerDevice refused: {error}"
                        );
                        return None;
                    }
                };
                for (flags, dx, pressure) in [
                    (
                        POINTER_FLAG_DOWN | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT,
                        0,
                        512,
                    ),
                    (
                        POINTER_FLAG_UPDATE | POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT,
                        6,
                        768,
                    ),
                    (POINTER_FLAG_UP, 6, 0),
                ] {
                    let at = POINT {
                        x: target.x + dx,
                        y: target.y,
                    };
                    let info = POINTER_INFO {
                        pointerType: native_kind,
                        pointerId: 1,
                        pointerFlags: flags,
                        ptPixelLocation: at,
                        ptPixelLocationRaw: at,
                        ..Default::default()
                    };
                    let packet = if native_kind == PT_TOUCH {
                        let contact = RECT {
                            left: at.x - 10,
                            top: at.y - 15,
                            right: at.x + 10,
                            bottom: at.y + 15,
                        };
                        POINTER_TYPE_INFO {
                            r#type: native_kind,
                            Anonymous: POINTER_TYPE_INFO_0 {
                                touchInfo: POINTER_TOUCH_INFO {
                                    pointerInfo: info,
                                    touchMask: TOUCH_MASK_CONTACTAREA | TOUCH_MASK_PRESSURE,
                                    pressure,
                                    rcContact: contact,
                                    rcContactRaw: contact,
                                    ..Default::default()
                                },
                            },
                        }
                    } else {
                        POINTER_TYPE_INFO {
                            r#type: native_kind,
                            Anonymous: POINTER_TYPE_INFO_0 {
                                penInfo: POINTER_PEN_INFO {
                                    pointerInfo: info,
                                    penMask: PEN_MASK_PRESSURE | PEN_MASK_TILT_X | PEN_MASK_TILT_Y,
                                    pressure,
                                    tiltX: 30,
                                    tiltY: 0,
                                    ..Default::default()
                                },
                            },
                        }
                    };
                    // SAFETY: the union arm matches the type and the device;
                    // the packet outlives the synchronous native copy.
                    let injected = unsafe { InjectSyntheticPointerInput(device, &[packet]) };
                    if let Err(error) = injected {
                        // SAFETY: this fixture owns the device it created.
                        unsafe { DestroySyntheticPointerDevice(device) };
                        eprintln!(
                            "CANNOT_VERIFY native capture: InjectSyntheticPointerInput refused: {error}"
                        );
                        return None;
                    }
                    let limit = Instant::now() + Duration::from_millis(300);
                    while Instant::now() < limit {
                        pump_pointer_thread();
                        std::thread::sleep(Duration::from_millis(2));
                    }
                }
                // SAFETY: this fixture owns the device it created.
                unsafe { DestroySyntheticPointerDevice(device) };
            }
            let delivered = events.lock().expect("pointer log").clone();
            window.close_now();
            Some(delivered)
        });
        let Some(delivered) = delivered else {
            return;
        };
        let [recording] = recordings.as_slice() else {
            panic!("one window, one registry: {recordings:?}");
        };
        assert!(
            delivered
                .iter()
                .filter(|event| matches!(event, flui_platform::pointer::PointerEvent::Down(_)))
                .count()
                >= 2,
            "the injected touch and pen were delivered: {delivered:?}"
        );
        let replayed: Vec<String> = recording
            .replay()
            .iter()
            .map(|input| format!("{:?}", input.as_pointer().expect("pointer input")))
            .collect();
        let delivered: Vec<String> = delivered.iter().map(|event| format!("{event:?}")).collect();
        assert_eq!(
            replayed, delivered,
            "the journal replays to what the window delivered"
        );
        crate::native_recording::capture::hold_against_golden(
            "win32_injected_touch_and_pen",
            recording,
        );
    }

    /// Mouse buttons, a Control-held click and both wheels sent through
    /// `SendInput` to a real window under `native-capture` journal a native
    /// recording that replays to exactly the events the window delivered, and
    /// to the shape of its golden `win32_sent_mouse_and_wheel.jsonl`.
    fn a_captured_native_mouse_session_replays_to_what_it_delivered() {
        #[cfg(all(debug_assertions, feature = "native-capture"))]
        captured_native_mouse_session();
        #[cfg(not(all(debug_assertions, feature = "native-capture")))]
        eprintln!("CANNOT_VERIFY native capture: needs a debug build with `native-capture`");
    }

    #[cfg(all(debug_assertions, feature = "native-capture"))]
    #[expect(
        unsafe_code,
        reason = "real mouse and modifier input sent to an ephemeral foreground test window, with the cursor restored"
    )]
    fn captured_native_mouse_session() {
        use crate::native_recording::capture::{hold_against_golden, record};
        use crate::native_recording::codec::Backend;
        use flui_platform::pointer::PointerEvent;
        use windows::Win32::UI::{
            Input::KeyboardAndMouse::{
                INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT,
                KEYEVENTF_KEYUP, MOUSE_EVENT_FLAGS, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN,
                MOUSEEVENTF_LEFTUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL,
                MOUSEINPUT, SendInput,
            },
            WindowsAndMessaging::{
                GetCursorPos, GetForegroundWindow, SetCursorPos, WindowFromPoint,
            },
        };
        struct Cursor(POINT);
        impl Drop for Cursor {
            fn drop(&mut self) {
                // SAFETY: restore the position captured before this row.
                let _ = unsafe { SetCursorPos(self.0.x, self.0.y) };
            }
        }
        /// One step of the session: the cursor to a client point, or input.
        enum Step {
            Cursor(i32, i32),
            Mouse(MOUSE_EVENT_FLAGS, i32),
            Key(VIRTUAL_KEY, KEYBD_EVENT_FLAGS),
        }
        let mut original = POINT::default();
        // SAFETY: an initialized point; nothing is retained.
        unsafe { GetCursorPos(&raw mut original) }.expect("capture cursor for restoration");
        let _restore = Cursor(original);
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let (delivered, recordings) = record(Backend::Win32, || {
            let (window, _close) = open_pointer_target(&platform)?;
            let hwnd = hwnd_of(&window);
            let screen = |x: i32, y: i32| {
                let mut point = POINT { x, y };
                // SAFETY: the live shown owner window and an initialized point.
                assert!(unsafe { ClientToScreen(hwnd, &raw mut point) }.as_bool());
                point
            };
            for (x, y) in [(40, 40), (46, 40)] {
                // SAFETY: a hit test of a screen point; no handle is retained.
                if unsafe { WindowFromPoint(screen(x, y)) } != hwnd {
                    eprintln!("CANNOT_VERIFY native capture: the shown target is occluded");
                    return None;
                }
            }
            let events = record_pointer(&window);
            let (left_down, left_up) = (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP);
            let steps = [
                Step::Cursor(40, 40),
                // A click, then a second within the double-click time.
                Step::Mouse(left_down, 0),
                Step::Mouse(left_up, 0),
                Step::Mouse(left_down, 0),
                Step::Mouse(left_up, 0),
                // A drag with the secondary button chorded in.
                Step::Cursor(46, 40),
                Step::Mouse(left_down, 0),
                Step::Mouse(MOUSEEVENTF_RIGHTDOWN, 0),
                Step::Mouse(MOUSEEVENTF_RIGHTUP, 0),
                Step::Mouse(left_up, 0),
                // A Control-held click.
                Step::Key(VK_CONTROL, KEYBD_EVENT_FLAGS(0)),
                Step::Mouse(left_down, 0),
                Step::Mouse(left_up, 0),
                Step::Key(VK_CONTROL, KEYEVENTF_KEYUP),
                // A wheel detent away from the user, a tilt right, and a
                // half detent toward the user (a precise device).
                Step::Mouse(MOUSEEVENTF_WHEEL, 120),
                Step::Mouse(MOUSEEVENTF_HWHEEL, 120),
                Step::Mouse(MOUSEEVENTF_WHEEL, -60),
            ];
            let mut cursor = screen(40, 40);
            for step in steps {
                let mut now = POINT::default();
                // SAFETY: observes activation and an initialized point; no
                // handle is retained.
                let (foreground, placed) = unsafe {
                    (
                        GetForegroundWindow() == hwnd,
                        GetCursorPos(&raw mut now).is_ok() && now == cursor,
                    )
                };
                if !foreground || !(placed || matches!(step, Step::Cursor(..))) {
                    eprintln!(
                        "CANNOT_VERIFY native capture: the target lost the foreground, or another input moved the cursor, mid-session (foreground {foreground}, cursor {now:?}, expected {cursor:?})"
                    );
                    return None;
                }
                let input = match step {
                    Step::Cursor(x, y) => {
                        cursor = screen(x, y);
                        // SAFETY: a point checked against this shown target.
                        unsafe { SetCursorPos(cursor.x, cursor.y) }.expect("move the cursor");
                        None
                    }
                    Step::Mouse(flags, data) => Some(INPUT {
                        r#type: INPUT_MOUSE,
                        Anonymous: INPUT_0 {
                            mi: MOUSEINPUT {
                                mouseData: data.cast_unsigned(),
                                dwFlags: flags,
                                ..Default::default()
                            },
                        },
                    }),
                    Step::Key(key, flags) => Some(INPUT {
                        r#type: INPUT_KEYBOARD,
                        Anonymous: INPUT_0 {
                            ki: KEYBDINPUT {
                                wVk: key,
                                dwFlags: flags,
                                ..Default::default()
                            },
                        },
                    }),
                };
                if let Some(input) = input {
                    // SAFETY: an initialized union arm matching its type; the
                    // foreground window is this row's own target.
                    let sent = unsafe { SendInput(&[input], size_of::<INPUT>() as i32) };
                    if sent != 1 {
                        eprintln!("CANNOT_VERIFY native capture: SendInput was refused");
                        return None;
                    }
                }
                let limit = Instant::now() + Duration::from_millis(40);
                while Instant::now() < limit {
                    pump_pointer_thread();
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
            let mut now = POINT::default();
            // SAFETY: an initialized point; nothing is retained.
            if unsafe { GetCursorPos(&raw mut now) }.is_err() || now != cursor {
                eprintln!(
                    "CANNOT_VERIFY native capture: another input moved the cursor mid-session"
                );
                return None;
            }
            let delivered = events.lock().expect("pointer log").clone();
            window.close_now();
            Some(delivered)
        });
        let Some(delivered) = delivered else {
            return;
        };
        let [recording] = recordings.as_slice() else {
            panic!("one window, one registry: {recordings:?}");
        };
        let count =
            |wanted: fn(&PointerEvent) -> bool| delivered.iter().filter(|e| wanted(e)).count();
        assert_eq!(
            count(|event| matches!(event, PointerEvent::Down(_))),
            4,
            "four primary presses were delivered: {delivered:?}"
        );
        assert_eq!(
            count(|event| matches!(event, PointerEvent::Scroll(_))),
            3,
            "three wheel turns were delivered: {delivered:?}"
        );
        let replayed: Vec<String> = recording
            .replay()
            .iter()
            .map(|input| format!("{:?}", input.as_pointer().expect("pointer input")))
            .collect();
        let delivered: Vec<String> = delivered.iter().map(|event| format!("{event:?}")).collect();
        assert_eq!(
            replayed, delivered,
            "the journal replays to what the window delivered"
        );
        hold_against_golden("win32_sent_mouse_and_wheel", recording);
    }

    #[expect(
        unsafe_code,
        reason = "drains actual native pointer broker messages on the child window's owner thread"
    )]
    fn pump_pointer_thread() {
        for _ in 0..256 {
            let mut message = MSG::default();
            // SAFETY: this child owns every window on this thread, including
            // the platform's message-only broker. No HWND filter excludes it.
            if !unsafe { PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
                return;
            }
            // SAFETY: dispatch the actual message retrieved by this thread.
            unsafe {
                let _ = TranslateMessage(&raw const message);
                DispatchMessageW(&raw const message);
            }
        }
        panic!("native pointer child queue did not drain");
    }

    #[expect(
        unsafe_code,
        reason = "real native mouse input directed to an ephemeral hit-tested window with cursor restoration"
    )]
    fn native_mouse_wheels_keep_hover_identity_and_signed_units() {
        use flui_platform::pointer::{PointerEvent, PointerKind, ScrollUnit};
        use windows::Win32::UI::{
            Input::KeyboardAndMouse::{
                INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_WHEEL, MOUSEINPUT,
                SendInput,
            },
            WindowsAndMessaging::{GetCursorPos, SetCursorPos, WindowFromPoint},
        };
        struct Cursor(POINT);
        impl Drop for Cursor {
            fn drop(&mut self) {
                // SAFETY: restore the position captured before this child row.
                let _ = unsafe { SetCursorPos(self.0.x, self.0.y) };
            }
        }
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let Some((window, _close)) = open_pointer_target(&platform) else {
            return;
        };
        let hwnd = hwnd_of(&window);
        let mut target = POINT { x: 40, y: 40 };
        let mut original = POINT::default();
        // SAFETY: initialized points and a live owner-thread window.
        unsafe {
            assert!(ClientToScreen(hwnd, &raw mut target).as_bool());
            GetCursorPos(&raw mut original).expect("capture cursor for restoration");
        }
        let _restore = Cursor(original);
        if unsafe { WindowFromPoint(target) } != hwnd {
            eprintln!(
                "CANNOT_VERIFY native wheel: shown target is occluded or desktop unavailable"
            );
            window.close_now();
            return;
        }
        let events = record_pointer(&window);
        // SAFETY: this point was checked against our shown ephemeral target.
        unsafe { SetCursorPos(target.x, target.y) }.expect("move cursor to owned target");
        for _ in 0..25 {
            pump_translated(hwnd);
            std::thread::sleep(Duration::from_millis(2));
        }
        let hover = events
            .lock()
            .expect("pointer log")
            .iter()
            .find_map(|event| match event {
                PointerEvent::Move(movement) if movement.pointer.kind == PointerKind::Mouse => {
                    Some(movement.pointer)
                }
                PointerEvent::Enter(signal) if signal.pointer.kind == PointerKind::Mouse => {
                    Some(signal.pointer)
                }
                _ => None,
            })
            .expect("actual native mouse hover identity");
        events.lock().expect("pointer log").clear();
        for flags in [MOUSEEVENTF_WHEEL, MOUSEEVENTF_HWHEEL] {
            let input = INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        mouseData: 120,
                        dwFlags: flags,
                        ..Default::default()
                    },
                },
            };
            // SAFETY: initialized mouse union arm; cursor is over owned target.
            assert_eq!(
                unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) },
                1,
                "native wheel injection accepted"
            );
            for _ in 0..25 {
                pump_translated(hwnd);
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        let log = events.lock().expect("pointer log");
        let scrolls: Vec<_> = log
            .iter()
            .filter_map(|event| {
                if let PointerEvent::Scroll(scroll) = event {
                    Some(scroll)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            scrolls.len(),
            2,
            "exactly one dispatch per native wheel: {log:?}"
        );
        for (scroll, expected) in scrolls.iter().zip([(0.0, -1.0), (1.0, 0.0)]) {
            assert_eq!(
                scroll.pointer, hover,
                "native wheel retains actual hover metadata"
            );
            assert_eq!(scroll.delta.unit(), ScrollUnit::Detents);
            assert_eq!((scroll.delta.x(), scroll.delta.y()), expected);
            assert_eq!(scroll.phase, None, "a wheel tick supplies no gesture phase");
            let point = scroll.position.get();
            assert_eq!(
                (point.x, point.y),
                (40.0 / scale_of(&**window), 40.0 / scale_of(&**window))
            );
        }
        drop(log);
        window.close_now();
    }

    /// A mouse that reaches the window by `WM_POINTER` (the default under
    /// `EnableMouseInPointer`) hovers it: entering raises the hover change
    /// once and `is_hovered` follows, a drag released outside the client area
    /// still delivers its release, and leaving clears the hover. The cursor
    /// moves by injected input, never `SetCursorPos`, which the system
    /// reports as a legacy `WM_MOUSEMOVE`.
    #[expect(
        unsafe_code,
        reason = "real mouse input sent to an ephemeral foreground test window, with the cursor restored"
    )]
    fn a_pointer_routed_mouse_hovers_the_window() {
        use flui_platform::pointer::{PointerEvent, PointerKind};
        use windows::Win32::UI::{
            Input::KeyboardAndMouse::{
                INPUT, INPUT_0, INPUT_MOUSE, MOUSE_EVENT_FLAGS, MOUSEEVENTF_ABSOLUTE,
                MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE,
                MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT, SendInput,
            },
            WindowsAndMessaging::{
                GetCursorPos, GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
                SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SetCursorPos, WindowFromPoint,
            },
        };
        struct Cursor(POINT);
        impl Drop for Cursor {
            fn drop(&mut self) {
                // SAFETY: restore the position captured before this row.
                let _ = unsafe { SetCursorPos(self.0.x, self.0.y) };
            }
        }
        let mut original = POINT::default();
        // SAFETY: an initialized point; nothing is retained.
        unsafe { GetCursorPos(&raw mut original) }.expect("capture cursor for restoration");
        let _restore = Cursor(original);
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let Some((window, _close)) = open_pointer_target(&platform) else {
            return;
        };
        let hwnd = hwnd_of(&window);
        let screen = |x: i32, y: i32| {
            let mut point = POINT { x, y };
            // SAFETY: the live shown owner window and an initialized point.
            assert!(unsafe { ClientToScreen(hwnd, &raw mut point) }.as_bool());
            point
        };
        // SAFETY: argument-free metric queries.
        let desk = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        };
        let (inside, outside) = (screen(40, 40), screen(-40, 40));
        let on_desk = |point: POINT| {
            point.x >= desk.0
                && point.y >= desk.1
                && point.x < desk.0 + desk.2
                && point.y < desk.1 + desk.3
        };
        // SAFETY: hit tests of screen points; no handle is retained.
        let hits = |point: POINT| unsafe { WindowFromPoint(point) } == hwnd;
        if !hits(inside) || hits(outside) || !on_desk(outside) {
            eprintln!(
                "CANNOT_VERIFY pointer-routed hover: the target is occluded or has no desktop beside it"
            );
            return;
        }
        // Sends one mouse input and pumps; `false` when it was refused or
        // another input (a person's mouse) moved the cursor elsewhere.
        let expected = std::cell::Cell::new(outside);
        let send = |flags: MOUSE_EVENT_FLAGS, to: Option<POINT>| {
            if let Some(point) = to {
                expected.set(point);
            }
            let (dx, dy, flags) = match to {
                // Normalized virtual-desktop coordinates, rounded to the
                // pixel's centre.
                Some(point) => (
                    ((i64::from(point.x - desk.0) * 65535 + i64::from(desk.2) / 2)
                        / i64::from(desk.2 - 1).max(1)) as i32,
                    ((i64::from(point.y - desk.1) * 65535 + i64::from(desk.3) / 2)
                        / i64::from(desk.3 - 1).max(1)) as i32,
                    flags | MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                ),
                None => (0, 0, flags),
            };
            let input = INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx,
                        dy,
                        dwFlags: flags,
                        ..Default::default()
                    },
                },
            };
            // SAFETY: an initialized union arm matching its type; the
            // foreground window is this row's own target.
            let sent = unsafe { SendInput(&[input], size_of::<INPUT>() as i32) } == 1;
            let limit = Instant::now() + Duration::from_millis(60);
            while Instant::now() < limit {
                pump_pointer_thread();
                std::thread::sleep(Duration::from_millis(2));
            }
            let mut now = POINT::default();
            let want = expected.get();
            // SAFETY: an initialized point; nothing is retained.
            let placed = unsafe { GetCursorPos(&raw mut now) }.is_ok()
                && (now.x - want.x).abs() <= 1
                && (now.y - want.y).abs() <= 1;
            if !(sent && placed) {
                eprintln!(
                    "CANNOT_VERIFY pointer-routed hover: input refused or the cursor moved by other input (at {now:?}, expected {want:?})"
                );
            }
            sent && placed
        };
        // The cursor starts beside the window, so it enters by input.
        // SAFETY: a point checked to be on the desktop and off the target.
        unsafe { SetCursorPos(outside.x, outside.y) }.expect("place the cursor beside the target");
        for _ in 0..25 {
            pump_pointer_thread();
            std::thread::sleep(Duration::from_millis(2));
        }
        let hovers = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&hovers);
        let weak = Rc::downgrade(&window);
        let hovered = std::cell::Cell::new(window.state().hovered);
        window.handler.react_to_state_change(move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            let now = window.state().hovered;
            if hovered.replace(now) != now {
                observed.lock().expect("hover log").push(now);
            }
        });
        let events = record_pointer(&window);

        if !send(MOUSE_EVENT_FLAGS(0), Some(inside)) {
            return;
        }
        assert!(
            window.state().hovered,
            "the entered window reports its hover"
        );
        assert_eq!(
            *hovers.lock().expect("hover log"),
            [true],
            "entering raises one hover change"
        );
        if !send(MOUSE_EVENT_FLAGS(0), Some(screen(44, 40))) {
            return;
        }
        assert_eq!(
            *hovers.lock().expect("hover log"),
            [true],
            "moving within the window raises no further change"
        );

        // A drag released outside the client area still ends.
        if !(send(MOUSEEVENTF_LEFTDOWN, None)
            && send(MOUSE_EVENT_FLAGS(0), Some(outside))
            && send(MOUSEEVENTF_LEFTUP, None))
        {
            return;
        }
        let mouse_kinds: Vec<_> = events
            .lock()
            .expect("pointer log")
            .iter()
            .filter_map(|event| match event {
                PointerEvent::Down(press) if press.pointer.kind == PointerKind::Mouse => {
                    Some("down")
                }
                PointerEvent::Up(release) if release.pointer.kind == PointerKind::Mouse => {
                    Some("up")
                }
                PointerEvent::Cancel(cancel) if cancel.pointer.kind == PointerKind::Mouse => {
                    Some("cancel")
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            mouse_kinds,
            ["down", "up"],
            "the release outside the window ends the press: {:?}",
            events.lock().expect("pointer log")
        );
        let released_at = events
            .lock()
            .expect("pointer log")
            .iter()
            .find_map(|event| match event {
                PointerEvent::Up(release) => Some(release.sample.position.get()),
                _ => None,
            })
            .expect("the release");
        assert!(
            released_at.x < 0.0,
            "the release is where it happened, outside the client area: {released_at:?}"
        );

        // With no button held, the cursor beside the window has left it.
        if !send(MOUSE_EVENT_FLAGS(0), Some(screen(-44, 40))) {
            return;
        }
        // The system's leave can trail the move that caused it.
        let limit = Instant::now() + Duration::from_millis(500);
        while window.state().hovered && Instant::now() < limit {
            pump_pointer_thread();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(
            !window.state().hovered,
            "the left window reports no hover: {:?}",
            events.lock().expect("pointer log")
        );
        assert_eq!(
            *hovers.lock().expect("hover log"),
            [true, false],
            "leaving raises one hover change"
        );

        // A press dragged outside keeps no leave coming while it is held;
        // when its capture is taken (`WM_POINTERCAPTURECHANGED`, which no
        // pointer message follows) the hover is read from where the cursor is.
        if !(send(MOUSE_EVENT_FLAGS(0), Some(inside))
            && send(MOUSEEVENTF_LEFTDOWN, None)
            && send(MOUSE_EVENT_FLAGS(0), Some(outside)))
        {
            let _ = send(MOUSEEVENTF_LEFTUP, None);
            return;
        }
        assert!(
            window.state().hovered,
            "a held press dragged outside still hovers its window"
        );
        // The mouse's pointer id, as the held press's messages carry it.
        let mouse = (1..=256_u32).find(|&id| {
            let mut kind = windows::Win32::UI::WindowsAndMessaging::POINTER_INPUT_TYPE::default();
            // SAFETY: an initialized out value; an id naming no pointer is refused.
            unsafe { windows::Win32::UI::Input::Pointer::GetPointerType(id, &raw mut kind) }.is_ok()
                && kind == windows::Win32::UI::WindowsAndMessaging::PT_MOUSE
        });
        let Some(mouse) = mouse else {
            let _ = send(MOUSEEVENTF_LEFTUP, None);
            eprintln!("CANNOT_VERIFY pointer-routed hover: the mouse has no pointer id");
            return;
        };
        // SAFETY: the fixture's live window on its owner thread; the message
        // carries the mouse's pointer id and no pointer.
        unsafe {
            SendMessageW(
                hwnd,
                windows::Win32::UI::WindowsAndMessaging::WM_POINTERCAPTURECHANGED,
                Some(WPARAM(mouse as usize)),
                Some(LPARAM(0)),
            )
        };
        let after_loss = hovers.lock().expect("hover log").clone();
        // The system still holds this press for the window (the change was
        // sent, not taken): its release ends it here.
        let _ = send(MOUSEEVENTF_LEFTUP, None);
        assert_eq!(
            after_loss,
            [true, false, true, false],
            "a capture lost with the cursor outside ends the hover"
        );
        window.close_now();
    }

    #[expect(
        unsafe_code,
        reason = "queued native wheel packets target an actual owned hidden HWND"
    )]
    fn fractional_native_wheel_packets_preserve_observed_precision_and_source() {
        use flui_platform::pointer::{PointerEvent, ScrollPrecision, ScrollUnit};
        use windows::Win32::UI::WindowsAndMessaging::{WM_MOUSEHWHEEL, WM_MOUSEWHEEL};
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, false);
        let hwnd = hwnd_of(&window);
        let events = record_pointer(&window);
        pump_translated(hwnd);
        // SAFETY: this owner-thread HWND stays live until the end of the row;
        // the queued message contains only by-value mouse coordinates.
        unsafe { PostMessageW(Some(hwnd), WM_MOUSEMOVE, WPARAM(0), mouse_lparam(40, 40)) }
            .expect("queue actual hover source");
        pump_translated(hwnd);
        let source = events
            .lock()
            .expect("pointer log")
            .iter()
            .find_map(|event| {
                if let PointerEvent::Move(movement) = event {
                    Some(movement.pointer)
                } else {
                    None
                }
            })
            .expect("native window procedure supplied hover metadata");
        events.lock().expect("pointer log").clear();
        let mut screen = POINT { x: 40, y: 40 };
        // SAFETY: a writable POINT and this row's live HWND.
        assert!(unsafe { ClientToScreen(hwnd, &raw mut screen) }.as_bool());
        for (message, distance) in [
            (WM_MOUSEWHEEL, 30_i16),
            (WM_MOUSEWHEEL, -60),
            (WM_MOUSEHWHEEL, 30),
            (WM_MOUSEHWHEEL, -60),
            (WM_MOUSEWHEEL, 120),
            (WM_MOUSEHWHEEL, 120),
            (WM_MOUSEWHEEL, 0),
        ] {
            let packet = WPARAM(usize::from(distance.cast_unsigned()) << 16);
            // SAFETY: exact owned hidden HWND and native by-value wheel data;
            // no global injection or focus/cursor changes occur.
            unsafe {
                PostMessageW(
                    Some(hwnd),
                    message,
                    packet,
                    mouse_lparam(
                        i16::try_from(screen.x).expect("screen x fits native wheel coordinates"),
                        i16::try_from(screen.y).expect("screen y fits native wheel coordinates"),
                    ),
                )
            }
            .expect("queue native wheel packet");
            pump_translated(hwnd);
        }
        let log = events.lock().expect("pointer log");
        let scrolls: Vec<_> = log
            .iter()
            .filter_map(|event| {
                if let PointerEvent::Scroll(scroll) = event {
                    Some(scroll)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            scrolls.len(),
            7,
            "each native packet remains deliverable: {log:?}"
        );
        for (scroll, (x, y, precision)) in scrolls.iter().zip([
            (0.0, -0.25, ScrollPrecision::Precise),
            (0.0, 0.5, ScrollPrecision::Precise),
            (0.25, 0.0, ScrollPrecision::Precise),
            (-0.5, 0.0, ScrollPrecision::Precise),
            (0.0, -1.0, ScrollPrecision::Unknown),
            (1.0, 0.0, ScrollPrecision::Unknown),
            (0.0, 0.0, ScrollPrecision::Unknown),
        ]) {
            assert_eq!(
                scroll.pointer, source,
                "packet classification retains actual source"
            );
            assert_eq!(scroll.delta.unit(), ScrollUnit::Detents);
            assert_eq!((scroll.delta.x(), scroll.delta.y()), (x, y));
            assert_eq!(
                scroll.precision, precision,
                "observed granularity is not hardware identity"
            );
            assert_eq!(scroll.phase, None);
            assert_eq!(scroll.position.get().x, 40.0 / scale_of(&**window));
            assert_eq!(scroll.position.get().y, 40.0 / scale_of(&**window));
        }
        drop(log);
        window.close_now();
    }

    /// The mouse's `WM_POINTER` id while a mouse pointer message is current.
    #[expect(unsafe_code, reason = "queries pointer types by id")]
    fn mouse_pointer_id() -> Option<u32> {
        use windows::Win32::UI::{
            Input::Pointer::GetPointerType,
            WindowsAndMessaging::{POINTER_INPUT_TYPE, PT_MOUSE},
        };
        (1..=256_u32).find(|&id| {
            let mut kind = POINTER_INPUT_TYPE::default();
            // SAFETY: an initialized out value; an id naming no pointer is refused.
            unsafe { GetPointerType(id, &raw mut kind) }.is_ok() && kind == PT_MOUSE
        })
    }

    /// One `SendInput` mouse event, moving to the screen point `to` when it
    /// is given, then pumping; `false` when the input was refused.
    #[expect(unsafe_code, reason = "real mouse input to the foreground test window")]
    fn inject_mouse(
        flags: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS,
        to: Option<POINT>,
    ) -> bool {
        use windows::Win32::UI::{
            Input::KeyboardAndMouse::{
                INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_MOVE,
                MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT, SendInput,
            },
            WindowsAndMessaging::{
                GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
                SM_YVIRTUALSCREEN,
            },
        };
        // SAFETY: argument-free metric queries.
        let desk = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        };
        let (dx, dy, flags) = match to {
            Some(point) => (
                ((i64::from(point.x - desk.0) * 65535 + i64::from(desk.2) / 2)
                    / i64::from(desk.2 - 1).max(1)) as i32,
                ((i64::from(point.y - desk.1) * 65535 + i64::from(desk.3) / 2)
                    / i64::from(desk.3 - 1).max(1)) as i32,
                flags | MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
            ),
            None => (0, 0, flags),
        };
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    dwFlags: flags,
                    ..Default::default()
                },
            },
        };
        // SAFETY: an initialized union arm matching its type.
        let sent = unsafe { SendInput(&[input], size_of::<INPUT>() as i32) } == 1;
        let limit = Instant::now() + Duration::from_millis(60);
        while Instant::now() < limit {
            pump_pointer_thread();
            std::thread::sleep(Duration::from_millis(2));
        }
        sent
    }

    /// A mouse leave whose `Leave` callback pumps a message that brings the
    /// cursor back keeps the window hovered: the leave's hover is settled
    /// before user code sees its input, so nothing after the callback undoes
    /// what the nested message said.
    #[expect(
        unsafe_code,
        reason = "real mouse input to an ephemeral foreground test window, with the cursor restored"
    )]
    fn a_leave_callback_that_hovers_again_keeps_the_hover() {
        use flui_platform::pointer::{PointerEvent, PointerKind};
        use windows::Win32::UI::{
            Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS,
            WindowsAndMessaging::{GetCursorPos, SetCursorPos, WM_POINTERENTER, WindowFromPoint},
        };
        struct Cursor(POINT);
        impl Drop for Cursor {
            fn drop(&mut self) {
                // SAFETY: restore the position captured before this row.
                let _ = unsafe { SetCursorPos(self.0.x, self.0.y) };
            }
        }
        let mut original = POINT::default();
        // SAFETY: an initialized point; nothing is retained.
        unsafe { GetCursorPos(&raw mut original) }.expect("capture cursor for restoration");
        let _restore = Cursor(original);
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let Some((window, _close)) = open_pointer_target(&platform) else {
            return;
        };
        let hwnd = hwnd_of(&window);
        let screen = |x: i32, y: i32| {
            let mut point = POINT { x, y };
            // SAFETY: the live shown owner window and an initialized point.
            assert!(unsafe { ClientToScreen(hwnd, &raw mut point) }.as_bool());
            point
        };
        let (inside, outside) = (screen(40, 40), screen(-40, 40));
        // SAFETY: hit tests of screen points; no handle is retained.
        let hits = |point: POINT| unsafe { WindowFromPoint(point) } == hwnd;
        if !hits(inside) || hits(outside) || outside.x < 0 {
            eprintln!(
                "CANNOT_VERIFY reentrant leave: the target is occluded or at the screen edge"
            );
            return;
        }
        // SAFETY: a point checked to be off the target.
        unsafe { SetCursorPos(outside.x, outside.y) }.expect("place the cursor beside the target");
        for _ in 0..25 {
            pump_pointer_thread();
            std::thread::sleep(Duration::from_millis(2));
        }
        let raw = hwnd.0 as isize;
        let came_back = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&came_back);
        window.handler.react_to_input(move |input| {
            if let Some(PointerEvent::Leave(signal)) = input.as_pointer()
                && signal.pointer.kind == PointerKind::Mouse
                && seen.load(Ordering::SeqCst) == 0
                && let Some(mouse) = mouse_pointer_id()
            {
                seen.store(1, Ordering::SeqCst);
                // The callback pumps, and the message it dispatches is the
                // mouse entering again.
                // SAFETY: the window's own owner thread; the message carries
                // the mouse's pointer id, current while its leave is handled.
                unsafe {
                    SendMessageW(
                        HWND(raw as *mut _),
                        WM_POINTERENTER,
                        Some(WPARAM(mouse as usize)),
                        Some(LPARAM(0)),
                    )
                };
            }
            DispatchEventResult::default()
        });
        if !inject_mouse(MOUSE_EVENT_FLAGS(0), Some(inside)) || !window.state().hovered {
            eprintln!("CANNOT_VERIFY reentrant leave: the mouse did not enter by input");
            return;
        }
        let _ = inject_mouse(MOUSE_EVENT_FLAGS(0), Some(outside));
        let limit = Instant::now() + Duration::from_millis(500);
        while came_back.load(Ordering::SeqCst) == 0 && Instant::now() < limit {
            pump_pointer_thread();
            std::thread::sleep(Duration::from_millis(2));
        }
        if came_back.load(Ordering::SeqCst) == 0 {
            eprintln!("CANNOT_VERIFY reentrant leave: no mouse leave arrived");
            return;
        }
        assert!(
            window.state().hovered,
            "the mouse that entered again during the leave's callback still hovers"
        );
        window.close_now();
    }

    /// Delaying dispatch cannot compress the time between generated samples.
    #[expect(
        unsafe_code,
        reason = "queues and dispatches mouse messages for an owned window"
    )]
    fn queued_mouse_samples_keep_native_message_time() {
        use flui_platform::pointer::PointerEvent;
        let platform = WindowsPlatform::new().expect("native Windows platform");
        // A hidden native window excludes unrelated physical cursor traffic
        // while retaining the actual queue and window-procedure producer.
        let window = open(&platform, false);
        let hwnd = hwnd_of(&window);
        let events = record_pointer(&window);
        // Remove creation traffic before the two queued samples.
        pump_translated(hwnd);
        events.lock().expect("pointer log").clear();
        // SAFETY: an owned live HWND, with only integer mouse coordinates.
        unsafe { PostMessageW(Some(hwnd), WM_MOUSEMOVE, WPARAM(0), mouse_lparam(5, 5)) }
            .expect("queue first sample");
        std::thread::sleep(Duration::from_millis(40));
        // SAFETY: same owned window and by-value message data.
        unsafe { PostMessageW(Some(hwnd), WM_MOUSEMOVE, WPARAM(0), mouse_lparam(6, 6)) }
            .expect("queue second sample");
        std::thread::sleep(Duration::from_millis(40));
        let mut ticks = Vec::new();
        for _ in 0..32 {
            let mut message = MSG::default();
            // SAFETY: writable MSG and a live window on this owner thread.
            if !unsafe {
                PeekMessageW(
                    &raw mut message,
                    Some(hwnd),
                    WM_MOUSEMOVE,
                    WM_MOUSEMOVE,
                    PM_REMOVE,
                )
            }
            .as_bool()
            {
                break;
            }
            ticks.push(message.time);
            // SAFETY: dispatch the message just retrieved from this queue.
            unsafe { DispatchMessageW(&raw const message) };
        }
        assert_eq!(ticks.len(), 2, "two queued native samples");
        let native_gap = ticks[1].wrapping_sub(ticks[0]);
        assert!(
            native_gap >= 20,
            "samples were generated apart before dispatch"
        );
        let log = events.lock().expect("pointer log");
        let times: Vec<_> = log
            .iter()
            .filter_map(|event| match event {
                PointerEvent::Move(update) => Some(update.current().time.as_nanos()),
                _ => None,
            })
            .collect();
        assert_eq!(times.len(), 2, "both samples delivered");
        assert_eq!(
            times[1] - times[0],
            u64::from(native_gap) * 1_000_000,
            "event timestamps preserve queue generation time rather than dispatch time"
        );
        drop(log);
        window.close_now();
    }

    /// A key carries its message's generation time on the clock the mouse
    /// uses, so a key and a click queued behind a long frame keep their order
    /// and spacing however late they are dispatched.
    #[expect(
        unsafe_code,
        reason = "queues and dispatches key and mouse messages for an owned window"
    )]
    fn queued_key_events_keep_native_message_time() {
        use flui_platform::PlatformInput;
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let window = open(&platform, false);
        let hwnd = hwnd_of(&window);
        pump_translated(hwnd);
        let times = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&times);
        window.handler.react_to_input(move |input| {
            let time = match &input {
                PlatformInput::Keyboard(key) => Some(key.time),
                PlatformInput::Pointer(flui_platform::pointer::PointerEvent::Move(update)) => {
                    Some(update.current().time)
                }
                _ => None,
            };
            if let Some(time) = time {
                recorded.lock().expect("time log").push(time.as_nanos());
            }
            DispatchEventResult::default()
        });
        // 'A', scan code 0x1e: a key down, a mouse move, the key up.
        let a_down = LPARAM(1 | (0x1e << 16));
        let a_up = LPARAM(1 | (0x1e << 16) | (1 << 30) | (1 << 31));
        for (message, wparam, lparam) in [
            (WM_KEYDOWN, WPARAM(0x41), a_down),
            (WM_MOUSEMOVE, WPARAM(0), mouse_lparam(5, 5)),
            (WM_KEYUP, WPARAM(0x41), a_up),
        ] {
            // SAFETY: an owned live HWND, with by-value message data.
            unsafe { PostMessageW(Some(hwnd), message, wparam, lparam) }.expect("queue a message");
            std::thread::sleep(Duration::from_millis(40));
        }
        let mut ticks = Vec::new();
        for _ in 0..32 {
            let mut message = MSG::default();
            // SAFETY: writable MSG and a live window on this owner thread.
            if !unsafe { PeekMessageW(&raw mut message, Some(hwnd), 0, 0, PM_REMOVE) }.as_bool() {
                break;
            }
            if matches!(message.message, WM_KEYDOWN | WM_MOUSEMOVE | WM_KEYUP) {
                ticks.push(message.time);
            }
            // SAFETY: dispatch the message just retrieved from this queue.
            unsafe { DispatchMessageW(&raw const message) };
        }
        assert_eq!(ticks.len(), 3, "three queued native messages");
        let times = times.lock().expect("time log").clone();
        assert_eq!(times.len(), 3, "each message delivered one timed input");
        for step in 0..2 {
            let native_gap = ticks[step + 1].wrapping_sub(ticks[step]);
            assert!(native_gap >= 20, "messages were generated apart");
            assert_eq!(
                times[step + 1] - times[step],
                u64::from(native_gap) * 1_000_000,
                "input {step} to {}: timestamps are the queue's generation times, not dispatch times",
                step + 1
            );
        }
        window.close_now();
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
        let modifiers: Vec<flui_platform::keyboard::Modifiers> = log
            .iter()
            .map(|event| match event {
                flui_platform::pointer::PointerEvent::Move(update) => update.modifiers,
                _ => unreachable!("only moves were sent"),
            })
            .collect();
        assert_eq!(
            modifiers,
            [
                flui_platform::keyboard::Modifiers::SHIFT,
                flui_platform::keyboard::Modifiers::NONE
            ]
        );
        drop(log);
        window.close_now();
    }

    /// Runs `platform`'s message loop with `ready` as its bootstrap, which
    /// sets what the handler answers; returns once the loop ends. A loop that
    /// never ends is the child's timeout.
    fn run_loop(
        platform: WindowsPlatform,
        ready: impl FnOnce(&crate::app::AppLog, OwnerPlatform) + 'static,
    ) {
        Box::new(platform)
            .run(crate::app::with_log(move |log, owner| {
                ready(log, owner);
                Ok(())
            }))
            .expect("Win32 message loop");
    }

    fn open_owned(owner: &OwnerPlatform) -> Rc<dyn HostWindow> {
        owner
            .open_window(
                WindowOptions::default()
                    .with_visible(false)
                    .with_size(Size::new(320.0, 240.0)),
                flui_testing::window_handler::RecordingWindowHandler::new(),
            )
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
    fn post_close(window: &Rc<dyn HostWindow>) {
        let hwnd = hwnd_of(window);
        // SAFETY: the platform still tracks this live HWND; WM_CLOSE carries
        // no message data and the post dereferences no caller memory.
        unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) }
            .expect("post native close");
    }

    fn closing_the_last_window_ends_the_loop() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        run_loop(platform, |log, owner| {
            log.answer_exit(true);
            post_close(&open_owned(&owner));
        });
    }

    fn exit_policy_veto_holds_until_a_reevaluation_allows_exit() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let answers = Arc::new(Mutex::new(Vec::new()));
        let releaser = Arc::new(Mutex::new(None));
        run_loop(platform, {
            let answers = Arc::clone(&answers);
            let releaser = Arc::clone(&releaser);
            move |log, owner| {
                let allow = Arc::new(AtomicBool::new(false));
                let shared = owner.shared();
                log.answer_exit_with({
                    let allow = Arc::clone(&allow);
                    let answers = Arc::clone(&answers);
                    move || {
                        let answer = allow.load(Ordering::SeqCst);
                        answers.lock().expect("hook answers").push(answer);
                        answer
                    }
                });
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

    fn panicking_exit_policy_vetoes_and_is_asked_again() {
        let platform = WindowsPlatform::new().expect("native Windows platform");
        let calls = Arc::new(AtomicUsize::new(0));
        let releaser = Arc::new(Mutex::new(None));
        run_loop(platform, {
            let calls = Arc::clone(&calls);
            let releaser = Arc::clone(&releaser);
            move |log, owner| {
                let shared = owner.shared();
                log.answer_exit_with({
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
                });
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
            move |log, owner| {
                // The first consultation opens (and queues the close of) a
                // replacement window yet allows exit; the loop must outlive
                // that answer and end only after the replacement closes.
                log.answer_exit_with(move || {
                    if consultations.fetch_add(1, Ordering::SeqCst) == 0 {
                        OWNER.with(|slot| {
                            let slot = slot.borrow();
                            post_close(&open_owned(slot.as_ref().expect("owner platform")));
                        });
                    }
                    true
                });
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

/// AppKit's nested-pump row of ADR-0207 §7. A `keyDown:` AppKit delivers to
/// the content view while the window's input callback runs (a modal run loop
/// pumped from inside it) is queued behind the running callback and
/// delivered once, after it returns. AppKit's `keyDown:` takes no answer, and
/// the backend implements no `performKeyEquivalent:`, so no AppKit key needs
/// a synchronous one.
///
/// Type-checked on every host by `cargo check --target aarch64-apple-darwin`;
/// it runs only in an AppKit-run-loop-pumping macOS process.
#[cfg(target_os = "macos")]
mod native_appkit {
    use flui_testing::window_commands::AdmitAndApply as _;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };

    use flui_platform::WindowOptions;
    use flui_platform::{DispatchEventResult, PlatformInput};
    use objc2::{class, msg_send, runtime::AnyObject};
    use objc2_foundation::{NSPoint, NSString};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    /// A `keyDown:` NSEvent for the `k` key, the press AppKit would deliver.
    #[expect(unsafe_code, reason = "constructing a real NSEvent through AppKit")]
    fn key_event() -> *mut AnyObject {
        let characters = NSString::from_str("k");
        // SAFETY: a class-method constructor with plain arguments; the event
        // is autoreleased and outlives this test's synchronous use of it.
        unsafe {
            msg_send![
                class!(NSEvent),
                keyEventWithType: 10_usize,
                location: NSPoint::new(0.0, 0.0),
                modifierFlags: 0_usize,
                timestamp: 1.0_f64,
                windowNumber: 0_isize,
                context: std::ptr::null_mut::<AnyObject>(),
                characters: &*characters,
                charactersIgnoringModifiers: &*characters,
                isARepeat: false,
                keyCode: 40_u16
            ]
        }
    }

    #[test]
    #[ignore = "requires an AppKit-run-loop-pumping test process (ADR-0039)"]
    #[expect(
        unsafe_code,
        reason = "native keyDown: delivery to the real content view"
    )]
    fn a_reentrant_event_needing_an_answer_is_unhandled_and_delivered_once() {
        let platform = flui_native::current_platform().expect("AppKit platform");
        let window = crate::Opened::on(&*platform, WindowOptions::default().with_visible(false))
            .expect("AppKit window");
        let target = window.surface_target();
        let RawWindowHandle::AppKit(handle) =
            target.window_handle().expect("live window handle").as_raw()
        else {
            panic!("an AppKit window handle");
        };
        let view = handle.ns_view.as_ptr().cast::<AnyObject>() as usize;
        let log = Arc::new(Mutex::new(Vec::new()));
        let nested = Arc::new(AtomicBool::new(false));
        let (sink, inside) = (Arc::clone(&log), Arc::clone(&nested));
        window.handler.react_to_input(move |input| {
            let PlatformInput::Keyboard(_) = input else {
                return DispatchEventResult::default();
            };
            let first = sink.lock().expect("log").is_empty();
            sink.lock()
                .expect("log")
                .push(if inside.load(Ordering::SeqCst) {
                    "nested"
                } else {
                    "key"
                });
            if first {
                inside.store(true, Ordering::SeqCst);
                // SAFETY: the content view is live on this main thread; a
                // second `keyDown:` reaches it while this callback runs.
                unsafe {
                    let _: () = msg_send![view as *mut AnyObject, keyDown: key_event()];
                }
                inside.store(false, Ordering::SeqCst);
            }
            DispatchEventResult::default()
        });
        // SAFETY: as above, outside any callback.
        unsafe {
            let _: () = msg_send![view as *mut AnyObject, keyDown: key_event()];
        }
        assert_eq!(
            *log.lock().expect("log"),
            ["key", "key"],
            "the nested press is queued, never run inside the callback, and delivered once"
        );
        window.close_now();
    }
}

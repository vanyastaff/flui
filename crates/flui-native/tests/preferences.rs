//! Preference-source contracts; no test changes the user's OS settings.

fn unavailable_observations_do_not_claim_system_defaults() {
    let platform = flui_native::headless_platform();
    let observed = platform.preferences().expect("headless observation");
    assert!(observed.text_scale().is_none());
    assert!(observed.motion().is_none());
    assert!(observed.high_contrast().is_none());
    assert!(observed.locales().is_none());
    assert!(observed.gestures().double_click_interval().is_none());
    assert!(observed.wheel().vertical().is_none());
    assert!(observed.wheel().horizontal_characters().is_none());
}

#[cfg(target_os = "windows")]
#[allow(
    unsafe_code,
    reason = "compare the live presentation query with independent Win32 metrics"
)]
fn windows_reads_preferences_before_a_user_window_exists() {
    use flui_native::Platform;
    use windows::UI::ViewManagement::UISettings;

    // A later host must not reuse an activation factory from a retired apartment.
    {
        let previous = flui_native::WindowsPlatform::new().expect("previous native platform");
        previous.preferences().expect("previous native observation");
    }
    let platform = flui_native::WindowsPlatform::new().expect("native platform");
    let refused = std::thread::scope(|scope| {
        scope
            .spawn(|| platform.preferences())
            .join()
            .expect("foreign reader returned without panic")
    });
    assert!(
        matches!(refused, Err(flui_native::PlatformError::Preferences { .. })),
        "a foreign thread must not create an independent native observation set"
    );
    let native_ui = UISettings::new().expect("independent native preference reader");
    for _ in 0..3 {
        let observed = platform
            .preferences()
            .expect("native preference observation");
        assert!(
            observed
                .text_scale()
                .is_some_and(|scale| scale.is_finite() && scale > 0.0)
        );
        assert_eq!(
            observed
                .motion()
                .map(|motion| motion == flui_platform::MotionPreference::NoPreference),
            Some(
                native_ui
                    .AnimationsEnabled()
                    .expect("native animation preference")
            ),
            "the public observation must match Windows, not just contain a value"
        );
        assert!(observed.high_contrast().is_some());
        assert!(
            observed
                .locales()
                .is_some_and(|languages| !languages.is_empty()),
            "Windows must observe its ordered UI languages before a user window exists"
        );
        assert!(observed.gestures().double_click_interval().is_some());
        assert!(observed.gestures().double_tap_interval().is_none());
        assert!(observed.gestures().native_mouse_geometry().is_some());
        assert!(observed.wheel().vertical().is_some());
        assert!(observed.wheel().horizontal_characters().is_some());
    }
    // Repeated sampling must preserve the platform's outer COM lifetime so
    // a subsequent real native window can still initialize its text services.
    let window = platform
        .open_window(
            flui_platform::WindowOptions::default().with_visible(false),
            flui_testing::window_handler::RecordingWindowHandler::new(),
        )
        .expect("native window after preference reads");
    let projected = window
        .gesture_geometry()
        .expect("native geometry query")
        .expect("Windows observes mouse geometry");
    use flui_testing::window_commands::AdmitAndApply as _;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::{
        Foundation::HWND,
        UI::{
            HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi},
            WindowsAndMessaging::{SM_CXDOUBLECLK, SM_CXDRAG, SM_CYDOUBLECLK, SM_CYDRAG},
        },
    };
    let surface = window.surface_target();
    let RawWindowHandle::Win32(handle) = surface.window_handle().expect("live handle").as_raw()
    else {
        panic!("native Win32 handle")
    };
    let hwnd = HWND(handle.hwnd.get() as *mut _);
    // SAFETY: the owning window remains live on this thread for these reads.
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    let ratio = f64::from(dpi) / 96.0;
    // SAFETY: each index is a documented mouse metric, at the live window DPI.
    let (area, drag) = unsafe {
        (
            flui_foundation::geometry::Size::new(
                f64::from(GetSystemMetricsForDpi(SM_CXDOUBLECLK, dpi)) / ratio,
                f64::from(GetSystemMetricsForDpi(SM_CYDOUBLECLK, dpi)) / ratio,
            ),
            flui_foundation::geometry::Size::new(
                f64::from(GetSystemMetricsForDpi(SM_CXDRAG, dpi)).abs() / ratio,
                f64::from(GetSystemMetricsForDpi(SM_CYDRAG, dpi)).abs() / ratio,
            ),
        )
    };
    assert_eq!(projected.pixel_ratio().get(), ratio);
    assert_eq!(projected.mouse_double_click_area(), Some(area));
    assert_eq!(projected.mouse_drag_tolerance(), Some(drag));
    assert!(projected.touch_slop().is_none());
    window.close_now();
    assert_eq!(
        window.gesture_geometry(),
        Err(flui_platform::PreferenceQueryError::Unsupported(
            flui_platform::Unsupported::new(flui_platform::UnsupportedReason::NoWindow)
        ))
    );
}

/// A change that wakes the owner while no owner-turn callback is registered
/// (the loop already runs; the application has not installed its callback yet,
/// or is between two) is owed to the first callback that is: one turn runs it.
fn a_change_with_no_turn_callback_reaches_the_next_one_registered() {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_native::{HeadlessPlatform, Platform as _};

    let platform = HeadlessPlatform::new();
    let turns = platform.owner_turns();
    let source = platform.preference_source();
    let saved = Rc::new(RefCell::new(None));
    let installed = Rc::clone(&saved);
    Box::new(platform)
        .run(crate::app::on_ready(move |owner| {
            *installed.borrow_mut() = Some(owner);
            Ok(())
        }))
        .expect("headless owner");
    let owner = saved.borrow_mut().take().expect("started owner");

    source.set(
        flui_platform::SystemPreferences::default()
            .with_text_scale(2.0)
            .expect("valid scale"),
    );
    turns.drive();

    let runs = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&runs);
    owner
        .on_wake(Box::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        }))
        .expect("register the owner's turn callback");
    turns.drive();

    assert_eq!(
        runs.load(Ordering::SeqCst),
        1,
        "the change that found no callback is owed to the first one registered"
    );
    assert_eq!(
        owner
            .preferences()
            .expect("headless observation")
            .text_scale(),
        Some(2.0)
    );
    turns.drive();
    assert_eq!(runs.load(Ordering::SeqCst), 1, "and is owed only once");
}

#[test]
fn preferences_contract() {
    crate::run_table(
        "preferences_contract",
        &[
            (
                "unavailable_observations_do_not_claim_system_defaults",
                unavailable_observations_do_not_claim_system_defaults as fn(),
            ),
            (
                "a_change_with_no_turn_callback_reaches_the_next_one_registered",
                a_change_with_no_turn_callback_reaches_the_next_one_registered as fn(),
            ),
            #[cfg(target_os = "windows")]
            (
                "windows_reads_preferences_before_a_user_window_exists",
                windows_reads_preferences_before_a_user_window_exists as fn(),
            ),
            #[cfg(target_os = "windows")]
            (
                "setting_messages_wake_the_owner_without_a_user_window",
                setting_messages_wake_the_owner_without_a_user_window as fn(),
            ),
        ],
    );
}

#[cfg(target_os = "windows")]
#[allow(
    unsafe_code,
    reason = "exercise native notification routing through only this platform's HWNDs"
)]
fn setting_messages_wake_the_owner_without_a_user_window() {
    use flui_native::Platform;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    use windows::Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        System::Threading::GetCurrentThreadId,
        UI::WindowsAndMessaging::{EnumThreadWindows, SendMessageW, WM_SETTINGCHANGE},
    };

    fn top_levels() -> Vec<isize> {
        unsafe extern "system" fn collect(hwnd: HWND, parameter: LPARAM) -> windows::core::BOOL {
            // SAFETY: EnumThreadWindows receives this live vector exclusively
            // for the duration of its synchronous enumeration on this thread.
            unsafe { &mut *(parameter.0 as *mut Vec<isize>) }.push(hwnd.0 as isize);
            windows::core::BOOL(1)
        }
        let mut result = Vec::new();
        // SAFETY: collect only appends to the live local passed here.
        unsafe {
            EnumThreadWindows(
                GetCurrentThreadId(),
                Some(collect),
                LPARAM((&raw mut result) as isize),
            )
            .ok()
            .expect("enumerate owned thread windows");
        }
        result
    }

    let before = top_levels();
    let platform = flui_native::WindowsPlatform::new().expect("native platform");
    let receivers: Vec<_> = top_levels()
        .into_iter()
        .filter(|hwnd| !before.contains(hwnd))
        .collect();
    let woke = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&woke);
    let (finished, wait) = mpsc::channel();
    let (watchdog, watchdog_result) = mpsc::channel();
    Box::new(platform)
        .run(crate::app::on_ready(move |owner| {
            owner.preferences().expect("initial native observation");
            let quit = owner.proxy();
            owner
                .on_wake(Box::new(move || {
                    observed.store(true, Ordering::Release);
                    quit.request_quit().expect("quit after preference wake");
                }))
                .expect("register owner wake");
            let timeout = owner.proxy();
            watchdog
                .send(std::thread::spawn(move || {
                    if wait
                        .recv_timeout(std::time::Duration::from_secs(3))
                        .is_err()
                    {
                        let _ = timeout.request_quit();
                    }
                }))
                .expect("retain watchdog");
            for raw in receivers {
                // SAFETY: only same-thread top-level HWNDs created by this platform
                // are targeted. No global broadcast or OS setting is changed.
                unsafe {
                    SendMessageW(
                        HWND(raw as *mut _),
                        WM_SETTINGCHANGE,
                        Some(WPARAM(0)),
                        Some(LPARAM(0)),
                    )
                };
            }
            Ok(())
        }))
        .expect("native owner loop");
    let _ = finished.send(());
    watchdog_result
        .recv()
        .expect("watchdog handle")
        .join()
        .expect("watchdog finished");
    assert!(
        woke.load(Ordering::Acquire),
        "setting notification did not reach the owner"
    );
    assert_eq!(
        top_levels(),
        before,
        "host teardown retained a native receiver"
    );
}

//! `WM_DISPLAYCHANGE` on the hidden settings receiver: the cached snapshot is
//! sampled again, on the owner's turn and not inside the message. The displays'
//! and the preferences' subscribers only observe, so a work-area change reaches
//! both in either order.

use crate::WindowsPlatform;
use crate::owner::windows::broadcast::send_for_test;
use crate::platform::Platform;
use crate::platforms::windows::WindowsWindow;
use flui_platform::WindowOptions;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_SETWORKAREA, WM_DISPLAYCHANGE, WM_SETTINGCHANGE,
};

#[test]
fn display_change_message_invalidates_the_snapshot_and_wakes_the_owner() {
    let platform = WindowsPlatform::new().expect("native platform");
    let displays = Arc::clone(&platform.owner().displays);
    let receiver = platform
        .owner()
        .owner_control
        .gate()
        .shares("display message fixture")
        .expect("owner")
        .broadcast_receiver;
    platform.display_snapshot().expect("first sample");
    let sampled = displays.sample_count();
    platform.display_snapshot().expect("cached answer");
    assert_eq!(
        displays.sample_count(),
        sampled,
        "an unchanged snapshot was sampled again"
    );

    let samples_in_message = Arc::new(AtomicUsize::new(usize::MAX));
    let samples_on_turn = Arc::new(AtomicUsize::new(usize::MAX));
    let (finish, finished) = std::sync::mpsc::channel();
    let (watchdog_tx, watchdog_rx) = std::sync::mpsc::channel();
    let (message, turn) = (
        Arc::clone(&samples_in_message),
        Arc::clone(&samples_on_turn),
    );
    let counted = Arc::clone(&displays);
    Box::new(platform)
        .run(Box::new(move |owner| {
            let quit = owner.proxy();
            let timeout = quit.clone();
            let observer = Arc::clone(&counted);
            owner.on_wake(Box::new(move || {
                observer.snapshot().expect("sample on the owner turn");
                turn.store(observer.sample_count(), Ordering::Release);
                quit.request_quit().expect("observed the wake");
            }))?;
            watchdog_tx
                .send(std::thread::spawn(move || {
                    if finished
                        .recv_timeout(std::time::Duration::from_secs(2))
                        .is_err()
                    {
                        let _ = timeout.request_quit();
                    }
                }))
                .expect("watchdog handle");
            send_for_test(receiver, WM_DISPLAYCHANGE, 32, 0);
            // SendMessage returned: the handler ran to completion.
            message.store(counted.sample_count(), Ordering::Release);
            Ok(())
        }))
        .expect("display message loop");
    let _ = finish.send(());
    watchdog_rx
        .recv()
        .expect("watchdog")
        .join()
        .expect("watchdog finished");
    assert_eq!(
        samples_in_message.load(Ordering::Acquire),
        sampled,
        "the message handler sampled the displays"
    );
    assert_eq!(
        samples_on_turn.load(Ordering::Acquire),
        sampled + 1,
        "the owner's turn did not sample the invalidated snapshot"
    );
}

#[test]
fn work_area_change_invalidates_the_snapshot() {
    let platform = WindowsPlatform::new().expect("native platform");
    let receiver = platform
        .owner()
        .owner_control
        .gate()
        .shares("work area fixture")
        .expect("owner")
        .broadcast_receiver;
    platform.display_snapshot().expect("first sample");
    let sampled = platform.owner().displays.sample_count();
    send_for_test(receiver, WM_SETTINGCHANGE, SPI_SETWORKAREA.0 as usize, 0);
    platform
        .display_snapshot()
        .expect("sample after the change");
    assert_eq!(
        platform.owner().displays.sample_count(),
        sampled + 1,
        "a work-area change left the snapshot cached"
    );
    // Any other setting leaves the displays alone.
    send_for_test(receiver, WM_SETTINGCHANGE, 0, 0);
    platform.display_snapshot().expect("cached answer");
    assert_eq!(platform.owner().displays.sample_count(), sampled + 1);
}

/// The displays' and the preferences' subscribers only observe, so each hears
/// a setting change and a display change whichever the receiver lists first.
/// One that answered either message would keep it from the other in one of the
/// two orders.
#[test]
fn the_displays_and_the_preferences_hear_every_change_in_either_order() {
    use std::rc::Rc;

    use crate::display::windows::{DisplayBroadcasts, WindowsDisplays};
    use crate::owner::signal::OwnerSignal;
    use crate::owner::sources::SystemPreferenceSource;
    use crate::owner::windows::HostDisplays;
    use crate::owner::windows::broadcast::{Receiver, Subscriber};
    use crate::owner::windows::control::OwnerPreferences;
    use crate::preferences::windows::PreferenceSource;

    let samples_again = |displays: &Arc<dyn HostDisplays>| {
        let before = displays.sample_count();
        displays.snapshot().expect("a snapshot");
        displays.sample_count() > before
    };
    for displays_first in [true, false] {
        let order = if displays_first {
            "displays first"
        } else {
            "preferences first"
        };
        let signal = OwnerSignal::new(Arc::new(|| Ok(())));
        signal.start().expect("the owner signal starts");
        let displays: Arc<dyn HostDisplays> = Arc::new(WindowsDisplays::new());
        let preferences = PreferenceSource::new(&signal).expect("native preference source");
        let display_subscriber: Rc<dyn Subscriber> =
            Rc::new(DisplayBroadcasts::new(&displays, &signal));
        let preference_subscriber: Rc<dyn Subscriber> = preferences.broadcasts();
        let subscribers = if displays_first {
            [display_subscriber, preference_subscriber]
        } else {
            [preference_subscriber, display_subscriber]
        };
        let receiver = Receiver::new(subscribers).expect("a receiver");
        preferences.sample().expect("the first preference read");
        displays.snapshot().expect("the first sample");

        receiver.send_with_for_test(WM_SETTINGCHANGE, SPI_SETWORKAREA.0 as usize, 0);
        assert!(
            samples_again(&displays),
            "{order}: the displays did not hear the work-area change"
        );
        assert!(
            preferences.pending(),
            "{order}: the preferences did not hear the work-area change"
        );

        receiver.send_with_for_test(WM_DISPLAYCHANGE, 32, 0);
        assert!(
            samples_again(&displays),
            "{order}: the displays did not hear the display change"
        );
    }
}

#[test]
#[expect(unsafe_code, reason = "sending WM_DPICHANGED to an owned window")]
fn dpi_change_invalidates_the_snapshot() {
    use windows::Win32::{
        Foundation::{LPARAM, RECT, WPARAM},
        UI::{
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{GetWindowRect, SendMessageW, WM_DPICHANGED},
        },
    };
    let platform = WindowsPlatform::new().expect("native platform");
    let window = platform
        .open_window(WindowOptions::default().with_visible(false))
        .expect("hidden window");
    let hwnd = window
        .as_any()
        .downcast_ref::<WindowsWindow>()
        .expect("Win32 backend")
        .hwnd();
    platform.display_snapshot().expect("first sample");
    let sampled = platform.owner().displays.sample_count();
    // SAFETY: the window is live on this thread; `rect` outlives the
    // synchronous send, which is the pointer WM_DPICHANGED's lparam carries.
    unsafe {
        let dpi = GetDpiForWindow(hwnd);
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &raw mut rect).expect("window rect");
        SendMessageW(
            hwnd,
            WM_DPICHANGED,
            Some(WPARAM(((dpi << 16) | dpi) as usize)),
            Some(LPARAM((&raw const rect) as isize)),
        );
    }
    platform
        .display_snapshot()
        .expect("sample after the change");
    assert_eq!(
        platform.owner().displays.sample_count(),
        sampled + 1,
        "a DPI change left the snapshot cached"
    );
    window.close();
}

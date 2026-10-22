//! Private native getter failure injection; public native tests remain in tests/.

use super::*;
use std::panic::{AssertUnwindSafe, catch_unwind};

fn values(scale: f64) -> SystemPreferences {
    SystemPreferences::default()
        .with_text_scale(scale)
        .expect("valid scale")
}

fn clean_reads_reuse_the_observation() {
    let cache = SampleCache::default();
    let pending = AtomicBool::new(true);
    assert_eq!(
        cache
            .read(&pending, || Ok(values(2.0)))
            .expect("first read"),
        values(2.0)
    );
    assert_eq!(
        cache
            .read(&pending, || panic!("clean cache queried the OS"))
            .expect("cached read"),
        values(2.0)
    );
}

fn invalidation_during_read_requires_another_observation() {
    let cache = SampleCache::default();
    let pending = AtomicBool::new(true);
    cache
        .read(&pending, || {
            pending.store(true, Ordering::Release);
            Ok(values(2.0))
        })
        .expect("first observation");
    assert_eq!(
        cache.read(&pending, || Ok(values(3.0))).expect("refresh"),
        values(3.0)
    );
}

fn failed_refresh_is_not_a_stale_success() {
    let cache = SampleCache::default();
    let pending = AtomicBool::new(true);
    let now = web_time::Instant::now();
    cache
        .read(&pending, || Ok(values(2.0)))
        .expect("initial read");
    pending.store(true, Ordering::Release);
    assert!(
        cache
            .read_at(&pending, now, || Err(PlatformError::Preferences {
                message: "injected read failure".into()
            }))
            .is_err()
    );
    assert_eq!(
        cache
            .read_at(&pending, now + RETRY_INTERVAL, || Ok(values(3.0)))
            .expect("retry"),
        values(3.0)
    );
}

fn nested_read_cannot_publish_an_observation() {
    let cache = SampleCache::default();
    let pending = AtomicBool::new(true);
    cache
        .read(&pending, || Ok(values(1.0)))
        .expect("initial read");
    pending.store(true, Ordering::Release);
    let observed = cache
        .read(&pending, || {
            assert!(
                cache
                    .read(&pending, || panic!("nested getter entered"))
                    .is_err()
            );
            Ok(values(2.0))
        })
        .expect("outer read");
    assert_eq!(observed, values(2.0));
}

fn panic_preserves_refresh_and_releases_the_reader() {
    let cache = SampleCache::default();
    let pending = AtomicBool::new(true);
    let now = web_time::Instant::now();
    cache
        .read(&pending, || Ok(values(1.0)))
        .expect("initial read");
    pending.store(true, Ordering::Release);
    assert!(
        catch_unwind(AssertUnwindSafe(|| cache.read_at(
            &pending,
            now,
            || panic!("injected getter panic")
        )))
        .is_err()
    );
    assert!(
        pending.load(Ordering::Acquire),
        "failed read lost its retry obligation"
    );
    assert_eq!(
        cache
            .read_at(&pending, now + RETRY_INTERVAL, || Ok(values(3.0)))
            .expect("post-panic refresh"),
        values(3.0)
    );
}

#[test]
fn native_preference_read_recovery() {
    crate::table_test::run_table(
        "native_preference_read_recovery",
        &[
            (
                "unrelated_wakes_do_not_repeat_failed_native_reads",
                unrelated_wakes_do_not_repeat_failed_native_reads,
            ),
            (
                "clean_reads_reuse_the_observation",
                clean_reads_reuse_the_observation,
            ),
            (
                "invalidation_during_read_requires_another_observation",
                invalidation_during_read_requires_another_observation,
            ),
            (
                "failed_refresh_is_not_a_stale_success",
                failed_refresh_is_not_a_stale_success,
            ),
            (
                "nested_read_cannot_publish_an_observation",
                nested_read_cannot_publish_an_observation,
            ),
            (
                "panic_preserves_refresh_and_releases_the_reader",
                panic_preserves_refresh_and_releases_the_reader,
            ),
        ],
    );
}

fn unrelated_wakes_do_not_repeat_failed_native_reads() {
    let cache = SampleCache::default();
    let pending = AtomicBool::new(true);
    let now = web_time::Instant::now();
    assert!(
        cache
            .read_at(&pending, now, || Err(PlatformError::Preferences {
                message: "native read failed".into(),
            }))
            .is_err()
    );
    for offset in [0, 1, 20, 99] {
        assert!(
            cache
                .read_at(&pending, now + Duration::from_millis(offset), || {
                    panic!("another wake bypassed retry pacing")
                })
                .is_err()
        );
    }
    assert_eq!(
        cache
            .read_at(&pending, now + RETRY_INTERVAL, || Ok(values(2.0)))
            .expect("due read"),
        values(2.0)
    );
    pending.store(true, Ordering::Release);
    assert_eq!(
        cache
            .read_at(&pending, now + RETRY_INTERVAL, || Ok(values(3.0)))
            .expect("healthy change is not delayed"),
        values(3.0)
    );
}

/// A receiver wired to a host, with the notices it delivers collected.
fn hosted_receiver() -> (
    Receiver,
    Rc<crate::platforms::windows::host_events::HostEvents>,
    Arc<std::sync::Mutex<Vec<flui_platform::SystemNotice>>>,
) {
    let notices = Arc::new(crate::shared::system_notice::SystemNotices::default());
    let heard = Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = Arc::clone(&heard);
    notices.set_hook(Box::new(move |notice| {
        recorded.lock().expect("notice log").push(notice);
    }));
    let host = Rc::new(crate::platforms::windows::host_events::HostEvents::new(
        Rc::new(RefCell::new(
            crate::owner::handlers::PlatformHandlers::default(),
        )),
        notices,
    ));
    let invalidation = Arc::new(Invalidation {
        accepting: AtomicBool::new(true),
        pending: AtomicBool::new(false),
        signal: Weak::new(),
        displays: Arc::new(crate::platforms::windows::display::WindowsDisplays::new()),
    });
    let receiver = Receiver::new(invalidation, Some(Rc::clone(&host) as Rc<dyn Subscriber>))
        .expect("receiver");
    (receiver, host, heard)
}

fn low_memory_is_reported_once_per_hold() {
    use flui_platform::{MemoryPressureLevel, SystemNotice};

    let (receiver, host, heard) = hosted_receiver();
    let report =
        || receiver.send_with_for_test(crate::platforms::windows::host_events::LOW_MEMORY, 0, 0);
    let critical = SystemNotice::MemoryPressure(MemoryPressureLevel::Critical);

    assert!(
        host.signal_low_memory_for_test(),
        "the first signal wakes the owner"
    );
    report();
    assert_eq!(*heard.lock().expect("log"), [critical]);

    // The event stays signaled while memory is low: more signals during the
    // hold are the same episode, whatever reaches the owner's queue.
    assert!(!host.signal_low_memory_for_test());
    report();
    assert_eq!(heard.lock().expect("log").len(), 1);

    // The hold over, the wait is registered again and a new episode reports.
    host.end_low_memory_hold_for_test(receiver.hwnd_for_test());
    assert!(host.signal_low_memory_for_test());
    report();
    assert_eq!(*heard.lock().expect("log"), [critical, critical]);
}

fn a_wake_that_could_not_be_posted_is_delivered_with_the_next_message() {
    use flui_platform::{MemoryPressureLevel, SystemNotice};

    let (receiver, host, heard) = hosted_receiver();
    host.fail_low_memory_post_for_test();
    assert!(heard.lock().expect("log").is_empty());
    receiver.send_with_for_test(windows::Win32::UI::WindowsAndMessaging::WM_NULL, 0, 0);
    assert_eq!(
        *heard.lock().expect("log"),
        [SystemNotice::MemoryPressure(MemoryPressureLevel::Critical)],
        "the report was owed, not lost"
    );
}

fn a_refused_rearm_timer_is_set_again_by_the_next_message() {
    let (receiver, host, _heard) = hosted_receiver();
    host.owe_rearm_timer_for_test();
    assert!(host.rearm_timer_owed_for_test());
    // Any message the receiver gets pays the debt.
    receiver.send_with_for_test(windows::Win32::UI::WindowsAndMessaging::WM_NULL, 0, 0);
    assert!(!host.rearm_timer_owed_for_test());
}

/// An owner signal that counts its wakes.
fn counting_signal() -> (Arc<OwnerSignal>, Arc<std::sync::atomic::AtomicUsize>) {
    let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let signal = OwnerSignal::new({
        let wakes = Arc::clone(&wakes);
        Arc::new(move || {
            wakes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    });
    signal.start().expect("the owner signal starts");
    (signal, wakes)
}

/// A window the OS still owns after a failed `DestroyWindow` keeps its
/// context and its class, so a message it still gets runs against a closed,
/// inert source instead of a freed one.
fn a_receiver_the_os_keeps_stays_registered_and_inert() {
    use windows::Win32::UI::WindowsAndMessaging::{DestroyWindow, IsWindow, UnregisterClassW};
    use windows::core::PCWSTR;

    let (signal, wakes) = counting_signal();
    let invalidation = Arc::new(Invalidation {
        accepting: AtomicBool::new(true),
        pending: AtomicBool::new(false),
        signal: Arc::downgrade(&signal),
        displays: Arc::new(crate::platforms::windows::display::WindowsDisplays::new()),
    });
    let receiver = Receiver::new(Arc::clone(&invalidation), None).expect("receiver");
    let hwnd = receiver.hwnd_for_test();
    let (class, instance) = receiver.class_for_test();
    // `PreferenceSource` closes admission before its receiver retires.
    invalidation.accepting.store(false, Ordering::Release);
    let woken = wakes.load(Ordering::SeqCst);
    let mut unregistered = false;
    receiver.retire_for_test(
        |_| Err(windows::Win32::Foundation::E_ACCESSDENIED.into()),
        |_, _| {
            unregistered = true;
            Ok(())
        },
    );
    assert_eq!(
        Arc::strong_count(&invalidation),
        2,
        "the kept window's context, which shares the invalidation, is still alive"
    );
    assert!(!unregistered, "the kept window's class is not retired");

    // SAFETY: `hwnd` is a handle this test created; `IsWindow` only asks.
    assert!(
        unsafe { IsWindow(Some(hwnd)) }.as_bool(),
        "the OS kept the window"
    );
    // SAFETY: a pointer-free notification to the window this test kept.
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::SendMessageW(
            hwnd,
            windows::Win32::UI::WindowsAndMessaging::WM_SETTINGCHANGE,
            Some(windows::Win32::Foundation::WPARAM(0)),
            Some(windows::Win32::Foundation::LPARAM(0)),
        );
    }
    assert!(!invalidation.pending.load(Ordering::Acquire));
    assert_eq!(
        wakes.load(Ordering::SeqCst),
        woken,
        "a closed source wakes nothing"
    );

    // Retire what the failed destruction kept. Its context stays leaked.
    // SAFETY: this thread created the window, so it can destroy it.
    unsafe { DestroyWindow(hwnd) }.expect("the kept window is destroyed");
    // SAFETY: no window of the class is left.
    unsafe { UnregisterClassW(PCWSTR(class.as_ptr()), Some(instance)) }
        .expect("the kept class is retired");
}

/// Every WinRT delegate captures only its source's invalidation, which the
/// source closes before it revokes them: one that outlives the source, as a
/// failed revocation would leave it, marks nothing pending and wakes nobody,
/// while a successor still wakes.
fn a_delegate_that_outlives_its_source_is_inert() {
    let (signal, wakes) = counting_signal();
    let displays = Arc::new(crate::platforms::windows::display::WindowsDisplays::new());
    let source = PreferenceSource::with_host(&signal, &displays, None).expect("native source");
    // What each WinRT delegate captures.
    let kept = Arc::clone(&source.invalidation);
    drop(source);
    kept.pending.store(false, Ordering::Release);
    let woken = wakes.load(Ordering::SeqCst);
    kept.notify();
    kept.notify_displays();
    assert!(!kept.pending.load(Ordering::Acquire));
    assert_eq!(
        wakes.load(Ordering::SeqCst),
        woken,
        "the old delegate wakes nobody"
    );

    let successor = PreferenceSource::with_host(&signal, &displays, None).expect("successor");
    let woken = wakes.load(Ordering::SeqCst);
    successor.send_setting_change_for_test();
    assert!(successor.pending());
    assert!(
        wakes.load(Ordering::SeqCst) > woken,
        "the successor still wakes"
    );
}

#[test]
fn preference_receiver_retirement() {
    crate::table_test::run_table(
        "preference_receiver_retirement",
        &[
            (
                "a_receiver_the_os_keeps_stays_registered_and_inert",
                a_receiver_the_os_keeps_stays_registered_and_inert as fn(),
            ),
            (
                "a_delegate_that_outlives_its_source_is_inert",
                a_delegate_that_outlives_its_source_is_inert as fn(),
            ),
        ],
    );
}

#[test]
fn host_event_sources() {
    crate::table_test::run_table(
        "host_event_sources",
        &[
            (
                "low_memory_is_reported_once_per_hold",
                low_memory_is_reported_once_per_hold as fn(),
            ),
            (
                "a_wake_that_could_not_be_posted_is_delivered_with_the_next_message",
                a_wake_that_could_not_be_posted_is_delivered_with_the_next_message as fn(),
            ),
            (
                "a_refused_rearm_timer_is_set_again_by_the_next_message",
                a_refused_rearm_timer_is_set_again_by_the_next_message as fn(),
            ),
        ],
    );
}

fn vertical_wheel_count_is_lines_until_the_page_sentinel() {
    // WHEEL_PAGESCROLL is UINT_MAX; zero disables the axis and stays a count.
    for (count, expected) in [
        (0, WheelStep::Lines(0)),
        (1, WheelStep::Lines(1)),
        (3, WheelStep::Lines(3)),
        (u32::MAX - 1, WheelStep::Lines(u32::MAX - 1)),
        (u32::MAX, WheelStep::Page),
    ] {
        assert_eq!(
            vertical_wheel(count),
            expected,
            "vertical wheel count {count}"
        );
    }
}

fn horizontal_wheel_characters_are_never_a_page() {
    // SPI_GETWHEELSCROLLCHARS has no page sentinel: UINT_MAX is a character count.
    for (lines, characters) in [(3, 3), (0, 0), (u32::MAX, u32::MAX), (3, u32::MAX)] {
        let wheel = wheel_preferences(lines, characters);
        assert_eq!(
            wheel.horizontal_characters(),
            Some(characters),
            "lines {lines}, characters {characters}"
        );
        assert_eq!(
            wheel.vertical(),
            Some(vertical_wheel(lines)),
            "lines {lines}, characters {characters}"
        );
    }
}

fn drag_metric_is_a_half_extent_of_the_same_magnitude() {
    let invalid = Err(PreferenceQueryError::Invalid(
        flui_platform::InvalidPreference::GestureArea,
    ));
    for (metric, expected) in [
        (0, Ok(0)),
        (4, Ok(4)),
        (-4, Ok(4)),
        (i32::MAX, Ok(i32::MAX)),
        (-i32::MAX, Ok(i32::MAX)),
        // |i32::MIN| exists only after widening, and does not fit back.
        (i32::MIN, invalid),
    ] {
        assert_eq!(drag_extent(metric), expected, "drag metric {metric}");
    }
}

#[test]
fn native_unit_conversions() {
    crate::table_test::run_table(
        "native_unit_conversions",
        &[
            (
                "vertical_wheel_count_is_lines_until_the_page_sentinel",
                vertical_wheel_count_is_lines_until_the_page_sentinel as fn(),
            ),
            (
                "horizontal_wheel_characters_are_never_a_page",
                horizontal_wheel_characters_are_never_a_page as fn(),
            ),
            (
                "drag_metric_is_a_half_extent_of_the_same_magnitude",
                drag_metric_is_a_half_extent_of_the_same_magnitude as fn(),
            ),
        ],
    );
}

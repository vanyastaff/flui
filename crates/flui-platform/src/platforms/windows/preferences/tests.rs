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
    receiver::Receiver,
    Rc<crate::platforms::windows::host_events::HostEvents>,
    Arc<std::sync::Mutex<Vec<flui_platform_api::SystemNotice>>>,
) {
    let notices = Arc::new(crate::shared::system_notice::SystemNotices::default());
    let heard = Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = Arc::clone(&heard);
    notices.set_hook(Box::new(move |notice| {
        recorded.lock().expect("notice log").push(notice);
    }));
    let host = Rc::new(crate::platforms::windows::host_events::HostEvents::new(
        Rc::new(RefCell::new(crate::shared::PlatformHandlers::default())),
        notices,
    ));
    let invalidation = Arc::new(Invalidation {
        accepting: AtomicBool::new(true),
        pending: AtomicBool::new(false),
        signal: Weak::new(),
        displays: Arc::new(crate::platforms::windows::display::WindowsDisplays::new()),
    });
    let receiver = receiver::Receiver::new(invalidation, Some(Rc::clone(&host))).expect("receiver");
    (receiver, host, heard)
}

fn low_memory_is_reported_once_per_hold() {
    use flui_platform_api::{MemoryPressureLevel, SystemNotice};

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

fn a_refused_rearm_timer_is_set_again_by_the_next_message() {
    let (receiver, host, _heard) = hosted_receiver();
    host.owe_rearm_timer_for_test();
    assert!(host.rearm_timer_owed_for_test());
    // Any message the receiver gets pays the debt.
    receiver.send_with_for_test(windows::Win32::UI::WindowsAndMessaging::WM_NULL, 0, 0);
    assert!(!host.rearm_timer_owed_for_test());
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
                "a_refused_rearm_timer_is_set_again_by_the_next_message",
                a_refused_rearm_timer_is_set_again_by_the_next_message as fn(),
            ),
        ],
    );
}

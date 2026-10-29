//! What a loop asks of the windows it holds, and the frame requests a host
//! stamps on the event clock. The application's handler is reached through
//! the loop's [`AppDelivery`](super::app_delivery::AppDelivery), a window's
//! through its [`WindowDelivery`](super::window_delivery::WindowDelivery).

use std::sync::Arc;

use flui_platform::{EventTime, FramePresented, FrameRequest};
use parking_lot::Mutex;

/// The windows a loop holds through its hook, as one object: the window
/// feature's. Each loop's hook extends it with what that loop asks of them.
pub(crate) trait OpenWindows: Send + Sync {
    /// Whether no window is open.
    fn is_empty(&self) -> bool;
}

/// A frame request for a backend whose platform reports no frame timestamp: the vsync is this
/// reading of the event clock, and the target present time is unknown (ADR-0257).
#[must_use]
pub fn frame_request_now() -> FrameRequest {
    frame_request_aged(std::time::Duration::ZERO)
}

/// A frame request whose vsync the platform stamped `age` before this reading of the event clock,
/// for a platform that reports its frame time on its own clock (the browser's animation frame
/// timestamp): the age is the one quantity both clocks agree on, so no epoch has to be aligned.
/// An age past the start of the timeline is the start of it.
#[must_use]
pub fn frame_request_aged(age: std::time::Duration) -> FrameRequest {
    frame_request_timed(age, None)
}

/// [`frame_request_aged`] for a platform that also knows when the frame will next reach the
/// screen, `until_present` after this reading.
#[must_use]
pub fn frame_request_timed(
    age: std::time::Duration,
    until_present: Option<std::time::Duration>,
) -> FrameRequest {
    frame_request_at(EventTime::now().as_nanos(), age, until_present)
}

fn frame_request_at(
    now_nanos: u64,
    age: std::time::Duration,
    until_present: Option<std::time::Duration>,
) -> FrameRequest {
    let nanos = |d: std::time::Duration| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX);
    let request = FrameRequest::new(flui_platform::EventTime::from_nanos(
        now_nanos.saturating_sub(nanos(age)),
    ));
    match until_present {
        Some(wait) => request.with_target_present(flui_platform::EventTime::from_nanos(
            now_nanos.saturating_add(nanos(wait)),
        )),
        None => request,
    }
}

/// The event-clock time of a stamp the platform took on its own monotonic clock, given a reading
/// of that clock taken now (`platform_now`): the stamp lies as far before or after this reading of
/// the event clock as it lies from `platform_now`, so no epoch has to be aligned. A stamp further
/// back than the timeline's start is its start.
#[cfg(any(windows, wayland_host))]
#[must_use]
pub(crate) fn rebased(platform_now: std::time::Duration, stamp: std::time::Duration) -> EventTime {
    rebased_at(EventTime::now().as_nanos(), platform_now, stamp)
}

/// The event-clock time `age` before this reading of it, for a platform stamp whose distance from
/// a reading of the platform's clock is known; the timeline's start for an age longer than it.
#[cfg(windows)]
#[must_use]
pub(crate) fn aged(age: std::time::Duration) -> EventTime {
    rebased(age, std::time::Duration::ZERO)
}

#[cfg(any(windows, wayland_host, target_os = "macos", test))]
pub(crate) fn rebased_at(
    event_now: u64,
    platform_now: std::time::Duration,
    stamp: std::time::Duration,
) -> EventTime {
    let nanos = |d: std::time::Duration| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX);
    // One of the two is zero: the stamp lies before the reading or after it.
    let (before, after) = (
        platform_now.saturating_sub(stamp),
        stamp.saturating_sub(platform_now),
    );
    EventTime::from_nanos(
        event_now
            .saturating_sub(nanos(before))
            .saturating_add(nanos(after)),
    )
}

/// What a window's platform reported about frames that reached the screen, kept until a frame
/// request of the window carries it (`FrameRequest::presented`).
///
/// Any thread reports: the presenting thread (a swap chain's statistics), the platform's own
/// callback thread (Metal's presented handler) or the owner (a compositor's event). The newest
/// report wins; a report no later than one kept or already carried is a repeat or out of order,
/// and is dropped, so the times a window's requests carry never run backwards. A report of a time
/// still ahead when a request is delivered (a platform that names when a frame is due on screen)
/// waits for a later request: a request never carries a presentation that has not happened.
#[derive(Clone, Debug, Default)]
pub(crate) struct PresentationReports(Arc<Mutex<Reported>>);

#[derive(Debug, Default)]
struct Reported {
    /// The newest report no request has carried yet.
    pending: Option<FramePresented>,
    /// The time of the newest report a request carried.
    carried: Option<EventTime>,
}

impl PresentationReports {
    /// The platform showed a frame.
    pub(crate) fn report(&self, presented: FramePresented) {
        let mut reported = self.0.lock();
        let newest = reported
            .pending
            .map(|pending| pending.at)
            .max(reported.carried);
        if newest.is_none_or(|newest| presented.at > newest) {
            reported.pending = Some(presented);
        }
    }

    /// `request`, delivered when the event clock reads `now`, carrying the newest report no
    /// request carried yet, if that report is not later than `now`.
    pub(crate) fn carried_by(&self, request: FrameRequest, now: EventTime) -> FrameRequest {
        let mut reported = self.0.lock();
        match reported.pending {
            Some(presented) if presented.at <= now => {
                reported.pending = None;
                reported.carried = Some(presented.at);
                request.with_presented(presented)
            }
            Some(_) | None => request,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use parking_lot::Mutex;

    use crate::owner::call_point::call;
    use crate::owner::panic_boundary::contain_owner_callback;
    use crate::owner::signal::OwnerSignal;

    use super::*;

    /// A platform stamp lands as far from the event reading as it lies from the platform's own
    /// reading, on either side, and never before the timeline's start.
    #[test]
    fn a_platform_stamp_is_rebased_by_its_distance_from_the_reading() {
        let ms = std::time::Duration::from_millis;
        let at = |platform_now, stamp| rebased_at(50_000_000, platform_now, stamp).as_nanos();
        assert_eq!(at(ms(900), ms(880)), 30_000_000, "20 ms before");
        assert_eq!(at(ms(900), ms(905)), 55_000_000, "5 ms after");
        assert_eq!(at(ms(900), ms(900)), 50_000_000);
        assert_eq!(at(ms(900), ms(0)), 0, "before the timeline's start");
        assert_eq!(
            rebased_at(u64::MAX - 1, ms(0), ms(1)).as_nanos(),
            u64::MAX,
            "past the timeline's end"
        );
    }

    /// The newest report reaches the first request delivered at or after it, once; a repeat,
    /// an older report and one still ahead of the delivery do not.
    #[test]
    fn a_presentation_report_rides_the_first_request_at_or_after_it() {
        let at = EventTime::from_nanos;
        let shown = |nanos| FramePresented::new(at(nanos));
        let carried = |reports: &PresentationReports, now| {
            reports
                .carried_by(FrameRequest::new(at(1)), at(now))
                .presented
                .map(|presented| presented.at.as_nanos())
        };

        let reports = PresentationReports::default();
        assert_eq!(carried(&reports, 10), None, "nothing reported");

        reports.report(shown(20));
        reports.report(shown(30));
        reports.report(shown(25));
        assert_eq!(carried(&reports, 29), None, "the newest is still ahead");
        assert_eq!(carried(&reports, 30), Some(30), "the newest report");
        assert_eq!(carried(&reports, 40), None, "carried once");

        reports.report(shown(30));
        reports.report(shown(28));
        assert_eq!(carried(&reports, 50), None, "a repeat and an older report");

        reports.report(shown(45));
        assert_eq!(carried(&reports, 50), Some(45));
    }

    /// A platform's frame stamp, given as an age, lands that long before the reading, and an age
    /// longer than the timeline is its start.
    #[test]
    fn an_aged_frame_request_is_stamped_before_the_reading() {
        use std::time::Duration;

        let at = |now, age| frame_request_at(now, age, None).vsync.as_nanos();
        assert_eq!(at(5_000_000, Duration::from_millis(2)), 3_000_000);
        assert_eq!(at(5_000_000, Duration::ZERO), 5_000_000);
        assert_eq!(at(1_000, Duration::from_secs(1)), 0);
        assert_eq!(at(1_000, Duration::MAX), 0);

        let request = frame_request_at(
            5_000_000,
            Duration::from_millis(2),
            Some(Duration::from_millis(3)),
        );
        assert_eq!(request.vsync.as_nanos(), 3_000_000);
        assert_eq!(
            request
                .target_present
                .map(flui_platform::EventTime::as_nanos),
            Some(8_000_000)
        );
    }

    fn owner_callback_and_capture_panics_do_not_replace_an_existing_unwind() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        struct Hostile;
        impl Drop for Hostile {
            fn drop(&mut self) {
                panic!("hostile payload drop");
            }
        }
        struct Capture(Arc<AtomicUsize>);
        impl Drop for Capture {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
                std::panic::panic_any(Hostile);
            }
        }
        // A teardown that runs a one-shot callback from a destructor, as the
        // Win32 run guard tells the application it quits: the callback is
        // taken for good, so a failed call retains it rather than putting it
        // back for the guard's own drop.
        struct OneShot<'a>(&'a Mutex<Option<Box<dyn FnMut() + Send>>>);
        impl crate::owner::call_point::HandlerSlot for OneShot<'_> {
            type Handler = Box<dyn FnMut() + Send>;
            fn take(&self) -> Option<Self::Handler> {
                self.0.lock().take()
            }
            fn restore(&self, handler: Self::Handler) -> Option<Self::Handler> {
                Some(handler)
            }
            fn replace(&self, handler: Self::Handler) -> Option<Self::Handler> {
                self.0.lock().replace(handler)
            }
        }
        struct Guard(Mutex<Option<Box<dyn FnMut() + Send>>>);
        impl Drop for Guard {
            fn drop(&mut self) {
                call(&OneShot(&self.0), |callback| {
                    callback();
                })
                .or_contain((), "the guarded callback");
            }
        }
        for already_unwinding in [false, true] {
            let invoked = Arc::new(AtomicUsize::new(0));
            let dropped = Arc::new(AtomicUsize::new(0));
            let capture = Capture(Arc::clone(&dropped));
            let observed = Arc::clone(&invoked);
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                let guard = Guard(Mutex::new(Some(Box::new(move || {
                    let _ = &capture;
                    observed.fetch_add(1, Ordering::SeqCst);
                    std::panic::panic_any(Hostile);
                }))));
                assert!(!already_unwinding, "original bootstrap panic");
                drop(guard);
            }));
            if already_unwinding {
                assert_eq!(
                    *outcome
                        .expect_err("original panic")
                        .downcast::<&str>()
                        .expect("original identity"),
                    "original bootstrap panic"
                );
            } else {
                assert!(outcome.is_ok());
            }
            assert_eq!(invoked.load(Ordering::SeqCst), 1);
            // The call failed: its captures are retained, never dropped into
            // a second failure (ADR-0127 §2).
            assert_eq!(dropped.load(Ordering::SeqCst), 0);
        }
    }

    fn off_owner_retention_case(fail_report: bool, already_unwinding: bool) {
        struct Resource(Arc<AtomicUsize>);
        impl Drop for Resource {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
                panic!("owner-affine resource retired on a foreign thread");
            }
        }
        struct Guard {
            resource: Option<(Resource, Resource)>,
            report_drops: Arc<AtomicUsize>,
            fail_report: bool,
        }
        impl Drop for Guard {
            fn drop(&mut self) {
                let report_drops = Arc::clone(&self.report_drops);
                let fail_report = self.fail_report;
                crate::owner::panic_boundary::retain_off_owner_resource(
                    self.resource.take().expect("resource"),
                    || {
                        if fail_report {
                            std::panic::panic_any((
                                Resource(Arc::clone(&report_drops)),
                                Resource(report_drops),
                            ));
                        }
                    },
                );
            }
        }
        let resource_drops = Arc::new(AtomicUsize::new(0));
        let report_drops = Arc::new(AtomicUsize::new(0));
        let result = catch_unwind(AssertUnwindSafe(|| {
            let guard = Guard {
                resource: Some((
                    Resource(Arc::clone(&resource_drops)),
                    Resource(Arc::clone(&resource_drops)),
                )),
                report_drops: Arc::clone(&report_drops),
                fail_report,
            };
            assert!(!already_unwinding, "original teardown failure");
            drop(guard);
        }));
        assert_eq!(resource_drops.load(Ordering::SeqCst), 0);
        assert_eq!(report_drops.load(Ordering::SeqCst), 0);
        if already_unwinding {
            assert_eq!(
                *result
                    .expect_err("original failure")
                    .downcast::<&str>()
                    .expect("original payload"),
                "original teardown failure"
            );
        } else {
            assert!(result.is_ok(), "report failure must remain contained");
        }
        let next = AtomicUsize::new(0);
        contain_owner_callback(|| {
            next.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(next.load(Ordering::SeqCst), 1);
    }

    fn off_owner_retention_precedes_normal_diagnostics() {
        off_owner_retention_case(false, false);
    }
    fn off_owner_retention_contains_opaque_diagnostic_failure() {
        off_owner_retention_case(true, false);
    }
    fn off_owner_retention_preserves_an_unwind_with_normal_diagnostics() {
        off_owner_retention_case(false, true);
    }
    fn off_owner_retention_preserves_the_existing_unwind() {
        off_owner_retention_case(true, true);
    }

    fn worker_burst_coalesces_and_reentrant_wake_gets_a_later_finite_turn() {
        let posts = Arc::new(AtomicUsize::new(0));
        let recorded = Arc::clone(&posts);
        let signal = OwnerSignal::new(Arc::new(move || {
            recorded.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
        let calls = Arc::new(AtomicUsize::new(0));
        let recorded = Arc::clone(&calls);
        let weak = Arc::downgrade(&signal);
        signal
            .register(Box::new(move || {
                if recorded.fetch_add(1, Ordering::SeqCst) == 0 {
                    let signal = weak.upgrade().expect("live");
                    signal.wake().expect("reentrant wake");
                    assert!(!signal.drive());
                }
            }))
            .expect("register");
        signal.start().expect("start");
        let worker = Arc::clone(&signal);
        std::thread::spawn(move || {
            for _ in 0..100 {
                worker.wake().expect("wake");
            }
        })
        .join()
        .expect("worker");
        assert_eq!(posts.load(Ordering::SeqCst), 1);
        assert!(!signal.drive());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(posts.load(Ordering::SeqCst), 2);
        assert!(!signal.drive());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(posts.load(Ordering::SeqCst), 2);
        signal.close();
    }

    /// A loop that reads what wakes asked of it begins the turn, reads, then
    /// drives. A wake that lands while it reads posts the next turn, and the
    /// loop reads again there: a change recorded during the first read is not
    /// left for an unrelated later wake.
    fn a_wake_while_the_loop_reads_gets_a_turn_that_reads_again() {
        let posts = Arc::new(AtomicUsize::new(0));
        let recorded = Arc::clone(&posts);
        let signal = OwnerSignal::new(Arc::new(move || {
            recorded.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        signal
            .register(Box::new(move || {
                counted.fetch_add(1, Ordering::SeqCst);
            }))
            .expect("register");
        signal.start().expect("start");
        // What a background source records for the loop to read, and wakes
        // the owner for.
        let changes = Arc::new(Mutex::new(Vec::<u32>::new()));
        changes.lock().push(1);
        signal.wake().expect("wake");
        assert_eq!(posts.load(Ordering::SeqCst), 1);

        let mut read = Vec::new();
        let mut turns = 0;
        // The loop: one pass per posted turn.
        while turns < posts.load(Ordering::SeqCst) {
            turns += 1;
            let turn = signal.begin_turn();
            read.extend(std::mem::take(&mut *changes.lock()));
            if turns == 1 {
                // The source records another change while the loop reads.
                changes.lock().push(2);
                signal.wake().expect("wake");
            }
            assert!(!turn.drive());
        }
        assert_eq!(
            read,
            [1, 2],
            "the change made during the read was never read"
        );
        assert_eq!(turns, 2);
        // The wake during the read was the first turn's to run: the second
        // turn only read.
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        signal.close();
    }

    /// A turn begun and dropped before it was driven (a read that unwinds, a
    /// loop that returns early) posts the next turn for the work still owed,
    /// so the application's turn never waits for an unrelated wake.
    fn an_undriven_turn_posts_the_next_one() {
        let posts = Arc::new(AtomicUsize::new(0));
        let recorded = Arc::clone(&posts);
        let signal = OwnerSignal::new(Arc::new(move || {
            recorded.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        signal
            .register(Box::new(move || {
                counted.fetch_add(1, Ordering::SeqCst);
            }))
            .expect("register");
        signal.start().expect("start");
        signal.wake().expect("wake");
        assert_eq!(posts.load(Ordering::SeqCst), 1);

        let unwound = catch_unwind(AssertUnwindSafe(|| {
            let _turn = signal.begin_turn();
            panic!("a read panics before the turn is driven");
        }));
        assert!(unwound.is_err());
        assert_eq!(
            posts.load(Ordering::SeqCst),
            2,
            "the undriven turn posted no turn for the work it left owed"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(!signal.drive());
        assert_eq!(calls.load(Ordering::SeqCst), 1, "the posted turn runs it");
        assert_eq!(posts.load(Ordering::SeqCst), 2);
        signal.close();
    }

    /// A read inside a turn that finds work for the application's turn owes
    /// it to that turn: the drive runs it, and nothing is posted for it.
    #[cfg(all(windows, feature = "winit-backend"))]
    fn a_turn_owed_by_its_read_posts_nothing() {
        let posts = Arc::new(AtomicUsize::new(0));
        let recorded = Arc::clone(&posts);
        let signal = OwnerSignal::new(Arc::new(move || {
            recorded.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        signal
            .register(Box::new(move || {
                counted.fetch_add(1, Ordering::SeqCst);
            }))
            .expect("register");
        signal.start().expect("start");

        let turn = signal.begin_turn();
        turn.owe();
        assert!(!turn.drive());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            posts.load(Ordering::SeqCst),
            0,
            "an owed turn posted a turn of its own"
        );
        signal.close();
    }

    /// The owner-thread machinery's contracts: hostile callback captures and
    /// payloads are contained (a direct clear drops each capture without
    /// unwinding; an owner callback's own and capture panics never replace an
    /// existing unwind), and the owner signal coalesces a burst and gives a
    /// re-entrant wake a later turn, gives a wake that lands while the loop
    /// reads a turn that reads again, posts the next turn when a begun turn is
    /// dropped undriven, and runs work a read owes to its turn without posting
    /// another.
    #[test]
    fn the_owner_thread_machinery_honours_its_contracts() {
        let cases: &[(&str, fn())] = &[
            #[cfg(target_os = "windows")]
            (
                "window identity exhaustion",
                crate::owner::windows::WindowsLoop::window_identity_exhaustion_permanently_refuses_retries,
            ),
            #[cfg(target_os = "windows")]
            (
                "platform identity exhaustion",
                crate::owner::windows::WindowsLoop::platform_identity_exhaustion_acquires_nothing,
            ),
            (
                "owner callback and capture panic priority",
                owner_callback_and_capture_panics_do_not_replace_an_existing_unwind,
            ),
            (
                "worker burst and reentrant wake",
                worker_burst_coalesces_and_reentrant_wake_gets_a_later_finite_turn,
            ),
            (
                "a wake while the loop reads",
                a_wake_while_the_loop_reads_gets_a_turn_that_reads_again,
            ),
            (
                "an undriven turn",
                an_undriven_turn_posts_the_next_one,
            ),
            #[cfg(all(windows, feature = "winit-backend"))]
            (
                "a turn owed by its read",
                a_turn_owed_by_its_read_posts_nothing,
            ),
            (
                "off-owner normal diagnostics",
                off_owner_retention_precedes_normal_diagnostics,
            ),
            (
                "off-owner diagnostic failure",
                off_owner_retention_contains_opaque_diagnostic_failure,
            ),
            (
                "off-owner unwind with normal diagnostics",
                off_owner_retention_preserves_an_unwind_with_normal_diagnostics,
            ),
            (
                "off-owner existing unwind",
                off_owner_retention_preserves_the_existing_unwind,
            ),
        ];
        let mut failures = Vec::new();
        for &(name, case) in cases {
            if catch_unwind(AssertUnwindSafe(case)).is_err() {
                failures.push(name);
            }
        }
        assert!(
            failures.is_empty(),
            "failed owner machinery cases: {failures:?}"
        );
    }
}

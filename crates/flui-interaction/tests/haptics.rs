//! A presentation's haptics host: the frame queue, its deadline and the
//! outcome mailbox (ADR-0210 §3–§5).

use std::cell::{Cell, RefCell};
use std::num::NonZeroU32;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_foundation::panic::RecoveryScope;
use flui_interaction::haptics::{
    FALLBACK_REFRESH_PERIOD, HapticsHost, HapticsHostBuilder, MAILBOX_CAPACITY,
    haptic_frame_deadline,
};
use flui_interaction::{GestureBinding, HapticObserver, HapticsHandle, ManualClock};

/// A registration an observer can reach from its own call.
type Registration = Rc<std::cell::OnceCell<HapticObserver>>;
use flui_platform::haptics::{
    HapticOutcome, HapticPattern, HapticRequest, HapticTicket, HapticTiming,
};
use flui_platform::testing::FakeHaptics;
use flui_platform::{HapticFeedback, PlatformHaptics};

/// The refresh period every fixture's window reports: the deadline is two of
/// them.
const PERIOD: Duration = Duration::from_millis(10);
const DEADLINE: Duration = Duration::from_millis(20);

const FRAME: HapticRequest = HapticRequest::new(HapticTiming::FramePresented);
const NOW: HapticRequest = HapticRequest::new(HapticTiming::Immediate);

type Delivered = Rc<RefCell<Vec<(HapticTicket, HapticOutcome)>>>;

struct Fixture {
    clock: ManualClock,
    fake: Arc<FakeHaptics>,
    host: HapticsHost,
    handle: HapticsHandle,
    frames: Rc<Cell<u32>>,
    period: Rc<Cell<Option<Duration>>>,
    registrations: RefCell<Vec<HapticObserver>>,
    _binding: GestureBinding,
}

fn fixture_with(configure: impl FnOnce(HapticsHostBuilder) -> HapticsHostBuilder) -> Fixture {
    let clock = ManualClock::new();
    let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
    let fake = Arc::new(FakeHaptics::new());
    let actuator: Arc<dyn PlatformHaptics> = fake.clone();
    let frames = Rc::new(Cell::new(0));
    let period = Rc::new(Cell::new(Some(PERIOD)));
    let host = configure(
        HapticsHost::builder(&binding, move || Some(Arc::clone(&actuator)))
            .frame_requester({
                let frames = Rc::clone(&frames);
                move || frames.set(frames.get() + 1)
            })
            .refresh_period({
                let period = Rc::clone(&period);
                move || period.get()
            })
            .deadline_factor(NonZeroU32::new(2).expect("nonzero")),
    )
    .build();
    let handle = host.handle();
    Fixture {
        clock,
        fake,
        host,
        handle,
        frames,
        period,
        registrations: RefCell::default(),
        _binding: binding,
    }
}

fn fixture() -> Fixture {
    fixture_with(|host| host)
}

impl Fixture {
    /// Register an observer that records what it receives, kept for the
    /// fixture's lifetime.
    fn observe(&self) -> Delivered {
        let delivered: Delivered = Rc::default();
        let sink = Rc::clone(&delivered);
        let registration = self
            .handle
            .observe(move |ticket, outcome| sink.borrow_mut().push((ticket, outcome)));
        self.registrations.borrow_mut().push(registration);
        delivered
    }

    /// One owner turn, resuming the first failure it contained.
    fn service(&self) {
        RecoveryScope::boundary(|recovery| self.host.service(recovery));
    }

    fn close(&self) {
        RecoveryScope::boundary(|recovery| self.host.close(recovery));
    }

    fn queue(&self) -> HapticTicket {
        match self.handle.perform(HapticFeedback::LightImpact, FRAME) {
            HapticOutcome::Queued(ticket) => ticket,
            outcome => panic!("expected a queued request, got {outcome:?}"),
        }
    }

    fn now(&self) -> web_time::Instant {
        flui_foundation::MonotonicClock::now(&self.clock)
    }

    fn played(&self) -> usize {
        self.fake.calls().len()
    }
}

fn panic_text(payload: &(dyn std::any::Any + Send)) -> &str {
    flui_foundation::panic::payload_text(payload).unwrap_or("<opaque>")
}

fn run_table(table: &str, cases: &[(&str, fn())]) {
    let failed: Vec<&str> = cases
        .iter()
        .filter(|(_, case)| catch_unwind(*case).is_err())
        .map(|(name, _)| *name)
        .collect();
    assert!(failed.is_empty(), "{table}: failing cases: {failed:?}");
}

// --- frame_presented_haptics_play_with_the_frame ----------------------------

fn the_frame_is_requested_and_the_ticket_matched() {
    let f = fixture();
    let delivered = f.observe();
    let ticket = f.queue();
    assert_eq!(f.frames.get(), 1, "queueing requests a frame");
    assert_eq!(f.played(), 0, "nothing plays before the frame");
    f.host.frame_presented();
    assert_eq!(f.played(), 1);
    assert!(delivered.borrow().is_empty(), "the frame runs no observer");
    f.service();
    assert_eq!(*delivered.borrow(), [(ticket, HapticOutcome::Submitted)]);
}

fn an_expired_request_is_suppressed_and_never_played_late() {
    let f = fixture();
    let delivered = f.observe();
    let ticket = f.queue();
    let wake = f.host.next_wake().expect("a queued request needs a wake");
    assert!(
        wake > f.now() + DEADLINE,
        "the wake is strictly after the deadline"
    );
    f.clock.advance(DEADLINE);
    f.service();
    assert!(
        delivered.borrow().is_empty(),
        "now == deadline has not expired"
    );
    f.clock.advance(Duration::from_nanos(1));
    f.service();
    assert_eq!(*delivered.borrow(), [(ticket, HapticOutcome::Suppressed)]);
    f.host.frame_presented();
    assert_eq!(f.played(), 0, "an expired request never plays");
    assert_eq!(f.host.next_wake(), None);
}

fn a_frame_presented_exactly_at_the_deadline_plays() {
    let f = fixture();
    let delivered = f.observe();
    let ticket = f.queue();
    f.clock.advance(DEADLINE);
    f.host.frame_presented();
    f.service();
    assert_eq!(*delivered.borrow(), [(ticket, HapticOutcome::Submitted)]);
    assert_eq!(f.played(), 1);
}

fn a_frame_after_the_deadline_suppresses_whichever_wake_comes_first() {
    let f = fixture();
    let delivered = f.observe();
    let ticket = f.queue();
    f.clock.advance(DEADLINE + Duration::from_nanos(1));
    f.host.frame_presented();
    f.service();
    assert_eq!(*delivered.borrow(), [(ticket, HapticOutcome::Suppressed)]);
    assert_eq!(f.played(), 0);
}

fn hiding_drains_the_queue_and_nothing_plays_while_hidden() {
    let f = fixture();
    let delivered = f.observe();
    let first = f.queue();
    let second = f.queue();
    f.host.set_visible(false);
    assert_eq!(
        f.handle.perform(HapticFeedback::LightImpact, NOW),
        HapticOutcome::Suppressed
    );
    assert_eq!(
        f.handle.perform(HapticFeedback::LightImpact, FRAME),
        HapticOutcome::Suppressed,
        "a hidden window queues nothing"
    );
    f.host.frame_presented();
    f.service();
    assert_eq!(
        *delivered.borrow(),
        [
            (first, HapticOutcome::Suppressed),
            (second, HapticOutcome::Suppressed)
        ]
    );
    assert_eq!(f.played(), 0);
    f.host.set_visible(true);
    assert_eq!(
        f.handle.perform(HapticFeedback::LightImpact, NOW),
        HapticOutcome::Submitted
    );
}

fn turning_haptics_off_drains_the_queue_and_nothing_plays_while_off() {
    let f = fixture();
    let delivered = f.observe();
    let first = f.queue();
    let second = f.queue();
    f.host.set_preference(false);
    assert!(
        f.host.has_pending_delivery(),
        "the queued requests resolved when haptics were turned off, before any frame or deadline"
    );
    assert_eq!(
        f.handle.perform(HapticFeedback::LightImpact, NOW),
        HapticOutcome::Suppressed
    );
    assert_eq!(
        f.handle.perform(HapticFeedback::LightImpact, FRAME),
        HapticOutcome::Suppressed,
        "haptics turned off queue nothing"
    );
    f.service();
    assert_eq!(
        *delivered.borrow(),
        [
            (first, HapticOutcome::Suppressed),
            (second, HapticOutcome::Suppressed)
        ]
    );
    f.host.frame_presented();
    f.service();
    assert_eq!(f.played(), 0, "a frame presented while off plays nothing");
    assert_eq!(delivered.borrow().len(), 2);
    f.host.set_preference(true);
    assert_eq!(
        f.handle.perform(HapticFeedback::LightImpact, NOW),
        HapticOutcome::Submitted
    );
    let third = f.queue();
    f.host.frame_presented();
    f.service();
    assert_eq!(
        delivered.borrow().last(),
        Some(&(third, HapticOutcome::Submitted)),
        "turned on again, a frame-aligned request plays with its frame"
    );
    assert_eq!(f.played(), 2);
}

fn a_visible_surface_that_never_presents_resolves_by_its_wake() {
    let f = fixture();
    let delivered = f.observe();
    let ticket = f.queue();
    let wake = f.host.next_wake().expect("deadline wake");
    f.clock.advance(wake - f.now());
    f.service();
    assert_eq!(*delivered.borrow(), [(ticket, HapticOutcome::Suppressed)]);
}

fn a_request_outside_dispatch_carries_no_sequence() {
    let f = fixture();
    let _ = f.handle.perform(HapticFeedback::SelectionClick, NOW);
    assert_eq!(f.fake.calls()[0].request.sequence, None);
}

fn ticket_exhaustion_degrades_to_immediate() {
    let f = fixture_with(|host| host.tickets_after(u64::MAX - 1));
    let last = f.queue();
    assert_eq!(last.get().get(), u64::MAX, "the terminal ticket is issued");
    assert_eq!(
        f.handle.perform(HapticFeedback::LightImpact, FRAME),
        HapticOutcome::Submitted,
        "after the terminal ticket a request plays at once"
    );
    assert_eq!(f.played(), 1);
    assert_eq!(f.frames.get(), 1, "the degraded request queued nothing");
}

fn both_tiers_queue() {
    let f = fixture();
    let delivered = f.observe();
    let pattern = HapticPattern::builder()
        .transient(Duration::ZERO, 1.0, 1.0)
        .build()
        .expect("valid");
    let HapticOutcome::Queued(ticket) = f.handle.play(&pattern, HapticFeedback::HeavyImpact, FRAME)
    else {
        panic!("a pattern queues too");
    };
    f.host.frame_presented();
    f.service();
    assert_eq!(*delivered.borrow(), [(ticket, HapticOutcome::Submitted)]);
}

#[test]
fn frame_presented_haptics_play_with_the_frame() {
    run_table(
        "frame-presented haptics",
        &[
            (
                "the_frame_is_requested_and_the_ticket_matched",
                the_frame_is_requested_and_the_ticket_matched,
            ),
            (
                "an_expired_request_is_suppressed_and_never_played_late",
                an_expired_request_is_suppressed_and_never_played_late,
            ),
            (
                "a_frame_presented_exactly_at_the_deadline_plays",
                a_frame_presented_exactly_at_the_deadline_plays,
            ),
            (
                "a_frame_after_the_deadline_suppresses_whichever_wake_comes_first",
                a_frame_after_the_deadline_suppresses_whichever_wake_comes_first,
            ),
            (
                "hiding_drains_the_queue_and_nothing_plays_while_hidden",
                hiding_drains_the_queue_and_nothing_plays_while_hidden,
            ),
            (
                "turning_haptics_off_drains_the_queue_and_nothing_plays_while_off",
                turning_haptics_off_drains_the_queue_and_nothing_plays_while_off,
            ),
            (
                "a_visible_surface_that_never_presents_resolves_by_its_wake",
                a_visible_surface_that_never_presents_resolves_by_its_wake,
            ),
            (
                "a_request_outside_dispatch_carries_no_sequence",
                a_request_outside_dispatch_carries_no_sequence,
            ),
            (
                "ticket_exhaustion_degrades_to_immediate",
                ticket_exhaustion_degrades_to_immediate,
            ),
            ("both_tiers_queue", both_tiers_queue),
        ],
    );
}

// --- deadline ---------------------------------------------------------------

fn overflowing_deadline() {
    let f = fixture_with(|host| host.deadline_factor(NonZeroU32::MIN));
    f.period.set(Some(Duration::from_nanos(u64::MAX)));
    f.clock.advance(Duration::from_nanos(1));
    assert_eq!(
        f.handle.perform(HapticFeedback::LightImpact, FRAME),
        HapticOutcome::Suppressed
    );
    assert_eq!((f.played(), f.frames.get()), (0, 0));
    assert_eq!(f.host.next_wake(), None, "nothing was queued");
    f.period.set(Some(PERIOD));
    assert_eq!(
        f.queue().get().get(),
        1,
        "the refused request consumed no ticket"
    );
}

fn deadline_at_the_terminal_time() {
    let f = fixture_with(|host| host.deadline_factor(NonZeroU32::MIN));
    f.period.set(Some(Duration::from_nanos(u64::MAX)));
    assert_eq!(
        f.handle.perform(HapticFeedback::LightImpact, FRAME),
        HapticOutcome::Suppressed,
        "no time lies after the terminal EventTime to expire at"
    );
    assert_eq!((f.played(), f.frames.get()), (0, 0));
    assert_eq!(f.host.next_wake(), None);
}

#[test]
fn deadline_overflow_resolves_without_queueing() {
    run_table(
        "deadline overflow",
        &[
            ("overflowing_deadline", overflowing_deadline),
            (
                "deadline_at_the_terminal_time",
                deadline_at_the_terminal_time,
            ),
        ],
    );
}

fn two(period: Option<Duration>) -> Duration {
    haptic_frame_deadline(NonZeroU32::new(2).expect("nonzero"), period)
}

fn falls_back_with_a_diagnostic(period: Option<Duration>) {
    let (deadline, log) = flui_testing::log_capture::capture(|| two(period));
    assert_eq!(deadline, FALLBACK_REFRESH_PERIOD * 2, "{period:?}");
    assert_eq!(
        log.count_containing("the 60 Hz period"),
        1,
        "{period:?} is reported: {log}"
    );
}

fn unknown_period() {
    falls_back_with_a_diagnostic(None);
    assert_eq!(FALLBACK_REFRESH_PERIOD, Duration::from_nanos(16_666_667));
}

fn zero_period() {
    falls_back_with_a_diagnostic(Some(Duration::ZERO));
}

fn overflowing_product() {
    falls_back_with_a_diagnostic(Some(Duration::MAX));
}

fn known_period() {
    let (deadline, log) =
        flui_testing::log_capture::capture(|| two(Some(Duration::from_nanos(8_333_333))));
    assert_eq!(deadline, Duration::from_nanos(16_666_666));
    assert_eq!(log.count_containing("the 60 Hz period"), 0, "{log}");
}

fn a_queued_request_keeps_its_snapshot_across_a_display_change() {
    let f = fixture();
    let delivered = f.observe();
    let ticket = f.queue();
    f.period.set(Some(PERIOD * 4));
    let later = f.queue();
    f.clock.advance(DEADLINE + Duration::from_nanos(1));
    f.service();
    assert_eq!(
        *delivered.borrow(),
        [(ticket, HapticOutcome::Suppressed)],
        "the queued request kept the deadline it was enqueued with"
    );
    f.clock.advance(DEADLINE * 3);
    f.service();
    assert_eq!(
        delivered.borrow().last(),
        Some(&(later, HapticOutcome::Suppressed)),
        "a request queued after the change waits four periods of the new display"
    );
}

#[test]
fn unknown_refresh_period_uses_the_60_hz_fallback() {
    run_table(
        "frame deadline",
        &[
            ("unknown_period", unknown_period),
            ("zero_period", zero_period),
            ("overflowing_product", overflowing_product),
            ("known_period", known_period),
            (
                "a_queued_request_keeps_its_snapshot_across_a_display_change",
                a_queued_request_keeps_its_snapshot_across_a_display_change,
            ),
        ],
    );
}

// --- the outcome mailbox ----------------------------------------------------

/// Queue `count` requests and present their frame, leaving their outcomes in
/// the mailbox.
fn resolved(f: &Fixture, count: usize) -> Vec<HapticTicket> {
    let tickets = (0..count).map(|_| f.queue()).collect();
    f.host.frame_presented();
    tickets
}

#[test]
fn queued_outcomes_wait_for_an_observer() {
    let f = fixture();
    let tickets = resolved(&f, 2);
    f.service();
    assert_eq!(f.host.next_wake(), None, "no observer, no wake");
    let delivered = f.observe();
    assert!(
        f.host.has_pending_delivery(),
        "a registration starts a pass"
    );
    assert!(f.host.next_wake().is_some());
    f.service();
    let expected: Vec<_> = tickets
        .into_iter()
        .map(|ticket| (ticket, HapticOutcome::Submitted))
        .collect();
    assert_eq!(*delivered.borrow(), expected);
}

#[test]
fn an_observer_replaced_mid_delivery_hands_over_the_rest() {
    let f = fixture();
    let tickets = resolved(&f, 3);
    let first: Delivered = Rc::default();
    let second: Delivered = Rc::default();
    let registration: Registration = Rc::default();
    let me = Rc::clone(&registration);
    let (into_first, into_second) = (Rc::clone(&first), Rc::clone(&second));
    let _ = registration.set(f.handle.observe(move |ticket, outcome| {
        into_first.borrow_mut().push((ticket, outcome));
        let into_second = Rc::clone(&into_second);
        me.get()
            .expect("registered")
            .replace(move |ticket, outcome| into_second.borrow_mut().push((ticket, outcome)));
    }));
    f.service();
    assert_eq!(*first.borrow(), [(tickets[0], HapticOutcome::Submitted)]);
    assert_eq!(
        *second.borrow(),
        [
            (tickets[1], HapticOutcome::Submitted),
            (tickets[2], HapticOutcome::Submitted)
        ]
    );
}

#[test]
fn an_observer_panic_keeps_the_outcome_for_the_next_pass() {
    let f = fixture();
    let tickets = resolved(&f, 2);
    let delivered: Delivered = Rc::default();
    let sink = Rc::clone(&delivered);
    let panicked = Rc::new(Cell::new(false));
    let once = Rc::clone(&panicked);
    let _registration = f.handle.observe(move |ticket, outcome| {
        assert!(once.replace(true), "observer failed");
        sink.borrow_mut().push((ticket, outcome));
    });
    let failure =
        catch_unwind(AssertUnwindSafe(|| f.service())).expect_err("contained, then reported");
    assert_eq!(panic_text(failure.as_ref()), "observer failed");
    assert!(delivered.borrow().is_empty());
    f.service();
    assert_eq!(
        *delivered.borrow(),
        [
            (tickets[0], HapticOutcome::Submitted),
            (tickets[1], HapticOutcome::Submitted)
        ],
        "the head stayed, with the tail behind it"
    );
}

#[test]
fn an_observer_that_installs_a_replacement_then_panics_hands_over_the_head() {
    let f = fixture();
    let tickets = resolved(&f, 2);
    let delivered: Delivered = Rc::default();
    let calls = Rc::new(Cell::new(0));
    let registration: Registration = Rc::default();
    let me = Rc::clone(&registration);
    let (sink, counted) = (Rc::clone(&delivered), Rc::clone(&calls));
    let _ = registration.set(f.handle.observe(move |_, _| {
        counted.set(counted.get() + 1);
        let sink = Rc::clone(&sink);
        me.get()
            .expect("registered")
            .replace(move |ticket, outcome| sink.borrow_mut().push((ticket, outcome)));
        panic!("after replacing");
    }));
    let failure = catch_unwind(AssertUnwindSafe(|| f.service())).expect_err("reported");
    assert_eq!(panic_text(failure.as_ref()), "after replacing");
    assert_eq!(
        calls.get(),
        1,
        "the panicking observer is retired, never restored"
    );
    assert_eq!(
        *delivered.borrow(),
        [
            (tickets[0], HapticOutcome::Submitted),
            (tickets[1], HapticOutcome::Submitted)
        ],
        "the replacement receives the retained head and the tail"
    );
}

/// A capture whose destructor panics.
struct PanicsOnDrop;
impl Drop for PanicsOnDrop {
    fn drop(&mut self) {
        assert!(std::thread::panicking(), "observer capture dropped");
    }
}

#[test]
fn a_replaced_observer_whose_drop_panics_does_not_stop_delivery() {
    let f = fixture();
    let tickets = resolved(&f, 3);
    let delivered: Delivered = Rc::default();
    let registration: Registration = Rc::default();
    let me = Rc::clone(&registration);
    let sink = Rc::clone(&delivered);
    let capture = PanicsOnDrop;
    let _ = registration.set(f.handle.observe(move |ticket, outcome| {
        let _ = &capture;
        sink.borrow_mut().push((ticket, outcome));
        let sink = Rc::clone(&sink);
        me.get()
            .expect("registered")
            .replace(move |ticket, outcome| sink.borrow_mut().push((ticket, outcome)));
    }));
    let failure =
        catch_unwind(AssertUnwindSafe(|| f.service())).expect_err("the drop panic is reported");
    assert_eq!(panic_text(failure.as_ref()), "observer capture dropped");
    let expected: Vec<_> = tickets
        .into_iter()
        .map(|ticket| (ticket, HapticOutcome::Submitted))
        .collect();
    assert_eq!(
        *delivered.borrow(),
        expected,
        "the replacement drained the rest"
    );
    assert!(!f.host.has_pending_delivery());
}

#[test]
fn an_observer_panic_on_an_idle_presentation_is_retried_by_a_wake() {
    let f = fixture();
    let tickets = resolved(&f, 1);
    let delivered: Delivered = Rc::default();
    let sink = Rc::clone(&delivered);
    let panicked = Rc::new(Cell::new(false));
    let once = Rc::clone(&panicked);
    let _registration = f.handle.observe(move |ticket, outcome| {
        assert!(once.replace(true), "transient failure");
        sink.borrow_mut().push((ticket, outcome));
    });
    let _ = catch_unwind(AssertUnwindSafe(|| f.service()));
    assert_eq!(
        f.host.next_wake(),
        Some(f.now()),
        "the failed pass wakes the owner at once, with no frame or input"
    );
    f.service();
    assert_eq!(
        *delivered.borrow(),
        [(tickets[0], HapticOutcome::Submitted)]
    );
    assert_eq!(f.host.next_wake(), None);
}

#[test]
fn a_permanently_panicking_observer_stops_waking_after_the_bound() {
    let f = fixture();
    let _ = resolved(&f, 1);
    let calls = Rc::new(Cell::new(0));
    let counted = Rc::clone(&calls);
    let _registration = f.handle.observe(move |_, _| {
        counted.set(counted.get() + 1);
        panic!("always");
    });
    for _ in 0..3 {
        assert!(f.host.next_wake().is_some());
        let _ = catch_unwind(AssertUnwindSafe(|| f.service()));
    }
    assert_eq!(calls.get(), 3);
    assert_eq!(
        f.host.next_wake(),
        None,
        "no wake after three failed passes"
    );
    f.service();
    assert_eq!(calls.get(), 3, "the head waits for a new registration");
    let delivered = f.observe();
    assert!(
        f.host.next_wake().is_some(),
        "a new registration is due at once"
    );
    f.service();
    assert_eq!(delivered.borrow().len(), 1);
}

#[test]
fn closing_delivers_pending_outcomes_in_a_final_pass() {
    let f = fixture();
    let delivered = f.observe();
    let played = f.queue();
    f.host.frame_presented();
    let pending = f.queue();
    f.close();
    assert_eq!(
        *delivered.borrow(),
        [
            (played, HapticOutcome::Submitted),
            (pending, HapticOutcome::Suppressed)
        ]
    );
    assert_eq!(
        f.handle.perform(HapticFeedback::LightImpact, NOW),
        HapticOutcome::Suppressed,
        "a closed presentation plays nothing"
    );
    assert_eq!(f.host.next_wake(), None);

    // With no observer the final pass delivers nothing; the entries are
    // reported and retired with the presentation.
    let g = fixture();
    let _ = g.queue();
    g.close();
    let late = g.observe();
    g.service();
    assert!(late.borrow().is_empty());
}

#[test]
fn sustained_requests_with_no_observer_degrade_to_immediate_when_the_mailbox_is_full() {
    let f = fixture();
    let tickets = resolved(&f, MAILBOX_CAPACITY / 2);
    let queued: Vec<_> = (0..MAILBOX_CAPACITY / 2).map(|_| f.queue()).collect();
    assert_eq!(tickets.len() + queued.len(), MAILBOX_CAPACITY);
    let played = f.played();
    assert_eq!(
        f.handle.perform(HapticFeedback::LightImpact, FRAME),
        HapticOutcome::Submitted,
        "a full mailbox refuses admission and plays at once"
    );
    assert_eq!(f.played(), played + 1);
    let delivered = f.observe();
    f.host.frame_presented();
    f.service();
    assert_eq!(delivered.borrow().len(), MAILBOX_CAPACITY);
    assert!(
        matches!(f.handle.perform(HapticFeedback::LightImpact, FRAME), HapticOutcome::Queued(t) if t.get().get() == MAILBOX_CAPACITY as u64 + 1),
        "once drained, requests queue again and the refused one took no ticket"
    );
}

/// Reaching the actuator for a queued request hides the window: the request
/// resolves once, suppressed, and does not play.
#[test]
fn hiding_the_window_while_a_frame_plays_suppresses_the_request_once() {
    let clock = ManualClock::new();
    let binding = GestureBinding::with_clock(Arc::new(clock));
    let fake = Arc::new(FakeHaptics::new());
    let host_slot: Rc<std::cell::OnceCell<std::rc::Weak<HapticsHost>>> = Rc::default();
    let hide = Rc::new(Cell::new(false));
    let host = {
        let (fake, host_slot, hide) = (Arc::clone(&fake), Rc::clone(&host_slot), Rc::clone(&hide));
        Rc::new(
            HapticsHost::builder(&binding, move || {
                if hide.replace(false)
                    && let Some(host) = host_slot.get().and_then(std::rc::Weak::upgrade)
                {
                    host.set_visible(false);
                }
                Some(Arc::clone(&fake) as Arc<dyn PlatformHaptics>)
            })
            .refresh_period(|| Some(PERIOD))
            .build(),
        )
    };
    let _ = host_slot.set(Rc::downgrade(&host));
    let handle = host.handle();
    let delivered: Delivered = Rc::default();
    let sink = Rc::clone(&delivered);
    let _registration = handle.observe(move |ticket, outcome| {
        sink.borrow_mut().push((ticket, outcome));
    });
    let tickets: Vec<_> = (0..2)
        .map(
            |_| match handle.perform(HapticFeedback::LightImpact, FRAME) {
                HapticOutcome::Queued(ticket) => ticket,
                outcome => panic!("expected a queued request, got {outcome:?}"),
            },
        )
        .collect();
    hide.set(true);
    host.frame_presented();
    RecoveryScope::boundary(|recovery| host.service(recovery));
    assert_eq!(
        *delivered.borrow(),
        [
            (tickets[0], HapticOutcome::Suppressed),
            (tickets[1], HapticOutcome::Suppressed)
        ],
        "each resolves once, in ticket order"
    );
    assert!(fake.calls().is_empty(), "a suppressed request never plays");
}

/// Reaching the actuator for a queued request closes the presentation: the
/// request being played and the one queued behind it are both reported with
/// their tickets, as a close reports what it could not deliver.
#[test]
fn closing_while_a_frame_plays_reports_every_outcome() {
    let clock = ManualClock::new();
    let binding = GestureBinding::with_clock(Arc::new(clock));
    let fake = Arc::new(FakeHaptics::new());
    let host_slot: Rc<std::cell::OnceCell<std::rc::Weak<HapticsHost>>> = Rc::default();
    let close = Rc::new(Cell::new(false));
    let host = {
        let (fake, host_slot, close) =
            (Arc::clone(&fake), Rc::clone(&host_slot), Rc::clone(&close));
        Rc::new(
            HapticsHost::builder(&binding, move || {
                if close.replace(false)
                    && let Some(host) = host_slot.get().and_then(std::rc::Weak::upgrade)
                {
                    RecoveryScope::boundary(|recovery| host.close(recovery));
                }
                Some(Arc::clone(&fake) as Arc<dyn PlatformHaptics>)
            })
            .refresh_period(|| Some(PERIOD))
            .build(),
        )
    };
    let _ = host_slot.set(Rc::downgrade(&host));
    let handle = host.handle();
    for _ in 0..2 {
        assert!(matches!(
            handle.perform(HapticFeedback::LightImpact, FRAME),
            HapticOutcome::Queued(_)
        ));
    }
    close.set(true);
    let ((), log) = flui_testing::log_capture::capture(|| host.frame_presented());
    assert_eq!(
        log.count_containing("not delivered before its presentation closed"),
        2,
        "both outcomes are reported: {log}"
    );
    assert!(fake.calls().is_empty(), "nothing plays once closed");
}

/// Notes that it was dropped.
struct DropFlag(Rc<Cell<bool>>);
impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.set(true);
    }
}

/// Withdrawn after a failure, the host runs none of its observers' code: a
/// pending outcome is not delivered, and the observer is retained, so its
/// captures are never dropped, not even with the host.
#[test]
fn withdrawing_runs_no_observer_code() {
    let f = fixture();
    let dropped = Rc::new(Cell::new(false));
    let calls = Rc::new(Cell::new(0));
    let capture = DropFlag(Rc::clone(&dropped));
    let counted = Rc::clone(&calls);
    let registration = f.handle.observe(move |_, _| {
        let _ = &capture;
        counted.set(counted.get() + 1);
    });
    let _ = resolved(&f, 1);
    let _ = f.queue();
    f.host.withdraw();
    assert_eq!(calls.get(), 0, "no outcome is delivered");
    assert!(!dropped.get(), "the observer is retained");
    assert!(!f.host.has_pending_delivery());
    assert_eq!(f.host.next_wake(), None, "the queue was resolved");
    assert_eq!(
        f.handle.perform(HapticFeedback::LightImpact, FRAME),
        HapticOutcome::Suppressed,
        "a withdrawn presentation plays nothing"
    );
    drop(registration);
    let Fixture { host, handle, .. } = f;
    drop(handle);
    drop(host);
    assert!(!dropped.get(), "not even with the host");
    assert_eq!(calls.get(), 0);
}

/// A backend whose `failing`th call panics.
struct FailsOnCall {
    calls: std::sync::atomic::AtomicUsize,
    failing: usize,
}

impl PlatformHaptics for FailsOnCall {
    fn perform(&self, _: HapticFeedback) {
        let call = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        assert!(call != self.failing, "backend failed");
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// The backend panics on the second of three requests a frame plays: the
/// first's outcome is already in the mailbox, and the other two stay queued
/// for the next frame within their deadline.
#[test]
fn a_backend_panic_in_a_frame_keeps_every_other_request() {
    let clock = ManualClock::new();
    let binding = GestureBinding::with_clock(Arc::new(clock));
    let backend: Arc<dyn PlatformHaptics> = Arc::new(FailsOnCall {
        calls: std::sync::atomic::AtomicUsize::new(0),
        failing: 2,
    });
    let host = HapticsHost::builder(&binding, move || Some(Arc::clone(&backend)))
        .refresh_period(|| Some(PERIOD))
        .build();
    let handle = host.handle();
    let delivered: Delivered = Rc::default();
    let sink = Rc::clone(&delivered);
    let _registration = handle.observe(move |ticket, outcome| {
        sink.borrow_mut().push((ticket, outcome));
    });
    let tickets: Vec<_> = (0..3)
        .map(
            |_| match handle.perform(HapticFeedback::LightImpact, FRAME) {
                HapticOutcome::Queued(ticket) => ticket,
                outcome => panic!("expected a queued request, got {outcome:?}"),
            },
        )
        .collect();
    let failure = catch_unwind(AssertUnwindSafe(|| host.frame_presented()))
        .expect_err("the backend's panic propagates");
    assert_eq!(panic_text(failure.as_ref()), "backend failed");
    RecoveryScope::boundary(|recovery| host.service(recovery));
    assert_eq!(
        *delivered.borrow(),
        [(tickets[0], HapticOutcome::Submitted)],
        "the outcome resolved before the panic was kept"
    );
    host.frame_presented();
    RecoveryScope::boundary(|recovery| host.service(recovery));
    let expected: Vec<_> = tickets
        .into_iter()
        .map(|ticket| (ticket, HapticOutcome::Submitted))
        .collect();
    assert_eq!(
        *delivered.borrow(),
        expected,
        "the failed request and the one behind it play with the next frame"
    );
}

#[test]
fn several_observers_each_receive_every_outcome_once() {
    let f = fixture();
    let tickets = resolved(&f, 2);
    let first: Delivered = Rc::default();
    let sink = Rc::clone(&first);
    let panicked = Rc::new(Cell::new(false));
    let once = Rc::clone(&panicked);
    let _first = f.handle.observe(move |ticket, outcome| {
        assert!(once.replace(true), "first observer failed once");
        sink.borrow_mut().push((ticket, outcome));
    });
    let second = f.observe();
    let failure = catch_unwind(AssertUnwindSafe(|| f.service())).expect_err("reported");
    assert_eq!(panic_text(failure.as_ref()), "first observer failed once");
    assert!(
        second.borrow().is_empty(),
        "the failed pass ended at the head"
    );
    f.service();
    let expected: Vec<_> = tickets
        .into_iter()
        .map(|ticket| (ticket, HapticOutcome::Submitted))
        .collect();
    assert_eq!(*first.borrow(), expected);
    assert_eq!(
        *second.borrow(),
        expected,
        "no observer receives an outcome twice"
    );
    assert!(!f.host.has_pending_delivery());
}

#[test]
fn removing_an_observer_releases_the_head_it_held() {
    let f = fixture();
    let tickets = resolved(&f, 2);
    let failing = f.handle.observe(|_, _| panic!("always"));
    let healthy = f.observe();
    for _ in 0..3 {
        let _ = catch_unwind(AssertUnwindSafe(|| f.service()));
    }
    f.service();
    assert_eq!(
        *healthy.borrow(),
        [(tickets[0], HapticOutcome::Submitted)],
        "a healthy observer still receives the head"
    );
    assert_eq!(
        f.host.next_wake(),
        None,
        "the bounded observer holds the head"
    );
    drop(failing);
    assert!(
        f.host.has_pending_delivery(),
        "removal makes the next pass due"
    );
    f.service();
    let expected: Vec<_> = tickets
        .into_iter()
        .map(|ticket| (ticket, HapticOutcome::Submitted))
        .collect();
    assert_eq!(*healthy.borrow(), expected);
}

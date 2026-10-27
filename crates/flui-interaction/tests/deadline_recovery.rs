//! Owner deadline recovery uses the same binding and open members as consumers.

use flui_interaction::gesture::GestureContest;
use std::{
    cell::{Cell, RefCell},
    panic::AssertUnwindSafe,
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use flui_foundation::{
    MonotonicClock as _,
    geometry::Offset,
    panic::{FailureClass, PanicRecovery},
};
use flui_interaction::{
    GestureBinding, GestureDeadlineError, GestureDeadlineUpdate, ManualClock, PointerId,
    gesture::GestureArenaMember,
};
use flui_platform::EventTime;

thread_local! {
    static WAKE_ACTION: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
}

struct WakeWindow {
    drops: Arc<std::sync::atomic::AtomicUsize>,
    panic_on_drop: bool,
}

impl flui_platform::PlatformWindow for WakeWindow {
    fn id(&self) -> flui_platform::WindowId {
        flui_platform::WindowId(1)
    }
    fn request_redraw(&self) {
        let action = WAKE_ACTION.with(|action| action.borrow_mut().take());
        if let Some(action) = action {
            action();
        }
    }
    fn set_frame_rate_hint(
        &self,
        _: flui_platform::FrameRateHint,
    ) -> Result<(), flui_platform::Unsupported> {
        Err(flui_platform::Unsupported::new(
            flui_platform::UnsupportedReason::NotOnThisPlatform,
        ))
    }

    fn set_input_regions(
        &self,
        _: &[flui_platform::input_regions::InputRegion],
    ) -> Result<(), flui_platform::Unsupported> {
        Err(flui_platform::Unsupported::new(
            flui_platform::UnsupportedReason::NotOnThisPlatform,
        ))
    }

    fn set_material(
        &self,
        _: flui_platform::WindowMaterial,
    ) -> Result<(), flui_platform::Unsupported> {
        Err(flui_platform::Unsupported::new(
            flui_platform::UnsupportedReason::NotOnThisPlatform,
        ))
    }

    fn set_cursor(&self, _: flui_platform::CursorIcon) -> Result<(), flui_platform::CursorError> {
        Ok(())
    }
    fn state(&self) -> flui_platform::WindowState {
        let size = flui_foundation::geometry::Size::new(1.0, 1.0);
        flui_platform::WindowState::new(flui_platform::WindowGeometry::new(None, size), size)
            .with_focused(true)
            .with_metrics(flui_foundation::geometry::WindowMetrics::from_logical(size, 1.0).ok())
    }

    fn apply(
        &self,
        command: flui_platform::AdmittedCommand,
    ) -> Result<(), flui_platform::ApplyError> {
        command.against(&flui_platform::WindowCapabilities::NONE)?;
        Ok(())
    }

    fn text_input(&self) -> Option<std::sync::Arc<dyn flui_platform::PlatformTextInput>> {
        None
    }

    fn haptics(&self) -> Option<std::sync::Arc<dyn flui_platform::PlatformHaptics>> {
        None
    }

    fn system_chrome(&self) -> Option<std::sync::Arc<dyn flui_platform::PlatformSystemChrome>> {
        None
    }
    fn drag_source(&self) -> Option<std::sync::Arc<dyn flui_platform::drag::PlatformDragSource>> {
        None
    }
}

impl Drop for WakeWindow {
    fn drop(&mut self) {
        self.drops
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        assert!(!self.panic_on_drop, "wake backend capture Drop failure");
    }
}

struct CallbackCapture(Rc<Cell<usize>>);
impl CallbackCapture {
    fn observe(&self) -> usize {
        self.0.get()
    }
}
impl Drop for CallbackCapture {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

type MemberOwner = Rc<RefCell<Option<Rc<DeadlineMember>>>>;

struct VerdictMember {
    owner: std::rc::Weak<RefCell<Option<Rc<Self>>>>,
    notifications: Rc<Cell<usize>>,
    drops: Rc<Cell<usize>>,
    panic_on_drop: bool,
    panic_on_notify: bool,
    reenter: RefCell<Option<Box<dyn FnOnce()>>>,
    _drop_capture: Option<VerdictDropFailure>,
}

struct VerdictDropFailure;

impl Drop for VerdictDropFailure {
    fn drop(&mut self) {
        panic!("opaque arena member capture retirement");
    }
}

impl VerdictMember {
    fn notified(&self) {
        self.notifications.set(self.notifications.get() + 1);
        let owner = self.owner.upgrade().expect("live external owner slot");
        drop(owner.borrow_mut().take());
        let reenter = self.reenter.borrow_mut().take();
        if let Some(reenter) = reenter {
            reenter();
        }
        assert!(!self.panic_on_notify, "arena verdict callback failure");
    }
}

impl GestureArenaMember for VerdictMember {
    fn accept_gesture(&self, _: GestureContest) {
        self.notified();
    }
    fn reject_gesture(&self, _: GestureContest) {
        self.notified();
    }
}

impl Drop for VerdictMember {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
        assert!(!self.panic_on_drop, "opaque arena member retirement");
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RejectionDeadlineObservation {
    Unobserved,
    Armed(EventTime),
    Disarmed,
}

struct DeadlineMember {
    due: Cell<Option<EventTime>>,
    fail_query: Cell<bool>,
    fail_poll: Cell<bool>,
    queries: Cell<usize>,
    effects: Cell<usize>,
    query_publication: RefCell<Option<(GestureDeadlineUpdate, Option<EventTime>)>>,
    poll_publication: RefCell<Option<(GestureDeadlineUpdate, Option<EventTime>)>>,
    release_owner: RefCell<Option<MemberOwner>>,
    drops: Rc<Cell<usize>>,
    rejection_binding: RefCell<Option<Rc<GestureBinding>>>,
    rejection_deadline: Cell<RejectionDeadlineObservation>,
}

impl DeadlineMember {
    fn new(due: EventTime) -> Rc<Self> {
        Rc::new(Self {
            due: Cell::new(Some(due)),
            fail_query: Cell::new(false),
            fail_poll: Cell::new(false),
            queries: Cell::new(0),
            effects: Cell::new(0),
            query_publication: RefCell::new(None),
            poll_publication: RefCell::new(None),
            release_owner: RefCell::new(None),
            drops: Rc::new(Cell::new(0)),
            rejection_binding: RefCell::new(None),
            rejection_deadline: Cell::new(RejectionDeadlineObservation::Unobserved),
        })
    }
}

impl GestureArenaMember for DeadlineMember {
    fn accept_gesture(&self, _: GestureContest) {}
    fn reject_gesture(&self, _: GestureContest) {
        let binding = self.rejection_binding.borrow().clone();
        if let Some(binding) = binding
            && self.rejection_deadline.get() == RejectionDeadlineObservation::Unobserved
        {
            let observation = match flui_interaction::__runtime::next_deadline_snapshot(&binding) {
                Some(due) => RejectionDeadlineObservation::Armed(due),
                None => RejectionDeadlineObservation::Disarmed,
            };
            self.rejection_deadline.set(observation);
        }
    }
    fn deadline(&self) -> Option<EventTime> {
        self.queries.set(self.queries.get() + 1);
        let publication = self.query_publication.borrow_mut().take();
        if let Some((update, due)) = publication {
            update
                .publish(due)
                .expect("reentrant query update is accepted");
        }
        let release = self.release_owner.borrow_mut().take();
        if let Some(owner) = release {
            drop(owner.borrow_mut().take());
        }
        assert!(!self.fail_query.get(), "deadline query failure");
        self.due.get()
    }
    fn poll_deadline(&self, _: EventTime) {
        self.effects.set(self.effects.get() + 1);
        let publication = self.poll_publication.borrow_mut().take();
        if let Some((update, due)) = publication {
            update
                .publish(due)
                .expect("reentrant poll update is accepted");
        }
        assert!(
            !self.fail_poll.get(),
            "deadline poll failure after an effect"
        );
        self.due.set(None);
    }
}

impl Drop for DeadlineMember {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}

fn pointer(value: u64) -> PointerId {
    PointerId::try_from(value).expect("fixture pointer is nonzero")
}

/// `clock`'s reading on the event timeline a binding built over it uses.
fn at(clock: &ManualClock) -> EventTime {
    EventTime::from_nanos(u64::try_from(clock.elapsed().as_nanos()).expect("test time fits"))
}

fn sample(binding: &GestureBinding, now: web_time::Instant) -> bool {
    let mut recovery = PanicRecovery::new();
    flui_interaction::__runtime::tick_deadlines_with_recovery(binding, now, &mut recovery.scope());
    recovery.has_failure()
}

fn query_failure_has_one_bounded_probe_and_inert_reads() {
    let clock = ManualClock::new();
    let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
    let member = DeadlineMember::new(at(&clock));
    member.fail_query.set(true);
    let _entry = binding.arena().add(pointer(1), &member);
    let healthy = DeadlineMember::new(at(&clock) + Duration::from_millis(50));
    let _healthy_entry = binding.arena().add(pointer(2), &healthy);
    assert!(sample(&binding, clock.now()));
    assert_eq!(member.queries.get(), 1);
    let deadline = std::panic::catch_unwind(AssertUnwindSafe(|| {
        flui_interaction::__runtime::next_deadline_snapshot(&binding)
    }));
    assert!(
        deadline.is_ok(),
        "a host snapshot read must not invoke a failed query"
    );
    assert_eq!(
        deadline.expect("inert read"),
        at(&clock).checked_add(Duration::from_millis(50))
    );
    clock.advance(Duration::from_millis(99));
    assert!(
        !sample(&binding, clock.now()),
        "the same obligation is reported once"
    );
    assert_eq!(
        member.queries.get(),
        1,
        "an earlier healthy turn cannot probe early"
    );
    assert_eq!(
        healthy.effects.get(),
        1,
        "an earlier healthy deadline progresses before the failed query retry"
    );
    clock.advance(Duration::from_millis(1));
    assert!(
        !sample(&binding, clock.now()),
        "the re-probe cannot repeat the report"
    );
    assert_eq!(member.queries.get(), 2, "one automatic re-probe is due");
    assert_eq!(
        flui_interaction::__runtime::next_deadline_snapshot(&binding),
        None
    );
    clock.advance(Duration::from_secs(5));
    assert!(!sample(&binding, clock.now()));
    assert_eq!(
        member.queries.get(),
        2,
        "repeated query failure parks discovery debt"
    );
}

fn unknown_poll_is_not_replayed_and_healthy_tail_progresses() {
    let clock = ManualClock::new();
    let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
    let failed = DeadlineMember::new(at(&clock));
    failed.fail_poll.set(true);
    let healthy = DeadlineMember::new(at(&clock));
    let _failed_entry = binding.arena().add(pointer(1), &failed);
    let _healthy_entry = binding.arena().add(pointer(2), &healthy);
    assert!(sample(&binding, clock.now()));
    assert_eq!(
        healthy.effects.get(),
        1,
        "a failed open poll cannot erase the healthy tail"
    );
    assert_eq!(failed.effects.get(), 1);
    clock.advance(Duration::from_secs(1));
    assert!(!sample(&binding, clock.now()));
    assert_eq!(
        failed.effects.get(),
        1,
        "an unknown side effect cannot replay for the same revision"
    );
}

fn a_reused_pointer_admits_a_new_deadline_identity() {
    let clock = ManualClock::new();
    let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
    let member = DeadlineMember::new(at(&clock));
    member.fail_poll.set(true);
    let _old = binding.arena().add(pointer(1), &member);
    assert!(sample(&binding, clock.now()));
    binding.arena().sweep(pointer(1));
    let _new = binding.arena().add(pointer(1), &member);
    assert!(
        sample(&binding, clock.now()),
        "new contact identity can attempt its new obligation"
    );
    assert_eq!(member.effects.get(), 2);
}

fn explicit_same_time_rearm_is_independent_and_stale_authority_refuses() {
    let clock = ManualClock::new();
    let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
    let member = DeadlineMember::new(at(&clock));
    member.fail_poll.set(true);
    let erased: Rc<dyn GestureArenaMember> = member.clone();
    let registration = binding
        .arena()
        .register_deadline_member(pointer(1), &erased)
        .expect("one member-wide source");
    let update = registration.update_handle();
    update.publish(Some(at(&clock))).expect("first revision");
    assert!(sample(&binding, clock.now()));
    assert!(!sample(&binding, clock.now()));
    assert_eq!(member.effects.get(), 1);
    update
        .publish(Some(at(&clock)))
        .expect("same numerical deadline is a new revision");
    assert!(sample(&binding, clock.now()));
    assert_eq!(member.effects.get(), 2);
    drop(registration);
    assert_eq!(
        flui_interaction::__runtime::next_deadline_snapshot(&binding),
        None
    );
    let replacement = binding
        .arena()
        .register_deadline_member(pointer(1), &erased)
        .expect("replacement source");
    replacement
        .update_handle()
        .publish(Some(at(&clock) + Duration::from_secs(1)))
        .expect("replacement deadline");
    assert_eq!(
        update.publish(Some(at(&clock))),
        Err(GestureDeadlineError::Retired)
    );
    assert_eq!(
        flui_interaction::__runtime::next_deadline_snapshot(&binding),
        Some(at(&clock) + Duration::from_secs(1))
    );
    flui_interaction::__runtime::close_gestures(
        &binding,
        flui_interaction::__runtime::CloseMode::Ordinary,
    );
    assert_eq!(
        replacement.update_handle().refresh(),
        Err(GestureDeadlineError::Closed)
    );
}

fn a_member_wide_poll_cannot_escape_through_another_captured_slot() {
    let clock = ManualClock::new();
    let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
    let member = DeadlineMember::new(at(&clock));
    member.fail_poll.set(true);
    let _one = binding.arena().add(pointer(1), &member);
    let _two = binding.arena().add(pointer(2), &member);
    assert!(sample(&binding, clock.now()));
    binding.arena().sweep(pointer(1));
    assert!(!sample(&binding, clock.now()));
    assert_eq!(
        member.effects.get(),
        1,
        "another captured slot is the same member-wide obligation"
    );
    let erased: Rc<dyn GestureArenaMember> = member.clone();
    let registration = binding
        .arena()
        .register_deadline_member(pointer(2), &erased)
        .expect("explicit new source");
    assert_eq!(
        binding
            .arena()
            .register_deadline_member(pointer(1), &erased)
            .err(),
        Some(GestureDeadlineError::AlreadyRegistered)
    );
    registration
        .update_handle()
        .publish(Some(at(&clock)))
        .expect("explicit new revision");
    assert!(sample(&binding, clock.now()));
    assert!(!sample(&binding, clock.now()));
    assert_eq!(member.effects.get(), 2);
}

fn reentrant_query_and_poll_publications_survive_the_old_failure() {
    for during_query in [true, false] {
        let clock = ManualClock::new();
        let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
        let member = DeadlineMember::new(at(&clock));
        let erased: Rc<dyn GestureArenaMember> = member.clone();
        let registration = binding
            .arena()
            .register_deadline_member(pointer(1), &erased)
            .expect("source");
        let update = registration.update_handle();
        let replacement_due = at(&clock) + Duration::from_secs(2);
        if during_query {
            member.fail_query.set(true);
            *member.query_publication.borrow_mut() = Some((update.clone(), Some(replacement_due)));
            update.refresh().expect("query revision");
        } else {
            member.fail_poll.set(true);
            *member.poll_publication.borrow_mut() = Some((update.clone(), Some(replacement_due)));
            update.publish(Some(at(&clock))).expect("due poll revision");
        }
        assert!(sample(&binding, clock.now()));
        assert_eq!(
            flui_interaction::__runtime::next_deadline_snapshot(&binding),
            Some(replacement_due),
            "old failure cannot quarantine a reentrant replacement"
        );
        assert!(!sample(&binding, clock.now()));
    }
}

fn a_successful_reprobe_and_explicit_refresh_restore_discovery() {
    for recover_on_reprobe in [true, false] {
        let clock = ManualClock::new();
        let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
        let due = at(&clock) + Duration::from_secs(2);
        let member = DeadlineMember::new(due);
        member.fail_query.set(true);
        let erased: Rc<dyn GestureArenaMember> = member.clone();
        let registration = binding
            .arena()
            .register_deadline_member(pointer(1), &erased)
            .expect("source");
        let update = registration.update_handle();
        assert!(sample(&binding, clock.now()));
        clock.advance(Duration::from_millis(100));
        member.fail_query.set(!recover_on_reprobe);
        assert!(!sample(&binding, clock.now()));
        if !recover_on_reprobe {
            assert_eq!(
                flui_interaction::__runtime::next_deadline_snapshot(&binding),
                None
            );
            member.fail_query.set(false);
            update
                .refresh()
                .expect("explicit source update after parked retry");
            assert!(!sample(&binding, clock.now()));
        }
        assert_eq!(
            flui_interaction::__runtime::next_deadline_snapshot(&binding),
            Some(due)
        );
    }
}

fn a_shared_traversal_clone_does_not_keep_the_member_alive_after_failure() {
    let clock = ManualClock::new();
    let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
    let member = DeadlineMember::new(at(&clock));
    member.fail_poll.set(true);
    let weak = Rc::downgrade(&member);
    let _entry = binding.arena().add(pointer(1), &member);
    assert!(sample(&binding, clock.now()));
    drop(member);
    assert!(
        weak.upgrade().is_none(),
        "the surviving external owner retains sole member custody"
    );
    assert_eq!(
        flui_interaction::__runtime::next_deadline_snapshot(&binding),
        None
    );
}

fn built_in_deadline_notifications_have_a_committed_delivery_postcondition() {
    use flui_interaction::events::{PointerKind, make_down_event_for_id, make_up_event_for_id};
    use flui_interaction::pointer::PointerDispatch;
    use flui_interaction::{
        DoubleTapGestureRecognizer, GestureRecognizer as _, LongPressGestureRecognizer,
        MultiTapGestureRecognizer,
    };
    for kind in ["long press", "double tap", "multi tap"] {
        let clock = ManualClock::new();
        let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
        let notices = Rc::new(Cell::new(0));
        let captured = Rc::clone(&notices);
        let down = make_down_event_for_id(pointer(1), Offset::ZERO, PointerKind::Touch)
            .expect("down fixture");
        let recognizer: Rc<dyn flui_interaction::GestureRecognizer> = match kind {
            "long press" => {
                let recognizer = LongPressGestureRecognizer::builder(binding.arena().clone())
                    .on_long_press(move || {
                        captured.set(captured.get() + 1);
                        panic!("committed long-press notice");
                    })
                    .build();
                recognizer.add_pointer(PointerDispatch::at_root(&down));
                binding.arena().close(pointer(1));
                binding.arena().drain_deferred_resolutions();
                recognizer
            }
            "double tap" => {
                let recognizer = DoubleTapGestureRecognizer::builder(binding.arena().clone())
                    .on_double_tap_cancel(move |_| {
                        captured.set(captured.get() + 1);
                        panic!("committed double-tap cancellation");
                    })
                    .build();
                recognizer.add_pointer(PointerDispatch::at_root(&down));
                let up = make_up_event_for_id(pointer(1), Offset::ZERO, PointerKind::Touch)
                    .expect("up fixture");
                recognizer.handle_event(PointerDispatch::at_root(&up));
                recognizer
            }
            _ => {
                let recognizer = MultiTapGestureRecognizer::builder(
                    binding.arena().clone(),
                    flui_interaction::MultiTapCount::TWO,
                )
                .on_multi_tap_cancel(move |_| {
                    captured.set(captured.get() + 1);
                    panic!("committed multi-tap cancellation");
                })
                .build();
                recognizer.add_pointer(PointerDispatch::at_root(&down));
                recognizer
            }
        };
        clock.advance(Duration::from_secs(1));
        let healthy = DeadlineMember::new(at(&clock));
        let _entry = binding.arena().add(pointer(2), &healthy);
        let mut recovery = PanicRecovery::new();
        flui_interaction::__runtime::tick_deadlines_with_recovery(
            &binding,
            clock.now(),
            &mut recovery.scope(),
        );
        assert_eq!(
            recovery.first_failure_class(),
            Some(FailureClass::Delivery),
            "{kind} commits before notification"
        );
        assert_eq!(
            recovery.blocking_failure_class(),
            None,
            "{kind} retains valid state"
        );
        assert_eq!(healthy.effects.get(), 1);
        assert_eq!(notices.get(), 1);
        assert!(
            !sample(&binding, clock.now()),
            "{kind} cannot repeat its committed notice"
        );
        assert_eq!(notices.get(), 1);
        drop(recognizer);
    }
}

fn retired_and_held_sources_cannot_resurrect_an_unknown_poll() {
    for explicit in [true, false] {
        let clock = ManualClock::new();
        let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
        let member = DeadlineMember::new(at(&clock));
        member.fail_poll.set(true);
        let entry = binding.arena().add(pointer(1), &member);
        let erased: Rc<dyn GestureArenaMember> = member.clone();
        let registration = explicit.then(|| {
            binding
                .arena()
                .register_deadline_member(pointer(1), &erased)
                .expect("explicit source")
        });
        assert!(sample(&binding, clock.now()));
        if explicit {
            drop(registration);
        } else {
            entry.hold();
            binding.arena().sweep(pointer(1));
        }
        assert!(!sample(&binding, clock.now()));
        assert_eq!(
            member.effects.get(),
            1,
            "retirement or held transfer cannot authorize replay"
        );
        let _fresh = binding.arena().add(pointer(2), &member);
        assert!(
            sample(&binding, clock.now()),
            "a genuine new admission can own new deadline work"
        );
        assert_eq!(member.effects.get(), 2);
    }
}

fn retry_failure_preserves_retirement_even_when_reentry_supersedes_observation() {
    let clock = ManualClock::new();
    let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
    let member = DeadlineMember::new(at(&clock));
    member.fail_query.set(true);
    let erased: Rc<dyn GestureArenaMember> = member.clone();
    let registration = binding
        .arena()
        .register_deadline_member(pointer(1), &erased)
        .expect("source");
    drop(erased);
    let update = registration.update_handle();
    assert!(sample(&binding, clock.now()));
    let drops = Rc::clone(&member.drops);
    *member.query_publication.borrow_mut() =
        Some((update.clone(), Some(at(&clock) + Duration::from_secs(2))));
    let owner = Rc::new(RefCell::new(Some(Rc::clone(&member))));
    *member.release_owner.borrow_mut() = Some(Rc::clone(&owner));
    drop(member);
    clock.advance(Duration::from_millis(100));
    assert!(
        !sample(&binding, clock.now()),
        "the retry fault is already reported"
    );
    assert!(owner.borrow().is_none());
    assert_eq!(
        drops.get(),
        0,
        "caught retry failure retains final opaque ownership"
    );
    assert_eq!(
        flui_interaction::__runtime::next_deadline_snapshot(&binding),
        None,
        "retained ownership is not scheduling authority"
    );
    assert_eq!(update.refresh(), Err(GestureDeadlineError::Retired));
}

fn disarm_wake_cannot_make_an_old_up_retire_a_replacement_contact() {
    use flui_interaction::events::{PointerKind, make_down_event_for_id, make_up_event_for_id};
    use flui_interaction::pointer::PointerDispatch;
    use flui_interaction::{GestureRecognizer as _, LongPressGestureRecognizer};
    let clock = ManualClock::new();
    let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
    let window: Arc<dyn flui_platform::PlatformWindow> = Arc::new(WakeWindow {
        drops: Arc::default(),
        panic_on_drop: false,
    });
    flui_interaction::__runtime::set_pointer_capture_wake(&binding, Arc::downgrade(&window));
    let notices = Rc::new(Cell::new(0));
    let captured = Rc::clone(&notices);
    let recognizer = LongPressGestureRecognizer::builder(binding.arena().clone())
        .on_long_press(move || captured.set(captured.get() + 1))
        .build();
    let down =
        make_down_event_for_id(pointer(1), Offset::ZERO, PointerKind::Touch).expect("old down");
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    let replacement = Rc::clone(&recognizer);
    WAKE_ACTION.with(|action| {
        *action.borrow_mut() = Some(Box::new(move || {
            replacement.cancel();
            let down = make_down_event_for_id(pointer(2), Offset::ZERO, PointerKind::Touch)
                .expect("replacement down");
            replacement.add_pointer(PointerDispatch::at_root(&down));
        }));
    });
    let up = make_up_event_for_id(pointer(1), Offset::ZERO, PointerKind::Touch).expect("old up");
    recognizer.handle_event(PointerDispatch::at_root(&up));
    clock.advance(Duration::from_secs(1));
    assert!(!sample(&binding, clock.now()));
    assert_eq!(
        notices.get(),
        1,
        "the replacement contact keeps its Possible phase and timer"
    );
}

fn a_wake_failure_retains_the_last_backend_owner_before_unwind() {
    const CHILD: &str = "FLUI_DEADLINE_WAKE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let clock = ManualClock::new();
        let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
        let drops = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let window: Arc<dyn flui_platform::PlatformWindow> = Arc::new(WakeWindow {
            drops: Arc::clone(&drops),
            panic_on_drop: true,
        });
        flui_interaction::__runtime::set_pointer_capture_wake(&binding, Arc::downgrade(&window));
        let owner = Arc::new(std::sync::Mutex::new(Some(window)));
        let released = Arc::clone(&owner);
        WAKE_ACTION.with(|action| {
            *action.borrow_mut() = Some(Box::new(move || {
                drop(released.lock().expect("backend owner fixture").take());
                panic!("wake delivery first failure");
            }));
        });
        let member = DeadlineMember::new(at(&clock));
        let erased: Rc<dyn GestureArenaMember> = member;
        let registration = binding
            .arena()
            .register_deadline_member(pointer(1), &erased)
            .expect("source");
        let caught = std::panic::catch_unwind(AssertUnwindSafe(|| {
            registration.update_handle().publish(Some(at(&clock)))
        }));
        let payload = caught.expect_err("wake delivery fails after deadline commit");
        assert_eq!(
            flui_foundation::panic::payload_text(payload.as_ref()),
            Some("wake delivery first failure")
        );
        assert_eq!(
            drops.load(std::sync::atomic::Ordering::Relaxed),
            0,
            "last backend ownership retained before propagation"
        );
        assert!(owner.lock().expect("fixture owner").is_none());
        assert_eq!(
            flui_interaction::__runtime::next_deadline_snapshot(&binding),
            Some(at(&clock)),
            "failed wake cannot erase accepted timing"
        );
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            "deadline_recovery::owner_deadline_recovery_matrix",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .output()
        .expect("isolated wake failure process");
    assert!(
        output.status.success(),
        "wake failure child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn an_earlier_producer_failure_does_not_leak_a_healthy_callback_owner() {
    use flui_interaction::events::{PointerKind, make_down_event_for_id};
    use flui_interaction::pointer::PointerDispatch;
    use flui_interaction::{GestureRecognizer as _, LongPressGestureRecognizer};
    let clock = ManualClock::new();
    let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
    let drops = Rc::new(Cell::new(0));
    let capture = CallbackCapture(Rc::clone(&drops));
    let recognizer = LongPressGestureRecognizer::builder(binding.arena().clone())
        .on_long_press(move || {
            let _ = capture.observe();
        })
        .build();
    let down =
        make_down_event_for_id(pointer(1), Offset::ZERO, PointerKind::Touch).expect("contact down");
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    clock.advance(Duration::from_secs(1));
    let mut recovery = PanicRecovery::new();
    recovery.run(|| panic!("earlier independent producer failure"));
    flui_interaction::__runtime::tick_deadlines_with_recovery(
        &binding,
        clock.now(),
        &mut recovery.scope(),
    );
    assert_eq!(drops.get(), 0, "the live recognizer owns callback captures");
    drop(recognizer);
    assert_eq!(
        drops.get(),
        1,
        "healthy shared snapshot releases its capture ownership"
    );
}

fn a_member_withdrawal_removes_its_snapshot_before_rejection_delivery() {
    let clock = ManualClock::new();
    let binding = Rc::new(GestureBinding::with_clock(Arc::new(clock.clone())));
    let member = DeadlineMember::new(at(&clock));
    let healthy_due = at(&clock) + Duration::from_secs(5);
    let healthy = DeadlineMember::new(healthy_due);
    let entry = binding.arena().add(pointer(1), &member);
    let _healthy_entry = binding.arena().add(pointer(1), &healthy);
    *member.rejection_binding.borrow_mut() = Some(Rc::clone(&binding));
    let mut recovery = PanicRecovery::new();
    flui_interaction::__runtime::refresh_deadlines_with_recovery(
        &binding,
        clock.now(),
        &mut recovery.scope(),
    );
    assert_eq!(
        flui_interaction::__runtime::next_deadline_snapshot(&binding),
        Some(at(&clock))
    );
    entry.resolve(flui_interaction::GestureDisposition::Rejected);
    assert_eq!(
        member.rejection_deadline.get(),
        RejectionDeadlineObservation::Armed(healthy_due),
        "rejection observes only the still-admitted member's timing"
    );
}

fn abandoned_generations_are_withdrawn_before_rejection(all_pointers: bool) {
    let clock = ManualClock::new();
    let binding = Rc::new(GestureBinding::with_clock(Arc::new(clock.clone())));
    let member = DeadlineMember::new(at(&clock));
    let held = binding.arena().add(pointer(1), &member);
    held.hold();
    binding.arena().sweep(pointer(1));
    let _fresh = binding.arena().add(pointer(1), &member);
    let _other = all_pointers.then(|| binding.arena().add(pointer(2), &member));
    *member.rejection_binding.borrow_mut() = Some(Rc::clone(&binding));
    let mut recovery = PanicRecovery::new();
    flui_interaction::__runtime::refresh_deadlines_with_recovery(
        &binding,
        clock.now(),
        &mut recovery.scope(),
    );
    assert_eq!(
        flui_interaction::__runtime::next_deadline_snapshot(&binding),
        Some(at(&clock))
    );
    if all_pointers {
        binding.cancel_all_pointer_sequences();
    } else {
        binding.arena().abandon(pointer(1));
    }
    assert_eq!(
        member.rejection_deadline.get(),
        RejectionDeadlineObservation::Disarmed,
        "all detached generations lose timing before the first rejection"
    );
}

fn an_abandoned_batch_withdraws_every_generation_before_first_rejection() {
    abandoned_generations_are_withdrawn_before_rejection(false);
}

fn cancel_all_withdraws_every_batch_before_first_rejection() {
    abandoned_generations_are_withdrawn_before_rejection(true);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VerdictFault {
    None,
    Retirement,
    NotificationAndRetirement,
}

fn builtin_deadline_verdict_retirement(kind: &str, incoming_failure: bool, fault: VerdictFault) {
    use flui_interaction::events::{PointerKind, make_down_event_for_id, make_up_event_for_id};
    use flui_interaction::pointer::PointerDispatch;
    use flui_interaction::{
        DoubleTapGestureRecognizer, LongPressGestureRecognizer, MultiTapGestureRecognizer,
    };
    let clock = ManualClock::new();
    let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
    let down = make_down_event_for_id(pointer(1), Offset::ZERO, PointerKind::Touch)
        .expect("verdict down fixture");
    let notices = Rc::new(Cell::new(0));
    let captured = Rc::clone(&notices);
    let recognizer: Rc<dyn flui_interaction::GestureRecognizer> = match kind {
        "long press" => LongPressGestureRecognizer::builder(binding.arena().clone())
            .on_long_press(move || captured.set(captured.get() + 1))
            .build(),
        "double tap" => DoubleTapGestureRecognizer::builder(binding.arena().clone())
            .on_double_tap_cancel(move |_| captured.set(captured.get() + 1))
            .build(),
        _ => MultiTapGestureRecognizer::builder(
            binding.arena().clone(),
            flui_interaction::MultiTapCount::TWO,
        )
        .on_multi_tap_cancel(move |_| captured.set(captured.get() + 1))
        .build(),
    };
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    let owner = Rc::new(RefCell::new(None));
    let drops = Rc::new(Cell::new(0));
    let notifications = Rc::new(Cell::new(0));
    let member = Rc::new(VerdictMember {
        owner: Rc::downgrade(&owner),
        notifications: Rc::clone(&notifications),
        drops: Rc::clone(&drops),
        panic_on_drop: fault != VerdictFault::None,
        panic_on_notify: fault == VerdictFault::NotificationAndRetirement,
        reenter: RefCell::new(None),
        _drop_capture: std::env::var_os("FLUI_DEADLINE_VERDICT_CHILD").map(|_| VerdictDropFailure),
    });
    let _entry = binding.arena().add(pointer(1), &member);
    *owner.borrow_mut() = Some(member);
    binding.arena().close(pointer(1));
    if kind == "double tap" {
        let up = make_up_event_for_id(pointer(1), Offset::ZERO, PointerKind::Touch)
            .expect("verdict up fixture");
        recognizer.handle_event(PointerDispatch::at_root(&up));
    }
    if kind != "long press" {
        binding.arena().sweep(pointer(1));
    }
    clock.advance(Duration::from_secs(1));
    let mut recovery = PanicRecovery::new();
    if incoming_failure {
        recovery
            .scope()
            .run_classified(FailureClass::Computation, |_| {
                panic!("earlier animation computation");
            });
    }
    flui_interaction::__runtime::tick_deadlines_with_recovery(
        &binding,
        clock.now(),
        &mut recovery.scope(),
    );
    assert_eq!(
        notifications.get(),
        1,
        "{kind} delivers the committed verdict"
    );
    assert!(
        owner.borrow().is_none(),
        "{kind} callback releases external ownership"
    );
    assert_eq!(
        notices.get(),
        1,
        "{kind} completes its healthy callback tail"
    );
    if incoming_failure {
        assert_eq!(
            drops.get(),
            0,
            "{kind} inherits first-failure retirement custody"
        );
        assert_eq!(
            recovery.first_failure_class(),
            Some(FailureClass::Computation)
        );
        let failure = recovery.into_failure().expect("original failure");
        assert_eq!(
            flui_foundation::panic::payload_text(failure.payload()),
            Some("earlier animation computation")
        );
    } else if fault == VerdictFault::NotificationAndRetirement {
        assert_eq!(
            drops.get(),
            0,
            "{kind} callback failure protects final ownership"
        );
        assert_eq!(recovery.first_failure_class(), Some(FailureClass::Delivery));
        assert_eq!(recovery.blocking_failure_class(), None);
        let failure = recovery.into_failure().expect("verdict callback failure");
        assert_eq!(
            flui_foundation::panic::payload_text(failure.payload()),
            Some("arena verdict callback failure")
        );
    } else if fault == VerdictFault::Retirement {
        assert_eq!(drops.get(), 1, "{kind} ordinary retirement runs");
        assert_eq!(recovery.first_failure_class(), Some(FailureClass::Unknown));
        assert_eq!(
            recovery.blocking_failure_class(),
            Some(FailureClass::Unknown)
        );
    } else {
        assert_eq!(drops.get(), 1, "{kind} keeps healthy-path destruction");
        assert!(!recovery.has_failure());
    }
}

fn long_press_verdict_inherits_incoming_retirement_custody() {
    builtin_deadline_verdict_retirement("long press", true, VerdictFault::None);
}

fn double_tap_verdict_inherits_incoming_retirement_custody() {
    builtin_deadline_verdict_retirement("double tap", true, VerdictFault::None);
}

fn multi_tap_verdict_inherits_incoming_retirement_custody() {
    builtin_deadline_verdict_retirement("multi tap", true, VerdictFault::None);
}

fn long_press_opaque_retirement_is_unknown() {
    builtin_deadline_verdict_retirement("long press", false, VerdictFault::Retirement);
}

fn double_tap_opaque_retirement_is_unknown() {
    builtin_deadline_verdict_retirement("double tap", false, VerdictFault::Retirement);
}

fn multi_tap_opaque_retirement_is_unknown() {
    builtin_deadline_verdict_retirement("multi tap", false, VerdictFault::Retirement);
}

fn builtin_verdict_healthy_destruction_remains() {
    for kind in ["long press", "double tap", "multi tap"] {
        builtin_deadline_verdict_retirement(kind, false, VerdictFault::None);
    }
}

fn builtin_verdict_callback_failure_protects_competing_retirement() {
    for kind in ["long press", "double tap", "multi tap"] {
        builtin_deadline_verdict_retirement(kind, false, VerdictFault::NotificationAndRetirement);
    }
}

fn builtin_incoming_verdict_retains_double_panicking_aggregate() {
    const CHILD: &str = "FLUI_DEADLINE_VERDICT_CHILD";
    if std::env::var_os(CHILD).is_some() {
        builtin_deadline_verdict_retirement("long press", true, VerdictFault::Retirement);
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            "deadline_recovery::owner_deadline_recovery_matrix",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .output()
        .expect("isolated verdict retirement process");
    assert!(
        output.status.success(),
        "incoming verdict retirement child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NestedVerdictFault {
    None,
    Retirement,
    RetirementThenCallback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NestedVerdictAction {
    Abandon,
    Resolve,
    HeldRelease,
}

fn nested_public_verdict_operation(
    action: NestedVerdictAction,
    incoming: bool,
    fault: NestedVerdictFault,
) {
    use flui_interaction::events::{PointerKind, make_down_event_for_id};
    use flui_interaction::pointer::PointerDispatch;
    use flui_interaction::{GestureRecognizer as _, LongPressGestureRecognizer};
    let clock = ManualClock::new();
    let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
    let notices = Rc::new(Cell::new(0));
    let captured = Rc::clone(&notices);
    let recognizer = LongPressGestureRecognizer::builder(binding.arena().clone())
        .on_long_press(move || captured.set(captured.get() + 1))
        .build();
    let down = make_down_event_for_id(pointer(1), Offset::ZERO, PointerKind::Touch)
        .expect("nested verdict down");
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    let child_owner = Rc::new(RefCell::new(None));
    let child_drops = Rc::new(Cell::new(0));
    let child_notices = Rc::new(Cell::new(0));
    let child = Rc::new(VerdictMember {
        owner: Rc::downgrade(&child_owner),
        notifications: Rc::clone(&child_notices),
        drops: Rc::clone(&child_drops),
        panic_on_drop: fault != NestedVerdictFault::None,
        panic_on_notify: false,
        reenter: RefCell::new(None),
        _drop_capture: None,
    });
    let child_entry = binding.arena().add(pointer(2), &child);
    *child_owner.borrow_mut() = Some(child);
    if action == NestedVerdictAction::Resolve {
        binding.arena().close(pointer(2));
    } else if action == NestedVerdictAction::HeldRelease {
        child_entry.hold();
        child_entry.sweep();
    }
    let outer_owner = Rc::new(RefCell::new(None));
    let outer_drops = Rc::new(Cell::new(0));
    let arena = binding.arena().clone();
    let outer = Rc::new(VerdictMember {
        owner: Rc::downgrade(&outer_owner),
        notifications: Rc::new(Cell::new(0)),
        drops: Rc::clone(&outer_drops),
        panic_on_drop: false,
        panic_on_notify: fault == NestedVerdictFault::RetirementThenCallback,
        reenter: RefCell::new(Some(Box::new(move || match action {
            NestedVerdictAction::Abandon => arena.abandon(pointer(2)),
            NestedVerdictAction::Resolve => {
                child_entry.resolve(flui_interaction::GestureDisposition::Accepted);
            }
            NestedVerdictAction::HeldRelease => child_entry.release(),
        }))),
        _drop_capture: None,
    });
    let _outer_entry = binding.arena().add(pointer(1), &outer);
    *outer_owner.borrow_mut() = Some(outer);
    binding.arena().close(pointer(1));
    clock.advance(Duration::from_secs(1));
    let mut recovery = PanicRecovery::new();
    if incoming {
        recovery
            .scope()
            .run_classified(FailureClass::Computation, |_| {
                panic!("earlier nested animation computation");
            });
    }
    flui_interaction::__runtime::tick_deadlines_with_recovery(
        &binding,
        clock.now(),
        &mut recovery.scope(),
    );
    let child_released = child_owner.borrow().is_none();
    if !child_released {
        // A failed fixture assertion must not run the leftover opaque destructor.
        std::mem::forget(child_owner.borrow_mut().take());
    }
    assert_eq!(child_notices.get(), 1);
    assert!(child_released);
    assert_eq!(notices.get(), 1, "healthy deadline tail still progresses");
    if incoming {
        assert_eq!(
            child_drops.get(),
            0,
            "nested public {action:?} inherits custody"
        );
        assert_eq!(outer_drops.get(), 0);
        assert_eq!(
            recovery.first_failure_class(),
            Some(FailureClass::Computation)
        );
        assert_eq!(
            recovery.blocking_failure_class(),
            Some(FailureClass::Computation)
        );
        let failure = recovery.into_failure().expect("original incoming failure");
        assert_eq!(
            flui_foundation::panic::payload_text(failure.payload()),
            Some("earlier nested animation computation")
        );
    } else if fault != NestedVerdictFault::None {
        assert_eq!(child_drops.get(), 1);
        assert_eq!(
            outer_drops.get(),
            0,
            "nested {action:?} failure protects the outer notification snapshot"
        );
        assert_eq!(recovery.first_failure_class(), Some(FailureClass::Unknown));
        assert_eq!(
            recovery.blocking_failure_class(),
            Some(FailureClass::Unknown)
        );
        let failure = recovery.into_failure().expect("nested retirement failure");
        assert_eq!(
            flui_foundation::panic::payload_text(failure.payload()),
            Some("opaque arena member retirement")
        );
    } else {
        assert_eq!(child_drops.get(), 1);
        assert_eq!(outer_drops.get(), 1);
        assert!(!recovery.has_failure());
    }
    let owner = Rc::new(RefCell::new(None));
    let drops = Rc::new(Cell::new(0));
    let fresh_notices = Rc::new(Cell::new(0));
    let fresh = Rc::new(VerdictMember {
        owner: Rc::downgrade(&owner),
        notifications: Rc::clone(&fresh_notices),
        drops: Rc::clone(&drops),
        panic_on_drop: false,
        panic_on_notify: false,
        reenter: RefCell::new(None),
        _drop_capture: None,
    });
    let fresh_entry = binding.arena().add(pointer(3), &fresh);
    *owner.borrow_mut() = Some(fresh);
    if action == NestedVerdictAction::Resolve {
        binding.arena().close(pointer(3));
    } else if action == NestedVerdictAction::HeldRelease {
        fresh_entry.hold();
        fresh_entry.sweep();
    }
    match action {
        NestedVerdictAction::Abandon => binding.arena().abandon(pointer(3)),
        NestedVerdictAction::Resolve => {
            fresh_entry.resolve(flui_interaction::GestureDisposition::Accepted);
        }
        NestedVerdictAction::HeldRelease => fresh_entry.release(),
    }
    assert_eq!(
        fresh_notices.get(),
        1,
        "the next {action:?} delivers its verdict"
    );
    assert!(owner.borrow().is_none());
    assert_eq!(
        drops.get(),
        1,
        "custody belongs to one operation, not the arena lifetime"
    );
}

fn nested_public_abandon_inherits_custody_then_retires_normally_next_operation() {
    nested_public_verdict_operation(NestedVerdictAction::Abandon, true, NestedVerdictFault::None);
}

fn nested_public_retirement_keeps_unknown_provenance() {
    nested_public_verdict_operation(
        NestedVerdictAction::Abandon,
        false,
        NestedVerdictFault::Retirement,
    );
}

fn nested_public_retirement_precedes_later_callback_failure() {
    nested_public_verdict_operation(
        NestedVerdictAction::Abandon,
        false,
        NestedVerdictFault::RetirementThenCallback,
    );
}

fn nested_public_verdict_healthy_retirement_remains() {
    for action in [
        NestedVerdictAction::Abandon,
        NestedVerdictAction::Resolve,
        NestedVerdictAction::HeldRelease,
    ] {
        nested_public_verdict_operation(action, false, NestedVerdictFault::None);
    }
}

fn nested_public_resolve_inherits_custody_then_retires_normally_next_operation() {
    for fault in [
        NestedVerdictFault::None,
        NestedVerdictFault::RetirementThenCallback,
    ] {
        nested_public_verdict_operation(NestedVerdictAction::Resolve, true, fault);
    }
}

fn nested_public_resolve_retirement_keeps_unknown_provenance() {
    nested_public_verdict_operation(
        NestedVerdictAction::Resolve,
        false,
        NestedVerdictFault::Retirement,
    );
}

fn nested_public_resolve_retirement_precedes_later_callback_failure() {
    nested_public_verdict_operation(
        NestedVerdictAction::Resolve,
        false,
        NestedVerdictFault::RetirementThenCallback,
    );
}

fn nested_public_held_release_inherits_custody_then_retires_normally_next_operation() {
    for fault in [
        NestedVerdictFault::None,
        NestedVerdictFault::RetirementThenCallback,
    ] {
        nested_public_verdict_operation(NestedVerdictAction::HeldRelease, true, fault);
    }
}

fn nested_public_held_release_retirement_keeps_unknown_provenance() {
    nested_public_verdict_operation(
        NestedVerdictAction::HeldRelease,
        false,
        NestedVerdictFault::Retirement,
    );
}

fn nested_public_held_release_retirement_precedes_later_callback_failure() {
    nested_public_verdict_operation(
        NestedVerdictAction::HeldRelease,
        false,
        NestedVerdictFault::RetirementThenCallback,
    );
}

fn open_deadline_poll_with_nested_retirement(panic_after_retirement: bool) {
    struct NestedPollMember {
        arena: flui_interaction::GestureArena,
        due: EventTime,
        polls: Rc<Cell<usize>>,
        panic_after_retirement: bool,
    }

    impl GestureArenaMember for NestedPollMember {
        fn accept_gesture(&self, _: GestureContest) {}
        fn reject_gesture(&self, _: GestureContest) {}
        fn deadline(&self) -> Option<EventTime> {
            Some(self.due)
        }
        fn poll_deadline(&self, _: EventTime) {
            self.polls.set(self.polls.get() + 1);
            self.arena.abandon(pointer(2));
            assert!(
                !self.panic_after_retirement,
                "later open deadline poll failure"
            );
        }
    }

    let clock = ManualClock::new();
    let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
    let polls = Rc::new(Cell::new(0));
    let member = Rc::new(NestedPollMember {
        arena: binding.arena().clone(),
        due: at(&clock),
        polls: Rc::clone(&polls),
        panic_after_retirement,
    });
    let _deadline_entry = binding.arena().add(pointer(1), &member);
    let child_owner = Rc::new(RefCell::new(None));
    let child_drops = Rc::new(Cell::new(0));
    let child_notices = Rc::new(Cell::new(0));
    let child = Rc::new(VerdictMember {
        owner: Rc::downgrade(&child_owner),
        notifications: Rc::clone(&child_notices),
        drops: Rc::clone(&child_drops),
        panic_on_drop: true,
        panic_on_notify: false,
        reenter: RefCell::new(None),
        _drop_capture: None,
    });
    let _child_entry = binding.arena().add(pointer(2), &child);
    *child_owner.borrow_mut() = Some(child);
    let mut recovery = PanicRecovery::new();
    flui_interaction::__runtime::tick_deadlines_with_recovery(
        &binding,
        clock.now(),
        &mut recovery.scope(),
    );
    let child_released = child_owner.borrow().is_none();
    if !child_released {
        // Keep the original failed observation while protecting assertion unwind.
        std::mem::forget(child_owner.borrow_mut().take());
    }
    assert_eq!(child_notices.get(), 1);
    assert_eq!(child_drops.get(), 1);
    assert!(child_released);
    assert_eq!(recovery.first_failure_class(), Some(FailureClass::Unknown));
    assert_eq!(
        recovery.blocking_failure_class(),
        Some(FailureClass::Unknown)
    );
    let failure = recovery.into_failure().expect("nested retirement failure");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.payload()),
        Some("opaque arena member retirement"),
        "the nested retirement remains the first failure of the open poll"
    );
    assert_eq!(polls.get(), 1);
    assert!(!sample(&binding, clock.now()));
    if panic_after_retirement {
        assert_eq!(polls.get(), 1, "the unknown poll revision is quarantined");
    } else {
        assert_eq!(
            polls.get(),
            2,
            "the normally completed outer poll progresses"
        );
        assert_eq!(
            child_notices.get(),
            1,
            "the retired child is not notified again"
        );
        assert_eq!(
            child_drops.get(),
            1,
            "the failed nested retirement cannot replay"
        );
    }

    let owner = Rc::new(RefCell::new(None));
    let drops = Rc::new(Cell::new(0));
    let notices = Rc::new(Cell::new(0));
    let fresh = Rc::new(VerdictMember {
        owner: Rc::downgrade(&owner),
        notifications: Rc::clone(&notices),
        drops: Rc::clone(&drops),
        panic_on_drop: false,
        panic_on_notify: false,
        reenter: RefCell::new(None),
        _drop_capture: None,
    });
    let _fresh_entry = binding.arena().add(pointer(3), &fresh);
    *owner.borrow_mut() = Some(fresh);
    binding.arena().abandon(pointer(3));
    assert_eq!(
        notices.get(),
        1,
        "the next independent verdict is delivered"
    );
    assert!(owner.borrow().is_none());
    assert_eq!(
        drops.get(),
        1,
        "the next operation keeps ordinary retirement"
    );
}

fn an_open_deadline_poll_preserves_nested_retirement_before_its_later_panic() {
    open_deadline_poll_with_nested_retirement(true);
}

fn a_returning_open_deadline_poll_contains_its_distinct_nested_failure() {
    open_deadline_poll_with_nested_retirement(false);
}

#[test]
fn owner_deadline_recovery_matrix() {
    if std::env::var_os("FLUI_DEADLINE_WAKE_CHILD").is_some() {
        a_wake_failure_retains_the_last_backend_owner_before_unwind();
        return;
    }
    if std::env::var_os("FLUI_DEADLINE_VERDICT_CHILD").is_some() {
        builtin_incoming_verdict_retains_double_panicking_aggregate();
        return;
    }
    let mut first_failure = None;
    for (name, case) in [
        (
            "query_failure_has_one_bounded_probe_and_inert_reads",
            query_failure_has_one_bounded_probe_and_inert_reads as fn(),
        ),
        (
            "unknown_poll_is_not_replayed_and_healthy_tail_progresses",
            unknown_poll_is_not_replayed_and_healthy_tail_progresses as fn(),
        ),
        (
            "a_reused_pointer_admits_a_new_deadline_identity",
            a_reused_pointer_admits_a_new_deadline_identity as fn(),
        ),
        (
            "explicit_same_time_rearm_is_independent_and_stale_authority_refuses",
            explicit_same_time_rearm_is_independent_and_stale_authority_refuses as fn(),
        ),
        (
            "a_member_wide_poll_cannot_escape_through_another_captured_slot",
            a_member_wide_poll_cannot_escape_through_another_captured_slot as fn(),
        ),
        (
            "reentrant_query_and_poll_publications_survive_the_old_failure",
            reentrant_query_and_poll_publications_survive_the_old_failure as fn(),
        ),
        (
            "a_successful_reprobe_and_explicit_refresh_restore_discovery",
            a_successful_reprobe_and_explicit_refresh_restore_discovery as fn(),
        ),
        (
            "a_shared_traversal_clone_does_not_keep_the_member_alive_after_failure",
            a_shared_traversal_clone_does_not_keep_the_member_alive_after_failure as fn(),
        ),
        (
            "built_in_deadline_notifications_have_a_committed_delivery_postcondition",
            built_in_deadline_notifications_have_a_committed_delivery_postcondition as fn(),
        ),
        (
            "retired_and_held_sources_cannot_resurrect_an_unknown_poll",
            retired_and_held_sources_cannot_resurrect_an_unknown_poll as fn(),
        ),
        (
            "retry_failure_preserves_retirement_even_when_reentry_supersedes_observation",
            retry_failure_preserves_retirement_even_when_reentry_supersedes_observation as fn(),
        ),
        (
            "disarm_wake_cannot_make_an_old_up_retire_a_replacement_contact",
            disarm_wake_cannot_make_an_old_up_retire_a_replacement_contact as fn(),
        ),
        (
            "a_wake_failure_retains_the_last_backend_owner_before_unwind",
            a_wake_failure_retains_the_last_backend_owner_before_unwind as fn(),
        ),
        (
            "an_earlier_producer_failure_does_not_leak_a_healthy_callback_owner",
            an_earlier_producer_failure_does_not_leak_a_healthy_callback_owner as fn(),
        ),
        (
            "a_member_withdrawal_removes_its_snapshot_before_rejection_delivery",
            a_member_withdrawal_removes_its_snapshot_before_rejection_delivery as fn(),
        ),
        (
            "an_abandoned_batch_withdraws_every_generation_before_first_rejection",
            an_abandoned_batch_withdraws_every_generation_before_first_rejection as fn(),
        ),
        (
            "cancel_all_withdraws_every_batch_before_first_rejection",
            cancel_all_withdraws_every_batch_before_first_rejection as fn(),
        ),
        (
            "long_press_verdict_inherits_incoming_retirement_custody",
            long_press_verdict_inherits_incoming_retirement_custody as fn(),
        ),
        (
            "double_tap_verdict_inherits_incoming_retirement_custody",
            double_tap_verdict_inherits_incoming_retirement_custody as fn(),
        ),
        (
            "multi_tap_verdict_inherits_incoming_retirement_custody",
            multi_tap_verdict_inherits_incoming_retirement_custody as fn(),
        ),
        (
            "long_press_opaque_retirement_is_unknown",
            long_press_opaque_retirement_is_unknown as fn(),
        ),
        (
            "double_tap_opaque_retirement_is_unknown",
            double_tap_opaque_retirement_is_unknown as fn(),
        ),
        (
            "multi_tap_opaque_retirement_is_unknown",
            multi_tap_opaque_retirement_is_unknown as fn(),
        ),
        (
            "builtin_verdict_healthy_destruction_remains",
            builtin_verdict_healthy_destruction_remains as fn(),
        ),
        (
            "builtin_verdict_callback_failure_protects_competing_retirement",
            builtin_verdict_callback_failure_protects_competing_retirement as fn(),
        ),
        (
            "builtin_incoming_verdict_retains_double_panicking_aggregate",
            builtin_incoming_verdict_retains_double_panicking_aggregate as fn(),
        ),
        (
            "nested_public_abandon_inherits_custody_then_retires_normally_next_operation",
            nested_public_abandon_inherits_custody_then_retires_normally_next_operation as fn(),
        ),
        (
            "nested_public_retirement_keeps_unknown_provenance",
            nested_public_retirement_keeps_unknown_provenance as fn(),
        ),
        (
            "nested_public_retirement_precedes_later_callback_failure",
            nested_public_retirement_precedes_later_callback_failure as fn(),
        ),
        (
            "nested_public_verdict_healthy_retirement_remains",
            nested_public_verdict_healthy_retirement_remains as fn(),
        ),
        (
            "nested_public_resolve_inherits_custody_then_retires_normally_next_operation",
            nested_public_resolve_inherits_custody_then_retires_normally_next_operation as fn(),
        ),
        (
            "nested_public_resolve_retirement_keeps_unknown_provenance",
            nested_public_resolve_retirement_keeps_unknown_provenance as fn(),
        ),
        (
            "nested_public_resolve_retirement_precedes_later_callback_failure",
            nested_public_resolve_retirement_precedes_later_callback_failure as fn(),
        ),
        (
            "nested_public_held_release_inherits_custody_then_retires_normally_next_operation",
            nested_public_held_release_inherits_custody_then_retires_normally_next_operation
                as fn(),
        ),
        (
            "nested_public_held_release_retirement_keeps_unknown_provenance",
            nested_public_held_release_retirement_keeps_unknown_provenance as fn(),
        ),
        (
            "nested_public_held_release_retirement_precedes_later_callback_failure",
            nested_public_held_release_retirement_precedes_later_callback_failure as fn(),
        ),
        (
            "an_open_deadline_poll_preserves_nested_retirement_before_its_later_panic",
            an_open_deadline_poll_preserves_nested_retirement_before_its_later_panic as fn(),
        ),
        (
            "a_returning_open_deadline_poll_contains_its_distinct_nested_failure",
            a_returning_open_deadline_poll_contains_its_distinct_nested_failure as fn(),
        ),
    ] {
        if let Err(payload) = std::panic::catch_unwind(case) {
            eprintln!("deadline recovery case `{name}` failed");
            if first_failure.is_none() {
                first_failure = Some(payload);
            } else {
                flui_foundation::panic::retain_opaque_payload(payload);
            }
        }
    }
    if let Some(payload) = first_failure {
        std::panic::resume_unwind(payload);
    }
}

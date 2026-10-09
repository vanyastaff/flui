//! Public extension contracts, mounted with the atomic recognizer migration.

use std::{
    cell::{Cell, RefCell},
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    rc::{Rc, Weak},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use flui_foundation::MonotonicClock;
use flui_foundation::geometry::Offset;
use flui_interaction::{
    ArenaMembership, BeginContactError, CancelOutcome, DoubleTapGestureRecognizer, GestureArena,
    GestureArenaMember, GestureRecognizer, GestureSettings, MultiTapGestureRecognizer, PointerId,
    PrimaryContact, RecognizerSet, ScaleGestureRecognizer,
    arena::run_pointer_lifecycle,
    cancel_all,
    events::{
        PointerButton, PointerEvent, PointerKind, make_down_event_for_id,
        make_down_event_for_id_with_button, make_move_event_for_id, make_up_event_for_id,
    },
    routing::PointerDispatch,
};

type Log = Rc<RefCell<Vec<&'static str>>>;

thread_local! {
    // A Send clock can reenter owner-local state on its calling thread.
    static CLOCK_CONTACT: RefCell<Option<Weak<Extension>>> = const { RefCell::new(None) };
    static CLOCK_SCALE: RefCell<Option<Weak<ScaleGestureRecognizer>>> = const { RefCell::new(None) };
}

#[derive(Debug)]
struct ReentrantClock {
    now: web_time::Instant,
    replace_contact: AtomicBool,
}

impl MonotonicClock for ReentrantClock {
    fn now(&self) -> web_time::Instant {
        if self.replace_contact.swap(false, Ordering::Relaxed) {
            let owner = CLOCK_CONTACT.with(|slot| slot.borrow().as_ref().and_then(Weak::upgrade));
            if let Some(owner) = owner {
                let old = owner
                    .contact
                    .current()
                    .expect("clock sees committed contact");
                owner.contact.withdraw();
                let event = down(old.pointer);
                owner
                    .contact
                    .begin(PointerDispatch::at_root(&event), &old.settings)
                    .expect("clock reentrant admission");
            }
        }
        self.now
    }
}

struct Extension {
    contact: PrimaryContact,
    log: Log,
    name: &'static str,
    fail_delivery: Cell<bool>,
    fail_cancel: Cell<bool>,
}

impl Extension {
    fn new(arena: GestureArena, name: &'static str, log: Log) -> Rc<Self> {
        Rc::new_cyclic(|this: &Weak<Self>| {
            let member: Weak<dyn GestureArenaMember> = this.clone();
            Self {
                contact: PrimaryContact::new(ArenaMembership::new(arena, member)),
                log,
                name,
                fail_delivery: Cell::new(false),
                fail_cancel: Cell::new(false),
            }
        })
    }
}

impl GestureArenaMember for Extension {
    fn accept_gesture(&self, _pointer: PointerId) {
        self.log.borrow_mut().push("accepted");
    }

    fn reject_gesture(&self, _pointer: PointerId) {
        self.log.borrow_mut().push("rejected");
    }

    fn deadline(&self) -> Option<web_time::Instant> {
        self.contact.deadline()
    }
}

impl GestureRecognizer for Extension {
    fn add_pointer(&self, down: PointerDispatch<'_>) {
        self.log.borrow_mut().push(self.name);
        let _ = self.contact.begin(down, &GestureSettings::default());
        assert!(!self.fail_delivery.get(), "{}", self.name);
    }

    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        self.log.borrow_mut().push(self.name);
        if matches!(
            dispatch.local,
            PointerEvent::Up(_) | PointerEvent::Cancel(_)
        ) {
            self.contact.withdraw();
        }
        assert!(!self.fail_delivery.get(), "{}", self.name);
    }

    fn cancel(&self) -> CancelOutcome {
        self.log.borrow_mut().push(self.name);
        let outcome = if self.contact.withdraw().is_some() {
            CancelOutcome::Cancelled
        } else {
            CancelOutcome::Idle
        };
        assert!(!self.fail_cancel.get(), "{}", self.name);
        outcome
    }
}

fn pointer(raw: u64) -> PointerId {
    PointerId::new(std::num::NonZeroU64::new(raw).expect("nonzero fixture pointer"))
}

fn down(pointer: PointerId) -> PointerEvent {
    make_down_event_for_id_with_button(
        pointer,
        Offset::new(3.0, 4.0),
        PointerKind::Touch,
        PointerButton::PRIMARY,
    )
    .expect("valid fixture sample")
}

fn second_pointer_does_not_replace_the_primary_contact() {
    let owner = Extension::new(GestureArena::new(), "owner", Log::default());
    let first = down(pointer(71));
    let second = down(pointer(72));
    let settings = GestureSettings::default();
    let id = owner
        .contact
        .begin(PointerDispatch::at_root(&first), &settings)
        .expect("first contact");
    assert!(matches!(
        owner.contact.begin(PointerDispatch::at_root(&second), &settings),
        Err(BeginContactError::Busy { current }) if current == pointer(71)
    ));
    assert!(owner.contact.is_current(id));
    assert!(!owner.contact.tracks(pointer(72)));
    let snapshot = owner.contact.current().expect("first contact remains");
    assert_eq!(snapshot.pointer, pointer(71));
    assert_eq!(snapshot.kind, PointerKind::Touch);
    assert_eq!(snapshot.local, Offset::new(3.0, 4.0));
    assert_eq!(snapshot.settings.touch_slop(), settings.touch_slop());
    owner.contact.withdraw();
    let next = owner
        .contact
        .begin(PointerDispatch::at_root(&first), &settings)
        .expect("same pointer reused");
    assert_ne!(id, next);
    assert!(!owner.contact.is_current(id));
}

fn cancellation_finishes_every_owner_before_first_panic_resumes() {
    for failures in [[true, false], [true, true], [false, true]] {
        let log = Log::default();
        let arena = GestureArena::new();
        let first = Extension::new(arena.clone(), "first", log.clone());
        let second = Extension::new(arena, "second", log.clone());
        let event = down(pointer(73));
        first.add_pointer(PointerDispatch::at_root(&event));
        second.add_pointer(PointerDispatch::at_root(&event));
        first.fail_cancel.set(failures[0]);
        second.fail_cancel.set(failures[1]);
        log.borrow_mut().clear();
        let failure = catch_unwind(AssertUnwindSafe(|| {
            cancel_all([first.as_ref() as &dyn GestureRecognizer, second.as_ref()])
        }))
        .expect_err("configured cancellation failure");
        let expected = if failures[0] { "first" } else { "second" };
        assert_eq!(
            failure.downcast_ref::<String>().map(String::as_str),
            Some(expected)
        );
        assert!(first.contact.current().is_none());
        assert!(second.contact.current().is_none());
        assert_eq!(
            log.borrow()
                .iter()
                .filter(|name| **name == "first" || **name == "second")
                .copied()
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
        first.fail_cancel.set(false);
        second.fail_cancel.set(false);
        assert!(matches!(
            cancel_all([first.as_ref() as &dyn GestureRecognizer, second.as_ref()]),
            CancelOutcome::Idle
        ));
    }
}

fn recognizer_set_preserves_first_failure_and_recovers() {
    let log = Log::default();
    let arena = GestureArena::new();
    let first = Extension::new(arena.clone(), "first", log.clone());
    let second = Extension::new(arena, "second", log.clone());
    let mut set = RecognizerSet::default();
    set.attach(&first);
    set.attach(&second);
    first.fail_delivery.set(true);
    second.fail_delivery.set(true);
    let event = down(pointer(74));
    let failure = catch_unwind(AssertUnwindSafe(|| {
        set.dispatch(PointerDispatch::at_root(&event));
    }))
    .expect_err("delivery failures");
    assert_eq!(
        failure.downcast_ref::<String>().map(String::as_str),
        Some("first")
    );
    assert_eq!(*log.borrow(), ["first", "second"]);
    first.fail_delivery.set(false);
    second.fail_delivery.set(false);
    log.borrow_mut().clear();
    let up = make_up_event_for_id(pointer(74), Offset::new(3.0, 4.0), PointerKind::Touch)
        .expect("valid fixture sample");
    set.dispatch(PointerDispatch::at_root(&up));
    assert!(first.contact.current().is_none());
    assert!(second.contact.current().is_none());
    assert_eq!(
        log.borrow()
            .iter()
            .filter(|name| **name == "first" || **name == "second")
            .copied()
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
}

fn predicates_only_gate_admission_and_sets_do_not_own_recognizers() {
    let log = Log::default();
    let owner = Extension::new(GestureArena::new(), "owner", log.clone());
    let predicate_calls = Rc::new(Cell::new(0));
    let calls = predicate_calls.clone();
    let mut set = RecognizerSet::default();
    set.attach_when(&owner, move |_| {
        calls.set(calls.get() + 1);
        false
    });
    let event = down(pointer(75));
    set.dispatch(PointerDispatch::at_root(&event));
    assert!(log.borrow().is_empty());
    let up = make_up_event_for_id(pointer(75), Offset::new(3.0, 4.0), PointerKind::Touch)
        .expect("valid fixture sample");
    set.dispatch(PointerDispatch::at_root(&up));
    assert_eq!(*log.borrow(), ["owner"]);
    assert_eq!(predicate_calls.get(), 1);
    let weak = Rc::downgrade(&owner);
    drop(owner);
    assert!(weak.upgrade().is_none());
    log.borrow_mut().clear();
    set.clone().dispatch(PointerDispatch::at_root(&up));
    assert!(log.borrow().is_empty());
}

fn contact_drop_defers_rival_notifications_and_closed_arena_refuses_admission() {
    let log = Log::default();
    let arena = GestureArena::new();
    let owner = Extension::new(arena.clone(), "owner", log.clone());
    let rival = Extension::new(arena.clone(), "rival", log.clone());
    let member: Rc<dyn GestureArenaMember> = owner.clone();
    let contact = PrimaryContact::new(ArenaMembership::new(arena.clone(), Rc::downgrade(&member)));
    let event = down(pointer(76));
    contact
        .begin(
            PointerDispatch::at_root(&event),
            &GestureSettings::default(),
        )
        .expect("live member admitted");
    let rival_entry = arena.add(pointer(76), &rival);
    arena.close(pointer(76));
    log.borrow_mut().clear();
    drop(contact);
    assert!(log.borrow().is_empty(), "Drop must not notify a live rival");
    assert!(rival_entry.member().is_some());
    arena.drain_deferred_resolutions();
    assert_eq!(*log.borrow(), ["accepted"]);
    let another = down(pointer(77));
    let _entry = arena.add(pointer(77), &rival);
    arena.close(pointer(77));
    assert!(matches!(
        owner.contact.begin(
            PointerDispatch::at_root(&another),
            &GestureSettings::default()
        ),
        Err(BeginContactError::ArenaClosed)
    ));
}

fn deadlines_refuse_overflow_and_survive_arena_resolution() {
    let arena = GestureArena::new();
    let owner = Extension::new(arena.clone(), "owner", Log::default());
    let event = down(pointer(78));
    owner
        .contact
        .begin(
            PointerDispatch::at_root(&event),
            &GestureSettings::default(),
        )
        .expect("contact admitted");
    assert!(owner.contact.arm_deadline(Duration::MAX).is_none());
    assert!(owner.contact.deadline().is_none());
    let deadline = owner
        .contact
        .arm_deadline(Duration::from_secs(1))
        .expect("deadline armed");
    arena.close(pointer(78));
    arena.drain_deferred_resolutions();
    assert_eq!(arena.next_deadline(), Some(deadline));
    owner.contact.withdraw();
    assert!(arena.next_deadline().is_none());
}

fn stale_contact_drop_cannot_withdraw_a_reused_pointer() {
    let log = Log::default();
    let arena = GestureArena::new();
    let owner = Extension::new(arena.clone(), "owner", log.clone());
    let rival = Extension::new(arena.clone(), "rival", log.clone());
    let member: Rc<dyn GestureArenaMember> = owner;
    let contact = PrimaryContact::new(ArenaMembership::new(arena.clone(), Rc::downgrade(&member)));
    let event = down(pointer(79));
    contact
        .begin(
            PointerDispatch::at_root(&event),
            &GestureSettings::default(),
        )
        .expect("old generation admitted");
    arena.abandon(pointer(79));
    let _new_generation = arena.add(pointer(79), &rival);
    arena.close(pointer(79));
    log.borrow_mut().clear();
    drop(contact);
    assert!(log.borrow().is_empty());
    arena.drain_deferred_resolutions();
    assert_eq!(*log.borrow(), ["accepted"]);
}

fn invalid_admission_remains_idle_and_settings_are_frozen() {
    let owner = Extension::new(GestureArena::new(), "owner", Log::default());
    let up = make_up_event_for_id(pointer(80), Offset::ZERO, PointerKind::Mouse)
        .expect("valid fixture sample");
    assert!(matches!(
        owner
            .contact
            .begin(PointerDispatch::at_root(&up), &GestureSettings::default()),
        Err(BeginContactError::NotDown)
    ));
    for position in [Offset::new(f64::NAN, 0.0), Offset::new(0.0, f64::INFINITY)] {
        let event = make_down_event_for_id_with_button(
            pointer(80),
            position,
            PointerKind::Mouse,
            PointerButton::PRIMARY,
        );
        assert!(
            event.is_err(),
            "checked pointer positions refuse nonfinite input before admission"
        );
        assert!(owner.contact.current().is_none());
    }
    let event = down(pointer(80));
    let settings = GestureSettings::default()
        .try_with_touch_slop(11.0)
        .expect("finite settings");
    owner
        .contact
        .begin(PointerDispatch::at_root(&event), &settings)
        .expect("valid admission after refusal");
    let settings = settings
        .try_with_touch_slop(25.0)
        .expect("replacement settings");
    assert_eq!(
        owner
            .contact
            .current()
            .expect("active contact")
            .settings
            .touch_slop(),
        11.0
    );
    assert!(!owner.contact.moved_beyond(Offset::new(14.0, 4.0), 11.0));
    assert!(owner.contact.moved_beyond(Offset::new(14.5, 4.0), 11.0));
    assert!(owner.contact.moved_beyond(Offset::new(f64::NAN, 4.0), 11.0));
    owner.contact.withdraw();
    owner
        .contact
        .begin(PointerDispatch::at_root(&event), &settings)
        .expect("new settings on next sequence");
    assert_eq!(
        owner
            .contact
            .current()
            .expect("new contact")
            .settings
            .touch_slop(),
        25.0
    );
}

fn clock_reentry_cannot_arm_a_replacement_contact() {
    let clock = Arc::new(ReentrantClock {
        now: web_time::Instant::now(),
        replace_contact: AtomicBool::new(false),
    });
    let owner = Extension::new(
        GestureArena::with_clock(clock.clone()),
        "owner",
        Log::default(),
    );
    let event = down(pointer(81));
    let old_id = owner
        .contact
        .begin(
            PointerDispatch::at_root(&event),
            &GestureSettings::default(),
        )
        .expect("old admission");
    CLOCK_CONTACT.with(|slot| *slot.borrow_mut() = Some(Rc::downgrade(&owner)));
    clock.replace_contact.store(true, Ordering::Relaxed);
    assert!(owner.contact.arm_deadline(Duration::from_secs(1)).is_none());
    CLOCK_CONTACT.with(|slot| slot.borrow_mut().take());
    let current = owner.contact.current().expect("clock replacement survives");
    assert_ne!(current.id, old_id);
    assert_eq!(current.pointer, pointer(81));
    assert!(owner.contact.deadline().is_none());
    assert!(owner.contact.arm_deadline(Duration::from_secs(1)).is_some());
}

fn scale_clock_can_cancel_admission_and_preserve_reentrant_replacements() {
    #[derive(Debug)]
    struct ScaleClock {
        now: web_time::Instant,
        armed: AtomicBool,
        replace: bool,
        fail: bool,
    }
    impl MonotonicClock for ScaleClock {
        fn now(&self) -> web_time::Instant {
            if self.armed.swap(false, Ordering::Relaxed) {
                let owner = CLOCK_SCALE.with(|slot| slot.borrow().as_ref().and_then(Weak::upgrade));
                if let Some(owner) = owner {
                    assert_eq!(
                        owner.cancel(),
                        CancelOutcome::Cancelled,
                        "Down commits admission before user clock"
                    );
                    if self.replace {
                        for (raw, x) in [(301, 10.0), (302, 110.0)] {
                            let event = make_down_event_for_id(
                                pointer(raw),
                                Offset::new(x, 0.0),
                                PointerKind::Touch,
                            )
                            .expect("finite replacement Down");
                            owner.add_pointer(PointerDispatch::at_root(&event));
                        }
                    }
                    assert!(!self.fail, "scale clock failure");
                }
            }
            self.now
        }
    }
    struct ClearScaleClock;
    impl Drop for ClearScaleClock {
        fn drop(&mut self) {
            CLOCK_SCALE.with(|slot| slot.borrow_mut().take());
        }
    }
    for (replace, fail) in [(false, false), (true, false), (true, true)] {
        let clock = Arc::new(ScaleClock {
            now: web_time::Instant::now(),
            armed: AtomicBool::new(false),
            replace,
            fail,
        });
        let arena = GestureArena::with_clock(clock.clone());
        let updates = Rc::new(RefCell::new(Vec::new()));
        let observed = updates.clone();
        let scale = ScaleGestureRecognizer::builder(arena.clone())
            .on_update(move |details| observed.borrow_mut().push(details.scale))
            .build();
        CLOCK_SCALE.with(|slot| *slot.borrow_mut() = Some(Rc::downgrade(&scale)));
        let _clear = ClearScaleClock;
        clock.armed.store(true, Ordering::Relaxed);
        let original =
            make_down_event_for_id(pointer(301), Offset::new(-200.0, 0.0), PointerKind::Touch)
                .expect("finite original Down");
        let result = catch_unwind(AssertUnwindSafe(|| {
            scale.add_pointer(PointerDispatch::at_root(&original));
        }));
        if fail {
            let payload = result.expect_err("the original clock failure propagates");
            assert_eq!(payload.downcast_ref::<&str>(), Some(&"scale clock failure"));
        } else {
            result.expect("the clock can cancel through the public owner without a RefCell panic");
        }
        if !replace {
            assert_eq!(
                scale.cancel(),
                CancelOutcome::Idle,
                "cancelled Down must not be restored by outer admission"
            );
            for (raw, x) in [(301, 10.0), (302, 110.0)] {
                let event =
                    make_down_event_for_id(pointer(raw), Offset::new(x, 0.0), PointerKind::Touch)
                        .expect("finite recovery Down");
                scale.add_pointer(PointerDispatch::at_root(&event));
            }
        }
        for raw in [301, 302] {
            arena.close(pointer(raw));
        }
        arena.drain_deferred_resolutions();
        let movement =
            make_move_event_for_id(pointer(302), Offset::new(210.0, 0.0), PointerKind::Touch)
                .expect("finite replacement Move");
        scale.handle_event(PointerDispatch::at_root(&movement));
        assert_eq!(
            updates.borrow().last(),
            Some(&2.0),
            "same-ID replacement keeps its own 100px baseline after reentry or caught clock failure"
        );
        scale.cancel();
        assert!(
            arena.is_empty(),
            "replacement retires its exact arena generations"
        );
    }
}

fn diagnostic_panic_cannot_replace_first_delivery_failure_or_skip_a_peer() {
    struct DiagnosticPanic(Arc<AtomicUsize>);
    impl tracing::Subscriber for DiagnosticPanic {
        fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
            *metadata.level() == tracing::Level::ERROR
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, _: &tracing::Event<'_>) {
            self.0.fetch_add(1, Ordering::Relaxed);
            panic!("secondary failure diagnostic panicked");
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }

    let log = Log::default();
    let arena = GestureArena::new();
    let first = Extension::new(arena.clone(), "first", log.clone());
    let second = Extension::new(arena.clone(), "second", log.clone());
    let third = Extension::new(arena, "third", log.clone());
    first.fail_delivery.set(true);
    second.fail_delivery.set(true);
    let mut set = RecognizerSet::default();
    set.attach(&first);
    set.attach(&second);
    set.attach(&third);
    let diagnostic_calls = Arc::new(AtomicUsize::new(0));
    let event = down(pointer(82));
    let failure =
        tracing::subscriber::with_default(DiagnosticPanic(diagnostic_calls.clone()), || {
            catch_unwind(AssertUnwindSafe(|| {
                set.dispatch(PointerDispatch::at_root(&event));
            }))
        })
        .expect_err("first delivery failure resumes");
    assert_eq!(
        failure.downcast_ref::<String>().map(String::as_str),
        Some("first")
    );
    assert_eq!(
        diagnostic_calls.load(Ordering::Relaxed),
        1,
        "real diagnostic ran"
    );
    assert_eq!(*log.borrow(), ["first", "second", "third"]);
    first.fail_delivery.set(false);
    second.fail_delivery.set(false);
    let up = make_up_event_for_id(pointer(82), Offset::ZERO, PointerKind::Touch)
        .expect("valid fixture sample");
    set.dispatch(PointerDispatch::at_root(&up));
    assert!(first.contact.current().is_none());
    assert!(second.contact.current().is_none());
    assert!(third.contact.current().is_none());
}

fn double_tap_drop_releases_a_pending_sweep_without_inline_notifications() {
    held_recognizer_drop_releases_pending_sweep(
        |arena| DoubleTapGestureRecognizer::builder(arena).build(),
        false,
    );
}

fn multi_tap_drop_releases_a_pending_sweep_without_inline_notifications() {
    held_recognizer_drop_releases_pending_sweep(
        |arena| MultiTapGestureRecognizer::builder(arena, 2).build(),
        true,
    );
}

fn held_recognizer_drop_releases_pending_sweep(
    make_owner: fn(GestureArena) -> Rc<dyn GestureRecognizer>,
    second_contact: bool,
) {
    let arena = GestureArena::binding_driven(Arc::new(flui_interaction::ManualClock::new()));
    let owner = make_owner(arena.clone());
    let first_log = Log::default();
    let second_log = Log::default();
    let first = Extension::new(arena.clone(), "first rival", first_log.clone());
    let second = Extension::new(arena.clone(), "second rival", second_log.clone());
    let event = down(pointer(83));
    owner.add_pointer(PointerDispatch::at_root(&event));
    let _first_entry = arena.add(pointer(83), &first);
    let _second_entry = arena.add(pointer(83), &second);
    run_pointer_lifecycle(&arena, &event);
    if second_contact {
        let another = down(pointer(84));
        owner.add_pointer(PointerDispatch::at_root(&another));
        run_pointer_lifecycle(&arena, &another);
    }
    let up = make_up_event_for_id(pointer(83), Offset::new(3.0, 4.0), PointerKind::Touch)
        .expect("valid fixture sample");
    owner.handle_event(PointerDispatch::at_root(&up));
    run_pointer_lifecycle(&arena, &up);
    assert!(arena.is_held(pointer(83)), "actual recognizer owns a hold");
    assert!(
        arena.has_pending_sweep(pointer(83)),
        "Up accepted deferred sweep debt"
    );
    assert!(first_log.borrow().is_empty());
    assert!(second_log.borrow().is_empty());
    drop(owner);
    assert!(
        first_log.borrow().is_empty(),
        "Drop must not notify a rival"
    );
    assert!(
        second_log.borrow().is_empty(),
        "Drop must not notify a rival"
    );
    arena.drain_deferred_resolutions();
    assert_eq!(
        *first_log.borrow(),
        ["accepted"],
        "pending sweep chooses the surviving front member"
    );
    assert_eq!(*second_log.borrow(), ["rejected"]);
    assert!(arena.is_empty());
}

#[test]
fn public_recognizer_extension_contracts() {
    let cases: &[(&str, fn())] = &[
        (
            "second_pointer_does_not_replace_the_primary_contact",
            second_pointer_does_not_replace_the_primary_contact,
        ),
        (
            "cancel_all_cancels_every_recognizer_before_resuming_the_first_panic",
            cancellation_finishes_every_owner_before_first_panic_resumes,
        ),
        (
            "two_recognizers_panicking_on_one_pointer_keep_the_first_payload",
            recognizer_set_preserves_first_failure_and_recovers,
        ),
        (
            "listener_admits_by_predicate_and_forwards_terminal_events",
            predicates_only_gate_admission_and_sets_do_not_own_recognizers,
        ),
        (
            "contact_drop_defers_rival_notifications_and_closed_arena_refuses_admission",
            contact_drop_defers_rival_notifications_and_closed_arena_refuses_admission,
        ),
        (
            "deadlines_refuse_overflow_and_survive_arena_resolution",
            deadlines_refuse_overflow_and_survive_arena_resolution,
        ),
        (
            "stale_contact_drop_cannot_withdraw_a_reused_pointer",
            stale_contact_drop_cannot_withdraw_a_reused_pointer,
        ),
        (
            "invalid_admission_remains_idle_and_settings_are_frozen",
            invalid_admission_remains_idle_and_settings_are_frozen,
        ),
        (
            "clock_reentry_cannot_arm_a_replacement_contact",
            clock_reentry_cannot_arm_a_replacement_contact,
        ),
        (
            "scale_clock_can_cancel_admission_and_preserve_reentrant_replacements",
            scale_clock_can_cancel_admission_and_preserve_reentrant_replacements,
        ),
        (
            "diagnostic_panic_cannot_replace_first_delivery_failure_or_skip_a_peer",
            diagnostic_panic_cannot_replace_first_delivery_failure_or_skip_a_peer,
        ),
        (
            "double_tap_drop_releases_a_pending_sweep_without_inline_notifications",
            double_tap_drop_releases_a_pending_sweep_without_inline_notifications,
        ),
        (
            "multi_tap_drop_releases_a_pending_sweep_without_inline_notifications",
            multi_tap_drop_releases_a_pending_sweep_without_inline_notifications,
        ),
    ];
    let mut first = None;
    for &(name, case) in cases {
        if let Err(payload) = catch_unwind(AssertUnwindSafe(case)) {
            eprintln!("public recognizer contract failed: {name}");
            if first.is_none() {
                first = Some(payload);
            } else {
                flui_foundation::panic::retain_opaque_payload(payload);
            }
        }
    }
    if let Some(payload) = first {
        resume_unwind(payload);
    }
}

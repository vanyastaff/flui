//! Public extension contracts, mounted with the atomic recognizer migration.

use std::{
    cell::{Cell, RefCell},
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    rc::{Rc, Weak},
    time::Duration,
};

use flui_foundation::geometry::Offset;
use flui_interaction::{
    ArenaMembership, BeginContactError, CancelOutcome, GestureArena, GestureArenaMember,
    GestureRecognizer, GestureSettings, PointerId, PrimaryContact, RecognizerSet, cancel_all,
    events::{
        PointerButton, PointerEvent, PointerType, make_down_event_for_id_with_button,
        make_up_event_for_id,
    },
    routing::PointerDispatch,
};

type Log = Rc<RefCell<Vec<&'static str>>>;

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
    PointerId::new(raw).expect("nonzero fixture pointer")
}

fn down(pointer: PointerId) -> PointerEvent {
    make_down_event_for_id_with_button(
        pointer,
        Offset::new(3.0, 4.0),
        PointerType::Touch,
        PointerButton::Primary,
    )
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
    assert_eq!(snapshot.kind, PointerType::Touch);
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
        set.dispatch(PointerDispatch::at_root(&event))
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
    let up = make_up_event_for_id(pointer(74), Offset::new(3.0, 4.0), PointerType::Touch);
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
    let up = make_up_event_for_id(pointer(75), Offset::new(3.0, 4.0), PointerType::Touch);
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
    let member: Rc<dyn GestureArenaMember> = owner.clone();
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
    ];
    for &(name, case) in cases {
        if let Err(payload) = catch_unwind(AssertUnwindSafe(case)) {
            eprintln!("public recognizer contract failed: {name}");
            resume_unwind(payload);
        }
    }
}

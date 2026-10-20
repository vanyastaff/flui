use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use flui_foundation::geometry::Offset;
use flui_interaction::{
    CancelOutcome, DoubleTapGestureRecognizer, GestureArena, GestureRecognizer, GestureSettings,
    GestureSettingsSource, PointerId, TapDownDetails, TapGestureRecognizer, TapUpDetails,
    events::{
        PointerButton, PointerEventExt, PointerKind, make_down_event,
        make_down_event_for_id_with_button, make_move_event, make_up_event,
        make_up_event_for_id_with_button,
    },
    pointer::PointerDispatch,
};

#[test]
fn tap_builder_lifecycle_contract() {
    for (name, row) in [
        ("cancel_reuses_tap", cancel_reuses_tap as fn()),
        (
            "phase payloads preserve each button and coordinate space",
            tap_phase_payloads_preserve_button_families_and_spaces,
        ),
        (
            "live_settings_apply_to_next_contact",
            live_settings_apply_to_next_contact,
        ),
        (
            "panicking_cancel_callback_cannot_strand_tap_tracking",
            panicking_cancel_callback_cannot_strand_tap_tracking,
        ),
        (
            "cancel_during_up_suppresses_tap",
            cancel_during_up_suppresses_tap,
        ),
        ("cancel_reuses_double_tap", cancel_reuses_double_tap),
        (
            "replacing_builder_callback_preserves_retirement_failure",
            replacing_builder_callback_preserves_retirement_failure,
        ),
        (
            "a_tap_joins_only_contacts_of_buttons_it_reports",
            a_tap_joins_only_contacts_of_buttons_it_reports,
        ),
        (
            "an_undecided_press_shows_before_its_release",
            an_undecided_press_shows_before_its_release,
        ),
    ] {
        if let Err(payload) = std::panic::catch_unwind(row) {
            eprintln!("tap contract `{name}` failed");
            std::panic::resume_unwind(payload);
        }
    }
}

/// A press its arena has not decided shows as pressed 100 ms after Down,
/// before its release; the up and the tap still wait for the win. A press
/// another member takes before then showed nothing and hears nothing, not
/// even a cancellation: a cancel only ends a down that was sent.
fn an_undecided_press_shows_before_its_release() {
    use flui_interaction::gesture::{GestureArenaMember, GestureContest};
    struct Rival;
    impl GestureArenaMember for Rival {
        fn accept_gesture(&self, _: GestureContest) {}
        fn reject_gesture(&self, _: GestureContest) {}
    }
    /// What happens after Down: wait this long, then the rival takes the
    /// contact or the press is released and swept.
    #[derive(Clone, Copy, Debug)]
    enum Then {
        Released,
        Taken,
    }
    let rows: [(u64, Then, &[&str]); 4] = [
        (150, Then::Released, &["down", "up", "tap"]),
        (150, Then::Taken, &["down", "cancel"]),
        (50, Then::Taken, &[]),
        (50, Then::Released, &["down", "up", "tap"]),
    ];
    for (wait, then, heard) in rows {
        let clock = flui_interaction::ManualClock::new();
        let arena = GestureArena::binding_driven(std::sync::Arc::new(clock.clone()));
        let log = Rc::new(RefCell::new(Vec::new()));
        let entry = |name: &'static str| {
            let log = Rc::clone(&log);
            move || log.borrow_mut().push(name)
        };
        let (on_down, on_up, on_tap, on_cancel) =
            (entry("down"), entry("up"), entry("tap"), entry("cancel"));
        let recognizer = TapGestureRecognizer::builder(arena.clone())
            .on_tap_down(move |_| on_down())
            .on_tap_up(move |_| on_up())
            .on_tap(move |_| on_tap())
            .on_tap_cancel(move |_| on_cancel())
            .build();
        let rival = Rc::new(Rival);
        let down = make_down_event(Offset::ZERO, PointerKind::Touch).expect("valid fixture sample");
        let pointer = down.pointer_id().expect("fixture contact identity");
        recognizer.add_pointer(PointerDispatch::at_root(&down));
        let _rival_entry = arena.add(pointer, &rival);
        arena.close(pointer);
        arena.drain_deferred_resolutions();
        assert!(
            log.borrow().is_empty(),
            "an undecided press shows nothing yet"
        );
        clock.advance(std::time::Duration::from_millis(wait));
        arena.poll_deadlines();
        let shown: &[&str] = if wait >= 100 { &["down"] } else { &[] };
        assert_eq!(&*log.borrow(), shown, "{wait} ms undecided, still held");
        match then {
            Then::Taken => {
                let erased: Rc<dyn GestureArenaMember> = rival.clone();
                arena.accept(pointer, &erased);
            }
            Then::Released => {
                let up =
                    make_up_event(Offset::ZERO, PointerKind::Touch).expect("valid fixture sample");
                recognizer.handle_event(PointerDispatch::at_root(&up));
                arena.sweep(pointer);
            }
        }
        arena.drain_deferred_resolutions();
        assert_eq!(&*log.borrow(), heard, "{then:?} after {wait} ms");
        assert!(arena.is_empty());
    }
}

fn tap_phase_payloads_preserve_button_families_and_spaces() {
    let arena = GestureArena::new();
    let log = Rc::new(RefCell::new(Vec::new()));
    let down_callback = |family| {
        let log = Rc::clone(&log);
        move |details: TapDownDetails| {
            log.borrow_mut().push((
                family,
                "down",
                details.global_position,
                details.local_position,
                details.kind,
            ));
        }
    };
    let up_callback = |family, phase| {
        let log = Rc::clone(&log);
        move |details: TapUpDetails| {
            log.borrow_mut().push((
                family,
                phase,
                details.global_position,
                details.local_position,
                details.kind,
            ));
        }
    };
    let recognizer = TapGestureRecognizer::builder(arena.clone())
        .on_tap_down(down_callback("primary"))
        .on_tap_up(up_callback("primary", "up"))
        .on_tap(up_callback("primary", "tap"))
        .on_secondary_tap_down(down_callback("secondary"))
        .on_secondary_tap_up(up_callback("secondary", "up"))
        .on_secondary_tap(up_callback("secondary", "tap"))
        .on_tertiary_tap_down(down_callback("auxiliary"))
        .on_tertiary_tap_up(up_callback("auxiliary", "up"))
        .on_tertiary_tap(up_callback("auxiliary", "tap"))
        .build();
    let pointer = PointerId::try_from(81_u64).expect("fixture pointer");
    for (family, button) in [
        ("primary", PointerButton::PRIMARY),
        ("secondary", PointerButton::SECONDARY),
        ("auxiliary", PointerButton::AUXILIARY),
    ] {
        let local_down = make_down_event_for_id_with_button(
            pointer,
            Offset::new(1.0, 2.0),
            PointerKind::Mouse,
            button,
        )
        .expect("local down");
        let root_down = make_down_event_for_id_with_button(
            pointer,
            Offset::new(10.0, 20.0),
            PointerKind::Mouse,
            button,
        )
        .expect("root down");
        let local_up = make_up_event_for_id_with_button(
            pointer,
            Offset::new(3.0, 4.0),
            PointerKind::Mouse,
            button,
        )
        .expect("local up");
        let root_up = make_up_event_for_id_with_button(
            pointer,
            Offset::new(30.0, 40.0),
            PointerKind::Mouse,
            button,
        )
        .expect("root up");
        recognizer.add_pointer(PointerDispatch::new(&local_down, &root_down));
        arena.close(pointer);
        recognizer.handle_event(PointerDispatch::new(&local_up, &root_up));
        arena.drain_deferred_resolutions();
        assert_eq!(
            log.borrow().as_slice(),
            &[
                (
                    family,
                    "down",
                    Offset::new(10.0, 20.0),
                    Offset::new(1.0, 2.0),
                    PointerKind::Mouse
                ),
                (
                    family,
                    "up",
                    Offset::new(30.0, 40.0),
                    Offset::new(3.0, 4.0),
                    PointerKind::Mouse
                ),
                (
                    family,
                    "tap",
                    Offset::new(30.0, 40.0),
                    Offset::new(3.0, 4.0),
                    PointerKind::Mouse
                ),
            ]
        );
        log.borrow_mut().clear();
    }
}

fn live_settings_apply_to_next_contact() {
    let arena = GestureArena::new();
    let source = GestureSettingsSource::new(GestureSettings::default());
    let taps = Rc::new(Cell::new(0));
    let recognizer = TapGestureRecognizer::builder(arena.clone())
        .settings(source.provider())
        .on_tap({
            let taps = taps.clone();
            move |_| taps.set(taps.get() + 1)
        })
        .build();
    let down = make_down_event(Offset::ZERO, PointerKind::Touch).expect("valid sample");
    let moved = make_move_event(Offset::new(10.0, 0.0), PointerKind::Touch).expect("valid sample");
    let up = make_up_event(Offset::new(10.0, 0.0), PointerKind::Touch).expect("valid sample");
    let pointer = down.pointer_id().expect("contact identity");

    recognizer.add_pointer(PointerDispatch::at_root(&down));
    source.replace(
        GestureSettings::default()
            .try_with_touch_slop(2.0)
            .expect("valid slop"),
    );
    recognizer.handle_event(PointerDispatch::at_root(&moved));
    arena.close(pointer);
    recognizer.handle_event(PointerDispatch::at_root(&up));
    arena.drain_deferred_resolutions();
    assert_eq!(
        taps.get(),
        1,
        "active contact retains its 18 px admission tolerance"
    );

    recognizer.add_pointer(PointerDispatch::at_root(&down));
    recognizer.handle_event(PointerDispatch::at_root(&moved));
    arena.close(pointer);
    recognizer.handle_event(PointerDispatch::at_root(&up));
    arena.drain_deferred_resolutions();
    assert_eq!(
        taps.get(),
        1,
        "next contact captures the new 2 px tolerance"
    );

    source.replace(GestureSettings::default());
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    recognizer.handle_event(PointerDispatch::at_root(&moved));
    arena.close(pointer);
    recognizer.handle_event(PointerDispatch::at_root(&up));
    arena.drain_deferred_resolutions();
    assert_eq!(
        taps.get(),
        2,
        "restoring policy restores next-contact behavior"
    );
}

struct BuilderCapture {
    dropped: Rc<Cell<u32>>,
    panic_on_drop: bool,
}
impl Drop for BuilderCapture {
    fn drop(&mut self) {
        self.dropped.set(self.dropped.get() + 1);
        assert!(!self.panic_on_drop, "old callback drop");
    }
}

fn replacing_builder_callback_preserves_retirement_failure() {
    let arena = GestureArena::new();
    let old_drops = Rc::new(Cell::new(0));
    let incoming_drops = Rc::new(Cell::new(0));
    let old = BuilderCapture {
        dropped: old_drops.clone(),
        panic_on_drop: true,
    };
    let incoming = BuilderCapture {
        dropped: incoming_drops.clone(),
        panic_on_drop: false,
    };
    let builder = TapGestureRecognizer::builder(arena.clone()).on_tap(move |_| {
        std::hint::black_box(&old);
    });
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        drop(builder.on_tap(move |_| {
            std::hint::black_box(&incoming);
        }));
    }))
    .expect_err("old capture failure must resume");
    assert_eq!(failure.downcast_ref::<&str>(), Some(&"old callback drop"));
    assert_eq!(old_drops.get(), 1);
    assert_eq!(
        incoming_drops.get(),
        0,
        "incoming capture must be retained after failure"
    );

    let healthy_drops = Rc::new(Cell::new(0));
    let healthy = BuilderCapture {
        dropped: healthy_drops.clone(),
        panic_on_drop: false,
    };
    let fresh = TapGestureRecognizer::builder(arena).on_tap(move |_| {
        std::hint::black_box(&healthy);
    });
    drop(fresh);
    assert_eq!(
        healthy_drops.get(),
        1,
        "fresh healthy builder retires normally"
    );
}

fn panicking_cancel_callback_cannot_strand_tap_tracking() {
    let arena = GestureArena::new();
    let cancels = Rc::new(Cell::new(0));
    let taps = Rc::new(Cell::new(0));
    let recognizer = TapGestureRecognizer::builder(arena.clone())
        .on_tap_cancel({
            let cancels = cancels.clone();
            move |_| {
                cancels.set(cancels.get() + 1);
                panic!("tap cancel panic");
            }
        })
        .on_tap({
            let taps = taps.clone();
            move |_| taps.set(taps.get() + 1)
        })
        .build();
    let down = make_down_event(Offset::ZERO, PointerKind::Touch).expect("valid fixture sample");
    let up = make_up_event(Offset::ZERO, PointerKind::Touch).expect("valid fixture sample");
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    // The cancel only ends a press whose down was sent: the sole member wins
    // at close, which sends it.
    arena.close(down.pointer_id().expect("fixture contact identity"));
    arena.drain_deferred_resolutions();
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| recognizer.cancel()));
    assert!(failure.is_err());
    assert_eq!(recognizer.cancel(), CancelOutcome::Idle);
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    arena.close(down.pointer_id().expect("fixture contact identity"));
    recognizer.handle_event(PointerDispatch::at_root(&up));
    arena.drain_deferred_resolutions();
    assert_eq!(cancels.get(), 1);
    assert_eq!(taps.get(), 1);
}

/// A tap with callbacks only for the secondary button stays out of a primary
/// contact's arena, so it cannot hold a primary drag back (a context menu's
/// secondary click beside a dismiss drag); a secondary press it does join.
fn a_tap_joins_only_contacts_of_buttons_it_reports() {
    let arena = GestureArena::new();
    let opened = Rc::new(Cell::new(0));
    let recognizer = TapGestureRecognizer::builder(arena.clone())
        .on_secondary_tap({
            let opened = opened.clone();
            move |_| opened.set(opened.get() + 1)
        })
        .build();
    let primary = make_down_event_for_id_with_button(
        PointerId::new(std::num::NonZeroU64::MIN),
        Offset::ZERO,
        PointerKind::Mouse,
        PointerButton::PRIMARY,
    )
    .expect("valid fixture sample");
    recognizer.add_pointer(PointerDispatch::at_root(&primary));
    assert!(
        !arena.is_open(PointerId::new(std::num::NonZeroU64::MIN)),
        "no member joined the primary contact"
    );
    let pointer = PointerId::new(std::num::NonZeroU64::new(2).expect("nonzero"));
    let secondary = make_down_event_for_id_with_button(
        pointer,
        Offset::ZERO,
        PointerKind::Mouse,
        PointerButton::SECONDARY,
    )
    .expect("valid fixture sample");
    recognizer.add_pointer(PointerDispatch::at_root(&secondary));
    assert!(arena.is_open(pointer), "the secondary press joined");
    arena.close(pointer);
    let up = make_up_event_for_id_with_button(
        pointer,
        Offset::ZERO,
        PointerKind::Mouse,
        PointerButton::SECONDARY,
    )
    .expect("valid fixture sample");
    recognizer.handle_event(PointerDispatch::at_root(&up));
    arena.drain_deferred_resolutions();
    assert_eq!(opened.get(), 1, "the secondary click is reported");
}

fn cancel_reuses_tap() {
    let arena = GestureArena::new();
    let taps = Rc::new(Cell::new(0));
    let cancels = Rc::new(Cell::new(0));
    let recognizer = TapGestureRecognizer::builder(arena.clone())
        .on_tap({
            let taps = taps.clone();
            move |_| taps.set(taps.get() + 1)
        })
        .on_tap_cancel({
            let cancels = cancels.clone();
            move |_| cancels.set(cancels.get() + 1)
        })
        .build();
    let down = make_down_event(Offset::ZERO, PointerKind::Touch).expect("valid fixture sample");
    let up = make_up_event(Offset::ZERO, PointerKind::Touch).expect("valid fixture sample");
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    assert_eq!(recognizer.cancel(), CancelOutcome::Cancelled);
    assert_eq!(recognizer.cancel(), CancelOutcome::Idle);
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    arena.close(down.pointer_id().expect("fixture contact identity"));
    recognizer.handle_event(PointerDispatch::at_root(&up));
    arena.drain_deferred_resolutions();
    assert_eq!(taps.get(), 1);
    assert_eq!(cancels.get(), 1);
}

fn cancel_during_up_suppresses_tap() {
    let arena = GestureArena::new();
    let holder = Rc::new(RefCell::new(None::<Rc<TapGestureRecognizer>>));
    let taps = Rc::new(Cell::new(0));
    let recognizer = TapGestureRecognizer::builder(arena.clone())
        .on_tap_up({
            let holder = holder.clone();
            move |_| {
                holder.borrow().as_ref().expect("recognizer owner").cancel();
            }
        })
        .on_tap({
            let taps = taps.clone();
            move |_| taps.set(taps.get() + 1)
        })
        .build();
    *holder.borrow_mut() = Some(recognizer.clone());
    let down = make_down_event(Offset::ZERO, PointerKind::Touch).expect("valid fixture sample");
    let up = make_up_event(Offset::ZERO, PointerKind::Touch).expect("valid fixture sample");
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    arena.close(down.pointer_id().expect("fixture contact identity"));
    arena.drain_deferred_resolutions();
    recognizer.handle_event(PointerDispatch::at_root(&up));
    assert_eq!(taps.get(), 0);
    holder.borrow_mut().take();
}

fn cancel_reuses_double_tap() {
    let clock = flui_interaction::ManualClock::new();
    let arena = GestureArena::with_clock(std::sync::Arc::new(clock.clone()));
    let doubles = Rc::new(Cell::new(0));
    let recognizer = DoubleTapGestureRecognizer::builder(arena.clone())
        .on_double_tap({
            let doubles = doubles.clone();
            move |_| doubles.set(doubles.get() + 1)
        })
        .build();
    let down = make_down_event(Offset::ZERO, PointerKind::Touch).expect("valid fixture sample");
    let up = make_up_event(Offset::ZERO, PointerKind::Touch).expect("valid fixture sample");
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    assert_eq!(recognizer.cancel(), CancelOutcome::Cancelled);
    assert_eq!(recognizer.cancel(), CancelOutcome::Idle);
    for _ in 0..2 {
        recognizer.add_pointer(PointerDispatch::at_root(&down));
        arena.close(down.pointer_id().expect("fixture contact identity"));
        recognizer.handle_event(PointerDispatch::at_root(&up));
        arena.sweep(down.pointer_id().expect("fixture contact identity"));
        clock.advance(std::time::Duration::from_millis(40));
    }
    assert_eq!(doubles.get(), 1);
    assert_eq!(recognizer.cancel(), CancelOutcome::Idle);
}

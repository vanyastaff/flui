use std::cell::RefCell;

use flui_interaction::events::{PointerType, make_down_event};
use flui_interaction::testing::input::KeyEventBuilder;
use flui_types::ImeEvent;
use flui_types::geometry::{Offset, Pixels};

use super::*;

fn headless_text_input() -> (
    Arc<flui_platform::FakeTextInput>,
    Arc<dyn flui_platform::traits::PlatformTextInput>,
) {
    let fake = Arc::new(flui_platform::FakeTextInput::new());
    let capability: Arc<dyn flui_platform::traits::PlatformTextInput> = fake.clone();
    (fake, capability)
}

/// A pointer event addressed to B (the NON-primary presentation)
/// must reach ONLY B's own gesture arena — never A's (the primary),
/// even though both presentations share one realm's `enter()` scope
/// and dispatch machinery.
///
/// Deliberately stamps for B, not A: A is this realm's primary, so a
/// mutant that reroutes `handle_input_addressed` to
/// `self.presentations.primary()` regardless of the addressed id
/// would still (accidentally) satisfy an A-stamped version of this
/// test — addressed-vs-primary is unobservable when the stamped
/// target IS the primary. Stamping for B is the only fixture that
/// actually exercises the addressed lookup: reverting the `Pointer`
/// arm to `self.presentations.primary()` makes this fail (B's count
/// stays `0`, A's becomes `1`).
#[test]
fn input_stamped_for_b_never_reaches_as_arena() {
    let mut realm = UiRealm::for_test();
    let a_id = realm.presentation_id();
    let b_id = realm.install_second_presentation_for_test();

    let down = make_down_event(Offset::new(Pixels(4.0), Pixels(6.0)), PointerType::Mouse);
    realm.enter(|realm| {
        realm.handle_input_addressed(b_id, PlatformInput::Pointer(down));
    });

    assert_eq!(
        realm
            .presentations
            .get(b_id)
            .expect("B installed")
            .gestures()
            .active_pointer_count(),
        1,
        "the addressed (non-primary) presentation must receive the pointer event"
    );
    assert_eq!(
        realm
            .presentations
            .get(a_id)
            .expect("A installed")
            .gestures()
            .active_pointer_count(),
        0,
        "a pointer event stamped for B must never reach A's own gesture arena, even \
         though A is this realm's primary"
    );
}

/// Input addressed to a presentation this realm does not (or no
/// longer) host must drop traced -- never silently fall through to
/// the primary, and never reach ANY live presentation's own arena.
/// Covers both shapes of "not a live presentation": an id that
/// never existed in this realm, and a real id that existed, was
/// closed, and is now gone from the forest.
///
/// If reverted (`handle_input_addressed` falling back to
/// `self.presentations.primary()` when the addressed id is not
/// found, instead of tracing and returning): this fails in BOTH
/// cases -- A's (the primary's) pointer count would become
/// nonzero instead of staying `0`.
#[test]
fn input_addressed_to_a_closed_or_unknown_presentation_drops_traced_never_falls_through() {
    let mut realm = UiRealm::for_test();
    let a_id = realm.presentation_id();
    let b_id = realm.install_second_presentation_for_test();

    // Case 1: an id that never existed in this realm at all.
    let bogus = flui_foundation::PresentationId::new_gen(
        999,
        std::num::NonZeroU32::new(999).expect("nonzero"),
    );
    let down_for_bogus = make_down_event(Offset::new(Pixels(4.0), Pixels(6.0)), PointerType::Mouse);
    realm.enter(|realm| {
        realm.handle_input_addressed(bogus, PlatformInput::Pointer(down_for_bogus));
    });
    assert_eq!(
        realm
            .presentations
            .get(a_id)
            .expect("A installed")
            .gestures()
            .active_pointer_count(),
        0,
        "input addressed to a never-existed presentation id must never fall through \
         to the primary"
    );
    assert_eq!(
        realm
            .presentations
            .get(b_id)
            .expect("B installed")
            .gestures()
            .active_pointer_count(),
        0,
        "input addressed to a never-existed presentation id must never reach ANY \
         live presentation"
    );

    // Case 2: a real id that existed, was closed, and is now gone
    // from the forest.
    realm.close_presentation_entered(b_id);
    assert!(
        realm.presentations.get(b_id).is_none(),
        "precondition: B must actually be gone from the forest after closing"
    );
    let down_for_closed =
        make_down_event(Offset::new(Pixels(8.0), Pixels(10.0)), PointerType::Mouse);
    realm.enter(|realm| {
        realm.handle_input_addressed(b_id, PlatformInput::Pointer(down_for_closed));
    });
    assert_eq!(
        realm
            .presentations
            .get(a_id)
            .expect("A installed")
            .gestures()
            .active_pointer_count(),
        0,
        "input addressed to a CLOSED presentation id must never fall through to the \
         surviving primary either"
    );
}

/// Two independently attached IME sessions, one per presentation:
/// delivering an event addressed to B (the NON-primary presentation)
/// must reach only B's attached client and must not disturb A's
/// (the primary's) own platform IME session.
///
/// Deliberately addresses B, not A — see `input_stamped_for_b_never_
/// reaches_as_arena`'s doc for why an A-addressed fixture cannot
/// distinguish "delivered to the addressed presentation" from
/// "delivered to the primary": reverting the `Ime` arm to
/// `self.presentations.primary()` makes this fail (B's sink stays
/// empty, A's receives the event instead).
#[test]
fn ime_event_addressed_to_b_does_not_reach_as_session() {
    let (fake_a, capability_a) = headless_text_input();
    let (fake_b, capability_b) = headless_text_input();

    let mut realm = UiRealm::for_test_with_text_input(Some(capability_a));
    let a_id = realm.presentation_id();
    let window_b = crate::app::presentation::test_platform_window(Some(capability_b));
    let presentation_b = realm.assemble_presentation(window_b);
    let b_id = realm.install_presentation(presentation_b);

    let received_a = Rc::new(RefCell::new(Vec::new()));
    let sink_a = Rc::clone(&received_a);
    let handle_a = realm.presentation_text_input_handle_for_test(a_id);
    let _token_a = handle_a
        .attach(Rc::new(move |event: &ImeEvent| {
            sink_a.borrow_mut().push(event.clone());
        }))
        .expect("A's headless window supports text input");

    let received_b = Rc::new(RefCell::new(Vec::new()));
    let sink_b = Rc::clone(&received_b);
    let handle_b = realm.presentation_text_input_handle_for_test(b_id);
    let _token_b = handle_b
        .attach(Rc::new(move |event: &ImeEvent| {
            sink_b.borrow_mut().push(event.clone());
        }))
        .expect("B's headless window supports text input");

    assert_eq!(fake_a.last_ime_allowed(), Some(true));
    assert_eq!(fake_b.last_ime_allowed(), Some(true));

    realm.enter(|realm| {
        realm.handle_input_addressed(
            b_id,
            PlatformInput::Ime(ImeEvent::Commit("hello".to_string())),
        );
    });

    assert_eq!(
        received_b.borrow().as_slice(),
        [ImeEvent::Commit("hello".to_string())],
        "B's attached client must receive the event addressed to B, even though A is \
         this realm's primary"
    );
    assert!(
        received_a.borrow().is_empty(),
        "an IME event addressed to B must never reach A's attached client"
    );
    assert_eq!(
        fake_a.last_ime_allowed(),
        Some(true),
        "delivering an event to B must not touch A's own platform IME session"
    );
}

/// Keyboard input always routes to the realm's ACTIVE presentation
/// (`FocusCoordinator`), never to whichever presentation an
/// individual event happens to be stamped for — the race/stray-event
/// case where the stamp and the OS-focused window disagree.
///
/// Oracle: each presentation's own wake-only `redraw_pending` bit
/// (`take_redraw_pending`), set only by the presentation that
/// actually ran `dispatch_key_event`. If reverted (the `Keyboard`
/// arm dispatches to the STAMPED `presentation_id` instead of
/// `self.focus_coordinator.active()`): A's bit would be set and B's
/// would not, the opposite of what this test asserts.
#[test]
fn keyboard_routes_to_active_presentation_only() {
    let mut realm = UiRealm::for_test();
    let a_id = realm.presentation_id();
    let b_id = realm.install_second_presentation_for_test();
    assert_eq!(
        realm.active_presentation_for_test(),
        a_id,
        "a freshly constructed realm starts with its initial presentation active"
    );

    realm.notify_presentation_focus_gained(b_id);
    assert_eq!(
        realm.active_presentation_for_test(),
        b_id,
        "a WindowFocus(true) naming B must move the active presentation to B"
    );

    let key_event = KeyEventBuilder::new(flui_interaction::events::Code::KeyA).build();
    // Stamped for A -- but B is the currently active presentation.
    realm.enter(|realm| {
        realm.handle_input_addressed(a_id, PlatformInput::Keyboard(key_event));
    });

    let a_presentation = realm.presentations.get(a_id).expect("A installed");
    let b_presentation = realm.presentations.get(b_id).expect("B installed");
    assert!(
        !a_presentation.take_redraw_pending(),
        "A is only the STAMPED presentation, never the active one here -- it must not \
         have received the keyboard event"
    );
    assert!(
        b_presentation.take_redraw_pending(),
        "keyboard input must route to B, the ACTIVE presentation, regardless of which \
         presentation the event was stamped for"
    );
}

// This module can only exercise `UiRealm`'s own write side
// (`notify_presentation_focus_gained`) directly -- the production
// `WindowFocus(false)`-never-moves-active guard actually lives in
// `runner.rs`'s `PlatformToUi::run` (the `if focused` check around
// the call to `notify_presentation_focus_gained`), which a direct
// `UiRealm`-level test cannot reach or mutate. See
// `realm_dispatch_tests::window_focus_true_moves_active_
// presentation_end_to_end_and_false_does_not` (`runner.rs`) for the
// real end-to-end proof, driven through `dispatch_platform_realm`
// with genuine `RealmTask::Event(PlatformToUi::WindowFocus(_))`
// tasks -- a vacuous direct-call version of this test (set B active,
// assert B active, assert B active again with no operation between)
// used to live here and was replaced for exactly that reason.

/// A focus-gained notification naming a presentation this realm no
/// longer hosts (already closed, or a forged id) is a traced no-op —
/// arbitration never points at a dead presentation.
#[test]
fn focus_gained_for_an_unknown_presentation_is_a_traced_no_op() {
    let realm = UiRealm::for_test();
    let a_id = realm.presentation_id();
    let bogus = flui_foundation::PresentationId::new_gen(
        999,
        std::num::NonZeroU32::new(999).expect("nonzero"),
    );

    realm.notify_presentation_focus_gained(bogus);

    assert_eq!(
        realm.active_presentation_for_test(),
        a_id,
        "an unknown presentation id must never become the active one"
    );
}

/// Closing the realm's currently ACTIVE presentation must re-stamp
/// `FocusCoordinator` to the surviving primary before returning --
/// never leave it pointing at a now-dead id, which would
/// black-hole every keyboard event until a fresh `WindowFocus(true)`
/// happened to name a live presentation. Closes B (the non-primary)
/// while B is active, so this also exercises "the active
/// presentation being closed is not the primary" -- not just "close
/// the primary while it's active".
///
/// If reverted (the re-stamp removed from
/// `close_presentation_entered`): this fails -- the post-close
/// keyboard event resolves `self.presentations.get(dead_b_id)` to
/// `None` and drops traced, so A's `redraw_pending` bit is never
/// set.
#[test]
fn keyboard_after_closing_the_active_presentation_routes_to_the_survivor() {
    let mut realm = UiRealm::for_test();
    let a_id = realm.presentation_id();
    let b_id = realm.install_second_presentation_for_test();

    realm.notify_presentation_focus_gained(b_id);
    assert_eq!(realm.active_presentation_for_test(), b_id);

    realm.close_presentation_entered(b_id);

    assert_eq!(
        realm.active_presentation_for_test(),
        a_id,
        "closing the active presentation must re-stamp active to the surviving primary"
    );

    let key_event = KeyEventBuilder::new(flui_interaction::events::Code::KeyA).build();
    realm.enter(|realm| {
        realm.handle_input_addressed(a_id, PlatformInput::Keyboard(key_event));
    });

    assert!(
        realm
            .presentations
            .get(a_id)
            .expect("A installed")
            .take_redraw_pending(),
        "keyboard input must reach the surviving primary after the active presentation \
         closes, never drop as if no presentation were active"
    );
}

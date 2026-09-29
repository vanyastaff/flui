use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use flui_foundation::geometry::Offset;
use flui_interaction::events::{PointerType, make_down_event};
use flui_interaction::routing::{FocusNode, KeyEventResult};
use flui_interaction::testing::input::KeyEventBuilder;
use flui_platform_api::ImeEvent;
use flui_platform_api::text_store::{
    CommitGate, LockGrant, LockOutcome, LockTiming, TextStore, TextStoreError, TextStoreObserver,
    TextStoreStatus,
};
use flui_view::{Signal, SignalWriteExt};
use flui_widgets::{Focus, SizedBox};

use super::*;

fn headless_text_input() -> (
    Arc<flui_platform::FakeTextInput>,
    Arc<dyn flui_platform::traits::PlatformTextInput>,
) {
    let fake = Arc::new(flui_platform::FakeTextInput::new());
    let capability: Arc<dyn flui_platform::traits::PlatformTextInput> = fake.clone();
    (fake, capability)
}

#[derive(Clone)]
struct PanickingKeyReader {
    signal: Signal<u32>,
    node: Rc<FocusNode>,
}

impl flui_view::View for PanickingKeyReader {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl StatelessView for PanickingKeyReader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let _ = self.signal.get(ctx);
        let signal = self.signal;
        Focus::new(SizedBox::square(10.0))
            .focus_node(Rc::clone(&self.node))
            .on_key_event(move |cx, _event| {
                let _ = signal.update(cx, |value| {
                    *value = 7;
                    panic!("keyboard updater probe");
                });
                KeyEventResult::Handled
            })
    }
}

struct PanickingTextStore;

impl TextStore for PanickingTextStore {
    fn status(&self) -> TextStoreStatus {
        TextStoreStatus::EDITABLE_SINGLE_LINE
    }

    fn request_lock(
        &self,
        _grant: LockGrant,
        _timing: LockTiming,
    ) -> Result<LockOutcome, TextStoreError> {
        panic!("IME store probe");
    }

    fn run_deferred_grants(&self) -> usize {
        0
    }

    fn set_commit_gate(&self, _gate: CommitGate) {}

    fn set_observer(&self, _observer: Option<Rc<dyn TextStoreObserver>>) {}
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

    let down = make_down_event(Offset::new(4.0, 6.0), PointerType::Mouse);
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
    let down_for_bogus = make_down_event(Offset::new(4.0, 6.0), PointerType::Mouse);
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
    let down_for_closed = make_down_event(Offset::new(8.0, 10.0), PointerType::Mouse);
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

#[test]
fn panicking_ime_dispatch_preserves_addressed_redraw_demand() {
    let (_fake_a, capability_a) = headless_text_input();
    let (_fake_b, capability_b) = headless_text_input();
    let mut realm = UiRealm::for_test_with_text_input(Some(capability_a));
    let a_id = realm.presentation_id();
    let window_b = crate::presentation::test_platform_window(Some(capability_b));
    let presentation_b = realm.assemble_presentation(window_b);
    let b_id = realm.install_presentation(presentation_b);
    let store: Rc<dyn TextStore> = Rc::new(PanickingTextStore);
    let _token = realm
        .presentation_text_input_handle_for_test(b_id)
        .attach(flui_interaction::TextInputClient::new(store))
        .expect("B supports text input");
    let _ = realm.take_redraw_request();
    let _ = realm
        .presentations
        .get(b_id)
        .expect("B installed")
        .take_redraw_pending();

    let outcome = catch_unwind(AssertUnwindSafe(|| {
        realm.enter(|realm| {
            realm
                .handle_input_addressed(b_id, PlatformInput::Ime(ImeEvent::Commit("x".to_owned())));
        });
    }));

    let payload = outcome.expect_err("the IME panic must resume");
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"IME store probe"));
    assert!(
        realm
            .presentations
            .get(b_id)
            .expect("B installed")
            .take_redraw_pending(),
        "the addressed presentation retains redraw demand"
    );
    assert!(
        !realm
            .presentations
            .get(a_id)
            .expect("A installed")
            .take_redraw_pending(),
        "the sibling presentation stays untouched"
    );
}

#[test]
fn panicking_keyboard_signal_update_preserves_active_redraw_demand() {
    let mut realm = UiRealm::for_test();
    let a_id = realm.presentation_id();
    let b_id = realm.install_second_presentation_for_test();
    let graph_b = realm
        .presentation_widgets_for_test(b_id)
        .with_build_owner(|owner| owner.reactive().clone());
    let signal = graph_b.signal(1u32);
    let node = FocusNode::with_debug_label("panicking-key-reader");
    realm
        .attach_root_widget_to_for_test(
            b_id,
            &PanickingKeyReader {
                signal,
                node: Rc::clone(&node),
            },
        )
        .expect("B root attaches");
    let mut backend = ScriptedSink::always_presents();
    realm.render_frame(&mut backend);
    realm.notify_presentation_focus_gained(b_id);
    node.request_focus();
    let _ = realm.take_redraw_request();
    let _ = realm
        .presentations
        .get(b_id)
        .expect("B installed")
        .take_redraw_pending();

    let key_event = KeyEventBuilder::new(flui_interaction::events::Code::KeyA).build();
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        realm.enter(|realm| {
            realm.handle_input_addressed(a_id, PlatformInput::Keyboard(key_event));
        });
    }));

    let payload = outcome.expect_err("the keyboard panic must resume");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"keyboard updater probe")
    );
    assert_eq!(signal.peek(&graph_b, |value| *value), Ok(7));
    assert!(
        realm
            .presentations
            .get(b_id)
            .expect("B installed")
            .take_redraw_pending(),
        "the active presentation retains redraw demand"
    );
    assert!(
        !realm
            .presentations
            .get(a_id)
            .expect("A installed")
            .take_redraw_pending(),
        "the stamped sibling stays untouched"
    );
}

#[test]
fn panicking_keyboard_dispatch_keeps_priority_over_a_panicking_wake() {
    let panic_on_wake = Arc::new(AtomicBool::new(false));
    let wake_attempts = Arc::new(AtomicUsize::new(0));
    let wake_gate = Arc::clone(&panic_on_wake);
    let attempts = Arc::clone(&wake_attempts);
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        attempts.fetch_add(1, Ordering::AcqRel);
        assert!(!wake_gate.load(Ordering::Acquire), "wake probe");
    });
    let mut realm = new_runtime(wake).expect("runtime");
    let a_id = realm.presentation_id();
    let b_id = realm.install_second_presentation_for_test();
    let graph_b = realm
        .presentation_widgets_for_test(b_id)
        .with_build_owner(|owner| owner.reactive().clone());
    let signal = graph_b.signal(1u32);
    let node = FocusNode::with_debug_label("panicking-key-reader-with-panicking-wake");
    realm
        .attach_root_widget_to_for_test(
            b_id,
            &PanickingKeyReader {
                signal,
                node: Rc::clone(&node),
            },
        )
        .expect("B root attaches");
    let mut backend = ScriptedSink::always_presents();
    realm.render_frame(&mut backend);
    realm.notify_presentation_focus_gained(b_id);
    node.request_focus();
    let _ = realm.take_redraw_request();
    let _ = realm
        .presentations
        .get(b_id)
        .expect("B installed")
        .take_redraw_pending();
    panic_on_wake.store(true, Ordering::Release);

    let key_event = KeyEventBuilder::new(flui_interaction::events::Code::KeyA).build();
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        realm.enter(|realm| {
            realm.handle_input_addressed(a_id, PlatformInput::Keyboard(key_event));
        });
    }));

    let payload = outcome.expect_err("the original keyboard panic must resume");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"keyboard updater probe"),
        "the secondary wake panic must not replace the dispatch failure"
    );
    assert_eq!(signal.peek(&graph_b, |value| *value), Ok(7));
    assert!(
        realm
            .presentations
            .get(b_id)
            .expect("B installed")
            .take_redraw_pending(),
        "the active presentation retains redraw demand"
    );

    let attempts_after_failure = wake_attempts.load(Ordering::Acquire);
    panic_on_wake.store(false, Ordering::Release);
    assert_eq!(realm.drain_commands(), DrainReport::default());
    assert_eq!(
        wake_attempts.load(Ordering::Acquire),
        attempts_after_failure + 1,
        "the next owner boundary must retry the failed input-redraw wake"
    );
    assert!(realm.needs_redraw(), "the committed redraw demand survives");
    realm.render_frame(&mut backend);
    assert_eq!(signal.peek(&graph_b, |value| *value), Ok(7));
}

#[test]
fn a_panicking_wake_resumes_when_addressed_dispatch_succeeds() {
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(|| panic!("wake probe"));
    let realm = new_runtime(wake).expect("runtime");
    let presentation = realm.presentations.primary();

    let outcome = catch_unwind(AssertUnwindSafe(|| {
        realm.finish_addressed_input_dispatch(presentation, Ok(()));
    }));

    let payload = outcome.expect_err("the wake panic must resume");
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"wake probe"));
    assert!(
        presentation.take_redraw_pending(),
        "redraw demand is durable before the wake runs"
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

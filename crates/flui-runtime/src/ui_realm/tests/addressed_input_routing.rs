use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use flui_foundation::geometry::Offset;
use flui_interaction::events::{PointerType, make_down_event};
use flui_interaction::routing::{FocusNode, KeyEventResult};
use flui_interaction::testing::input::KeyEventBuilder;
use flui_view::{Signal, SignalWriteExt};
use flui_widgets::{Focus, SizedBox};

use super::*;

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
pub(crate) fn input_stamped_for_b_never_reaches_as_arena() {
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

pub(crate) fn panicking_keyboard_dispatch_keeps_priority_over_a_panicking_wake() {
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

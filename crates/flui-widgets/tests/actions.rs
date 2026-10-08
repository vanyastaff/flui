//! [`Actions`] resolution against a mounted tree, driven the way an action is
//! invoked: a key the focused subtree ignored reaches a `Shortcuts`, which
//! maps it to an intent and invokes the nearest enabled action with the key
//! event's `EventCx`. The nearest enabled action wins, a disabled one stops
//! resolution at its own scope, and a lookup with no binding leaves the key
//! unconsumed.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_interaction::events::{Code, Key, KeyEvent, KeyState, Modifiers};
use flui_interaction::routing::FocusNode;
use flui_interaction::testing::input::KeyEventBuilder;
use flui_view::prelude::*;
use flui_widgets::SizedBox;
use flui_widgets::interaction::{
    Actions, CallbackAction, Focus, Intent, Shortcuts, SingleActivator,
};

use crate::common::harness::{Harness, mount};
use crate::common::{ProbeSignals, SignalProbe};

struct AddToCounter(usize);
impl Intent for AddToCounter {}

/// Ctrl+A, the key every test here binds to `AddToCounter`.
fn ctrl_a() -> KeyEvent {
    KeyEventBuilder::new(Code::Unidentified)
        .with_state(KeyState::Down)
        .with_key(Key::character("a"))
        .with_modifiers(Modifiers::CONTROL)
        .build()
}

/// A focusable leaf under a `Shortcuts` that maps Ctrl+A to
/// `AddToCounter(amount)`. Whatever `Actions` wrap it resolve the intent.
fn invoker(amount: usize, field: &Rc<FocusNode>) -> Shortcuts {
    Shortcuts::new(Focus::new(SizedBox::new(1.0, 1.0)).focus_node(Rc::clone(field))).shortcut(
        SingleActivator::character("a").control(),
        AddToCounter(amount),
    )
}

/// Focus `field` and press Ctrl+A. Returns whether the key was consumed —
/// `true` only when an enabled action ran.
fn press(harness: &Harness, field: &Rc<FocusNode>) -> bool {
    let _ = field.request_focus();
    harness
        .focus_manager()
        .dispatch_key_event(&ctrl_a())
        .is_handled()
}

/// Nearest-scope-first with the typed payload delivered: the inner
/// binding shadows the outer, and the intent's field reaches the closure.
///
/// Red-check: resolve from the raw own-map instead of the layered chain —
/// the outer counter moves and the inner assertion flips.
///
/// FLUI has one dispatch path (no replaceable `ActionDispatcher`, ADR-0023
/// deferred).
pub(crate) fn the_nearest_enabled_action_wins_and_receives_the_payload() {
    let outer_sum = Arc::new(AtomicUsize::new(0));
    let inner_sum = Arc::new(AtomicUsize::new(0));
    let field = FocusNode::with_debug_label("nearest-field");

    let outer_counter = Arc::clone(&outer_sum);
    let inner_counter = Arc::clone(&inner_sum);
    let harness = mount(
        Actions::new(Actions::new(invoker(5, &field)).action(CallbackAction::new(
            move |_cx, intent: &AddToCounter| {
                inner_counter.fetch_add(intent.0, Ordering::SeqCst);
            },
        )))
        .action(CallbackAction::new(move |_cx, intent: &AddToCounter| {
            outer_counter.fetch_add(intent.0, Ordering::SeqCst);
        })),
    );

    assert!(
        press(&harness, &field),
        "an enabled action consumed the key"
    );
    assert_eq!(
        inner_sum.load(Ordering::SeqCst),
        5,
        "the nearest action ran, payload intact"
    );
    assert_eq!(
        outer_sum.load(Ordering::SeqCst),
        0,
        "the outer action was shadowed"
    );
}

/// A `CallbackAction` runs inside the key event's dispatch: it writes a
/// signal through the `cx` it receives, and the signal's reader rebuilds.
pub(crate) fn callback_action_writes_through_the_key_events_cx() {
    let field = FocusNode::with_debug_label("writing-field");
    let probe_field = Rc::clone(&field);
    let probe = SignalProbe::new(move |ProbeSignals { count, .. }| {
        Actions::new(invoker(3, &probe_field)).action(CallbackAction::new(
            move |cx, intent: &AddToCounter| count.update(cx, |n| *n += intent.0 as u32),
        ))
    });
    let mut harness = mount(probe.view());

    assert!(press(&harness, &field), "the action consumed the key");
    assert_eq!(probe.value(), Ok(3), "the write landed at dispatch");
    harness.tick();
    assert_eq!(probe.reads(), [0, 3], "and the reader rebuilt once");
}

/// A refused write inside an action is reported at the dispatch boundary,
/// not panicked, and the action still counts as performed.
pub(crate) fn a_refused_write_in_a_callback_action_is_reported_not_panicked() {
    let field = FocusNode::with_debug_label("refused-field");
    let probe_field = Rc::clone(&field);
    let probe = SignalProbe::new(move |ProbeSignals { released, .. }| {
        Actions::new(invoker(1, &probe_field)).action(CallbackAction::new(
            move |cx, _: &AddToCounter| released.set(cx, 1),
        ))
    });
    let mut harness = mount(probe.view());

    let (consumed, log) = flui_testing::log_capture::capture(|| press(&harness, &field));

    assert!(consumed, "the action ran and consumed the key");
    assert!(
        log.contains("an event callback's signal write was refused"),
        "the refusal is logged at the dispatch boundary: {log}"
    );
    harness.tick();
    assert_eq!(probe.value(), Ok(0), "other state is intact");
}

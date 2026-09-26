//! [`Actions`] resolution against a mounted tree: the nearest enabled action
//! wins, a disabled one stops resolution at its own scope, and a lookup with no
//! binding reports `false`.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_widgets::SizedBox;
use flui_widgets::interaction::{Action, ActionOutcome, Actions, CallbackAction, Intent};

use crate::common::harness::mount;

struct AddToCounter(usize);
impl Intent for AddToCounter {}

/// A leaf whose build invokes `intent` through the ambient chain — the
/// only place a `BuildContext` exists.
#[derive(Clone)]
struct InvokeProbe {
    amount: usize,
    ran: Arc<AtomicUsize>,
}

impl View for InvokeProbe {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl StatelessView for InvokeProbe {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        if Actions::maybe_invoke(ctx, &AddToCounter(self.amount)) {
            self.ran.fetch_add(1, Ordering::SeqCst);
        }
        SizedBox::new(1.0, 1.0)
    }
}

/// An action that reports disabled, to prove fall-through.
struct Disabled;
impl Action<AddToCounter> for Disabled {
    fn is_enabled(&self, _intent: &AddToCounter) -> bool {
        false
    }
    fn invoke(&self, _intent: &AddToCounter) -> ActionOutcome {
        unreachable!("BUG: a disabled action must never be invoked (actions.dart:1032-1044)");
    }
}

/// Nearest-scope-first with the typed payload delivered: the inner
/// binding shadows the outer, and the intent's field reaches the closure.
///
/// Red-check: resolve from the raw own-map instead of the layered chain —
/// the outer counter moves and the inner assertion flips.
///
/// Flutter parity (`actions_test.dart`, tag `3.44.0`): covers
/// `'Actions widget can invoke actions with default dispatcher'` and
/// `'Actions widget can invoke actions with default dispatcher and
/// maybeInvoke'` — FLUI has one dispatch path (no replaceable
/// `ActionDispatcher`, ADR-0023 deferred), so both oracle cases collapse
/// onto this one.
#[test]
fn the_nearest_enabled_action_wins_and_receives_the_payload() {
    let ran = Arc::new(AtomicUsize::new(0));
    let outer_sum = Arc::new(AtomicUsize::new(0));
    let inner_sum = Arc::new(AtomicUsize::new(0));

    let outer_counter = Arc::clone(&outer_sum);
    let inner_counter = Arc::clone(&inner_sum);
    let _harness = mount(
        Actions::new(
            Actions::new(InvokeProbe {
                amount: 5,
                ran: Arc::clone(&ran),
            })
            .action(CallbackAction::new(move |intent: &AddToCounter| {
                inner_counter.fetch_add(intent.0, Ordering::SeqCst);
            })),
        )
        .action(CallbackAction::new(move |intent: &AddToCounter| {
            outer_counter.fetch_add(intent.0, Ordering::SeqCst);
        })),
    );

    assert_eq!(ran.load(Ordering::SeqCst), 1, "maybe_invoke reported true");
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

/// Flutter parity (`actions_test.dart`, tag `3.44.0`): stands in for
/// `'CallbackAction passes correct intent when invoked.'`.
#[test]
fn callback_action_accepts_owner_local_rc_state() {
    let ran = Arc::new(AtomicUsize::new(0));
    let total = Rc::new(Cell::new(0));
    let total_for_action = Rc::clone(&total);

    let _harness = mount(
        Actions::new(InvokeProbe {
            amount: 11,
            ran: Arc::clone(&ran),
        })
        .action(CallbackAction::new(move |intent: &AddToCounter| {
            total_for_action.set(total_for_action.get() + intent.0);
        })),
    );

    assert_eq!(ran.load(Ordering::SeqCst), 1, "maybe_invoke ran");
    assert_eq!(total.get(), 11, "owner-local callback captured Rc<Cell<_>>");
}

/// A **disabled** nearer action stops resolution at its own scope — it
/// does *not* fall through to an outer scope's mapping for the same
/// intent type. This is Flutter's actual contract, not the inverse:
/// `Actions.maybeInvoke`'s own doc states "If a suitable Action is found
/// but its `isEnabled` returns false, the search will stop"
/// (`actions.dart:993-995`) — the walk stops at the first scope that
/// *declares* the type at all, whether or not it is enabled, and never
/// reaches the outer action.
///
/// Red-check: merge `own` into the enclosing chain as a fallback list
/// instead of an outright replace (i.e. keep the outer entry reachable
/// once the inner one is checked) — `outer_sum` becomes `7` and `ran`
/// becomes `1`, silently reintroducing the fall-through this test pins
/// against.
#[test]
fn a_disabled_nearer_action_stops_resolution_at_its_own_scope() {
    let ran = Arc::new(AtomicUsize::new(0));
    let outer_sum = Arc::new(AtomicUsize::new(0));

    let outer_counter = Arc::clone(&outer_sum);
    let _harness = mount(
        Actions::new(
            Actions::new(InvokeProbe {
                amount: 7,
                ran: Arc::clone(&ran),
            })
            .action(Disabled),
        )
        .action(CallbackAction::new(move |intent: &AddToCounter| {
            outer_counter.fetch_add(intent.0, Ordering::SeqCst);
        })),
    );

    assert_eq!(
        ran.load(Ordering::SeqCst),
        0,
        "maybe_invoke reported false: the disabled nearer mapping stopped the search"
    );
    assert_eq!(
        outer_sum.load(Ordering::SeqCst),
        0,
        "the outer action was never reached, let alone invoked"
    );
}

/// No binding anywhere: `maybe_invoke` reports `false` and nothing runs.
///
/// Flutter parity (`actions_test.dart`, tag `3.44.0`): stands in for
/// `'maybeInvoke returns null when no action is found'` — FLUI's
/// `maybe_invoke` reports "did anything run" as a `bool` rather than
/// Dart's `Object?`, so "returns null" ports as "returns `false`".
#[test]
fn maybe_invoke_without_a_binding_reports_false() {
    let ran = Arc::new(AtomicUsize::new(0));
    let _harness = mount(InvokeProbe {
        amount: 1,
        ran: Arc::clone(&ran),
    });
    assert_eq!(ran.load(Ordering::SeqCst), 0, "nothing to invoke");
}

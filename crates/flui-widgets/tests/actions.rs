//! [`Actions`] resolution against a mounted tree: the nearest enabled action
//! wins, a disabled one stops resolution at its own scope, and a lookup with no
//! binding reports `false`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_widgets::SizedBox;
use flui_widgets::interaction::{Actions, CallbackAction, Intent};

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

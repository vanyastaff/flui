//! Regression coverage for the `AnimatedBehavior` listenable-instance-swap
//! leak: `Element::update` (`flui-view/src/element/unified.rs`) replaces the
//! view before `on_update` fires, so the old buggy `on_update` read
//! `core.view().listenable()` for BOTH the "unsubscribe old" and "subscribe
//! new" halves — both resolved to the NEW view. The old subscription leaked
//! (its notifier kept notifying a dead rebuild hook), and because
//! `ListenerId`s are assigned from a per-notifier counter, the stale id could
//! collide with and silently detach an unrelated listener already registered
//! on the new listenable.
//!
//! The fix moves the unsubscribe/resubscribe into `on_view_updated`, which is
//! handed the pre-swap `old_view` explicitly, guarded by `Arc::ptr_eq` so a
//! same-instance rebuild (by far the common case) does not
//! unsubscribe/resubscribe at all.
//!
//! `AnimatedBuilder` is exercised directly here (rather than a transition
//! widget) because it takes an arbitrary `Arc<dyn Listenable>`, so a bare
//! `ChangeNotifier` swap is enough to drive the element-level bug without an
//! `AnimationController`/`Vsync` in the loop.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{lay_out, tight};
use flui_foundation::{ChangeNotifier, Listenable, ListenerId};
use flui_widgets::{AnimatedBuilder, SizedBox};

/// Mounting on notifier A subscribes; rebuilding with notifier B unsubscribes
/// from A and subscribes to B — and B's notifications now drive rebuilds
/// while A's no longer do.
///
/// Red-check (run against the pre-fix `on_update`): the
/// `!notifier_a.has_listeners()` assertion below fails, because the buggy
/// code removed a listener id from B (the new view, already swapped in by
/// the time `on_update` ran) instead of from A.
#[test]
fn animated_builder_swap_unsubscribes_old_and_subscribes_new_listenable() {
    let notifier_a = Arc::new(ChangeNotifier::new());
    let notifier_b = Arc::new(ChangeNotifier::new());

    let build_count = Arc::new(AtomicUsize::new(0));

    let listenable_a: Arc<dyn Listenable> = notifier_a.clone();
    let count_for_a = Arc::clone(&build_count);
    let mut laid = lay_out(
        AnimatedBuilder::new(listenable_a, move || {
            count_for_a.fetch_add(1, Ordering::SeqCst);
            SizedBox::new(10.0, 10.0)
        }),
        tight(10.0, 10.0),
    );

    assert!(
        notifier_a.has_listeners(),
        "mount subscribes to the initial listenable"
    );
    assert!(
        !notifier_b.has_listeners(),
        "the second listenable has no subscriber before the swap"
    );

    let listenable_b: Arc<dyn Listenable> = notifier_b.clone();
    let count_for_b = Arc::clone(&build_count);
    laid.pump_widget(AnimatedBuilder::new(listenable_b, move || {
        count_for_b.fetch_add(1, Ordering::SeqCst);
        SizedBox::new(10.0, 10.0)
    }));

    assert!(
        !notifier_a.has_listeners(),
        "the swap must unsubscribe from the old listenable"
    );
    assert!(
        notifier_b.has_listeners(),
        "the swap must subscribe to the new listenable"
    );

    // `tick()` drives a frame WITHOUT marking the root dirty (unlike `pump()`,
    // which always forces a rebuild) — the only way to observe whether a
    // listenable notification itself schedules the rebuild, matching the
    // pattern `fade_transition.rs` uses for the same reason.
    let count_before = build_count.load(Ordering::SeqCst);
    notifier_a.notify_listeners();
    laid.tick();
    assert_eq!(
        build_count.load(Ordering::SeqCst),
        count_before,
        "the old listenable must no longer trigger rebuilds"
    );

    notifier_b.notify_listeners();
    laid.tick();
    assert!(
        build_count.load(Ordering::SeqCst) > count_before,
        "the new listenable must trigger a rebuild"
    );
}

/// The id-collision consequence: an unrelated listener already registered on
/// the *new* listenable, before the swap, must survive the swap.
///
/// Deterministic by construction, not by timing: `ChangeNotifier::next_id`
/// starts at 1 independently per notifier instance, so the unrelated
/// listener registered directly on `notifier_b` below and the
/// `AnimatedBuilder`'s own subscription to `notifier_a` are both assigned id
/// 1 — the exact collision the bug produces once B is swapped in for A.
///
/// Red-check (pre-fix): the buggy `on_update` calls
/// `core.view().listenable().remove_listener(1)`, and `core.view()` at that
/// point is already the new view (B) — so it removes id 1 from B, which is
/// this test's unrelated listener, not the leaked A subscription.
#[test]
fn animated_builder_swap_does_not_detach_a_preexisting_listener_on_the_new_listenable() {
    let notifier_a = Arc::new(ChangeNotifier::new());
    let notifier_b = Arc::new(ChangeNotifier::new());

    let unrelated_fired = Arc::new(AtomicUsize::new(0));
    let unrelated_fired_clone = Arc::clone(&unrelated_fired);
    let unrelated_listener_id = notifier_b.add_listener(Arc::new(move || {
        unrelated_fired_clone.fetch_add(1, Ordering::SeqCst);
    }));
    assert_eq!(
        unrelated_listener_id,
        ListenerId::new(1),
        "setup sanity: the unrelated listener must be B's first (id 1) to \
         collide with A's own first subscription below"
    );

    let listenable_a: Arc<dyn Listenable> = notifier_a.clone();
    let mut laid = lay_out(
        AnimatedBuilder::new(listenable_a, || SizedBox::new(10.0, 10.0)),
        tight(10.0, 10.0),
    );

    let listenable_b: Arc<dyn Listenable> = notifier_b.clone();
    laid.pump_widget(AnimatedBuilder::new(listenable_b, || {
        SizedBox::new(10.0, 10.0)
    }));

    notifier_b.notify_listeners();
    laid.tick();
    assert_eq!(
        unrelated_fired.load(Ordering::SeqCst),
        1,
        "the swap must not detach a listener that predates it on the new listenable"
    );
}

/// A rebuild that passes the SAME listenable instance through (a new
/// `AnimatedBuilder` value wrapping a clone of the same `Arc`) must not
/// unsubscribe/resubscribe at all — the `Arc::ptr_eq` guard in
/// `on_view_updated`.
///
/// Proven without reaching into `AnimatedBehavior` internals: if the guard
/// did not fire, the spurious remove+add cycle would advance
/// `notifier_a`'s internal id counter by one extra step, so a probe listener
/// registered right after the rebuild would get id 3 instead of id 2 (mount
/// takes id 1; a same-instance rebuild that incorrectly resubscribes takes
/// id 2, leaving id 3 for the probe).
#[test]
fn animated_builder_same_instance_rebuild_does_not_resubscribe() {
    let notifier_a = Arc::new(ChangeNotifier::new());
    let listenable_a: Arc<dyn Listenable> = notifier_a.clone();

    let mut laid = lay_out(
        AnimatedBuilder::new(listenable_a.clone(), || SizedBox::new(10.0, 10.0)),
        tight(10.0, 10.0),
    );

    assert!(notifier_a.has_listeners());

    laid.pump_widget(AnimatedBuilder::new(listenable_a.clone(), || {
        SizedBox::new(10.0, 10.0)
    }));

    assert_eq!(
        notifier_a.len(),
        1,
        "same-instance rebuild keeps exactly one subscription"
    );

    let probe_id = notifier_a.add_listener(Arc::new(|| {}));
    assert_eq!(
        probe_id,
        ListenerId::new(2),
        "no unsubscribe/resubscribe cycle ran on a same-instance rebuild"
    );
}

/// A widget swap must tolerate the OLD listenable already having been
/// disposed by its owner before the swap runs — e.g. the user disposes
/// notifier A themselves ahead of pumping the new-listenable frame.
///
/// `on_view_updated` calls `old_listenable.remove_listener(...)` to detach
/// from the pre-swap instance; if A is already disposed by then, that call
/// must be a silent no-op (Flutter parity — `ChangeNotifier.removeListener`
/// carries no `debugAssertNotDisposed`), not a panic. The swap must still
/// complete and subscribe to the new listenable.
#[test]
fn animated_builder_swap_tolerates_a_disposed_old_listenable() {
    let notifier_a = Arc::new(ChangeNotifier::new());
    let notifier_b = Arc::new(ChangeNotifier::new());

    let listenable_a: Arc<dyn Listenable> = notifier_a.clone();
    let mut laid = lay_out(
        AnimatedBuilder::new(listenable_a, || SizedBox::new(10.0, 10.0)),
        tight(10.0, 10.0),
    );

    assert!(notifier_a.has_listeners(), "mount subscribes to A");

    // The owner disposes A before the swap runs.
    notifier_a.dispose();

    let listenable_b: Arc<dyn Listenable> = notifier_b.clone();
    // Must not panic: `on_view_updated`'s `remove_listener` against the
    // now-disposed A is a no-op, and the swap proceeds to subscribe to B.
    laid.pump_widget(AnimatedBuilder::new(listenable_b, || {
        SizedBox::new(10.0, 10.0)
    }));

    assert!(
        notifier_b.has_listeners(),
        "the swap must still subscribe to the new listenable even though \
         detaching from the disposed old one was a no-op"
    );
}

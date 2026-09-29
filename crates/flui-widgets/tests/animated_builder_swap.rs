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

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::common::{lay_out, tight};
use flui_foundation::{ChangeNotifier, Listenable};
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

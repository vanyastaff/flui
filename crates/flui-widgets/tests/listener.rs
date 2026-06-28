//! End-to-end pointer routing: a `Listener` widget's callbacks fire for pointer
//! events that reach it through the real hit-test + dispatch path. Covers the
//! `HitTestBehavior` contract — `DeferToChild` (the default) fires only when a
//! descendant is hit, `Opaque` fires for any pointer within bounds.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{lay_out, size, tight};
use flui_types::Color;
use flui_widgets::prelude::HitTestBehavior;
use flui_widgets::{ColoredBox, Listener, SizedBox};

/// A counter callback + a readable handle.
fn counter() -> (
    Arc<AtomicUsize>,
    impl Fn(&flui_widgets::prelude::PointerEvent) + Send + Sync,
) {
    let count = Arc::new(AtomicUsize::new(0));
    let in_cb = Arc::clone(&count);
    (count, move |_event| {
        in_cb.fetch_add(1, Ordering::SeqCst);
    })
}

#[test]
fn default_listener_fires_on_pointer_landing_on_a_hittable_child() {
    let (downs, on_down) = counter();

    // Default behavior is DeferToChild: a hittable `ColoredBox` child (fills the
    // 100×100 bounds, hit-tests true) lets the listener register and receive.
    let laid = lay_out(
        Listener::new()
            .on_pointer_down(on_down)
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(100.0, 100.0),
    );

    assert_eq!(laid.size(laid.root()), size(100.0, 100.0));

    laid.dispatch_pointer_down(20.0, 20.0);
    assert_eq!(
        downs.load(Ordering::SeqCst),
        1,
        "DeferToChild fires when the pointer lands on a hittable child",
    );
}

#[test]
fn default_listener_does_not_fire_without_a_hittable_target() {
    let (downs, on_down) = counter();

    // DeferToChild + a childless `SizedBox` (RenderConstrainedBox hit-tests
    // false when childless) → nothing under the listener is hit → the listener
    // does NOT register, so the callback never fires.
    let laid = lay_out(
        Listener::new()
            .on_pointer_down(on_down)
            .child(SizedBox::new(100.0, 100.0)),
        tight(100.0, 100.0),
    );

    laid.dispatch_pointer_down(20.0, 20.0);
    assert_eq!(
        downs.load(Ordering::SeqCst),
        0,
        "DeferToChild must NOT fire when no descendant is hit",
    );
}

#[test]
fn opaque_listener_fires_within_bounds_even_without_a_hittable_child() {
    let (downs, on_down) = counter();

    // Opaque registers for any pointer within its own bounds, regardless of
    // whether a child was hit.
    let laid = lay_out(
        Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .on_pointer_down(on_down)
            .child(SizedBox::new(100.0, 100.0)),
        tight(100.0, 100.0),
    );

    laid.dispatch_pointer_down(20.0, 20.0);
    assert_eq!(
        downs.load(Ordering::SeqCst),
        1,
        "Opaque fires for any pointer within bounds",
    );
}

#[test]
fn listener_routes_down_and_up_to_their_own_callbacks() {
    let (downs, on_down) = counter();
    let (ups, on_up) = counter();

    let laid = lay_out(
        Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .on_pointer_down(on_down)
            .on_pointer_up(on_up)
            .child(SizedBox::new(80.0, 80.0)),
        tight(80.0, 80.0),
    );

    laid.dispatch_pointer_down(40.0, 40.0);
    assert_eq!(
        downs.load(Ordering::SeqCst),
        1,
        "down routes to on_pointer_down"
    );
    assert_eq!(
        ups.load(Ordering::SeqCst),
        0,
        "down does not invoke on_pointer_up"
    );

    laid.dispatch_pointer_up(40.0, 40.0);
    assert_eq!(ups.load(Ordering::SeqCst), 1, "up routes to on_pointer_up");
    assert_eq!(
        downs.load(Ordering::SeqCst),
        1,
        "up does not re-invoke on_pointer_down",
    );
}

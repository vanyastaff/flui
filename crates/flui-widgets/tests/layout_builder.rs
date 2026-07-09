//! Public-API tests for [`LayoutBuilder`] (ADR-0017 U4).
//!
//! These drive the widget through the real `flui-widgets` surface and a real
//! `HeadlessBinding` frame — the same path `AppBinding::draw_frame` takes. The
//! `flui-view` unit tests cover the seam's internals; this file covers what an
//! app author can actually observe.
//!
//! # Parity oracles
//!
//! Expected values come from Flutter, not from running the code first:
//! `.flutter/packages/flutter/test/widgets/layout_builder_test.dart`
//! (`'LayoutBuilder parent size'`, `'LayoutBuilder does not crash at zero area'`,
//! `'LayoutBuilder can change size without rebuild'`) and
//! `.flutter/packages/flutter/lib/src/widgets/layout_builder.dart`
//! (`_RenderLayoutBuilder.performLayout`).

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{lay_out, loose};
use flui_rendering::constraints::BoxConstraints;
use flui_types::geometry::px;
use flui_types::{Offset, Size};
use parking_lot::Mutex;

// Exercise the public prelude import path: if `LayoutBuilder` were not exported
// from `flui_widgets::prelude`, this file would not compile.
use flui_widgets::prelude::*;
use flui_widgets::{Center, ConstrainedBox, SizedBox};

/// A builder that records the constraints it was handed.
fn recorder(
    log: Arc<Mutex<Vec<BoxConstraints>>>,
) -> impl Fn(&dyn BuildContext, BoxConstraints) -> SizedBox + Send + Sync + 'static {
    move |_ctx, constraints| {
        log.lock().push(constraints);
        SizedBox::new(
            constraints.max_width.get() / 2.0,
            constraints.max_height.get() / 2.0,
        )
    }
}

/// Flutter's `'LayoutBuilder parent size'` oracle, transcribed:
/// `Center > ConstrainedBox(maxWidth: 100, maxHeight: 200) > LayoutBuilder`,
/// whose builder returns `SizedBox(biggest / 2)`.
///
/// The builder sees the real loose constraints, and the `LayoutBuilder` sizes to
/// `constraints.constrain(child.size)` — 50x100, **not** `biggest` (100x200).
/// The child is laid out on the very first frame; no extra pump.
#[test]
fn layout_builder_receives_real_constraints_and_sizes_to_its_child() {
    let log = Arc::new(Mutex::new(Vec::new()));

    let laid = lay_out(
        Center::new().child(
            ConstrainedBox::new(BoxConstraints::new(px(0.0), px(100.0), px(0.0), px(200.0)))
                .child(LayoutBuilder::new(recorder(Arc::clone(&log)))),
        ),
        loose(400.0),
    );

    assert_eq!(
        log.lock().as_slice(),
        &[BoxConstraints::new(px(0.0), px(100.0), px(0.0), px(200.0))],
        "the builder must be handed the real incoming constraints, exactly once"
    );

    // Find the LayoutBuilder's render node: Center > ConstrainedBox > LayoutBuilder.
    let root = laid.root();
    let constrained = laid.only_child(root);
    let builder_node = laid.only_child(constrained);

    assert_eq!(
        laid.size(builder_node),
        Size::new(px(50.0), px(100.0)),
        "size = constraints.constrain(child.size); not constraints.biggest"
    );
    assert_eq!(
        laid.size(laid.only_child(builder_node)),
        Size::new(px(50.0), px(100.0)),
        "the child returned by the builder is laid out in the SAME frame"
    );
}

/// Flutter's `'LayoutBuilder does not crash at zero area'`: a zero-area box
/// still runs the builder and lays out to `Size::ZERO`.
#[test]
fn layout_builder_does_not_crash_at_zero_area() {
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_for_builder = Arc::clone(&calls);

    let laid = lay_out(
        SizedBox::new(0.0, 0.0).child(LayoutBuilder::new(move |_ctx, _constraints| {
            calls_for_builder.fetch_add(1, Ordering::Relaxed);
            SizedBox::new(10.0, 10.0)
        })),
        loose(400.0),
    );

    assert_eq!(calls.load(Ordering::Relaxed), 1);
    let builder_node = laid.only_child(laid.root());
    assert_eq!(laid.size(builder_node), Size::ZERO);
}

/// Changing the constraints re-invokes the builder and relays the child out —
/// in the frame the change lands, not the one after.
///
/// Loose constraints throughout, so the child's size (`biggest / 2`) actually
/// discriminates: under tight constraints the child would be stretched to fill
/// and the assertion would hold for the wrong reason.
#[test]
fn layout_builder_constraint_change_rebuilds_in_the_same_frame() {
    let log = Arc::new(Mutex::new(Vec::new()));

    let bounds = |w: f32, h: f32| BoxConstraints::new(px(0.0), px(w), px(0.0), px(h));

    let mut laid = lay_out(
        ConstrainedBox::new(bounds(200.0, 100.0))
            .child(LayoutBuilder::new(recorder(Arc::clone(&log)))),
        loose(400.0),
    );
    assert_eq!(log.lock().len(), 1);
    assert_eq!(
        laid.size(laid.only_child(laid.root())),
        Size::new(px(100.0), px(50.0)),
        "first frame: constrain(biggest/2)"
    );

    // A new root imposes different constraints on the same LayoutBuilder.
    laid.pump_widget(
        ConstrainedBox::new(bounds(80.0, 60.0))
            .child(LayoutBuilder::new(recorder(Arc::clone(&log)))),
    );

    let seen = log.lock().clone();
    assert_eq!(
        seen.last().copied(),
        Some(bounds(80.0, 60.0)),
        "the builder must re-run under the NEW constraints"
    );
    // Documented divergence (ADR-0017): `pump_widget` both rebuilds the widget
    // and changes the constraints, so FLUI invokes the builder twice this frame —
    // once in the leading `build_scope` with the last-published constraints, once
    // in the layout<->build fixpoint with the fresh ones. Flutter invokes it once,
    // because its `_LayoutBuilderElement` defers all building to layout. Both
    // paint the same final child; the builder must be a pure function of its
    // inputs. Pinned here so a change to that behavior is deliberate.
    assert_eq!(
        seen.len(),
        3,
        "1 mount + 1 stale-constraints rebuild + 1 fresh (see ADR-0017 divergence)"
    );

    let builder_node = laid.only_child(laid.current_root());
    assert_eq!(
        laid.size(builder_node),
        Size::new(px(40.0), px(30.0)),
        "the builder node follows its rebuilt child, in the same frame"
    );
    assert_eq!(
        laid.size(laid.only_child(builder_node)),
        Size::new(px(40.0), px(30.0)),
        "the rebuilt child (biggest/2) must be laid out in the same frame"
    );
}

/// Flutter's `'LayoutBuilder can change size without rebuild'` direction: frames
/// that neither change the constraints nor rebuild the widget must not re-invoke
/// the builder.
///
/// `tick()` drives a frame without dirtying the root — the analogue of Flutter
/// pumping the *same* `Widget` instance, where `Element.update` is skipped
/// entirely. (`pump()` deliberately marks the root dirty, which rebuilds the
/// `LayoutBuilder` widget and therefore *must* re-invoke the builder: Flutter's
/// `updateShouldRebuild` defaults to `true`. That direction is covered by
/// `layout_builder_new_builder_closure_is_honored`.)
#[test]
fn layout_builder_same_constraints_do_not_reinvoke_the_builder() {
    let log = Arc::new(Mutex::new(Vec::new()));

    let mut laid = lay_out(
        SizedBox::new(200.0, 100.0).child(LayoutBuilder::new(recorder(Arc::clone(&log)))),
        loose(400.0),
    );
    assert_eq!(log.lock().len(), 1);

    laid.tick();
    laid.tick();

    assert_eq!(
        log.lock().len(),
        1,
        "unchanged constraints and no widget update are not a rebuild trigger"
    );
}

/// Flutter's `updateShouldRebuild` defaults to `true`: rebuilding the widget
/// with a **new builder closure** re-invokes it even though the constraints are
/// identical.
#[test]
fn layout_builder_new_builder_closure_is_honored() {
    let bounds = BoxConstraints::new(px(0.0), px(100.0), px(0.0), px(100.0));

    let mut laid = lay_out(
        ConstrainedBox::new(bounds).child(LayoutBuilder::new(|_ctx, _c| SizedBox::new(20.0, 20.0))),
        loose(400.0),
    );
    let builder_node = laid.only_child(laid.root());
    assert_eq!(laid.size(builder_node), Size::new(px(20.0), px(20.0)));

    // Identical constraints, different closure: Flutter's `updateShouldRebuild`
    // default (`true`) means the builder must run again.
    laid.pump_widget(
        ConstrainedBox::new(bounds).child(LayoutBuilder::new(|_ctx, _c| SizedBox::new(40.0, 10.0))),
    );

    let builder_node = laid.only_child(laid.current_root());
    let child = laid.only_child(builder_node);
    assert_eq!(
        laid.size(child),
        Size::new(px(40.0), px(10.0)),
        "the new closure's child must replace the old one"
    );
    assert_eq!(
        laid.size(builder_node),
        Size::new(px(40.0), px(10.0)),
        "the builder node follows its new child"
    );
    assert_eq!(
        laid.offset(child),
        Offset::ZERO,
        "the replacement child sits at the builder's origin"
    );
}

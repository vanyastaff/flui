//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/stack_test.dart` (tag `3.44.0`).
//!
//! Ported cases:
//! - `'Can align non-positioned children (LTR)'` —
//!   [`stack_aligns_non_positioned_children_per_alignment`].
//! - `'Stack sizing: default'` (the sizing invariant, not the `LayoutBuilder`
//!   constraint-string technique) —
//!   [`stack_sizes_to_the_biggest_non_positioned_child`].
//! - `'Stack sizing: explicit'` —
//!   [`stack_fit_variants_constrain_non_positioned_children_differently`].
//! - `RenderStack.layoutPositionedChild` (`stack.dart`), exercised by the
//!   `Positioned(left)`/`Positioned(top)` cases inside `'Alignment with
//!   partially-positioned children'` —
//!   [`positioned_child_left_top_offsets_from_the_stack_origin`].
//! - `'Alignment with partially-positioned children'`'s implicit
//!   paired-edge case (`left` + `right` together tighten the child) —
//!   [`positioned_child_with_all_four_edges_is_over_constrained_and_stretches`].
//! - `Positioned.fill` (`widgets/basic.dart` doc contract: all four edges
//!   pinned to zero) —
//!   [`positioned_fill_pins_the_child_to_every_edge`].
//! - `'Stack clip test'` — geometry half only (paint/clip assertions deferred
//!   to Phase 3, same headless-harness limitation `container_test.rs`
//!   documents) — [`positioned_child_outside_stack_bounds_overflows_geometrically`].
//!
//! Not ported: `Positioned.directional` / `PositionedDirectional` control
//! tests and the RTL half of `'Alignment with partially-positioned
//! children'` — FLUI's `Positioned` has no text-direction-aware
//! `start`/`end` variant (no `Positioned::directional` constructor exists),
//! so there is nothing to port those cases onto. `IndexedStack`-specific
//! cases (visibility/offstage/focus-exclusion under a selected index) are
//! out of scope for this geometry-only pass — `Stack`/`Positioned` are the
//! load-bearing family per the parity plan.
//!
//! Widget → render-object mapping:
//! - `Stack` → `RenderStack` (variable-arity root)
//! - `Positioned` → parent-data only (`StackParentData`, no render object of
//!   its own)
//!
//! Divergence: none identified beyond the paint/clip deferral noted above —
//! `RenderStack`'s sizing (`compute_size`) and positioning
//! (`PositionedSpec::child_offset`/`child_constraints`) are direct,
//! behavior-faithful ports of `stack.dart`'s `_computeSize` and
//! `layoutPositionedChild`.

use flui_rendering::constraints::BoxConstraints;
use flui_types::geometry::px;
use flui_view::ViewExt;
use flui_widgets::prelude::*;

use crate::common::{loose, offset, size, tight};
use crate::harness;

/// Non-positioned children are aligned per [`Stack::alignment`] within the
/// stack's box — the biggest child anchors the size, smaller ones shift
/// toward the alignment point.
///
/// Flutter parity: `'Can align non-positioned children (LTR)'` —
/// `Alignment::CENTER` centers the 10×10 child inside the 20×20 stack
/// (offset (5, 5)); `Alignment::BOTTOM_RIGHT` (Flutter's
/// `AlignmentDirectional.bottomEnd` under LTR) pushes it flush to the
/// bottom-right corner (offset (10, 10)). The bigger 20×20 child stays at
/// (0, 0) — it has no free space to move within — in both cases.
#[test]
fn stack_aligns_non_positioned_children_per_alignment() {
    let centered = harness::pump_widget(
        Stack::new(vec![
            SizedBox::new(20.0, 20.0).boxed(),
            SizedBox::new(10.0, 10.0).boxed(),
        ])
        .alignment(Alignment::CENTER),
        loose(1000.0),
    );
    let root = centered.root();
    assert_eq!(centered.offset(centered.child(root, 0)), offset(0.0, 0.0));
    assert_eq!(centered.offset(centered.child(root, 1)), offset(5.0, 5.0));

    let bottom_right = harness::pump_widget(
        Stack::new(vec![
            SizedBox::new(20.0, 20.0).boxed(),
            SizedBox::new(10.0, 10.0).boxed(),
        ])
        .alignment(Alignment::BOTTOM_RIGHT),
        loose(1000.0),
    );
    let root = bottom_right.root();
    assert_eq!(
        bottom_right.offset(bottom_right.child(root, 0)),
        offset(0.0, 0.0)
    );
    assert_eq!(
        bottom_right.offset(bottom_right.child(root, 1)),
        offset(10.0, 10.0)
    );
}

/// A `Stack` with only non-positioned children sizes itself to the biggest
/// child on each axis independently (not the biggest child overall).
///
/// Flutter parity: `'Stack sizing: default'` establishes that a `Stack`
/// under loose constraints shrink-wraps its non-positioned content
/// (`RenderStack._computeSize`, `stack.dart`); this port checks the
/// resulting geometry directly instead of Flutter's `LayoutBuilder`
/// constraint-string capture (FLUI has a `LayoutBuilder` widget too, but a
/// direct size assertion is the more faithful port of the underlying
/// invariant: max-per-axis, not max-of-either-child).
#[test]
fn stack_sizes_to_the_biggest_non_positioned_child() {
    let laid = harness::pump_widget(
        Stack::new(vec![
            SizedBox::new(100.0, 50.0).boxed(),
            SizedBox::new(60.0, 80.0).boxed(),
        ]),
        loose(1000.0),
    );

    assert_eq!(
        laid.size(laid.root()),
        size(100.0, 80.0),
        "Stack size must be (max width, max height) taken independently per axis, \
         not either child's own size"
    );
}

/// [`StackFit`] governs the constraints given to non-positioned children:
/// `Loose` loosens, `Expand` tightens to the biggest incoming size,
/// `Passthrough` forwards the incoming constraints unchanged.
///
/// Flutter parity: `'Stack sizing: explicit'` pumps all three `StackFit`
/// variants and asserts the constraint string a `LayoutBuilder` child
/// observes. This port asserts the same three `non_positioned_constraints`
/// outcomes (`RenderStack::non_positioned_constraints`, `stack.dart`) through
/// a `SizedBox::shrink()` child's resulting size instead: under a bounded,
/// non-tight incoming box (min 50, max 200 on both axes),
/// `Loose` lets the 0×0-preferring child collapse to (0, 0), `Expand` forces
/// it up to the biggest incoming size (200, 200), and `Passthrough` clamps it
/// up to the incoming minimum (50, 50).
#[test]
fn stack_fit_variants_constrain_non_positioned_children_differently() {
    let bounded_not_tight = BoxConstraints::new(px(50.0), px(200.0), px(50.0), px(200.0));

    let loose_fit = harness::pump_widget(
        Stack::new(vec![SizedBox::shrink().boxed()]).fit(StackFit::Loose),
        bounded_not_tight,
    );
    assert_eq!(
        loose_fit.size(loose_fit.only_child(loose_fit.root())),
        size(0.0, 0.0),
        "StackFit::Loose must let a shrink-preferring child collapse to 0×0"
    );

    let expand_fit = harness::pump_widget(
        Stack::new(vec![SizedBox::shrink().boxed()]).fit(StackFit::Expand),
        bounded_not_tight,
    );
    assert_eq!(
        expand_fit.size(expand_fit.only_child(expand_fit.root())),
        size(200.0, 200.0),
        "StackFit::Expand must force the child up to the biggest incoming size"
    );

    let passthrough_fit = harness::pump_widget(
        Stack::new(vec![SizedBox::shrink().boxed()]).fit(StackFit::Passthrough),
        bounded_not_tight,
    );
    assert_eq!(
        passthrough_fit.size(passthrough_fit.only_child(passthrough_fit.root())),
        size(50.0, 50.0),
        "StackFit::Passthrough must forward the incoming constraints unchanged, \
         clamping the shrink-preferring child up to the incoming minimum (50)"
    );
}

/// A `Positioned` child with only `left`/`top` set keeps its own natural
/// size and is offset directly by those edge distances from the stack's
/// origin; it does not contribute to the stack's size.
///
/// Flutter parity: `RenderStack.layoutPositionedChild` (`stack.dart`), the
/// same edge-offset math `'Alignment with partially-positioned children'`
/// exercises for its `Positioned(left: 0.0, ...)` / `Positioned(top: 0.0,
/// ...)` cases — ported here with non-zero edges to also prove the distance
/// (not just presence) is honored.
#[test]
fn positioned_child_left_top_offsets_from_the_stack_origin() {
    let laid = harness::pump_widget(
        Stack::new(vec![
            Positioned::new(SizedBox::new(30.0, 40.0))
                .left(10.0)
                .top(20.0)
                .boxed(),
        ]),
        loose(200.0),
    );

    let root = laid.root();
    assert_eq!(
        laid.size(root),
        size(200.0, 200.0),
        "a Stack with only a positioned child sizes to the incoming biggest size"
    );
    let positioned = laid.only_child(root);
    assert_eq!(
        laid.size(positioned),
        size(30.0, 40.0),
        "an unpaired left/top Positioned child keeps its own natural size"
    );
    assert_eq!(laid.offset(positioned), offset(10.0, 20.0));
}

/// A `Positioned` child with all four edges set is over-constrained: the
/// paired edges tighten its size to fill the remaining gap, overriding
/// whatever size the child itself would have preferred.
///
/// Flutter parity: the paired-edge branch of `PositionedSpec::child_constraints`
/// / `RenderStack.layoutPositionedChild` (`stack.dart`) — the same mechanism
/// `'Alignment with partially-positioned children'` exercises one edge at a
/// time; this case sets all four at once (left=10, top=5, right=15,
/// bottom=25) on a naturally 0×0-preferring child to prove the edges win over
/// the child's own size preference, inside a Stack whose 200×200 size comes
/// from the incoming bound (no other non-positioned sibling).
#[test]
fn positioned_child_with_all_four_edges_is_over_constrained_and_stretches() {
    let laid = harness::pump_widget(
        Stack::new(vec![
            Positioned::new(SizedBox::shrink())
                .left(10.0)
                .top(5.0)
                .right(15.0)
                .bottom(25.0)
                .boxed(),
        ]),
        loose(200.0),
    );

    let root = laid.root();
    assert_eq!(laid.size(root), size(200.0, 200.0));
    let positioned = laid.only_child(root);
    assert_eq!(
        laid.size(positioned),
        size(175.0, 170.0),
        "left+right (10+15) and top+bottom (5+25) must tighten the shrink-preferring \
         child to fill the remaining 200-10-15=175 × 200-5-25=170 gap"
    );
    assert_eq!(laid.offset(positioned), offset(10.0, 5.0));
}

/// `Positioned::fill` pins all four edges to zero, so the child fills the
/// stack exactly regardless of its own size preference.
///
/// Flutter parity: `Positioned.fill`'s documented contract in
/// `widgets/basic.dart` (`left = top = right = bottom = 0.0`) — the same
/// all-edges-set mechanism as
/// [`positioned_child_with_all_four_edges_is_over_constrained_and_stretches`],
/// specialized to the zero-inset convenience constructor.
#[test]
fn positioned_fill_pins_the_child_to_every_edge() {
    let laid = harness::pump_widget(
        Stack::new(vec![Positioned::fill(SizedBox::shrink()).boxed()]),
        tight(150.0, 100.0),
    );

    let root = laid.root();
    assert_eq!(laid.size(root), size(150.0, 100.0));
    let positioned = laid.only_child(root);
    assert_eq!(
        laid.size(positioned),
        size(150.0, 100.0),
        "Positioned::fill must stretch its shrink-preferring child to the full stack size"
    );
    assert_eq!(laid.offset(positioned), offset(0.0, 0.0));
}

/// A `Positioned` child larger than (and offset past) the stack's own
/// content size overflows the stack's bounds — the geometry `RenderStack`
/// flags via `has_visual_overflow` and clips on paint.
///
/// Flutter parity: `'Stack clip test'` — a `Positioned` child bigger than the
/// stack triggers the clip path (`pushClipRect`) vs. the no-clip path
/// (`paintChild`) under `Clip.none`. This headless harness has no paint
/// backend (see `container_test.rs`'s note on the same limitation), so only
/// the geometry half is ported: the positioned child's offset + size must
/// exceed the stack's own (non-positioned-content-derived) size on both axes
/// — the exact condition `RenderStack::child_overflows` (`stack.rs`) tests to
/// set the flag paint later reads.
#[test]
fn positioned_child_outside_stack_bounds_overflows_geometrically() {
    let laid = harness::pump_widget(
        Stack::new(vec![
            SizedBox::new(50.0, 50.0).boxed(),
            Positioned::new(SizedBox::new(60.0, 60.0))
                .left(30.0)
                .top(30.0)
                .boxed(),
        ]),
        loose(1000.0),
    );

    let root = laid.root();
    let stack_size = laid.size(root);
    assert_eq!(
        stack_size,
        size(50.0, 50.0),
        "the stack's size comes only from the non-positioned 50×50 child"
    );

    let positioned = laid.child(root, 1);
    let child_offset = laid.offset(positioned);
    let child_size = laid.size(positioned);
    assert!(
        (child_offset.dx + child_size.width).get() > stack_size.width.get()
            && (child_offset.dy + child_size.height).get() > stack_size.height.get(),
        "a Positioned child at (30, 30) sized 60×60 must extend past the 50×50 \
         stack bounds on both axes: got offset {child_offset:?}, size {child_size:?}, \
         stack size {stack_size:?}"
    );
}

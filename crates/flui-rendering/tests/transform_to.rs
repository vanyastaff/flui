//! ADR-0021: `PipelineOwner::{transform_to, local_to_global, global_to_local}`.
//!
//! # Parity oracles
//!
//! `.flutter/packages/flutter/lib/src/rendering/object.dart:3686`
//! (`RenderObject.getTransformTo`), `:3639` (`applyPaintTransform`);
//! `.../rendering/box.dart:3014` (`RenderBox.applyPaintTransform`), `:3062`
//! (`globalToLocal`), `:3113` (`localToGlobal`). Expected values are read from the
//! reference, not from running this code.
//!
//! The render objects here are local fixtures: `flui-rendering` cannot depend on
//! `flui-objects`, where the real transforming objects live. Those get their own
//! coverage in `flui-objects/tests/render_object_harness.rs`.

#![cfg(feature = "testing")]

use flui_foundation::geometry::{Matrix4, Offset, Point, Size};
use flui_foundation::{Leaf, Single};
use flui_rendering::prelude::*;
use flui_rendering::testing::{Probe, RenderTester, box_node};
use flui_rendering::traits::PaintEffects;

/// A leaf of fixed size.
#[derive(Debug, Default)]
struct FixedBox;
impl flui_foundation::Diagnosticable for FixedBox {}
impl RenderBox for FixedBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;
    fn perform_layout(&mut self, _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        Size::new(20.0, 20.0)
    }
    fn paint(&self, _ctx: &mut PaintCx<'_, Leaf>) {}
}

/// A single-child box that positions its child at a fixed offset — the plain
/// case, where `apply_paint_transform`'s default (translate by the child's
/// committed offset) is the whole story.
#[derive(Debug)]
struct OffsetBox(Offset);
impl flui_foundation::Diagnosticable for OffsetBox {}
impl RenderBox for OffsetBox {
    type Arity = Single;
    type ParentData = BoxParentData;
    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        ctx.layout_child(0, constraints.loosen());
        ctx.position_child(0, self.0);
        constraints.biggest()
    }
    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        ctx.paint_child();
    }
}

/// A single-child box whose `paint_effects` reports a `transform`, like
/// `RenderTransform`. It positions its child at the origin, so the default
/// composition (`paint_effects`'s `transform` field · translate(0)) reduces
/// to the matrix itself.
#[derive(Debug)]
struct MatrixBox(Matrix4);
impl flui_foundation::Diagnosticable for MatrixBox {}
impl RenderBox for MatrixBox {
    type Arity = Single;
    type ParentData = BoxParentData;
    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        ctx.layout_child(0, constraints.loosen());
        ctx.position_child(0, Offset::ZERO);
        constraints.biggest()
    }
    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        ctx.paint_child();
    }
    fn paint_effects(&self, _size: Size) -> PaintEffects {
        PaintEffects::NONE.with_transform(self.0)
    }
}

fn tight(w: f64, h: f64) -> BoxConstraints {
    BoxConstraints::tight(Size::new(w, h))
}

fn point(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

fn assert_point_eq(actual: Point, expected: Point) {
    assert!(
        (actual.x - expected.x).abs() < 1e-4 && (actual.y - expected.y).abs() < 1e-4,
        "expected {expected:?}, got {actual:?}"
    );
}

/// `RenderBox.applyPaintTransform` translates by the child's committed offset
/// (`box.dart:3014`), and `getTransformTo` composes one step per level
/// (`object.dart:3728-3731`). Two nested offsets must add.
#[test]
fn transform_to_accumulates_offsets_through_a_plain_chain() {
    let run = RenderTester::mount(
        box_node(OffsetBox(Offset::new(10.0, 5.0)))
            .label("outer")
            .child(
                box_node(OffsetBox(Offset::new(3.0, 7.0)))
                    .label("inner")
                    .child(box_node(FixedBox).label("leaf")),
            ),
    )
    .with_constraints(tight(200.0, 200.0))
    .run_layout();

    let owner = run.owner();
    let (outer, leaf) = (run.id("outer"), run.id("leaf"));

    let transform = owner
        .transform_to(leaf, outer)
        .expect("leaf is a descendant of outer");

    // The leaf's local origin sits at (10+3, 5+7) in `outer`'s space.
    let (x, y) = transform.transform_point(0.0, 0.0);
    assert_point_eq(Point::new(x, y), point(13.0, 12.0));

    // And a point inside the leaf shifts by the same amount.
    let (x, y) = transform.transform_point(2.0, 1.0);
    assert_point_eq(Point::new(x, y), point(15.0, 13.0));
}

/// An ancestor whose `paint_effects` reports a `transform` contributes it —
/// this is the case a naive offset-only accumulation gets silently wrong, and
/// the reason ADR-0021 exists.
#[test]
fn transform_to_respects_a_render_transform_ancestor() {
    let scale = Matrix4::scaling(2.0, 3.0, 1.0);
    let run = RenderTester::mount(
        box_node(MatrixBox(scale)).label("scaler").child(
            box_node(OffsetBox(Offset::new(4.0, 6.0)))
                .label("offset")
                .child(box_node(FixedBox).label("leaf")),
        ),
    )
    .with_constraints(tight(100.0, 100.0))
    .run_layout();

    let owner = run.owner();
    let transform = owner
        .transform_to(run.id("leaf"), run.id("scaler"))
        .expect("descendant");

    // Local (1, 1) in the leaf → (4+1, 6+1) under `offset` → scaled by (2, 3).
    let (x, y) = transform.transform_point(1.0, 1.0);
    assert_point_eq(Point::new(x, y), point(10.0, 21.0));
}

/// `None` means "the question was malformed", not "no transform". A sibling is
/// not an ancestor; Flutter throws `'$target and $this are not in the same render
/// tree.'` (`object.dart:3708`) once the walk falls off the root.
#[test]
fn transform_to_returns_none_when_ancestor_is_not_an_ancestor() {
    let run = RenderTester::mount(
        box_node(OffsetBox(Offset::new(1.0, 1.0)))
            .label("root")
            .child(
                box_node(OffsetBox(Offset::ZERO)).label("branch").child(
                    box_node(OffsetBox(Offset::ZERO))
                        .label("mid")
                        .child(box_node(FixedBox).label("leaf")),
                ),
            ),
    )
    .with_constraints(tight(100.0, 100.0))
    .run_layout();

    let owner = run.owner();
    let (root, branch, leaf) = (run.id("root"), run.id("branch"), run.id("leaf"));

    // Descendant → ancestor works; the reverse is not an ancestor walk.
    assert!(owner.transform_to(leaf, root).is_some());
    assert_eq!(
        owner.transform_to(root, leaf),
        None,
        "an ancestor is not a descendant of its own child"
    );
    assert_eq!(
        owner.transform_to(branch, leaf),
        None,
        "walking up from `branch` never reaches `leaf`"
    );
}

/// `globalToLocal` is `getTransformTo(ancestor)` inverted (`box.dart:3077-3081`).
/// Every transform any FLUI render object produces is affine 2-D, so the plain
/// inverse is exact and the round trip is lossless.
#[test]
fn global_to_local_inverts_local_to_global() {
    let run = RenderTester::mount(
        box_node(MatrixBox(Matrix4::scaling(2.0, 4.0, 1.0)))
            .label("root")
            .child(
                box_node(OffsetBox(Offset::new(7.0, 11.0)))
                    .label("mid")
                    .child(box_node(FixedBox).label("leaf")),
            ),
    )
    .with_constraints(tight(100.0, 100.0))
    .run_layout();

    let owner = run.owner();
    let leaf = run.id("leaf");

    for local in [point(0.0, 0.0), point(3.0, 5.0), point(-2.0, 9.5)] {
        let global = owner
            .local_to_global(leaf, local, None)
            .expect("descendant of the root");
        let back = owner
            .global_to_local(leaf, global, None)
            .expect("an invertible affine transform");
        assert_point_eq(back, local);
    }
}

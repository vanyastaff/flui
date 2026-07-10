//! ADR-0021 U1: `PipelineOwner::{transform_to, local_to_global, global_to_local}`.
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

use flui_rendering::prelude::*;
use flui_rendering::testing::{Probe, RenderTester, box_node};
use flui_tree::{Leaf, Single};
use flui_types::{Matrix4, Offset, Point, Size, geometry::px};

/// A leaf of fixed size.
#[derive(Debug, Default)]
struct FixedBox;
impl flui_foundation::Diagnosticable for FixedBox {}
impl RenderBox for FixedBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;
    fn perform_layout(&mut self, _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        Size::new(px(20.0), px(20.0))
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

/// A single-child box that reports a `paint_transform`, like `RenderTransform`.
/// It positions its child at the origin, so the default composition
/// (`paint_transform · translate(0)`) reduces to the matrix itself.
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
    fn paint_transform(&self, _size: Size) -> Option<Matrix4> {
        Some(self.0)
    }
}

fn tight(w: f32, h: f32) -> BoxConstraints {
    BoxConstraints::tight(Size::new(px(w), px(h)))
}

fn point(x: f32, y: f32) -> Point {
    Point::new(px(x), px(y))
}

fn assert_point_eq(actual: Point, expected: Point) {
    assert!(
        (actual.x.0 - expected.x.0).abs() < 1e-4 && (actual.y.0 - expected.y.0).abs() < 1e-4,
        "expected {expected:?}, got {actual:?}"
    );
}

/// `RenderBox.applyPaintTransform` translates by the child's committed offset
/// (`box.dart:3014`), and `getTransformTo` composes one step per level
/// (`object.dart:3728-3731`). Two nested offsets must add.
#[test]
fn transform_to_accumulates_offsets_through_a_plain_chain() {
    let run = RenderTester::mount(
        box_node(OffsetBox(Offset::new(px(10.0), px(5.0))))
            .label("outer")
            .child(
                box_node(OffsetBox(Offset::new(px(3.0), px(7.0))))
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
    let (x, y) = transform.transform_point(px(0.0), px(0.0));
    assert_point_eq(Point::new(x, y), point(13.0, 12.0));

    // And a point inside the leaf shifts by the same amount.
    let (x, y) = transform.transform_point(px(2.0), px(1.0));
    assert_point_eq(Point::new(x, y), point(15.0, 13.0));
}

/// A node's own space maps to itself. Flutter returns `Matrix4.identity()` when
/// `from` and `to` are the same object (`object.dart:3701`, the loop never runs).
#[test]
fn transform_to_is_identity_for_a_node_and_itself() {
    let run = RenderTester::mount(box_node(FixedBox).label("root"))
        .with_constraints(tight(20.0, 20.0))
        .run_layout();

    let root = run.id("root");
    let transform = run.owner().transform_to(root, root).expect("same node");
    assert!(
        transform.is_identity(),
        "expected identity, got {transform:?}"
    );
}

/// An ancestor that reports a `paint_transform` contributes it — this is the
/// case a naive offset-only accumulation gets silently wrong, and the reason
/// ADR-0021 §3 S4 exists.
#[test]
fn transform_to_respects_a_render_transform_ancestor() {
    let scale = Matrix4::scaling(2.0, 3.0, 1.0);
    let run = RenderTester::mount(
        box_node(MatrixBox(scale)).label("scaler").child(
            box_node(OffsetBox(Offset::new(px(4.0), px(6.0))))
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
    let (x, y) = transform.transform_point(px(1.0), px(1.0));
    assert_point_eq(Point::new(x, y), point(10.0, 21.0));
}

/// The step order is outermost-first: `T = A · B · … · P` (`object.dart:3729`).
/// Scaling *then* offsetting is not the same as offsetting *then* scaling, so a
/// reversed composition would show up here.
#[test]
fn transform_to_composes_ancestor_before_descendant() {
    let scale = Matrix4::scaling(2.0, 2.0, 1.0);

    let scale_outside = RenderTester::mount(
        box_node(MatrixBox(scale)).label("top").child(
            box_node(OffsetBox(Offset::new(px(5.0), px(0.0))))
                .label("mid")
                .child(box_node(FixedBox).label("leaf")),
        ),
    )
    .with_constraints(tight(100.0, 100.0))
    .run_layout();

    let scale_inside = RenderTester::mount(
        box_node(OffsetBox(Offset::new(px(5.0), px(0.0))))
            .label("top")
            .child(
                box_node(MatrixBox(scale))
                    .label("mid")
                    .child(box_node(FixedBox).label("leaf")),
            ),
    )
    .with_constraints(tight(100.0, 100.0))
    .run_layout();

    let outside = scale_outside
        .owner()
        .transform_to(scale_outside.id("leaf"), scale_outside.id("top"))
        .expect("descendant");
    let inside = scale_inside
        .owner()
        .transform_to(scale_inside.id("leaf"), scale_inside.id("top"))
        .expect("descendant");

    // scale(offset(0)) = 2 * 5 = 10;  offset(scale(0)) = 5 + 0 = 5.
    let (x, _) = outside.transform_point(px(0.0), px(0.0));
    assert!((x.0 - 10.0).abs() < 1e-4, "scale outside: got {x:?}");
    let (x, _) = inside.transform_point(px(0.0), px(0.0));
    assert!((x.0 - 5.0).abs() < 1e-4, "scale inside: got {x:?}");
}

/// `None` means "the question was malformed", not "no transform". A sibling is
/// not an ancestor; Flutter throws `'$target and $this are not in the same render
/// tree.'` (`object.dart:3708`) once the walk falls off the root.
#[test]
fn transform_to_returns_none_when_ancestor_is_not_an_ancestor() {
    let run = RenderTester::mount(
        box_node(OffsetBox(Offset::new(px(1.0), px(1.0))))
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

/// `RenderBox::local_to_global` and `global_to_local` were identity stubs on the
/// trait (`render_box.rs:192-199`) and could not be anything else — a FLUI render
/// object has no parent link. ADR-0021 U1 moved them to the pipeline, which owns
/// the tree, and made them real.
#[test]
fn local_to_global_is_no_longer_identity() {
    let run = RenderTester::mount(
        box_node(OffsetBox(Offset::new(px(30.0), px(40.0))))
            .label("root")
            .child(box_node(FixedBox).label("leaf")),
    )
    .with_constraints(tight(100.0, 100.0))
    .run_layout();

    let owner = run.owner();
    let leaf = run.id("leaf");

    let global = owner
        .local_to_global(leaf, point(0.0, 0.0), None)
        .expect("leaf converts against the render root");
    assert_point_eq(global, point(30.0, 40.0));
    assert_ne!(
        global,
        point(0.0, 0.0),
        "the identity stub would return the input"
    );

    // An explicit ancestor is the same walk, stopped early.
    let root = run.id("root");
    assert_point_eq(
        owner
            .local_to_global(leaf, point(2.0, 3.0), Some(root))
            .expect("descendant"),
        point(32.0, 43.0),
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
                box_node(OffsetBox(Offset::new(px(7.0), px(11.0))))
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

/// A singular transform — a zero scale, which a `FittedBox` with an empty child
/// produces — maps every local point onto one global point and cannot be
/// inverted. Flutter returns `Offset.zero` (`box.dart:3079-3081`); FLUI returns
/// `None` rather than inventing an answer.
#[test]
fn global_to_local_returns_none_for_a_singular_transform() {
    let run = RenderTester::mount(
        box_node(MatrixBox(Matrix4::scaling(0.0, 0.0, 1.0)))
            .label("root")
            .child(box_node(FixedBox).label("leaf")),
    )
    .with_constraints(tight(100.0, 100.0))
    .run_layout();

    let owner = run.owner();
    let leaf = run.id("leaf");

    assert!(
        owner.local_to_global(leaf, point(5.0, 5.0), None).is_some(),
        "the forward direction is still well defined"
    );
    assert_eq!(owner.global_to_local(leaf, point(0.0, 0.0), None), None);
}

/// The production `box_size` reader — `flui_rendering::testing::box_geometry` is
/// behind the `testing` feature and unreachable from shipped code.
#[test]
fn box_size_reads_committed_geometry() {
    let run = RenderTester::mount(
        box_node(OffsetBox(Offset::ZERO))
            .label("root")
            .child(box_node(FixedBox).label("leaf")),
    )
    .with_constraints(tight(100.0, 60.0))
    .run_layout();

    let owner = run.owner();
    assert_eq!(
        owner.box_size(run.id("root")),
        Some(Size::new(px(100.0), px(60.0)))
    );
    assert_eq!(
        owner.box_size(run.id("leaf")),
        Some(Size::new(px(20.0), px(20.0)))
    );
}

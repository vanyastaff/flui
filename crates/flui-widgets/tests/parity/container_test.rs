//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/container_test.dart` (tag
//! `3.44.0`, 18 `testWidgets` cases; 17 are `Container`-subject, 1
//! (`'DecoratedBox does not crash at zero area'`) is `DecoratedBox`-subject
//! and out of scope for this file — not yet ported to `tests/decorated_box.rs`
//! either, which currently has no zero-area case).
//!
//! `Container` is not a single render object: `StatelessView::build`
//! (`crates/flui-widgets/src/container.rs`) composes, from the child
//! outward, `Align` → `Padding` → `ColoredBox`/`DecoratedBox` →
//! `ConstrainedBox` → `Padding` (margin) → `Transform` — the same order as
//! Flutter's `Container.build`. The cases below exercise that composition
//! order and the child/no-child sizing contract; paint-color values are out
//! of scope (this crate's test harness has no display-list/paint-command
//! introspection, same limitation `tests/decorated_box.rs` and
//! `tests/clip.rs` already document).
//!
//! Ported (6 upstream names, 10 Rust tests — 8 new plus the 2 pre-existing):
//! - `'paints as expected'` — the layout geometry implicit in the
//!   `group('Container control tests:', ...)` setup (alignment + padding +
//!   color + width/height + constraints-clamp + margin, composed together) —
//!   [`container_padding_enlarges_to_wrap_child`],
//!   [`container_alignment_bottom_right_positions_child_correctly`] (both
//!   pre-existing) and
//!   [`container_full_composition_clamps_size_and_positions_child`] (new: the
//!   full combined chain, matching the oracle's exact rect positions).
//!   `foregroundDecoration` is dropped — `Container` has no such field (see
//!   `docs/ROADMAP.md` Cross.H).
//! - `'Can be placed in an infinite box'` —
//!   [`container_collapses_to_zero_in_the_unbounded_dimension_when_childless`].
//! - `'Container transformAlignment'` — partial: ported only as the box-geometry invariant
//!   the oracle's `getSize`/`getTopLeft`/`getTopRight`/`getBottomLeft`/
//!   `getBottomRight` asserts really pin (a `Transform`-wrapped `Container`'s
//!   own laid-out box is unaffected by its transform; the transform only
//!   affects painting/hit-testing of what's *inside* it) —
//!   [`container_transform_is_outermost_and_does_not_affect_own_box_size`].
//!   `transformAlignment` itself is dropped — `Container` has no such
//!   setter (see `docs/ROADMAP.md` Cross.H).
//! - `'Container is hittable only when having decorations'` — the `color`,
//!   `decoration`, and "everything but color or decoration" legs —
//!   [`container_with_color_is_hittable`],
//!   [`container_with_decoration_is_hittable`],
//!   [`container_with_no_color_or_decoration_is_not_hittable`]. The
//!   `foregroundDecoration` leg is dropped (same missing field as above).
//! - `'Container discards alignment when the child parameter is null and
//!   constraints is not Tight'` — ported as an `#[ignore]`d oracle-expectation
//!   test: FLUI's `Container::build` wraps `alignment` unconditionally
//!   whenever it is set, where Flutter's `if (child == null && ...) { … }
//!   else if (alignment != null) { current = Align(...) }` only applies
//!   `Align` when the childless-placeholder branch was *not* taken — a real
//!   divergence, filed as a `docs/ROADMAP.md` Cross.H known gap —
//!   [`container_discards_alignment_when_childless_and_constraints_not_tight`].
//! - `'Container does not crash at zero area'` —
//!   [`container_does_not_crash_at_zero_area`].
//!
//! Out of scope (11 upstream names):
//! - `'has reasonable/expected info/debug/fine/hidden diagnostics'` (5 cases)
//!   and `'painting error has expected diagnostics'` — no
//!   `DiagnosticPropertiesBuilder`-string-matching harness exists for
//!   `flui-widgets` tests.
//! - `'giving clipBehaviour Clip.None, will not add a ClipPath to the tree'`,
//!   `'giving clipBehaviour not a Clip.None, will add a ClipPath to the
//!   tree'`, `'getClipPath() works for lots of kinds of decorations'`,
//!   `'using clipBehaviour and shadow, should not clip the shadow'` —
//!   `Container` has no `clip_behavior` field at all (`docs/ROADMAP.md`
//!   Cross.H); the latter two are also golden-pixel tests this crate's
//!   harness cannot run.
//! - `'Container with BorderRadiusDirectional and no Directionality throws a
//!   detailed error'` — `flui_types::styling::BoxDecoration::border_radius`
//!   is `Option<BorderRadius>` only; there is no path to hand it a
//!   `BorderRadiusDirectional` to trigger the error at all
//!   (`docs/ROADMAP.md` Cross.H).

use crate::common::{lay_out, loose, offset, size, tight};
use flui_geometry::EdgeInsets;
use flui_geometry::Matrix4;
use flui_geometry::px;
use flui_rendering::constraints::BoxConstraints;
use flui_types::styling::BoxDecoration;
use flui_types::{Alignment, Color};
use flui_widgets::{Center, Container, GestureDetector, SizedBox, Text};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Container with explicit padding shrink-wraps the padded child.
///
/// Flutter parity: container_test.dart implicit in the `'paints as expected'`
/// group setup — `Container(padding: all(7), child: SizedBox(25×33))`.
/// The outer box must be `25 + 14 × 33 + 14 = 39×47` (padding doubles each axis).
#[test]
fn container_padding_enlarges_to_wrap_child() {
    let laid = lay_out(
        Container::new()
            .padding(EdgeInsets::all(px(7.0)))
            .child(SizedBox::new(25.0, 33.0)),
        loose(1000.0),
    );
    assert_eq!(
        laid.size(laid.root()),
        size(39.0, 47.0),
        "Container(padding: all(7)) around SizedBox(25,33) must be 39×47"
    );
}

/// Container with explicit alignment positions child at `bottomRight` inside a
/// forced outer size.
///
/// Flutter parity: container_test.dart `'paints as expected'` group setup uses
/// `Container(alignment: Alignment.bottomRight, …)`. The child is positioned
/// at `(outer_w - child_w, outer_h - child_h)`.
#[test]
fn container_alignment_bottom_right_positions_child_correctly() {
    let laid = lay_out(
        Container::new()
            .width(100.0)
            .height(80.0)
            .alignment(Alignment::BOTTOM_RIGHT)
            .child(SizedBox::new(30.0, 20.0)),
        loose(1000.0),
    );
    assert_eq!(laid.size(laid.root()), size(100.0, 80.0));

    // FLUI's Container with alignment composes Align → child.
    // root → constrained_box → align → sized_box
    let align_node = laid.only_child(laid.root());
    let child_node = laid.only_child(align_node);
    assert_eq!(
        laid.size(child_node),
        size(30.0, 20.0),
        "SizedBox inside aligned Container must keep its natural size"
    );
    // bottom-right: x = 100 - 30 = 70, y = 80 - 20 = 60
    assert_eq!(
        laid.offset(child_node),
        offset(70.0, 60.0),
        "child must be positioned at bottom-right: (70, 60)"
    );
}

/// The full composition chain in one shot: alignment → padding → color →
/// constraints (clamping folded-in width/height) → margin, mirroring the
/// oracle's `Container(alignment:, padding:, color:, width:, height:,
/// constraints:, margin:, child:)` control fixture end to end.
///
/// `width: 53, height: 76` fold into `constraints: (minW 50, maxW 55, minH
/// 78, maxH 82)` via `tighten`: width 53 is already in range, height 76
/// clamps *up* to the 78 minimum — so the color layer's box is exactly
/// `53×78`, not `53×76`. Margin (5 all sides) adds outside that: total
/// `63×88`, color layer at absolute `(5, 5)`. Padding (7 all sides) plus
/// `bottomRight` alignment then places the `25×33` child at
/// `(5 + 7 + (39 - 25), 5 + 7 + (64 - 33)) = (26, 43)` — the exact rect the
/// oracle's `paints..rect(...)` pins for the yellow child.
///
/// Flutter parity: container_test.dart `'paints as expected'` (3.44.0).
/// `color`'s paint value and `foregroundDecoration` (no FLUI field, see the
/// module doc) are out of scope; the geometry is exact.
#[test]
fn container_full_composition_clamps_size_and_positions_child() {
    let laid = lay_out(
        Container::new()
            .alignment(Alignment::BOTTOM_RIGHT)
            .padding(EdgeInsets::all(px(7.0)))
            .color(Color::rgb(0, 255, 0))
            .width(53.0)
            .height(76.0)
            .constraints(BoxConstraints::new(px(50.0), px(55.0), px(78.0), px(82.0)))
            .margin(EdgeInsets::all(px(5.0)))
            .child(SizedBox::new(25.0, 33.0)),
        loose(1000.0),
    );

    let root = laid.root();
    assert_eq!(
        laid.size(root),
        size(63.0, 88.0),
        "margin(5) around the clamped 53×78 color box must be 63×88"
    );

    let color_node = laid.find_by_render_type("RenderDecoratedBox");
    assert_eq!(
        laid.size(color_node),
        size(53.0, 78.0),
        "height 76 must clamp up to the constraints' 78 minimum"
    );
    assert_eq!(
        laid.absolute_offset(color_node),
        offset(5.0, 5.0),
        "the color layer sits inside the 5px margin"
    );

    // Padding∘Align and Align∘Padding commute for the child's final offset,
    // so pin the order by layer identity and intermediate box: the layer
    // directly inside the color box must be the Padding, filling it at
    // 53×78, with Align inside it at the padded 39×64 — the swapped order
    // would put Align first and shrink-wrap the Padding to 39×47.
    let padding_node = laid.only_child(color_node);
    assert!(
        laid.find_all_by_render_type("RenderPadding")
            .contains(&padding_node),
        "the layer directly inside the color box must be the Padding layer"
    );
    assert_eq!(
        laid.size(padding_node),
        size(53.0, 78.0),
        "Padding fills the color box"
    );
    let align_node = laid.only_child(padding_node);
    assert_eq!(
        laid.find_by_render_type("RenderAlign"),
        align_node,
        "Align must sit inside the Padding layer"
    );
    assert_eq!(
        laid.size(align_node),
        size(39.0, 64.0),
        "Align's box is the color box minus 7px padding per side"
    );
    let child_node = laid.only_child(align_node);
    assert_eq!(laid.size(child_node), size(25.0, 33.0));
    assert_eq!(
        laid.absolute_offset(child_node),
        offset(26.0, 43.0),
        "padding(7) + bottomRight alignment inside the 53×78 color box, \
         offset by the 5px margin, must place the child at (26, 43)"
    );
}

/// A childless, unconfigured `Container` given unbounded space in one axis
/// must collapse to zero on that axis rather than panic — the childless
/// placeholder (`LimitedBox(0,0)` over `ConstrainedBox.expand()`) replaces
/// the unbounded max with its own zero limit before `expand()` forces a tight
/// fit.
///
/// Flutter parity: container_test.dart `'Can be placed in an infinite box'`
/// (3.44.0) — a smoke test there (no explicit size assertion); ported here
/// with a concrete size pin since FLUI's harness can assert it directly.
#[test]
fn container_collapses_to_zero_in_the_unbounded_dimension_when_childless() {
    let unbounded_height = BoxConstraints::new(px(0.0), px(300.0), px(0.0), px(f32::INFINITY));
    let laid = lay_out(Container::new(), unbounded_height);

    assert_eq!(
        laid.size(laid.root()),
        size(300.0, 0.0),
        "a childless Container must collapse to 0 height under an unbounded \
         height constraint, not panic"
    );
}

/// `Transform` is the outermost layer in `Container::build`, and a
/// transform never changes its own render object's laid-out box (Flutter
/// parity: transform affects painting/hit-testing of descendants, not this
/// object's own geometry) — the oracle's `getSize`/corner-offset assertions
/// on the transformed `Container` all resolve to its untransformed box.
///
/// Flutter parity: container_test.dart `'Container transformAlignment'`
/// (3.44.0) — the box-geometry invariant the `getSize`/`getTopLeft`/etc.
/// assertions actually pin. `transformAlignment` itself has no FLUI
/// `Container` setter (module doc, `docs/ROADMAP.md` Cross.H).
#[test]
fn container_transform_is_outermost_and_does_not_affect_own_box_size() {
    let laid = lay_out(
        Container::new()
            .width(100.0)
            .height(100.0)
            .transform(Matrix4::scaling(0.5, 0.5, 1.0))
            .child(SizedBox::square(50.0)),
        loose(1000.0),
    );

    let root = laid.root();
    assert_eq!(
        laid.find_by_render_type("RenderTransform"),
        root,
        "transform must be the outermost composed layer"
    );
    assert_eq!(
        laid.size(root),
        size(100.0, 100.0),
        "the Container's own box size is unaffected by its transform"
    );
}

/// A `Container` painting a solid `color` is hittable even with no child —
/// `RenderDecoratedBox::hit_test` self-hits within its decorated bounds.
///
/// Flutter parity: container_test.dart `'Container is hittable only when
/// having decorations'` (3.44.0), the `color` leg.
#[test]
fn container_with_color_is_hittable() {
    let tapped = Arc::new(AtomicBool::new(false));
    let on_tap = Arc::clone(&tapped);

    let laid = lay_out(
        GestureDetector::new()
            .on_tap(move || tapped_on_tap(&on_tap))
            .child(Container::new().color(Color::rgb(0, 0, 0))),
        tight(50.0, 50.0),
    );

    laid.dispatch_pointer_down(25.0, 25.0);
    laid.dispatch_pointer_up(25.0, 25.0);

    assert!(
        tapped.load(Ordering::SeqCst),
        "a Container painting a color must be hittable even with no child"
    );
}

/// A `Container` painting a `decoration` is hittable even with no child —
/// same `RenderDecoratedBox` self-hit as the `color` leg.
///
/// Flutter parity: container_test.dart `'Container is hittable only when
/// having decorations'` (3.44.0), the `decoration` leg.
#[test]
fn container_with_decoration_is_hittable() {
    let tapped = Arc::new(AtomicBool::new(false));
    let on_tap = Arc::clone(&tapped);

    let laid = lay_out(
        GestureDetector::new()
            .on_tap(move || tapped_on_tap(&on_tap))
            .child(
                Container::new()
                    .decoration(BoxDecoration::new().set_color(Some(Color::rgb(0, 0, 0)))),
            ),
        tight(50.0, 50.0),
    );

    laid.dispatch_pointer_down(25.0, 25.0);
    laid.dispatch_pointer_up(25.0, 25.0);

    assert!(
        tapped.load(Ordering::SeqCst),
        "a Container painting a decoration must be hittable even with no child"
    );
}

/// A childless `Container` with every non-paint property set (alignment,
/// padding, width/height, margin) but no `color`/`decoration` must NOT be
/// hittable — every layer in its composed chain (`Align`, `Padding`,
/// `ConstrainedBox`, the childless placeholder) is a pure geometry proxy
/// that only forwards hit-testing to a child, never self-hits.
///
/// Flutter parity: container_test.dart `'Container is hittable only when
/// having decorations'` (3.44.0), the "everything but color or decorations"
/// leg. `transform` is dropped from the property set here (orthogonal to the
/// no-self-hit contract under test, and its own hit-test transform math is
/// exercised separately in `tests/parity/transform_test.rs`).
#[test]
fn container_with_no_color_or_decoration_is_not_hittable() {
    let tapped = Arc::new(AtomicBool::new(false));
    let on_tap = Arc::clone(&tapped);

    let laid = lay_out(
        GestureDetector::new()
            .on_tap(move || tapped_on_tap(&on_tap))
            .child(
                Container::new()
                    .alignment(Alignment::BOTTOM_RIGHT)
                    .padding(EdgeInsets::all(px(2.0)))
                    .width(50.0)
                    .height(50.0)
                    .margin(EdgeInsets::all(px(2.0))),
            ),
        loose(1000.0),
    );

    let root_size = laid.size(laid.root());
    laid.dispatch_pointer_down(root_size.width.get() / 2.0, root_size.height.get() / 2.0);
    laid.dispatch_pointer_up(root_size.width.get() / 2.0, root_size.height.get() / 2.0);

    assert!(
        !tapped.load(Ordering::SeqCst),
        "a Container with no color/decoration and no child must never be \
         hittable, no matter what other geometry properties are set"
    );
}

fn tapped_on_tap(flag: &Arc<AtomicBool>) {
    flag.store(true, Ordering::SeqCst);
}

/// `Container` composes `Align` unconditionally whenever `alignment` is set,
/// regardless of whether it has a child. Flutter's `Container.build` only
/// does that in the `else if` branch of `if (child == null && (constraints
/// == null || !constraints!.isTight)) { current = <placeholder> } else if
/// (alignment != null) { current = Align(...) }` — when `child` is `None`
/// and `constraints` is null (the case here), the placeholder branch is
/// taken and `alignment` is discarded entirely; no `Align` is ever mounted.
///
/// This is a real, confirmed divergence — see `docs/ROADMAP.md` Cross.H
/// (`crates/flui-widgets/src/container.rs`, `Container::build`). Red-checked:
/// this test fails against the current `Container::build` (which always
/// mounts `RenderAlign` when `alignment` is set), confirming the gap is
/// real, not a mistaken expectation.
///
/// Flutter parity: container_test.dart `'Container discards alignment when
/// the child parameter is null and constraints is not Tight'` (3.44.0).
#[test]
#[ignore = "known divergence: Container::build wraps Align unconditionally; \
            see docs/ROADMAP.md Cross.H"]
fn container_discards_alignment_when_childless_and_constraints_not_tight() {
    let laid = lay_out(
        Container::new()
            .decoration(BoxDecoration::new().set_color(Some(Color::rgb(0, 0, 0))))
            .alignment(Alignment::CENTER_LEFT),
        loose(1000.0),
    );

    assert!(
        laid.find_all_by_render_type("RenderAlign").is_empty(),
        "Container must not mount an Align/RenderAlign when child is None \
         and constraints is not tight — alignment is discarded in that case"
    );
}

/// A `Container` squeezed to a zero-area box (decoration + text child
/// included) must size to `Size::ZERO` without panicking.
///
/// Mirrors the oracle's tree exactly: `Center → SizedBox.shrink →
/// Container(decoration:, child: Text('X'))` — the `Text` child matters
/// because it drags text layout/paint into the zero-area crash surface,
/// not just box geometry.
///
/// Flutter parity: container_test.dart `'Container does not crash at zero
/// area'` (3.44.0).
#[test]
fn container_does_not_crash_at_zero_area() {
    let laid = lay_out(
        Center::new().child(
            SizedBox::shrink().child(
                Container::new()
                    .decoration(BoxDecoration::new())
                    .child(Text::new("X")),
            ),
        ),
        tight(800.0, 600.0),
    );

    let container_node = laid.find_by_render_type("RenderDecoratedBox");
    assert_eq!(
        laid.size(container_node),
        size(0.0, 0.0),
        "Container squeezed to zero area must report Size::ZERO, not panic"
    );
}

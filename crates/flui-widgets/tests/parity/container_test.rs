//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/container_test.dart`
//! Ported case: the layout geometry implicit in the paint test at line 52
//! (`group('Container control tests:', ...)` → `'paints as expected'`).
//! The Flutter test verifies specific rect positions at pixel level; here we
//! port the layout invariants (alignment + padding → child geometry) without
//! the paint-pattern assertions (deferred to Phase 3).
//!
//! Widget → render-object mapping:
//! - `Container(alignment: …, padding: …)` composes `RenderConstrainedBox` →
//!   `RenderPadding` → `RenderPositionedBox` (Align) in FLUI, mirroring
//!   Flutter's composition chain.
//!
//! Divergence: none. Geometry contract is faithful; paint assertions deferred.

use crate::common::{lay_out, loose, offset, size};
use flui_geometry::EdgeInsets;
use flui_geometry::px;
use flui_types::Alignment;
use flui_widgets::{Container, SizedBox};

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
/// Flutter parity: container_test.dart line 52 group setup uses
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

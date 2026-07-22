//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/aspect_ratio_test.dart`
//! (tag `3.44.0`, 2 cases — both `testWidgets`). `AspectRatio` is a small,
//! pure-layout widget; the render-object-level constraint-fitting algorithm
//! (`_applyAspectRatio`: tight pass-through, width-first bias, max/min clamp
//! fallbacks in both directions, degenerate all-unbounded fallback) is
//! already exhaustively unit-tested against the same oracle algorithm in
//! `crates/flui-objects/src/layout/aspect_ratio.rs`
//! (`width_first_basic_case`, `height_constraint_kicks_in`,
//! `unbounded_width_uses_height_path`, `min_width_pushes_up`,
//! `tight_constraints_pass_through_unchanged`,
//! `unbounded_both_dims_falls_back_to_zero`, `new_rejects_invalid`) and at
//! the render-pipeline level in
//! `crates/flui-objects/tests/render_object_harness.rs`
//! (`harness_aspect_ratio_enforces_ratio`,
//! `harness_aspect_ratio_tight_constraints_use_smallest_size`) — cited here
//! as already-covered rather than duplicated. This file adds the missing
//! layer: the `AspectRatio` *widget* wired through `View` →
//! `RenderAspectRatio` via a real widget tree (`pump_widget`), not the
//! render object driven directly.
//!
//! Ported cases (2 of 2):
//! - `'Aspect ratio control test'` — [`aspect_ratio_control_test`]. Two
//!   sub-expectations (ratio 2.0 and ratio 0.5) inside
//!   `Center(ConstrainedBox(loose(500,500), AspectRatio(...)))` — covers
//!   both `ratio > 1` and `ratio < 1` through the width-first branch.
//! - `'Aspect ratio infinite width'` — [`aspect_ratio_infinite_width`].
//!   `AspectRatio` inside a horizontal `SingleChildScrollView` gets an
//!   unbounded main-axis (width) constraint and a bounded cross-axis
//!   (height) constraint from the composed `Viewport`, exercising the
//!   width-unbounded / height-first fallback branch of
//!   `_applyAspectRatio` end-to-end through the widget tree.
//!
//! Denominator: 2 upstream `testWidgets` cases, 2 ported, 0 out of scope.
//!
//! Widget → render-object mapping: `AspectRatio`
//! (`crates/flui-widgets/src/layout/aspect_ratio.rs`) → `RenderAspectRatio`
//! (`crates/flui-objects/src/layout/aspect_ratio.rs`). Both cases read the
//! `AspectRatio` view's own render node size directly
//! (`find_by_render_type("RenderAspectRatio")`) rather than a keyed child's
//! size as the Dart oracle does — `RenderAspectRatio::perform_layout`
//! always lays its child out tight to the computed target size
//! (`BoxConstraints::tight(target_size)`), so the render object's own size
//! and a child's measured size are identical; reading the render object
//! directly means `AspectRatio` needs no child at all here.

use crate::common::size;
use flui_rendering::constraints::BoxConstraints;
use flui_types::geometry::px;
use flui_widgets::prelude::Axis;
use flui_widgets::{AspectRatio, Center, ConstrainedBox, SingleChildScrollView};

use crate::harness;

/// A loose `width × height` box: `min = 0`, `max = (width, height)` — Flutter
/// parity: `BoxConstraints.loose(Size(width, height))`.
fn loose(width: f32, height: f32) -> BoxConstraints {
    BoxConstraints::new(px(0.0), px(width), px(0.0), px(height))
}

/// `AspectRatio` inside a loosely constrained box picks width first and
/// derives height from the ratio, for both a wide (`ratio > 1`) and a tall
/// (`ratio < 1`) ratio.
///
/// Flutter parity: `'Aspect ratio control test'` (`aspect_ratio_test.dart`,
/// tag `3.44.0`) — `Center` wraps a `ConstrainedBox(loose(500, 500))`
/// wrapping `AspectRatio`; the render object's own size (which a child,
/// laid out tight to it, would always match) is asserted against both a
/// wide (2.0) and a tall (0.5) ratio.
#[test]
fn aspect_ratio_control_test() {
    let laid = harness::pump_widget(
        Center::new().child(ConstrainedBox::new(loose(500.0, 500.0)).child(AspectRatio::new(2.0))),
        harness::screen(),
    );
    let node = laid.find_by_render_type("RenderAspectRatio");
    assert_eq!(
        laid.size(node),
        size(500.0, 250.0),
        "ratio 2.0 in a loose 500x500 box must pick width=500, height=250"
    );

    let laid = harness::pump_widget(
        Center::new().child(ConstrainedBox::new(loose(500.0, 500.0)).child(AspectRatio::new(0.5))),
        harness::screen(),
    );
    let node = laid.find_by_render_type("RenderAspectRatio");
    assert_eq!(
        laid.size(node),
        size(250.0, 500.0),
        "ratio 0.5 in a loose 500x500 box must pick width=250, height=500"
    );
}

/// `AspectRatio` with an unbounded main-axis constraint (from a horizontal
/// `SingleChildScrollView`) falls back to computing width from the bounded
/// cross-axis height.
///
/// Flutter parity: `'Aspect ratio infinite width'` (`aspect_ratio_test.dart`,
/// tag `3.44.0`) — a horizontal `SingleChildScrollView` on the default
/// 800x600 test surface gives its child an unbounded width and a height
/// bounded to 600; `AspectRatio` (ratio 2.0) must derive
/// width = height * ratio = 1200.
#[test]
fn aspect_ratio_infinite_width() {
    let laid = harness::pump_widget(
        Center::new().child(
            SingleChildScrollView::new()
                .scroll_direction(Axis::Horizontal)
                .child(AspectRatio::new(2.0)),
        ),
        harness::screen(),
    );
    let node = laid.find_by_render_type("RenderAspectRatio");
    assert_eq!(
        laid.size(node),
        size(1200.0, 600.0),
        "AspectRatio(2.0) with unbounded width and height=600 must resolve to 1200x600"
    );
}

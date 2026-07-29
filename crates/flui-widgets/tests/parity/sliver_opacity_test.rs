//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/slivers_test.dart` (tag
//! `3.44.0`) — `group('SliverOpacity - ')`'s single case, `'painting &
//! semantics'` (one of the file's 38 `testWidgets` cases; see
//! `sliver_ignore_pointer_test.rs`'s module doc for the full 38-row closeout
//! map). Not `SliverOffstage` — `'painting & semantics'` sits physically
//! between the `SliverOffstage` and `SliverIgnorePointer` groups in the
//! oracle file, close enough to misattribute by proximity, but its own
//! `group()` wrapper and every widget constructed in its body name
//! `SliverOpacity` (`widgets/sliver.dart`, tag `3.44.0`) exclusively.
//!
//! Widget → render-object mapping: `SliverOpacity` → FLUI [`SliverOpacity`]
//! (`crates/flui-widgets/src/scroll/sliver_opacity.rs`) →
//! [`RenderSliverOpacity`] (`crates/flui-objects/src/sliver/sliver_opacity.rs`).
//!
//! ## Splitting the oracle case by concern
//!
//! The oracle case pumps seven trees in sequence (opacity `1.0`, `0.0`,
//! `0.0` + `alwaysIncludeSemantics`, `0.0` again, `0.1`, `0.1` again, `0.1` +
//! `alwaysIncludeSemantics`), each followed by a `SemanticsTester` node-count
//! assertion and a `paints`/`paintsNothing` pattern match. Read by body, the
//! seven legs collapse to exactly three DISTINCT paint-observable states
//! (`1.0`, `0.0`, `0.1` — the `alwaysIncludeSemantics` toggle and the two
//! repeated `0.0`/`0.1` legs change only the semantics assertion, never the
//! paint one) plus one semantics-only axis (`alwaysIncludeSemantics`).
//! FLUI's `RenderSliverOpacity` has no analog of that semantics knob; its
//! `always_needs_compositing` field
//! (`crates/flui-objects/src/sliver/sliver_opacity.rs`) is a paint-side
//! concept — set, it forces `paint_alpha` to report a layer at every alpha
//! and disables the `alpha == 0` skip fast path — and every leg below holds
//! it at its default `false`, so the three paint legs exercise the same
//! paint behavior the oracle's pump cycles do:
//!
//! - **Paint-behavior legs** (does the node emit an alpha layer, does it
//!   suppress painting its child) — ported below, same split
//!   `opacity_test.rs` (`opacity_test.dart`'s box-protocol `'Opacity'` case)
//!   already established for the identical box-vs-sliver shape:
//!   [`sliver_opacity_one_paints_through_with_no_layer`] (the `1.0` leg),
//!   [`sliver_opacity_zero_skips_paint_with_no_layer`] (the `0.0` leg, both
//!   repeats — identical assertion each time),
//!   [`sliver_opacity_zero_point_one_paints_with_a_rounded_alpha_layer`] (the
//!   `0.1` leg, both repeats).
//! - **`SemanticsTester`/`nodesWith(...)` assertions, including the
//!   `alwaysIncludeSemantics: true` legs** — out of scope: FLUI's headless
//!   test harness has no semantics-tree assembly step (no `SemanticsTester`
//!   analogue) to check these against — the same standing gap every other
//!   port in this directory that touches semantics already cites (e.g.
//!   `sliver_grid_test.rs` case 8, `sliver_fixed_extent_list_test.rs` case
//!   6). Separately, FLUI's `SliverOpacity`
//!   (`crates/flui-widgets/src/scroll/sliver_opacity.rs`) has no
//!   `always_include_semantics` field at all yet — observed, not filed as a
//!   hardening item: there is no compiling, currently-red test that could
//!   pin it, since the harness has no semantics tree to assert against in
//!   the first place (same reasoning `opacity_test.rs`'s own module doc
//!   already applies to the box-protocol `Opacity`).
//!
//! **Total: 1 oracle case = 1 accounted for above (3 paint-observable legs
//! ported real green, covering all 7 oracle pumps' paint half; the
//! semantics half of all 7 is out of scope by the standing harness gap) — no
//! narrowing: every DISTINCT paint assertion the oracle makes has a ported
//! counterpart.**

use flui_types::Size;
use flui_types::geometry::px;
use flui_widgets::{SizedBox, SliverOpacity, SliverToBoxAdapter};

use crate::harness;

/// `CustomScrollView` substitution isn't needed here (unlike the
/// scroll-offset-correction ports) — nothing below drags or scrolls, so a
/// bare `Viewport` over one `SliverOpacity` is the direct, faithful
/// composition (mirrors `tests/sliver_opacity.rs`'s own pre-existing
/// geometry-pass-through tests, which use the identical shape without an
/// oracle citation).
fn sliver_opacity_scene(opacity: f32) -> impl flui_view::View {
    flui_widgets::Viewport::new((SliverOpacity::new(opacity)
        .child(SliverToBoxAdapter::new().child(SizedBox::new(200.0, 120.0))),))
}

// ── Paint-behavior legs of the 'painting & semantics' oracle case ──────────

/// Opacity `1.0`: fast-path passthrough, no alpha layer, child painted.
///
/// Flutter parity: `slivers_test.dart` `'painting & semantics'` (tag
/// `3.44.0`) — the `opacity: 1.0` leg (`expect(find.byType(SliverOpacity),
/// paints..paragraph())`).
#[test]
fn sliver_opacity_one_paints_through_with_no_layer() {
    let laid = harness::pump_widget(sliver_opacity_scene(1.0), harness::screen());
    let sliver_opacity_id = laid.only_child(laid.root());

    assert_eq!(
        laid.sliver_opacity_paint_alpha(sliver_opacity_id),
        None,
        "opacity 1.0 must not need an alpha layer (fast-path passthrough)"
    );
    assert!(
        !laid.sliver_opacity_skip_paint(sliver_opacity_id),
        "opacity 1.0 must still paint its child"
    );
}

/// Opacity `0.0`: fully transparent, no alpha layer, child paint suppressed
/// entirely. Accounts for BOTH the oracle's `0.0` legs (with and without
/// `alwaysIncludeSemantics`) — a knob FLUI has no equivalent field for
/// (module doc), but which does not vary the paint assertion either way, so
/// one port covers both.
///
/// Flutter parity: `slivers_test.dart` `'painting & semantics'` (tag
/// `3.44.0`) — the `opacity: 0.0` legs (`expect(find.byType(SliverOpacity),
/// paintsNothing)`).
#[test]
fn sliver_opacity_zero_skips_paint_with_no_layer() {
    let laid = harness::pump_widget(sliver_opacity_scene(0.0), harness::screen());
    let sliver_opacity_id = laid.only_child(laid.root());

    assert_eq!(
        laid.sliver_opacity_paint_alpha(sliver_opacity_id),
        None,
        "opacity 0.0 must not need an alpha layer either -- the subtree is \
         skipped, so there is nothing to composite"
    );
    assert!(
        laid.sliver_opacity_skip_paint(sliver_opacity_id),
        "opacity 0.0 must suppress painting its child entirely"
    );
}

/// Opacity `0.1`: a genuine partial blend, needs an alpha layer at the
/// rounded alpha (`0.1 * 255 = 25.5`, rounds to `26`) — same rounding
/// `opacity_test.rs`'s box-protocol equivalent pins. Accounts for BOTH the
/// oracle's `0.1` legs (with and without `alwaysIncludeSemantics`), same
/// reasoning as the `0.0` case above.
///
/// Flutter parity: `slivers_test.dart` `'painting & semantics'` (tag
/// `3.44.0`) — the `opacity: 0.1` legs (`expect(find.byType(SliverOpacity),
/// paints..paragraph())`).
#[test]
fn sliver_opacity_zero_point_one_paints_with_a_rounded_alpha_layer() {
    let laid = harness::pump_widget(sliver_opacity_scene(0.1), harness::screen());
    let sliver_opacity_id = laid.only_child(laid.root());

    assert_eq!(
        laid.sliver_opacity_paint_alpha(sliver_opacity_id),
        Some(26),
        "opacity 0.1 needs an alpha layer at round(0.1 * 255) = 26"
    );
    assert!(
        !laid.sliver_opacity_skip_paint(sliver_opacity_id),
        "a partial opacity must still paint its child (into the layer)"
    );
}

/// Opacity never affects the sliver's own layout geometry, at any of the
/// three ported values -- the render-level counterpart to the three paint
/// legs above, isolating that the alpha-layer decision is paint-only and
/// never perturbs what the parent `Viewport` lays the sliver out at.
///
/// Flutter parity: `packages/flutter/lib/src/rendering/sliver.dart`
/// `RenderSliverOpacity` (3.44.0) -- `performLayout` is a pure passthrough
/// of the incoming `SliverConstraints` to the child.
#[test]
fn sliver_opacity_does_not_change_geometry_across_the_ported_values() {
    for opacity in [0.0_f32, 0.1, 1.0] {
        let laid = harness::pump_widget(sliver_opacity_scene(opacity), harness::screen());
        let sliver_opacity_id = laid.only_child(laid.root());
        let adapter_id = laid.only_child(sliver_opacity_id);
        let box_child_id = laid.only_child(adapter_id);

        assert_eq!(
            laid.sliver_geometry(sliver_opacity_id),
            laid.sliver_geometry(adapter_id),
            "opacity {opacity} must not affect the sliver's own layout geometry"
        );
        assert_eq!(
            laid.size(box_child_id),
            // Cross-axis (width, for this vertical `Viewport`) is stretched
            // tight to the 800px screen regardless of the `SizedBox`'s own
            // 200px request -- `SliverToBoxAdapter` hands its child a box
            // constraint with a TIGHT cross axis (Flutter parity:
            // `SliverToBoxAdapter`'s child box constraints always pin the
            // cross axis to the sliver's `crossAxisExtent`). Height (the
            // main axis) is the one dimension `SizedBox` gets to pick.
            Size::new(px(800.0), px(120.0)),
            "opacity {opacity} must not affect the wrapped box child's laid-out size"
        );
    }
}

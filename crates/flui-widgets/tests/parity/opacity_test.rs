//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/opacity_test.dart` (tag
//! `3.44.0`, 4 `testWidgets` cases).
//!
//! ## Oracle denominator: 4 `testWidgets`, reconciled
//!
//! - `'Opacity'` (the large sequential test that pumps `Opacity` at 1.0,
//!   0.0, 0.0-with-`alwaysIncludeSemantics`, and 0.1, asserting a
//!   `SemanticsTester` tree plus a `paints`/`paintsNothing` pattern at each
//!   step) — split by concern:
//!   - **Paint-behavior legs** (does the node emit an `OpacityLayer`, does it
//!     suppress painting its child) — ported below:
//!     [`opacity_one_paints_through_with_no_layer`] (the `1.0` leg),
//!     [`opacity_zero_skips_paint_with_no_layer`] (the `0.0` leg),
//!     [`opacity_zero_point_one_paints_with_a_rounded_alpha_layer`] (the `0.1`
//!     leg).
//!   - **`SemanticsTester`/`hasSemantics(...)` assertions** (including the
//!     `alwaysIncludeSemantics: true` leg) — out of scope: FLUI's headless
//!     test harness has no semantics-tree assembly step (no `SemanticsTester`
//!     analogue) to check these against — the same reason this cluster is
//!     dropped in `crates/flui-widgets/tests/parity/visibility_test.rs`.
//!     Separately, FLUI's `Opacity` widget (`crates/flui-widgets/src/paint/opacity.rs`)
//!     has no `always_include_semantics` field at all yet — noted here as an
//!     observed gap, not filed as a hardening item: there is no compiling,
//!     currently-red test that could pin it, because the harness has nothing
//!     to assert a semantics tree against in the first place.
//! - `'offset is correctly handled in Opacity'` (a `matchesGoldenFile` pixel
//!   comparison of 10 stacked `Opacity`-wrapped boxes in a `Column`) — the
//!   golden-image comparison itself is out of scope (no golden-file harness
//!   here); substituted below by
//!   [`stacked_opacity_widgets_preserve_column_layout_offsets`], which asserts
//!   the same underlying invariant the golden was guarding (opacity must not
//!   perturb a child's paint position) via direct layout-offset assertions
//!   instead of a pixel diff.
//! - `'empty opacity does not crash'` (an `OffsetLayer.toImage` call on a
//!   zero-content `Opacity`-wrapped `Container()`, guarding a historical
//!   engine crash — flutter/flutter#49857) — the exact engine/image-extraction
//!   mechanism is out of scope (FLUI's headless harness produces a
//!   `LayerTree`, not a rasterized `ui.Image`, and has no `toImage` step);
//!   substituted below by
//!   [`zero_area_child_under_partial_opacity_does_not_panic_during_a_full_frame`],
//!   which drives the same root condition — a partial-alpha `OpacityLayer`
//!   over zero-content — through a real headless frame (layout → compositing
//!   → paint) and asserts it completes without panicking.
//! - `'Child shows up in the right spot when opacity is disabled'` (a
//!   `debugDisableOpacityLayers` + golden-image test) — out of scope: no
//!   golden-file harness, and FLUI has no `debugDisableOpacityLayers`-style
//!   debug toggle (there is no alternate non-layered opacity paint path to
//!   disable into). The underlying invariant (opacity must not move painted
//!   content) is the same one
//!   [`stacked_opacity_widgets_preserve_column_layout_offsets`] already
//!   covers.
//!
//! Beyond the 4-test oracle, two additional cases pin the core
//! `RenderOpacity` contract (`packages/flutter/lib/src/rendering/proxy_box.dart`
//! `RenderOpacity`, 3.44.0) the task brief calls out explicitly, since the
//! small oracle file above never names a dedicated `testWidgets` case for
//! either:
//! - [`opacity_zero_still_lays_out_and_hit_tests_its_child`] — opacity is a
//!   paint-time-only effect; unlike `IgnorePointer`, `RenderOpacity` never
//!   overrides hit-testing, so a fully transparent subtree still receives
//!   pointer events. (Already indirectly exercised through a `ClipOval`
//!   wrapper in `clip_test.rs`'s `transparent_clip_oval_hit_test_*` pair;
//!   this test isolates the same contract on a bare `Opacity` with no
//!   intermediary.)
//! - [`opacity_does_not_change_child_layout_size_across_the_full_range`] and
//!   [`nested_opacity_nodes_track_independent_alpha_not_multiplied`] — opacity
//!   never affects layout geometry, and each `RenderOpacity` node derives its
//!   own alpha from its own `opacity` field independently of any ancestor
//!   `Opacity` (Flutter composites nested `OpacityLayer`s at the compositor;
//!   `RenderOpacity` itself never multiplies opacities together).

use flui_rendering::hit_testing::HitTestBehavior;
use flui_types::Size;
use flui_types::geometry::px;
use flui_widgets::{Column, GestureDetector, Opacity, SizedBox, column};

use crate::harness::{pump_widget, screen, screen_of};

// ── Paint-behavior legs of the 'Opacity' oracle test ────────────────────────

/// Opacity `1.0`: fast-path passthrough, no `OpacityLayer`, child painted.
///
/// Flutter parity: `opacity_test.dart` `'Opacity'` — the `opacity: 1.0` leg
/// (`expect(find.byType(Opacity), paints..paragraph())`).
#[test]
fn opacity_one_paints_through_with_no_layer() {
    let laid = pump_widget(Opacity::new(1.0).child(SizedBox::new(50.0, 20.0)), screen());
    let opacity_id = laid.root();

    assert_eq!(
        laid.opacity_paint_alpha(opacity_id),
        None,
        "opacity 1.0 must not need an OpacityLayer (fast-path passthrough)"
    );
    assert!(
        !laid.opacity_skip_paint(opacity_id),
        "opacity 1.0 must still paint its child"
    );
}

/// Opacity `0.0`: fully transparent, no `OpacityLayer`, child paint
/// suppressed entirely.
///
/// Flutter parity: `opacity_test.dart` `'Opacity'` — the `opacity: 0.0` leg
/// (`expect(find.byType(Opacity), paintsNothing)`).
#[test]
fn opacity_zero_skips_paint_with_no_layer() {
    let laid = pump_widget(Opacity::new(0.0).child(SizedBox::new(50.0, 20.0)), screen());
    let opacity_id = laid.root();

    assert_eq!(
        laid.opacity_paint_alpha(opacity_id),
        None,
        "opacity 0.0 must not need an OpacityLayer either -- the subtree is \
         skipped, so there is nothing to composite"
    );
    assert!(
        laid.opacity_skip_paint(opacity_id),
        "opacity 0.0 must suppress painting its child entirely"
    );
}

/// Opacity `0.1`: a genuine partial blend, needs an `OpacityLayer` at the
/// rounded alpha (`0.1 * 255 = 25.5`, rounds to `26`).
///
/// Flutter parity: `opacity_test.dart` `'Opacity'` — the `opacity: 0.1` leg
/// (`expect(find.byType(Opacity), paints..paragraph())`); the alpha value
/// itself matches `dart:ui`'s `Color.getAlphaFromOpacity`, which rounds the
/// same way FLUI's `RenderOpacity::opacity_to_alpha` does.
#[test]
fn opacity_zero_point_one_paints_with_a_rounded_alpha_layer() {
    let laid = pump_widget(Opacity::new(0.1).child(SizedBox::new(50.0, 20.0)), screen());
    let opacity_id = laid.root();

    assert_eq!(
        laid.opacity_paint_alpha(opacity_id),
        Some(26),
        "opacity 0.1 needs an OpacityLayer at alpha round(0.1 * 255) = 26"
    );
    assert!(
        !laid.opacity_skip_paint(opacity_id),
        "a partial opacity must still paint its child (into the layer)"
    );
}

// ── Layout-offset substitute for 'offset is correctly handled in Opacity' ──

/// Three `Opacity`-wrapped fixed-height boxes stacked in a `Column` land at
/// the same main-axis offsets they would without the `Opacity` wrapper --
/// opacity must never perturb where its child paints.
///
/// Flutter parity: `opacity_test.dart` `'offset is correctly handled in
/// Opacity'` -- ported as a layout-offset assertion (this harness has no
/// golden-file comparison) rather than the oracle's `matchesGoldenFile` pixel
/// diff. This is a real substitute for the same invariant the golden was
/// guarding, not a narrowed one: the historical bug the golden caught was an
/// `OpacityLayer` painting its child at the wrong offset, which a
/// parent-relative offset assertion on each stacked child directly detects.
#[test]
fn stacked_opacity_widgets_preserve_column_layout_offsets() {
    const CHILD_HEIGHT: f32 = 50.0;

    let laid = pump_widget(
        Column::new(column![
            Opacity::new(0.5).child(SizedBox::new(200.0, CHILD_HEIGHT)),
            Opacity::new(0.5).child(SizedBox::new(200.0, CHILD_HEIGHT)),
            Opacity::new(0.5).child(SizedBox::new(200.0, CHILD_HEIGHT)),
        ]),
        screen(),
    );
    let column_id = laid.root();

    for (index, expected_y) in [0.0, CHILD_HEIGHT, CHILD_HEIGHT * 2.0]
        .into_iter()
        .enumerate()
    {
        let opacity_id = laid.child(column_id, index);
        assert_eq!(
            laid.offset(opacity_id).dy,
            px(expected_y),
            "stacked Opacity child {index} should paint at y={expected_y}"
        );
    }
}

// ── Full-frame-does-not-panic substitute for 'empty opacity does not crash' ─

/// A partial-alpha `Opacity` over a zero-area (but non-null) child subtree
/// must not panic anywhere across a full headless frame (layout →
/// compositing → paint) -- the same root condition ('an `OpacityLayer` over
/// empty content') as the historical engine crash the oracle guards against,
/// driven through FLUI's pipeline instead of `dart:ui`'s `OffsetLayer.toImage`.
///
/// The child must be present (a real, zero-*size* `SizedBox`), not absent:
/// Flutter's own `RenderOpacity.alwaysNeedsCompositing => child != null &&
/// _alpha > 0` (`proxy_box.dart:884`) and `paint`'s `if (child == null)
/// return;` mean a *childless* `Opacity` never composites at all, regardless
/// of alpha -- matching the oracle's `Opacity(opacity: 0.5, child:
/// Container())`, which is empty in *content* (`Container()` has no
/// decoration or further child) but not `child: null`.
///
/// Flutter parity: `opacity_test.dart` `'empty opacity does not crash'`
/// (`skip: isBrowser`, flutter/flutter#49857) -- the oracle's own mechanism
/// (rasterizing a 1×1 region of an `OffsetLayer` to a `ui.Image`) has no FLUI
/// equivalent (this harness never rasterizes; it only builds a `LayerTree`),
/// so this substitute is narrower than the original bug's surface -- it
/// proves the pipeline-side half (frame completes without panicking), not
/// the wgpu/engine-side image-extraction half.
#[test]
fn zero_area_child_under_partial_opacity_does_not_panic_during_a_full_frame() {
    let laid = pump_widget(
        Opacity::new(0.5).child(SizedBox::new(0.0, 0.0)),
        screen_of(0.0, 0.0),
    );
    let opacity_id = laid.root();

    assert_eq!(
        laid.size(opacity_id),
        Size::ZERO,
        "a zero-size child under 0x0 constraints lays the Opacity out to zero area"
    );
    assert_eq!(
        laid.opacity_paint_alpha(opacity_id),
        Some(128),
        "opacity 0.5 over a real (zero-area) child still needs an \
         OpacityLayer at round(0.5 * 255) = 128"
    );
}

// ── Beyond-oracle: the core RenderOpacity contract the task brief names ─────

/// Opacity `0.0` is a paint-time-only effect: the child still receives
/// pointer events. Isolates the same contract
/// `transparent_clip_oval_hit_test_still_hits_inside_the_oval` (in
/// `clip_test.rs`) exercises through a `ClipOval` intermediary, on a bare
/// `Opacity` with no other widget in between.
///
/// Flutter parity: `packages/flutter/lib/src/rendering/proxy_box.dart`
/// `RenderOpacity` (3.44.0) -- it never overrides `hitTestChildren`, so
/// `RenderProxyBox`'s default (hit-test the child unconditionally) applies
/// regardless of `_alpha`.
#[test]
fn opacity_zero_still_lays_out_and_hit_tests_its_child() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let did_tap = Arc::new(AtomicBool::new(false));
    let tap_cb = Arc::clone(&did_tap);

    let laid = pump_widget(
        Opacity::new(0.0).child(
            GestureDetector::new()
                .behavior(HitTestBehavior::Opaque)
                .on_tap(move || tap_cb.store(true, Ordering::SeqCst)),
        ),
        screen(),
    );

    // Layout still ran: the fully transparent Opacity has real box geometry,
    // not a stubbed zero size.
    assert_eq!(
        laid.size(laid.root()),
        Size::new(px(800.0), px(600.0)),
        "a fully transparent Opacity still lays out at the screen size"
    );

    laid.dispatch_pointer_down(400.0, 300.0);
    laid.dispatch_pointer_up(400.0, 300.0);

    assert!(
        did_tap.load(Ordering::SeqCst),
        "a fully transparent (opacity 0.0) Opacity must still deliver taps \
         to its child"
    );
}

/// Opacity never changes its child's layout size, at any point across the
/// full `0.0..=1.0` range -- it is exclusively a paint-time effect.
///
/// Flutter parity: `packages/flutter/lib/src/rendering/proxy_box.dart`
/// `RenderOpacity` (3.44.0) -- it has no `performLayout` override of its own
/// (`RenderProxyBox`'s passthrough applies), so the child is laid out with
/// the incoming constraints exactly as if `Opacity` were not there.
#[test]
fn opacity_does_not_change_child_layout_size_across_the_full_range() {
    let child_size = Size::new(px(123.0), px(45.0));

    for value in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
        let laid = pump_widget(
            Opacity::new(value).child(SizedBox::new(123.0, 45.0)),
            crate::common::loose(800.0),
        );
        assert_eq!(
            laid.size(laid.root()),
            child_size,
            "opacity {value} must not change the child's laid-out size"
        );
    }
}

/// A nested `Opacity` inside another `Opacity` computes its own alpha purely
/// from its own `opacity` field -- ancestor opacity must not be folded in
/// (Flutter multiplies nested `OpacityLayer`s visually at the compositor;
/// `RenderOpacity` itself never does that arithmetic).
///
/// Flutter parity: `packages/flutter/lib/src/rendering/proxy_box.dart`
/// `RenderOpacity` (3.44.0) -- `_alpha = ui.Color.getAlphaFromOpacity(opacity)`
/// reads only the node's own field, never a parent's.
#[test]
fn nested_opacity_nodes_track_independent_alpha_not_multiplied() {
    let laid = pump_widget(
        Opacity::new(0.5).child(Opacity::new(0.5).child(SizedBox::new(50.0, 20.0))),
        screen(),
    );
    let outer_id = laid.root();
    let inner_id = laid.only_child(outer_id);

    assert_eq!(
        laid.opacity_paint_alpha(outer_id),
        Some(128),
        "the outer Opacity's own alpha is round(0.5 * 255) = 128"
    );
    assert_eq!(
        laid.opacity_paint_alpha(inner_id),
        Some(128),
        "the inner Opacity's own alpha is ALSO 128 -- it must not be \
         multiplied down by the outer opacity (which would give 64)"
    );
}

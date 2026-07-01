//! Structural paint-snapshot dogfood for the render harness (sub-project A),
//! plus fallible run entry points and overflow-flag inspection.

#[test]
fn insta_tooling_smoke() {
    insta::assert_snapshot!("smoke", "line one\nline two");
}

use flui_objects::RenderColoredBox;
use flui_rendering::testing::{DrawKind, RenderTester, box_node};
use flui_types::{Size, geometry::px};

#[test]
fn frame_snapshot_and_predicate() {
    let run = RenderTester::mount(box_node(RenderColoredBox::red(40.0, 40.0)))
        .with_size(Size::new(px(40.0), px(40.0)))
        .run_frame();
    insta::assert_snapshot!("colored_box", run.snapshot());
    run.assert_paints_any(|c| c.kind == DrawKind::Rect);
}

#[test]
#[should_panic(expected = "no painted command matched")]
fn assert_paints_any_fails_on_absent_op() {
    let run = RenderTester::mount(box_node(RenderColoredBox::red(40.0, 40.0)))
        .with_size(Size::new(px(40.0), px(40.0)))
        .run_frame();
    run.assert_paints_any(|c| c.kind == DrawKind::Shadow);
}

#[test]
fn run_to_paint_exposes_layer_tree() {
    let run = RenderTester::mount(box_node(RenderColoredBox::red(40.0, 40.0)))
        .with_size(Size::new(px(40.0), px(40.0)))
        .run_to_paint();
    assert!(
        run.layer_tree().is_some(),
        "PaintRun must hold the painted layer tree"
    );
    run.assert_paints_any(|c| c.kind == DrawKind::Rect);
}

#[test]
fn run_to_compositing_is_probed_before_paint() {
    use flui_rendering::testing::Probe;
    let run = RenderTester::mount(box_node(RenderColoredBox::red(40.0, 40.0)))
        .with_size(Size::new(px(40.0), px(40.0)))
        .run_to_compositing();
    // CompositingRun has no layer tree; geometry is committed.
    let _ = run.pipeline();
}

#[test]
fn run_to_semantics_is_probed_after_paint() {
    use flui_rendering::testing::Probe;
    let run = RenderTester::mount(box_node(RenderColoredBox::red(40.0, 40.0)))
        .with_size(Size::new(px(40.0), px(40.0)))
        .run_to_semantics();
    let _ = run.pipeline();
}

// ============================================================================
// Fallible run entry points and overflow-flag inspection
// ============================================================================

use flui_objects::{RenderFittedBox, RenderStack, RenderViewport};
use flui_rendering::{
    context::FragmentRecorder,
    error::RenderError,
    protocol::{BoxProtocol, Protocol, ProtocolGeometry, ProtocolPosition, RenderObject},
    testing::Probe,
};
use flui_types::{Alignment, layout::BoxFit, painting::Clip};

/// Returns `true` when the render object at `node` reports visual overflow.
///
/// Downcasts to the concrete objects that carry an overflow flag
/// (`RenderFittedBox`, `RenderStack`, `RenderViewport`). Moved here from
/// `flui-rendering::testing` because those types now live in `flui-objects`.
fn has_overflow(probe: &impl Probe, node: flui_foundation::RenderId) -> bool {
    let pipeline = probe.pipeline();
    let Some(render_node) = pipeline.render_tree().get(node) else {
        return false;
    };
    let Some(entry) = render_node.as_box() else {
        return false;
    };
    let obj = entry.render_object();
    if let Some(fitted) = obj.as_any().downcast_ref::<RenderFittedBox>() {
        return fitted.has_visual_overflow();
    }
    if let Some(stack) = obj.as_any().downcast_ref::<RenderStack>() {
        return stack.has_visual_overflow();
    }
    if let Some(viewport) = obj.as_any().downcast_ref::<RenderViewport>() {
        return viewport.has_visual_overflow();
    }
    false
}

/// A minimal `RenderObject<BoxProtocol>` whose `paint_raw` always panics.
///
/// Direct impl (not via the `RenderBox` blanket) so the panic fires in
/// `paint_raw` — the site the pipeline wraps with `catch_unwind`. The blanket's
/// `paint` default for leaf objects is a no-op; `paint_raw` is the real gate.
///
/// Geometry is owned by `RenderState` (2B field dedup); this struct holds none.
#[derive(Debug)]
struct PanicPaintBox;

impl PanicPaintBox {
    fn new() -> Self {
        Self
    }
}

impl flui_foundation::Diagnosticable for PanicPaintBox {}

impl RenderObject<BoxProtocol> for PanicPaintBox {
    fn perform_layout_raw(
        &mut self,
        ctx: &mut <BoxProtocol as Protocol>::LayoutCtxErased<'_>,
    ) -> flui_rendering::error::RenderResult<ProtocolGeometry<BoxProtocol>> {
        Ok(ctx.constraints().biggest())
    }

    fn paint_raw(
        &self,
        _recorder: &mut FragmentRecorder,
        _child_count: usize,
        _size: flui_types::Size,
    ) {
        panic!("PanicPaintBox::paint_raw — intentional test panic");
    }

    fn hit_test_raw(
        &self,
        _position: ProtocolPosition<BoxProtocol>,
        _child_count: usize,
        _size: flui_types::Size,
        _hit_child: &mut (
                 dyn FnMut(usize, Option<ProtocolPosition<BoxProtocol>>) -> bool + Send + Sync
             ),
    ) -> flui_rendering::traits::HitTestOutcome {
        flui_rendering::traits::HitTestOutcome::miss()
    }
}

/// A panicking `paint_raw` must surface as `RenderError::Poisoned` via the
/// pipeline's `catch_unwind`, never abort the test process.
#[test]
fn try_run_frame_captures_poisoned_paint() {
    let err = RenderTester::mount(box_node(PanicPaintBox::new()))
        .with_size(Size::new(px(10.0), px(10.0)))
        .try_run_frame()
        .expect_err("a tree whose paint panics must produce Err");

    assert!(
        matches!(err, RenderError::Poisoned { .. }),
        "expected Poisoned but got {err:?}",
    );
}

/// `has_overflow` returns `true` for a `RenderFittedBox` whose scaled child
/// exceeds the box bounds, and `false` when the child fits exactly.
///
/// `BoxFit::None` leaves the child at its natural size; a 100×100 child
/// inside a tight 50×50 parent has `destination (100) > size (50)`, so
/// `RenderFittedBox::perform_layout` sets `has_visual_overflow = true`.
/// `BoxFit::Contain` scales the child down to fit, producing no overflow.
#[test]
fn has_overflow_reflects_fitted_box_overflow_flag() {
    // Overflowing: BoxFit::None — child stays 100×100 inside a 50×50 box.
    let overflowing = RenderTester::mount(
        box_node(RenderFittedBox::new(
            BoxFit::None,
            Alignment::CENTER,
            Clip::None,
        ))
        .label("fitted")
        .child(box_node(RenderColoredBox::red(100.0, 100.0))),
    )
    .with_size(Size::new(px(50.0), px(50.0)))
    .run_layout();

    assert!(
        has_overflow(&overflowing, overflowing.id("fitted")),
        "100×100 child with BoxFit::None inside a 50×50 box must report overflow",
    );

    // Non-overflowing: BoxFit::Contain — child is scaled to fit exactly.
    let clean = RenderTester::mount(
        box_node(RenderFittedBox::new(
            BoxFit::Contain,
            Alignment::CENTER,
            Clip::None,
        ))
        .label("fitted")
        .child(box_node(RenderColoredBox::red(80.0, 80.0))),
    )
    .with_size(Size::new(px(80.0), px(80.0)))
    .run_layout();

    assert!(
        !has_overflow(&clean, clean.id("fitted")),
        "80×80 child with BoxFit::Contain inside an 80×80 box must not overflow",
    );
}

// ============================================================================
// Dogfood snapshots — paint-logic-HEAVY objects (sub-project A, Task 7)
//
// Each test proves the structural snapshot catches facts that geometry/structure
// asserts miss: shadow/border ordering, clip-layer scoping, opacity layer
// alpha, and virtualized-child count at the paint layer.
// ============================================================================

// ---------------------------------------------------------------------------
// 1. RenderDecoratedBox — shadow + border + fill ordering
// ---------------------------------------------------------------------------

/// Snapshot of a `RenderDecoratedBox` carrying a box-shadow, a solid border,
/// and a background fill color.
///
/// The snapshot must show:
/// - a `DrawShadow` (or equivalent shadow command) before the fill/border,
/// - a border command (`DrawDRRect` or `DrawRRect` stroke) and/or a fill `DrawRect`,
/// - all in a sensible order consistent with CSS-style painting (shadow-behind-fill).
///
/// This is the highest-value snapshot: the command sequence (shadow → fill →
/// border) is invisible to `structure()` and `picture_bounds()`.
#[test]
fn snapshot_decorated_box() {
    use flui_objects::RenderDecoratedBox;
    use flui_types::{
        Offset, Pixels,
        geometry::px,
        styling::{Border, BorderSide, BorderStyle, BoxDecoration, BoxShadow, Color},
    };

    let decoration = BoxDecoration::<Pixels>::new()
        .set_color(Some(Color::WHITE))
        .set_border(Some(Border::all(BorderSide::new(
            Color::BLACK,
            px(2.0),
            BorderStyle::Solid,
        ))))
        .set_box_shadow(Some(vec![BoxShadow::new(
            Color::rgba(0, 0, 0, 128),
            Offset::new(px(2.0), px(4.0)),
            px(6.0),
            px(0.0),
        )]));

    let run = RenderTester::mount(box_node(RenderDecoratedBox::new(decoration)))
        .with_size(Size::new(px(80.0), px(60.0)))
        .run_frame();

    insta::assert_snapshot!("decorated_box", run.snapshot());
}

// ---------------------------------------------------------------------------
// 2. RenderClipRect — clip layer wraps the child's picture
// ---------------------------------------------------------------------------

/// Snapshot of a `RenderClipRect` wrapping a colored child.
///
/// The snapshot must show a `ClipRect` layer (or equivalent clip scope) that
/// wraps the child's picture — proving clip scoping is a structural property
/// visible at the layer level, not just a paint-command detail.
#[test]
fn snapshot_clip_layer() {
    use flui_objects::RenderClipRect;
    use flui_types::{geometry::px, painting::Clip};

    let run = RenderTester::mount(
        box_node(RenderClipRect::new(Clip::AntiAlias))
            .child(box_node(RenderColoredBox::blue(40.0, 40.0))),
    )
    .with_size(Size::new(px(40.0), px(40.0)))
    .run_frame();

    insta::assert_snapshot!("clip_layer", run.snapshot());
}

// ---------------------------------------------------------------------------
// 3. RenderOpacity — opacity layer with alpha = 0.5
// ---------------------------------------------------------------------------

/// Snapshot of a `RenderOpacity(0.5)` wrapping a colored child.
///
/// The snapshot must show an `Opacity` layer carrying alpha ≈ 128 (0x80),
/// which is invisible to geometry assertions.
#[test]
fn snapshot_opacity_layer() {
    use flui_objects::RenderOpacity;
    use flui_types::geometry::px;

    let run = RenderTester::mount(
        box_node(RenderOpacity::new(0.5)).child(box_node(RenderColoredBox::red(50.0, 50.0))),
    )
    .with_size(Size::new(px(50.0), px(50.0)))
    .run_frame();

    insta::assert_snapshot!("opacity_layer", run.snapshot());
}

// ---------------------------------------------------------------------------
// 4. RenderSliverListLazy — only visible+cache children appear in the snapshot
// ---------------------------------------------------------------------------

/// Snapshot of a `RenderSliverListLazy` with 1 000 items inside a small
/// viewport, after enough frames to settle the visible+cache band.
///
/// The key invariant: the snapshot shows `DrawRect` entries for ONLY a bounded
/// set of children (≈ visible+cache band), NOT all 1 000.  This proves
/// virtualization works at the paint layer — off-band children are never painted.
///
/// Each live child is a `RenderColoredBox` that paints exactly one `DrawRect`.
/// Counting `DrawRect` lines in the snapshot gives the painted child count.
#[test]
#[allow(clippy::type_complexity)]
fn snapshot_lazy_sliver_visible_band() {
    use std::sync::Arc;

    use flui_objects::{RenderColoredBox as SnapColoredBox, RenderSliverListLazy, RenderViewport};
    use flui_rendering::{
        protocol::{BoxProtocol, RenderObject},
        testing::sliver_node,
    };
    use flui_types::{Size, geometry::px, layout::AxisDirection};

    // N=1000 items, 50 px each; viewport = 200 px → ~4 visible + cache band.
    let n_items = 1_000usize;
    let item_height = 50.0_f32;
    let viewport_height = 200.0_f32;
    // Default cache_extent ≈ 250 px → band ≈ (200+500)/50 ≈ 14.
    // Allow 3× for pipeline timing jitter.
    let band_limit = ((viewport_height + 500.0) / item_height).ceil() as usize * 3 + 5;

    // Each item is a colored box (paints a DrawRect → visible in the layer tree).
    // The sliver lays children out to tight cross-axis × item_height constraints.
    let source: Arc<dyn Fn(usize) -> Option<Box<dyn RenderObject<BoxProtocol>>> + Send + Sync> =
        Arc::new(move |_idx| {
            Some(Box::new(SnapColoredBox::red(300.0, item_height))
                as Box<dyn RenderObject<BoxProtocol>>)
        });

    let lazy = RenderSliverListLazy::new(n_items, item_height, Arc::clone(&source), None);

    let mut run = RenderTester::mount(
        box_node(RenderViewport::new(AxisDirection::TopToBottom)).child(sliver_node(lazy)),
    )
    .with_size(Size::new(px(300.0), px(viewport_height)))
    .run_frame();

    // Pump enough frames to settle the full visible+cache band.
    // The v1 next-frame backend builds one absent child per frame.
    let settle_frames = ((viewport_height + 500.0) / item_height).ceil() as usize + 10;
    run.pump_frames(settle_frames);

    // Mark root paint-dirty and pump one final frame so the snapshot captures
    // the fully-settled, all-band-children-visible layer tree.
    run.mark_needs_paint(run.root());
    run.pump();

    let snap = run.snapshot();

    // Each painted sliver child emits exactly one DrawRect line (one per
    // RenderColoredBox).  Count DrawRect lines to get the painted child count.
    let painted_children = snap.lines().filter(|l| l.contains("DrawRect")).count();

    assert!(
        painted_children > 0,
        "at least one child must be painted after settling; snap:\n{snap}"
    );
    assert!(
        painted_children <= band_limit,
        "painted child count {painted_children} exceeds band_limit {band_limit}: \
         virtualization must prevent painting all {n_items} items.\n\
         Snapshot:\n{snap}"
    );

    insta::assert_snapshot!("lazy_sliver_visible_band", snap);
}

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
        _hit_child: &mut dyn FnMut(
            usize,
            Option<ProtocolPosition<BoxProtocol>>,
            Option<flui_types::Matrix4>,
        ) -> bool,
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
// 4. RenderSliverList — the request-strategy band tracks scroll position
//    across head/mid/tail stops, staying bounded
// ---------------------------------------------------------------------------
//
// The paint-layer virtualization claim this section used to make against
// `RenderSliverListLazy` (only a bounded set of `DrawRect`s appear out of
// 1 000 declared items) had no analog once the render-owned build strategy
// was deleted: `RenderSliverList` never builds a child itself, so what
// paints depends entirely on what a `ChildManager` attached beforehand.  A
// bare render-only harness carries no `ChildManager`, so seeding a bounded
// set of residents directly would prove nothing about virtualization — it
// would just paint exactly the residents seeded, whatever that count is.
// That end-to-end claim (bounded render-tree node count against a huge
// declared item count, through a real `ChildManager`) is covered by
// `crates/flui-widgets/tests/lazy_list.rs`
// (`lazy_list_view_builder_convergence_stabilizes`,
// `lazy_list_view_builder_off_band_eviction_bounded`).
//
// What a render-only harness CAN still verify is the render-side half of the
// seam: the layout pass's own windowing math must ask for the right logical
// indices via `request_child_build`, tracking scroll position, regardless of
// who services the request.  That is what the test below checks.

/// Scrolls a `RenderSliverList` of 1 000 declared items from the top, to a
/// mid offset (~item 500), to a deep offset (the tail), and at each stop
/// asserts the logical indices requested via `request_child_build` are both
/// bounded (virtualization: distant items are never requested) and correctly
/// windowed (the requested band tracks the scroll position, not a stale one).
///
/// Nothing is ever attached in this harness (no `ChildManager`), so every
/// in-band index is "absent" on every pass and re-requested in full — this
/// test is about the freshly computed window each pass, not about anything
/// persisting across passes.
#[test]
fn scrolling_lazy_sliver_request_band_tracks_scroll_position_and_stays_bounded() {
    use flui_objects::RenderSliverList;
    use flui_rendering::{testing::sliver_node, view::ScrollableViewportOffset};
    use flui_types::layout::AxisDirection;

    let n_items = 1_000usize;
    let item_height = 50.0_f32;
    let viewport_height = 200.0_f32;
    // Default cache_extent ≈ 250 px each side → band ≈ (200+500)/50 ≈ 14;
    // +5 covers rounding at the window edges.
    let band_limit = ((viewport_height + 500.0) / item_height).ceil() as usize + 5;
    let max_scroll = n_items as f32 * item_height - viewport_height;
    let mid_scroll = 500.0 * item_height;

    let mut run = RenderTester::mount(
        box_node(flui_objects::RenderViewport::new(
            AxisDirection::TopToBottom,
        ))
        .child(sliver_node(RenderSliverList::new(n_items, item_height)).label("list")),
    )
    .with_size(Size::new(px(300.0), px(viewport_height)))
    .run_layout();

    let vp_id = run.root();

    let requested_indices = |run: &mut flui_rendering::testing::LayoutRun| -> Vec<usize> {
        let mut indices: Vec<usize> = run
            .owner_mut()
            .take_pending_child_requests()
            .into_iter()
            .map(|(_sliver_id, logical_index)| logical_index)
            .collect();
        indices.sort_unstable();
        indices
    };

    let scroll_to = |run: &mut flui_rendering::testing::LayoutRun, pixels: f32| {
        run.update::<flui_objects::RenderViewport<ScrollableViewportOffset>>(vp_id, |vp| {
            vp.offset_mut().set_pixels(pixels);
        });
        run.relayout();
    };

    // ---- Stop 1: offset 0 — the band sits at the head ---------------------
    let head = requested_indices(&mut run);
    assert!(
        !head.is_empty(),
        "head stop: at least one index must be requested"
    );
    assert!(
        head.len() <= band_limit,
        "head stop: {} requested indices exceeds band_limit {band_limit} \
         (virtualization violated): {head:?}",
        head.len(),
    );
    assert!(
        head.iter().all(|&idx| idx < 100),
        "head stop: a far-tail item was requested at scroll_offset=0 \
         (virtualization violated): {head:?}",
    );
    assert!(
        !head.contains(&999),
        "head stop: the very last item must not be requested while scrolled to the top",
    );

    // ---- Stop 2: mid offset (~item 500) ------------------------------------
    scroll_to(&mut run, mid_scroll);
    let mid = requested_indices(&mut run);
    assert!(
        !mid.is_empty(),
        "mid stop: at least one index must be requested"
    );
    assert!(
        mid.len() <= band_limit,
        "mid stop: {} requested indices exceeds band_limit {band_limit}: {mid:?}",
        mid.len(),
    );
    assert!(
        mid.iter().all(|&idx| (440..=560).contains(&idx)),
        "mid stop: requested band did not track scroll_offset={mid_scroll} \
         (expected indices near item 500): {mid:?}",
    );
    assert!(
        !mid.contains(&0) && !mid.contains(&999),
        "mid stop: head/tail items must not still be requested after scrolling away: {mid:?}",
    );

    // ---- Stop 3: deep offset (the tail) ------------------------------------
    scroll_to(&mut run, max_scroll);
    let tail = requested_indices(&mut run);
    assert!(
        !tail.is_empty(),
        "tail stop: at least one index must be requested"
    );
    assert!(
        tail.len() <= band_limit,
        "tail stop: {} requested indices exceeds band_limit {band_limit}: {tail:?}",
        tail.len(),
    );
    assert!(
        tail.iter().all(|&idx| idx >= 900),
        "tail stop: requested band did not reach the list's tail \
         at scroll_offset={max_scroll}: {tail:?}",
    );
    assert!(
        !tail.contains(&0) && !tail.contains(&500),
        "tail stop: head/mid items must not still be requested at the tail: {tail:?}",
    );
}

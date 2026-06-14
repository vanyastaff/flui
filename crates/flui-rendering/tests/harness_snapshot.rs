//! Structural paint-snapshot dogfood for the render harness (sub-project A),
//! plus fallible run entry points and overflow-flag inspection.

#[test]
fn insta_tooling_smoke() {
    insta::assert_snapshot!("smoke", "line one\nline two");
}

use flui_rendering::objects::RenderColoredBox;
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

use flui_rendering::{
    context::FragmentRecorder,
    error::RenderError,
    objects::RenderFittedBox,
    protocol::{BoxProtocol, Protocol, ProtocolGeometry, ProtocolPosition, RenderObject},
    testing::{Probe, has_overflow},
    traits::{HotReloadCapability, PaintEffectsCapability, SemanticsCapability},
};
use flui_types::{Alignment, layout::BoxFit, painting::Clip};

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
impl PaintEffectsCapability for PanicPaintBox {}
impl SemanticsCapability for PanicPaintBox {}
impl HotReloadCapability for PanicPaintBox {}

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
    ) -> bool {
        false
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

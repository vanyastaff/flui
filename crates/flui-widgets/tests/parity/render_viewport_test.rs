//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/rendering/viewport_test.dart` (tag
//! `3.44.0`) — two adjacent `group()`s, `'Viewport paint order'` and
//! `'Viewport hit-test order'`, each with two `testWidgets` cases sharing the
//! literal names `'default (firstIsTop)'` and `'lastIsTop'` (the same name
//! occurs twice in this one file — declared under `[[shared_cases]]` in the
//! manifest). Both groups build the identical scene: `CustomScrollView` with
//! `center: ValueKey(2)` (FLUI: index-based `center: Some(2)`, per
//! [`Viewport::center`](flui_widgets::Viewport::center)'s own doc — a
//! key-based center is a follow-up) and `anchor: 0.5`, over 5 generated
//! slivers.
//!
//! 1. `'Viewport paint order'` — **ported, real green**:
//!    [`paint_order_default_first_is_top_paints_the_last_sliver_first`],
//!    [`paint_order_last_is_top_paints_the_first_sliver_first`]. The oracle
//!    wraps each sliver's child in a `CustomPaint` whose painter appends the
//!    sliver's index to a shared log; FLUI's own `CustomPaint` +
//!    `CustomPainter` give the identical mechanism directly, no bespoke
//!    render object needed.
//! 2. `'Viewport hit-test order'` — **ported, real green**:
//!    [`hit_test_order_default_first_is_top_starts_at_the_first_sliver`],
//!    [`hit_test_order_last_is_top_starts_at_the_last_sliver`]. The oracle's
//!    `_RenderAllOverlapSliver` is a leaf sliver that fills the whole
//!    remaining paint extent (`layoutExtent: 0.0`, so every sibling starts at
//!    the same position and all 5 overlap completely) and always adds itself
//!    to the hit-test result, returning `false` so the walk continues into
//!    the siblings behind it. FLUI has no equivalent stock sliver, so this
//!    port defines [`AllOverlapSliver`] directly against `RenderSliver`. Its
//!    `hit_test` calls `ctx.register_self_hit_entry()`. Writing this port is
//!    what found issue #844: the `add_self` that looked like the obvious hook
//!    wrote into a protocol-level `SliverHitTestResult` nothing read, and is
//!    now deleted along with that whole path. `register_self_hit_entry` is
//!    the mechanism the pipeline actually wires (`HitTestContext`'s own doc:
//!    models `HitTestBehavior::Translucent`), and needs no id — the driver already
//!    knows this node's `RenderId` and builds the `HitTestEntry` itself.
//!    `HitTestResult::path()` exposes ordered entries with a public
//!    `target: RenderId` field, so the port reads hit order the same way the
//!    oracle reads `result.path.map((e) => e.target)`.

use std::sync::{Arc, Mutex};

use flui_foundation::RenderId;
use flui_rendering::constraints::SliverGeometry;
use flui_rendering::context::{SliverHitTestContext, SliverLayoutContext};
use flui_rendering::delegates::CustomPainter;
use flui_rendering::pipeline::Canvas;
use flui_rendering::prelude::Leaf;
use flui_rendering::protocol::SliverProtocol;
use flui_rendering::traits::RenderSliver;
use flui_rendering::view::SliverPaintOrder;
use flui_types::Size;
use flui_view::{BoxedView, RenderView, ViewExt, impl_render_view};
use flui_widgets::{CustomPaint, CustomScrollView, SliverToBoxAdapter, Text};

use crate::common::{lay_out, offset, tight};

// ============================================================================
// 'Viewport paint order'
// ============================================================================

/// Appends its sliver's `id` to a shared log when painted — the oracle's
/// `TestCustomPainter()..onPaint = (_, _) => paintLog.add(i)`.
#[derive(Debug)]
struct LoggingPainter {
    id: usize,
    log: Arc<Mutex<Vec<usize>>>,
}

impl CustomPainter for LoggingPainter {
    fn paint(&self, _canvas: &mut Canvas, _size: Size) {
        self.log.lock().expect("paint log mutex").push(self.id);
    }

    fn should_repaint(&self, _old_delegate: &dyn CustomPainter) -> bool {
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// One `SliverToBoxAdapter(child: CustomPaint(painter: LoggingPainter, child:
/// Text))` — the oracle's `makeSliver(i)` for the paint-order group.
fn logging_sliver(id: usize, log: Arc<Mutex<Vec<usize>>>) -> BoxedView {
    SliverToBoxAdapter::new()
        .child(
            CustomPaint::new()
                .painter(Arc::new(LoggingPainter { id, log }))
                .child(Text::new(format!("Item {id}"))),
        )
        .boxed()
}

/// Flutter parity: `'Viewport paint order'` `'default (firstIsTop)'` — the
/// first sliver paints LAST (on top), the last paints first.
#[test]
fn paint_order_default_first_is_top_paints_the_last_sliver_first() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let slivers: Vec<BoxedView> = (0..5).map(|i| logging_sliver(i, log.clone())).collect();
    let _laid = lay_out(
        CustomScrollView::new(slivers).center(Some(2)).anchor(0.5),
        tight(800.0, 600.0),
    );

    assert_eq!(
        *log.lock().expect("paint log mutex"),
        vec![4, 3, 2, 1, 0],
        "default paint order (FirstIsTop) must paint 4,3,2,1,0",
    );
}

/// Flutter parity: `'Viewport paint order'` `'lastIsTop'` — the last sliver
/// paints last (on top), the first paints first.
#[test]
fn paint_order_last_is_top_paints_the_first_sliver_first() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let slivers: Vec<BoxedView> = (0..5).map(|i| logging_sliver(i, log.clone())).collect();
    let _laid = lay_out(
        CustomScrollView::new(slivers)
            .center(Some(2))
            .anchor(0.5)
            .paint_order(SliverPaintOrder::LastIsTop),
        tight(800.0, 600.0),
    );

    assert_eq!(
        *log.lock().expect("paint log mutex"),
        vec![0, 1, 2, 3, 4],
        "LastIsTop paint order must paint 0,1,2,3,4",
    );
}

// ============================================================================
// 'Viewport hit-test order'
// ============================================================================

/// A leaf sliver that fills the whole remaining paint extent with zero
/// `layout_extent` — every sibling in its growth-direction group starts at
/// the same position, so all 5 overlap completely — and always adds itself
/// to the hit-test result. Flutter parity: `_RenderAllOverlapSliver`
/// (`rendering/viewport_test.dart`).
#[derive(Debug)]
struct AllOverlapSliver;

impl flui_foundation::Diagnosticable for AllOverlapSliver {}

impl RenderSliver for AllOverlapSliver {
    type Arity = Leaf;
    type ParentData = flui_rendering::parent_data::SliverParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>,
    ) -> SliverGeometry {
        let extent = ctx.constraints().remaining_paint_extent;
        SliverGeometry {
            paint_extent: extent,
            max_paint_extent: extent,
            layout_extent: 0.0,
            hit_test_extent: extent,
            visible: extent > 0.0,
            ..SliverGeometry::ZERO
        }
    }

    fn hit_test(&self, ctx: &mut SliverHitTestContext<'_, Leaf, Self::ParentData>) -> bool {
        // `register_self_hit_entry` — NOT `ctx.inner_mut().add_self(id)`,
        // which looked like the obvious hook but is a dead end: it writes
        // into `SliverHitTestCtx`'s own protocol-level `SliverHitTestResult`,
        // which `hit_test_raw`'s bridge (`traits/render_sliver.rs`) never
        // reads — the bridge builds its `HitTestOutcome` from
        // `ctx.self_hit_entry_registered() || blocks_below` alone. A first
        // attempt using `add_self` compiled and ran, but every hit-test
        // query on this scene came back empty (confirmed with a debug probe:
        // `path_len=0` even for the viewport itself, since the viewport's
        // own `hit_test` only adds itself when a child call returns `true`).
        // `register_self_hit_entry` is the mechanism `HitTestContext` actually
        // wires to the pipeline (its own doc: models `HitTestBehavior::Translucent`
        // — receive the hit, keep testing what is behind), and needs no id:
        // the driver (`hit_test_sliver_subtree_impl`) already knows this
        // node's `RenderId` and builds the `HitTestEntry` itself. No local
        // bounds re-check either — the driver's own gate on `geometry`
        // already enforces `[0, hit_test_extent)` before this method runs.
        ctx.register_self_hit_entry();
        false
    }
}

/// Widget wrapper for [`AllOverlapSliver`] — a leaf sliver, no children, no
/// configuration of its own.
#[derive(Clone, Debug, Default)]
struct AllOverlapWidget;

impl RenderView for AllOverlapWidget {
    type Protocol = SliverProtocol;
    type RenderObject = AllOverlapSliver;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        AllOverlapSliver
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}

impl_render_view!(AllOverlapWidget);

/// Reads the hit-test path at the viewport's center point, mapped back to
/// each of the 5 slivers' index by `RenderId` — the oracle's
/// `result.path.map((e) => e.target).map((t) => t.id).nonNulls`.
fn hit_order_at_center(laid: &flui_widgets::testing::LaidOut) -> Vec<usize> {
    let sliver_ids: Vec<RenderId> = (0..5).map(|i| laid.child(laid.root(), i)).collect();
    let result = laid.hit_test_pointer(offset(400.0, 300.0));
    result
        .path()
        .iter()
        .filter_map(|entry| sliver_ids.iter().position(|&id| id == entry.target))
        .collect()
}

fn all_overlap_slivers() -> Vec<BoxedView> {
    (0..5).map(|_| AllOverlapWidget.boxed()).collect()
}

/// Flutter parity: `'Viewport hit-test order'` `'default (firstIsTop)'`.
#[test]
fn hit_test_order_default_first_is_top_starts_at_the_first_sliver() {
    let laid = lay_out(
        CustomScrollView::new(all_overlap_slivers())
            .center(Some(2))
            .anchor(0.5),
        tight(800.0, 600.0),
    );

    assert_eq!(
        hit_order_at_center(&laid),
        vec![0, 1, 2, 3, 4],
        "default paint order (FirstIsTop) hit-tests front-to-back: 0,1,2,3,4",
    );
}

/// Flutter parity: `'Viewport hit-test order'` `'lastIsTop'`.
#[test]
fn hit_test_order_last_is_top_starts_at_the_last_sliver() {
    let laid = lay_out(
        CustomScrollView::new(all_overlap_slivers())
            .center(Some(2))
            .anchor(0.5)
            .paint_order(SliverPaintOrder::LastIsTop),
        tight(800.0, 600.0),
    );

    assert_eq!(
        hit_order_at_center(&laid),
        vec![4, 3, 2, 1, 0],
        "LastIsTop hit-tests back-to-front: 4,3,2,1,0",
    );
}

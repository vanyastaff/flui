//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/list_view_test.dart`
//! Pattern: a `ListView.builder` over N items whose combined extent fits the
//! viewport builds all N items after the lazy settle sequence (two ticks).
//!
//! Widget → render-object mapping:
//! - `ListView` → `RenderViewport` (root) + `RenderSliverList` (sliver child)
//! - Each item → one `RenderConstrainedBox` (via `SizedBox`)
//!
//! Divergence: Flutter's test asserts item count via `find.byType`; FLUI uses
//! `render_node_count` (render-tree node count) because the type-finder is a
//! new primitive verified elsewhere. The frame sequence (two ticks) is an
//! intentional FLUI-specific detail documented in `lazy_list.rs`.
//!
//! Two more cases ported from the same upstream file (tag `3.44.0`):
//! - `'Updates viewport dimensions when scroll direction changes'` →
//!   [`viewport_dimension_updates_across_a_scroll_direction_rebuild`].
//! - `'ListView large scroll jump'` →
//!   [`large_scroll_jump_settles_the_new_window_without_materializing_the_skipped_band`],
//!   adapted: upstream asserts the EXACT sequence of item-builder indices
//!   invoked (`log`); FLUI's lazy virtualizer's exact cache-extent/windowing
//!   constants are a separate, already-tested concern
//!   (`crates/flui-objects/src/sliver/sliver_list.rs`), so this instead
//!   asserts the property upstream's exact log is really checking: a large
//!   jump does not force-build every index it skipped over.
//!
//! Divergence found and fixed by this port: `ListView::build`
//! (`crates/flui-widgets/src/scroll/list_view.rs`) hardcoded
//! `AxisDirection::LeftToRight` for `Axis::Horizontal`, ignoring the ambient
//! `Directionality` entirely — a horizontal `ListView` under an RTL ancestor
//! laid its items out left-to-right instead of Flutter's right-to-left
//! (`ScrollView.getDirection`, `widgets/scroll_view.dart`, delegating to
//! `getAxisDirectionFromAxisReverseAndDirectionality`,
//! `widgets/basic.dart:4513-4527`). See
//! [`horizontal_under_rtl_directionality_lays_the_first_item_out_at_the_right_edge`]
//! for the oracle and the falsifying geometry.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use crate::common::{lay_out, tight};
use flui_types::typography::TextDirection;
use flui_view::ViewExt;
use flui_widgets::prelude::Axis;
use flui_widgets::{Directionality, ListView, ScrollController, SizedBox};

/// A `ListView.builder` over 3 items that all fit in the viewport builds
/// all 3 items after the two-tick lazy-settle sequence.
///
/// Flutter parity: list_view_test.dart — dynamic list populates the viewport
/// when all items are visible (C1.7 / C2-dynamic contract).
///
/// Frame sequence (see `lazy_list.rs` module doc for rationale):
/// - After mount: sliver has no children yet.
/// - After tick 1: `run_frame` emits build requests; `service_child_requests`
///   builds all 3 children.
/// - After tick 2: sliver re-lays with real children; tree is settled.
///
/// Expected node count: 1 `RenderViewport` + 1 `RenderSliverList` + 3 items
/// = 5 render nodes total.
#[test]
fn list_view_builder_builds_all_visible_items() {
    // 3 items × 60 px = 180 px total; viewport is 300 × 300 → all visible.
    let mut laid = lay_out(
        ListView::builder(3, 60.0, |index| {
            if index < 3 {
                Some(SizedBox::new(300.0, 60.0).boxed())
            } else {
                None
            }
        }),
        tight(300.0, 300.0),
    );

    // tick 1: dispatches child-build requests.
    laid.tick();
    // tick 2: sliver re-lays with the built children.
    laid.tick();

    // 1 RenderViewport + 1 RenderSliverList + 3 RenderRepaintBoundary
    // + 3 SizedBox nodes = 8.
    //
    // The per-item boundary matches Flutter: `SliverChildBuilderDelegate`
    // defaults `addRepaintBoundaries` to `true` and wraps each child
    // (`widgets/scroll_delegate.dart:560`), so items that did not change are
    // not repainted as the list scrolls.
    assert_eq!(
        laid.render_node_count(),
        8,
        "ListView(3 items) must have exactly 8 render nodes after settle \
         (1 viewport + 1 sliver-list + 3 boundaries + 3 items)"
    );
}

/// `ScrollController::viewport_dimension_pixels` must track the CURRENT
/// scroll axis's real viewport length, and must switch correctly when a
/// rebuild flips `scroll_direction` on the SAME controller (no remount).
///
/// Flutter parity: list_view_test.dart `'Updates viewport dimensions when
/// scroll direction changes'` (regression for flutter/flutter#43380) — a
/// 100×200 box hosting the list reports `viewportDimension == 100.0` when
/// horizontal, `200.0` when vertical, and `100.0` again once switched back.
#[test]
fn viewport_dimension_updates_across_a_scroll_direction_rebuild() {
    let controller = ScrollController::new();
    let list = |axis| {
        ListView::new(50.0, vec![SizedBox::new(50.0, 50.0).boxed()])
            .scroll_direction(axis)
            .position(controller.position())
    };

    // 100 wide × 200 tall: horizontal viewport dimension is the width (100).
    let mut laid = lay_out(list(Axis::Horizontal), tight(100.0, 200.0));
    assert_eq!(
        controller.viewport_dimension_pixels(),
        100.0,
        "horizontal scroll direction must report the viewport's WIDTH"
    );

    // Same controller, same root constraints, vertical instead: viewport
    // dimension becomes the height (200).
    laid.pump_widget(list(Axis::Vertical));
    assert_eq!(
        controller.viewport_dimension_pixels(),
        200.0,
        "switching to vertical must update viewport_dimension_pixels to the HEIGHT"
    );

    // Back to horizontal: must report 100 again, not get stuck at 200.
    laid.pump_widget(list(Axis::Horizontal));
    assert_eq!(
        controller.viewport_dimension_pixels(),
        100.0,
        "switching back to horizontal must update viewport_dimension_pixels back \
         to the WIDTH, not retain the previous axis's value"
    );
}

/// A single large `jump_to` well past the currently-built window must settle
/// (after the lazy virtualizer's two-tick settle sequence) by building only
/// the new visible window — not every index between the old and new
/// position. This is the property Flutter's exact-index-log assertion is
/// really checking (see this file's module doc for why the log itself isn't
/// ported verbatim).
///
/// Flutter parity: list_view_test.dart `'ListView large scroll jump'` —
/// `position.jumpTo(2025.0)` on a 20-item, 200px-extent list produces a build
/// log of `[8, 9, 10, 11, 12, 13, 14]` (the new window), never the indices in
/// between the old window (`[0..4]`) and the new one.
#[test]
fn large_scroll_jump_settles_the_new_window_without_materializing_the_skipped_band() {
    let controller = ScrollController::new();
    let built_indices: Rc<RefCell<HashSet<usize>>> = Rc::new(RefCell::new(HashSet::new()));
    let log = Rc::clone(&built_indices);

    // 30 items * 60px estimate = 1800px content in a 180px viewport ->
    // max_scroll_extent = 1620.
    let widget = ListView::builder(30, 60.0, move |index| {
        log.borrow_mut().insert(index);
        (index < 30).then(|| SizedBox::new(200.0, 60.0).boxed())
    })
    .position(controller.position());

    let mut laid = lay_out(widget, tight(200.0, 180.0));
    laid.tick();
    laid.tick();
    built_indices.borrow_mut().clear();

    // Jump deep into the list — the new visible window sits around index 20
    // (1200px / 60px per item); index 10 sits squarely in the skipped band
    // between the old window (near index 0) and the new one.
    //
    // Unlike `AnimatedBuilder`-wrapped `Scrollable`, a bare `ListView` in
    // position mode has no listener that reacts to `jump_to` on its own — a
    // `pump()` (mark-dirty, matching `list_view_position_passthrough_feeds_the_content_dimension_feedback_loop`
    // in `tests/scroll.rs`) is what makes the render tree notice the new
    // position; the two follow-up `tick()`s replay the same
    // dispatch-then-settle cadence the initial mount above needed.
    controller.jump_to(1200.0);
    laid.pump();
    laid.tick();
    laid.tick();

    let built = built_indices.borrow();
    assert!(
        !built.contains(&10),
        "a large jump must not force-build an index in the skipped band between \
         the old and new visible windows; built indices: {built:?}"
    );
    assert!(
        built.iter().any(|&index| (18..=23).contains(&index)),
        "a large jump must build items in the new visible window (around index 20); \
         built indices: {built:?}"
    );
}

/// A horizontal, non-reversed `ListView` under an RTL `Directionality`
/// resolves its `AxisDirection` to `RightToLeft`: the first item
/// (`children[0]`) paints at the viewport's TRAILING (right) edge, and each
/// later item sits progressively further LEFT — the mirror image of the LTR
/// case, not just "the same layout with a flipped default."
///
/// Flutter parity: `ScrollView.getDirection` (`widgets/scroll_view.dart`)
/// delegates to `getAxisDirectionFromAxisReverseAndDirectionality`
/// (`widgets/basic.dart:4513-4527`), which for `Axis.horizontal` reads
/// `Directionality.of(context)` and maps RTL to `AxisDirection.left`.
///
/// Oracle: `RenderSliverFixedExtentList` positions each child through the
/// same shared per-child paint-offset rule every sliver uses —
/// `crates/flui-rendering/src/constraints/sliver_layout.rs`'s
/// `child_paint_offset` (ported from `RenderSliverMultiBoxAdaptorMixin`'s
/// child-positioning contract): for a not-right-way-up sliver (RTL, forward
/// growth), `main_axis_delta = paint_extent - child_main_extent -
/// child_main_axis_position`, where `child_main_axis_position = layout_offset
/// - scroll_offset`. Three 100px-wide items in an exactly-filled 300px-wide
/// viewport (`scroll_offset = 0`, `paint_extent = 300`):
/// - item 0: `layout_offset = 0` → `300 - 100 - 0 = 200`
/// - item 1: `layout_offset = 100` → `300 - 100 - 100 = 100`
/// - item 2: `layout_offset = 200` → `300 - 100 - 200 = 0`
///
/// Before the fix, `ListView::build` hardcoded `AxisDirection::LeftToRight`
/// for `Axis::Horizontal`, so this test failed with item 0 at `x = 0.0`
/// (the LTR position) instead of the RTL oracle's `x = 200.0` — see the
/// module doc's "Divergence found and fixed by this port".
#[test]
fn horizontal_under_rtl_directionality_lays_the_first_item_out_at_the_right_edge() {
    let items: Vec<flui_view::BoxedView> =
        (0..3).map(|_| SizedBox::new(100.0, 50.0).boxed()).collect();
    let laid = lay_out(
        Directionality::new(
            TextDirection::Rtl,
            ListView::new(100.0, items).scroll_direction(Axis::Horizontal),
        ),
        tight(300.0, 50.0),
    );

    let list_sliver = laid.find_by_render_type("RenderSliverFixedExtentList");
    let expected_x = [200.0, 100.0, 0.0];
    for (index, expected_x) in expected_x.into_iter().enumerate() {
        let item = laid.child(list_sliver, index);
        assert_eq!(
            laid.offset(item).dx.get(),
            expected_x,
            "item {index}'s local x offset within the sliver must match the RTL oracle"
        );
    }
}

/// A list whose length only its builder knows reports an honest scroll extent
/// on the FIRST frame.
///
/// `ItemCount::Unknown` resolves once, by probing the builder — doubling then
/// bisecting, the same walk as `SliverMultiBoxAdaptorElement.childCount`.
/// Before this, an unknown length was spelled `usize::MAX` and the first `None`
/// clamped the count in whichever layout pass happened to meet it, so the
/// reported extent was absurd until the band walked that far.
///
/// The oracle is `max_scroll_extent` rather than a rendered row: a list that
/// materialises its visible rows correctly while advertising a scroll range of
/// billions is exactly the defect, and every row-level assertion passes
/// through it.
#[test]
fn an_unknown_item_count_reports_its_real_extent_on_the_first_frame() {
    use flui_view::ViewExt as _;
    use flui_view::element::ItemCount;
    use flui_widgets::{SliverList, Viewport};

    const ROWS: usize = 7;
    const ROW_HEIGHT: f32 = 40.0;

    let controller = ScrollController::new();
    // Bound, not dropped: the layout state the assertion reads lives in this
    // harness, and `let _ = ...` would drop it before the assertion runs.
    let _laid = lay_out(
        Viewport::new((SliverList::new(
            ItemCount::Unknown,
            ROW_HEIGHT,
            std::rc::Rc::new(|i: usize| {
                (i < ROWS).then(|| SizedBox::new(200.0, ROW_HEIGHT).boxed())
            }),
        ),))
        .position(controller.position()),
        tight(200.0, 100.0),
    );
    // 7 rows × 40 px of content in a 100 px viewport.
    assert_eq!(
        controller.max_scroll_extent(),
        ROWS as f32 * ROW_HEIGHT - 100.0,
        "an unknown count must be resolved before the first layout, not \
         clamped over later passes",
    );
}

/// The probe runs once per mount, not once per rebuild.
///
/// A view value is reconstructed on every build, so resolving `Unknown` in the
/// view's constructor would repeat `2·log₂(n)` builder calls every frame — and
/// a builder with side effects would see every one of them. The resolution
/// belongs at `create_render_object`, which runs once.
///
/// The oracle is DIFFERENTIAL: the same tree declared `Exact` and declared
/// `Unknown` must call the builder the same number of times *per rebuild*. An
/// absolute bound cannot work — a rebuild legitimately re-consults the builder
/// for every resident, so the count grows either way, and picking a threshold
/// by hand just encodes whatever the current reconcile happens to do. Asserting
/// the extent again would prove nothing at all: a per-rebuild probe produces
/// the right extent every time.
#[test]
fn an_unknown_item_count_is_probed_once_per_mount_not_per_rebuild() {
    use flui_view::element::ItemCount;
    use flui_widgets::{SliverList, Viewport};

    const ROWS: usize = 7;

    // The list is constructed INSIDE a `build`, which is the only shape where
    // per-rebuild view construction is observable: a view value handed to
    // `lay_out` directly is built once and never again, so a fixture like that
    // passes whether the probe is in the constructor or at mount. (It did.)
    #[derive(Clone)]
    struct ListHost {
        declared: ItemCount,
        calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    impl flui_view::view::StatelessView for ListHost {
        fn build(&self, _ctx: &dyn flui_view::BuildContext) -> impl flui_view::IntoView {
            let calls = std::sync::Arc::clone(&self.calls);
            Viewport::new((SliverList::new(
                self.declared,
                40.0,
                std::rc::Rc::new(move |i: usize| {
                    calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    (i < ROWS).then(|| SizedBox::new(200.0, 40.0).boxed())
                }),
            ),))
        }
    }

    impl flui_view::View for ListHost {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateless(self)
        }
    }

    // Builder calls at mount, and the additional calls four rebuilds cost.
    let measure = |declared: ItemCount| -> (usize, usize) {
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut laid = lay_out(
            ListHost {
                declared,
                calls: std::sync::Arc::clone(&calls),
            },
            tight(200.0, 100.0),
        );
        let at_mount = calls.load(std::sync::atomic::Ordering::SeqCst);
        for _ in 0..4 {
            laid.pump();
        }
        let total = calls.load(std::sync::atomic::Ordering::SeqCst);
        (at_mount, total - at_mount)
    };

    let (exact_mount, exact_rebuilds) = measure(ItemCount::Exact(ROWS));
    let (unknown_mount, unknown_rebuilds) = measure(ItemCount::Unknown);

    // An unknown-count list pays ONE extra builder call per rebuild: the O(1)
    // check for whether the source grew past its known end. What it must not
    // pay is another SEARCH, which for this list is about a dozen calls — so
    // the bound is per-rebuild and tight enough that a re-probe cannot hide
    // under it.
    let extra_per_rebuild = 1;
    assert!(
        unknown_rebuilds <= exact_rebuilds + 4 * extra_per_rebuild,
        "rebuilds must cost an unknown-count list at most one extra builder \
         call each (the growth check), not another search: {unknown_rebuilds} \
         vs {exact_rebuilds} for an exact count"
    );
    assert!(
        unknown_mount > exact_mount,
        "the probe must have run at mount ({unknown_mount} calls vs \
         {exact_mount} for an exact count)"
    );
}

/// An unknown-count source that GROWS is discovered.
///
/// The manager's clamp only ever shrinks the count — it fires when a requested
/// index answers `None` — so nothing else can find a source that gained items
/// after mount. A paged feed is exactly that, and it is the case
/// `ItemCount::Unknown`'s own documentation names, so leaving it undiscovered
/// would make the variant useless for its stated purpose.
///
/// The oracle is `max_scroll_extent`: the rows themselves render correctly at
/// any count, and only the advertised extent says whether the list knows how
/// long it is.
#[test]
fn an_unknown_item_count_that_grows_is_rediscovered_on_the_next_build() {
    use flui_view::ViewExt as _;
    use flui_view::element::ItemCount;
    use flui_widgets::{ScrollController, SliverList, Viewport};

    const ROW: f32 = 40.0;
    let rows = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(3));
    let controller = ScrollController::new();

    #[derive(Clone)]
    struct Feed {
        rows: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        position: flui_rendering::view::ScrollPosition,
    }

    impl flui_view::view::StatelessView for Feed {
        fn build(&self, _ctx: &dyn flui_view::BuildContext) -> impl flui_view::IntoView {
            let rows = std::sync::Arc::clone(&self.rows);
            Viewport::new((SliverList::new(
                ItemCount::Unknown,
                ROW,
                std::rc::Rc::new(move |i: usize| {
                    (i < rows.load(std::sync::atomic::Ordering::SeqCst))
                        .then(|| SizedBox::new(200.0, ROW).boxed())
                }),
            ),))
            .position(self.position.clone())
        }
    }

    impl flui_view::View for Feed {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateless(self)
        }
    }

    let mut laid = lay_out(
        Feed {
            rows: std::sync::Arc::clone(&rows),
            position: controller.position(),
        },
        tight(200.0, 80.0),
    );
    assert_eq!(
        controller.max_scroll_extent(),
        3.0 * ROW - 80.0,
        "the initial probe found three rows"
    );

    // The feed gained a page.
    rows.store(9, std::sync::atomic::Ordering::SeqCst);
    laid.pump();

    assert_eq!(
        controller.max_scroll_extent(),
        9.0 * ROW - 80.0,
        "a grown source must be rediscovered; the clamp alone can only shrink"
    );
}

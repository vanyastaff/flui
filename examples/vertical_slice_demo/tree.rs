//! The vertical-slice demo tree — shared, via `#[path]`-inclusion, between
//! `examples/vertical_slice_demo/main.rs` (mounted on a live window through
//! `flui_app::run_app`) and the root-crate acceptance test
//! `tests/vertical_slice_demo.rs` (mounted headlessly through
//! `flui_binding::HeadlessBinding`). Both consumers exercise the exact same
//! tree, so the acceptance test proves the tree the example actually runs.
//!
//! Self-contained over `flui-widgets` (plus the lower-layer `flui-view` and
//! `flui-animation` crates it already depends on for `RebuildHandle` and
//! `Curves`) — no `flui-app` import here; each consumer decides how to mount.
//!
//! Composition: a `StatefulView` root (`count`, `expanded`, `scroll_offset`)
//! builds a `Column` of:
//! - a counter row: `Text` showing the count, plus a `GestureDetector` "+"
//!   button that increments it;
//! - a fixed-height, drag-to-scroll list with enough rows to overflow its
//!   box;
//! - a `GestureDetector`-wrapped `AnimatedContainer` that toggles its
//!   width/height/color between a collapsed and an expanded target.
//!
//! # Drag-to-scroll wiring (demo-local, not `Scrollable`)
//!
//! The list is wrapped in a plain `GestureDetector` feeding a
//! `ScrollController` directly, deliberately NOT the `Scrollable` widget:
//! `Scrollable` hardwires a `SingleChildScrollView` as its layout/paint host
//! with no offset feed-through to an arbitrary scrollable child
//! (`scrollable.rs`), so nesting an arbitrary scrollable child inside it
//! would produce a degenerate viewport. Making `Scrollable` accept one is a
//! framework-level fix, out of scope here (tracked as a Business.1 item).
//!
//! The list is composed directly on `Viewport` + `SliverFixedExtentList`
//! (`flui-widgets/src/scroll/viewport.rs`, `sliver_fixed_extent_list.rs`)
//! rather than through the `ListView` convenience wrapper, because
//! `ListView::offset` only accepts a plain `f32` — it has no passthrough for
//! an injected `ScrollPosition`. `Viewport::position` does: it hands the
//! render object the controller's own shared `ScrollPosition`
//! (`ScrollController::position`), so `RenderViewport::perform_layout`'s
//! committed content extents flush straight back into the same controller
//! the drag handler reads — no manual extent feed needed (contrast the
//! deleted `update_dimensions` call this module used to carry, before the
//! content-dimension feedback loop existed).
//!
//! Drag-only: there is no fling/ballistic simulation. The pan gesture's
//! release velocity (`on_pan_end`) is intentionally left unwired — hand-
//! rolling ballistics in the demo was ruled out, and a real fling belongs to
//! the same Business.1 item.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use flui_animation::Curves;
use flui_view::RebuildHandle;
use flui_widgets::prelude::*;
use flui_widgets::{AnimatedContainer, column, row};

/// The fixed height of the box the list is clipped to. `pub` (along with the
/// animated box's collapsed/expanded constants below) so the acceptance test
/// can distinguish the tree's several `RenderConstrainedBox` nodes by
/// committed size instead of a duplicated magic number.
pub const LIST_BOX_HEIGHT: f32 = 200.0;
/// Per-row height of the list.
pub const LIST_ITEM_EXTENT: f32 = 32.0;
/// Row count: `LIST_ITEM_COUNT * LIST_ITEM_EXTENT` (768px) overflows
/// `LIST_BOX_HEIGHT` (200px), so the list is genuinely scrollable.
pub const LIST_ITEM_COUNT: usize = 24;

/// The animated box's width/height at rest (`expanded == false`).
pub const COLLAPSED_WIDTH: f32 = 96.0;
pub const COLLAPSED_HEIGHT: f32 = 64.0;
/// The animated box's width/height target once expanded.
pub const EXPANDED_WIDTH: f32 = 220.0;
pub const EXPANDED_HEIGHT: f32 = 140.0;
/// How long the animated box takes to reach a new target.
pub const ANIMATION_DURATION: Duration = Duration::from_millis(240);

const COLLAPSED_COLOR: Color = Color::rgb(64, 64, 90);
const EXPANDED_COLOR: Color = Color::rgb(230, 126, 34);
const PLUS_BUTTON_COLOR: Color = Color::rgb(33, 150, 243);
const BACKGROUND_COLOR: Color = Color::rgb(18, 18, 24);

/// The vertical-slice demo root.
///
/// `count`/`expanded`/`scroll_offset` are `Rc<Cell<_>>` so a caller (the
/// acceptance test) can keep a clone from before mounting. All three are
/// driven the same way the running example drives them — by dispatching
/// synthetic pointer gestures through the mounted `GestureDetector`s: taps
/// for the counter and the animated box, a drag for the list. `scroll_offset`
/// is kept in sync with [`DemoRootState`]'s internal `ScrollController` by
/// the drag's `on_pan_update` callback (see the module doc's drag-to-scroll
/// wiring section above) — the render path itself reads the controller's
/// live `ScrollPosition` directly (`Viewport::position`), not this cell; the
/// cell exists as a `pub` read/seed escape hatch for callers that construct a
/// `DemoRoot` directly (see the field doc below).
#[derive(Clone, StatefulView)]
pub struct DemoRoot {
    /// Tap count, shown by the counter row and incremented by its "+" button.
    pub count: Rc<Cell<i32>>,
    /// Whether the animated box currently targets its expanded size/color.
    pub expanded: Rc<Cell<bool>>,
    /// The list's scroll offset, in logical pixels — mirrored from the
    /// internal `ScrollController` by the drag gesture wired in
    /// [`DemoRootState::build`]. Also the seed value
    /// [`DemoRootState::create_state`] writes into that controller before the
    /// first layout, so constructing a `DemoRoot` with a nonzero offset does
    /// not start the list at zero.
    pub scroll_offset: Rc<Cell<f32>>,
}

impl DemoRoot {
    /// A fresh demo tree at rest: `count == 0`, collapsed, unscrolled.
    #[must_use]
    pub fn new() -> Self {
        Self {
            count: Rc::new(Cell::new(0)),
            expanded: Rc::new(Cell::new(false)),
            scroll_offset: Rc::new(Cell::new(0.0)),
        }
    }
}

impl Default for DemoRoot {
    fn default() -> Self {
        Self::new()
    }
}

/// Persistent state for [`DemoRoot`].
///
/// Captures a [`RebuildHandle`] in `init_state` (ADR-0018) so a tap callback
/// — which runs outside `build`/layout/paint — can schedule the next frame's
/// rebuild without touching the tree itself.
pub struct DemoRootState {
    count: Rc<Cell<i32>>,
    expanded: Rc<Cell<bool>>,
    scroll_offset: Rc<Cell<f32>>,
    /// The list's live scroll position — injected directly into the composed
    /// `Viewport` (`Viewport::position`), so `RenderViewport`'s own layout
    /// feeds committed content extents back into this same controller (the
    /// content-dimension feedback loop). Owned by the state (not `DemoRoot`)
    /// because it is pure wiring, not app-visible data — `scroll_offset`
    /// above is the `pub` mirror external callers can read.
    scroll_controller: ScrollController,
    /// `None` only before `init_state` has run; every `build` call happens
    /// after it (`ViewState` lifecycle order), so it is always `Some` there.
    rebuild: Option<RebuildHandle>,
}

impl StatefulView for DemoRoot {
    type State = DemoRootState;

    fn create_state(&self) -> Self::State {
        let scroll_controller = ScrollController::new();
        // Seed from the pub cell, unclamped: extents aren't known yet (no
        // layout has run), so `set_pixels` (not `jump_to`) avoids clamping a
        // nonzero seed down to the not-yet-real `[0, 0]` range. The first
        // layout's `apply_content_dimensions` clamps it against the real
        // extents once they're known — the same clamp path `jump_to` uses,
        // just deferred to when it's actually meaningful.
        scroll_controller.set_pixels(self.scroll_offset.get());

        DemoRootState {
            count: Rc::clone(&self.count),
            expanded: Rc::clone(&self.expanded),
            scroll_offset: Rc::clone(&self.scroll_offset),
            scroll_controller,
            rebuild: None,
        }
    }
}

impl ViewState<DemoRoot> for DemoRootState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.rebuild = Some(ctx.rebuild_handle());
        // Lifecycle-only acquisition (ADR-0021, port-check trigger #22): lets
        // `RenderViewport::perform_layout`'s committed content extents flush
        // a coalesced notification after layout instead of never notifying —
        // see `ScrollPosition`'s docs. A no-op read (`max_scroll_extent()`
        // etc.) doesn't need this; only listener notification does, and
        // nothing in this demo listens for it today — installed anyway so
        // the controller behaves identically to one wired through
        // `Scrollable` (`ScrollableState::install_flush_handle`).
        if let Some(handle) = ctx.post_frame_handle() {
            self.scroll_controller.position().set_flush_handle(handle);
        }
    }

    fn build(&self, _view: &DemoRoot, _ctx: &dyn BuildContext) -> impl IntoView {
        let rebuild = self
            .rebuild
            .clone()
            .expect("BUG: init_state runs before build (ViewState lifecycle order)");

        let count = self.count.get();
        let expanded = self.expanded.get();

        // -- counter row: Text + a "+" GestureDetector button -----------------
        let count_for_tap = Rc::clone(&self.count);
        let rebuild_for_count = rebuild.clone();
        let counter_row = Row::new(row![
            Text::new(format!("Count: {count}")),
            SizedBox::width(16.0),
            GestureDetector::new()
                // Opaque: the button must be tappable across its whole
                // painted area, not only where a hit-testable descendant
                // (the "+" glyph's own ink) happens to sit.
                .behavior(HitTestBehavior::Opaque)
                .on_tap(move || {
                    count_for_tap.set(count_for_tap.get() + 1);
                    rebuild_for_count.schedule();
                })
                .child(
                    Container::new()
                        .padding(EdgeInsets::all(px(8.0)))
                        .color(PLUS_BUTTON_COLOR)
                        .child(Text::new("+")),
                ),
        ]);

        // -- fixed-height, drag-to-scroll list ---------------------------------
        let scroll_offset_for_drag = Rc::clone(&self.scroll_offset);
        let scroll_controller_for_drag = self.scroll_controller.clone();
        let rebuild_for_drag = rebuild.clone();
        let list_area = GestureDetector::new()
            // Opaque: the drag must fire from anywhere in the list's box, not
            // only where a hit-testable descendant (an item's `Text`/padding)
            // happens to sit — same reasoning as the "+" button above.
            .behavior(HitTestBehavior::Opaque)
            .on_pan_update(move |details: DragUpdateDetails| {
                // Flutter convention (matches `Scrollable`'s own pan-update
                // wiring in `scrollable.rs`): a downward finger drag (positive
                // delta on the scroll axis) moves the viewport toward the
                // START of the content, so the offset DECREASES; dragging up
                // increases it. `jump_to` clamps to the extents `RenderViewport`'s
                // own layout committed into this controller's shared
                // `ScrollPosition` (the content-dimension feedback loop —
                // see the module doc), not a manually fed value.
                let proposed = scroll_controller_for_drag.pixels() - details.delta.dy.get();
                scroll_controller_for_drag.jump_to(proposed);
                scroll_offset_for_drag.set(scroll_controller_for_drag.pixels());
                rebuild_for_drag.schedule();
            })
            .child(
                SizedBox::height(LIST_BOX_HEIGHT).child(
                    // Directly on `Viewport` + `SliverFixedExtentList`, not
                    // `ListView` — see the module doc's drag-to-scroll wiring
                    // section for why: `ListView::offset` has no `ScrollPosition`
                    // passthrough, and this list needs one so the feedback
                    // loop above has somewhere to write.
                    Viewport::new((SliverFixedExtentList::new(
                        LIST_ITEM_EXTENT,
                        (0..LIST_ITEM_COUNT)
                            .map(|index| {
                                Container::new()
                                    .padding(EdgeInsets::all(px(4.0)))
                                    .child(Text::new(format!("Item {index}")))
                                    .boxed()
                            })
                            .collect::<Vec<_>>(),
                    ),))
                    .axis_direction(AxisDirection::TopToBottom)
                    .position(self.scroll_controller.position()),
                ),
            );

        // -- animated box: tap toggles expanded, AnimatedContainer eases to it -
        let expanded_for_tap = Rc::clone(&self.expanded);
        let rebuild_for_toggle = rebuild;
        let (target_width, target_height, target_color) = if expanded {
            (EXPANDED_WIDTH, EXPANDED_HEIGHT, EXPANDED_COLOR)
        } else {
            (COLLAPSED_WIDTH, COLLAPSED_HEIGHT, COLLAPSED_COLOR)
        };
        let animated_area = GestureDetector::new()
            // Opaque: the box's own child is a zero-size filler (the box's
            // painted color IS its content), so hit-testing must fire across
            // the whole animated box regardless of child hit-testability.
            .behavior(HitTestBehavior::Opaque)
            .on_tap(move || {
                expanded_for_tap.set(!expanded_for_tap.get());
                rebuild_for_toggle.schedule();
            })
            .child(
                // An empty `Text` filler, not `SizedBox` — a `SizedBox`
                // nested inside this container's own tight width/height
                // constraint would be tightened down to the SAME committed
                // size (Flutter's `enforce` semantics: an incoming tight
                // constraint always wins), producing a second
                // `RenderConstrainedBox` indistinguishable from the
                // container's own by geometry alone.
                AnimatedContainer::new(Text::new(""))
                    .width(target_width)
                    .height(target_height)
                    .color(target_color)
                    .duration(ANIMATION_DURATION)
                    .curve(Curves::EaseOut),
            );

        Container::new()
            .color(BACKGROUND_COLOR)
            .padding(EdgeInsets::all(px(16.0)))
            .alignment(Alignment::TOP_LEFT)
            .child(Column::new(column![counter_row, list_area, animated_area]))
    }
}

/// Build a fresh demo tree, ready to mount.
#[must_use]
pub fn demo_root() -> DemoRoot {
    DemoRoot::new()
}

/// Thin `StatelessView` entry point for [`flui_app::run_app`](https://docs.rs/flui-app),
/// which requires a stateless root — the demo's actual state lives one level
/// down, in [`DemoRoot`]. Purely a pass-through: it contributes no render
/// node and no behavior of its own, so the tree an app author sees on screen
/// is exactly [`DemoRoot`]'s. The acceptance test mounts `DemoRoot` directly
/// (skipping this adapter) to reach its `Rc<Cell<_>>` handles before mount;
/// structurally and behaviorally that is the identical tree.
#[derive(Clone, StatelessView)]
pub struct DemoApp;

impl StatelessView for DemoApp {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        demo_root()
    }
}

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
//! - a fixed-height, drag-to-scroll `ListView` with enough rows to overflow
//!   its box;
//! - a `GestureDetector`-wrapped `AnimatedContainer` that toggles its
//!   width/height/color between a collapsed and an expanded target.
//!
//! # Drag-to-scroll wiring (demo-local, not `Scrollable`)
//!
//! The list is wrapped in a plain `GestureDetector` feeding a
//! `ScrollController` directly, deliberately NOT the `Scrollable` widget:
//! `Scrollable` hardwires a `SingleChildScrollView` as its layout/paint host
//! with no offset feed-through to an arbitrary scrollable child
//! (`scrollable.rs`), so nesting this `ListView` inside it would produce a
//! degenerate viewport. Making `Scrollable` accept an arbitrary scrollable
//! child is a framework-level fix, out of scope here (tracked as a Business.1
//! item).
//!
//! Nothing in the framework yet propagates a `Viewport`'s committed
//! content/viewport extents back to a `ScrollController`
//! (`ViewportOffset` → `ScrollController` feedback is the same Business.1
//! item), so [`DemoRootState::create_state`] feeds `update_dimensions` the
//! same fixed constants the tree renders with, once, up front.
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
/// wiring section above) — `ListView::offset` only accepts a plain `f32`, so
/// the cell is still the value it reads each build.
#[derive(Clone, StatefulView)]
pub struct DemoRoot {
    /// Tap count, shown by the counter row and incremented by its "+" button.
    pub count: Rc<Cell<i32>>,
    /// Whether the animated box currently targets its expanded size/color.
    pub expanded: Rc<Cell<bool>>,
    /// The list's scroll offset, in logical pixels — driven by the drag
    /// gesture wired in [`DemoRootState::build`], and read by `ListView`'s
    /// `.offset(...)` each build. Also the seed value
    /// [`DemoRootState::create_state`] jump-starts the internal
    /// `ScrollController` from, so constructing a `DemoRoot` with a nonzero
    /// offset does not snap back to zero on the first drag.
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
    /// The list's drag position, clamped to `[0, max_scroll_extent]` by
    /// `jump_to`. Owned by the state (not `DemoRoot`) because it is pure
    /// wiring, not app-visible data — `scroll_offset` above is what `build`
    /// actually reads.
    scroll_controller: ScrollController,
    /// `None` only before `init_state` has run; every `build` call happens
    /// after it (`ViewState` lifecycle order), so it is always `Some` there.
    rebuild: Option<RebuildHandle>,
}

impl StatefulView for DemoRoot {
    type State = DemoRootState;

    fn create_state(&self) -> Self::State {
        let scroll_controller = ScrollController::new();
        // Manual extent feed (see the module doc's Drag-to-scroll wiring
        // section): these are the same fixed constants `build` renders the
        // list with, so computing them once here — rather than from a real
        // `Viewport` layout callback, which doesn't exist yet — is exact, not
        // an approximation.
        let max_scroll_extent =
            (LIST_ITEM_EXTENT * LIST_ITEM_COUNT as f32 - LIST_BOX_HEIGHT).max(0.0);
        scroll_controller.update_dimensions(LIST_BOX_HEIGHT, 0.0, max_scroll_extent);
        // Seed from the pub cell (clamped through the extents just set) so a
        // `DemoRoot` constructed with a nonzero `scroll_offset` doesn't snap
        // back to 0 on the first drag — `jump_to` is the same clamp path the
        // drag callback below uses.
        scroll_controller.jump_to(self.scroll_offset.get());

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
    }

    fn build(&self, _view: &DemoRoot, _ctx: &dyn BuildContext) -> impl IntoView {
        let rebuild = self
            .rebuild
            .clone()
            .expect("BUG: init_state runs before build (ViewState lifecycle order)");

        let count = self.count.get();
        let expanded = self.expanded.get();
        let scroll_offset = self.scroll_offset.get();

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
                // increases it. `jump_to` clamps to the extents fed in
                // `create_state` (see the module doc).
                let proposed = scroll_controller_for_drag.pixels() - details.delta.dy.get();
                scroll_controller_for_drag.jump_to(proposed);
                scroll_offset_for_drag.set(scroll_controller_for_drag.pixels());
                rebuild_for_drag.schedule();
            })
            .child(
                SizedBox::height(LIST_BOX_HEIGHT).child(
                    ListView::new(
                        LIST_ITEM_EXTENT,
                        (0..LIST_ITEM_COUNT)
                            .map(|index| {
                                Container::new()
                                    .padding(EdgeInsets::all(px(4.0)))
                                    .child(Text::new(format!("Item {index}")))
                                    .boxed()
                            })
                            .collect::<Vec<_>>(),
                    )
                    .offset(scroll_offset),
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
